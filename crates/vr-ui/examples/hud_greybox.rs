//! Greybox demo of the VR HUD (VR-10) with fake data: a scripted player turns the wrist,
//! listens to an NPC (subtitle), picks a dialog option, opens the backpack over the shoulder
//! and takes an item, uses a quick slot, reads the journal and looks at the map.
//!
//! ```sh
//! cargo run -p gothic-vr-ui --example hud_greybox                         # window, gizmo HUD
//! cargo run -p gothic-vr-ui --example hud_greybox -- --headless           # CI: exit code
//! cargo run -p gothic-vr-ui --example hud_greybox -- --screenshot hud.png [--screenshot-frame 90]
//! cargo run -p gothic-vr-ui --example hud_greybox -- --dump-panels .local/panels  # each panel's texture
//! cargo run -p gothic-vr-ui --example hud_greybox -- --font /path/to/font.ttf    # HUD font
//! ```
use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_ui::journal::QuestRow;
use gothic_ui::map::{MapMarker, MapSheet};
use gothic_ui::{PageId, PageRow, PlayerPanel, QuickSlot};
use gothic_vr::greybox::GreyboxBox;
use gothic_vr::{Hand, PendingXrFrame, Pose, VrPlugin, VrRenderPlugin, XrBackendKind, XrFrame};
use gothic_vr_ui::hud::fingertip;
use gothic_vr_ui::*;
use std::time::Duration;

const RIGHT: Vec3 = Vec3::new(0.25, 1.3, -0.3);
const LEFT: Vec3 = Vec3::new(-0.15, 1.45, -0.35);
const NPC_HEAD: Vec3 = Vec3::new(0.1, 1.65, -1.8);
const EYE: Vec3 = Vec3::new(0.0, 1.7, 0.0);
const END: u32 = 330;

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless");
    let mut app = App::new();
    if headless {
        app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
            .add_plugins((
                VrPlugin {
                    backend: XrBackendKind::External,
                },
                VrUiPlugin { gizmos: false },
            ));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged VR HUD greybox".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            VrPlugin {
                backend: XrBackendKind::External,
            },
            VrRenderPlugin,
            VrUiPlugin { gizmos: false },
            VrUiRenderPlugin,
        ))
        .add_systems(Startup, (spawn_scene, load_font))
        .add_systems(Update, draw_laser);
    }
    if let Some(i) = args.iter().position(|a| a == "--screenshot")
        && let Some(path) = args.get(i + 1)
    {
        let frame = args
            .iter()
            .position(|a| a == "--screenshot-frame")
            .and_then(|i| args.get(i + 1)?.parse().ok())
            .unwrap_or(90);
        app.insert_resource(gothic_vr::render::CaptureAndExit::new(path.clone(), frame));
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-panels")
        && let Some(dir) = args.get(i + 1)
    {
        app.insert_resource(DumpPanels(dir.into()))
            .add_systems(Last, dump_panels);
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 72.0,
    )))
    .insert_resource(HudDataRes(fake_data()))
    .init_resource::<Script>()
    .add_systems(First, drive)
    .add_systems(Last, check);
    app.run()
}

/// Made-up inventory (template-style names, not game data).
const ITEMS: [(&str, i32); 22] = [
    ("ItMw_Kurzschwert", 1),
    ("ItMw_Streitaxt", 1),
    ("ItRw_Jagdbogen", 1),
    ("ItRw_Pfeil", 48),
    ("ItPo_Heiltrank", 3),
    ("ItPo_Manatrank", 2),
    ("ItFo_Apfel", 6),
    ("ItFo_Brot", 2),
    ("ItFo_Braten", 4),
    ("ItFo_Käse", 1),
    ("ItMi_Fackel", 5),
    ("ItMi_Dietrich", 7),
    ("ItMi_Erzbrocken", 12),
    ("ItMi_Wolfsfell", 3),
    ("ItSc_Feuerball", 2),
    ("ItSc_Heilung", 1),
    ("ItAr_Lederrüstung", 1),
    ("ItAm_Schutzamulett", 1),
    ("ItRi_Kraftring", 1),
    ("ItWr_Brief", 1),
    ("ItWr_Karte", 1),
    ("ItKe_Truhenschlüssel", 1),
];

