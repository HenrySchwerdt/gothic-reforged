//! `gothic-reforged --play [--smoke] [--record DIR]`: the first playable slice.
//!
//! Reuses the `--walk` world (Ardea terrain, props, collision, streaming) and replaces the
//! capsule with the animated hero. All rules live in `gothic-runtime`'s core (quests, dialog,
//! combat, AI); this module only turns input into [`Command`]s and core [`Event`]s into
//! animation, audio and UI.
//!
//! Controls: the original key bindings, see [`controls`] (WASD / QE, Shift, Ctrl, Alt, Space,
//! 1..0 quick slots, mouse attacks and parry, L I C M B, F5 F6 F9 F10); developer keys of this
//! port: F fly, V camera. The original shows no help overlay and neither does this mode.
mod anim;
mod audio;
mod barks;
mod cam;
mod controls;
mod data;
mod events;
mod fx;
mod hero;
mod hud;
mod interact;
mod kit;
mod nav;
mod npcs;
mod objects;
mod pose;
mod record;
mod routines;
mod script;
mod windows;

use crate::walk::{Player, Smoke};
use anyhow::Result;
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, AnimationPlugin};
use gothic_audio::{AudioSettings, GothicAudioPlugin};
use gothic_gameplay::GameDatabase;
use gothic_runtime::{CorePlugin, CoreRes};
use gothic_sim::data::NpcFile;
use gothic_sim::item::ItemDb;
use gothic_sim::types::ActorId;
use gothic_ui::{GameSession, GothicUiPlugin, ItemCatalog, UiConfig};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// Marks the animated hero entity.
#[derive(Component)]
pub struct HeroActor;

/// Binds an entity to a simulation actor.
#[derive(Component, Clone, Copy)]
pub struct NpcActor(pub ActorId);

pub struct PendingNpc {
    pub record: gothic_sim::data::NpcRecord,
    /// Mirrored, origin-relative centimetres (the space the world is built in).
    pub pos_cm: Vec3,
    /// Appearance data (visual body parts + equipped item visuals) of this entity.
    pub look: Option<gothic_formats::props::records::NpcLook>,
}

#[derive(Resource, Default)]
pub struct Roster {
    pub pending: Vec<PendingNpc>,
    /// Entities waiting for their `ActorSpawned` event (FIFO, same order as the commands).
    pub binds: VecDeque<Entity>,
    pub by_actor: HashMap<ActorId, Entity>,
    pub total: usize,
    pub spawned: usize,
    /// Script name -> display name.
    pub display: HashMap<String, String>,
}

/// Per-NPC data the presentation needs besides the look: stock-voice class and routine points.
#[derive(Component, Clone, Default)]
pub struct NpcExtra {
    /// `gCNPC_PS.Voice` (`Hum_Fat_Weak`), the `SVM_<voice>_<event>` class.
    pub voice: String,
    /// Working / relaxing / sleeping point ids (world entity ids).
    pub work: Option<String>,
    pub relax: Option<String>,
    pub sleep: Option<String>,
}

/// What the interaction focus points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A living character (talk).
    Npc(ActorId),
    /// A dead or knocked-out character (loot).
    Body(ActorId),
    /// An interact object, index into [`objects::WorldObjects`].
    Object(usize),
    /// A loose item, index into [`objects::WorldObjects`].
    Item(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Explore,
    Dialog,
}

#[derive(Resource, Default)]
pub struct PlayState {
    pub mode: Mode,
    /// NPC in front of the camera within talking range.
    pub focus: Option<ActorId>,
    pub dialog_npc: Option<ActorId>,
    pub hero_down: bool,
    pub drawn: bool,
    pub hero_yaw: f32,
    pub last_hero_cm: Option<Vec3>,
    pub hero_entity: Option<Entity>,
    pub relock_cursor: bool,
    /// Cutscene camera: (position, look-at) in metres.
    pub cinematic: Option<(Vec3, Vec3)>,
    /// Target of the current fight (hero faces it) and when it was last attacked.
    pub combat_focus: Option<(ActorId, f32)>,
    /// Smoothed dialog camera position (metres).
    pub dialog_cam: Option<Vec3>,
    /// The focused thing (object, item, body or character).
    pub target: Option<Target>,
    /// The hero sneaks (`AniState::Sneak` of the ported state).
    pub sneaking: bool,
    /// The quick bar: template per slot 1..10.
    pub quick: Vec<Option<String>>,
}

