//! The Bevy-free game core: simulation, dialog engine and state behind one fixed-rate tick.
//!
//! `Core::tick(dt, &[Command]) -> Vec<Event>`; presentation never mutates state, it sends
//! [`Command`]s and reacts to [`Event`]s.
use crate::processing::ProcessingRange;
use crate::world::PlayWorld;
use anyhow::{Context, Result};
use gothic_gameplay::dialog::{Choice, DialogSession};
use gothic_gameplay::script::{ExecResult, Output};
use gothic_gameplay::{Assumptions, Engine, GameDatabase, GameState};
use gothic_sim::data::NpcRecord;
use gothic_sim::item::ItemDb;
use gothic_sim::script::host::EntityId;
use gothic_sim::sim::{EventKind, Sim};
use gothic_sim::types::{Action, ActorId, AiMode};
use gothic_sim::world::{Pos, WorldQuery};
use gothic_sim::{Actor, SimConfig, brain::Task};
use gothic_world::trigger::TriggerRuntime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Fixed simulation rate.
pub const TICK_HZ: f32 = 30.0;
/// Melee reach of the hero in cm.
pub const HERO_REACH: f32 = 260.0;
/// Minimum time between hero strikes (ms).
const HERO_STRIKE_GAP_MS: u64 = 750;

