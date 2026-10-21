//! The VR HUD as a whole: which world-space parts are up follows `gothic_ui`'s
//! [`GuiManager`] (page stack and HUD flags), and the controllers drive them.
//!
//! Default controls (main hand = right):
//! * turn the off hand's wrist to the eyes: vitals;
//! * hold the off hand's X/A (`Primary`): quick-slot wheel at the main hand, release on a slot;
//! * main-hand grip over the shoulder: backpack inventory (again over the shoulder closes it);
//!   press items with the fingertip or the laser + trigger;
//! * dialog: laser + trigger or a fingertip on a row, thumbstick flicks scroll;
//! * the journal (`PageId::Quests`) is a book in the off hand, swipe over it to turn pages;
//!   the map is a sheet in the off hand, the laser shows marker labels.
use crate::backpack::{Backpack, over_shoulder};
use crate::dialog::DialogPanel;
use crate::handheld::{Book, HeldMap, MAP_SIZE, PAGE_SIZE, held_pose, journal_lines};
use crate::panel::Panel;
use crate::subtitles::{SubtitleView, layout};
use crate::wheel::QuickWheel;
use crate::wrist::{Vitals, wrist_panel, wrist_visible};
use glam::{Vec2, Vec3};
use gothic_ui::journal::QuestRow;
use gothic_ui::map::{MapMarker, MapSheet};
use gothic_ui::{GuiManager, PageId, PlayerPanel, QuickSlot, Subtitle};
use gothic_vr::{Hand, Pose, VrButton, VrInput};

/// Head and hands in world metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked {
    pub head: Pose,
    pub hands: [Option<Pose>; 2],
}

impl Tracked {
    pub fn from_input(input: &VrInput) -> Self {
        Self {
            head: input.head(),
            hands: Hand::BOTH.map(|h| input.hand_pose(h)),
        }
    }
}

/// The dialog on show: options and where the speaker's head is.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogData {
    pub options: Vec<String>,
    pub npc_head: Vec3,
}

/// What the host gives the HUD each frame (the same view models the flat UI uses).
#[derive(Debug, Clone, Default)]
pub struct HudData {
    pub vitals: Vitals,
    pub quick_slots: Vec<Option<QuickSlot>>,
    pub inventory: PlayerPanel,
    pub dialog: Option<DialogData>,
    pub subtitle: Subtitle,
    pub speaker_head: Option<Vec3>,
    pub quests: Vec<QuestRow>,
    pub map: Option<(MapSheet, Vec<MapMarker>, Vec2)>,
}

/// HUD -> host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudAction {
    /// Use quick slot `i` (key 1..0).
    UseQuickSlot(usize),
    /// An inventory stack was pressed (`PageRow::key`).
    InventoryItem(usize),
    /// A dialog option was chosen (`ChooseOption`).
    ChooseOption(usize),
    /// The page changed (the host mirrors it into the flat UI / the core's GUI state).
    Page(Option<PageId>),
}

#[derive(Debug, Clone)]
pub struct VrHud {
    pub gui: GuiManager,
    pub main_hand: Hand,
    pub wheel: QuickWheel,
    pub backpack: Option<Backpack>,
    pub dialog: Option<DialogPanel>,
    pub book: Option<(Book, Pose)>,
    pub map: Option<(HeldMap, Panel)>,
    pub map_open: bool,
    /// Wrist panel while visible.
    pub wrist: Option<Panel>,
    pub subtitle: Option<SubtitleView>,
    /// Map label under the laser.
    pub map_label: Option<String>,
}

impl Default for VrHud {
    fn default() -> Self {
        Self {
            gui: GuiManager::default(),
            main_hand: Hand::Right,
            wheel: QuickWheel::new(10),
            backpack: None,
            dialog: None,
            book: None,
            map: None,
            map_open: false,
            wrist: None,
            subtitle: None,
            map_label: None,
        }
    }
}

/// Fingertip: a little ahead of the grip.
pub fn fingertip(grip: &Pose) -> Vec3 {
    grip.transform_point(Vec3::new(0.0, -0.01, -0.09))
}

impl VrHud {
    fn set_page(&mut self, page: Option<PageId>, out: &mut Vec<HudAction>) {
        if self.gui.set_page(page) {
            out.push(HudAction::Page(self.gui.page()));
        }
    }

    /// Open a page from the host (journal, ...).
    pub fn open_page(&mut self, page: Option<PageId>) -> Vec<HudAction> {
        let mut out = Vec::new();
        self.set_page(page, &mut out);
        out
    }

