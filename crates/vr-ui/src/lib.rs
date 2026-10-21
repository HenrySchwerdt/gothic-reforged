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
//! * [`hud`]: the whole HUD ([`VrHud`]) following `GuiManager`.
//! * [`plugin`]: Bevy integration and the greybox gizmo view.
//! * [`render`]: the drawn HUD (UI rendered to textures on world-space quads).
//!
//! All layout and selection logic is Bevy-free and tested without a window.
pub mod backpack;
pub mod comfort;
pub mod dialog;
pub mod handheld;
pub mod hud;
pub mod panel;
pub mod plugin;
pub mod render;
pub mod subtitles;
pub mod wheel;
pub mod wrist;

pub use hud::{DialogData, HudAction, HudData, Tracked, VrHud};
pub use panel::Panel;
pub use plugin::{HudActionMsg, HudDataRes, VrHudRes, VrUiPlugin};
pub use render::{HudFont, VrUiRenderPlugin};
pub use wrist::Vitals;
