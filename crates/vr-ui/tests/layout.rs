//! Layout and selection logic of the VR HUD parts (no window).
use glam::{Quat, Vec2, Vec3};
use gothic_ui::journal::QuestRow;
use gothic_ui::map::{MapMarker, MapSheet};
use gothic_ui::{PageRow, PlayerPanel, Subtitle};
use gothic_vr::{Hand, Pose};
use gothic_vr_ui::backpack::{Backpack, CELL, PanelHit, over_shoulder};
use gothic_vr_ui::comfort::*;
use gothic_vr_ui::dialog::{DialogPanel, ROW_DMM, VISIBLE_ROWS};
use gothic_vr_ui::handheld::{Book, HeldMap, LINES_PER_PAGE, page_text_dmm};
use gothic_vr_ui::panel::{Panel, cell_at, row_at, wrap};
use gothic_vr_ui::subtitles::{Side, layout};
use gothic_vr_ui::wheel::QuickWheel;
use gothic_vr_ui::wrist::{Vitals, WRIST_SIZE, health_pulse, wrist_panel, wrist_visible};

fn head() -> Pose {
    Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY)
}

fn faces(panel: &Panel, eye: Vec3) -> bool {
    let to_eye =
        Vec3::new(eye.x, 0.0, eye.z) - Vec3::new(panel.pose.position.x, 0.0, panel.pose.position.z);
    panel.normal().angle_between(to_eye) < 1e-3
}

#[test]
fn comfort_helpers() {
    assert!((dmm(20.0, 2.0) - 0.04).abs() < 1e-6);
    assert!((to_dmm(0.04, 2.0) - 20.0).abs() < 1e-3);
    let eye = Vec3::new(0.0, 1.7, 0.0);
    let p = clamp_distance(eye, Vec3::new(0.0, 1.7, -0.3), &POINT_DISTANCE);
    assert!((p.distance(eye) - 0.9).abs() < 1e-5);
    let q = facing(Vec3::new(1.0, 1.0, -1.0), eye);
    let n = q * Vec3::Z;
    assert!(n.y.abs() < 1e-6, "upright");
    assert!(n.angle_between(Vec3::new(-1.0, 0.0, 1.0)) < 1e-5);
    assert!(in_comfort_cone(&head(), Vec3::new(0.0, 1.2, -1.0)));
    assert!(
        !in_comfort_cone(&head(), Vec3::new(0.0, 2.7, -1.0)),
        "too high"
    );
    assert!(
        !in_comfort_cone(&head(), Vec3::new(2.0, 1.7, -1.0)),
        "too far to the side"
    );
}

#[test]
fn panel_hits_and_text() {
    let p = Panel::new(
        Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY),
        Vec2::new(0.4, 0.2),
    );
    let uv = p.ray(Vec3::new(0.1, 1.55, 0.0), Vec3::NEG_Z).unwrap();
    assert!((uv - Vec2::new(0.75, 0.25)).length() < 1e-5, "{uv}");
    assert!(
        p.ray(Vec3::new(0.0, 1.5, -2.0), Vec3::Z).is_none(),
        "from behind"
    );
    assert!(
        p.ray(Vec3::new(1.0, 1.5, 0.0), Vec3::NEG_Z).is_none(),
        "beside"
    );
    assert!(p.touch(Vec3::new(0.0, 1.5, -0.995), 0.01).is_some());
    assert!(p.touch(Vec3::new(0.0, 1.5, -0.95), 0.01).is_none());
    assert!((p.point(Vec2::new(0.0, 0.0)) - Vec3::new(-0.2, 1.6, -1.0)).length() < 1e-5);
    assert_eq!(row_at(Vec2::new(0.5, 0.99), 4), Some(3));
    assert_eq!(row_at(Vec2::new(0.5, 1.0), 4), None);
    assert_eq!(cell_at(Vec2::new(0.55, 0.3), 5, 4), Some((2, 1)));
    assert_eq!(
        wrap("Bring the sword to Diego in Montera", 12),
        ["Bring the", "sword to", "Diego in", "Montera"]
    );
    assert_eq!(
        wrap("Unbelievablylongword", 8),
        ["Unbeliev", "ablylong", "word"]
    );
}

#[test]
fn wrist_vitals_show_when_turned_to_the_eyes() {
    let eye = head().position;
    let at = Vec3::new(-0.15, 1.3, -0.35);
    // The panel faces the grip's +X for the left hand.
    let towards = Quat::from_rotation_arc(Vec3::X, (eye - at).normalize());
    let w = wrist_panel(&Pose::new(at, towards), Hand::Left);
    assert!(wrist_visible(&w, eye));
    let away = Quat::from_rotation_arc(Vec3::X, -(eye - at).normalize());
    assert!(!wrist_visible(
        &wrist_panel(&Pose::new(at, away), Hand::Left),
        eye
    ));
    // Each bar is above the minimum text size at that distance.
    assert!(to_dmm(WRIST_SIZE.y / 3.0, w.pose.position.distance(eye)) >= MIN_TEXT_DMM);
    let v = Vitals {
        health: (30.0, 120.0),
        mana: (80.0, 40.0),
        stamina: (5.0, 0.0),
    };
    assert_eq!(v.fractions(), [0.25, 1.0, 0.0]);
    assert_eq!(health_pulse(0.5, 0.3), 1.0);
    assert!(health_pulse(0.1, 0.3) < 1.0);
}

