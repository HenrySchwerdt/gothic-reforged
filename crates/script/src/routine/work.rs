//! B04 + B05: the routine task bodies (`ZS_*` work-station, social, guard, sleep and ambient
//! states) as data.
//!
//! The DLL implements each state as a resumable function with one "already done" slot per
//! blocking step. Here every state is a [`Def`]: either a flat table of [`Step`]s executed by one
//! interpreter ([`run_state`]) or, for the few states with real branching, a custom function.
//!
//! **Interpreter contract.** `run_state` is called whenever the AI queue of the NPC is empty. It
//! executes steps until one queues an AI command (returns [`Flow::Yield`]) or the state changes
//! (returns [`Flow::Done`]). The per-state step counter lives in the host
//! (`state_progress`) and is reset whenever the state changes. A state that runs off its end
//! restarts from step 0 on the next call (the engine re-enters the task), which is how the
//! dispatchers (`ZS_Smith`, ...) pick their next object.
//!
//! Randomised durations follow the DLL: *long* work is `(rand(3) + 2) * 60` s, *medium*
//! `rand(31) + 30` s, *short* `rand(11) + 10` s; with the debug variant every action lasts 10 s.

use crate::enums::{Action, AiMode, Slot, UseType};
use crate::helpers::{ambient_action_secs, long_work_secs, medium_work_secs};
use crate::host::{AiCommand, EntityId, Pos, ScriptHost};
use crate::predicates::{is_ambient_creature, is_gate_guard};
use crate::routine::select::{
    ambient_action, continue_routine, have_a_break, set_current_destination_point,
};
use crate::routine::state::{Callback, State};
use crate::routine::time::{RoutineTime, routine_time};

/// Result of one interpreter call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// An AI command was queued; call again once the host has executed it.
    Yield,
    /// The state changed or ended; call again to run the (new) state.
    Done,
}

/// An action duration recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dur {
    /// Exactly this many seconds.
    Fixed(i32),
    /// `(rand(3) + 2) * 60` (S:76b0).
    Long,
    /// `rand(31) + 30`.
    Medium,
    /// `rand(11) + 10`.
    Short,
    /// Ambient creatures `rand(11) + 10`, everyone else `rand(31) + 30` (S:76e0).
    Ambient,
}

fn secs(host: &mut dyn ScriptHost, npc: EntityId, d: Dur) -> i32 {
    if host.variants().fast_routines && !matches!(d, Dur::Fixed(_)) {
        return 10;
    }
    match d {
        Dur::Fixed(n) => n,
        Dur::Long => long_work_secs(host),
        Dur::Medium => medium_work_secs(host),
        Dur::Short => host.random(11) + 10,
        Dur::Ambient => {
            let ambient = is_ambient_creature(host, npc);
            ambient_action_secs(host, ambient)
        }
    }
}

/// What to do when no object can be found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// `SetTask(ZS_Idiot)`: stand around.
    Idiot,
    /// `ContinueRoutine`.
    Continue,
    /// `SetState(...)`.
    State(State),
}

/// How a dispatcher chooses the object to walk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The routine's current destination point.
    Destination,
    /// A random one among the free objects of these use types found for the NPC (the scanner
    /// result table, S:7c90/S:8190).
    Random(&'static [UseType]),
    /// Visit the objects in order, one per pass (`PSRoutine.TaskPosition`); `true` entries are
    /// required (a missing one aborts the task to the fallback), optional ones are skipped.
    Cycle(&'static [(UseType, bool)]),
}

/// What happens after the NPC arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrive {
    /// Only switch the callback to `OnWork`; the following steps do the work.
    Work,
    /// Switch by the use type of the destination point.
    ByUse(&'static [(UseType, State)]),
    /// Random choice (`rand(n)`).
    Random(&'static [State]),
}

/// How a state ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// `ContinueRoutine`.
    Continue,
    /// `HaveABreak` (S:8c390).
    Break,
    /// Fall off the end; the engine re-enters the state from step 0.
    Restart,
    /// `SetState`.
    State(State),
    /// `SetTask`.
    Task(State),
}

/// One step of a state body.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// `_AI_StandUp`.
    StandUp,
    /// `_AI_HoldInventoryItems(-1, -1)`: put everything away.
    HoldNone,
    /// `_AI_HoldItems(template, "")`.
    Hold(&'static str),
    /// `_AI_HoldItems(a, b)`.
    HoldPair(&'static str, &'static str),
    /// Create the templates if missing, then hold them (`AssureItems` + `HoldStacks`).
    AssureHold(&'static [&'static str]),
    /// `_AI_AniStateAction(action)`: enter a pose.
    Pose(Action),
    /// `_AI_Action(action, duration)`.
    Act(Action, Dur),
    /// `_AI_Action` where the engine picks the animation from the point's use type.
    ActHere(Dur),
    /// `rand(n)` chooses which of these actions to play (`ZS_Boss_Stand`, `ZS_Trader_Stand`).
    ActOneOf(&'static [Action], Dur),
    /// `_AI_Enjoy(a, b, rand(n))`.
    Enjoy(&'static str, &'static str, i32),
    /// Choose an object, switch to `OnGoto` and walk there.
    GotoPick(Pick, Fallback),
    /// If this state is entered with the NPC already on a destination point whose use type is in
    /// the table, jump to the sub-state; otherwise fall through (`ZS_Break`).
    ByUseIfThere(&'static [(UseType, State)]),
    /// Arrival handling.
    OnArrival(Arrive),
    /// Walk to a different interact point of the destination's anchor (field rows, planks).
    GotoOtherPoint,
    /// `_AI_SaySVM(family, index)`.
    Say(&'static str, i32),
    /// `sAIWaitInstr`.
    Wait(Dur),
    /// Stop the state: transition.
    Then(End),
    /// Custom body.
    Custom(fn(&mut dyn ScriptHost, EntityId) -> Out),
}

/// A state definition.
#[derive(Debug, Clone, Copy)]
pub enum Def {
    /// Flat step table run by [`run_state`].
    Steps(&'static [Step]),
    /// Hand-written body for the states with real branching.
    Custom(fn(&mut dyn ScriptHost, EntityId) -> Flow),
}

use Step::*;

// ---- B04: crafting, farming, trading work stations --------------------------------------------

/// Port of `ZS_Smith` (S:a0a40): cycle forge, anvil, forge, anvil, water barrel by task position; no anvil falls back to idling.
const SMITH: &[Step] = &[
    GotoPick(
        Pick::Cycle(&[
            (UseType::Forge, false),
            (UseType::Anvil, true),
            (UseType::Forge, false),
            (UseType::Anvil, true),
            (UseType::WaterBarrel, false),
        ]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Anvil, State::SmithAnvil),
        (UseType::Forge, State::SmithForge),
        (UseType::WaterBarrel, State::SmithCool),
    ])),
];
/// Port of `ZS_Smith_Anvil` (S:a1250): hammer the blade for a long time, then back to the smith dispatcher.
const SMITH_ANVIL: &[Step] = &[
    StandUp,
    AssureHold(&["It_Hammer_Smith", "It_Sword"]),
    Act(Action::Anvil, Dur::Long),
    Then(End::State(State::Smith)),
];
/// Port of `ZS_Smith_Forge` (S:a0ec0): heat the blade at the forge for a medium time, then back to the dispatcher.
const SMITH_FORGE: &[Step] = &[
    StandUp,
    AssureHold(&["It_Hammer_Smith", "It_Sword"]),
    Act(Action::Forge, Dur::Medium),
    Then(End::State(State::Smith)),
];
/// Port of `ZS_Smith_Cool` (S:a15f0): cool the weapon (10 s), then continue the routine.
const SMITH_COOL: &[Step] = &[
    StandUp,
    HoldPair("It_Hammer_Smith", "It_Sword"),
    Act(Action::CoolWeapon, Dur::Fixed(10)),
    Then(End::Continue),
];
/// Port of `ZS_Smith_GrindStone` (S:a1a60): sharpen a sword on the grindstone for a long time, then continue.
const SMITH_GRINDSTONE: &[Step] = &[
    StandUp,
    Hold("It_Sword"),
    Act(Action::GrindStone, Dur::Long),
    Then(End::Continue),
];

/// Port of `ZS_Alchemy` (S:86ff0): walk to a random alchemy table, bookstand or cauldron and start the matching sub-state.
const ALCHEMY: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::Alchemy, UseType::Bookstand, UseType::Cauldron]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Cauldron, State::AlchemyCauldron),
        (UseType::Alchemy, State::AlchemyAlchemy),
        (UseType::Bookstand, State::AlchemyBookstand),
    ])),
];
/// Port of `ZS_Alchemy_Alchemy` (S:87410): brew potions at the alchemy table for a medium time.
const ALCHEMY_ALCHEMY: &[Step] = &[
    StandUp,
    Hold("It_Potion"),
    Act(Action::Alchemy, Dur::Medium),
    HoldNone,
    Then(End::Continue),
];
/// Port of `ZS_Alchemy_Bookstand` (S:879e0): read at the bookstand for a medium time.
const ALCHEMY_BOOKSTAND: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Bookstand, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Alchemy_Cauldron` (S:87e50): stir the cauldron with the scoop for a medium time.
const ALCHEMY_CAULDRON: &[Step] = &[
    StandUp,
    Hold("It_Scoop"),
    Act(Action::Cauldron, Dur::Medium),
    HoldNone,
    Then(End::Continue),
];

/// Port of `ZS_Barbecue` (S:8a210): walk to the barbecue, put items away, work a medium time, then maybe take a break.
const BARBECUE: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    HoldNone,
    StandUp,
    ActHere(Dur::Medium),
    Then(End::Break),
];
/// Port of `ZS_Bard` (S:8a830): walk to the stage, play the lute for a long time, then maybe take a break.
const BARD: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    StandUp,
    Hold("It_Lute"),
    ActHere(Dur::Long),
    Then(End::Break),
];
/// Port of `ZS_Cook` (S:8eec0): walk to the cauldron, stir with the scoop for a long time, then maybe take a break.
const COOK: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    StandUp,
    Hold("It_Scoop"),
    ActHere(Dur::Long),
    Then(End::Break),
];
/// Port of `ZS_Grind` (S:92fc0): walk to the grindstone, sharpen a sword for a long time, then maybe take a break.
const GRIND: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    StandUp,
    Hold("It_Sword"),
    ActHere(Dur::Long),
    Then(End::Break),
];

/// Port of `ZS_Digger` (S:8f4d0): walk to the mine point and dispatch on ore, ground, digging or water barrel.
const DIGGER: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::ByUse(&[
        (UseType::PickOre, State::DiggerPickOre),
        (UseType::PickGround, State::DiggerPickGround),
        (UseType::DigGround, State::DiggerDigGround),
        (UseType::WaterBarrel, State::DiggerWaterBarrel),
    ])),
];
/// Port of `ZS_Digger_PickOre` (S:8f820): pick ore with the pickaxe for a long time, then maybe take a break.
const DIGGER_PICK_ORE: &[Step] = &[
    StandUp,
    Hold("It_Pickaxe"),
    Act(Action::PickOre, Dur::Long),
    Then(End::Break),
];
/// Port of `ZS_Digger_PickGround` (S:8fcc0): pick the ground with the pickaxe for a long time, then maybe take a break.
const DIGGER_PICK_GROUND: &[Step] = &[
    StandUp,
    Hold("It_Pickaxe"),
    Act(Action::PickGround, Dur::Long),
    Then(End::Break),
];
/// Port of `ZS_Digger_DigGround` (S:90160): dig with the shovel for a long time, then maybe take a break.
const DIGGER_DIG_GROUND: &[Step] = &[
    StandUp,
    Hold("It_Shovel"),
    Act(Action::DigGround, Dur::Long),
    Then(End::Break),
];
/// Port of `ZS_Digger_WaterBarrel` (S:90600): drink at the water barrel for a medium time.
const DIGGER_WATER_BARREL: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::DrinkWaterBarrel, Dur::Medium),
    Then(End::Continue),
];

/// Port of `ZS_Farmer` (S:90a70): walk to the field and randomly rake it or do field work.
const FARMER: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Random(&[
        State::FarmerRakeField,
        State::FarmerFieldWork,
    ])),
];
/// Port of `ZS_Farmer_FieldWork` (S:91720): field work, walk to another field point twice, medium time each.
const FARMER_FIELD_WORK: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::FieldWork, Dur::Medium),
    GotoOtherPoint,
    Act(Action::FieldWork, Dur::Medium),
    GotoOtherPoint,
    Act(Action::FieldWork, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Farmer_RakeField` (S:90d50): rake the field, walk to another field point twice, medium time each.
