//! Greybox demo of the VR HUD (VR-10) with fake data: a scripted player turns the wrist,
//! listens to an NPC (subtitle), picks a dialog option, opens the backpack over the shoulder
//! and takes an item, uses a quick slot, reads the journal and looks at the map.
//!
//! ```sh
//! cargo run -p gothic-vr-ui --example hud_greybox                         # window, gizmo HUD
//! cargo run -p gothic-vr-ui --example hud_greybox -- --headless           # CI: exit code
//! cargo run -p gothic-vr-ui --example hud_greybox -- --screenshot hud.png [--screenshot-frame 90]
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
            VrUiPlugin::default(),
        ))
        .add_systems(Startup, spawn_scene);
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
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 72.0,
    )))
    .insert_resource(HudDataRes(fake_data()))
    .init_resource::<Script>()
    .add_systems(First, drive)
    .add_systems(Last, check);
    app.run()
}

fn fake_data() -> HudData {
    HudData {
        vitals: Vitals {
            health: (18.0, 100.0),
            mana: (35.0, 50.0),
            stamina: (80.0, 100.0),
        },
        quick_slots: (0..10)
            .map(|i| {
                (i % 3 == 0).then(|| QuickSlot {
                    item: format!("ItFake_{i}"),
                    count: 1,
                })
            })
            .collect(),
        inventory: PlayerPanel {
            title: "Inventar".into(),
            rows: (0..27)
                .map(|i| PageRow {
                    item: format!("ItFake_{i}"),
                    count: 1 + i as i32 % 4,
                    key: i,
                    ..Default::default()
                })
                .collect(),
            gold: 250,
        },
        quests: (0..30)
            .map(|i| QuestRow {
                name: format!("q{i}"),
                title: format!("Fake quest number {i} with a longer title"),
                updated: 0,
            })
            .collect(),
        map: Some((
            MapSheet {
                bitmap: "fake.tga".into(),
                name: "Greybox".into(),
                top_left: Vec2::new(-2000.0, 2000.0),
                bottom_right: Vec2::new(2000.0, -2000.0),
            },
            vec![
                MapMarker::from_entity("MAP_X_Village", 800.0, 600.0),
                MapMarker::from_entity("MAP_X_Tower", -1200.0, -300.0),
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
            && hud.0.map_label.as_deref() == Some("Village");
        println!("hud greybox: {}", if ok { "OK" } else { "FAILED" });
        exit.write(if ok {
            AppExit::Success
        } else {
            AppExit::from_code(1)
        });
    }
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
