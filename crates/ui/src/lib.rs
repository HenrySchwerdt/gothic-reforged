//! Gothic Reforged in-game UI (dialog, journal, inventory, character, trade, HUD).
//!
//! A Bevy plugin that is independent of the 3D world: it reads a [`GameSession`]
//! (`GameDatabase` + `GameState` from `gothic-gameplay`) and talks to the host through
//! messages ([`StartDialog`], [`ChooseOption`], [`UseItem`], [`OpenScreen`], [`Notify`] ...).
//! Original fonts/textures are loaded at runtime from the installation (`UiConfig`).
//! Layouts are rebuilt from the original `g3.gui.res` and atlas art (see
//! `docs/research/UI_ORIGINAL.md`); screens are visual ports only, not gameplay-complete.
pub mod assets;
mod atlas;
pub mod demo;
pub mod dialogmenu;
pub mod files;
mod hud;
pub mod items;
pub mod journal;
mod logic;
pub mod manager;
pub mod options;
pub mod pages;
pub mod reading;
mod screens;
mod screens_menu;
mod screens_read;
mod style;

pub use assets::{Page, UiAssets};
pub use dialogmenu::{GameMessages, MessageKind, OptionList, Subtitle};
pub use files::{FileCommand, SlotKind, SlotList, SlotRow};
pub use hud::{FocusHealth, QuickSlot, QuickSlots};
pub use items::{ItemCatalog, ItemInfo};
pub use manager::{EscOutcome, GuiManager, HudFlags, InputFocus, MenuPage, PageId};
pub use options::{ApplyVideo, VideoOptions};
pub use pages::{
    DocumentView, Ingredient, LootView, PageEvent, PageRow, PageView, PlayerPanel, RecipeRow,
    RecipeView, ShowPage, TradeView,
};

use bevy::prelude::*;
use gothic_gameplay::dialog::{Choice, DialogSession};
use gothic_gameplay::script::Output;
use gothic_gameplay::{Assumptions, GameDatabase, GameState};
use std::collections::HashMap;
use std::path::PathBuf;

/// Learning-point price of teaching `points` units of an attribute (`tag`, current value):
/// 1:1 unless the host installs the game rule (`gothic_sim::economy::teach_cost`).
#[derive(Resource, Clone)]
pub struct TeachCost(pub std::sync::Arc<journal::CostRule>);

impl Default for TeachCost {
    fn default() -> Self {
        Self(std::sync::Arc::new(|_, _, points| points))
    }
}

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
    /// The "subtitles" option (`Options.Audio` / `Subtitle`): talk lines are shown.
    pub subtitles: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            install_dir: PathBuf::from("D:/SteamLibrary/steamapps/common/Gothic 3"),
            hotkeys: true,
            drive_dialog: true,
            spawn_camera: false,
            auto_scale: true,
            subtitles: true,
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
    /// Loot and work-station list page (host-driven, see [`pages`]).
    Loot,
    /// Full-screen main menu (banner background, entries report [`MenuChoice`]).
    MainMenu,
    /// Save / load page of the menu (`gCHUDMenuFiles2`), fed by [`SlotList`].
    Files,
    /// Video options page of the menu (`gCHUDMenuOptionsVideo2`).
    Video,
    /// Work-station recipe page (`gCHUDPageSynth2`), host driven (see [`pages`]).
    Recipe,
    /// Readable book or letter (`gCHUDViewBook2` / `gCHUDViewLetter2`).
    Document,
}

impl Screen {
    /// The HUD page this screen is, if it is one.
    pub fn page_id(self) -> Option<PageId> {
        match self {
            Screen::Dialog => Some(PageId::Dialog),
            Screen::Journal => Some(PageId::Quests),
            Screen::Inventory => Some(PageId::Inventory),
            Screen::Character => Some(PageId::Skills),
            Screen::Trade => Some(PageId::Trade),
            Screen::Loot => Some(PageId::Loot),
            Screen::Recipe => Some(PageId::Alchemy),
            Screen::Document => Some(PageId::Documents),
            Screen::None | Screen::MainMenu | Screen::Files | Screen::Video => None,
        }
    }