/// A key press the smoke script injects (the same code path as the keyboard).
#[derive(Clone, Debug)]
pub enum ScriptAct {
    Confirm,
    /// Quick slot, 0-based.
    Quick(usize),
    DrawWeapon,
    Sneak,
    Jump,
    /// Left mouse button held for this many seconds, then released.
    LeftClick(f32),
    /// Right mouse button down (true) or up (false).
    Parry(bool),
    /// Digit key `n` (1-based) of an open window (take / craft entry).
    WindowKey(usize),
    /// Enter in an open window (take all / first entry).
    WindowEnter,
    /// Esc in an open window.
    WindowClose,
    /// Leave a seat / pose.
    EndInteraction,
}

/// Input injected by the smoke script (merged with the keyboard in `hero::input`).
#[derive(Resource, Default)]
pub struct ScriptInput {
    pub active: bool,
    /// Desired move direction in world space (mirrored cm space, y ignored).
    pub wish: Vec3,
    pub talk: bool,
    pub attack: bool,
    pub draw: Option<bool>,
    pub choose: Option<usize>,
    pub end_dialog: bool,
    pub acts: std::collections::VecDeque<ScriptAct>,
}

pub fn requested(args: &[String]) -> bool {
    args.iter().any(|a| a == "--play")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .cloned()
}

/// `PC_Hero` world position of the original start (Gothic space, cm) as mirrored,
/// origin-relative centimetres (see `world_loader::Origin::point`).
pub const HERO_START: Vec3 = Vec3::new(92_400.0 - 87_984.94, 5_145.56, -10_197.48 + 11_000.0);

/// Mesh of the hero's start weapon (from the item template), used for the held sword.
#[derive(Resource)]
pub struct HeroKit {
    pub sword_mesh: Option<String>,
    /// `PC_Hero` appearance from the NPC database.
    pub look: Option<gothic_formats::props::records::NpcLook>,
}

/// New-game character: `PC_Hero` only wears Head_Player / Body_Player; the start kit is granted
/// by the script function `InventoryAssureStartupItems` (PS_Inventory.cpp, Script_Game.dll
/// 0x100784a0): quick slot 1 `It_1H_OrcSlayer_01`, 2 `It_Torch_Burning`, 3 `It_Bow_Hunting`,
/// 4 `Arrow` x50, 5 `It_Potion_Health` x10 and 123 `It_Gold`.
fn new_game_state() -> gothic_gameplay::GameState {
    let mut st = gothic_gameplay::GameState::new();
    for (item, n, _slot) in kit::START_KIT {
        st.add_item(if *item == "It_Gold" { "it_gold" } else { item }, *n);
    }
    st
}

