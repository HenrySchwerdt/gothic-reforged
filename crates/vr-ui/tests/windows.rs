//! World windows: layout (comfort numbers), pointer input, scrolling, tabs, documents.
use bevy::prelude::*;
use gothic_ui::{DocumentView, LootView, PageEvent, PageRow, PlayerPanel, RecipeRow, RecipeView, TradeView};
use gothic_vr::{Hand, Pose, VrInput, XrFrame};
use gothic_vr_ui::comfort::MIN_TARGET_DMM;
use gothic_vr_ui::hud::{Tracked, fingertip};
use gothic_vr_ui::views::{self, WindowUi, guess_name};
use gothic_vr_ui::*;

fn row(item: &str, count: i32, price: i32, key: usize) -> PageRow {
    PageRow {
        item: item.into(),
        count,
        price,
        key,
        ..Default::default()
    }
}

fn trade() -> TradeView {
    TradeView {
        vendor: "Hamlar".into(),
        vendor_rows: (0..60).map(|i| row(&format!("ItFo_Bread_{i}"), 2, 5 + i, i as usize)).collect(),
        vendor_gold: 300,
        player: PlayerPanel {
            title: String::new(),
            rows: (0..4).map(|i| row(&format!("ItMi_Junk_{i}"), 1, 3, 10 + i as usize)).collect(),
            gold: 750,
        },
        ask_rows: vec![row("ItFo_Bread_0", 1, 5, 0)],
        offer_rows: vec![],
        ask_total: 5,
        offer_total: 0,
        deal: false,
        balance: true,
        hint: String::new(),
    }
}

fn quests() -> QuestLogView {
    let q = |n: &str, tab: usize| QuestEntry {
        name: n.into(),
        title: format!("Quest {n}"),
        tab,
        log: vec!["Newest entry of the quest log that is long enough to wrap onto several lines of the detail area.".into(), "Older entry.".into()],
    };
    QuestLogView {
        quests: vec![q("a", 0), q("b", 0), q("c", 1), q("d", 2)],
        clock: "1. Tag, 12:23".into(),
        hour: 12,
        reputation: vec![("Nordmar".into(), 12)],
    }
}

fn name(t: &str) -> String {
    guess_name(t)
}

fn labels() -> views::Labels {
    views::Labels::default()
}

fn cx() -> views::Ctx<'static> {
    // Test helper: labels leaked once per call (tests are short lived).
    views::Ctx { name: &name, labels: Box::leak(Box::new(labels())) }
}

#[test]
fn every_window_meets_the_comfort_numbers() {
    let views = [
        WindowView::Trade(trade()),
        WindowView::Loot(LootView {
            title: "Truhe".into(),
            rows: (0..5).map(|i| row("ItMi_Gold", 10 * i, 0, i as usize)).collect(),
            pool_gold: Some(100),
            take_all: true,
            player: PlayerPanel::default(),
            hint: String::new(),
        }),
        WindowView::Recipe(RecipeView {
            title: "Amboss".into(),
            recipes: vec![RecipeRow { key: 0, name: "Schwert".into(), craftable: true, ..Default::default() }],
            gold: 1,
            hint: String::new(),
        }),
        WindowView::Document(DocumentView { title: "Brief".into(), left: "word ".repeat(200), right: String::new(), is_book: false }),
        WindowView::Quests(quests()),
        WindowView::Character(CharacterView { skills: vec![(3, "Schmieden".into())], protection: vec![0; 6], ..Default::default() }),
    ];
    for v in &views {
        let s = views::build(v, &WindowUi::default(), &views::Ctx { name: &name, labels: &labels() });
        assert!(s.distance >= 0.9, "{}: {} m is inside the fixation limit", v.name(), s.distance);
        assert!(
            s.min_target_dmm() >= MIN_TARGET_DMM,
            "{}: smallest target {} dmm",
            v.name(),
            s.min_target_dmm()
        );
        let size = s.size_m();
        assert!(size.x <= 1.4 && size.y <= 1.0, "{}: {size}", v.name());
        // Interactive items never overlap each other (Midas touch).
        let hot: Vec<_> = s.items.iter().filter(|i| i.interactive()).collect();
        for (a, i) in hot.iter().enumerate() {
            for j in &hot[a + 1..] {
                let (p, q) = (i.rect, j.rect);
                let overlap = p.x < q.x + q.w - 1e-4 && q.x < p.x + p.w - 1e-4 && p.y < q.y + q.h - 1e-4 && q.y < p.y + p.h - 1e-4;
                assert!(!overlap, "{}: {:?} overlaps {:?}", v.name(), i.text, j.text);
            }
        }
    }
}