/// Everything the presentation or input may ask of the core.
#[derive(Debug, Clone)]
pub enum Command {
    SpawnNpc {
        record: Box<NpcRecord>,
        at: Pos,
        yaw: f32,
    },
    /// Where the (player-controlled) hero stands now.
    HeroTransform {
        pos: Pos,
        yaw: f32,
    },
    Talk {
        npc: ActorId,
    },
    Choose {
        index: usize,
    },
    EndDialog,
    /// The hero swings at `target` (resolved with the sim's combat rules).
    Attack {
        target: ActorId,
    },
    /// Draw or holster the hero's weapon.
    SetDrawn(bool),
    // ---- player interaction (ported PS_* states, see `gothic_script::player`) ----------
    /// The confirm key on the focused entity (`Event::Confirmed` reports what it did).
    Confirm {
        /// Raw script entity id: actor `n` is `n + 1`, objects and items come from the
        /// simulation's script tables.
        focus: u32,
    },
    /// Walk into `PS_Interact` for an interact object (chest, door, work station, bed, ...).
    UseInteractObject {
        /// Raw script entity id of the object.
        object: u32,
    },
    /// Leave a seat, a bed or a work station.
    EndInteraction,
    /// Pick a loose item up (`PS_Normal_Take`).
    PickUp {
        /// Raw script entity id of the item.
        item: u32,
    },
    /// Open a lock with key or lockpick (`_AI_Unlock`); `success` is the UI mini-game result,
    /// `None` lets the script roll.
    Lockpick {
        /// Raw script entity id of the lock owner.
        object: u32,
        /// Mini-game result.
        success: Option<bool>,
    },
    /// Make a recipe at a work station (`OnPlayerSynthRecipe`).
    Craft {
        /// Recipe id.
        recipe: i32,
    },
    /// Use an inventory item (`OnPlayerUseStack`).
    UseItem {
        /// Item template.
        template: String,
    },
    /// Take everything from a body.
    QuickLoot {
        /// Raw script entity id of the body.
        body: u32,
    },
    /// Sleep (`PlayerSleep`, S:76440): `None` sleeps until the wake-up hour like the bed
    /// (`PS_DuskToDawn`, 09:00), `Some(h)` until the next `h` o'clock like the dialog `Sleep`
    /// command of the beds (8, 12, 20, 0). The clock advances, the hero's hit points, stamina and
    /// mana are restored and `Event::Player(Slept { hours })` is reported.
    Sleep {
        /// Target hour of the day, `None` until morning.
        hour: Option<u32>,
    },
    /// Order the companions: attack the focused character (`SendPerceptionCmdAttack`) or follow
    /// again (`SendPerceptionCmdFollow`). The original has no script caller; the order comes from
    /// the player's input, which a host binds to this command.
    CompanionCommand {
        /// Attack (true) or follow (false).
        attack: bool,
    },
    /// Toggle sneaking.
    ToggleSneak,
    /// Jump.
    Jump,
    /// The movement mode machine of the hero's body changed the mode
    /// (`OnMovementModeChanged`, S:adbf0): raw `gECharMovementMode` values and the time spent in
    /// the left state. Answered by [`Event::ModeChanged`].
    HeroModeChanged {
        /// `GetLastMovementMode`.
        last: u8,
        /// `GetMovementMode`.
        mode: u8,
        /// Seconds in the left state (the fall time when leaving the fall mode).
        state_secs: f32,
    },
    /// What the hero's body is doing (read by the stamina tick, S:b0520).
    HeroMovement(gothic_sim::vitals::Movement),
    /// Dodge (`ZS_Evade*`).
    Dodge {
        /// Direction.
        direction: gothic_sim::script::player::evade::Direction,
    },
    /// A key action of the hero: `OnPlayerGameKeyPressed` queues this `gEAction` id with the
    /// focus entity in the player's action queue; the loop state of the current task
    /// (`PS_Melee_Loop`) reads it.
    PlayerAction {
        /// `gEAction` id.
        id: u32,
        /// The focus entity at the time of the key.
        focus: Option<ActorId>,
    },
    /// `FullStop` + `SetState(name)` of the hero (the riposte out of the guard, the chains of
    /// `OnPlayerAction`); `name` is a `PS_*` state name.
    PlayerEnterState {
        /// State name.
        name: String,
    },
    /// The weapon key in `PS_Normal_Loop` (`OnPlayerSelectWeapon`).
    SelectWeapon,
    /// The host's per-frame view of the hero's control layer (`PSFocus`, `PSCamera`, the movement
    /// key latch of the script DLL).
    HeroControl {
        /// `PSFocus::GetFocusEntity(melee)`.
        melee_focus: Option<ActorId>,
        /// `PSFocus::GetFocusEntity()`.
        focus: Option<ActorId>,
        /// `PSCamera::IsInFirstPerson`.
        first_person: bool,
        /// A movement key is down.
        movement_key: bool,
    },
    /// The host plays the hero's animations and reports their moments back.
    HostDrivesHero(bool),
    /// The `Hit` clip of the hero's combat move reached the damage moment (`OnHit`).
    HeroMoveHit {
        /// Sequence number of the `HeroMove` event.
        seq: u32,
    },
    /// The hero's instruction (combat move or hold) ended.
    HeroExecDone {
        /// Sequence number of the event.
        seq: u32,
    },
    /// The host plays the clips of an NPC (its body is shown) and reports their moments back.
    HostDrivesNpc {
        actor: ActorId,
        on: bool,
    },
    /// The `Hit` clip of an NPC's combat move reached the damage moment (`OnHit`).
    NpcMoveHit {
        actor: ActorId,
        /// Sequence number of the `NpcMove` event.
        seq: u32,
    },
    /// The clip of an NPC's script instruction moved its body (root motion, in cm).
    NpcNudge {
        actor: ActorId,
        dx: f32,
        dz: f32,
    },
    /// The pose token `Pn` the clips of an NPC's last move ended in (`GetPrimaryPoseExt`).
    NpcPrimaryPose {
        actor: ActorId,
        pose: u8,
    },
    /// An NPC's instruction ended.
    NpcExecDone {
        actor: ActorId,
        /// Sequence number of the event.
        seq: u32,
    },
    /// The `Begin` clip of the hero's hold ended: the item changes slots.
    HeroHoldSwap {
        /// Sequence number of the `HeroHold` event.
        seq: u32,
    },
    /// Take stack `index` (or everything) of an opened container into the hero's inventory.
    TakeLoot {
        /// Raw script entity id of the container.
        object: u32,
        /// Stack to take, `None` for all.
        index: Option<usize>,
    },
    /// The host opened (true) or closed (false) a full-screen window that holds the world.
    GuiOpen(bool),
    // ---- magic and ranged (ported `PS_Magic*` / `PS_Ranged*`, see `gothic_script::magic`) ----
    /// Hold a spell item (rune, scroll, potion) in the right hand and enter the magic mode.
    SelectSpell {
        /// Item template (`It_Spell_Fireball`).
        template: String,
    },
    /// The cast key: quick cast, cast, power cast or summon, by the spell in hand.
    CastSpell {
        /// The focused character, `None` for a free cast.
        target: Option<ActorId>,
        /// A long press: a spell that can be charged is cast with all its mana.
        power: bool,
    },
    /// Let go of a cast in progress (the mana paid comes back).
    AbortCast,
    /// Put the bow or crossbow up (it must be equipped).
    ReadyRanged,
    /// Aim (zoom in, a bow starts drawing) or lower the weapon again.
    AimRanged {
        /// True to aim.
        aim: bool,
    },
    /// Release the string or pull the trigger at the focused character.
    ReleaseRanged {
        /// The focus target, `None` shoots along the aim.
        target: Option<ActorId>,
    },
    /// Offer a guard money to forget a crime (`_AI_Bribe`).
    Bribe {
        /// The guard.
        guard: ActorId,
        /// The crime he knows.
        crime: gothic_sim::script::enums::PlayerCrime,
        /// The dialog accepted the price.
        accepted: bool,
    },
    // ---- world content registration (host -> core, once at load) ---------------------------
    /// Register an interact object (chest, door, work station, bed, bench, ...); the core
    /// answers with [`Event::ObjectRegistered`] carrying the same `tag`.
    AddObject {
        /// Host-side correlation id.
        tag: u32,
        /// What it is used for (`UseType`).
        use_type: gothic_sim::script::enums::UseType,
        /// Position (centimetres, the world's space).
        pos: Pos,
        /// Lock, owner, loot, ...
        data: gothic_sim::script_player::ObjectData,
    },
    /// Register a loose item lying in the world; answered by [`Event::ItemRegistered`].
    AddItem {
        /// Host-side correlation id.
        tag: u32,
        /// Item template.
        template: String,
        /// Position.
        pos: Pos,
    },
    /// Register a synthesis recipe.
    AddRecipe {
        /// Recipe id.
        id: i32,
        /// The recipe.
        recipe: gothic_sim::script::player::api::Recipe,
    },
    /// Declare a recipe item (blueprint) of a skill category.
    AddRecipeItem {
        /// Item template.
        template: String,
        /// Category.
        category: gothic_sim::script::player::api::SkillCategory,
    },
    /// Register the spell templates (`spells.json` text).
    LoadSpells(String),
    /// Give an NPC its working / relaxing / sleeping points (script entity ids of registered
    /// objects); from then on the ported routine tasks run for it.
    SetRoutinePoints {
        /// The NPC.
        actor: ActorId,
        /// Working point.
        work: Option<u32>,
        /// Relaxing point.
        relax: Option<u32>,
        /// Sleeping point.
        sleep: Option<u32>,
    },
    /// Put an item into an NPC's hands or body (requirements are ignored for NPCs).
    EquipNpc {
        /// The NPC.
        actor: ActorId,
        /// Item template.
        template: String,
    },
    /// Add items to an NPC's inventory (treasure sets, plunder).
    GiveNpc {
        /// The NPC.
        actor: ActorId,
        /// Item template.
        template: String,
        /// Amount.
        amount: u32,
    },
    /// Give the hero a skill (debug and scripted tests; the game teaches skills through trainers).
    GrantPerk(String),
    /// Add items to the hero's inventory (debug and scripted tests; the game gives items
    /// through dialogs, plunder and pick-ups).
    GiveHero {
        /// Item template.
        template: String,
        /// Amount.
        amount: u32,
    },
    /// Write a save game (the slot kind follows the file name: `quick`, `auto_*`, else manual).
    Save(std::path::PathBuf),
    Load(std::path::PathBuf),
    /// Presentation state stored with the next saves (`region`, `camera`, ...) and handed back
    /// by [`Event::Loaded`] consumers through [`Core::host`].
    SetHost {
        key: String,
        value: String,
    },
    /// The world has a sector `name` with its initial flag (`gCSector`, from the sector list of
    /// the world file and the `Enabled` flag of the layer roots). Sectors that scripts switch
    /// without a registration are registered on the first switch.
    RegisterSector {
        name: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    /// An actor crossed the border of the entity processing range
    /// (`OnEnterProcessingRange` / `OnExitProcessingRange`).
    ProcessingRange {
        actor: ActorId,
        entered: bool,
    },
    /// A sector was switched (`SetSectorStatus`, a load): its layers exist (`enabled`) or are
    /// gone. The host spawns or hides the entities of its documents; see
    /// [`Core::sector_of_document`].
    SectorSwitched {
        sector: String,
        enabled: bool,
    },
    ActorSpawned {
        actor: ActorId,
        name: String,
    },
    DialogStarted {
        npc: ActorId,
        session: DialogSession,
    },
    LineSpoken {
        /// Display label (`Du` for the hero).
        speaker: String,
        key: String,
        text: Option<String>,
        player: bool,
        /// The NPC that speaks (None for the hero).
        actor: Option<ActorId>,
    },
    MenuChanged {
        session: DialogSession,
        menu: Vec<Choice>,
    },
    DialogEnded {
        npc: Option<ActorId>,
    },
    QuestChanged {
        quest: String,
        to: gothic_gameplay::model::QuestStatus,
    },
    QuestLogged {
        quest: String,
    },
    XpGained {
        amount: u32,
        level_before: u32,
        level_after: u32,
    },
    ItemMoved {
        item: String,
        amount: u32,
        gained: bool,
    },
    Notice(String),
    /// A simulation event (hit, kill, aggro, ...).
    Sim(EventKind),
    /// What `OnMovementModeChanged` made of a mode change of the hero's body.
    ModeChanged(gothic_sim::moves::ModeOutcome),
    /// A character says a stock voice line: play `SVM_<voice>_<key>`
    /// (`gothic_audio::PlayVoice::svm(voice, key)`).
    Voice {
        /// The speaker (the hero for the player's own lines).
        actor: ActorId,
        /// Sample id after the voice prefix, e.g. `Confirm_01`.
        key: String,
    },
    /// The player acted or was refused: windows to open, doors, locks, crafting, sleep.
    Player(gothic_sim::script::player::api::PlayerEvent),
    /// What the confirm key did.
    Confirmed(gothic_sim::script::player::normal::Confirm),
    /// Result of a `Craft` command.
    CraftResult(gothic_sim::script::player::craft::Synth),
    /// Result of a `UseItem` command.
    ItemUsed(gothic_sim::script::player::use_item::UseOutcome),
    /// Something magical happened: cast phases, effects to play, summons, status bits, teleports.
    Magic(gothic_sim::script::magic::api::MagicEvent),
    /// A projectile left its shooter (play the missile; the hit follows as `Sim(Hit)`).
    Projectile {
        /// Who shot.
        owner: ActorId,
        /// Whom it is aimed at.
        target: Option<ActorId>,
        /// Missile or ammunition template.
        template: String,
        /// A spell missile (otherwise an arrow or bolt).
        spell: bool,
    },
    /// The cast key started this cast state.
    CastStarted(gothic_sim::script::routine::state::State),
    /// A spell command could not run (no spell in hand, hero busy).
    CastRefused(gothic_sim::script::player::commands::CastError),
    /// What the guard did with a bribe.
    Bribed(gothic_sim::script::routine::dialog::BribeOutcome),
    /// An interact object was registered: its script entity id (for `Confirm` / `UseInteractObject`).
    ObjectRegistered {
        /// Tag of the `AddObject` command.
        tag: u32,
        /// Raw script entity id.
        entity: u32,
    },
    /// A loose item was registered.
    ItemRegistered {
        /// Tag of the `AddItem` command.
        tag: u32,
        /// Raw script entity id.
        entity: u32,
    },
    /// A character plays a use animation or pose queued by its script state (work station
    /// tasks, sitting, sleeping, stock-voice family).
    Anim {
        /// The character.
        actor: ActorId,
        /// The command as queued.
        command: gothic_sim::script::host::AiCommand,
        /// Use type of the character's destination point.
        use_type: gothic_sim::script::enums::UseType,
    },
    /// A save game was written or read.
    Saved(std::path::PathBuf),
    Loaded {
        hero: Pos,
        yaw: f32,
    },
    Failed(String),
}

struct ActiveDialog {
    npc: ActorId,
    name: String,
    session: DialogSession,
    menu: Vec<Choice>,
}

pub struct Core {
    pub sim: Sim<PlayWorld>,
    pub db: Arc<GameDatabase>,
    pub hero: ActorId,
    dialog: Option<ActiveDialog>,
    acc: f32,
    hero_next_strike_ms: u64,
    /// Bumped whenever `sim.state` may have changed (for cheap UI mirroring).
    pub state_rev: u64,
    /// What the hero's carried items looked like at the last [`Core::sync_inventory`].
    inv_mirror: gothic_gameplay::inventory::Inventory,
    /// Arm spawned fighters with a stand-in weapon of their species (tests and tools without
    /// the character's own inventory); the play mode gives real weapons from the treasure sets.
    pub guess_weapons: bool,
    /// The processing range (region of interest around the hero); `enabled = false` processes
    /// every actor regardless of distance.
    pub range: ProcessingRange,
    /// Host presentation state stored in save games (see [`Command::SetHost`]).
    pub host: std::collections::BTreeMap<String, String>,
    /// Trigger runtime (`eCTrigger_PS` family). The host registers the triggers of the world
    /// documents and feeds contacts; the state persists with the save (`world.triggers`).
    pub triggers: TriggerRuntime,
    /// Seconds of simulated time since the core was created: the clock triggers run on.
    pub clock: f64,
    /// Sector group each document-placed actor belongs to.
    actor_sector: std::collections::BTreeMap<ActorId, String>,
    /// Initial flag of each registered sector (the layer roots' `Enabled`); a save without an
    /// entry for a sector means "as authored".
    sector_defaults: std::collections::BTreeMap<String, bool>,
}

#[derive(Serialize, Deserialize)]
struct ActorSave {
    name: String,
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
    hp: i32,
    dead: bool,
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    state: String,
    tick: u64,
    hero: ActorSave,
    actors: Vec<ActorSave>,
}

const SAVE_VERSION: u32 = 1;

impl Core {
    pub fn new(db: Arc<GameDatabase>, items: ItemDb, mut state: GameState, seed: u64) -> Self {
        let cfg = SimConfig {
            seed,
            tick_ms: (1000.0 / TICK_HZ) as u32,
            ..SimConfig::default()
        };
        state.hours = state.hours.max(8);
        let mut sim = Sim::new(cfg, PlayWorld::default(), items)
            .with_database(db.clone())
            .with_state(state);
        let hero = sim.spawn_hero(Pos::default());
        sim.actor_mut(hero).action = Action::Attack;
        sim.drain_events();
        let mut core = Self {
            sim,
            db,
            hero,
            dialog: None,
            acc: 0.0,
            hero_next_strike_ms: 0,
            state_rev: 1,
            inv_mirror: Default::default(),
            guess_weapons: true,
            range: ProcessingRange::new(),
            host: Default::default(),
            triggers: TriggerRuntime::new(),
            clock: 0.0,
            actor_sector: Default::default(),
            sector_defaults: Default::default(),
        };
        // The new game's start kit lives in the quest/dialog state; the hero actor carries it.
        core.sync_inventory();
        core
    }

