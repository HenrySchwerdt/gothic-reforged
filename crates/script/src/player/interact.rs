//! `PS_Interact` and the player's object states (B18, with `_AI_Unlock` of B13).
//!
//! The dispatcher [`ps_interact`] looks at the use type of the focused object, checks what the
//! object needs (tools, recipes, perks, a key or lockpick) and switches into the state of that
//! kind of object. A refused interaction prints a HUD message ([`PlayerEvent::Refused`]), makes
//! the hero grumble (`DoesntWork_0N`, `NeedKey`, ...) and returns to `PS_Normal`.
//!
//! The stations (anvil, cauldron, ...) follow one pattern: take the tools in hand, play the
//! change animation, record the use (`UsedByPlayer`, the object's use event), open the HUD window
//! and wait in `PS_Interact_Loop` until it closes; `PS_Interact_End` puts everything away.

use crate::assess::send_perception_use_interact_object;
use crate::crime::add_enclave_theft_count;
use crate::enums::{Action, AiMode, Slot, UseType};
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::player::api::{LockStatus, PlayerEvent, Refusal, SkillCategory, UiWindow};
use crate::player::{back_to_normal, step};
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};
use crate::speech::{say, start_say_doesnt_work, start_say_nothing_to_get};

/// Template of the lockpick item.
pub const LOCKPICK: &str = "It_Key_Lockpick";
/// `PSRoutine.TaskPosition` value a command writes to leave a persistent interaction.
pub const EXIT_REQUESTED: u32 = 1000;
/// Percent chance (out of 100) above which a risky lockpick attempt fails (`Random(100) > 70`).
const RISKY_PICK_LIMIT: i32 = 70;

// ---- CanOpen, _AI_Unlock ---------------------------------------------------------------------------------

/// Port of `CanOpen` (S:6cfe0): may the player open this chest or door?
///
/// Unlocked or lock-less objects always open. A key in the inventory opens a locked one (the
/// lock's `KeyAmount` keys, at least one). Otherwise the lock must be pickable: difficulty above
/// 3 is "Impossible", a valid key with a difficulty outside 1-3 means "need the key", a
/// lockpick (`It_Key_Lockpick`) must be in the inventory (the message differs with or without
/// a key template on the lock) and the perk of the difficulty (`Perk_LockPick_1..3`) is needed.
pub fn can_open(host: &dyn ScriptHost, player: EntityId, object: EntityId) -> Result<(), Refusal> {
    let Some(lock) = host.lock(object) else {
        return Ok(());
    };
    if lock.status == LockStatus::Unlocked {
        return Ok(());
    }
    let has_key_template = !lock.key.is_empty();
    if has_key_template {
        if host.has_items(player, &lock.key, lock.key_amount.max(1)) {
            return Ok(());
        }
        if !(1..=3).contains(&lock.difficulty) {
            return Err(Refusal::NeedKey);
        }
    }
    if lock.difficulty > 3 {
        return Err(Refusal::Impossible);
    }
    if !host.has_items(player, LOCKPICK, 1) {
        return Err(if has_key_template {
            Refusal::KeyOrLockpick
        } else {
            Refusal::NeedLockpick
        });
    }
    if !host.is_skill_active(player, "Perk_LockPick_1") {
        return Err(Refusal::MissingPerk("Perk_LockPick_1"));
    }
    match lock.difficulty {
        2 if !host.is_skill_active(player, "Perk_LockPick_2") => {
            Err(Refusal::MissingPerk("Perk_LockPick_2"))
        }
        3 if !host.is_skill_active(player, "Perk_LockPick_3") => {
            Err(Refusal::MissingPerk("Perk_LockPick_3"))
        }
        _ => Ok(()),
    }
}