/// Made-up journal entries.
const QUESTS: [&str; 14] = [
    "Bring dem Schmied drei Erzbrocken aus der alten Mine.",
    "Finde heraus, wer die Schafe des Bauern stiehlt.",
    "Sprich mit dem Wirt über das Gerücht am Turm.",
    "Besorge zehn Wolfsfelle für den Händler.",
    "Begleite den Pilger sicher bis zur Mühle.",
    "Der Jäger sucht jemanden, der Bogenschießen lernen will.",
    "Hol die gestohlene Truhe aus dem Banditenlager zurück.",
    "Überbringe den versiegelten Brief an den Hauptmann.",
    "Lerne bei der Kräuterfrau, wie man Heiltränke braut.",
    "Räume die Höhle hinter dem Wasserfall von Ratten frei.",
    "Finde den verschwundenen Sohn des Fischers.",
    "Kauf dem Schmied ein besseres Schwert ab.",
    "Erkunde die Ruinen nördlich des Dorfes.",
    "Gewinne das Wetttrinken in der Taverne.",
];

fn fake_data() -> HudData {
    HudData {
        vitals: Vitals {
            health: (18.0, 100.0),
            mana: (35.0, 50.0),
            stamina: (80.0, 100.0),
        },
        quick_slots: [
            Some("ItMw_Kurzschwert"),
            Some("ItRw_Jagdbogen"),
            None,
            Some("ItPo_Heiltrank"),
            Some("ItFo_Braten"),
            None,
            Some("ItMi_Fackel"),
            None,
            None,
            Some("ItSc_Feuerball"),
        ]
        .into_iter()
        .map(|s| {
            s.map(|item| QuickSlot {
                item: item.into(),
                count: 1,
            })
        })
        .collect(),
        inventory: PlayerPanel {
            title: "Inventar".into(),
            rows: ITEMS
                .iter()
                .enumerate()
                .map(|(i, (item, count))| PageRow {
                    item: (*item).into(),
                    count: *count,
                    key: i,
                    ..Default::default()
                })
                .collect(),
            gold: 250,
        },
        quests: QUESTS
            .iter()
            .enumerate()
            .map(|(i, title)| QuestRow {
                name: format!("q{i}"),
                title: (*title).into(),
                updated: 0,
            })
            .collect(),
        map: Some((
            MapSheet {
                bitmap: "fake.tga".into(),
                name: "Graukasten".into(),
                top_left: Vec2::new(-2000.0, 2000.0),
                bottom_right: Vec2::new(2000.0, -2000.0),
            },
            vec![
                MapMarker::from_entity("MAP_X_Dorf", 800.0, 600.0),
                MapMarker::from_entity("MAP_X_Turm", -1200.0, -300.0),
                MapMarker::from_entity("MAP_X_Mühle", -500.0, 1300.0),
            ],
            Vec2::ZERO,
        )),
        ..Default::default()
    }
}

#[derive(Resource, Default)]
struct Script {
    frame: u32,
    xr: XrFrame,
    seen: Vec<HudAction>,
    book_turned: bool,
}

fn pose(p: Vec3, r: Quat) -> Option<Pose> {
    Some(Pose::new(p, r))
}

