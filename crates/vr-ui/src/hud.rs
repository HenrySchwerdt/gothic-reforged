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
use crate::views::{WindowView, guess_name};
use crate::wheel::QuickWheel;
use crate::windows::{VrWindow, WindowAction};
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
    /// The window to show (trade, loot, recipes, document, quest log, character); `None`
    /// closes it. The host builds it from the core (`gothic-vr-bridge::feed`).
    pub window: Option<WindowView>,
    /// The focused character (name above the head, attitude colour, health).
    pub focus: Option<crate::tags::FocusData>,
    /// Game messages (`gothic_ui::GameMessages`): shown as short toasts below the view.
    pub messages: gothic_ui::GameMessages,
    /// Pickpocket noise 0..1 (`Pickpocket::noise`), shown as a fourth bar on the watch.
    pub noise: Option<f32>,
    /// The VR comfort options (main hand, subtitles are read from here).
    pub comfort: crate::comfort_opts::VrComfortOptions,
    /// Rising texts (XP, damage), pushed by the host from `Event::XpGained` / hits.
    pub floaters: crate::tags::Floaters,
    /// Localised item names by template (the HUD falls back to a guess from the template).
    pub names: std::collections::HashMap<String, String>,
    /// Icon file name (`IT_1H_OrcSlayer_01.tga`) by item template, for the item pictures.
    pub icons: std::collections::HashMap<String, String>,
    /// Fixed texts of the windows (the original loc-table entries via `Labels::from_text`).
    pub labels: crate::views::Labels,
}

impl HudData {
    pub fn name(&self, template: &str) -> String {
        self.names
            .get(template)
            .cloned()
            .unwrap_or_else(|| guess_name(template))
    }
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
    /// The off hand's `Y` with no window open: show the tablet (quest log / character);
    /// the host puts the window into `HudData::window`.
    OpenTablet,
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
    /// The open window, if any.
    pub window: Option<VrWindow>,
    /// Actions of the window since the last drain (the plugin sends them as messages).
    pub window_actions: Vec<WindowAction>,
    /// World text tags: the focus name first, then the floating texts.
    pub tags: Vec<crate::tags::Tag>,
    /// Yaw the toasts hang at; follows the head lazily (only when it turned more than 35 degrees).
    toast_yaw: Option<f32>,
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
            window: None,
            window_actions: Vec::new(),
            tags: Vec::new(),
            toast_yaw: None,
        }
    }
}

/// Fingertip: a little ahead of the grip.
pub fn fingertip(grip: &Pose) -> Vec3 {
    grip.transform_point(Vec3::new(0.0, -0.01, -0.09))
}

impl VrHud {
    /// The main hand follows the comfort option.
    pub fn apply_options(&mut self, o: &crate::comfort_opts::VrComfortOptions) {
        self.main_hand = o.main_hand();
    }

    /// Game messages as world-locked toasts below the view direction: they hang at a yaw that
    /// follows the head only when it turned far (no head-locked UI), newest at the top.
    fn toasts(&mut self, data: &HudData, head: &Pose) -> Vec<crate::tags::Tag> {
        let msgs: Vec<_> = data.messages.lines().collect();
        if msgs.is_empty() {
            return Vec::new();
        }
        let f = head.forward();
        let yaw = (-f.x).atan2(-f.z);
        let turned = self.toast_yaw.is_none_or(|y| {
            let d = (yaw - y + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            d.abs() > 35f32.to_radians()
        });
        if turned {
            self.toast_yaw = Some(yaw);
        }
        let y = self.toast_yaw.unwrap_or(yaw);
        let dir = Vec3::new(-y.sin(), 0.0, -y.cos());
        msgs.iter()
            .rev()
            .take(3)
            .enumerate()
            .map(|(i, m)| {
                let at = head.position + dir * 1.4 - Vec3::Y * (0.5 + 0.08 * i as f32);
                let c = match m.kind {
                    gothic_ui::MessageKind::Error => [0.85, 0.30, 0.25],
                    gothic_ui::MessageKind::SaveOk => [0.64, 0.84, 0.47],
                    gothic_ui::MessageKind::Info => [0.878, 0.769, 0.584],
                };
                crate::tags::Tag {
                    text: m.text.clone(),
                    color: c,
                    pose: Pose::new(at, crate::comfort::facing(at, head.position)),
                    height: crate::comfort::dmm(22.0, 1.4),
                    alpha: gothic_ui::GameMessages::alpha(m),
                    bar: None,
                }
            })
            .collect()
    }

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

        if data.window.is_none() && input.just_pressed(off, VrButton::Secondary) {
            out.push(HudAction::OpenTablet);
        }

        // World window (trade, loot, recipes, documents, quest log, character).
        match &data.window {
            Some(v) => {
                let name = |t: &str| data.name(t);
                let namer = crate::views::Ctx { name: &name, labels: &data.labels };
                match self.window.as_mut() {
                    Some(w) if std::mem::discriminant(&w.view) == std::mem::discriminant(v) => {
                        if w.view != *v {
                            w.set_view(v.clone(), &namer);
                        }
                    }
                    _ => {
                        self.window = Some(VrWindow::open(v.clone(), &head, &namer));
                        self.backpack = None;
                    }
                }
                if let Some(w) = self.window.as_mut() {
                    let acts = w.update(input, t, main, &namer);
                    self.window_actions.extend(acts);
                }
            }
            None => self.window = None,
        }

        // Vitals on the wrist (after the page changes of this frame).
        self.wrist = op
            .map(|p| {
                let mut w = wrist_panel(&p, off);
                if data.noise.is_some() {
                    w.size.y *= 1.35;
                }
                w
            })
            .filter(|w| self.gui.flags().bars && wrist_visible(w, head.position));

        // Focus name and floating texts, in the world.
        self.tags.clear();
        if let Some(f) = &data.focus {
            self.tags.push(crate::tags::focus_tag(f, &head, 1.0));
        }
        self.tags.extend(data.floaters.tags(&head));
        let toasts = self.toasts(data, &head);
        self.tags.extend(toasts);
        self.tags.truncate(8);

        // Subtitles.
        self.subtitle = layout(&data.subtitle, data.speaker_head, &head);
        out
    }
}