#[test]
fn quick_wheel_picks_by_direction_and_has_a_deadzone() {
    let mut w = QuickWheel::new(10);
    let c = Vec3::new(0.25, 1.3, -0.4);
    w.open_at(c, &head());
    assert!(!w.belt);
    let r = w.open.unwrap().rotation;
    let at = |deg: f32| {
        let a = deg.to_radians();
        c + r * Vec3::new(a.sin(), a.cos(), 0.0) * 0.1
    };
    assert_eq!(w.update(at(0.0)), Some(0), "top");
    assert_eq!(w.update(at(72.0)), Some(2), "clockwise");
    assert_eq!(w.update(at(-36.0)), Some(9));
    assert_eq!(w.update(c + r * Vec3::X * 0.02), None, "deadzone");
    w.update(at(108.0));
    assert_eq!(w.release(), Some(3));
    assert!(w.open.is_none());
    // Slots sit on the ring around the centre.
    w.open_at(Vec3::new(0.2, 0.95, -0.2), &head());
    assert!(w.belt, "pressed at the hip");
    assert!(
        (w.slot_position(4)
            .unwrap()
            .distance(Vec3::new(0.2, 0.95, -0.2))
            - 0.12)
            .abs()
            < 1e-5
    );
}

fn inventory(n: usize) -> PlayerPanel {
    PlayerPanel {
        title: String::new(),
        rows: (0..n)
            .map(|i| PageRow {
                item: format!("ItMi_Fake_{i}"),
                count: 1,
                key: 100 + i,
                ..Default::default()
            })
            .collect(),
        gold: 0,
    }
}

#[test]
fn backpack_opens_over_the_shoulder_at_touch_distance() {
    let h = head();
    assert!(over_shoulder(&h, Vec3::new(0.15, 1.65, 0.15)));
    assert!(!over_shoulder(&h, Vec3::new(0.2, 1.3, -0.3)), "in front");
    assert!(!over_shoulder(&h, Vec3::new(0.15, 1.0, 0.15)), "at the hip");
    let mut bp = Backpack::open(&h);
    let d = bp.panel.pose.position.distance(h.position);
    assert!(TOUCH_DISTANCE.contains(&d), "{d}");
    assert!(in_comfort_cone(&h, bp.panel.pose.position));
    assert!(faces(&bp.panel, h.position));
    assert!(to_dmm(CELL, d) >= MIN_TARGET_DMM);
    let inv = inventory(25);
    assert_eq!(Backpack::pages(&inv.rows), 2);
    let cell = |bp: &Backpack, i: usize| bp.panel.project(bp.cell_center(i)).0;
    assert_eq!(bp.hit(cell(&bp, 7), &inv), Some(PanelHit::Item(107)));
    assert_eq!(
        bp.hit(Vec2::new(0.25, 0.97), &inv),
        Some(PanelHit::PrevPage)
    );
    assert_eq!(bp.press(PanelHit::NextPage, &inv), None);
    assert_eq!(bp.page, 1);
    assert_eq!(bp.hit(cell(&bp, 2), &inv), Some(PanelHit::Item(122)));
    assert_eq!(bp.hit(cell(&bp, 7), &inv), None, "empty cell");
    bp.press(PanelHit::NextPage, &inv);
    assert_eq!(bp.page, 1, "last page");
}

#[test]
fn dialog_options_float_beside_the_npc_at_laser_distance() {
    let h = head();
    let npc = Vec3::new(0.2, 1.65, -1.6);
    let options: Vec<String> = (0..10).map(|i| format!("Option {i}")).collect();
    let mut dp = DialogPanel::new(options, npc, &h, Hand::Right);
    let d = dp.panel.pose.position.distance(h.position);
    assert!(POINT_DISTANCE.contains(&d), "{d}");
    assert!(faces(&dp.panel, h.position));
    assert!(
        dp.panel.pose.position.x > npc.x + 0.3,
        "beside the NPC, on the right"
    );
    assert!((to_dmm(dp.panel.size.y / VISIBLE_ROWS as f32, d) - ROW_DMM).abs() < 0.1);
    const { assert!(ROW_DMM >= MIN_TARGET_DMM) };
    assert_eq!(dp.rows(), 0..6);
    // Laser at the second row selects it; the trigger confirms it.
    let from = Vec3::new(0.25, 1.3, -0.3);
    assert_eq!(
        dp.point(from, (dp.row_center(1) - from).normalize()),
        Some(1)
    );
    assert_eq!(dp.confirm(), Some(1));
    assert_eq!(dp.point(from, Vec3::NEG_Z), None);
    assert_eq!(dp.confirm(), None, "the laser left the panel");
    // Scroll shows later rows; the laser picks absolute indices.
    dp.scroll(4);
    assert_eq!(dp.rows(), 4..10);
    assert_eq!(
        dp.point(from, (dp.row_center(0) - from).normalize()),
        Some(4)
    );
    // A fingertip on a row chooses it.
    assert_eq!(dp.touch(dp.row_center(5)), Some(9));
    // An NPC right in the face pushes the options out to the minimum distance.
    let close = DialogPanel::new(
        vec!["Hi".into()],
        Vec3::new(0.0, 1.65, -0.4),
        &h,
        Hand::Right,
    );
    assert!(close.panel.pose.position.distance(h.position) >= *POINT_DISTANCE.start() - 1e-4);
}

