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
        .add_systems(Update, (demo_system, use_item_toast))
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
        270 => shot(&mut commands, "hud"),
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
        400 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