/// The scripted player (frames at 72 Hz) and the fake host reacting to it.
fn drive(
    mut s: ResMut<Script>,
    mut pending: ResMut<PendingXrFrame>,
    mut data: ResMut<HudDataRes>,
    mut hud: ResMut<VrHudRes>,
) {
    let f = s.frame;
    s.frame += 1;
    let x = &mut s.xr;
    let wrist_to_eye = Quat::from_rotation_arc(Vec3::X, (EYE - LEFT).normalize());
    let aim = |p: Vec3| Quat::from_rotation_arc(Vec3::NEG_Z, (p - RIGHT).normalize());
    if f == 0 {
        x.hand_mut(Hand::Left).pose = pose(LEFT, wrist_to_eye);
        x.hand_mut(Hand::Right).pose = pose(RIGHT, Quat::IDENTITY);
    }
    let d = &mut data.0;
    match f {
        10 => {
            d.speaker_head = Some(NPC_HEAD);
            d.subtitle.start(
                "Du siehst aus, als hättest du einen langen Weg hinter dir. Was willst du?",
                true,
                false,
                500.0,
            );
        }
        40 => {
            d.dialog = Some(DialogData {
                options: vec![
                    "Wer bist du?".into(),
                    "Ich suche Arbeit.".into(),
                    "Was gibt es hier zu tun?".into(),
                    "Ich muss weiter.".into(),
                ],
                npc_head: NPC_HEAD,
            })
        }
        60 => {
            if let Some(dp) = &hud.0.dialog {
                x.hand_mut(Hand::Right).pose = pose(RIGHT, aim(dp.row_center(1)));
            }
        }
        100 => x.hand_mut(Hand::Right).trigger = 1.0,
        104 => x.hand_mut(Hand::Right).trigger = 0.0,
        110 => {
            d.dialog = None;
            d.subtitle.stop();
            x.hand_mut(Hand::Right).pose = pose(Vec3::new(0.15, 1.65, 0.15), Quat::IDENTITY);
            x.hand_mut(Hand::Left).pose = pose(LEFT, Quat::IDENTITY);
        }
        115 | 165 => x.hand_mut(Hand::Right).grip = 1.0,
        120 | 170 => x.hand_mut(Hand::Right).grip = 0.0,
        130 => {
            if let Some(bp) = &hud.0.backpack {
                let tip = fingertip(&Pose::new(Vec3::ZERO, Quat::IDENTITY));
                x.hand_mut(Hand::Right).pose = pose(bp.cell_center(2) - tip, Quat::IDENTITY);
            }
        }
        160 => x.hand_mut(Hand::Right).pose = pose(Vec3::new(0.15, 1.65, 0.15), Quat::IDENTITY),
        175 => {
            // Look down at the hands for the wheel, the book and the map.
            x.head.rotation = Quat::from_rotation_x(-0.5);
        }
        180 => {
            x.hand_mut(Hand::Right).pose = pose(RIGHT, Quat::IDENTITY);
            x.hand_mut(Hand::Left).primary = true;
        }
        185 => {
            if let Some(p) = hud.0.wheel.slot_position(3) {
                x.hand_mut(Hand::Right).pose = pose(p, Quat::IDENTITY);
            }
        }
        195 => x.hand_mut(Hand::Left).primary = false,
        210 => {
            let out = hud.0.open_page(Some(PageId::Quests));
            s.seen.extend(out);
            let x = &mut s.xr;
            x.hand_mut(Hand::Left).pose = pose(Vec3::new(-0.05, 1.2, -0.3), Quat::IDENTITY);
        }
        230..=250 => {
            // Swipe right to left over the book.
            if let Some((_, p)) = &hud.0.book {
                let k = (f - 230) as f32 / 20.0;
                let at = p.transform_point(Vec3::new(0.1 - 0.2 * k, 0.0, 0.05));
                x.hand_mut(Hand::Right).pose = pose(at, Quat::IDENTITY);
            }
        }
        260 => {
            let out = hud.0.open_page(None);
            s.seen.extend(out);
            hud.0.map_open = true;
            s.xr.hand_mut(Hand::Right).pose = pose(RIGHT, Quat::IDENTITY);
        }
        270 => {
            if let Some((map, panel)) = &hud.0.map
                && let Some((_, uv)) = map.markers.first()
            {
                let p = panel.point(*uv);
                s.xr.hand_mut(Hand::Right).pose = pose(RIGHT, aim(p));
            }
        }
        _ => {}
    }
    pending.0 = Some(s.xr);
}

