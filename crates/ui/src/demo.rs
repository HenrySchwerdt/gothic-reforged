//! Stand-alone UI demo: `--ui-demo NPC` opens a real dialog with the NPC on a plain
//! background; L / I / C / T open journal / inventory / character / trade.
//! Flags: `--data DIR` (game Data dir), `--install DIR`, `--items FILE` (item-dump json),
//! `--shots` (scripted run writing `.local/ui-*.png`, then exit).
use crate::*;
use anyhow::{Result, anyhow};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::WindowResolution;
use gothic_gameplay::model::QuestStatus;

#[derive(Resource)]
struct Demo {
    npc: String,
    shots: bool,
    frame: u32,
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Pre-fill a plausible mid-game state so every screen has content.
fn demo_state(db: &GameDatabase, items: &ItemCatalog) -> GameState {
    let mut st = GameState::new();
    st.add_xp(2600);
    st.learning_points = 25;
    for (a, v) in [
        ("str", 120),
        ("dex", 85),
        ("int", 40),
        ("smt", 10),
        ("alc", 25),
        ("thf", 5),
    ] {
        st.attributes.insert(a.into(), v);
    }
    st.skills.insert("perk_1h_1".into());
    st.skills.insert("perk_1h_2".into());
    st.skills.insert("perk_learn".into());
    st.add_item("it_gold", 350);
    let mut want: Vec<&str> = vec![
        "It_1H_Sword_01",
        "It_2H_Axe_01",
        "It_Bow_01",
        "It_Arrow",
        "It_Apple",
        "It_Bread",
        "It_Armor_Ass_M",
        "It_Scroll_Icelance",
        "It_Map_World",
        "It_Key_CPT_01",
        "It_Plant_Beliarboost",
        "It_Staff_JudgementStaff_01",
    ];
    // add some real item names of every category so the tabs are populated
    let mut seen = [0u32; 11];
    let mut extra: Vec<String> = Vec::new();
    for (name, info) in items_iter(items) {
        let c = info.category.clamp(0, 10) as usize;
        if info.category > 0
            && info.value > 0
            && seen[c] < 4
            && db.text(&format!("FO_{name}")).is_some()
            && !name.contains("_Test")
        {
            seen[c] += 1;
            extra.push(name.to_string());
        }
    }
    extra.sort();
    for n in &extra {
        want.push(n);
    }
    for (i, n) in want.iter().enumerate() {
        if items.get(n).is_some() {
            st.add_item(n, if i % 4 == 0 { 5 } else { 1 });
        }
    }
    let titled: Vec<&str> = db
        .quests
        .iter()
        .filter(|q| db.text(&q.log_topic).is_some())
        .map(|q| q.name.as_str())
        .take(60)
        .collect();
    for (i, q) in titled.iter().step_by(6).take(9).enumerate() {
        st.set_quest_status(
            q,
            match i % 3 {
                0 => QuestStatus::Running,
                1 => QuestStatus::Success,
                _ => QuestStatus::Failed,
            },
        );
    }
    st.add_fame("pol:reb", 40);
    st.add_fame("pol:ass", 15);
    st.add_fame("enc:ardea", 60);
    st
}

fn items_iter(items: &ItemCatalog) -> impl Iterator<Item = (&str, &ItemInfo)> {
    items.iter()
}

/// Report how many item templates resolve to a `g3.igl` icon tile (no window).
fn icon_coverage(install: &std::path::Path, items: &ItemCatalog) -> Result<()> {
    let igl = crate::assets::load_igl(install);
    let (mut total, mut ok, mut empty) = (0, 0, 0);
    for (name, info) in items_iter(items) {
        total += 1;
        match info.icon.as_deref() {
            None | Some("") => empty += 1,
            Some(i) if igl.contains_key(&gothic_formats::igl::bare_key(i)) => ok += 1,
            Some(i) => println!("MISS {name}: {i}"),
        }
    }
    println!(
        "templates {total}: icon resolved {ok}, no icon string {empty}, igl tiles {}",
        igl.len()
    );
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let npc = flag(args, "--ui-demo")
        .filter(|s| !s.starts_with("--"))
        .unwrap_or("Hamlar")
        .to_string();
    let default_install = UiConfig::default().install_dir;
    let install = flag(args, "--install").map_or(default_install, std::path::PathBuf::from);
    let data = flag(args, "--data").map_or_else(|| install.join("Data"), std::path::PathBuf::from);
    let items_path = flag(args, "--items").unwrap_or(".local/items.json");
    let db = GameDatabase::load_install(&data, "German")
        .map_err(|e| anyhow!("loading game database: {e:#}"))?;
    let items = ItemCatalog::load(std::path::Path::new(items_path))
        .map_err(|e| anyhow!("{items_path}: {e:#} (run `gothic-inspector item-dump` first)"))?;
    if args.iter().any(|a| a == "--icon-coverage") {
        return icon_coverage(&install, &items);
    }
    let state = demo_state(&db, &items);
    let shots = args.iter().any(|a| a == "--shots");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / UI demo".into(),
                resolution: WindowResolution::new(1024, 768),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.20, 0.23, 0.26)))
        .insert_resource(UiConfig {
            install_dir: install,
            spawn_camera: true,
            ..default()
        })
        .insert_resource(GameSession::new(db, state, items))
        .insert_resource(PlayerVitals {
            hp: 310.0,
            hp_max: 400.0,
            mp: 40.0,
            mp_max: 100.0,
            sp: 85.0,
            sp_max: 100.0,
        })
        .insert_resource(FocusHealth(Some(0.55)))
        .insert_resource(Demo {
            npc,
            shots,
            frame: 0,
        })
        .add_plugins(GothicUiPlugin)
        .add_message::<DemoLog>()
        .add_systems(Update, (demo_system, use_item_toast, feed_demo_log))
        .run();
    Ok(())
}