const FARMER_RAKE_FIELD: &[Step] = &[
    StandUp,
    Hold("It_Rake"),
    Act(Action::RakeField, Dur::Medium),
    GotoOtherPoint,
    Act(Action::RakeField, Dur::Medium),
    GotoOtherPoint,
    Act(Action::RakeField, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Lumberjack` (S:980e0): walk to the tree, assure and hold the lumberjack axe, chop for a long time, then maybe break.
const LUMBERJACK: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    StandUp,
    AssureHold(&["It_2H_Axe_Lumberjack"]),
    Act(Action::Lumberjack, Dur::Long),
    Then(End::Break),
];

/// Port of `ZS_Study` (S:a1ef0): walk to a random bookstand or bookshelf and start the matching sub-state.
const STUDY: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::Bookstand, UseType::Bookshelf]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Bookshelf, State::StudyBookshelf),
        (UseType::Bookstand, State::StudyBookstand),
    ])),
];
/// Port of `ZS_Study_Bookshelf` (S:a2750): browse the shelf (hands on hips) for a medium time.
const STUDY_BOOKSHELF: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::HandsOnHips, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Study_Bookstand` (S:a22e0): read at the bookstand for a long time.
const STUDY_BOOKSTAND: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Bookstand, Dur::Long),
    Then(End::Continue),
];

/// Port of `ZS_Trader` (S:a2bc0): walk to a random trader stand, bookstand, shelf, chest or floor spot and dispatch.
const TRADER: &[Step] = &[
    GotoPick(
        Pick::Random(&[
            UseType::Trader,
            UseType::Bookstand,
            UseType::Bookshelf,
            UseType::Chest,
            UseType::CleanFloor,
        ]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Bookshelf, State::TraderBookshelf),
        (UseType::Bookstand, State::TraderBookstand),
        (UseType::CleanFloor, State::TraderSweepFloor),
        (UseType::Trader, State::TraderStand),
        (UseType::Chest, State::TraderChest),
    ])),
];
/// Port of `ZS_Trader_Bookshelf` (S:a3a40): browse the shelf for a medium time.
const TRADER_BOOKSHELF: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::HandsOnHips, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Trader_Bookstand` (S:a35d0): read at the bookstand for a medium time.
const TRADER_BOOKSTAND: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Bookstand, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Trader_Chest` (S:a3eb0): kneel, open the chest, change action, stand up.
const TRADER_CHEST: &[Step] = &[
    HoldNone,
    Pose(Action::Kneel),
    Act(Action::OpenChest, Dur::Short),
    Custom(change_action),
    StandUp,
    Then(End::Continue),
];
/// Port of `ZS_Trader_Stand` (S:a30b0): stand behind the counter (arms crossed or hands on hips) for a long time.
const TRADER_STAND: &[Step] = &[
    HoldNone,
    StandUp,
    ActOneOf(&[Action::ArmsCrossed, Action::HandsOnHips], Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_Trader_SweepFloor` (S:a45b0): sweep the floor with the broom for a medium time.
const TRADER_SWEEP_FLOOR: &[Step] = &[
    StandUp,
    Hold("It_Broom"),
    Act(Action::CleanFloor, Dur::Medium),
    Then(End::Continue),
];

/// Port of `ZS_Woodwork` (S:a4a40): walk to a random saw or repair spot and dispatch.
const WOODWORK: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::SawLog, UseType::Repair]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Repair, State::WoodworkRepair),
        (UseType::SawLog, State::WoodworkSawLog),
    ])),
];
/// Port of `ZS_Woodwork_Repair` (S:a52a0): hammer three times, walking to another point between rounds, then maybe break.
const WOODWORK_REPAIR: &[Step] = &[
    StandUp,
    Hold("It_Hammer"),
    Act(Action::Repair, Dur::Medium),
    GotoOtherPoint,
    Hold("It_Hammer"),
    Act(Action::Repair, Dur::Medium),
    GotoOtherPoint,
    Hold("It_Hammer"),
    Act(Action::Repair, Dur::Medium),
    Then(End::Break),
];
/// Port of `ZS_Woodwork_SawLog` (S:a4e00): saw a log for a long time, then maybe take a break.
const WOODWORK_SAW_LOG: &[Step] = &[
    StandUp,
    Hold("It_Saw"),
    Act(Action::SawLog, Dur::Long),
    Then(End::Break),
];

// ---- B05: idle, social, guard, sleep, ceremonial --------------------------------------------------

/// Port of `ZS_Idiot` (S:966a0): fallback task: put everything away, stand and eat a carrot for rand(11) s.
const IDIOT: &[Step] = &[
    HoldNone,
    StandUp,
    Enjoy("It_Carrot", "", 11),
    Then(End::Continue),
];
/// Port of `ZS_Deadlock` (S:96b10): idle loop that restarts itself (never assigned by SelectRoutineTask).
const DEADLOCK: &[Step] = &[
    HoldNone,
    StandUp,
    Enjoy("It_Carrot", "", 11),
    Then(End::Restart),
];
/// Port of `ZS_StandUp` (S:a64c0): stand up, then continue the routine.
const STAND_UP: &[Step] = &[StandUp, Then(End::Continue)];

/// Port of `ZS_Boss` (S:8ae40): walk to a throne, boss stand or pace point and dispatch.
const BOSS: &[Step] = &[
    HoldNone,
    GotoPick(
        Pick::Random(&[UseType::Throne, UseType::Boss, UseType::Pace]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Boss, State::BossStand),
        (UseType::Throne, State::BossThrone),
        (UseType::Pace, State::BossPace),
    ])),
];
/// Port of `ZS_Boss_Stand` (S:8b350): stand with arms crossed or hands on hips for a long time.
const BOSS_STAND: &[Step] = &[
    HoldNone,
    StandUp,
    ActOneOf(&[Action::ArmsCrossed, Action::HandsOnHips], Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_Boss_Throne` (S:8b870): sit on the throne and enjoy for rand(3) s.
const BOSS_THRONE: &[Step] = &[
    HoldNone,
    Pose(Action::SitThrone),
    Enjoy("", "", 3),
    Then(End::Continue),
];
/// Port of `ZS_Boss_Pace` (S:8bd20): think for 5 s, walk to another point and pace until a long period passes.
const BOSS_PACE: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Think, Dur::Fixed(5)),
    GotoOtherPoint,
    Custom(pace_until_done),
];

/// Port of `ZS_Break` (S:8c4c0): take a break at the current seat/barrel or walk to a random break object.
const BREAK: &[Step] = &[
    ByUseIfThere(&[
        (UseType::Stool, State::BreakStool),
        (UseType::Bench, State::BreakBench),
        (UseType::WaterBarrel, State::BreakWaterBarrel),
        (UseType::SitGround, State::BreakSitGround),
        (UseType::Stand, State::BreakStand),
        (UseType::Pee, State::BreakPee),
        (UseType::Chest, State::BreakChest),
    ]),
    GotoPick(
        Pick::Random(&[
            UseType::Stool,
            UseType::Bench,
            UseType::SitGround,
            UseType::Stand,
            UseType::Pee,
            UseType::Chest,
            UseType::WaterBarrel,
        ]),
        Fallback::Continue,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Stool, State::BreakStool),
        (UseType::Bench, State::BreakBench),
        (UseType::WaterBarrel, State::BreakWaterBarrel),
        (UseType::SitGround, State::BreakSitGround),
        (UseType::Stand, State::BreakStand),
        (UseType::Pee, State::BreakPee),
        (UseType::Chest, State::BreakChest),
    ])),
];
/// Port of `ZS_Break_Bench` (S:8cdd0): sit on a bench and smoke a joint for rand(11) s.
const BREAK_BENCH: &[Step] = &[
    Pose(Action::SitBench),
    Enjoy("It_Joint", "", 11),
    Then(End::Continue),
];
/// Port of `ZS_Break_Stool` (S:8ca70): sit on a stool and smoke a joint for rand(11) s.
const BREAK_STOOL: &[Step] = &[
    Pose(Action::SitStool),
    Enjoy("It_Joint", "", 11),
    Then(End::Continue),
];
/// Port of `ZS_Break_SitGround` (S:8d140): sit on the ground and smoke a joint for rand(11) s.
const BREAK_SIT_GROUND: &[Step] = &[
    Pose(Action::SitGround),
    Enjoy("It_Joint", "", 11),
    Then(End::Continue),
];
/// Port of `ZS_Break_Stand` (S:8d4b0): stand and smoke a joint for rand(11) s.
const BREAK_STAND: &[Step] = &[
    HoldNone,
    StandUp,
    Enjoy("It_Joint", "", 11),
    Then(End::Continue),
];
/// Port of `ZS_Break_Pee` (S:8d920): relieve oneself (10 s).
const BREAK_PEE: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Pee, Dur::Fixed(10)),
    Then(End::Continue),
];
/// Port of `ZS_Break_Chest` (S:8dd50): kneel and rummage through a chest, stand up.
const BREAK_CHEST: &[Step] = &[
    HoldNone,
    Pose(Action::Kneel),
    Act(Action::OpenChest, Dur::Short),
    Custom(change_action),
    StandUp,
    Then(End::Continue),
];
/// Port of `ZS_Break_WaterBarrel` (S:8e450): drink from the water barrel for a short time.
const BREAK_WATER_BARREL: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::DrinkWaterBarrel, Dur::Short),
    Then(End::Continue),
];