fn check(
    mut s: ResMut<Script>,
    hud: Res<VrHudRes>,
    mut msgs: MessageReader<HudActionMsg>,
    mut exit: MessageWriter<AppExit>,
) {
    for m in msgs.read() {
        println!("frame {:3}: {:?}", s.frame, m.0);
        s.seen.push(m.0);
    }
    if s.frame == 275
        && let Some(l) = &hud.0.map_label
    {
        println!("frame {:3}: map label under the laser: {l}", s.frame);
    }
    if s.frame == 255
        && let Some((book, _)) = &hud.0.book
    {
        println!("frame {:3}: journal spread {}", s.frame, book.spread);
        s.book_turned = book.spread == 2;
    }
    if s.frame >= END {
        let has = |a: HudAction| s.seen.contains(&a);
        let ok = has(HudAction::Page(Some(PageId::Dialog)))
            && has(HudAction::ChooseOption(1))
            && has(HudAction::Page(Some(PageId::Inventory)))
            && has(HudAction::InventoryItem(2))
            && has(HudAction::UseQuickSlot(3))
            && has(HudAction::Page(Some(PageId::Quests)))
            && s.book_turned
            && hud.0.map_label.as_deref() == Some("Dorf");
        println!("hud greybox: {}", if ok { "OK" } else { "FAILED" });
        exit.write(if ok {
            AppExit::Success
        } else {
            AppExit::from_code(1)
        });
    }
}

#[derive(Resource)]
struct DumpPanels(std::path::PathBuf);

/// Save every visible panel texture at a few frames of the script (one per part).
fn dump_panels(
    mut commands: Commands,
    dir: Res<DumpPanels>,
    script: Res<Script>,
    surfaces: Query<(&gothic_vr_ui::render::Surface, &Visibility)>,
) {
    if ![30, 90, 145, 190, 245, 285].contains(&script.frame) {
        return;
    }
    let _ = std::fs::create_dir_all(&dir.0);
    for (s, vis) in &surfaces {
        if *vis == Visibility::Visible {
            let path = dir.0.join(format!("{:?}_{}.png", s.kind, script.frame));
            commands
                .spawn(bevy::render::view::screenshot::Screenshot::image(
                    s.image().clone(),
                ))
                .observe(bevy::render::view::screenshot::save_to_disk(path));
        }
    }
}

/// The main hand's pointer ray (the HUD itself is drawn by `VrUiRenderPlugin`).
fn draw_laser(input: Res<gothic_vr::VrInputRes>, mut gizmos: Gizmos) {
    if let Some(p) = input.0.hand_pose(Hand::Right) {
        gizmos.line(
            p.position,
            p.position + p.forward() * 2.5,
            Color::srgba(1.0, 0.85, 0.4, 0.6),
        );
    }
}

/// A system font with umlauts for the HUD (`--font <file>` or a common system path); the
/// game host passes its own `HudFont` instead.
fn load_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let args: Vec<String> = std::env::args().collect();
    let given = args
        .iter()
        .position(|a| a == "--font")
        .and_then(|i| args.get(i + 1).cloned());
    let candidates = given.into_iter().chain(
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "C:/Windows/Fonts/segoeui.ttf",
            "C:/Windows/Fonts/arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
        .map(String::from),
    );
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            info!("HUD font: {path}");
            commands.insert_resource(HudFont(fonts.add(Font::from_bytes(bytes))));
            return;
        }
    }
    warn!("no system font found; umlauts will show as boxes (pass --font <file>)");
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let boxes = [
        GreyboxBox::new(
            "floor",
            Vec3::new(0.0, -0.05, -2.0),
            Vec3::new(4.0, 0.05, 4.0),
        ),
        GreyboxBox::new(
            "npc",
            Vec3::new(0.1, 0.85, -1.8),
            Vec3::new(0.2, 0.85, 0.15),
        )
        .colored([0.6, 0.45, 0.3]),
        GreyboxBox::new("wall", Vec3::new(0.0, 1.5, -5.0), Vec3::new(4.0, 1.5, 0.1)),
    ];
    gothic_vr::render::spawn_greybox(&mut commands, &mut meshes, &mut materials, &boxes);
    gothic_vr::render::spawn_greybox_lighting(&mut commands);
}
