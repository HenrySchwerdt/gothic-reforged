//! Regions of the original `gui.pak` atlases (pixel rectangles).
//!
//! `g3.gui.res` names every view's background TGA but the compiled atlases ship without an
//! index, so these rectangles were recovered from the alpha layout of
//! `g3_hud_backgrounds_NN.dds` and matched to the view sizes in the resource file (each view
//! is exactly as large as its background). See `docs/research/UI_ORIGINAL.md`.
use crate::assets::Page;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pic {
    pub page: Page,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Lower-cased `g3.igl` image name; when the list resolves it, page and rectangle come from
    /// the original list (`UiAssets::resolve`) and the numbers below are only the fallback.
    pub key: Option<&'static str>,
}

impl Pic {
    pub const fn keyed(mut self, key: &'static str) -> Self {
        self.key = Some(key);
        self
    }
}

const fn bg(n: u8, x: u16, y: u16, w: u16, h: u16) -> Pic {
    Pic {
        page: Page::Bg(n),
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
        key: None,
    }
}
const fn ic(x: u16, y: u16, w: u16, h: u16) -> Pic {
    Pic {
        page: Page::Icons,
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
        key: None,
    }
}

// views (virtual 1024x768 position in the comment)
pub const QUESTS: Pic = bg(9, 1, 1, 937, 638).keyed("g3_hud_view_quests_back.tga"); // 14,16
pub const SKILLS: Pic = bg(10, 315, 1, 621, 638).keyed("g3_hud_view_skills_back.tga"); // 325,16
pub const INVENTORY: Pic = bg(11, 1, 1, 312, 638).keyed("g3_hud_view_inventory_back.tga"); // 634,16
pub const CHARACTER: Pic = bg(14, 1, 1, 312, 638).keyed("g3_hud_view_character_back.tga"); // 14,16
pub const SIDEBAR: Pic = bg(16, 604, 1, 62, 499).keyed("g3_hud_view_sidebar_back.tga"); // 946,76
pub const TRADE_VENDOR: Pic = bg(16, 1, 507, 303, 493).keyed("g3_hud_view_trade_vendor_back.tga"); // 14,16
pub const TRADE_PLAYER: Pic = bg(16, 668, 1, 304, 493).keyed("g3_hud_view_trade_player_back.tga"); // 691,16
/// Loot pool view (14,16, 312x638); the fallback rectangle is a guess, the igl resolves it.
pub const LOOT_POOL: Pic = bg(16, 330, 1, 312, 638).keyed("g3_hud_view_loot_back.tga");
pub const TRANSACTION: Pic =
    bg(17, 1, 216, 980, 213).keyed("g3_hud_view_trade_transaction_back.tga"); // 14,524
pub const DIALOG: Pic = bg(17, 1, 1, 980, 213).keyed("g3_hud_view_dialog_back.tga"); // 22,534
pub const TALK: Pic = bg(17, 1, 606, 570, 81).keyed("g3_hud_view_talk_back.tga"); // 227,100
pub const QUICKSLOTS: Pic = bg(17, 1, 689, 540, 56).keyed("g3_hud_view_quickslots_back.tga"); // 252,688
pub const STATUSBAR: Pic = bg(17, 290, 576, 201, 19).keyed("g3_hud_statusbar_back.tga");
pub const CONFIRM: Pic = bg(17, 290, 431, 482, 143).keyed("g3_hud_view_menu_confirm_back.tga");
pub const ITEM: Pic = bg(16, 306, 507, 287, 350).keyed("g3_hud_view_item_back.tga"); // 336,16
pub const TIP: Pic = bg(16, 595, 507, 350, 279).keyed("g3_hud_view_tutorial_back.tga");
pub const MAP_BACK: Pic = bg(11, 315, 1, 620, 638).keyed("g3_hud_view_map_back.tga");
pub const WORLD_MAP: Pic = bg(15, 1, 1, 580, 599).keyed("g3_hud_view_world_back.tga");
pub const TIME_SKY: Pic = bg(17, 573, 606, 158, 34).keyed("g3_hud_view_quests_time_day.tga");
pub const TIME_LABEL: Pic = bg(17, 573, 642, 158, 34).keyed("g3_hud_view_quests_time_back.tga");

