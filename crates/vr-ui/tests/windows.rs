//! World windows: layout (comfort numbers), pointer input, scrolling, tabs, documents.
use bevy::prelude::*;
use gothic_ui::{DocumentView, LootView, PageEvent, PageRow, PlayerPanel, RecipeRow, RecipeView, TradeView};
use gothic_vr::{Hand, Pose, VrInput, XrFrame};
use gothic_vr_ui::comfort::MIN_TARGET_DMM;
use gothic_vr_ui::hud::{Tracked, fingertip};
use gothic_vr_ui::views::{self, WindowUi, guess_name};
use gothic_vr_ui::*;
use gothic_vr_ui::sheet::MenuEntry;

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
        with_quest: vec!["ardea".into()],
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

#[test]
fn save_page_selects_a_slot_then_sends_the_file_command() {
    let view = WindowView::Files(FilesView {
        save_mode: false,
        can_save: true,
        rows: vec![
            FileRowView { key: "quick.json".into(), title: "Schnellspeicherung".into(), clock: "1. Tag, 12:23".into(), manual: false },
            FileRowView { key: "manual_5.json".into(), title: "Ardea".into(), clock: "2. Tag, 08:00".into(), manual: true },
        ],
    });
    let mut r = Rig::new(view);
    // Nothing selected: Laden is greyed.
    let k = r.find("Laden");
    assert!(r.press(k).is_empty());
    let k = r.find("Ardea");
    assert!(r.press(k).is_empty(), "selecting a slot is local");
    let k = r.find("Löschen");
    assert_eq!(r.press(k), [WindowAction::File(gothic_ui::FileCommand::Delete { key: "manual_5.json".into() })]);
    let k = r.find("Laden");
    assert_eq!(r.press(k), [WindowAction::File(gothic_ui::FileCommand::Load { key: "manual_5.json".into() })]);
}

#[test]
fn focus_name_hangs_above_the_head_and_floaters_rise_and_fade() {
    let eye = Pose::new(HEAD, Quat::IDENTITY);
    let f = FocusData {
        name: "Krieger".into(),
        attitude: gothic_ui::FocusAttitude::Hostile,
        health: Some(0.6),
        head: Vec3::new(0.0, 1.7, -3.0),
    };
    let t = tags::focus_tag(&f, &eye, 1.0);
    assert!(t.pose.position.y > 1.9 && (t.pose.position.z + 3.0).abs() < 1e-3);
    assert!(t.height > 0.0 && t.height <= tags::NAME_MAX_M);
    assert_eq!(t.bar, Some(0.6));
    assert!(t.color[0] > t.color[1] * 3.0, "hostile is red");
    let mut fl = Floaters::default();
    fl.push("+250 EP", [0.9, 0.85, 0.4], Vec3::new(0.0, 1.4, -0.5), 2.0);
    let y0 = fl.tags(&eye)[0].pose.position.y;
    fl.tick(1.0);
    let t1 = &fl.tags(&eye)[0];
    assert!(t1.pose.position.y > y0 + 0.3 && t1.alpha == 1.0);
    fl.tick(0.9);
    assert!(fl.tags(&eye)[0].alpha < 0.5, "fades at the end");
    fl.tick(0.5);
    assert!(fl.items.is_empty());
}

#[test]
fn quest_map_shows_location_markers_and_the_haupt_radio_filters_them() {
    let mut r = Rig::new(WindowView::Quests(quests()));
    let markers = |r: &Rig| r.win.sheet.items.iter().filter(|i| i.kind == sheet::ItemKind::Marker).count();
    assert_eq!(markers(&r), gothic_ui::map::LOCATIONS.len());
    let k = r.find("Haupt");
    assert!(r.press(k).is_empty());
    assert_eq!(markers(&r), 1, "only Ardea has a running quest");
    let k = r.find("Alle");
    r.press(k);
    assert_eq!(markers(&r), gothic_ui::map::LOCATIONS.len());
    let m = r.win.sheet.items.iter().find(|i| i.kind == sheet::ItemKind::Marker).unwrap();
    assert!(m.text.starts_with("g3_hud_icon_location_") && m.rect.w > 0.0);
}

