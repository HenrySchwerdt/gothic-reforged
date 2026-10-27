//! Renders every VR window (trade, loot, recipes, document, quest log, character) with
//! synthetic data and saves each surface texture, plus the pointer-hover state of one row.
//!
//! ```sh
//! cargo run -p gothic-vr-ui --example window_sheets -- --out .local/quest/vrui [--font file.ttf]
//! ```
//! `tools/vrui_contact.py`-style contact sheets are assembled from these PNGs by the caller.
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_ui::{
    DocumentView, Ingredient, LootView, PageRow, PlayerPanel, RecipeRow, RecipeView, TradeView,
};
use gothic_vr::{Hand, PendingXrFrame, Pose, VrPlugin, VrRenderPlugin, XrBackendKind, XrFrame};
use gothic_vr_ui::views::QuestEntry;
use gothic_vr_ui::*;
use std::time::Duration;

const EYE: Vec3 = Vec3::new(0.0, 1.7, 0.0);
const RIGHT: Vec3 = Vec3::new(0.25, 1.3, -0.3);
const PER_WINDOW: u32 = 90;

fn row(item: &str, count: i32, price: i32, key: usize) -> PageRow {
    PageRow {
        item: item.into(),
        count,
        price,
        key,
        ..Default::default()
    }
}

