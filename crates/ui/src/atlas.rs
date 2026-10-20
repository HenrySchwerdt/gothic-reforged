//! Regions of the original `gui.pak` atlases (pixel rectangles).
//!
//! `g3.gui.res` names every view's background TGA but the compiled atlases ship without an
//! index, so these rectangles were recovered from the alpha layout of
//! `g3_hud_backgrounds_NN.dds` and matched to the view sizes in the resource file (each view
//! is exactly as large as its background). See `docs/research/UI_ORIGINAL.md`.
use crate::assets::Page;

#[derive(Clone, Copy, Debug)]
pub struct Pic {
    pub page: Page,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const fn bg(n: u8, x: u16, y: u16, w: u16, h: u16) -> Pic {
    Pic {
        page: Page::Bg(n),
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
    }
}
const fn ic(x: u16, y: u16, w: u16, h: u16) -> Pic {
    Pic {
        page: Page::Icons,
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
    }
}

// views (virtual 1024x768 position in the comment)
pub const QUESTS: Pic = bg(9, 1, 1, 937, 638); // 14,16
pub const SKILLS: Pic = bg(10, 315, 1, 621, 638); // 325,16
pub const INVENTORY: Pic = bg(11, 1, 1, 312, 638); // 634,16
pub const CHARACTER: Pic = bg(14, 1, 1, 312, 638); // 14,16
pub const SIDEBAR: Pic = bg(16, 604, 1, 62, 499); // 946,76
pub const TRADE_VENDOR: Pic = bg(16, 1, 507, 303, 493); // 14,16
pub const TRADE_PLAYER: Pic = bg(16, 668, 1, 304, 493); // 691,16
pub const TRANSACTION: Pic = bg(17, 1, 216, 980, 213); // 14,524
pub const DIALOG: Pic = bg(17, 1, 1, 980, 213); // 22,534
pub const TALK: Pic = bg(17, 1, 606, 570, 81); // 227,100
pub const QUICKSLOTS: Pic = bg(17, 1, 689, 540, 56); // 252,688
pub const STATUSBAR: Pic = bg(17, 290, 576, 201, 19);
pub const CONFIRM: Pic = bg(17, 290, 431, 482, 143);
pub const TIP: Pic = bg(16, 595, 507, 350, 279);
pub const WORLD_MAP: Pic = bg(15, 1, 1, 580, 599);
pub const TIME_SKY: Pic = bg(17, 573, 606, 158, 34);
pub const TIME_LABEL: Pic = bg(17, 573, 642, 158, 34);

// icon atlas
pub const COMPASS_WHEEL: Pic = ic(218, 16, 169, 170);
pub const COMPASS_ROSE: Pic = ic(3, 1, 198, 199);
pub const PAGE_EXIT: Pic = ic(447, 138, 39, 39);
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
pub const MARK_QUEST: Pic = ic(481, 284, 18, 18);