// icon atlas
/// The map's player arrow (`G3_HUD_Arrow.tga`, 128x128).
pub const ARROW: Pic = ic(1, 203, 128, 128).keyed("g3_hud_arrow.tga");
pub const COMPASS_WHEEL: Pic = ic(218, 16, 169, 170);
pub const COMPASS_ROSE: Pic = ic(3, 1, 198, 199);
pub const PAGE_EXIT: Pic = ic(136, 249, 32, 30);
pub const PAGE_INVENTORY: Pic = ic(410, 55, 34, 32);
pub const PAGE_CHARACTER: Pic = ic(458, 94, 37, 38);
pub const PAGE_DOCUMENTS: Pic = ic(409, 98, 38, 35);
pub const PAGE_QUESTS: Pic = ic(454, 49, 42, 38);
pub const PAGE_QUILL: Pic = ic(457, 7, 36, 32);
pub const GLYPH_WEAPON: Pic = ic(137, 290, 30, 19);
pub const GLYPH_ARMOR: Pic = ic(361, 249, 31, 20);
pub const GLYPH_SCROLL: Pic = ic(184, 288, 25, 21);
pub const GLYPH_POTION: Pic = ic(232, 250, 17, 20);
pub const GLYPH_MISC: Pic = ic(452, 250, 26, 19);
pub const GLYPH_BOOK: Pic = ic(406, 250, 29, 20);
pub const GLYPH_ANVIL: Pic = ic(318, 247, 24, 22);
pub const MARK_QUEST: Pic = ic(481, 284, 18, 18).keyed("g3_hud_icon_location_ardea.tga");

pub const SYNTH_RECIPE: Pic = bg(14, 622, 1, 367, 638).keyed("g3_hud_view_synth_recipe_back.tga"); // 14,16
pub const RECIPE: Pic = bg(15, 583, 510, 333, 507).keyed("g3_hud_view_recipe_back.tga");
pub const LETTER: Pic = bg(15, 583, 1, 333, 507).keyed("g3_hud_view_letter_back.tga");
pub const BOOK: Pic = bg(16, 1, 1, 601, 504).keyed("g3_hud_view_book_back.tga"); // 24,83
pub const DOCUMENT: Pic = bg(12, 1, 1, 621, 638).keyed("g3_hud_view_document_back.tga"); // 14,16
/// Time-of-day strips of the quest view: dawn, day, dusk, night (158x34 each).
pub const TIME_OF_DAY: [Pic; 4] = [
    bg(17, 733, 606, 158, 34).keyed("g3_hud_view_quests_time_dawn.tga"),
    TIME_SKY,
    bg(17, 774, 501, 158, 34).keyed("g3_hud_view_quests_time_dusk.tga"),
    bg(17, 774, 537, 158, 34).keyed("g3_hud_view_quests_time_night.tga"),
];

/// Menu page backgrounds (1024x768 at the top of their atlas page).
pub const MENU_FILES_BG: Pic = bg(3, 0, 0, 1024, 768).keyed("g3_hud_view_menu_files_back.tga");
pub const MENU_VIDEO_BG: Pic =
    bg(8, 0, 0, 1024, 768).keyed("g3_hud_view_menu_options_video_back.tga");

pub const MENU_OPTIONS_BG: Pic = bg(1, 0, 0, 1024, 768).keyed("g3_hud_view_menu_options_back.tga");
/// The wide list page (the performance "customize" picture; also the key list).
pub const MENU_CUSTOM_BG: Pic =
    bg(6, 0, 0, 1024, 768).keyed("g3_hud_view_menu_performance_customize_back.tga");
pub const MENU_AUDIO_BG: Pic =
    bg(5, 0, 0, 1024, 768).keyed("g3_hud_view_menu_options_audio_back.tga");
pub const MENU_DIFFICULTY_BG: Pic =
    bg(4, 0, 0, 1024, 768).keyed("g3_hud_view_menu_difficulty_back.tga");

/// Main-menu banner background (page 02, 1024x768 at the top of the atlas).
pub const MENU_BG: Pic = bg(2, 0, 0, 1024, 768).keyed("g3_hud_view_menu_main_back.tga");
#[allow(dead_code)]
pub const MEDAL_ON: Pic = ic(131, 203, 39, 39).keyed("g3_hud_alternative_balancing_03.tga");
#[allow(dead_code)]
pub const MEDAL_OFF: Pic = ic(459, 203, 39, 39).keyed("g3_hud_alternative_balancing_00.tga");