#[test]
fn subtitles_hang_above_the_speaker_or_fall_back_in_front() {
    let h = head();
    let mut sub = Subtitle::default();
    assert!(layout(&sub, Some(Vec3::ZERO), &h).is_none(), "nothing said");
    sub.start(
        "Du bist also der Neue. Dann hör mir gut zu, bevor du dich hier umsiehst.",
        true,
        false,
        400.0,
    );
    sub.tick(0.3);
    let speaker = Vec3::new(0.0, 1.65, -2.0);
    let v = layout(&sub, Some(speaker), &h).unwrap();
    assert!(v.off_view.is_none());
    assert!(v.pose.position.y > speaker.y + 0.2, "above the head");
    let d = v.pose.position.distance(h.position);
    assert!(POINT_DISTANCE.contains(&d));
    assert!((to_dmm(v.text_height, d) - BODY_TEXT_DMM).abs() < 1e-3);
    assert!(v.lines.len() >= 2 && v.lines.iter().all(|l| l.chars().count() <= 42));
    assert_eq!(v.alpha, 1.0);
    let behind = layout(&sub, Some(Vec3::new(3.0, 1.65, 0.5)), &h).unwrap();
    assert_eq!(behind.off_view, Some(Side::Right));
    assert!(in_comfort_cone(&h, behind.pose.position));
    let narrator = layout(&sub, None, &h).unwrap();
    assert!(narrator.off_view.is_none() && in_comfort_cone(&h, narrator.pose.position));
}

#[test]
fn journal_book_pages_and_swipes() {
    let rows: Vec<QuestRow> = (0..40)
        .map(|i| QuestRow {
            name: format!("q{i}"),
            title: format!("Quest number {i}"),
            updated: 0,
        })
        .collect();
    let mut book = Book::new(gothic_vr_ui::handheld::journal_lines(&rows));
    assert_eq!(book.pages(), 40usize.div_ceil(LINES_PER_PAGE));
    assert_eq!(book.page(0).len(), LINES_PER_PAGE);
    let pose = Pose::new(Vec3::new(0.0, 1.2, -0.35), Quat::IDENTITY);
    let at = |x: f32| pose.transform_point(Vec3::new(x, 0.0, 0.05));
    // Right to left over the pages: next spread.
    assert!(!book.swipe(&pose, at(0.08)));
    assert!(book.swipe(&pose, at(-0.08)));
    assert_eq!(book.spread, 2);
    // No spread after the last one.
    book.swipe(&pose, at(0.08));
    assert_eq!(book.spread, 0, "left to right goes back");
    // Hands away from the book do not turn pages.
    assert!(!book.swipe(&pose, at(0.08) + Vec3::Z * 0.3));
    assert!(page_text_dmm(&pose, head().position) >= MIN_TEXT_DMM);
}

#[test]
fn held_map_places_markers_and_labels_them() {
    let sheet = MapSheet {
        bitmap: "fake.tga".into(),
        name: "Fake".into(),
        top_left: Vec2::new(-1000.0, 1000.0),
        bottom_right: Vec2::new(1000.0, -1000.0),
    };
    let markers = [
        MapMarker::from_entity("MAP_X_Vengard", 500.0, 500.0),
        MapMarker::from_entity("MAP_X_$Secret", 0.0, 0.0),
        MapMarker::from_entity("MAP_X_Far", 5000.0, 0.0),
    ];
    let map = HeldMap::new(sheet, &markers, Vec2::ZERO);
    assert_eq!(map.markers.len(), 1);
    assert_eq!(
        map.markers[0],
        ("Vengard".to_string(), Vec2::new(0.75, 0.25))
    );
    assert_eq!(map.player, Some(Vec2::new(0.5, 0.5)));
    let panel = Panel::new(
        Pose::new(Vec3::new(0.0, 1.2, -0.4), Quat::IDENTITY),
        Vec2::splat(0.3),
    );
    let target = panel.point(Vec2::new(0.76, 0.25));
    let from = target + Vec3::Z * 0.4;
    assert_eq!(map.label_at(&panel, from, Vec3::NEG_Z), Some("Vengard"));
    assert_eq!(
        map.label_at(&panel, panel.point(Vec2::splat(0.1)) + Vec3::Z, Vec3::NEG_Z),
        None
    );
}