#[test]
fn comfort_options_step_and_feed_the_hud() {
    let mut o = VrComfortOptions::default();
    let mut r = Rig::new(WindowView::Options(o));
    let change = |r: &mut Rig, id: OptId, d: i32, o: &mut VrComfortOptions| {
        let name = if d > 0 { "\u{203a}" } else { "\u{2039}" };
        let k = r
            .win
            .sheet
            .items
            .iter()
            .position(|i| i.text == name && i.act == Some(SheetAct::Opt(id, d)))
            .expect("button");
        let acts = r.press(k);
        assert_eq!(acts, [WindowAction::Option(id, d)]);
        o.apply(id, d);
        r.win.set_view(WindowView::Options(*o), &cx());
    };
    change(&mut r, OptId::SnapDegrees, 1, &mut o);
    assert_eq!(o.snap_degrees, 45.0);
    change(&mut r, OptId::Height, -1, &mut o);
    assert_eq!(o.height_cm, -5);
    change(&mut r, OptId::Handedness, 1, &mut o);
    assert!(!o.right_handed);
    let mut hud = VrHud::default();
    hud.apply_options(&o);
    assert_eq!(hud.main_hand, Hand::Left);
    change(&mut r, OptId::TurnMode, 1, &mut o);
    assert!(!o.snap_turn && !o.active(OptId::SnapDegrees), "degrees only matter for snap turning");
    assert!(r.win.sheet.items.iter().filter(|i| i.act == Some(SheetAct::Opt(OptId::SnapDegrees, 1))).all(|i| !i.enabled));
    for _ in 0..20 {
        o.apply(OptId::Height, 1);
        o.apply(OptId::VignetteStrength, 1);
    }
    assert_eq!((o.height_cm, o.vignette_strength), (30, 1.0));
}

#[test]
fn main_menu_lists_the_original_entries_and_wait_picker_sends_hours() {
    let mut r = Rig::new(WindowView::Menu(MenuView { in_game: false }));
    assert!(r.win.sheet.items.iter().all(|i| i.text != "Spiel speichern" || !i.enabled), "no save without a game");
    let k = r.find("Einstellungen");
    assert_eq!(r.press(k), [WindowAction::Menu(MenuEntry::Options)]);
    let mut r = Rig::new(WindowView::Wait(WaitView { max_hours: 3 }));
    let plus = r.win.sheet.items.iter().position(|i| i.text == "+").unwrap();
    r.press(plus);
    r.press(plus);
    r.press(plus);
    assert_eq!(r.win.ui.selected, Some(3), "clamped to the maximum");
    assert!(r.win.sheet.items.iter().any(|i| i.text == "3 Stunden"));
    let k = r.win.sheet.items.iter().position(|i| i.text == "Warten" && i.act.is_some()).unwrap();
    assert_eq!(r.press(k), [WindowAction::Wait(3)]);
}

#[test]
fn toasts_are_world_locked_and_follow_the_head_only_after_a_big_turn() {
    let mut hud = VrHud::default();
    let mut data = HudData::default();
    data.messages.print("Spiel gespeichert", gothic_ui::MessageKind::SaveOk);
    data.noise = Some(0.3);
    let mut input = VrInput::new();
    let mut frame = XrFrame::default();
    input.push(frame);
    hud.update(&input, &Tracked::from_input(&input), &data);
    assert_eq!(hud.tags.len(), 1);
    let p0 = hud.tags[0].pose.position;
    assert!(p0.y < 1.7 - 0.4, "below the view");
    frame.head = Pose::new(HEAD, Quat::from_rotation_y(0.3));
    input.push(frame);
    hud.update(&input, &Tracked::from_input(&input), &data);
    assert_eq!(hud.tags[0].pose.position, p0, "a small turn leaves the toast where it hangs");
    frame.head = Pose::new(HEAD, Quat::from_rotation_y(1.2));
    input.push(frame);
    hud.update(&input, &Tracked::from_input(&input), &data);
    assert!(hud.tags[0].pose.position.distance(p0) > 1.0, "a big turn re-hangs it");
}