fn use_item_toast(
    mut r: MessageReader<UseItem>,
    s: Res<GameSession>,
    mut n: MessageWriter<Notify>,
) {
    for u in r.read() {
        n.write(Notify {
            text: format!("Benutzt: {}", style::item_name(&s.db, &u.item)),
            kind: ToastKind::Item,
        });
    }
}

/// Writers of the E33 / E34 demo steps.
#[derive(bevy::ecs::system::SystemParam)]
struct Extra<'w> {
    logs: MessageWriter<'w, DemoLog>,
    screen_out: MessageWriter<'w, PrintScreen>,
    floats: MessageWriter<'w, FloatingText>,
    label: ResMut<'w, FocusLabel>,
}

/// Demo-only: a line for the HUD log (the log has no message of its own; the host pushes into
/// the `HudLog` resource directly).
#[derive(Message, Clone)]
struct DemoLog(LogCategory, String);

fn demo_log(c: LogCategory, t: &str) -> DemoLog {
    DemoLog(c, t.to_string())
}

fn feed_demo_log(
    mut r: MessageReader<DemoLog>,
    settings: Res<LogSettings>,
    mut log: ResMut<HudLog>,
) {
    for DemoLog(c, t) in r.read() {
        log.push(&settings, *c, t.clone());
    }
}

fn shot(commands: &mut Commands, name: &str) {
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(format!(".local/ui-{name}.png")));
}