/// Port of `_AI_Unlock` (S:4ca20): open the lock of `object` with the key or a lockpick.
///
/// A key in the inventory unlocks it (and is kept). Otherwise one lockpick is used up (message
/// `GO_LockpickUsed`) and the lock opens unless the attempt is *risky*: difficulty 1 without
/// `Perk_LockPick_2` or difficulty 2 without `Perk_LockPick_3` fail when `Random(100) > 70`.
/// Returns whether the lock is open afterwards.
pub fn unlock(host: &mut dyn ScriptHost, player: EntityId, object: EntityId) -> bool {
    let Some(lock) = host.lock(object) else {
        return true;
    };
    if lock.status == LockStatus::Unlocked {
        return true;
    }
    if !lock.key.is_empty() && host.has_items(player, &lock.key, lock.key_amount.max(1)) {
        host.set_lock_status(object, LockStatus::Unlocked);
        host.emit(PlayerEvent::LockPicked {
            object,
            success: true,
        });
        return true;
    }
    if !host.remove_item(player, LOCKPICK, 1) {
        return false;
    }
    let risky = match lock.difficulty {
        1 => !host.is_skill_active(player, "Perk_LockPick_2"),
        2 => !host.is_skill_active(player, "Perk_LockPick_3"),
        _ => false,
    };
    let success = !(risky && host.random(100) > RISKY_PICK_LIMIT);
    if success {
        host.set_lock_status(object, LockStatus::Unlocked);
    }
    host.emit(PlayerEvent::LockPicked { object, success });
    success
}

// ---- refusals ------------------------------------------------------------------------------------------------

/// The line the hero says for a refusal (`None`: nothing).
fn refusal_voice(host: &mut dyn ScriptHost, player: EntityId, r: &Refusal) {
    let key = match r {
        Refusal::NeedKey => "NeedKey",
        Refusal::KeyOrLockpick => "NeedKeyOrLockpick",
        Refusal::NeedLockpick => "NeedLockpick",
        Refusal::Impossible => "Impossible",
        Refusal::TooHard => "TooHard",
        Refusal::MissingPerk(p) if p.starts_with("Perk_LockPick") => "TooHard",
        Refusal::WrongGod => "No",
        Refusal::NothingToGet => {
            start_say_nothing_to_get(host, player);
            return;
        }
        Refusal::MissingIngredients => "MissingIngredients",
        Refusal::NotEnoughLp(_) => "NotEnoughLP",
        _ => {
            start_say_doesnt_work(host, player);
            return;
        }
    };
    say(host, player, key);
}

/// Report a refusal to the HUD and let the hero comment on it.
pub fn refuse(host: &mut dyn ScriptHost, player: EntityId, r: Refusal) {
    refusal_voice(host, player, &r);
    host.emit(PlayerEvent::Refused(r));
}

// ---- station table -----------------------------------------------------------------------------------------

/// How an object is used once the player stands at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A HUD window opens; the interaction ends with it.
    Window(UiWindow),
    /// An animation plays for a number of seconds, then the interaction ends.
    Timed(Action, i32),
    /// A pose is held until the player leaves ([`EXIT_REQUESTED`]).
    Pose(Action),
}