/// Port of `ZS_HangAround_Stool` (S:946e0): sit on a stool and enjoy for rand(3) s.
const HANG_AROUND_STOOL: &[Step] = &[
    HoldNone,
    Pose(Action::SitStool),
    Enjoy("", "", 3),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_Bench` (S:94b70): sit on a bench and enjoy for rand(3) s.
const HANG_AROUND_BENCH: &[Step] = &[
    HoldNone,
    Pose(Action::SitBench),
    Enjoy("", "", 3),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_SitGround` (S:95000): sit on the ground and enjoy for rand(3) s.
const HANG_AROUND_SIT_GROUND: &[Step] = &[
    HoldNone,
    Pose(Action::SitGround),
    Enjoy("", "", 3),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_Stand` (S:95df0): stand and enjoy for rand(31) s.
const HANG_AROUND_STAND: &[Step] = &[HoldNone, StandUp, Enjoy("", "", 31), Then(End::Continue)];
/// Port of `ZS_HangAround_Waterpipe` (S:95970): sit and smoke the waterpipe for a long time.
const HANG_AROUND_WATERPIPE: &[Step] = &[
    HoldNone,
    Pose(Action::SitGround),
    Act(Action::Waterpipe, Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_FryMeat` (S:95490): sit by the fire and fry meat on a stick for a long time.
const HANG_AROUND_FRY_MEAT: &[Step] = &[
    HoldNone,
    Pose(Action::SitGround),
    AssureHold(&["It_Stick", "It_Meat"]),
    Act(Action::Fry, Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_Pee` (S:96270): relieve oneself (10 s).
const HANG_AROUND_PEE: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Pee, Dur::Fixed(10)),
    Then(End::Continue),
];
/// Port of `ZS_HangAround_DoNothing` (S:96f60): put everything away, stand up and wait.
const HANG_AROUND_DO_NOTHING: &[Step] = &[
    Custom(hold_stacks_none),
    StandUp,
    Wait(Dur::Fixed(1)),
    Then(End::Continue),
];

/// Port of `ZS_Goblin` (S:920d0): goblin camp life: campfire, sit or chat (never assigned by the routine selector).
const GOBLIN: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::SitCampfire, UseType::Smalltalk, UseType::SitGround]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::SitCampfire, State::GoblinFryMeat),
        (UseType::SitGround, State::GoblinSitGround),
        (UseType::Smalltalk, State::GoblinSmalltalk),
    ])),
];
/// Port of `ZS_Goblin_FryMeat` (S:924c0): sit and fry meat, then eat the fried meatbug.
const GOBLIN_FRY_MEAT: &[Step] = &[
    Pose(Action::SitGround),
    HoldPair("It_Stick", "It_Meat"),
    Act(Action::Fry, Dur::Medium),
    Enjoy("It_Meatbug_Fried", "", 31),
    Then(End::Continue),
];
/// Port of `ZS_Goblin_SitGround` (S:92a90): sit on the ground idling for a medium time.
const GOBLIN_SIT_GROUND: &[Step] = &[
    HoldNone,
    Pose(Action::SitGround),
    Act(Action::Ambient, Dur::Medium),
    Then(End::Continue),
];

/// Port of `ZS_Listen` (S:979b0): walk to the listening spot, stand and listen.
const LISTEN: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    StandUp,
    HoldNone,
    Then(End::State(State::ListenLoop)),
];
/// Port of `ZS_Listen_Loop` (S:97ec0): half of the time listen for a medium time.
const LISTEN_LOOP: &[Step] = &[Custom(listen_loop)];

/// Port of `ZS_Guard` (S:935d0): stow back weapons, walk to the post, then start the guard loop.
const GUARD: &[Step] = &[
    Custom(guard_unequip_back),
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    Then(End::State(State::GuardLoop)),
];
/// Port of `ZS_Guard_Loop` (S:938b0): register the gate-guard callback, stow weapons and stand guard for a long time.
const GUARD_LOOP: &[Step] = &[
    Custom(guard_gate_callback),
    StandUp,
    Custom(guard_unequip_back),
    Act(Action::Ambient, Dur::Long),
    Then(End::Continue),
];

/// Port of `ZS_Pray` (S:9abb0): walk to the shrine and start praying.
const PRAY: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::ByUse(&[(UseType::Shrine, State::PrayShrine)])),
];
/// Port of `ZS_Pray_Shrine` (S:9aea0): kneel at the shrine for a medium time.
const PRAY_SHRINE: &[Step] = &[
    HoldNone,
    Pose(Action::Kneel),
    Wait(Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_Preach` (S:9b220): walk to the pulpit and choose voice-line or automatic preaching.
const PREACH: &[Step] = &[
    GotoPick(Pick::Destination, Fallback::Idiot),
    OnArrival(Arrive::Work),
    Custom(preach_branch),
];
/// Port of `ZS_Preach_AutoPreach` (S:9b510): preach with hands on hips for a long time.
const PREACH_AUTO: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::HandsOnHips, Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_Preach_SVM` (S:9b980): say voice lines Speech_00..09, then maybe take a break.
const PREACH_SVM: &[Step] = &[
    HoldNone,
    StandUp,
    Say("Speech", 0),
    Say("Speech", 1),
    Say("Speech", 2),
    Say("Speech", 3),
    Say("Speech", 4),
    Say("Speech", 5),
    Say("Speech", 6),
    Say("Speech", 7),
    Say("Speech", 8),
    Say("Speech", 9),
    Then(End::Break),
];

/// Port of `ZS_OrcParty` (S:992f0): walk to a random drum, dance, boulder or spectator spot and dispatch.
const ORC_PARTY: &[Step] = &[
    GotoPick(
        Pick::Random(&[
            UseType::OrcDrum,
            UseType::OrcDance,
            UseType::OrcBoulder,
            UseType::Spectator,
        ]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::Spectator, State::OrcPartySpectator),
        (UseType::OrcDance, State::OrcPartyOrcDance),
        (UseType::OrcDrum, State::OrcPartyOrcDrum),
        (UseType::OrcBoulder, State::OrcPartyOrcBoulder),
    ])),
];
/// Port of `ZS_OrcParty_OrcBoulder` (S:9a020): tilt, hold and lift the orc boulder.
const ORC_BOULDER: &[Step] = &[
    HoldNone,
    Pose(Action::TiltOrcBoulder),
    Pose(Action::HoldOrcBoulder),
    Pose(Action::LiftOrcBoulder),
    StandUp,
    Then(End::Continue),
];
/// Port of `ZS_OrcParty_OrcDance` (S:99bb0): dance for a long time.
const ORC_DANCE: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::Dance, Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_OrcParty_OrcDrum` (S:99720): drum with two drumsticks for a long time.
const ORC_DRUM: &[Step] = &[
    StandUp,
    HoldPair("It_DrumStick", "It_DrumStick"),
    Act(Action::OrcDrum, Dur::Long),
    Then(End::Continue),
];
/// Port of `ZS_OrcParty_Spectator` (S:9a740): watch with arms crossed for a medium time.
const ORC_SPECTATOR: &[Step] = &[
    HoldNone,
    StandUp,
    Act(Action::ArmsCrossed, Dur::Medium),
    Then(End::Continue),
];

/// Port of `ZS_SlaveBabe` (S:9cea0): walk to a cleaning, fan or dance spot and dispatch.
const SLAVE_BABE: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::CleanFloor, UseType::FanBoss, UseType::Dance]),
        Fallback::Idiot,
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::CleanFloor, State::SlaveBabeSweepFloor),
        (UseType::Dance, State::SlaveBabeDance),
        (UseType::FanBoss, State::SlaveBabeFanBoss),
    ])),
];
/// Port of `ZS_SlaveBabe_CleanFloor` (S:9d2a0): kneel and scrub with the brush.
const SLAVE_BABE_CLEAN_FLOOR: &[Step] = &[
    Pose(Action::Kneel),
    Hold("It_Brush"),
    Act(Action::CleanFloor, Dur::Medium),
    Then(End::Continue),
];
/// Port of `ZS_SlaveBabe_Dance` (S:9e590): dance for a long time.
const SLAVE_BABE_DANCE: &[Step] = &[
    StandUp,
    HoldNone,
    Act(Action::Dance, Dur::Long),
    Then(End::Restart),
];
/// Port of `ZS_SlaveBabe_FanBoss` (S:9e100): fan the boss for a long time.
const SLAVE_BABE_FAN_BOSS: &[Step] = &[
    StandUp,
    Hold("It_Fan"),
    Act(Action::Fan, Dur::Long),
    Then(End::Restart),
];
/// Port of `ZS_SlaveBabe_SweepFloor` (S:9d730): sweep with the broom in three rounds, walking between them.
const SLAVE_BABE_SWEEP_FLOOR: &[Step] = &[
    StandUp,
    Hold("It_Broom"),
    Act(Action::CleanFloor, Dur::Medium),
    GotoOtherPoint,
    Act(Action::CleanFloor, Dur::Medium),
    GotoOtherPoint,
    Act(Action::CleanFloor, Dur::Medium),
    Then(End::Continue),
];

/// Port of `ZS_Sleep` (S:9ea00): put everything away, walk to the bed or sleeping spot and lie down.
const SLEEP: &[Step] = &[
    HoldNone,
    GotoPick(Pick::Destination, Fallback::Idiot),
    Custom(arrive_sleep),
];
/// Port of `ZS_Sleep_Bed` (S:9eec0): lie in the bed, then poll in the sleep loop.
const SLEEP_BED: &[Step] = &[
    HoldNone,
    Pose(Action::SleepBed),
    Then(End::State(State::SleepLoop)),
];
/// Port of `ZS_Sleep_Ground` (S:9f1e0): lie on the ground, then poll in the sleep loop.
const SLEEP_GROUND: &[Step] = &[
    HoldNone,
    Pose(Action::SleepGround),
    Then(End::State(State::SleepLoop)),
];
/// Port of `ZS_Sleep_Loop` (S:9f500): every 60 s of state time: wake with an Awake line once the routine time is not sleep.
const SLEEP_LOOP: &[Step] = &[Custom(sleep_loop)];

