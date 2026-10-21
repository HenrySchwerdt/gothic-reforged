//! The GUI manager rules: which HUD page is on show, which parts of the persistent HUD stay
//! visible for it, what Esc does, and what the input focus is while a page is open.
//!
//! Pure state machine, no Bevy types: `logic.rs` feeds it key presses and reads the answers,
//! `hud.rs` applies [`HudFlags`] to the HUD groups. The original keeps the same rules in
//! `gCGUIManager` / `gCHUDDialogMain2` / `gCHUDPageMain2`; the numbers are verified in
//! `docs/research/ENGINE_PORT_PLAN.md` section 7.

/// The 17 HUD pages of `gCHUDDialogMain2`, in the original's page-index order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageId {
    Inventory = 0,
    Spells,
    Documents,
    Skills,
    Quests,
    Trade,
    Dialog,
    Loot,
    Alchemy,
    Cooking,
    Smithing,
    Bless,
    Poison,
    Sharpen,
    Fry,
    Tutorial,
    Slideshow,
}

impl PageId {
    pub const COUNT: usize = 17;

    /// Page for an original index; `None` for -1 and for the invalid values (> 16) the
    /// original logs as "Detected invalid page value".
    pub fn from_index(i: i32) -> Option<Self> {
        use PageId::*;
        const ALL: [PageId; PageId::COUNT] = [
            Inventory, Spells, Documents, Skills, Quests, Trade, Dialog, Loot, Alchemy, Cooking,
            Smithing, Bless, Poison, Sharpen, Fry, Tutorial, Slideshow,
        ];
        usize::try_from(i).ok().and_then(|i| ALL.get(i).copied())
    }

    /// Pages Esc cannot close (the tutorial waits for its button, the slideshow runs out).
    pub fn esc_closable(self) -> bool {
        !matches!(self, PageId::Tutorial | PageId::Slideshow)
    }

    /// Pages that show the one-time tip window the first time they open.
    pub fn has_tip(self) -> bool {
        (self as usize) <= PageId::Trade as usize
    }
}

/// The six visibility flags of the persistent HUD layer (`gCHUDPageMain2::OnPageChanged`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HudFlags {
    /// NPC health (focus) bar.
    pub focus: bool,
    /// Health, mana and endurance bars.
    pub bars: bool,
    pub quickslots: bool,
    /// Page-strip tab bar.
    pub page_strip: bool,
    /// The two message / log areas.
    pub log_a: bool,
    pub log_b: bool,
}

impl HudFlags {
    const fn new(f: [u8; 6]) -> Self {
        Self {
            focus: f[0] != 0,
            bars: f[1] != 0,
            quickslots: f[2] != 0,
            page_strip: f[3] != 0,
            log_a: f[4] != 0,
            log_b: f[5] != 0,
        }
    }
    pub const NONE: Self = Self::new([0; 6]);
    /// No page open.
    pub const DEFAULT: Self = Self::new([1, 1, 1, 0, 1, 1]);
    const INVENTORY_LIKE: Self = Self::new([0, 1, 1, 1, 0, 1]);
    const FOCUS_ONLY: Self = Self::new([1, 0, 0, 0, 0, 0]);
}

/// Flags for the open page (`None` = no page), before the two hide toggles.
/// Port of gCHUDPageMain2::OnPageChanged (Game:0x10088a30) table at 0x102d31b4.
pub fn page_flags(page: Option<PageId>) -> HudFlags {
    use PageId::*;
    match page {
        None | Some(Bless | Poison | Sharpen | Fry | Tutorial) => HudFlags::DEFAULT,
        Some(Inventory | Spells | Documents | Skills | Quests) => HudFlags::INVENTORY_LIKE,
        Some(Dialog) => HudFlags::FOCUS_ONLY,
        Some(Trade | Loot | Alchemy | Cooking | Smithing | Slideshow) => HudFlags::NONE,
    }
}

/// Pages of the menu dialog (`gCHUDDialogMenu2`) that are ported so far (of the original's 14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuPage {
    Main,
    /// The options hub (`gCHUDMenuOptions2`): buttons to the option pages.
    Options,
    Video,
    Audio,
    Controls,
    ControlsCustom,
    Difficulty,
    Files,
    Credits,
    Confirm,
}

impl MenuPage {
    /// The page Back / Esc returns to (`None` for the main page). The option pages hang below
    /// the hub, the custom key list below the controls page, the rest below the main page.
    pub fn parent(self) -> Option<MenuPage> {
        match self {
            MenuPage::Main => None,
            MenuPage::Video | MenuPage::Audio | MenuPage::Controls | MenuPage::Difficulty => {
                Some(MenuPage::Options)
            }
            MenuPage::ControlsCustom => Some(MenuPage::Controls),
            MenuPage::Options | MenuPage::Files | MenuPage::Credits | MenuPage::Confirm => {
                Some(MenuPage::Main)
            }
        }
    }
}

