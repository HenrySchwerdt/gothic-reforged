//! Gothic Reforged in-game UI (dialog, journal, inventory, character, trade, HUD).
//!
//! A Bevy plugin that is independent of the 3D world: it reads a [`GameSession`]
//! (`GameDatabase` + `GameState` from `gothic-gameplay`) and talks to the host through
//! messages ([`StartDialog`], [`ChooseOption`], [`UseItem`], [`OpenScreen`], [`Notify`] ...).
//! Original fonts/textures are loaded at runtime from the installation (`UiConfig`).
//! Layout, colours and tab names are an approximation of Gothic 3's look, not a port.
pub mod assets;
pub mod demo;
mod hud;
pub mod items;
mod logic;
mod screens;
mod style;

pub use assets::UiAssets;
pub use items::{ItemCatalog, ItemInfo};

use bevy::prelude::*;
use gothic_gameplay::dialog::{Choice, DialogSession};
use gothic_gameplay::script::Output;
use gothic_gameplay::{Assumptions, GameDatabase, GameState};
use std::collections::HashMap;
use std::path::PathBuf;

/// Everything the UI reads and (for dialogs) mutates.
#[derive(Resource)]
pub struct GameSession {
    pub db: std::sync::Arc<GameDatabase>,
    pub state: GameState,
    pub items: ItemCatalog,
    pub assume: Assumptions,
}

impl GameSession {
    pub fn new(
        db: impl Into<std::sync::Arc<GameDatabase>>,
        state: GameState,
        items: ItemCatalog,
    ) -> Self {
        Self {
            db: db.into(),
            state,
            items,
            assume: Assumptions::PERMISSIVE,
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct UiConfig {
    /// Gothic 3 installation (for `Gothic3.ttf` and `Data/gui.pak`).
    pub install_dir: PathBuf,
    /// Handle L / I / C / T / Esc / digits inside the plugin.
    pub hotkeys: bool,
    /// Let the plugin run the dialog engine for [`StartDialog`] / [`ChooseOption`].
    pub drive_dialog: bool,
    pub spawn_camera: bool,
    /// Scale the UI with the window (designed for 1280x720).
    pub auto_scale: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            install_dir: PathBuf::from("D:/SteamLibrary/steamapps/common/Gothic 3"),
            hotkeys: true,
            drive_dialog: true,
            spawn_camera: false,
            auto_scale: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    None,
    Dialog,
    Journal,
    Inventory,
    Character,
    Trade,
}

// ---- messages: host -> UI ----------------------------------------------------------
/// Open a conversation with an NPC (runs the dialog engine when `drive_dialog`).
#[derive(Message, Clone, Debug)]
pub struct StartDialog {
    pub npc: String,
}
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenScreen(pub Screen);
#[derive(Message, Clone, Copy, Debug)]
pub struct CloseScreen;
#[derive(Message, Clone, Debug)]
pub struct Notify {
    pub text: String,
    pub kind: ToastKind,
}
/// Fill the right-hand side of the trade screen (layout only; no trade rules yet).
#[derive(Message, Clone, Debug)]
pub struct OpenTrade {
    pub npc: String,
    pub stock: Vec<(String, u32)>,
}

/// Host-run dialog (the host's core drives the conversation; the UI only shows it): append
/// `lines` (speaker, text, is_player) and replace the menu. `open: false` closes the screen.
#[derive(Message, Clone, Debug)]
pub struct DialogUpdate {
    pub session: DialogSession,
    pub lines: Vec<(String, String, bool)>,
    pub menu: Vec<Choice>,
    pub open: bool,
}

// ---- messages: UI -> host -----------------------------------------------------------
/// The player picked entry `index` of the current dialog menu.
#[derive(Message, Clone, Copy, Debug)]
pub struct ChooseOption {
    pub index: usize,
}
#[derive(Message, Clone, Debug)]
pub struct UseItem {
    pub item: String,
}
#[derive(Message, Clone, Debug)]
pub struct TradeRequest {
    pub item: String,
    pub buy: bool,
}
#[derive(Message, Clone, Debug)]
pub struct DialogEnded {
    pub npc: String,
}
/// A world/AI effect produced by a dialog script that the host must carry out.
#[derive(Message, Clone, Debug)]
pub struct WorldEffect {
    pub npc: String,
    pub output: Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Quest,
    Xp,
    Item,
}

// ---- HUD inputs -------------------------------------------------------------------
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlayerVitals {
    pub hp: f32,
    pub hp_max: f32,
    pub mp: f32,
    pub mp_max: f32,
    pub sp: f32,
    pub sp_max: f32,
}

impl Default for PlayerVitals {
    fn default() -> Self {
        Self {
            hp: 100.0,
            hp_max: 100.0,
            mp: 0.0,
            mp_max: 0.0,
            sp: 100.0,
            sp_max: 100.0,
        }
    }
}

/// Camera/player yaw in radians (0 = north, clockwise) for the compass.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct PlayerHeading(pub f32);

// ---- internal state ------------------------------------------------------------------
#[derive(Clone, Debug)]
pub(crate) struct DialogLine {
    pub speaker: String,
    pub text: String,
    pub player: bool,
}

pub(crate) struct DialogView {
    pub session: DialogSession,
    pub lines: Vec<DialogLine>,
    pub menu: Vec<Choice>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Choose(usize),
    Close,
    Open(Screen),
    InvTab(usize),
    QuestTab(usize),
    SelectQuest(String),
    SelectItem(String),
    UseItem(String),
    Trade(String, bool),
}

#[derive(Message, Clone, Debug)]
pub(crate) struct Act(pub Action);

#[derive(Resource, Default)]
pub struct UiState {
    pub screen: Screen,
    pub(crate) dialog: Option<DialogView>,
    pub(crate) inv_tab: usize,
    pub(crate) inv_sel: Option<String>,
    pub(crate) quest_tab: usize,
    pub(crate) quest_sel: Option<String>,
    pub(crate) trade: Option<(String, Vec<(String, u32)>)>,
    pub(crate) scroll: HashMap<&'static str, f32>,
    pub(crate) dirty: bool,
}

impl UiState {
    pub fn set_screen(&mut self, s: Screen) {
        self.screen = s;
        self.dirty = true;
    }
    /// True while any full screen (not the HUD) is shown.
    pub fn is_open(&self) -> bool {
        self.screen != Screen::None
    }
}

pub struct GothicUiPlugin;

impl Plugin for GothicUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiConfig>()
            .init_resource::<UiState>()
            .init_resource::<PlayerVitals>()
            .init_resource::<PlayerHeading>()
            .add_message::<StartDialog>()
            .add_message::<OpenScreen>()
            .add_message::<CloseScreen>()
            .add_message::<Notify>()
            .add_message::<OpenTrade>()
            .add_message::<ChooseOption>()
            .add_message::<UseItem>()
            .add_message::<TradeRequest>()
            .add_message::<DialogEnded>()
            .add_message::<WorldEffect>()
            .add_message::<DialogUpdate>()
            .add_message::<Act>()
            .add_systems(Startup, (logic::setup, hud::spawn_hud).chain())
            .add_systems(
                Update,
                (
                    logic::ui_scale,
                    logic::hotkeys,
                    logic::dispatch_actions,
                    logic::drive_dialog,
                    logic::apply_dialog_updates,
                    logic::screens_requests,
                    screens::rebuild,
                    style::hover_style,
                    style::scroll_wheel,
                    hud::update_hud,
                    hud::toasts,
                )
                    .chain(),
            );
    }
}