#[allow(clippy::too_many_arguments)]
fn demo_system(
    mut commands: Commands,
    mut demo: ResMut<Demo>,
    ui: Res<UiState>,
    s: Res<GameSession>,
    mut start: MessageWriter<StartDialog>,
    mut open: MessageWriter<OpenScreen>,
    mut close: MessageWriter<CloseScreen>,
    mut choose: MessageWriter<ChooseOption>,
    mut notify: MessageWriter<Notify>,
    mut trade: MessageWriter<OpenTrade>,
    mut exit: MessageWriter<AppExit>,
    mut act: MessageWriter<Act>,
    mut modal: MessageWriter<ShowModal>,
    mut slots: MessageWriter<SlotList>,
    mut pages: MessageWriter<ShowPage>,
    mut extra: Extra,
) {
    demo.frame += 1;
    let f = demo.frame;
    if f == 2 {
        // the five slots of the original quick bar reference (.local/ref/orig-hud-tip.png)
        let slots: Vec<Option<QuickSlot>> = [
            ("It_1H_OrcSlayer_01", 1),
            ("It_Torch_Burning", 1),
            ("It_Bow_Hunting", 1),
            ("Arrow", 50),
            ("It_Potion_Health", 10),
        ]
        .iter()
        .map(|(k, n)| {
            s.items.get(k).map(|_| QuickSlot {
                item: (*k).to_string(),
                count: *n,
            })
        })
        .collect();
        commands.insert_resource(QuickSlots(slots));
        // the worn weapon the inventory tooltip compares against: the last weapon in the bag
        let worn = s
            .state
            .inventory
            .keys()
            .rfind(|k| s.items.get(k).is_some_and(|i| i.category == 1))
            .cloned();
        if let Some(w) = worn {
            commands.insert_resource(EquippedItems([(1, w)].into_iter().collect()));
        }
        // the HUD log shows everything in the demo
        commands.insert_resource(LogSettings {
            enabled: true,
            category_enabled: [true; 6],
            ..LogSettings::default()
        });
        commands.insert_resource(map::MapData {
            sheet: Some(map::MapSheet {
                bitmap: "g3_map_world.tga".into(),
                name: "Myrtana".into(),
                top_left: Vec2::new(-60000.0, 60000.0),
                bottom_right: Vec2::new(60000.0, -60000.0),
            }),
            markers: vec![
                map::MapMarker::from_entity("Map_Marker_Ardea", 20000.0, -30000.0),
                map::MapMarker::from_entity("Map_Marker_Gotha", -10000.0, 25000.0),
                map::MapMarker::from_entity("Map_Marker_$Hidden", 0.0, 0.0),
            ],
            player: Vec2::new(18000.0, -26000.0),
        });
    }
    if f == 5 {
        start.write(StartDialog {
            npc: demo.npc.clone(),
        });
    }
    // T without a trade partner: show the player's own stock on the right
    if !demo.shots || f == 1 {
        return;
    }
    let menu_len = ui.dialog.as_ref().map_or(0, |d| d.menu.len());
    match f {
        20 => {
            choose.write(ChooseOption {
                index: 11.min(menu_len.saturating_sub(1)),
            });
            notify.write(Notify {
                text: "Neue Aufgabe: Nach Reddock gehen".into(),
                kind: ToastKind::Quest,
            });
            notify.write(Notify {
                text: "+50 EP".into(),
                kind: ToastKind::Xp,
            });
        }
        60 => shot(&mut commands, "dialog"),
        70 => {
            close.write(CloseScreen);
        }
        75 => {
            open.write(OpenScreen(Screen::Journal));
        }
        110 => shot(&mut commands, "journal"),
        115 => {
            open.write(OpenScreen(Screen::Inventory));
        }
        118 => {
            let k = s
                .state
                .inventory
                .keys()
                .find(|k| s.items.get(k).is_some_and(|i| i.category == 1));
            if let Some(k) = k {
                act.write(Act(Action::SelectItem(k.clone())));
            }
        }
        150 => shot(&mut commands, "inventory"),
        155 => {
            open.write(OpenScreen(Screen::Character));
        }
        190 => shot(&mut commands, "character"),
        195 => {
            let stock = s
                .state
                .inventory
                .iter()
                .rev()
                .take(8)
                .map(|(k, n)| (k.clone(), *n))
                .collect();
            trade.write(OpenTrade {
                npc: demo.npc.clone(),
                stock,
            });
        }
        230 => shot(&mut commands, "trade"),
        235 => {
            close.write(CloseScreen);
        }
        262 => extra
            .label
            .show("Krieger", FocusAttitude::Hostile, 484.0, 364.0),
        270 => shot(&mut commands, "hud"),
        272 => extra.label.clear(),
        275 => {
            open.write(OpenScreen(Screen::MainMenu));
        }
        310 => shot(&mut commands, "menu"),
        315 => {
            modal.write(ShowModal(Modal::Confirm {
                tag: "new".into(),
                text: "Möchten Sie wirklich ein neues Spiel anfangen?".into(),
                yes: "Ja".into(),
                no: "Nein".into(),
            }));
        }
        345 => shot(&mut commands, "confirm"),
        350 => {
            close.write(CloseScreen);
            act.write(Act(Action::Modal(false)));
        }
        355 => {
            modal.write(ShowModal(Modal::Tip {
                title: "Tipp".into(),
                text: "Am unteren Bildschirmrand befindet sich die Schnellleiste. Durch Drücken der entsprechenden Nummerntaste wird der jeweilige Gegenstand benutzt oder die Waffe gezogen.

Drücke jetzt die Taste '1', um das Schwert zu ziehen.".into(),
            }));
        }
        385 => shot(&mut commands, "tip"),
        390 => {
            act.write(Act(Action::Modal(false)));
            act.write(Act(Action::Close));
            open.write(OpenScreen(Screen::Files));
        }
        394 => {
            let row = |n: usize, kind, ms: u64| SlotRow {
                key: format!("slot_{n:02}.g3rs"),
                name: format!("slot_{n:02}"),
                kind,
                saved_unix: 1_700_000_000 + n as u64,
                game_ms: ms,
                play_ms: ms / 24,
                region: "G3_World_01_Ardea".into(),
                level: 4 + n as u32,
            };
            slots.write(SlotList {
                rows: vec![
                    row(1, SlotKind::Quick, 90_000_000),
                    row(2, SlotKind::Manual, 200_000_000),
                    row(3, SlotKind::Auto, 310_000_000),
                ],
                can_save: true,
            });
            act.write(Act(Action::FileSelect(Some(1))));
        }
        420 => shot(&mut commands, "files"),
        425 => {
            act.write(Act(Action::Open(Screen::Video)));
        }
        460 => shot(&mut commands, "video"),
        465 => {
            act.write(Act(Action::Close));
            let ing = |n: &str, need, have| Ingredient {
                item: n.into(),
                name: n.replace('_', " "),
                need,
                have,
            };
            pages.write(ShowPage(Some(PageView::Recipe(RecipeView {
                title: "Alchemie".into(),
                recipes: vec![
                    RecipeRow {
                        key: 0,
                        product: "It_Potion_Health".into(),
                        name: "Trank der Lebenskraft".into(),
                        description: "Dieser Trank ist echt Klasse.".into(),
                        ingredients: vec![ing("It_Plant_Healing", 2, 3), ing("It_Water", 1, 0)],
                        requirement: "Alchemie 10".into(),
                        craftable: false,
                    },
                    RecipeRow {
                        key: 1,
                        product: "It_Torch_Burning".into(),
                        name: "Fackel".into(),
                        ..default()
                    },
                ],
                gold: 1234,
                hint: String::new(),
            }))));
        }
        500 => shot(&mut commands, "recipe"),
        505 => {
            pages.write(ShowPage(Some(PageView::Document(DocumentView {
                title: "Brief an die Korinther".into(),
                left: "Moin Jungs, was geht ab? Ich hoffe, ihr seid wohlauf und die Ernte war gut. Hier im Norden ist es kalt geworden.

Grüße, Bob".into(),
                right: String::new(),
                is_book: false,
            }))));
        }
        540 => shot(&mut commands, "letter"),
        545 => {
            pages.write(ShowPage(Some(PageView::Document(DocumentView {
                title: "Die Ankunft des weißen Mannes".into(),
                left: "Es war einmal ein Mann, der zog aus, um die Welt zu sehen. ".repeat(14),
                right: "Und dann kam der Winter. ".repeat(20),
                is_book: true,
            }))));
        }
        580 => shot(&mut commands, "book"),
        585 => {
            act.write(Act(Action::Close));
            act.write(Act(Action::Open(Screen::Options)));
        }
        600 => shot(&mut commands, "options"),
        605 => {
            act.write(Act(Action::Open(Screen::Audio)));
            act.write(Act(Action::Opt(OptAction::AudioVolume(
                settings::Volume::Music,
                -4,
            ))));
        }
        625 => shot(&mut commands, "audio"),
        630 => {
            act.write(Act(Action::Open(Screen::Controls)));
            act.write(Act(Action::Opt(OptAction::Sensitivity(
                optpages::Axis::X,
                3,
            ))));
            act.write(Act(Action::Opt(OptAction::Invert(optpages::Axis::Y))));
        }
        650 => shot(&mut commands, "controls"),
        655 => {
            act.write(Act(Action::Open(Screen::ControlsCustom)));
            act.write(Act(Action::Opt(OptAction::KeySelect(1, 0))));
        }
        675 => shot(&mut commands, "keys"),
        680 => {
            act.write(Act(Action::Open(Screen::Difficulty)));
            act.write(Act(Action::Opt(OptAction::Difficulty(
                settings::Difficulty::Hard,
            ))));
        }
        700 => shot(&mut commands, "difficulty"),
        705 => {
            act.write(Act(Action::Close));
            act.write(Act(Action::Open(Screen::Map)));
        }
        735 => shot(&mut commands, "map"),
        740 => {
            act.write(Act(Action::Close));
            for (c, t) in [
                (LogCategory::SystemInfo, "Spiel gespeichert."),
                (LogCategory::Battle1, "Held trifft Scavenger (24 Schaden)"),
                (LogCategory::SystemError, "Fehler: Skript nicht gefunden"),
            ] {
                extra.logs.write(demo_log(c, t));
            }
            extra.screen_out.write(PrintScreen {
                text: "Aufgabe erfüllt
[color=ffd040ff]Der Kampf um den Titel"
                    .into(),
                color: Color::WHITE,
                seconds: 30.0,
            });
            extra.floats.write(FloatingText {
                row: 0,
                text: "Stufe 5 erreicht!".into(),
                color: Color::srgb(1.0, 0.85, 0.3),
                flags: texteffect::flags::CENTER | texteffect::flags::FADE_IN,
                seconds: 30.0,
            });
        }
        790 => shot(&mut commands, "overlays"),
        795 => {
            act.write(Act(Action::Open(Screen::Journal)));
        }
        830 => shot(&mut commands, "journal-world"),
        835 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