/// Port of `ZS_Smalltalk` (S:9f6c0): pair the two chatters into leader and partner.
const SMALLTALK: &[Step] = &[Custom(smalltalk_dispatch)];
/// Port of `ZS_Smalltalk_Leader` (S:9ff50): put everything away, stand up, enter the leader loop.
const SMALLTALK_LEADER: &[Step] = &[
    HoldNone,
    StandUp,
    Then(End::State(State::SmalltalkLeaderLoop)),
];
/// Port of `ZS_Smalltalk_Leader_Loop` (S:a02e0): wait, say a smalltalk line, wait, end after the poll interval.
const SMALLTALK_LEADER_LOOP: &[Step] = &[
    Wait(Dur::Fixed(5)),
    Custom(smalltalk_say),
    Wait(Dur::Fixed(5)),
    Custom(smalltalk_leader_end),
];
/// Port of `ZS_Smalltalk_Partner` (S:9fb20): put everything away, stand up, enter the partner loop.
const SMALLTALK_PARTNER: &[Step] = &[
    HoldNone,
    StandUp,
    Then(End::State(State::SmalltalkPartnerLoop)),
];
/// Port of `ZS_Goblin_Smalltalk` (S:92f00): wait out the poll interval, then continue.
const SMALLTALK_PARTNER_LOOP: &[Step] = &[Custom(smalltalk_partner_end)];

// ---- ambient / animal / monster ----------------------------------------------------------------------

/// Port of `ZS_Herd` (S:971d0): walk to a grazing or watering spot.
const HERD: &[Step] = &[
    GotoPick(
        Pick::Random(&[UseType::EatGround, UseType::DrinkWater]),
        Fallback::State(State::HerdEatGround),
    ),
    OnArrival(Arrive::ByUse(&[
        (UseType::EatGround, State::HerdEatGround),
        (UseType::DrinkWater, State::HerdDrinkWater),
    ])),
];
/// Port of `ZS_Herd_DrinkWater` (S:977a0): drink for a medium time.
const HERD_DRINK_WATER: &[Step] = &[Act(Action::DrinkWater, Dur::Medium), Then(End::Continue)];
/// Port of `ZS_Herd_EatGround` (S:97590): graze for a medium time.
const HERD_EAT_GROUND: &[Step] = &[Act(Action::EatGround, Dur::Medium), Then(End::Continue)];

/// Port of `ZS_Monster` (S:987c0): walk to the roaming destination, then roam.
const MONSTER: &[Step] = &[
    Custom(hold_fist_if_in_water),
    GotoPick(Pick::Destination, Fallback::State(State::MonsterRoam)),
    OnArrival(Arrive::Random(&[State::MonsterRoam])),
];
/// Port of `ZS_Monster_Roam` (S:98ed0): idle action chosen by GetAmbientAction for a medium time.
const MONSTER_ROAM: &[Step] = &[
    Custom(hold_fist_if_in_water),
    Custom(monster_roam_action),
    Then(End::Continue),
];

/// Port of `ZS_Ambient` (S:88610): stand up, remember the spawn point, then sleep or wander by routine time.
const AMBIENT: &[Step] = &[
    Custom(ambient_start),
    Custom(hold_fist_if_in_water),
    Custom(ambient_home),
    Custom(ambient_dispatch),
];
/// Port of `ZS_Ambient_Loop` (S:88c40): wander near the spawn point, idle with the ambient action, repeat.
const AMBIENT_LOOP: &[Step] = &[
    Custom(hold_fist_if_in_water),
    Custom(ambient_wander),
    Custom(hold_fist_if_in_water),
    Custom(ambient_idle_action),
    Then(End::State(State::Ambient)),
];
/// Port of `ZS_Ambient_Sleep` (S:89410): lie down on the ground and enter the sleep loop (not on water for Bloodflies).
const AMBIENT_SLEEP: &[Step] = &[
    Custom(ambient_sleep_start),
    Pose(Action::SleepGround),
    Then(End::State(State::AmbientSleepLoop)),
];
/// Port of `ZS_Ambient_Sleep_Loop` (S:89660): sleep animation, wake after the poll interval.
const AMBIENT_SLEEP_LOOP: &[Step] = &[
    Act(Action::Ambient, Dur::Medium),
    Custom(ambient_sleep_wake),
];
/// Port of `ZS_AnimalSleep` (S:898c0): walk to the sleeping point, lie down and enter raw AI mode 7.
const ANIMAL_SLEEP: &[Step] = &[
    Custom(animal_sleep_check),
    StandUp,
    HoldNone,
    GotoPick(Pick::Destination, Fallback::Continue),
    Custom(animal_arrived),
    Pose(Action::SleepGround),
    Custom(animal_enter_sleep),
];
/// Port of `ZS_AnimalSleep_SleepGround` (S:8a150): every 60 s of state time re-check the routine.
const ANIMAL_SLEEP_GROUND: &[Step] = &[Custom(animal_sleep_wake)];

/// The step table or custom body of a state; `None` for states not driven by this module
/// (combat, party following, player states, `ZS_Dead`, ...).
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let steps = |s: &'static [Step]| Some(Def::Steps(s));
    match state {
        S::Smith => steps(SMITH),
        S::SmithAnvil => steps(SMITH_ANVIL),
        S::SmithForge => steps(SMITH_FORGE),
        S::SmithCool => steps(SMITH_COOL),
        S::SmithGrindStone => steps(SMITH_GRINDSTONE),
        S::Alchemy => steps(ALCHEMY),
        S::AlchemyAlchemy => steps(ALCHEMY_ALCHEMY),
        S::AlchemyBookstand => steps(ALCHEMY_BOOKSTAND),
        S::AlchemyCauldron => steps(ALCHEMY_CAULDRON),
        S::Barbecue => steps(BARBECUE),
        S::Bard => steps(BARD),
        S::Cook => steps(COOK),
        S::Grind => steps(GRIND),
        S::CarryStones => Some(Def::Custom(carry_stones)),
        S::Digger => steps(DIGGER),
        S::DiggerPickOre => steps(DIGGER_PICK_ORE),
        S::DiggerPickGround => steps(DIGGER_PICK_GROUND),
        S::DiggerDigGround => steps(DIGGER_DIG_GROUND),
        S::DiggerWaterBarrel => steps(DIGGER_WATER_BARREL),
        S::Farmer => steps(FARMER),
        S::FarmerFieldWork => steps(FARMER_FIELD_WORK),
        S::FarmerRakeField => steps(FARMER_RAKE_FIELD),
        S::Lumberjack => steps(LUMBERJACK),
        S::Study => steps(STUDY),
        S::StudyBookshelf => steps(STUDY_BOOKSHELF),
        S::StudyBookstand => steps(STUDY_BOOKSTAND),
        S::Trader => steps(TRADER),
        S::TraderBookshelf => steps(TRADER_BOOKSHELF),
        S::TraderBookstand => steps(TRADER_BOOKSTAND),
        S::TraderChest => steps(TRADER_CHEST),
        S::TraderStand => steps(TRADER_STAND),
        S::TraderSweepFloor => steps(TRADER_SWEEP_FLOOR),
        S::Woodwork => steps(WOODWORK),
        S::WoodworkRepair => steps(WOODWORK_REPAIR),
        S::WoodworkSawLog => steps(WOODWORK_SAW_LOG),
        S::Idiot => steps(IDIOT),
        S::Deadlock => steps(DEADLOCK),
        S::StandUp => steps(STAND_UP),
        S::Boss => steps(BOSS),
        S::BossStand => steps(BOSS_STAND),
        S::BossThrone => steps(BOSS_THRONE),
        S::BossPace => steps(BOSS_PACE),
        S::Break => steps(BREAK),
        S::BreakBench => steps(BREAK_BENCH),
        S::BreakStool => steps(BREAK_STOOL),
        S::BreakSitGround => steps(BREAK_SIT_GROUND),
        S::BreakStand => steps(BREAK_STAND),
        S::BreakPee => steps(BREAK_PEE),
        S::BreakChest => steps(BREAK_CHEST),
        S::BreakWaterBarrel => steps(BREAK_WATER_BARREL),
        S::HangAround => Some(Def::Custom(hang_around)),
        S::HangAroundStool => steps(HANG_AROUND_STOOL),
        S::HangAroundBench => steps(HANG_AROUND_BENCH),
        S::HangAroundSitGround => steps(HANG_AROUND_SIT_GROUND),
        S::HangAroundStand => steps(HANG_AROUND_STAND),
        S::HangAroundWaterpipe => steps(HANG_AROUND_WATERPIPE),
        S::HangAroundFryMeat => steps(HANG_AROUND_FRY_MEAT),
        S::HangAroundPee => steps(HANG_AROUND_PEE),
        S::HangAroundDoNothing => steps(HANG_AROUND_DO_NOTHING),
        S::Goblin => steps(GOBLIN),
        S::GoblinFryMeat => steps(GOBLIN_FRY_MEAT),
        S::GoblinSitGround => steps(GOBLIN_SIT_GROUND),
        S::GoblinSmalltalk => steps(SMALLTALK_PARTNER_LOOP),
        S::Listen => steps(LISTEN),
        S::ListenLoop => steps(LISTEN_LOOP),
        S::Guard => steps(GUARD),
        S::GuardLoop => steps(GUARD_LOOP),
        S::Pray => steps(PRAY),
        S::PrayShrine => steps(PRAY_SHRINE),
        S::Preach => steps(PREACH),
        S::PreachAutoPreach => steps(PREACH_AUTO),
        S::PreachSvm => steps(PREACH_SVM),
        S::OrcParty => steps(ORC_PARTY),
        S::OrcPartyOrcBoulder => steps(ORC_BOULDER),
        S::OrcPartyOrcDance => steps(ORC_DANCE),
        S::OrcPartyOrcDrum => steps(ORC_DRUM),
        S::OrcPartySpectator => steps(ORC_SPECTATOR),
        S::SlaveBabe => steps(SLAVE_BABE),
        S::SlaveBabeCleanFloor => steps(SLAVE_BABE_CLEAN_FLOOR),
        S::SlaveBabeDance => steps(SLAVE_BABE_DANCE),
        S::SlaveBabeFanBoss => steps(SLAVE_BABE_FAN_BOSS),
        S::SlaveBabeSweepFloor => steps(SLAVE_BABE_SWEEP_FLOOR),
        S::Sleep => steps(SLEEP),
        S::SleepBed => steps(SLEEP_BED),
        S::SleepGround => steps(SLEEP_GROUND),
        S::SleepLoop => steps(SLEEP_LOOP),
        S::Smalltalk => steps(SMALLTALK),
        S::SmalltalkLeader => steps(SMALLTALK_LEADER),
        S::SmalltalkLeaderLoop => steps(SMALLTALK_LEADER_LOOP),
        S::SmalltalkPartner => steps(SMALLTALK_PARTNER),
        S::SmalltalkPartnerLoop => steps(SMALLTALK_PARTNER_LOOP),
        S::RunToAttackPoint => Some(Def::Custom(run_to_attack_point)),
        S::Herd => steps(HERD),
        S::HerdDrinkWater => steps(HERD_DRINK_WATER),
        S::HerdEatGround => steps(HERD_EAT_GROUND),
        S::Monster => steps(MONSTER),
        S::MonsterRoam => steps(MONSTER_ROAM),
        S::Ambient => steps(AMBIENT),
        S::AmbientLoop => steps(AMBIENT_LOOP),
        S::AmbientSleep => steps(AMBIENT_SLEEP),
        S::AmbientSleepLoop => steps(AMBIENT_SLEEP_LOOP),
        S::AnimalSleep => steps(ANIMAL_SLEEP),
        S::AnimalSleepSleepGround => steps(ANIMAL_SLEEP_GROUND),
        _ => None,
    }
}