/// What Esc did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscOutcome {
    /// A HUD page was closed.
    ClosedPage,
    /// No page was open: the menu opened.
    OpenedMenu,
    /// The open menu sub page went back to the page before it.
    MenuBack(MenuPage),
    /// The menu closed.
    ClosedMenu,
    /// A modal box swallowed the key.
    Modal,
    /// Nothing to do (a page that Esc cannot close, or the host owns the key).
    Ignored,
}

/// Input state while the GUI is up (the original's four calls: cursor, input, `State` := 6,
/// camera look off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFocus {
    pub cursor_visible: bool,
    /// Game input is suspended and the session runs in the GUI state.
    pub game_suspended: bool,
    pub camera_look: bool,
}

impl InputFocus {
    pub const GAME: Self = Self {
        cursor_visible: false,
        game_suspended: false,
        camera_look: true,
    };
    pub const GUI: Self = Self {
        cursor_visible: true,
        game_suspended: true,
        camera_look: false,
    };
}

/// Page stack and toggles of `gCGUIManager`.
#[derive(Clone, Debug, Default)]
pub struct GuiManager {
    page: Option<PageId>,
    /// The page under an open tutorial (restored when it is dismissed).
    below_tutorial: Option<PageId>,
    menu: Option<MenuPage>,
    /// `ToggleHUD` hides everything.
    pub hud_hidden: bool,
    /// `ToggleQuickSlots` hides the quick bar and the first log area.
    pub quickslots_hidden: bool,
}

impl GuiManager {
    pub fn page(&self) -> Option<PageId> {
        self.page
    }
    pub fn menu(&self) -> Option<MenuPage> {
        self.menu
    }
    pub fn is_menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Port of gCHUDDialogMain2::SetPage (Game:0x1008b340): no-op changes are ignored; the
    /// tutorial opens over the current page and remembers it. Returns whether anything changed.
    pub fn set_page(&mut self, page: Option<PageId>) -> bool {
        if self.page == page {
            return false;
        }
        if page == Some(PageId::Tutorial) {
            self.below_tutorial = self.page;
        }
        self.page = page;
        true
    }

    /// Dismiss the tutorial (command 0x504b): back to the page below it.
    pub fn end_tutorial(&mut self) -> bool {
        if self.page != Some(PageId::Tutorial) {
            return false;
        }
        self.page = self.below_tutorial.take();
        true
    }

    /// Port of gCGUIManager::OpenMenu (Game:0x1007c2e0): the menu opens on its main page.
    pub fn open_menu(&mut self) {
        self.menu = Some(MenuPage::Main);
    }

    /// Port of gCGUIManager::CloseMenu (Game:0x1007c380).
    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// Show a page of the menu (`None` closes it).
    pub fn set_menu(&mut self, p: Option<MenuPage>) {
        self.menu = p;
    }

    /// Open a sub page of the open menu.
    pub fn menu_goto(&mut self, p: MenuPage) {
        if self.menu.is_some() {
            self.menu = Some(p);
        }
    }

    /// Port of gCGUIManager::HandleMessage for Esc (Game:0x1007d9e0). `modal` is a box that
    /// consumes the key first; `host_owns` is true while the host handles Esc itself (an
    /// interaction window of the world).
    pub fn escape(&mut self, modal: bool, host_owns: bool) -> EscOutcome {
        if modal {
            return EscOutcome::Modal;
        }
        if let Some(m) = self.menu {
            return match m {
                MenuPage::Main => {
                    self.close_menu();
                    EscOutcome::ClosedMenu
                }
                // A sub page goes back one level (options pages to the hub, the hub to main).
                sub => {
                    let up = sub.parent().unwrap_or(MenuPage::Main);
                    self.menu = Some(up);
                    EscOutcome::MenuBack(up)
                }
            };
        }
        if let Some(p) = self.page {
            if !p.esc_closable() {
                return EscOutcome::Ignored;
            }
            self.page = None;
            self.below_tutorial = None;
            return EscOutcome::ClosedPage;
        }
        if host_owns {
            return EscOutcome::Ignored;
        }
        self.open_menu();
        EscOutcome::OpenedMenu
    }

    /// Flags for the HUD groups: the page's table row, reduced by the two toggles.
    pub fn flags(&self) -> HudFlags {
        if self.hud_hidden {
            return HudFlags::NONE;
        }
        let mut f = page_flags(self.page);
        if self.quickslots_hidden {
            f.quickslots = false;
            f.log_a = false;
        }
        f
    }

    /// Input focus: any page or the menu takes the cursor and suspends the game.
    pub fn input_focus(&self) -> InputFocus {
        if self.page.is_some() || self.menu.is_some() {
            InputFocus::GUI
        } else {
            InputFocus::GAME
        }
    }