fn windows() -> Vec<(&'static str, WindowView)> {
    let goods = [
        ("It_1H_Sword_01", 1, 640),
        ("It_2H_Axe_Lumberjack", 1, 1200),
        ("It_Bow_01", 1, 420),
        ("Arrow", 40, 2),
        ("It_Potion", 5, 45),
        ("It_Bread", 12, 3),
        ("It_Bacon", 8, 8),
        ("It_Torch", 6, 4),
        ("It_Armor_Ass_L", 1, 800),
        ("It_Scroll", 2, 150),
    ];
    let mine = [
        ("It_Wolf_Fur", 4, 12),
        ("It_Ore", 9, 20),
        ("It_Apple", 3, 1),
        ("It_Alligator_Leather", 7, 5),
    ];
    let trade = TradeView {
        vendor: "Hamlar".into(),
        vendor_rows: goods
            .iter()
            .enumerate()
            .map(|(i, g)| row(g.0, g.1, g.2, i))
            .collect(),
        vendor_gold: 420,
        player: PlayerPanel {
            title: String::new(),
            rows: mine
                .iter()
                .enumerate()
                .map(|(i, g)| row(g.0, g.1, g.2, 20 + i))
                .collect(),
            gold: 230,
        },
        ask_rows: vec![row("It_Potion", 2, 45, 4)],
        offer_rows: vec![row("It_Wolf_Fur", 3, 12, 20)],
        ask_total: 90,
        offer_total: 36,
        deal: false,
        balance: true,
        hint: "Das ist kein Handelsgut".into(),
    };
    let loot = LootView {
        title: "Alte Truhe".into(),
        rows: vec![
            row("It_Gold", 85, 0, 0),
            row("It_Potion", 2, 0, 1),
            row("It_Bernado_Letter", 1, 0, 2),
            row("It_Alligator_Leather", 3, 0, 3),
        ],
        pool_gold: Some(85),
        take_all: true,
        player: PlayerPanel::default(),
        hint: String::new(),
    };
    let ing = |item: &str, name: &str, need, have| Ingredient {
        item: item.into(),
        name: name.into(),
        need,
        have,
    };
    let recipe = RecipeView {
        title: "Amboss".into(),
        recipes: vec![
            RecipeRow {
                key: 0,
                product: "It_1H_Sword_01".into(),
                name: "Kurzschwert".into(),
                description: "Eine einfache, gut ausgewogene Klinge für den Anfang.".into(),
                ingredients: vec![
                    ing("It_IronOre", "Stahlbarren", 2, 3),
                    ing("It_IronOreNugget", "Holz", 1, 0),
                ],
                requirement: "Schmiedekunst 1".into(),
                craftable: false,
            },
            RecipeRow {
                key: 1,
                product: "It_2H_Axe_Lumberjack".into(),
                name: "Streitaxt".into(),
                description: "Schwer und verheerend.".into(),
                ingredients: vec![ing("It_IronOre", "Stahlbarren", 3, 3)],
                requirement: String::new(),
                craftable: true,
            },
            RecipeRow {
                key: 2,
                name: "Dolch".into(),
                product: "It_Sword".into(),
                craftable: true,
                ..Default::default()
            },
        ],
        gold: 230,
        hint: String::new(),
    };
    let doc = DocumentView {
        title: "Das Tagebuch des Jägers".into(),
        left: "Der Winter kam früh in diesem Jahr. Die Wölfe wagen sich bis an die Palisade, und \
               niemand im Dorf will mehr nach Einbruch der Dunkelheit vor die Tür. Ich habe die \
               Fallen am alten Weg erneuert und die Felle zum Händler gebracht, doch er zahlt \
               kaum die Hälfte dessen, was sie wert sind."
            .repeat(2),
        right: String::new(),
        is_book: false,
    };
    let quests = QuestLogView {
        quests: [
            ("Die Wölfe von Ardea", 0),
            ("Erz für den Schmied", 0),
            ("Briefe an den Hauptmann", 0),
            ("Das gestohlene Schaf", 1),
            ("Der Schmuggler", 2),
        ]
        .iter()
        .enumerate()
        .map(|(i, (t, tab))| QuestEntry {
            name: format!("q{i}"),
            title: (*t).into(),
            tab: *tab,
            log: vec![
                "Der Jäger bittet dich, die Wölfe zu vertreiben, die das Dorf bedrohen. Such ihre Höhle im Norden."
                    .into(),
                "Aufgabe begonnen".into(),
            ],
        })
        .collect(),
        clock: "1. Tag, 12:23".into(),
        hour: 12,
        with_quest: vec!["ardea".into(), "faring".into()],
        reputation: vec![("Ruf in Nordmar".into(), 0), ("Ruf bei den Orks".into(), 0), ("Ruf bei den Rebellen".into(), 0), ("Ruf beim Waldvolk".into(), 0), ("Ruf bei den Assassinen".into(), 0), ("Ruf bei den Nomaden".into(), 0)],
    };
    let character = CharacterView {
        level: 4,
        xp: 3200,
        xp_next: 7500,
        learning_points: 20,
        vitals: Vitals {
            health: (140.0, 200.0),
            mana: (20.0, 50.0),
            stamina: (90.0, 150.0),
        },
        attributes: vec![
            ("Stärke".into(), 120),
            ("Geschick".into(), 90),
            ("Wissen".into(), 10),
            ("Schmiedekunst".into(), 10),
            ("Diebeskunst".into(), 10),
            ("Alchemie".into(), 10),
        ],
        protection: vec![12, 8, 5, 2, 0, 0],
        skills: [(0, "Einhänder 1"), (0, "Bogen 1"), (1, "Jagen 1"), (1, "Schleichen 1"), (3, "Schmieden 1")]
            .iter()
            .map(|(g, s)| (*g, (*s).into()))
            .collect(),
        equipment: vec![
            ("Rechts".into(), "It_1H_Sword_01".into()),
            ("Körper".into(), "It_Armor_Ass_L".into()),
        ],
    };
    vec![
        ("trade", WindowView::Trade(trade)),
        ("loot", WindowView::Loot(loot)),
        ("recipe", WindowView::Recipe(recipe)),
        ("document", WindowView::Document(doc)),
        ("quests", WindowView::Quests(quests)),
        ("character", WindowView::Character(character)),
        ("menu", WindowView::Menu(MenuView { in_game: true })),
        ("options", WindowView::Options(VrComfortOptions::default())),
        ("wait", WindowView::Wait(WaitView::default())),
        (
            "files",
            WindowView::Files(FilesView {
                save_mode: false,
                can_save: true,
                rows: vec![
                    FileRowView { key: "quick.json".into(), title: "Schnellspeicherung".into(), clock: "1. Tag, 12:23".into(), manual: false },
                    FileRowView { key: "auto_1.json".into(), title: "Ardea".into(), clock: "1. Tag, 11:50".into(), manual: false },
                    FileRowView { key: "manual_5.json".into(), title: "Vor dem Kampf".into(), clock: "2. Tag, 08:00".into(), manual: true },
                ],
            }),
        ),
    ]
}

#[derive(Resource)]
struct Out(std::path::PathBuf);

#[derive(Resource, Default)]
struct Script {
    frame: u32,
    list: Vec<(&'static str, WindowView)>,
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| ".local/quest/vrui".into());
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .add_plugins((
            VrPlugin {
                backend: XrBackendKind::External,
            },
            VrRenderPlugin,
            VrUiPlugin { gizmos: false },
            VrUiRenderPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 72.0,
        )))
        .insert_resource(Out(out.into()))
        .insert_resource(Script {
            frame: 0,
            list: windows(),
        })
        .add_systems(Startup, load_assets)
        .add_systems(First, drive)
        .add_systems(Last, dump);
    app.run()
}