// ---- interpreter -----------------------------------------------------------------------------------

/// Outcome of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Run the next step in this call.
    Next,
    /// A command was queued; return to the host.
    Yield,
    /// The state changed; return to the host.
    Leave,
    /// The state gave up without a transition; restart it next call.
    Abort,
}

/// Run the NPC's current state (port of the `ZS_*` function bodies; see the module docs).
pub fn run_state(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    let Some(state) = host.current_state(npc) else {
        return Flow::Done;
    };
    match definition(state) {
        Some(Def::Steps(steps)) => run_steps(host, npc, steps),
        Some(Def::Custom(f)) => f(host, npc),
        None => Flow::Done,
    }
}

fn run_steps(host: &mut dyn ScriptHost, npc: EntityId, steps: &'static [Step]) -> Flow {
    loop {
        let p = host.state_progress(npc) as usize;
        if p >= steps.len() {
            host.set_state_progress(npc, 0);
            return Flow::Done;
        }
        host.set_state_progress(npc, p as u32 + 1);
        match exec(host, npc, steps[p]) {
            Out::Next => {}
            Out::Yield => return Flow::Yield,
            Out::Leave => return Flow::Done,
            Out::Abort => {
                host.set_state_progress(npc, 0);
                return Flow::Done;
            }
        }
    }
}

fn exec(host: &mut dyn ScriptHost, npc: EntityId, step: Step) -> Out {
    match step {
        StandUp => queue(host, npc, AiCommand::StandUp),
        HoldNone => queue(
            host,
            npc,
            AiCommand::HoldInventoryStacks {
                right: -1,
                left: -1,
            },
        ),
        Hold(a) => queue(host, npc, AiCommand::HoldItems { right: a, left: "" }),
        HoldPair(a, b) => queue(host, npc, AiCommand::HoldItems { right: a, left: b }),
        AssureHold(list) => {
            let stacks: Vec<i32> = list
                .iter()
                .map(|t| host.assure_items(npc, t, 0, 1))
                .collect();
            let right = stacks.first().copied().unwrap_or(-1);
            let left = stacks.get(1).copied().unwrap_or(-1);
            queue(host, npc, AiCommand::HoldInventoryStacks { right, left })
        }
        Pose(a) => queue(host, npc, AiCommand::AniStateAction(a)),
        Act(a, d) => {
            let s = secs(host, npc, d);
            queue(host, npc, AiCommand::Action { action: a, secs: s })
        }
        ActHere(d) => {
            let s = secs(host, npc, d);
            queue(host, npc, AiCommand::ActionAtPoint { secs: s })
        }
        ActOneOf(list, d) => {
            let i = host.random(list.len() as i32) as usize;
            let s = secs(host, npc, d);
            queue(
                host,
                npc,
                AiCommand::Action {
                    action: list[i.min(list.len() - 1)],
                    secs: s,
                },
            )
        }
        Enjoy(a, b, n) => {
            let s = if host.variants().fast_routines {
                10
            } else {
                host.random(n)
            };
            queue(
                host,
                npc,
                AiCommand::Enjoy {
                    item_a: a,
                    item_b: b,
                    secs: s,
                },
            )
        }
        GotoPick(pick, fallback) => goto_pick(host, npc, pick, fallback),
        ByUseIfThere(table) => {
            let point = host.destination_point(npc);
            if point.is_some() && host.is_on_destination(npc) {
                let use_type = host.point_use_type(point);
                if let Some(&(_, next)) = table.iter().find(|(u, _)| *u == use_type) {
                    host.set_task_callback(npc, Callback::OnWork);
                    host.set_state(npc, next);
                    return Out::Leave;
                }
            }
            Out::Next
        }
        OnArrival(arrive) => on_arrival(host, npc, arrive),
        GotoOtherPoint => goto_other_point(host, npc),
        Say(family, index) => queue(host, npc, AiCommand::SaySvm { family, index }),
        Wait(d) => {
            let s = secs(host, npc, d);
            queue(host, npc, AiCommand::Wait { secs: s })
        }
        Then(end) => finish(host, npc, end),
        Custom(f) => f(host, npc),
    }
}

fn queue(host: &mut dyn ScriptHost, npc: EntityId, cmd: AiCommand) -> Out {
    host.queue_ai(npc, cmd);
    Out::Yield
}

fn finish(host: &mut dyn ScriptHost, npc: EntityId, end: End) -> Out {
    match end {
        End::Continue => {
            continue_routine(host, npc);
            Out::Leave
        }
        End::Break => {
            have_a_break(host, npc);
            Out::Leave
        }
        End::Restart => Out::Abort,
        End::State(s) => {
            host.set_state(npc, s);
            Out::Leave
        }
        End::Task(s) => {
            host.set_task(npc, s);
            Out::Leave
        }
    }
}

fn fall_back(host: &mut dyn ScriptHost, npc: EntityId, fb: Fallback) -> Out {
    match fb {
        Fallback::Idiot => {
            host.set_task(npc, State::Idiot);
            Out::Leave
        }
        Fallback::Continue => {
            continue_routine(host, npc);
            Out::Leave
        }
        Fallback::State(s) => {
            host.set_state(npc, s);
            Out::Leave
        }
    }
}

fn goto_pick(host: &mut dyn ScriptHost, npc: EntityId, pick: Pick, fb: Fallback) -> Out {
    let target = match pick {
        Pick::Destination => host.destination_point(npc),
        Pick::Random(list) => {
            let mut found: Vec<EntityId> = Vec::new();
            for &u in list {
                let o = host.find_use_object(npc, u);
                if o.is_some() && !found.contains(&o) {
                    found.push(o);
                }
            }
            if found.is_empty() {
                EntityId::NONE
            } else {
                let i = host.random(found.len() as i32) as usize;
                found[i.min(found.len() - 1)]
            }
        }
        Pick::Cycle(seq) => {
            let pos = host.task_position(npc) + 1;
            host.set_task_position(npc, pos);
            let Some(&(use_type, required)) = seq.get(pos as usize - 1) else {
                // Cycle exhausted: the DLL's last state (cooling) already ended the shift.
                continue_routine(host, npc);
                return Out::Leave;
            };
            let o = host.find_use_object(npc, use_type);
            if o.is_none() {
                return if required {
                    fall_back(host, npc, fb)
                } else {
                    Out::Abort
                };
            }
            o
        }
    };
    if target.is_none() {
        return fall_back(host, npc, fb);
    }
    host.set_task_callback(npc, Callback::OnGoto);
    if host.point_use_type(target) != UseType::None {
        set_current_destination_point(host, npc, target);
    }
    queue(host, npc, AiCommand::GotoExt { target })
}

fn on_arrival(host: &mut dyn ScriptHost, npc: EntityId, arrive: Arrive) -> Out {
    if !host.is_on_destination(npc) {
        return Out::Abort;
    }
    host.set_task_callback(npc, Callback::OnWork);
    match arrive {
        Arrive::Work => Out::Next,
        Arrive::ByUse(table) => {
            let use_type = host.point_use_type(host.destination_point(npc));
            match table.iter().find(|(u, _)| *u == use_type) {
                Some(&(_, next)) => {
                    host.set_state(npc, next);
                    Out::Leave
                }
                None => Out::Abort,
            }
        }
        Arrive::Random(list) => {
            let i = host.random(list.len() as i32) as usize;
            host.set_state(npc, list[i.min(list.len() - 1)]);
            Out::Leave
        }
    }
}

fn goto_other_point(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let here = host.destination_point(npc);
    let anchor = if here.is_some() {
        host.point_anchor(here)
    } else {
        EntityId::NONE
    };
    if anchor.is_none() {
        return Out::Next;
    }
    let others: Vec<EntityId> = host
        .anchor_points(anchor)
        .into_iter()
        .filter(|&p| p != here)
        .collect();
    if others.is_empty() {
        return Out::Next;
    }
    let i = host.random(others.len() as i32) as usize;
    queue(
        host,
        npc,
        AiCommand::GotoExt {
            target: others[i.min(others.len() - 1)],
        },
    )
}

// ---- custom bodies --------------------------------------------------------------------------------------

/// Progress-aware helper for whole-state custom bodies: true exactly once for step `n`.
fn once(host: &mut dyn ScriptHost, npc: EntityId, n: u32) -> bool {
    if host.state_progress(npc) <= n {
        host.set_state_progress(npc, n + 1);
        true
    } else {
        false
    }
}

fn change_action(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::ChangeAction)
}

fn hold_stacks_none(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(
        host,
        npc,
        AiCommand::HoldInventoryStacks {
            right: -1,
            left: -1,
        },
    )
}

/// Tail of `ZS_Boss_Pace` (S:8bd20): after the walk, keep pacing until a long-work period of state time has
/// passed, then resume the routine.
fn pace_until_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let limit = long_work_secs(host) as f32;
    if host.state_time(npc) > limit {
        continue_routine(host, npc);
        Out::Leave
    } else {
        Out::Abort
    }
}