    /// QuickLoad is refused while a dialog runs or a page is open
    /// (gCGUIManager::HandleMessage 0x8002).
    pub fn quick_load_allowed(&self, dialog_running: bool) -> bool {
        !dialog_running && self.page.is_none() && self.menu.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_indices_round_trip_and_reject_invalid() {
        assert_eq!(PageId::from_index(0), Some(PageId::Inventory));
        assert_eq!(PageId::from_index(6), Some(PageId::Dialog));
        assert_eq!(PageId::from_index(16), Some(PageId::Slideshow));
        assert_eq!(PageId::from_index(17), None);
        assert_eq!(PageId::from_index(-1), None);
    }

    #[test]
    fn visibility_table_matches_the_original_rows() {
        assert_eq!(page_flags(None), HudFlags::new([1, 1, 1, 0, 1, 1]));
        for p in [0, 1, 2, 3, 4] {
            let f = page_flags(PageId::from_index(p));
            assert_eq!(f, HudFlags::new([0, 1, 1, 1, 0, 1]), "page {p}");
        }
        for p in [5, 7, 8, 9, 10, 16] {
            assert_eq!(
                page_flags(PageId::from_index(p)),
                HudFlags::NONE,
                "page {p}"
            );
        }
        assert_eq!(
            page_flags(Some(PageId::Dialog)),
            HudFlags::new([1, 0, 0, 0, 0, 0])
        );
        for p in [11, 12, 13, 14, 15] {
            assert_eq!(
                page_flags(PageId::from_index(p)),
                HudFlags::DEFAULT,
                "page {p}"
            );
        }
    }

    #[test]
    fn toggles_override_the_table() {
        let mut g = GuiManager {
            quickslots_hidden: true,
            ..GuiManager::default()
        };
        let f = g.flags();
        assert!(!f.quickslots && !f.log_a && f.bars && f.focus);
        g.hud_hidden = true;
        assert_eq!(g.flags(), HudFlags::NONE);
    }

    #[test]
    fn esc_closes_page_then_opens_menu_then_closes_it() {
        let mut g = GuiManager::default();
        g.set_page(Some(PageId::Quests));
        assert_eq!(g.input_focus(), InputFocus::GUI);
        assert_eq!(g.escape(false, false), EscOutcome::ClosedPage);
        assert_eq!(g.input_focus(), InputFocus::GAME);
        assert_eq!(g.escape(false, false), EscOutcome::OpenedMenu);
        assert_eq!(g.menu(), Some(MenuPage::Main));
        assert_eq!(g.input_focus(), InputFocus::GUI);
        g.menu_goto(MenuPage::Files);
        assert_eq!(g.escape(false, false), EscOutcome::MenuBack(MenuPage::Main));
        g.menu_goto(MenuPage::Video);
        assert_eq!(
            g.escape(false, false),
            EscOutcome::MenuBack(MenuPage::Options)
        );
        assert_eq!(g.escape(false, false), EscOutcome::MenuBack(MenuPage::Main));
        g.menu_goto(MenuPage::ControlsCustom);
        assert_eq!(
            g.escape(false, false),
            EscOutcome::MenuBack(MenuPage::Controls)
        );
        assert_eq!(
            g.escape(false, false),
            EscOutcome::MenuBack(MenuPage::Options)
        );
        assert_eq!(g.escape(false, false), EscOutcome::MenuBack(MenuPage::Main));
        assert_eq!(g.escape(false, false), EscOutcome::ClosedMenu);
        assert_eq!(g.input_focus(), InputFocus::GAME);
    }

    #[test]
    fn esc_respects_modal_host_and_unclosable_pages() {
        let mut g = GuiManager::default();
        assert_eq!(g.escape(true, false), EscOutcome::Modal);
        assert_eq!(g.escape(false, true), EscOutcome::Ignored);
        assert_eq!(g.menu(), None);
        g.set_page(Some(PageId::Tutorial));
        assert_eq!(g.escape(false, false), EscOutcome::Ignored);
        assert_eq!(g.page(), Some(PageId::Tutorial));
    }

    #[test]
    fn tutorial_remembers_and_restores_the_page_below() {
        let mut g = GuiManager::default();
        g.set_page(Some(PageId::Skills));
        assert!(g.set_page(Some(PageId::Tutorial)));
        assert!(!g.set_page(Some(PageId::Tutorial)));
        assert!(g.end_tutorial());
        assert_eq!(g.page(), Some(PageId::Skills));
        assert!(!g.end_tutorial());
    }

    #[test]
    fn quick_load_is_refused_in_dialog_page_or_menu() {
        let mut g = GuiManager::default();
        assert!(g.quick_load_allowed(false));
        assert!(!g.quick_load_allowed(true));
        g.set_page(Some(PageId::Inventory));
        assert!(!g.quick_load_allowed(false));
    }

    #[test]
    fn only_the_first_six_pages_carry_a_tip() {
        assert!(PageId::Trade.has_tip());
        assert!(!PageId::Dialog.has_tip());
    }
}