fn drive(
    mut s: ResMut<Script>,
    mut pending: ResMut<PendingXrFrame>,
    mut data: ResMut<HudDataRes>,
    hud: Res<VrHudRes>,
) {
    let f = s.frame;
    s.frame += 1;
    let mut x = XrFrame::default();
    x.hand_mut(Hand::Right).pose = Some(Pose::new(RIGHT, Quat::IDENTITY));
    let slot = (f / PER_WINDOW) as usize;
    let phase = f % PER_WINDOW;
    if slot == s.list.len() && phase == 0 {
        data.0.window = None;
        data.0.focus = Some(FocusData {
            name: "Krieger".into(),
            attitude: gothic_ui::FocusAttitude::Hostile,
            health: Some(0.7),
            head: Vec3::new(0.3, 1.7, -2.5),
        });
        data.0.noise = Some(0.45);
        data.0.messages.print("Spiel gespeichert", gothic_ui::MessageKind::SaveOk);
        data.0.messages.print("Du hast den Schlüssel erhalten.", gothic_ui::MessageKind::Info);
        data.0.floaters.push("+250 EP", [0.9, 0.85, 0.4], Vec3::new(-0.3, 1.5, -1.5), 30.0);
    }
    if phase == 0 {
        data.0.window = s.list.get(slot).map(|(_, v)| v.clone());
    }
    // Hover the second interactive row so the highlight shows.
    if phase > 10
        && let Some(w) = &hud.0.window
    {
        let target = w
            .sheet
            .items
            .iter()
            .filter(|i| i.interactive() && i.kind == sheet::ItemKind::Row)
            .nth(1)
            .or_else(|| w.sheet.items.iter().find(|i| i.interactive()));
        if let Some(i) = target {
            let to = w.panel.point(i.rect.center());
            let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (to - RIGHT).normalize());
            x.hand_mut(Hand::Right).pose = Some(Pose::new(RIGHT, rot));
        }
    }
    x.head = Pose::new(EYE, Quat::IDENTITY);
    pending.0 = Some(x);
}

fn dump(
    mut commands: Commands,
    s: Res<Script>,
    out: Res<Out>,
    surfaces: Query<(&gothic_vr_ui::render::Surface, &Visibility)>,
    mut exit: MessageWriter<AppExit>,
) {
    let slot = ((s.frame.saturating_sub(1)) / PER_WINDOW) as usize;
    let phase = s.frame.saturating_sub(1) % PER_WINDOW;
    if phase == PER_WINDOW - 5 && slot < s.list.len() {
        let _ = std::fs::create_dir_all(&out.0);
        for (sf, vis) in &surfaces {
            if sf.kind == gothic_vr_ui::render::SurfaceKind::Window && *vis == Visibility::Visible {
                let path = out.0.join(format!("{}.png", s.list[slot].0));
                commands
                    .spawn(bevy::render::view::screenshot::Screenshot::image(
                        sf.image().clone(),
                    ))
                    .observe(bevy::render::view::screenshot::save_to_disk(path));
            }
        }
    }
    if slot == s.list.len() && phase == PER_WINDOW - 5 {
        let _ = std::fs::create_dir_all(&out.0);
        for (sf, vis) in &surfaces {
            if let gothic_vr_ui::render::SurfaceKind::Tag(i) = sf.kind
                && *vis == Visibility::Visible
            {
                let path = out.0.join(format!("tag{i}.png"));
                commands
                    .spawn(bevy::render::view::screenshot::Screenshot::image(sf.image().clone()))
                    .observe(bevy::render::view::screenshot::save_to_disk(path));
            }
        }
    }
    if s.frame > PER_WINDOW * (s.list.len() as u32 + 1) + 5 {
        exit.write(AppExit::Success);
    }
}

/// The installation's UI art, fonts and the item catalog (`--install`, `--items`); everything is
/// read at run time and only drawn into gitignored PNGs.
fn load_assets(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut data: ResMut<HudDataRes>,
) {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let install = arg("--install").unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3".into());
    if std::path::Path::new(&install).join("Data").exists() {
        let a = gothic_ui::assets::load_install_assets(std::path::Path::new(&install), &mut fonts, &mut images);
        commands.insert_resource(HudFont(a.font.clone()));
        commands.insert_resource(HudHeadingFont(a.heading.clone()));
        commands.insert_resource(a);
    } else {
        warn!("no installation at {install}: flat panels and the fallback font");
    }
    let items = arg("--items").unwrap_or_else(|| "C:/Users/PowerTower/dev/gothic-reforged/.local/items.json".into());
    if let Ok(cat) = gothic_ui::ItemCatalog::load(std::path::Path::new(&items)) {
        data.0.icons = cat.iter().filter_map(|(n, i)| Some((n.to_string(), i.icon.clone()?))).collect();
    }
}