    pub fn update(&mut self, input: &VrInput, t: &Tracked, data: &HudData) -> Vec<HudAction> {
        let mut out = Vec::new();
        let (main, off) = (self.main_hand, self.main_hand.other());
        let head = t.head;
        let mp = t.hands[main.index()];
        let op = t.hands[off.index()];
        let laser = |p: &Pose| (p.position, p.forward());

        // Quick-slot wheel.
        if self.gui.flags().quickslots && !self.gui.is_menu_open() {
            if input.just_pressed(off, VrButton::Primary)
                && let Some(p) = mp
            {
                self.wheel.slots = data.quick_slots.len().max(1);
                self.wheel.open_at(p.position, &head);
            }
            if let Some(p) = mp {
                self.wheel.update(p.position);
            }
            if input.just_released(off, VrButton::Primary)
                && let Some(i) = self.wheel.release()
                && data.quick_slots.get(i).is_some_and(Option::is_some)
            {
                out.push(HudAction::UseQuickSlot(i));
            }
        } else {
            self.wheel.release();
        }

        // Backpack.
        if input.just_pressed(main, VrButton::Grip)
            && let Some(p) = mp
            && over_shoulder(&head, p.position)
        {
            if self.backpack.take().is_some() {
                self.set_page(None, &mut out);
            } else if self.dialog.is_none() {
                self.backpack = Some(Backpack::open(&head));
                self.set_page(Some(PageId::Inventory), &mut out);
            }
        }
        if self.gui.page() != Some(PageId::Inventory) {
            self.backpack = None;
        }
        if let (Some(bp), Some(p)) = (self.backpack.as_mut(), mp) {
            let touched = bp
                .panel
                .touch(fingertip(&p), 0.012)
                .and_then(|uv| bp.hit(uv, &data.inventory));
            let (from, dir) = laser(&p);
            let pointed = bp
                .panel
                .ray(from, dir)
                .and_then(|uv| bp.hit(uv, &data.inventory));
            let was = std::mem::replace(&mut bp.touching, touched);
            bp.hovered = touched.or(pointed);
            let press = match touched {
                Some(h) if was != Some(h) => Some(h),
                Some(_) => None,
                None if input.just_pressed(main, VrButton::Trigger) => pointed,
                None => None,
            };
            if let Some(h) = press
                && let Some(key) = bp.press(h, &data.inventory)
            {
                out.push(HudAction::InventoryItem(key));
            }
        }

        // Dialog options.
        match &data.dialog {
            Some(d) => {
                let stale = self.dialog.as_ref().is_none_or(|p| p.options != d.options);
                if stale {
                    self.dialog =
                        Some(DialogPanel::new(d.options.clone(), d.npc_head, &head, main));
                    self.backpack = None;
                    self.set_page(Some(PageId::Dialog), &mut out);
                }
                if let (Some(dp), Some(p)) = (self.dialog.as_mut(), mp) {
                    // One row per flick of the thumbstick (down = later rows).
                    let stick = input.current.hand(main).thumbstick.y;
                    let before = input.previous.hand(main).thumbstick.y;
                    if stick.abs() > 0.8 && before.abs() <= 0.8 {
                        dp.scroll(if stick < 0.0 { 1 } else { -1 });
                    }
                    if let Some(i) = dp.touch(fingertip(&p)) {
                        out.push(HudAction::ChooseOption(i));
                    } else {
                        let (from, dir) = laser(&p);
                        dp.point(from, dir);
                        if input.just_pressed(main, VrButton::Trigger)
                            && let Some(i) = dp.confirm()
                        {
                            out.push(HudAction::ChooseOption(i));
                        }
                    }
                }
            }
            None => {
                if self.dialog.take().is_some() && self.gui.page() == Some(PageId::Dialog) {
                    self.set_page(None, &mut out);
                }
            }
        }

        // Journal book and map in the off hand.
        if self.gui.page() == Some(PageId::Quests) {
            if let Some(op) = op {
                let pose = held_pose(&op, off, Vec2::new(PAGE_SIZE.x * 2.0, PAGE_SIZE.y));
                let lines = journal_lines(&data.quests);
                let book = match self.book.take() {
                    Some((b, _)) if b.lines == lines => b,
                    _ => Book::new(lines),
                };
                let mut book = book;
                if let Some(p) = mp {
                    book.swipe(&pose, p.position);
                }
                self.book = Some((book, pose));
            }
        } else {
            self.book = None;
        }
        self.map_label = None;
        match (&data.map, op, self.map_open) {
            (Some((sheet, markers, player)), Some(op), true) => {
                let panel = Panel::new(held_pose(&op, off, MAP_SIZE), MAP_SIZE);
                let map = HeldMap::new(sheet.clone(), markers, *player);
                if let Some(p) = mp {
                    let (from, dir) = laser(&p);
                    self.map_label = map.label_at(&panel, from, dir).map(String::from);
                }
                self.map = Some((map, panel));
            }
            _ => self.map = None,
        }

        // Vitals on the wrist (after the page changes of this frame).
        self.wrist = op
            .map(|p| wrist_panel(&p, off))
            .filter(|w| self.gui.flags().bars && wrist_visible(w, head.position));

        // Subtitles.
        self.subtitle = layout(&data.subtitle, data.speaker_head, &head);
        out
    }
}