/// Port of `ZS_CarryStones` (S:8e8c0): stones are carried from a take point to a drop point; a drop-point trip
/// holds the stone, a pick-up trip walks empty-handed. Ends with `ContinueRoutine`.
fn carry_stones(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    if once(host, npc, 0) {
        host.queue_ai(npc, AiCommand::StandUp);
        return Flow::Yield;
    }
    if once(host, npc, 1) {
        let point = host.destination_point(npc);
        if point.is_none() {
            host.set_task(npc, State::Idiot);
            return Flow::Done;
        }
        if host.point_use_type(point) == UseType::DropStone {
            host.set_task_callback(npc, Callback::OnWork);
            host.queue_ai(
                npc,
                AiCommand::HoldItems {
                    right: "It_CarryFront_Stone",
                    left: "",
                },
            );
        } else {
            host.set_task_callback(npc, Callback::OnGoto);
            host.queue_ai(
                npc,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
        }
        return Flow::Yield;
    }
    if once(host, npc, 2) {
        let target = host.destination_point(npc);
        host.queue_ai(npc, AiCommand::GotoExt { target });
        return Flow::Yield;
    }
    if host.is_on_destination(npc) {
        continue_routine(host, npc);
    }
    host.set_state_progress(npc, 0);
    Flow::Done
}

/// Port of `ZS_Listen_Loop` (S:97ec0): with 50 percent chance listen for a medium time; the state restarts.
fn listen_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.random(100) < 50 {
        let s = secs(host, npc, Dur::Medium);
        return queue(
            host,
            npc,
            AiCommand::Action {
                action: Action::Listen,
                secs: s,
            },
        );
    }
    Out::Abort
}

/// Back-slot weapons a guard stows (`FindStackIndex` slots 5 and 6).
fn guard_unequip_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    for slot in [Slot::BackLeft, Slot::BackRight] {
        let stack = host.find_stack_index(npc, slot);
        host.unequip_stack(npc, stack);
    }
    Out::Next
}

fn guard_gate_callback(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if is_gate_guard(host, npc) {
        host.set_task_callback(npc, Callback::None);
        host.set_local_callback(npc, Callback::OnGateGuard);
    }
    Out::Next
}

fn preach_branch(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let next = if host.has_svm_sample(npc, "Speech") {
        State::PreachSvm
    } else {
        State::PreachAutoPreach
    };
    host.set_state(npc, next);
    Out::Leave
}

/// Arrival step of `ZS_Sleep` (S:9ea00): the `OnSleep` callback wakes the NPC; lie down on the bed or the ground.
fn arrive_sleep(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if !host.is_on_destination(npc) {
        return Out::Abort;
    }
    host.set_task_callback(npc, Callback::OnSleep);
    match host.point_use_type(host.destination_point(npc)) {
        UseType::Bed => host.set_state(npc, State::SleepBed),
        UseType::SleepGround => host.set_state(npc, State::SleepGround),
        _ => host.set_task(npc, State::Idiot),
    }
    Out::Leave
}

/// Seconds of state time between sleep wake-up checks (`_DAT_101074f0`).
pub const SLEEP_POLL_SECS: f32 = 60.0;

/// Port of `ZS_Sleep_Loop` (S:9f500): poll once a minute; stay asleep while the player talks to the NPC, otherwise
/// wake when the routine time is no longer "sleep" (saying the "Awake" voice line).
fn sleep_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) <= SLEEP_POLL_SECS {
        return Out::Abort;
    }
    host.set_state_time(npc, 0.0);
    if host.in_dialog_with_player(npc) {
        return Out::Abort;
    }
    let time = routine_time(host.species(npc), host.world_hour());
    if time != RoutineTime::Sleep {
        host.queue_ai(
            npc,
            AiCommand::SaySvm {
                family: "Awake",
                index: 0,
            },
        );
        continue_routine(host, npc);
        return Out::Yield;
    }
    Out::Abort
}

/// Port of `ZS_Smalltalk` (S:9f6c0): the NPCs are paired through their targets; one of them leads the talk.
fn smalltalk_dispatch(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnWork);
    let partner = host.current_target(npc);
    if partner.is_none() {
        host.set_task(npc, State::Idiot);
        return Out::Leave;
    }
    let leader = host.current_target(partner) == npc && npc.0 < partner.0;
    host.set_state(
        npc,
        if leader {
            State::SmalltalkLeader
        } else {
            State::SmalltalkPartner
        },
    );
    Out::Leave
}

fn smalltalk_say(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let i = host.random(5);
    queue(
        host,
        npc,
        AiCommand::SaySvm {
            family: "Smalltalk",
            index: i,
        },
    )
}

fn smalltalk_leader_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > SLEEP_POLL_SECS {
        continue_routine(host, npc);
        Out::Leave
    } else {
        Out::Abort
    }
}

fn smalltalk_partner_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > SLEEP_POLL_SECS {
        host.set_state_time(npc, 0.0);
        continue_routine(host, npc);
        Out::Leave
    } else {
        Out::Abort
    }
}

/// Port of `ZS_RunToAttackPoint` (S:9c910): draw the weapon, run to the assigned point, then mill about
/// (`ZS_HangAround`). Without a point the NPC mills about immediately.
fn run_to_attack_point(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    let point = host.destination_point(npc);
    if once(host, npc, 0) {
        if point.is_none() {
            host.set_task(npc, State::HangAround);
            return Flow::Done;
        }
        host.queue_ai(npc, AiCommand::SelectWeapon);
        return Flow::Yield;
    }
    if once(host, npc, 1) {
        host.set_task_callback(npc, Callback::OnGoto);
        host.queue_ai(npc, AiCommand::GotoExt { target: point });
        return Flow::Yield;
    }
    if host.is_on_destination(npc) {
        host.full_stop(npc);
        host.set_task(npc, State::HangAround);
    } else {
        host.set_state_progress(npc, 0);
    }
    Flow::Done
}

/// A Bloodfly standing in water draws its "Fist" so it can swim (species 23 plus floor material
/// 3 in the DLL).
fn hold_fist_if_in_water(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.species(npc) == crate::enums::Species::Bloodfly && host.standing_in_water(npc) {
        let stack = host.assure_items(npc, "Fist", 4, 1);
        return queue(
            host,
            npc,
            AiCommand::HoldInventoryStacks {
                right: stack,
                left: -1,
            },
        );
    }
    Out::Next
}

fn monster_roam_action(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let action = ambient_action(host, npc);
    let s = secs(host, npc, Dur::Medium);
    queue(host, npc, AiCommand::Action { action, secs: s })
}

fn ambient_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnGoto);
    queue(host, npc, AiCommand::StandUp)
}

/// First time through remember where the creature spawned and walk there.
fn ambient_home(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.start_position(npc).is_none() {
        let here = host.position(npc);
        host.set_start_position(npc, here);
        return queue(host, npc, AiCommand::GotoPos { pos: here });
    }
    Out::Next
}

fn ambient_dispatch(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let time = routine_time(host.species(npc), host.world_hour());
    host.set_state(
        npc,
        if time == RoutineTime::Sleep {
            State::AmbientSleep
        } else {
            State::AmbientLoop
        },
    );
    Out::Leave
}

/// Pig and Cow stay where they are; others wander `rand(200) + 200` (ambient creatures) or
/// `rand(200) + 300` (everything else) around their start position.
fn ambient_wander(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    use crate::enums::Species;
    if matches!(host.species(npc), Species::Pig | Species::Cow) {
        return Out::Next;
    }
    let Some(center) = host.start_position(npc) else {
        return Out::Next;
    };
    let base = if is_ambient_creature(host, npc) {
        200
    } else {
        300
    };
    let radius = (host.random(200) + base) as f32;
    let pos: Pos = host.random_navigation_point(center, radius);
    queue(host, npc, AiCommand::GotoPos { pos })
}

fn ambient_idle_action(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let action = ambient_action(host, npc);
    let s = secs(host, npc, Dur::Ambient);
    queue(host, npc, AiCommand::Action { action, secs: s })
}

/// Bloodflies do not sleep on water.
fn ambient_sleep_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.species(npc) == crate::enums::Species::Bloodfly && host.standing_in_water(npc) {
        host.set_task(npc, State::Ambient);
        return Out::Leave;
    }
    Out::Next
}

fn ambient_sleep_wake(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > SLEEP_POLL_SECS {
        host.set_state_time(npc, 0.0);
        host.set_task(npc, State::Ambient);
        return Out::Leave;
    }
    Out::Abort
}

/// First step of `ZS_AnimalSleep` (S:898c0): no sleeping point means an ordinary ambient creature.
fn animal_sleep_check(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.destination_point(npc).is_none() {
        host.set_task(npc, State::Ambient);
        return Out::Leave;
    }
    Out::Next
}

fn animal_arrived(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.is_on_destination(npc) {
        Out::Next
    } else {
        Out::Abort
    }
}

/// Final step of `ZS_AnimalSleep` (S:898c0): settle into the sleeping mode: raw AI mode 7, `OnAnimalSleep` wakes the animal when the
/// player comes near.
fn animal_enter_sleep(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Sleep);
    host.set_task_callback(npc, Callback::OnAnimalSleep);
    host.set_state(npc, State::AnimalSleepSleepGround);
    Out::Leave
}

fn animal_sleep_wake(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > SLEEP_POLL_SECS {
        continue_routine(host, npc);
        return Out::Leave;
    }
    Out::Abort
}