    /// The menu page this screen is, if it is one.
    pub fn menu_page(self) -> Option<MenuPage> {
        match self {
            Screen::MainMenu => Some(MenuPage::Main),
            Screen::Files => Some(MenuPage::Files),
            Screen::Video => Some(MenuPage::Video),
            _ => None,
        }
    }
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

/// Modal box shown above everything ([`ShowModal`]); answered with [`ModalAnswer`].
#[derive(Clone, Debug)]
pub enum Modal {
    /// Yes/no box (`tag` is echoed in the answer).
    Confirm {
        tag: String,
        text: String,
        yes: String,
        no: String,
    },
    /// Tip window with a "do not show again" checkbox and an Ok button.
    Tip { title: String, text: String },
}
#[derive(Message, Clone, Debug)]
pub struct ShowModal(pub Modal);
/// Answer of a modal: for `Confirm` `yes` is the chosen button, for `Tip` (tag `"tip"`) it is
/// the state of the "do not show again" checkbox.
#[derive(Message, Clone, Debug)]
pub struct ModalAnswer {
    pub tag: String,
    pub yes: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    NewGame,
    LoadGame,
    SaveGame,
    Options,
    Credits,
    Quit,
}
/// The player picked an entry of the main menu.
#[derive(Message, Clone, Copy, Debug)]
pub struct MenuChoice(pub MenuEntry);

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
#[allow(dead_code)]
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
    Page(PageEvent),
    Menu(MenuEntry),
    Modal(bool),
    TipCheck,
    /// Esc, routed by the GUI manager rules.
    Escape,
    /// Move the selection of the dialog option list.
    OptionStep(i32),
    /// Back one menu page (the "Zurück" buttons).
    MenuBack,
    FileSelect(Option<usize>),
    FileDo,
    FileDelete,
    VideoMode(i32),
    VideoSlider(options::Slider, i32),
    VideoVsync,
    VideoApply,
    SelectRecipe(usize),
    BookSpread(i32),
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
    /// The host-driven page on show (trade / loot).
    pub(crate) page: Option<PageView>,
    pub(crate) scroll: HashMap<&'static str, f32>,
    pub(crate) dirty: bool,
    pub(crate) modal: Option<Modal>,
    pub(crate) tip_never: bool,
    /// Whether "Spiel speichern" is enabled in the main menu.
    pub menu_can_save: bool,
    /// Page stack, HUD visibility flags and Esc routing (`gCGUIManager`).
    pub gui: GuiManager,
    /// Selection of the dialog option list.
    pub options: OptionList,
    pub subtitle: Subtitle,
    pub messages: GameMessages,
    pub(crate) files: FilesState,
    /// Draft of the video page (applied with the Apply button).
    pub(crate) video: VideoOptions,
    pub(crate) recipe_sel: usize,
    pub(crate) spread: usize,
}

/// State of the save / load page.
#[derive(Default)]
pub(crate) struct FilesState {
    pub rows: Vec<SlotRow>,
    pub can_save: bool,
    /// Opened from "Spiel speichern" (rows can be overwritten, a new slot can be written).
    pub save_mode: bool,
    pub selected: Option<usize>,
}

/// What the host tells the GUI manager about the world (Esc routing).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct HostGate {
    /// The host handles Esc itself (an interaction window of the world is open), so Esc must
    /// not open the menu.
    pub esc_blocked: bool,
}

impl UiState {
    pub fn set_screen(&mut self, s: Screen) {
        self.screen = s;
        self.dirty = true;
        self.gui.set_page(s.page_id());
        self.gui.set_menu(s.menu_page());
    }
    /// True while any full screen (not the HUD) is shown.
    pub fn is_open(&self) -> bool {
        self.screen != Screen::None || self.modal.is_some()
    }
}

pub struct GothicUiPlugin;

impl Plugin for GothicUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiConfig>()
            .init_resource::<UiState>()
            .init_resource::<PlayerVitals>()
            .init_resource::<PlayerHeading>()
            .init_resource::<QuickSlots>()
            .init_resource::<FocusHealth>()
            .init_resource::<HostGate>()
            .init_resource::<TeachCost>()
            .init_resource::<VideoOptions>()
            .add_message::<StartDialog>()
            .add_message::<OpenScreen>()
            .add_message::<CloseScreen>()
            .add_message::<Notify>()
            .add_message::<OpenTrade>()
            .add_message::<ChooseOption>()
            .add_message::<UseItem>()
            .add_message::<TradeRequest>()
            .add_message::<ShowPage>()
            .add_message::<PageEvent>()
            .add_message::<DialogEnded>()
            .add_message::<WorldEffect>()
            .add_message::<DialogUpdate>()
            .add_message::<ShowModal>()
            .add_message::<ModalAnswer>()
            .add_message::<MenuChoice>()
            .add_message::<SlotList>()
            .add_message::<FileCommand>()
            .add_message::<ApplyVideo>()
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
                    logic::sync_dialog_models,
                    logic::tick_models,
                    logic::dialog_wheel,
                    logic::screens_requests,
                    screens::rebuild,
                    style::hover_style,
                    style::scroll_wheel,
                    hud::update_hud,
                    hud::toasts,
                    hud::fade_subtitle,
                )
                    .chain(),
            );
    }
}