    /// Keep the hero's carried items in step: the sim actor (pick-ups, crafting, potions,
    /// ammunition) and the dialog / UI state (rewards, trades) are two views of one bag. Whichever
    /// side changed since the last call wins.
    pub fn sync_inventory(&mut self) {
        let hero = self.hero;
        if self.sim.actor(hero).inventory != self.inv_mirror {
            self.inv_mirror = self.sim.actor(hero).inventory.clone();
            self.sim.state.inventory = self.inv_mirror.clone();
            self.state_rev += 1;
        } else if self.sim.state.inventory != self.inv_mirror {
            self.inv_mirror = self.sim.state.inventory.clone();
            self.sim.actor_mut(hero).inventory = self.inv_mirror.clone();
            self.state_rev += 1;
        }
    }

    pub fn state(&self) -> &GameState {
        &self.sim.state
    }

    /// Put a weapon into the hero's hand regardless of requirements (the demo hero is not
    /// a fresh level-1 character).
    pub fn arm_hero(&mut self, item: &str) -> bool {
        let Some(def) = self.sim.items.get(item).cloned() else {
            return false;
        };
        self.sim.wear(self.hero, &def);
        true
    }

    /// Give the hero skills (`Perk_Bow_1`, ...) without paying learning points. The demo hero
    /// is not a fresh level-1 character (see [`Core::arm_hero`]): his start kit needs them.
    pub fn grant_perks(&mut self, perks: &[&str]) {
        let hero = self.hero;
        for p in perks {
            self.sim
                .actor_mut(hero)
                .perks
                .insert(p.to_ascii_lowercase());
        }
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn find_name(&self, name: &str) -> Option<ActorId> {
        self.sim.find(name)
    }

    /// Advance by `dt` seconds, running fixed 30 Hz steps; returns the events they produced.
    pub fn tick(&mut self, dt: f32, commands: &[Command]) -> Vec<Event> {
        let mut out = Vec::new();
        for c in commands {
            self.command(c, &mut out);
        }
        self.clock += f64::from(dt);
        // Writes to the game state since the last tick (a granted item) reach the hero's bag
        // before the step links its equipment; otherwise the bag would win the comparison.
        self.sync_inventory();
        self.sync_sectors(&mut out);
        self.update_processing_range(&mut out);
        self.acc += dt;
        let step = 1.0 / TICK_HZ;
        let mut n = 0;
        while self.acc >= step * 0.999 && n < 4 {
            self.acc = (self.acc - step).max(0.0);
            self.sim.step();
            n += 1;
        }
        self.sync_inventory();
        let evs = self.sim.drain_events();
        if !evs.is_empty() {
            self.state_rev += 1;
        }
        for e in evs {
            match &e.kind {
                EventKind::Xp {
                    amount,
                    level_before,
                    level_after,
                } => out.push(Event::XpGained {
                    amount: *amount as u32,
                    level_before: *level_before,
                    level_after: *level_after,
                }),
                EventKind::Quest { quest, to, .. } => out.push(Event::QuestChanged {
                    quest: quest.clone(),
                    to: *to,
                }),
                EventKind::Voice { actor, key } => out.push(Event::Voice {
                    actor: *actor,
                    key: key.clone(),
                }),
                EventKind::Anim {
                    actor,
                    command,
                    use_type,
                } => out.push(Event::Anim {
                    actor: *actor,
                    command: command.clone(),
                    use_type: *use_type,
                }),
                EventKind::Player(p) => out.push(Event::Player(p.clone())),
                EventKind::Magic(m) => out.push(Event::Magic(m.clone())),
                EventKind::Projectile {
                    owner,
                    target,
                    template,
                    spell,
                } => out.push(Event::Projectile {
                    owner: *owner,
                    target: *target,
                    template: template.clone(),
                    spell: *spell,
                }),
                _ => out.push(Event::Sim(e.kind)),
            }
        }
        out
    }

    /// The sector (layer group) a world document belongs to, if the world registered one.
    pub fn sector_of_document(&self, document: &str) -> Option<String> {
        self.range
            .scene()
            .sectors
            .sector_of_document(document)
            .map(|s| s.name.clone())
    }

    /// Is the sector of a document enabled (unknown sectors always are)?
    pub fn document_enabled(&self, document: &str) -> bool {
        self.range.scene().sectors.document_enabled(document)
    }

    /// Apply the sector flags `SetSectorStatus` wrote into the game state
    /// (`gCSector::Enable` through `gCSectorPersistence_PS::UpdateSector`): the layers' roots
    /// are switched, the actors placed by their documents follow, and the host hears about each
    /// change. This is also how a loaded save restores the sectors
    /// (`gCSectorPersistence_PS::UpdateSectors`).
    fn sync_sectors(&mut self, out: &mut Vec<Event>) {
        // Sector names compare case-insensitively (the registry does); the script's
        // `SetSectorStatus` spelling and the world's directory name must be one entry.
        let mut wanted: std::collections::BTreeMap<String, bool> = self
            .sector_defaults
            .iter()
            .map(|(n, on)| (n.to_ascii_lowercase(), *on))
            .collect();
        wanted.extend(
            self.sim
                .state
                .sectors
                .iter()
                .map(|(n, on)| (n.to_ascii_lowercase(), *on)),
        );
        let scene = self.range.scene_mut();
        let mut changed = false;
        let mut saved = Vec::new();
        for (name, on) in wanted {
            let known = scene.sectors.sector(&name).map(|s| s.enabled);
            if known == Some(on) {
                continue;
            }
            if known.is_none() {
                scene.sectors.register(&name, on);
            }
            saved.push(gothic_world::sector::SectorSave { name, enabled: on });
        }
        // `gCSector::Enable` (Game:0x10192cd0) for what `SetSectorStatus` wrote; a loaded save
        // goes through `gCSectorPersistence_PS::UpdateSectors` in `restore`.
        for s in &saved {
            let Some(sw) = scene.sectors.enable(&mut scene.tree, &s.name, s.enabled) else {
                continue;
            };
            let shown = scene
                .sectors
                .sector(&s.name)
                .map_or_else(|| s.name.clone(), |x| x.name.clone());
            let _ = sw;
            out.push(Event::SectorSwitched {
                sector: shown,
                enabled: s.enabled,
            });
            changed = true;
        }
        if changed {
            let scene = self.range.scene();
            let actors: Vec<(ActorId, bool)> = self
                .actor_sector
                .iter()
                .map(|(a, s)| (*a, scene.sectors.is_enabled(s)))
                .collect();
            for (a, on) in actors {
                self.range.set_actor_enabled(a, on);
            }
        }
    }

    /// Port of the range part of `eCEntityAdmin::Process`: the hero (the camera stand-in) centres
    /// the region of interest; actors outside it are not thought for.
    fn update_processing_range(&mut self, out: &mut Vec<Event>) {
        gothic_portcov::hit!("update_processing_range");
        if !self.range.enabled {
            // Everything is processed except the actors of switched-off sector layers.
            let sectors = &self.range.scene().sectors;
            let off: Vec<ActorId> = self
                .actor_sector
                .iter()
                .filter(|(_, s)| !sectors.is_enabled(s))
                .map(|(a, _)| *a)
                .collect();
            self.sim.processing = (!off.is_empty()).then(|| {
                self.sim
                    .actors
                    .iter()
                    .map(|a| a.id)
                    .filter(|a| !off.contains(a))
                    .collect()
            });
            return;
        }
        let centre = self
            .sim
            .world
            .actors
            .get(&self.hero)
            .map_or(bevy::math::Vec3::ZERO, |(p, _)| {
                bevy::math::Vec3::new(p.x, p.y, p.z)
            });
        let actors = &self.sim.actors;
        let world = &self.sim.world;
        let view = actors.iter().filter_map(|a| {
            let (p, _) = world.actors.get(&a.id)?;
            Some((
                a.id,
                a.name.as_str(),
                bevy::math::Vec3::new(p.x, p.y, p.z),
                a.ai_mode == AiMode::Dead,
            ))
        });
        let (events, mut inside) = self.range.update(centre, view);
        // Actors of a switched-off sector layer are not part of the world: never processed.
        let sectors = &self.range.scene().sectors;
        inside.retain(|a| {
            self.actor_sector
                .get(a)
                .is_none_or(|s| sectors.is_enabled(s))
        });
        for e in events {
            out.push(Event::ProcessingRange {
                actor: e.actor,
                entered: e.entered,
            });
        }
        self.sim.processing = Some(inside);
    }

    fn command(&mut self, c: &Command, out: &mut Vec<Event>) {
        match c {
            Command::RegisterSector { name, enabled } => {
                self.range.scene_mut().sectors.register(name, *enabled);
                self.sector_defaults.insert(name.clone(), *enabled);
            }
            Command::SpawnNpc { record, at, yaw } => {
                let id = self.sim.spawn_record(record, *at);
                let sector = self.sector_of_document(&record.document);
                if let Some(s) = sector {
                    self.actor_sector.insert(id, s);
                }
                if let Some(w) = self.sim.world.actors.get_mut(&id) {
                    w.1 = *yaw;
                }
                let species = self.sim.actor(id).species;
                if self.guess_weapons
                    && species.can_attack()
                    && let Some(w) = gothic_sim::data::default_weapon(&self.sim.items, species)
                {
                    self.sim.wear(id, &w);
                }
                out.push(Event::ActorSpawned {
                    actor: id,
                    name: record.name.clone(),
                });
            }
            Command::HeroTransform { pos, yaw } => {
                self.sim.world.actors.insert(self.hero, (*pos, *yaw));
            }
            Command::Talk { npc } => self.start_dialog(*npc, out),
            Command::Choose { index } => self.choose(*index, out),
            Command::EndDialog => self.end_dialog(out),
            Command::Attack { target } => self.hero_attack(*target),
            Command::SetDrawn(d) => self.sim.actor_mut(self.hero).drawn = *d,
            Command::Confirm { focus } => {
                // `HasInfo`: is there anything to say to the focused character?
                self.sim.dialog_info = gothic_sim::script::host::EntityId(*focus)
                    .0
                    .checked_sub(1)
                    .map(ActorId)
                    .filter(|a| (a.0 as usize) < self.sim.actors.len())
                    .map(|a| (a, self.has_dialog_info(a)));
                if let Some(c) = self.sim.player_confirm(EntityId(*focus)) {
                    out.push(Event::Confirmed(c));
                }
            }
            Command::UseInteractObject { object } => {
                self.sim.player_use_object(EntityId(*object));
            }
            Command::EndInteraction => self.sim.player_end_interaction(),
            Command::PickUp { item } => self.sim.player_pick_up(EntityId(*item)),
            Command::Lockpick { object, success } => {
                self.sim.player_lockpick(EntityId(*object), *success);
            }
            Command::Craft { recipe } => {
                if let Some(r) = self.sim.player_craft(*recipe) {
                    out.push(Event::CraftResult(r));
                }
            }
            Command::UseItem { template } => {
                if let Some(r) = self.sim.player_use_item(template) {
                    out.push(Event::ItemUsed(r));
                }
            }
            Command::QuickLoot { body } => {
                self.sim.player_quick_loot(EntityId(*body));
            }
            Command::Sleep { hour } => {
                self.sim.player_sleep(*hour);
                self.state_rev += 1;
            }
            Command::CompanionCommand { attack } => self.sim.player_companion_command(*attack),
            Command::ToggleSneak => self.sim.player_toggle_sneak(),
            Command::Jump => self.sim.player_jump(),
            Command::HeroModeChanged {
                last,
                mode,
                state_secs,
            } => {
                let hero = self.hero;
                let outcome = self
                    .sim
                    .movement_mode_changed(hero, *last, *mode, *state_secs);
                out.push(Event::ModeChanged(outcome));
            }
            Command::HeroMovement(m) => self.sim.hero_move = *m,
            Command::Dodge { direction } => {
                self.sim.player_dodge(*direction);
            }
            Command::PlayerAction { id, focus } => self.sim.player_push_action(*id, *focus),
            Command::PlayerEnterState { name } => {
                if let Some(state) = gothic_sim::script::routine::state::State::from_name(name) {
                    self.sim.player_enter_state(state);
                }
            }
            Command::SelectWeapon => {
                self.sim.player_select_weapon();
            }
            Command::HeroControl {
                melee_focus,
                focus,
                first_person,
                movement_key,
            } => self
                .sim
                .set_hero_control(*melee_focus, *focus, *first_person, *movement_key),
            Command::HostDrivesHero(on) => self.sim.script.player.host_drives = *on,
            Command::HeroMoveHit { seq } => self.sim.hero_move_hit(*seq),
            Command::HeroExecDone { seq } => self.sim.hero_exec_done(*seq),
            Command::HeroHoldSwap { seq } => self.sim.hero_hold_swap(*seq),
            Command::HostDrivesNpc { actor, on } => self.sim.set_host_drives_npc(*actor, *on),
            Command::NpcPrimaryPose { actor, pose } => {
                // The script counts poses from 1 (`P0` of the clip names is pose 1).
                self.sim.script.npc_mut(*actor).primary_pose = u32::from(*pose) + 1
            }
            Command::NpcNudge { actor, dx, dz } => self.sim.nudge_npc(*actor, *dx, *dz),
            Command::NpcMoveHit { actor, seq } => self.sim.npc_move_hit(*actor, *seq),
            Command::NpcExecDone { actor, seq } => self.sim.npc_exec_done(*actor, *seq),
            Command::TakeLoot { object, index } => {
                let taken = self.sim.script.take_loot(EntityId(*object), *index);
                for (item, n) in taken {
                    self.sim.state.add_item(&item.to_ascii_lowercase(), n);
                    out.push(Event::ItemMoved {
                        item,
                        amount: n,
                        gained: true,
                    });
                }
                self.sync_inventory();
                self.state_rev += 1;
            }
            Command::GuiOpen(open) => self.sim.gui_open = *open || self.dialog.is_some(),
            Command::SelectSpell { template } => {
                if !self.sim.player_select_spell(template) {
                    out.push(Event::Notice(format!("cannot select {template}")));
                }
            }
            Command::CastSpell { target, power } => match self.sim.player_cast(*target, *power) {
                Ok(state) => out.push(Event::CastStarted(state)),
                Err(e) => out.push(Event::CastRefused(e)),
            },
            Command::AbortCast => self.sim.player_abort_cast(),
            Command::ReadyRanged => {
                if !self.sim.player_ready_ranged() {
                    out.push(Event::Notice("no bow or crossbow in hand".into()));
                }
            }
            Command::AimRanged { aim } => self.sim.player_aim(*aim),
            Command::ReleaseRanged { target } => {
                self.sim.player_release(*target);
            }
            Command::Bribe {
                guard,
                crime,
                accepted,
            } => {
                if let Some(r) = self.sim.player_bribe(*guard, *crime, *accepted) {
                    out.push(Event::Bribed(r));
                }
            }
            Command::AddObject {
                tag,
                use_type,
                pos,
                data,
            } => {
                let id = self.sim.script.add_object(*use_type, *pos, data.clone());
                out.push(Event::ObjectRegistered {
                    tag: *tag,
                    entity: id.0,
                });
            }
            Command::AddItem { tag, template, pos } => {
                let id = self.sim.script.add_item(template, *pos);
                out.push(Event::ItemRegistered {
                    tag: *tag,
                    entity: id.0,
                });
            }
            Command::AddRecipe { id, recipe } => self.sim.script.add_recipe(*id, recipe.clone()),
            Command::AddRecipeItem { template, category } => {
                self.sim.script.add_recipe_item(template, *category);
            }
            Command::LoadSpells(json) => {
                if let Err(e) = self.sim.load_spells(json) {
                    out.push(Event::Failed(format!("spells: {e}")));
                }
            }
            Command::SetRoutinePoints {
                actor,
                work,
                relax,
                sleep,
            } => {
                use gothic_sim::script::routine::time::RoutineTime as Rt;
                self.sim.script_routines = true;
                for (time, point) in [(Rt::Work, work), (Rt::Relax, relax), (Rt::Sleep, sleep)] {
                    if let Some(p) = point {
                        self.sim
                            .script
                            .set_routine_point(*actor, time, EntityId(*p));
                    }
                }
            }
            Command::EquipNpc { actor, template } => {
                if let Some(def) = self.sim.items.get(template).cloned() {
                    self.sim.wear(*actor, &def);
                }
            }
            Command::GiveNpc {
                actor,
                template,
                amount,
            } => {
                let info = self.sim.items.info(template);
                self.sim.actor_mut(*actor).inventory.add_items(
                    template,
                    info,
                    (*amount).min(i32::MAX as u32) as i32,
                    0,
                    gothic_gameplay::inventory::StackType::Normal,
                );
            }
            Command::GrantPerk(p) => {
                self.grant_perks(&[p.as_str()]);
            }
            Command::GiveHero { template, amount } => {
                // Into the hero's own bag with the item's rule data (a stack without it cannot be
                // worn or held), the game state mirrors it.
                let info = self.sim.items.info(template);
                self.sim.actor_mut(self.hero).inventory.add_items(
                    template,
                    info,
                    (*amount).min(i32::MAX as u32) as i32,
                    0,
                    gothic_gameplay::inventory::StackType::Normal,
                );
                self.sync_inventory();
                self.state_rev += 1;
            }
            Command::Save(p) => {
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("save");
                let kind = if stem == "quick" {
                    crate::save::SlotKind::Quick
                } else if stem.starts_with("auto") {
                    crate::save::SlotKind::Auto
                } else {
                    crate::save::SlotKind::Manual
                };
                let meta = crate::save::SaveMeta {
                    name: stem.to_string(),
                    kind,
                };
                match self.save_file(p, &meta) {
                    Ok(()) => out.push(Event::Saved(p.clone())),
                    Err(e) => out.push(Event::Failed(format!("save: {e:#}"))),
                }
            }
            Command::Load(p) => match self.load_file(p) {
                Ok(_) => {
                    out.push(Event::Saved(p.clone()));
                    out.push(self.loaded_event());
                }
                Err(e) => out.push(Event::Failed(format!("load: {e:#}"))),
            },
            Command::SetHost { key, value } => {
                self.host.insert(key.clone(), value.clone());
            }
        }
    }

    fn hero_attack(&mut self, target: ActorId) {
        let now = self.sim.now_ms();
        if now < self.hero_next_strike_ms || !self.sim.actor(target).is_alive() {
            return;
        }
        let hero = self.hero;
        if self.sim.dist(hero, target) > HERO_REACH {
            return;
        }
        self.hero_next_strike_ms = now + HERO_STRIKE_GAP_MS;
        self.sim.actor_mut(hero).drawn = true;
        let at = self.sim.world.position(target).unwrap_or_default();
        self.sim.world.face_towards(hero, at);
        self.sim.melee_hit(hero, target);
        self.state_rev += 1;
    }

    // ---- dialog ---------------------------------------------------------------------

    /// `HasInfo`: does the dialog data have something to say to the hero for `npc` right now?
    pub fn has_dialog_info(&mut self, npc: ActorId) -> bool {
        let name = self.sim.actor(npc).name.clone();
        let db = self.db.clone();
        let eng = Engine::with(&db, Assumptions::PERMISSIVE);
        self.sim.state.triggers = self.sim.dialog_triggers(npc);
        // The player's generic root infos (goodbye, ...) are offered to everybody; only the
        // character's own infos and the partner-dependent ones (trade, teach, party, ...) count.
        !eng.own_root_available(&self.sim.state, &name).is_empty()
            || eng
                .root_list(&self.sim.state, &name)
                .iter()
                .any(|&i| eng.hero_info_depends_on_partner(i))
    }

    fn start_dialog(&mut self, npc: ActorId, out: &mut Vec<Event>) {
        if self.dialog.is_some() {
            return;
        }
        let name = self.sim.actor(npc).name.clone();
        self.sim.dialog_partner = Some(npc);
        self.sim.gui_open = true;
        let db = self.db.clone();
        let eng = Engine::with(&db, Assumptions::PERMISSIVE);
        self.sim.state.triggers = self.sim.dialog_triggers(npc);
        let (session, r) = eng.start(&mut self.sim.state, &name);
        out.push(Event::DialogStarted {
            npc,
            session: session.clone(),
        });
        self.dialog = Some(ActiveDialog {
            npc,
            name,
            session,
            menu: Vec::new(),
        });
        self.absorb(&r, out);
        self.refresh_menu(out);
    }

    fn choose(&mut self, index: usize, out: &mut Vec<Event>) {
        let db = self.db.clone();
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        let Some(choice) = d.menu.get(index).cloned() else {
            return;
        };
        let eng = Engine::with(&db, Assumptions::PERMISSIVE);
        // The kind of party move the info stands for (`JoinPlayerParty` / `JoinPlayerMob` /
        // `JoinPlayerSlavery`, `DismissPlayerMob`, `WaitPlayerParty`).
        let cond = choice
            .info
            .map(|id| gothic_gameplay::model::InfoCond::from_raw(db.info(id).condition_type));
        use gothic_gameplay::model::InfoCond as C;
        use gothic_sim::script::enums::PartyMemberType as K;
        self.sim.join_kind = match cond {
            Some(C::MobJoin | C::MobDismiss) => Some(K::Mob),
            Some(C::SlaveJoin) => Some(K::Slave),
            _ => None,
        };
        let npc_id = d.npc;
        let r = eng.choose(&mut self.sim.state, &mut d.session, &choice);
        self.sim.join_kind = None;
        if cond == Some(C::Wait) {
            self.sim.wait_party(npc_id);
        }
        let finished = r.ended || d.session.ended;
        self.absorb(&r, out);
        if finished {
            self.end_dialog(out);
        } else {
            self.refresh_menu(out);
        }
    }

    fn refresh_menu(&mut self, out: &mut Vec<Event>) {
        let db = self.db.clone();
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        let eng = Engine::with(&db, Assumptions::PERMISSIVE);
        d.menu = eng.menu(&self.sim.state, &d.session);
        out.push(Event::MenuChanged {
            session: d.session.clone(),
            menu: d.menu.clone(),
        });
    }

    fn end_dialog(&mut self, out: &mut Vec<Event>) {
        if let Some(d) = self.dialog.take() {
            self.sim.dialog_partner = None;
            self.sim.gui_open = false;
            out.push(Event::DialogEnded { npc: Some(d.npc) });
            self.state_rev += 1;
        }
    }

    fn absorb(&mut self, r: &ExecResult, out: &mut Vec<Event>) {
        let (npc, npc_name) = self
            .dialog
            .as_ref()
            .map_or((None, String::new()), |d| (Some(d.npc), d.name.clone()));
        for o in &r.outputs {
            match o {
                Output::Say {
                    speaker, key, text, ..
                } => {
                    let player = speaker == "player";
                    let own = speaker.is_empty() || speaker.eq_ignore_ascii_case(&npc_name);
                    let label = if player {
                        "Du".to_string()
                    } else if own {
                        npc_name.replace('_', " ")
                    } else {
                        speaker.replace('_', " ")
                    };
                    let actor = if player {
                        None
                    } else if own {
                        npc
                    } else {
                        self.sim.find(speaker).or(npc)
                    };
                    out.push(Event::LineSpoken {
                        speaker: label,
                        key: key.clone(),
                        text: text.clone(),
                        player,
                        actor,
                    });
                }
                Output::Presentation { text: Some(t), .. } => out.push(Event::LineSpoken {
                    speaker: String::new(),
                    key: String::new(),
                    text: Some(t.clone()),
                    player: false,
                    actor: None,
                }),
                Output::QuestChanged { quest, to, .. } => out.push(Event::QuestChanged {
                    quest: quest.clone(),
                    to: *to,
                }),
                Output::QuestLog { quest, .. } => out.push(Event::QuestLogged {
                    quest: quest.clone(),
                }),
                Output::Xp {
                    amount,
                    level_before,
                    level_after,
                } => out.push(Event::XpGained {
                    amount: *amount,
                    level_before: *level_before,
                    level_after: *level_after,
                }),
                Output::Item {
                    from,
                    to,
                    item,
                    amount,
                } => {
                    if to == "player" || from == "player" {
                        out.push(Event::ItemMoved {
                            item: item.clone(),
                            amount: *amount,
                            gained: to == "player",
                        });
                    }
                }
                Output::Voice { command, speaker } => {
                    let who = if speaker.is_empty() || speaker.eq_ignore_ascii_case(&npc_name) {
                        npc
                    } else {
                        self.sim.find(speaker).or(npc)
                    };
                    if let Some(w) = who {
                        self.sim.dialog_voice(command, w);
                    }
                }
                Output::Message { text } => out.push(Event::Notice(text.clone())),
                _ => {}
            }
        }
        if let Some(why) = &r.refused {
            out.push(Event::Notice(format!("Nicht möglich: {why}")));
        }
        // Forwarded effects (attack, spawn, ...) are carried out by the sim; its XP / quest
        // mirror events were already reported from the outputs above.
        self.sim.apply_outputs(&r.outputs);
        self.sim
            .events
            .retain(|e| !matches!(e.kind, EventKind::Xp { .. } | EventKind::Quest { .. }));
        self.state_rev += 1;
    }

    // ---- save / load -------------------------------------------------------------------

    fn actor_save(&self, a: &Actor) -> ActorSave {
        let (p, yaw) = self
            .sim
            .world
            .actors
            .get(&a.id)
            .copied()
            .unwrap_or_default();
        ActorSave {
            name: a.name.clone(),
            x: p.x,
            y: p.y,
            z: p.z,
            yaw,
            hp: a.hp,
            dead: a.ai_mode == AiMode::Dead,
        }
    }

    /// Serialise the whole core state (versioned JSON).
    pub fn save_json(&self) -> Result<String> {
        let hero = self.actor_save(self.sim.actor(self.hero));
        let actors = self
            .sim
            .actors
            .iter()
            .filter(|a| a.id != self.hero)
            .map(|a| self.actor_save(a))
            .collect();
        let file = SaveFile {
            version: SAVE_VERSION,
            state: self.sim.state.to_json()?,
            tick: self.sim.tick,
            hero,
            actors,
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    /// Reset everything derived or transient after a save was applied.
    pub(crate) fn after_restore(&mut self) {
        self.dialog = None;
        self.hero_next_strike_ms = 0;
        // The hero actor's bag is the authority of a save; the dialog state mirrors it.
        self.inv_mirror = self.sim.actor(self.hero).inventory.clone();
        self.sim.state.inventory = self.inv_mirror.clone();
        self.state_rev += 1;
    }

    /// The event that tells the host where the hero stands after a load.
    pub fn loaded_event(&self) -> Event {
        let hero = self
            .sim
            .world
            .actors
            .get(&self.hero)
            .copied()
            .unwrap_or_default();
        Event::Loaded {
            hero: hero.0,
            yaw: hero.1,
        }
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.save_json()?).with_context(|| path.display().to_string())
    }

    pub fn load_from(&mut self, path: &std::path::Path) -> Result<Event> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        self.load_json(&text)
    }

    pub fn load_json(&mut self, text: &str) -> Result<Event> {
        let file: SaveFile = serde_json::from_str(text).context("save file")?;
        anyhow::ensure!(file.version == SAVE_VERSION, "unsupported save version");
        let state = GameState::from_json(&file.state)?;
        self.sim.clock = gothic_sim::routine::GameClock {
            game_ms: state.hours * 3_600_000,
        };
        self.sim.state = state;
        self.sim.tick = file.tick;
        self.dialog = None;
        self.sim.dialog_partner = None;
        self.sim.gui_open = false;
        let hero = self.hero;
        // Actors are matched by spawn order (names are not unique).
        let ids: Vec<ActorId> = self
            .sim
            .actors
            .iter()
            .map(|a| a.id)
            .filter(|i| *i != hero)
            .collect();
        let pairs = std::iter::once((hero, &file.hero)).chain(ids.into_iter().zip(&file.actors));
        for (id, s) in pairs {
            self.sim
                .world
                .actors
                .insert(id, (Pos::new(s.x, s.y, s.z), s.yaw));
            let a = self.sim.actor_mut(id);
            a.hp = s.hp;
            if s.dead && a.ai_mode != AiMode::Dead {
                a.ai_mode = AiMode::Dead;
                a.brain.task = Task::Dead;
            } else if !s.dead && a.ai_mode == AiMode::Dead {
                a.ai_mode = AiMode::Routine;
                a.brain.task = Task::Routine;
            }
        }
        self.state_rev += 1;
        Ok(Event::Loaded {
            hero: Pos::new(file.hero.x, file.hero.y, file.hero.z),
            yaw: file.hero.yaw,
        })
    }
}