/// Mesh name (without extension) of an item template from the item export.
fn item_mesh(items_json: &std::path::Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(items_json).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.as_array()?
        .iter()
        .find(|i| i.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("mesh")?
        .as_str()
        .map(|m| m.trim_end_matches(".xcmsh").to_string())
}

/// The quest / dialog database: from the cooked cache when one exists (profile `desktop-high`,
/// German texts), otherwise parsed from the installation.
fn load_database(data_dir: &std::path::Path) -> Result<(GameDatabase, &'static str)> {
    if std::env::var_os("GR_NO_CACHE").is_none() {
        match gothic_cook::Cache::open_default("desktop-high").and_then(|c| c.load_db()) {
            Ok(mut db) if db.strings.language_index("German").is_some() => {
                // The cooked table carries every language; only the selection differs.
                db.language = "German".to_string();
                return Ok((db, "cooked cache"));
            }
            Ok(_) => eprintln!("play: cooked database has no German texts"),
            Err(e) => eprintln!("play: no usable cooked database: {e:#}"),
        }
    }
    let db = GameDatabase::load_install(data_dir, "German")
        .map_err(|e| anyhow::anyhow!("loading the quest / dialog database: {e:#}"))?;
    Ok((db, "installation (no usable cache)"))
}

/// Called from `walk::run` after the walk app is assembled.
pub fn install(app: &mut App, args: &[String]) -> Result<()> {
    let data_dir = PathBuf::from(
        flag(args, "--data-dir")
            .unwrap_or_else(|| r"D:\SteamLibrary\steamapps\common\Gothic 3\Data".to_string()),
    );
    let install_dir = data_dir.parent().map(PathBuf::from).unwrap_or_default();
    let smoke = args.iter().any(|a| a == "--smoke");
    let record = flag(args, "--record").map(PathBuf::from);
    let started = std::time::Instant::now();

    let t_db = std::time::Instant::now();
    let (db, db_source) = load_database(&data_dir)?;
    eprintln!(
        "play: game database from {db_source} in {} ms",
        t_db.elapsed().as_millis()
    );
    let db = std::sync::Arc::new(db);
    let items_json = PathBuf::from(".local/items.json");
    let items = ItemDb::load(&items_json).map_err(|e| {
        anyhow::anyhow!("item database (run `gothic-inspector item-dump` first): {e:#}")
    })?;
    let catalog = ItemCatalog::load(&items_json)?;
    let npcs = NpcFile::load(std::path::Path::new(".local/npcs.json")).map_err(|e| {
        anyhow::anyhow!("NPC database (run `gothic-inspector npc-dump` first): {e:#}")
    })?;
    let library = ActorLibrary::open(&data_dir)
        .map_err(|e| anyhow::anyhow!("opening actor archives: {e:#}"))?;
    let audio = GothicAudioPlugin::open(&data_dir, "German")
        .map_err(|e| anyhow::anyhow!("opening audio: {e:#}"))?;
    eprintln!("play: data loaded in {} ms", started.elapsed().as_millis());

    let state = new_game_state();
    let mut core = gothic_runtime::Core::new(db.clone(), items, state.clone(), 0x6074_1C20);
    core.guess_weapons = false;
    // The start kit's sword and bow need these skills (and STR 100 / DEX 80, which the new
    // game's attributes meet).
    core.grant_perks(&["Perk_1H_1", "Perk_Bow_1"]);
    if !core.arm_hero("It_1H_OrcSlayer_01") {
        eprintln!("play: It_1H_OrcSlayer_01 missing from the item database");
    }
    let sword_mesh = item_mesh(&items_json, "It_1H_OrcSlayer_01")
        .filter(|m| !library.static_mesh_names(m).is_empty());
    let session = GameSession::new(db, state, catalog);

    let looks = npcs::load_looks(std::path::Path::new(".local/npcs.json"))?;
    let hero_look = looks.pc_hero();
    let roster = npcs::select_roster(&npcs, &looks);
    eprintln!("play: {} NPCs near Ardea", roster.pending.len());

    {
        let mut player = app.world_mut().resource_mut::<Player>();
        player.third = true;
        player.ctrl.position = HERO_START;
        player.yaw = 2.2;
        player.pitch = -0.18;
    }
    app.world_mut().resource_mut::<Smoke>().enabled = false;

    let mut audio = audio;
    audio.settings = AudioSettings {
        // The world is built in metres (centimetre data x 0.01).
        units_per_meter: 1.0,
        ..audio.settings
    };
    let objects = objects::WorldObjects::load();
    eprintln!(
        "play: {} world entities around Ardea, {} item meshes",
        objects.list.len(),
        objects.item_mesh.len()
    );
    let mut quick = vec![None; 10];
    for (item, _, slot) in kit::START_KIT {
        if (1..=10).contains(slot) {
            quick[*slot as usize - 1] = Some((*item).to_string());
        }
    }
    app.insert_resource(PlayState { quick, ..default() });
    app.insert_resource(objects);
    app.insert_resource(HeroKit {
        sword_mesh,
        look: hero_look,
    })
    .insert_resource(library)
    .insert_resource(CoreRes(core))
    .insert_resource(session)
    .insert_resource(UiConfig {
        install_dir,
        drive_dialog: false,
        ..default()
    })
    .insert_resource(roster)
    .init_resource::<ScriptInput>()
    .init_resource::<interact::Interaction>()
    .init_resource::<barks::BarkState>()
    .add_plugins((AnimationPlugin, GothicUiPlugin, audio, CorePlugin));

    hero::register(app);
    objects::register(app);
    interact::register(app);
    windows::register(app);
    barks::register(app);
    fx::register(app);
    app.add_systems(Update, pose::pose_system.after(crate::walk::update_camera));
    npcs::register(app);
    routines::register(app);
    nav::register(app);
    events::register(app);
    cam::register(app);
    audio::register(app);
    hud::register(app);
    if smoke {
        script::register(app);
    }
    if let Some(dir) = record {
        record::register(app, dir);
    }
    Ok(())
}

pub(crate) fn sim_to_bevy_yaw(y: f32) -> f32 {
    (-y.cos()).atan2(-y.sin())
}

pub(crate) fn bevy_to_sim_yaw(t: f32) -> f32 {
    // Forward is -Z rotated by `t` about Y: (-sin t, -cos t) = (dx, dz).
    (-t.cos()).atan2(-t.sin())
}

/// Shortest signed angle from `from` to `to`.
pub(crate) fn angle_delta(from: f32, to: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Bevy yaw that makes an entity face the horizontal direction `d` (x, z).
pub(crate) fn yaw_of_dir(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(-dz)
}