#[test]
fn lists_scroll_and_clamp() {
    let v = WindowView::Trade(trade());
    let s0 = views::build(&v, &WindowUi::default(), &views::Ctx { name: &name, labels: &labels() });
    let r0 = s0.regions[0];
    assert!(r0.total > r0.visible, "60 stacks do not fit: {r0:?}");
    let rows0 = s0.items.iter().filter(|i| i.region == Some(0)).count();
    assert_eq!(rows0, r0.visible * 5);
    let far = WindowUi { scroll: vec![1000], ..Default::default() };
    let s1 = views::build(&v, &far, &views::Ctx { name: &name, labels: &labels() });
    assert_eq!(s1.regions[0].top, r0.max_top(), "clamped to the last page");
    let first = s1.items.iter().find(|i| i.region == Some(0)).unwrap();
    assert_eq!(first.text, guess_name(&format!("ItFo_Bread_{}", r0.max_top() * 5)));
}

struct Rig {
    input: VrInput,
    frame: XrFrame,
    win: VrWindow,
}

const HEAD: Vec3 = Vec3::new(0.0, 1.7, 0.0);

impl Rig {
    fn new(view: WindowView) -> Self {
        let mut frame = XrFrame::default();
        frame.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::new(0.25, 1.3, -0.3), Quat::IDENTITY));
        let mut input = VrInput::new();
        input.push(frame);
        let win = VrWindow::open(view, &Pose::new(HEAD, Quat::IDENTITY), &cx());
        Self { input, frame, win }
    }

    fn step(&mut self, f: impl FnOnce(&mut XrFrame)) -> Vec<WindowAction> {
        f(&mut self.frame);
        self.input.push(self.frame);
        let t = Tracked::from_input(&self.input);
        self.win.update(&self.input, &t, Hand::Right, &cx())
    }

    /// Laser at the centre of item `k`, trigger down then up; returns the actions of the press.
    fn press(&mut self, k: usize) -> Vec<WindowAction> {
        let from = Vec3::new(0.25, 1.3, -0.3);
        let to = self.win.panel.point(self.win.sheet.items[k].rect.center());
        let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (to - from).normalize());
        self.step(|f| f.hand_mut(Hand::Right).pose = Some(Pose::new(from, rot)));
        let out = self.step(|f| f.hand_mut(Hand::Right).trigger = 1.0);
        self.step(|f| f.hand_mut(Hand::Right).trigger = 0.0);
        out
    }

    fn find(&self, text: &str) -> usize {
        self.win.sheet.items.iter().position(|i| i.text == text).unwrap_or_else(|| panic!("no {text}"))
    }
}

#[test]
fn window_appears_in_front_upright_and_world_locked() {
    let r = Rig::new(WindowView::Trade(trade()));
    let p = r.win.panel.pose;
    let d = p.position.distance(HEAD);
    assert!((1.1..1.4).contains(&d), "{d}");
    assert!(p.position.y < HEAD.y, "a little below the eyes");
    let up = p.rotation * Vec3::Y;
    assert!(up.y > 0.999, "upright, no roll: {up}");
    let n = r.win.panel.normal();
    assert!(n.dot((HEAD - p.position).normalize()) > 0.99, "faces the eyes");
    assert!(comfort::in_comfort_cone(&Pose::new(HEAD, Quat::IDENTITY), r.win.panel.point(Vec2::new(0.5, 0.5))));
}

#[test]
fn trade_rows_buttons_and_close_send_the_flat_ui_page_events() {
    let mut r = Rig::new(WindowView::Trade(trade()));
    let k = r.find("Bread 3");
    assert_eq!(
        r.press(k),
        [WindowAction::Page(PageEvent::TradeAdd { player_side: false, key: 3 })]
    );
    let k = r.find("Gold-Ausgleich");
    assert_eq!(r.press(k), [WindowAction::Page(PageEvent::TradeBalance)]);
    // The deal button is greyed (deal = false): pressing does nothing.
    let k = r.find("Tauschen");
    assert!(r.press(k).is_empty());
    let k = r.find("×");
    assert_eq!(r.press(k), [WindowAction::Close]);
    // B closes as well.
    assert_eq!(r.step(|f| f.hand_mut(Hand::Right).secondary = true), [WindowAction::Close]);
}