/// Port of `ZS_HangAround` (S:93de0): the generic idle task.
///
/// Pass 1 puts everything away. Pass 2 branches on the use type of the destination: Stool,
/// Bench, Waterpipe and SitGround have their own sitting states, a campfire seat fries meat on a
/// coin toss, a `Smalltalk` point looks for a chat partner (pairing both NPCs into `ZS_Smalltalk`,
/// else `ZS_HangAround_DoNothing`), everything else stands around. An NPC without a destination
/// walks to a random free Bench, SitGround, SitCampfire or Stand object.
fn hang_around(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    if once(host, npc, 0) {
        host.queue_ai(
            npc,
            AiCommand::HoldInventoryStacks {
                right: -1,
                left: -1,
            },
        );
        return Flow::Yield;
    }
    let point = host.destination_point(npc);
    if once(host, npc, 1) && point.is_some() && host.is_on_destination(npc) {
        host.set_task_callback(npc, Callback::OnWork);
        let next = match host.point_use_type(point) {
            UseType::Stool => State::HangAroundStool,
            UseType::Bench => State::HangAroundBench,
            UseType::Waterpipe => State::HangAroundWaterpipe,
            UseType::SitGround => State::HangAroundSitGround,
            UseType::SitCampfire => {
                if host.random(2) != 0 {
                    State::HangAroundFryMeat
                } else {
                    State::HangAroundSitGround
                }
            }
            UseType::Smalltalk => {
                let partner = host.find_smalltalk_partner(npc);
                if partner.is_some() {
                    host.full_stop(npc);
                    host.set_current_target(npc, partner);
                    host.set_current_target(partner, npc);
                    host.set_task(partner, State::Smalltalk);
                    host.set_task(npc, State::Smalltalk);
                } else {
                    host.set_state(npc, State::HangAroundDoNothing);
                }
                return Flow::Done;
            }
            _ => State::HangAroundStand,
        };
        host.set_state(npc, next);
        return Flow::Done;
    }
    if once(host, npc, 2) {
        let mut found = Vec::new();
        for u in [
            UseType::Bench,
            UseType::SitGround,
            UseType::SitCampfire,
            UseType::Stand,
        ] {
            let o = host.find_use_object(npc, u);
            if o.is_some() && !found.contains(&o) {
                found.push(o);
            }
        }
        if found.is_empty() {
            host.set_state(npc, State::HangAroundDoNothing);
            return Flow::Done;
        }
        let i = host.random(found.len() as i32) as usize;
        let target = found[i.min(found.len() - 1)];
        host.set_task_callback(npc, Callback::OnGoto);
        set_current_destination_point(host, npc, target);
        host.queue_ai(npc, AiCommand::GotoExt { target });
        return Flow::Yield;
    }
    host.set_state_progress(npc, 0);
    Flow::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AnchorType, Species};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi};
    use crate::testhost::TestWorld;

    /// Drive an NPC's state until `Done` with no further transition or until `max` calls,
    /// returning every queued AI command in order.
    fn drive(w: &mut TestWorld, npc: EntityId, max: usize) -> Vec<AiCommand> {
        let start = w.current_state(npc);
        for _ in 0..max {
            run_state(w, npc);
            w.finish_commands(npc);
            if w.current_state(npc) != start {
                break;
            }
        }
        std::mem::take(&mut w.log)
    }

    fn work_world(use_type: UseType) -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        w.hour = 10.0;
        let npc = w.spawn_npc("Worker", Species::Human, 10, 10);
        let anchor = w.add_anchor("WORK", AnchorType::Local, 0);
        let point = w.add_point(use_type, anchor);
        w.set_destination(npc, point);
        (w, npc, point)
    }

    #[test]
    fn every_task_in_the_selection_table_has_a_body() {
        for s in State::ALL {
            let name = s.name();
            // Anything the table can hand out must be runnable here.
            let assigned = matches!(
                s,
                State::Smith
                    | State::Grind
                    | State::Cook
                    | State::Barbecue
                    | State::Alchemy
                    | State::Study
                    | State::CarryStones
                    | State::Digger
                    | State::Farmer
                    | State::Woodwork
                    | State::Lumberjack
                    | State::Sleep
                    | State::SlaveBabe
                    | State::Boss
                    | State::Bard
                    | State::Preach
                    | State::Guard
                    | State::Trader
                    | State::Pray
                    | State::RunToAttackPoint
                    | State::OrcParty
                    | State::HangAround
                    | State::AnimalSleep
                    | State::Monster
                    | State::Ambient
            );
            if assigned {
                assert!(definition(*s).is_some(), "{name} has no body");
            }
        }
    }

    #[test]
    fn smith_cycles_forge_anvil_forge_anvil_cool() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Smith", Species::Human, 10, 10);
        let forge = w.add_point(UseType::Forge, EntityId::NONE);
        let anvil = w.add_point(UseType::Anvil, EntityId::NONE);
        let barrel = w.add_point(UseType::WaterBarrel, EntityId::NONE);
        w.set_use_object(npc, UseType::Forge, forge);
        w.set_use_object(npc, UseType::Anvil, anvil);
        w.set_use_object(npc, UseType::WaterBarrel, barrel);
        w.set_task_directly(npc, State::Smith);
        let mut visited = Vec::new();
        for _ in 0..40 {
            run_state(&mut w, npc);
            for c in w.finish_commands(npc) {
                if let AiCommand::GotoExt { target } = c {
                    visited.push(target);
                }
            }
            if matches!(w.current_task(npc), Some(t) if t != State::Smith) {
                break;
            }
        }
        assert_eq!(visited, vec![forge, anvil, forge, anvil, barrel]);
        assert_eq!(w.task_position(npc), 0, "SetTask resets the position");
    }

    #[test]
    fn smith_without_anvil_goes_idle_but_may_skip_forge() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Smith", Species::Human, 10, 10);
        let anvil = w.add_point(UseType::Anvil, EntityId::NONE);
        w.set_use_object(npc, UseType::Anvil, anvil);
        w.set_task_directly(npc, State::Smith);
        // Position 1 (forge) is optional: the pass is skipped, position 2 walks to the anvil.
        run_state(&mut w, npc);
        assert!(w.finish_commands(npc).is_empty());
        run_state(&mut w, npc);
        assert!(matches!(
            w.finish_commands(npc).as_slice(),
            [AiCommand::GotoExt { .. }]
        ));
        // Remove the anvil: the required position now falls back to idling.
        let mut w2 = TestWorld::new();
        let npc2 = w2.spawn_npc("Smith", Species::Human, 10, 10);
        w2.set_task_directly(npc2, State::Smith);
        w2.set_task_position(npc2, 1);
        run_state(&mut w2, npc2);
        assert_eq!(w2.current_task(npc2), Some(State::Idiot));
    }

    #[test]
    fn anvil_state_sequence_and_durations() {
        let (mut w, npc, _) = work_world(UseType::Anvil);
        w.set_task_directly(npc, State::SmithAnvil);
        w.rolls = vec![1];
        let cmds = drive(&mut w, npc, 10);
        assert_eq!(cmds[0], AiCommand::StandUp);
        assert!(matches!(cmds[1], AiCommand::HoldInventoryStacks { .. }));
        // Long work: (1 + 2) * 60 seconds.
        assert_eq!(
            cmds[2],
            AiCommand::Action {
                action: Action::Anvil,
                secs: 180
            }
        );
        // Back to the Smith dispatcher.
        assert_eq!(w.current_state(npc), Some(State::Smith));
        assert!(w.has_item(npc, "It_Hammer_Smith") && w.has_item(npc, "It_Sword"));
    }

    #[test]
    fn debug_variant_makes_every_action_ten_seconds() {
        let (mut w, npc, _) = work_world(UseType::PickOre);
        w.variants.fast_routines = true;
        w.set_task_directly(npc, State::DiggerPickOre);
        let cmds = drive(&mut w, npc, 10);
        assert!(cmds.contains(&AiCommand::Action {
            action: Action::PickOre,
            secs: 10
        }));
    }

    #[test]
    fn destination_dispatcher_walks_then_switches_by_use_type() {
        let (mut w, npc, point) = work_world(UseType::PickGround);
        w.set_task_directly(npc, State::Digger);
        assert_eq!(run_state(&mut w, npc), Flow::Yield);
        let cmds = w.finish_commands(npc);
        assert_eq!(cmds, vec![AiCommand::GotoExt { target: point }]);
        assert_eq!(w.callback(npc), Callback::OnGoto);
        // Not there yet: the dispatcher gives up this pass and restarts.
        w.set_on_destination(npc, false);
        assert_eq!(run_state(&mut w, npc), Flow::Done);
        assert_eq!(w.current_state(npc), Some(State::Digger));
        // Arrived: switches to the matching sub-state.
        run_state(&mut w, npc);
        w.finish_commands(npc);
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        assert_eq!(w.callback(npc), Callback::OnWork);
        assert_eq!(w.current_state(npc), Some(State::DiggerPickGround));
    }

    #[test]
    fn missing_destination_falls_back_to_idiot() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Cook", Species::Human, 10, 10);
        w.set_task_directly(npc, State::Cook);
        run_state(&mut w, npc);
        assert_eq!(w.current_task(npc), Some(State::Idiot));
    }

    #[test]
    fn cook_ends_with_a_break_check() {
        let (mut w, npc, _) = work_world(UseType::Cauldron);
        w.set_task_directly(npc, State::Cook);
        w.set_on_destination(npc, true);
        w.rolls = vec![0, 0];
        let cmds = drive(&mut w, npc, 12);
        assert!(
            cmds.iter()
                .any(|c| matches!(c, AiCommand::ActionAtPoint { secs: 120 }))
        );
        assert!(cmds.contains(&AiCommand::HoldItems {
            right: "It_Scoop",
            left: ""
        }));
        // The anchor-backed point continues the routine instead of breaking.
        assert_ne!(w.current_task(npc), Some(State::Break));
    }

    #[test]
    fn random_pick_uses_scanner_objects() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Study", Species::Human, 10, 10);
        let stand = w.add_point(UseType::Bookstand, EntityId::NONE);
        w.set_use_object(npc, UseType::Bookstand, stand);
        w.set_task_directly(npc, State::Study);
        run_state(&mut w, npc);
        assert_eq!(
            w.finish_commands(npc),
            vec![AiCommand::GotoExt { target: stand }]
        );
        assert_eq!(w.destination_point(npc), stand, "reserved by the goto");
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::StudyBookstand));
    }

    #[test]
    fn farmer_chooses_between_rake_and_field_work() {
        let (mut w, npc, _) = work_world(UseType::Field);
        w.set_task_directly(npc, State::Farmer);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        w.set_on_destination(npc, true);
        w.rolls = vec![0];
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::FarmerRakeField));
        w.set_task_directly(npc, State::Farmer);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        w.rolls = vec![1];
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::FarmerFieldWork));
    }

    #[test]
    fn break_states_by_use_type() {
        let (mut w, npc, _) = work_world(UseType::Bench);
        w.set_task_directly(npc, State::Break);
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::BreakBench));
        let cmds = drive(&mut w, npc, 6);
        assert!(cmds.contains(&AiCommand::AniStateAction(Action::SitBench)));
        assert!(cmds.iter().any(|c| matches!(
            c,
            AiCommand::Enjoy {
                item_a: "It_Joint",
                ..
            }
        )));
    }

    #[test]
    fn break_without_objects_continues_the_routine() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guy", Species::Human, 10, 10);
        w.set_task_directly(npc, State::Break);
        run_state(&mut w, npc);
        assert!(matches!(
            w.current_task(npc),
            Some(t) if t != State::Break
        ));
    }

    #[test]
    fn sleep_lies_down_and_wakes_at_the_end_of_the_night() {
        let (mut w, npc, _) = work_world(UseType::Bed);
        w.hour = 2.0;
        w.set_task_directly(npc, State::Sleep);
        w.set_on_destination(npc, true);
        for _ in 0..8 {
            run_state(&mut w, npc);
            w.finish_commands(npc);
            if w.current_state(npc) == Some(State::SleepLoop) {
                break;
            }
        }
        assert_eq!(w.current_state(npc), Some(State::SleepLoop));
        assert_eq!(w.callback(npc), Callback::OnSleep);
        // Still night: stays asleep even after the poll interval.
        w.set_state_time(npc, 61.0);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::SleepLoop));
        // Morning: wakes, says "Awake", resumes the routine.
        w.hour = 6.5;
        w.set_state_time(npc, 61.0);
        w.log.clear();
        run_state(&mut w, npc);
        assert!(w.log.iter().any(|c| matches!(
            c,
            AiCommand::SaySvm {
                family: "Awake",
                ..
            }
        )));
        assert_ne!(w.current_state(npc), Some(State::SleepLoop));
        // A conversation partner keeps the sleeper in bed.
        let (mut w, npc, _) = work_world(UseType::Bed);
        w.hour = 6.5;
        w.set_task_directly(npc, State::SleepLoop);
        w.in_dialog = true;
        w.set_state_time(npc, 61.0);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::SleepLoop));
        assert!(w.state_time(npc) < 1.0, "poll timer reset");
    }

    #[test]
    fn hang_around_branches_on_the_use_type() {
        for (use_type, expect) in [
            (UseType::Stool, State::HangAroundStool),
            (UseType::Bench, State::HangAroundBench),
            (UseType::Waterpipe, State::HangAroundWaterpipe),
            (UseType::SitGround, State::HangAroundSitGround),
            (UseType::Stand, State::HangAroundStand),
            (UseType::PirateTreasure, State::HangAroundStand),
        ] {
            let (mut w, npc, _) = work_world(use_type);
            w.set_task_directly(npc, State::HangAround);
            w.set_on_destination(npc, true);
            run_state(&mut w, npc);
            w.finish_commands(npc);
            run_state(&mut w, npc);
            assert_eq!(w.current_state(npc), Some(expect), "{use_type:?}");
        }
    }

    #[test]
    fn hang_around_campfire_toss_and_smalltalk_pairing() {
        let (mut w, npc, _) = work_world(UseType::SitCampfire);
        w.set_task_directly(npc, State::HangAround);
        w.set_on_destination(npc, true);
        w.rolls = vec![1];
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::HangAroundFryMeat));

        let (mut w, npc, _) = work_world(UseType::Smalltalk);
        let buddy = w.spawn_npc("Buddy", Species::Human, 10, 10);
        w.set_smalltalk_partner(buddy);
        w.set_task_directly(npc, State::HangAround);
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(w.current_task(npc), Some(State::Smalltalk));
        assert_eq!(w.current_task(buddy), Some(State::Smalltalk));
        assert_eq!(w.current_target(npc), buddy);
        // No partner: stands around doing nothing.
        let (mut w, npc, _) = work_world(UseType::Smalltalk);
        w.set_task_directly(npc, State::HangAround);
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::HangAroundDoNothing));
    }

    #[test]
    fn hang_around_without_destination_finds_a_seat() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guy", Species::Human, 10, 10);
        let bench = w.add_point(UseType::Bench, EntityId::NONE);
        w.set_use_object(npc, UseType::Bench, bench);
        w.set_task_directly(npc, State::HangAround);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(
            w.finish_commands(npc),
            vec![AiCommand::GotoExt { target: bench }]
        );
        // No seats: do nothing.
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guy", Species::Human, 10, 10);
        w.set_task_directly(npc, State::HangAround);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::HangAroundDoNothing));
    }

    #[test]
    fn guard_stows_back_weapons_and_registers_gate_callback() {
        let (mut w, npc, _) = work_world(UseType::Guard);
        w.set_guard(npc, crate::enums::GuardStatus::Active, EntityId(42));
        w.equip_back(npc);
        w.set_task_directly(npc, State::GuardLoop);
        let cmds = drive(&mut w, npc, 6);
        assert_eq!(
            w.unequipped.len(),
            2,
            "both back slots are stowed once per pass"
        );
        assert_eq!(w.local_callback(npc), Callback::OnGateGuard);
        assert!(cmds.iter().any(|c| matches!(
            c,
            AiCommand::Action {
                action: Action::Ambient,
                ..
            }
        )));
    }

    #[test]
    fn preach_uses_voice_lines_when_available() {
        let (mut w, npc, _) = work_world(UseType::Preach);
        w.set_task_directly(npc, State::Preach);
        w.set_on_destination(npc, true);
        w.has_speech = true;
        run_state(&mut w, npc); // goto
        w.finish_commands(npc);
        run_state(&mut w, npc); // arrival + branch
        assert_eq!(w.current_state(npc), Some(State::PreachSvm));
        let cmds = drive(&mut w, npc, 20);
        let lines = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    AiCommand::SaySvm {
                        family: "Speech",
                        ..
                    }
                )
            })
            .count();
        assert_eq!(lines, 10);
        let (mut w, npc, _) = work_world(UseType::Preach);
        w.set_task_directly(npc, State::Preach);
        w.set_on_destination(npc, true);
        run_state(&mut w, npc);
        w.finish_commands(npc);
        run_state(&mut w, npc);
        assert_eq!(w.current_state(npc), Some(State::PreachAutoPreach));
    }

    #[test]
    fn ambient_creatures_wander_near_home_and_graze() {
        let mut w = TestWorld::new();
        w.hour = 12.0;
        let boar = w.spawn_npc("Boar", Species::Boar, 5, 5);
        w.set_position(boar, Pos::new(100.0, 0.0, 100.0));
        w.set_task_directly(boar, State::Ambient);
        let mut seen_goto = false;
        for _ in 0..12 {
            run_state(&mut w, boar);
            for c in w.finish_commands(boar) {
                if let AiCommand::GotoPos { pos } = c {
                    seen_goto = true;
                    let _ = pos;
                }
            }
            if w.current_state(boar) == Some(State::AmbientLoop) {
                break;
            }
        }
        assert!(seen_goto);
        assert_eq!(w.current_state(boar), Some(State::AmbientLoop));
        assert_eq!(w.start_position(boar), Some(Pos::new(100.0, 0.0, 100.0)));
        w.rolls = vec![10, 0, 49, 5];
        let mut acted = false;
        for _ in 0..8 {
            run_state(&mut w, boar);
            for c in w.finish_commands(boar) {
                if let AiCommand::Action { action, secs } = c {
                    assert!(matches!(action, Action::EatGround | Action::Ambient));
                    assert!((30..=60).contains(&secs));
                    acted = true;
                }
            }
            if w.current_state(boar) == Some(State::Ambient) {
                break;
            }
        }
        assert!(acted);
    }

    #[test]
    fn pigs_do_not_wander_and_creatures_sleep_at_night() {
        let mut w = TestWorld::new();
        w.hour = 12.0;
        let pig = w.spawn_npc("Pig", Species::Pig, 5, 5);
        w.set_task_directly(pig, State::AmbientLoop);
        w.set_start_position(pig, Pos::new(0.0, 0.0, 0.0));
        let mut gotos = 0;
        for _ in 0..6 {
            run_state(&mut w, pig);
            for c in w.finish_commands(pig) {
                if matches!(c, AiCommand::GotoPos { .. }) {
                    gotos += 1;
                }
            }
        }
        assert_eq!(gotos, 0);
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        w.hour = 5.0;
        w.set_start_position(wolf, Pos::default());
        w.set_task_directly(wolf, State::Ambient);
        for _ in 0..6 {
            run_state(&mut w, wolf);
            w.finish_commands(wolf);
            if w.current_state(wolf) == Some(State::AmbientSleep) {
                break;
            }
        }
        assert_eq!(w.current_state(wolf), Some(State::AmbientSleep));
    }

    #[test]
    fn animal_sleep_walks_lies_down_then_wakes() {
        let mut w = TestWorld::new();
        w.hour = 5.0;
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        let den = w.add_point(UseType::SleepGround, EntityId::NONE);
        w.set_destination(wolf, den);
        w.set_task_directly(wolf, State::AnimalSleep);
        w.set_on_destination(wolf, true);
        let mut cmds = Vec::new();
        for _ in 0..8 {
            run_state(&mut w, wolf);
            cmds.extend(w.finish_commands(wolf));
            if w.current_state(wolf) == Some(State::AnimalSleepSleepGround) {
                break;
            }
        }
        assert_eq!(w.current_state(wolf), Some(State::AnimalSleepSleepGround));
        assert_eq!(w.ai_mode(wolf), AiMode::Sleep);
        assert_eq!(w.callback(wolf), Callback::OnAnimalSleep);
        assert!(cmds.contains(&AiCommand::AniStateAction(Action::SleepGround)));
        // Morning: the poll wakes the animal and the routine resumes.
        w.hour = 12.0;
        w.set_state_time(wolf, 61.0);
        let ground = w.current_state(wolf);
        run_state(&mut w, wolf);
        assert_ne!(w.current_state(wolf), ground);
        // No den assigned: becomes an ordinary ambient creature.
        let mut w = TestWorld::new();
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        w.set_task_directly(wolf, State::AnimalSleep);
        run_state(&mut w, wolf);
        assert_eq!(w.current_task(wolf), Some(State::Ambient));
    }

    #[test]
    fn bloodfly_draws_fist_in_water() {
        let mut w = TestWorld::new();
        let fly = w.spawn_npc("Fly", Species::Bloodfly, 5, 5);
        w.in_water = true;
        w.set_task_directly(fly, State::MonsterRoam);
        run_state(&mut w, fly);
        assert!(w.has_item(fly, "Fist"));
    }

    #[test]
    fn carry_stones_holds_a_stone_only_when_dropping() {
        let (mut w, npc, _) = work_world(UseType::DropStone);
        w.set_task_directly(npc, State::CarryStones);
        w.set_on_destination(npc, true);
        let cmds = drive(&mut w, npc, 10);
        assert!(cmds.contains(&AiCommand::HoldItems {
            right: "It_CarryFront_Stone",
            left: ""
        }));
        let (mut w, npc, _) = work_world(UseType::TakeStone);
        w.set_task_directly(npc, State::CarryStones);
        w.set_on_destination(npc, true);
        let cmds = drive(&mut w, npc, 10);
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, AiCommand::HoldItems { .. }))
        );
    }

    #[test]
    fn smalltalk_leader_and_partner_roles() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        w.set_current_target(a, b);
        w.set_current_target(b, a);
        w.set_task_directly(a, State::Smalltalk);
        w.set_task_directly(b, State::Smalltalk);
        run_state(&mut w, a);
        run_state(&mut w, b);
        assert_eq!(w.current_state(a), Some(State::SmalltalkLeader));
        assert_eq!(w.current_state(b), Some(State::SmalltalkPartner));
        // The leader talks; both leave after the poll interval.
        w.set_state_time(a, 61.0);
        w.set_state_time(b, 61.0);
        w.set_task_directly(a, State::SmalltalkLeaderLoop);
        w.set_state_time(a, 61.0);
        for _ in 0..6 {
            run_state(&mut w, a);
            w.finish_commands(a);
        }
        assert!(w.log.iter().any(|c| matches!(
            c,
            AiCommand::SaySvm {
                family: "Smalltalk",
                ..
            }
        )));
    }
}