/// What a kind of interact object needs and does.
#[derive(Debug, Clone, Copy)]
pub struct Station {
    /// The interact point's use type.
    pub use_type: UseType,
    /// The player state of this kind of object.
    pub state: State,
    /// Items taken in the right and left hand (`AssureItems`).
    pub tools: [&'static str; 2],
    /// Use of the object.
    pub mode: Mode,
    /// The object is used up after one use (`UsedByPlayer`).
    pub single_use: bool,
}

const fn station(
    use_type: UseType,
    state: State,
    tools: [&'static str; 2],
    mode: Mode,
    single_use: bool,
) -> Station {
    Station {
        use_type,
        state,
        tools,
        mode,
        single_use,
    }
}

/// The stations a hero can use (the second switch of `PS_Interact`).
pub const STATIONS: &[Station] = &[
    station(
        UseType::Anvil,
        State::PsInteractAnvil,
        ["It_Hammer_Smith", "It_Sword"],
        Mode::Window(UiWindow::Smithing),
        false,
    ),
    // `PS_Interact_Forge` (S:711f0): take the hammer, change the action, hold the pose [I: the
    // DLL's tool template is not readable].
    station(
        UseType::Forge,
        State::PsInteractForge,
        ["It_Hammer_Smith", ""],
        Mode::Pose(Action::Forge),
        false,
    ),
    station(
        UseType::GrindStone,
        State::PsInteractGrindStone,
        ["", "It_Sword"],
        Mode::Window(UiWindow::Sharpen),
        false,
    ),
    station(
        UseType::Cauldron,
        State::PsInteractCauldron,
        ["It_Scoop", ""],
        Mode::Window(UiWindow::Cooking),
        false,
    ),
    station(
        UseType::Barbecue,
        State::PsInteractBarbecue,
        ["", ""],
        Mode::Window(UiWindow::Fry),
        false,
    ),
    station(
        UseType::Alchemy,
        State::PsInteractAlchemy,
        ["It_Potion", ""],
        Mode::Window(UiWindow::Alchemy),
        false,
    ),
    station(
        UseType::Bookstand,
        State::PsInteractBookstand,
        ["", ""],
        Mode::Window(UiWindow::Book),
        true,
    ),
    station(
        UseType::Stoneplate,
        State::PsInteractStoneplate,
        ["", ""],
        Mode::Window(UiWindow::Book),
        true,
    ),
    station(
        UseType::PickOre,
        State::PsInteractPickOre,
        ["It_Pickaxe", ""],
        Mode::Timed(Action::PickOre, 6),
        true,
    ),
    station(
        UseType::SawLog,
        State::PsInteractSawLog,
        ["It_Saw", ""],
        Mode::Timed(Action::SawLog, 8),
        false,
    ),
    station(
        UseType::OrcDrum,
        State::PsInteractOrcDrum,
        ["It_DrumStick", "It_DrumStick"],
        Mode::Timed(Action::OrcDrum, 8),
        false,
    ),
    station(
        UseType::Waterpipe,
        State::PsInteractUseWaterpipe,
        ["", ""],
        Mode::Timed(Action::Waterpipe, 10),
        false,
    ),
    station(
        UseType::WaterBarrel,
        State::PsInteractDrinkWaterBarrel,
        ["", ""],
        Mode::Timed(Action::DrinkWaterBarrel, 4),
        false,
    ),
    station(
        UseType::PirateTreasure,
        State::PsInteractPirateTreasure,
        ["It_Shovel", ""],
        Mode::Timed(Action::DigGround, 8),
        true,
    ),
    station(
        UseType::Campfire,
        State::PsInteractCampfire,
        ["", ""],
        Mode::Pose(Action::SitGround),
        false,
    ),
    station(
        UseType::Throne,
        State::PsInteractSitThrone,
        ["", ""],
        Mode::Pose(Action::SitThrone),
        false,
    ),
    station(
        UseType::Stool,
        State::PsInteractSitStool,
        ["", ""],
        Mode::Pose(Action::SitStool),
        false,
    ),
    station(
        UseType::Bench,
        State::PsInteractSitBench,
        ["", ""],
        Mode::Pose(Action::SitBench),
        false,
    ),
    station(
        UseType::SleepGround,
        State::PsInteractSleepGround,
        ["", ""],
        Mode::Pose(Action::SleepGround),
        false,
    ),
];

/// The station of a use type.
pub fn station_for_use(use_type: UseType) -> Option<&'static Station> {
    STATIONS.iter().find(|s| s.use_type == use_type)
}

/// The state a use type dispatches to (`None`: the interaction is not handled here).
pub fn state_for_use(use_type: UseType) -> Option<State> {
    match use_type {
        UseType::Door => Some(State::PsInteractDoor),
        UseType::Chest => Some(State::PsInteractChest),
        UseType::Shrine => Some(State::PsInteractShrine),
        UseType::Talk => Some(State::PsInteractTalk),
        UseType::Bed => Some(State::PsInteractSleepBed),
        UseType::OrcBoulder => Some(State::PsInteractTiltOrcBoulder),
        other => station_for_use(other).map(|s| s.state),
    }
}

// ---- PS_Interact -----------------------------------------------------------------------------------------

fn need_item(
    host: &dyn ScriptHost,
    p: EntityId,
    template: &'static str,
    n: u32,
) -> Result<(), Refusal> {
    if host.has_items(p, template, n) {
        Ok(())
    } else {
        Err(Refusal::MissingItem(template))
    }
}

fn need_recipes(host: &dyn ScriptHost, p: EntityId, c: SkillCategory) -> Result<(), Refusal> {
    if host.has_recipes(p, c) {
        Ok(())
    } else {
        Err(Refusal::NoRecipes(c))
    }
}

/// What the object needs before the hero may start (the first switch of `PS_Interact`).
pub fn check(host: &dyn ScriptHost, p: EntityId, object: EntityId) -> Result<(), Refusal> {
    match host.point_use_type(object) {
        UseType::Anvil => {
            need_recipes(host, p, SkillCategory::Smith)?;
            need_item(host, p, "It_Hammer_Smith", 1)
        }
        UseType::GrindStone => {
            if !host.is_skill_active(p, "Perk_SharpenBlade") {
                return Err(Refusal::MissingPerk("Perk_SharpenBlade"));
            }
            if host.has_blade_weapon(p) {
                Ok(())
            } else {
                Err(Refusal::NoBlade)
            }
        }
        UseType::Cauldron => {
            need_recipes(host, p, SkillCategory::Cooking)?;
            need_item(host, p, "It_Scoop", 1)
        }
        UseType::Alchemy => need_recipes(host, p, SkillCategory::Alchemy),
        UseType::Bookstand | UseType::Stoneplate => Ok(()),
        UseType::PickOre => {
            if host.used_by_player(object) {
                return Err(Refusal::NothingToGet);
            }
            need_item(host, p, "It_Pickaxe", 1)
        }
        UseType::SawLog => need_item(host, p, "It_Saw", 1),
        UseType::PirateTreasure => {
            if host.used_by_player(object) {
                return Err(Refusal::NothingToGet);
            }
            need_item(host, p, "It_Shovel", 1)
        }
        UseType::OrcDrum => need_item(host, p, "It_DrumStick", 2),
        UseType::Door | UseType::Chest => can_open(host, p, object),
        UseType::Shrine => {
            let name = host.name(object);
            let has = |event: &str| host.game_event_set(event);
            let refused = if has("Beliar_chosen") {
                name.contains("Beliar")
            } else if has("Innos_chosen") {
                name.contains("Innos")
            } else if has("Adanos_chosen") {
                name.contains("Innos") || name.contains("Beliar")
            } else {
                false
            };
            if refused {
                Err(Refusal::WrongGod)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// Port of `PS_Interact` (S:6da40): check the requirements of the focused object (the state's
/// target) and switch into its state. A refusal reports it and returns to `PS_Normal`; a use type
/// without a player state resumes the routine silently.
pub fn ps_interact(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    if object.is_none() {
        back_to_normal(host, p);
        return Flow::Done;
    }
    let use_type = host.point_use_type(object);
    let Some(state) = state_for_use(use_type) else {
        back_to_normal(host, p);
        return Flow::Done;
    };
    if let Err(r) = check(host, p, object) {
        refuse(host, p, r);
        back_to_normal(host, p);
        return Flow::Done;
    }
    host.set_task_position(p, 0);
    host.set_ai_mode(p, AiMode::Talk);
    host.emit(PlayerEvent::InteractStarted { object });
    host.set_state(p, state);
    Flow::Done
}

// ---- station states ----------------------------------------------------------------------------------------

fn tools_in_hand(host: &mut dyn ScriptHost, p: EntityId, tools: [&str; 2]) -> bool {
    if tools == ["", ""] {
        return false;
    }
    let right = if tools[0].is_empty() {
        -1
    } else {
        host.assure_items(p, tools[0], 0, 1)
    };
    let left = if tools[1].is_empty() {
        -1
    } else {
        host.assure_items(p, tools[1], 0, 1)
    };
    // The drum sticks are one stack: right and left hand take the same template.
    host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
    true
}

/// Common body of the station states: tools, change animation, record the use, open the window
/// or play the animation, hand over to the loop / the end.
fn station_state(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let Some(st) = host
        .current_state(p)
        .and_then(|s| STATIONS.iter().find(|x| x.state == s))
    else {
        return Flow::Done;
    };
    let object = host.current_target(p);
    loop {
        match step(host, p) {
            0 => {
                if tools_in_hand(host, p, st.tools) {
                    return Flow::Yield;
                }
            }
            1 => {
                host.queue_ai(p, AiCommand::ChangeAction);
                return Flow::Yield;
            }
            2 => {
                if st.single_use {
                    host.set_used_by_player(object, true);
                }
                let event = host.use_event(object);
                if !event.is_empty() {
                    host.raise_game_event(&event);
                }
                match st.mode {
                    Mode::Window(w) => {
                        host.emit(PlayerEvent::Window(w));
                        host.set_state(p, State::PsInteractLoop);
                        return Flow::Done;
                    }
                    Mode::Timed(action, secs) => {
                        host.queue_ai(p, AiCommand::Action { action, secs });
                        return Flow::Yield;
                    }
                    Mode::Pose(action) => {
                        host.queue_ai(p, AiCommand::AniStateAction(action));
                        return Flow::Yield;
                    }
                }
            }
            _ => {
                match st.mode {
                    Mode::Pose(_) if st.state == State::PsInteractSleepGround => {
                        host.set_state(p, State::PsDuskToDawn);
                    }
                    Mode::Pose(_) => host.set_state(p, State::PsInteractLoop),
                    _ => host.set_task(p, State::PsInteractEnd),
                }
                return Flow::Done;
            }
        }
    }
}

/// Port of `PS_Interact_Loop` (S:6fde0): wait while the window is open (or the pose is held),
/// then end.
fn ps_interact_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    let pose_held = matches!(
        station_for_use(host.point_use_type(object)).map(|s| s.mode),
        Some(Mode::Pose(_))
    );
    let waiting = if pose_held {
        host.task_position(p) != EXIT_REQUESTED
    } else {
        host.gui_page_open()
    };
    if waiting {
        host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
        return Flow::Yield;
    }
    host.set_task(p, State::PsInteractEnd);
    Flow::Done
}

/// Port of `IsBasket` (S:782e0): the hero just opened a chest (action `OpenChest`) and the object
/// is one of the harvestable "ChestPod" baskets, not a real chest.
pub fn is_basket(host: &dyn ScriptHost, player: EntityId, object: EntityId) -> bool {
    host.exists(object)
        && host.current_action(player) == Action::OpenChest
        && host.name(object).contains("ChestPod")
}

/// Port of `PS_Interact_WaitForExit`: wait for the exit key, then stop and leave the interaction.
fn ps_interact_wait_for_exit(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    if host.task_position(p) == EXIT_REQUESTED {
        host.full_stop(p);
        host.set_task(p, State::PsInteractEnd);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_Interact_End` (S:6f880): put the tools away, stand up, tell the game layer and
/// return to `PS_Normal`.
fn ps_interact_end(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    match step(host, p) {
        0 => {
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            Flow::Yield
        }
        1 => {
            host.queue_ai(p, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.emit(PlayerEvent::InteractEnded { object });
            back_to_normal(host, p);
            Flow::Done
        }
    }
}

// ---- chests and doors ------------------------------------------------------------------------------------

/// Port of `PS_Interact_Chest` (S:70210): open animation, tell the witnesses
/// (`SendPerceptionUseInteractObject`), unlock the lock (`_AI_Unlock`; a pick that fails makes
/// the hero say "PickBroke" and ends the interaction), generate the plunder from the treasure
/// sets, record the use event and open the loot window. The count of generated stacks is kept
/// in the task position so the loop can tell what was taken.
fn ps_interact_chest(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    loop {
        match step(host, p) {
            0 => {
                host.queue_ai(p, AiCommand::ChangeAction);
                return Flow::Yield;
            }
            1 => {
                send_perception_use_interact_object(host, p, object);
            }
            2 => {
                if !unlock(host, p, object) {
                    say(host, p, "PickBroke");
                    host.full_stop(p);
                    host.set_task(p, State::PsInteractEnd);
                    return Flow::Done;
                }
            }
            _ => {
                let stacks = host.generate_loot(object);
                host.set_task_position(p, stacks.max(0) as u32);
                let event = host.use_event(object);
                if !event.is_empty() {
                    host.raise_game_event(&event);
                }
                host.emit(PlayerEvent::Window(UiWindow::Loot));
                host.set_state(p, State::PsInteractChestLoop);
                return Flow::Done;
            }
        }
    }
}

/// Port of `PS_Interact_Chest_Loop` (S:70830): once the loot window is closed, every stack the
/// hero took counts as a theft for the enclave that owns the zone (`AddEnclaveTheftCount`).
fn ps_interact_chest_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    if host.gui_page_open() {
        host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
        return Flow::Yield;
    }
    let object = host.current_target(p);
    let initial = host.task_position(p) as i32;
    let taken = initial - host.loot_stacks(object);
    if taken > 0 {
        add_enclave_theft_count(host, p, p, taken);
    }
    host.emit(PlayerEvent::CloseWindow);
    host.set_task(p, State::PsInteractEnd);
    Flow::Done
}

/// Port of `PS_Interact_Door` (S:70970): a locked door is unlocked first; then the door swings
/// open or shut, the witnesses are told, the use event is raised and the interaction ends.
fn ps_interact_door(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    loop {
        match step(host, p) {
            0 => {
                host.queue_ai(p, AiCommand::ChangeAction);
                return Flow::Yield;
            }
            1 => {
                send_perception_use_interact_object(host, p, object);
                if !unlock(host, p, object) {
                    say(host, p, "PickBroke");
                    host.set_task(p, State::PsInteractEnd);
                    return Flow::Done;
                }
            }
            _ => {
                let open = !host.door_open(object);
                host.set_door_open(object, open);
                host.emit(PlayerEvent::DoorChanged { object, open });
                let event = host.use_event(object);
                if !event.is_empty() {
                    host.raise_game_event(&event);
                }
                host.set_task(p, State::PsInteractEnd);
                return Flow::Done;
            }
        }
    }
}

// ---- sleeping -------------------------------------------------------------------------------------------

/// Hour at which the hero wakes (`PS_DuskToDawn_Loop` waits for the clock to pass 8).
pub const WAKE_HOUR: f32 = 9.0;

/// Port of `PlayerSleep` (S:76440): advance the world by `hours` (playing time follows) and
/// refresh hit points, stamina and mana to their maximum.
pub fn player_sleep(host: &mut dyn ScriptHost, hours: f32) {
    use crate::host::Pool;
    host.advance_hours(hours);
    for pool in [Pool::Hit, Pool::Stamina, Pool::Mana] {
        let max = host.pm_pool_max(pool);
        host.pm_set_pool(pool, max);
    }
    host.emit(PlayerEvent::Slept { hours });
}

/// Hours of sleep from the given hour of the day until the wake-up hour.
pub fn hours_until_wake(hour: f32) -> f32 {
    if hour < WAKE_HOUR {
        WAKE_HOUR - hour
    } else {
        24.0 - hour + WAKE_HOUR
    }
}

/// Port of `PS_Interact_SleepBed` (S:75d40): a torch in hand is put away, the hero lies down and
/// the night starts (`PS_DuskToDawn`).
fn ps_interact_sleep_bed(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    loop {
        match step(host, p) {
            0 => {
                if host.slot_use_type(p, Slot::RightHand) == UseType::Torch {
                    host.queue_ai(
                        p,
                        AiCommand::HoldInventoryStacks {
                            right: -1,
                            left: -1,
                        },
                    );
                    return Flow::Yield;
                }
            }
            1 => {
                host.queue_ai(p, AiCommand::AniStateAction(Action::SleepBed));
                return Flow::Yield;
            }
            _ => {
                host.set_state(p, State::PsDuskToDawn);
                return Flow::Done;
            }
        }
    }
}

/// Port of `PS_DuskToDawn` + `PS_DuskToDawn_Loop` (S:66fc0, S:671d0): remember the hour, let the
/// clock run until it is morning, then restore the hero (`PlayerSleep`) and get up.
fn ps_dusk_to_dawn(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    match step(host, p) {
        0 => {
            host.emit(PlayerEvent::TimeScale(0.3));
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        1 => {
            let hours = hours_until_wake(host.world_hour());
            player_sleep(host, hours);
            host.emit(PlayerEvent::TimeScale(1.0));
            host.queue_ai(p, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.set_task(p, State::PsInteractEnd);
            Flow::Done
        }
    }
}

// ---- shrine and talk ------------------------------------------------------------------------------------

/// Port of `PS_Interact_Shrine` (S:72600) and `PS_Interact_Talk` (S:78210): kneel (shrine) and
/// start the conversation with the object. When nobody answers the interaction ends.
fn ps_interact_talk(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let object = host.current_target(p);
    let shrine = host.current_state(p) == Some(State::PsInteractShrine);
    loop {
        match step(host, p) {
            0 => {
                if shrine {
                    host.queue_ai(p, AiCommand::AniStateAction(Action::Kneel));
                    return Flow::Yield;
                }
            }
            _ => {
                host.start_dialog(object);
                host.emit(PlayerEvent::Window(UiWindow::Dialog));
                host.set_state(p, State::PsInteractLoop);
                return Flow::Done;
            }
        }
    }
}

/// Definitions of the interaction states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsInteract => custom(ps_interact),
        S::PsInteractLoop => custom(ps_interact_loop),
        S::PsInteractEnd => custom(ps_interact_end),
        S::PsInteractWaitForExit => custom(ps_interact_wait_for_exit),
        S::PsInteractChest => custom(ps_interact_chest),
        S::PsInteractChestLoop => custom(ps_interact_chest_loop),
        S::PsInteractDoor => custom(ps_interact_door),
        S::PsInteractSleepBed => custom(ps_interact_sleep_bed),
        S::PsDuskToDawn => custom(ps_dusk_to_dawn),
        S::PsInteractShrine | S::PsInteractTalk => custom(ps_interact_talk),
        s if STATIONS.iter().any(|x| x.state == s) => custom(station_state),
        _ => None,
    }
}