#[test]
fn stick_flick_scrolls_the_list_under_the_laser_once_per_flick() {
    let mut r = Rig::new(WindowView::Trade(trade()));
    let k = r.find("Bread 0");
    let from = Vec3::new(0.25, 1.3, -0.3);
    let to = r.win.panel.point(r.win.sheet.items[k].rect.center());
    let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (to - from).normalize());
    r.step(|f| f.hand_mut(Hand::Right).pose = Some(Pose::new(from, rot)));
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::new(0.0, -1.0));
    assert_eq!(r.win.sheet.regions[0].top, 1);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::new(0.0, -1.0));
    assert_eq!(r.win.sheet.regions[0].top, 1, "held stick scrolls once");
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::ZERO);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::new(0.0, 1.0));
    assert_eq!(r.win.sheet.regions[0].top, 0);
}

#[test]
fn quest_tabs_and_selection_are_local_ui_state() {
    let mut r = Rig::new(WindowView::Quests(quests()));
    let rows = |r: &Rig| r.win.sheet.items.iter().filter(|i| i.region == Some(0)).map(|i| i.text.clone()).collect::<Vec<_>>();
    assert_eq!(rows(&r), ["Quest a", "Quest b"]);
    let k = r.find("Erfolgreich");
    assert!(r.press(k).is_empty(), "tabs never reach the core");
    assert_eq!(rows(&r), ["Quest c"]);
    let k = r.find("Gescheitert");
    r.press(k);
    assert_eq!(rows(&r), ["Quest d"]);
    // The detail shows the log of the selected quest.
    let paragraphs: Vec<_> = r.win.sheet.items.iter().filter(|i| i.kind == sheet::ItemKind::Paragraph).collect();
    assert!(paragraphs.len() >= 3, "wrapped log lines");
    // Select the second running quest.
    let k = r.find("Aktuell");
    r.press(k);
    let k = r.find("Quest b");
    r.press(k);
    assert_eq!(r.win.ui.selected, Some(1));
    assert!(r.win.sheet.items.iter().any(|i| i.selected && i.text == "Quest b"));
}

#[test]
fn documents_turn_pages_by_button_and_flick() {
    let doc = DocumentView { title: "Buch".into(), left: "wort ".repeat(400), right: String::new(), is_book: false };
    let spreads = views::document_spreads(&doc);
    assert!(spreads > 2);
    let mut r = Rig::new(WindowView::Document(doc));
    let k = r.find("›");
    r.press(k);
    assert_eq!(r.win.ui.page, 1);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::new(1.0, 0.0));
    assert_eq!(r.win.ui.page, 2);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::ZERO);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick = Vec2::new(-1.0, 0.0));
    assert_eq!(r.win.ui.page, 1);
}

#[test]
fn hud_opens_the_tablet_on_y_and_shows_the_host_window() {
    let mut hud = VrHud::default();
    let mut input = VrInput::new();
    let mut frame = XrFrame::default();
    frame.hand_mut(Hand::Left).pose = Some(Pose::new(Vec3::new(-0.25, 1.3, -0.3), Quat::IDENTITY));
    frame.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::new(0.25, 1.3, -0.3), Quat::IDENTITY));
    input.push(frame);
    let mut data = HudData::default();
    let step = |hud: &mut VrHud, input: &mut VrInput, frame: &XrFrame, data: &HudData| {
        input.push(*frame);
        let t = Tracked::from_input(input);
        hud.update(input, &t, data)
    };
    step(&mut hud, &mut input, &frame, &data);
    frame.hand_mut(Hand::Left).secondary = true;
    assert_eq!(step(&mut hud, &mut input, &frame, &data), [HudAction::OpenTablet]);
    data.window = Some(WindowView::Quests(quests()));
    frame.hand_mut(Hand::Left).secondary = false;
    step(&mut hud, &mut input, &frame, &data);
    assert!(hud.window.is_some());
    // A poke with the main hand's fingertip works at arm's length too.
    let w = hud.window.as_ref().unwrap();
    let k = w.sheet.items.iter().position(|i| i.text == "Erfolgreich").unwrap();
    let tip = w.panel.point(w.sheet.items[k].rect.center()) + w.panel.normal() * 0.005;
    let grip = Pose::new(Vec3::ZERO, Quat::IDENTITY);
    frame.hand_mut(Hand::Right).pose = Some(Pose::new(tip - fingertip(&grip), Quat::IDENTITY));
    step(&mut hud, &mut input, &frame, &data);
    assert_eq!(hud.window.as_ref().unwrap().ui.tab, 1);
    // The host clears it: the window goes away.
    data.window = None;
    step(&mut hud, &mut input, &frame, &data);
    assert!(hud.window.is_none());
}
