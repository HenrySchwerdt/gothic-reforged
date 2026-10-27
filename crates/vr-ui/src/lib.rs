//! World-space, diegetic HUD for VR (issue VR-10), driven by the flat UI's page state and
//! view models from `gothic-ui` (`GuiManager`, `OptionList`, `Subtitle`, `QuickSlot`,
//! `PlayerPanel`, `QuestRow`, `MapSheet`).
//!
//! * [`comfort`]: sizes in dmm, distances, viewing cone, facing (after Meta's VR UI comfort
//!   guidance).
//! * [`panel`]: world panels, laser and fingertip hit tests, rows, grids, line wrapping.
//! * [`wrist`]: health / mana / stamina on the off-hand wrist.
//! * [`wheel`]: quick-slot wheel at the hand or belt.
//! * [`backpack`]: over-the-shoulder inventory panel.
//! * [`dialog`]: options beside the speaking NPC.
//! * [`handheld`]: the journal as a book and the map as a sheet in the off hand.
//! * [`subtitles`]: world-locked subtitles near the speaker.
//! * [`sheet`], [`views`], [`windows`]: world-space windows (trade, loot, recipes, documents,
//!   quest log, character) as retained sheets with laser / poke input and scrolling.
//! * [`hud`]: the whole HUD ([`VrHud`]) following `GuiManager`.
//! * [`plugin`]: Bevy integration and the greybox gizmo view.
//! * [`render`]: the drawn HUD (UI rendered to textures on world-space quads).
//!
//! All layout and selection logic is Bevy-free and tested without a window.
pub mod backpack;
pub mod comfort;
pub mod dialog;
pub mod fade;
pub mod handheld;
pub mod hud;
pub mod panel;
pub mod plugin;
pub mod render;
mod render_sheet;
pub mod sheet;
pub mod subtitles;
pub mod tags;
pub mod views;
pub mod wheel;
pub mod windows;
pub mod wrist;

pub use hud::{DialogData, HudAction, HudData, Tracked, VrHud};
pub use panel::Panel;
pub use plugin::{HudActionMsg, HudDataRes, VrHudRes, VrUiPlugin, WindowActionMsg};
pub use render::{HudFont, HudHeadingFont, VrUiRenderPlugin};
pub use fade::SleepFade;
pub use sheet::{Sheet, SheetAct};
pub use tags::{FocusData, Floaters, Tag};
pub use views::{CharacterView, FileRowView, FilesView, QuestEntry, QuestLogView, WindowView};
pub use windows::{VrWindow, WindowAction};
pub use wrist::Vitals;
