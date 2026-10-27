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
const PER_WINDOW: u32 = 60;

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
        ("ItMw_Kurzschwert", 1, 640),
        ("ItMw_Streitaxt", 1, 1200),
        ("ItRw_Jagdbogen", 1, 420),
        ("ItRw_Pfeil", 40, 2),
        ("ItPo_Heiltrank", 5, 45),
        ("ItFo_Brot", 12, 3),
        ("ItFo_Braten", 8, 8),
        ("ItMi_Fackel", 6, 4),
        ("ItAr_Lederruestung", 1, 800),
        ("ItSc_Feuerball", 2, 150),
    ];
    let mine = [
        ("ItMi_Wolfsfell", 4, 12),
        ("ItMi_Erzbrocken", 9, 20),
        ("ItFo_Apfel", 3, 1),
        ("ItMi_Dietrich", 7, 5),
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
        ask_rows: vec![row("ItPo_Heiltrank", 2, 45, 4)],
        offer_rows: vec![row("ItMi_Wolfsfell", 3, 12, 20)],
        ask_total: 90,
        offer_total: 36,
        deal: false,
        balance: true,
        hint: "Das ist kein Handelsgut".into(),
    };
    let loot = LootView {
        title: "Alte Truhe".into(),
        rows: vec![
            row("ItMi_Gold", 85, 0, 0),
            row("ItPo_Heiltrank", 2, 0, 1),
            row("ItWr_Brief", 1, 0, 2),
            row("ItMi_Dietrich", 3, 0, 3),
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
                product: "ItMw_Kurzschwert".into(),
                name: "Kurzschwert".into(),
                description: "Eine einfache, gut ausgewogene Klinge fuer den Anfang.".into(),
                ingredients: vec![
                    ing("ItMi_Stahl", "Stahlbarren", 2, 3),
                    ing("ItMi_Holz", "Holz", 1, 0),
                ],
                requirement: "Schmiedekunst 1".into(),
                craftable: false,
            },
            RecipeRow {
                key: 1,
                product: "ItMw_Streitaxt".into(),
                name: "Streitaxt".into(),
                description: "Schwer und verheerend.".into(),
                ingredients: vec![ing("ItMi_Stahl", "Stahlbarren", 3, 3)],
                requirement: String::new(),
                craftable: true,
            },
            RecipeRow {
                key: 2,
                name: "Dolch".into(),
                product: "ItMw_Dolch".into(),
                craftable: true,
                ..Default::default()
            },
        ],
        gold: 230,
        hint: String::new(),
    };
    let doc = DocumentView {
        title: "Das Tagebuch des Jaegers".into(),
        left: "Der Winter kam frueh in diesem Jahr. Die Woelfe wagen sich bis an die Palisade, und \
               niemand im Dorf will mehr nach Einbruch der Dunkelheit vor die Tuer. Ich habe die \
               Fallen am alten Weg erneuert und die Felle zum Haendler gebracht, doch er zahlt \
               kaum die Haelfte dessen, was sie wert sind."
            .repeat(2),
        right: String::new(),
        is_book: false,
    };
    let quests = QuestLogView {
        quests: [
            ("Die Woelfe von Ardea", 0),
            ("Erz fuer den Schmied", 0),
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
                "Der Jaeger bittet dich, die Woelfe zu vertreiben, die das Dorf bedrohen. Such ihre Hoehle im Norden."
                    .into(),
                "Aufgabe begonnen".into(),
            ],
        })
        .collect(),
        clock: "3. Tag, 14:05".into(),
        reputation: vec![("Nordmar".into(), 12), ("Ardea".into(), 40)],
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
            ("Staerke".into(), 120),
            ("Geschick".into(), 90),
            ("Wissen".into(), 10),
            ("Schmieden".into(), 15),
            ("Diebeskunst".into(), 5),
            ("Alchemie".into(), 0),
        ],
        protection: vec![("Klingen".into(), 12), ("Stumpf".into(), 8), ("Feuer".into(), 2)],
        skills: ["Einhaender 1", "Bogen 1", "Jagen 1", "Schleichen 1", "Schmieden 1"]
            .iter()
            .map(|s| ((*s).into(), true))
            .collect(),
        equipment: vec![
            ("Rechts".into(), "ItMw_Kurzschwert".into()),
            ("Koerper".into(), "ItAr_Lederruestung".into()),
        ],
    };
    vec![
        ("trade", WindowView::Trade(trade)),
        ("loot", WindowView::Loot(loot)),
        ("recipe", WindowView::Recipe(recipe)),
        ("document", WindowView::Document(doc)),
        ("quests", WindowView::Quests(quests)),
        ("character", WindowView::Character(character)),
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
        .add_systems(Startup, load_font)
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
    if s.frame > PER_WINDOW * s.list.len() as u32 + 5 {
        exit.write(AppExit::Success);
    }
}

fn load_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let args: Vec<String> = std::env::args().collect();
    let given = args
        .iter()
        .position(|a| a == "--font")
        .and_then(|i| args.get(i + 1).cloned());
    for path in given
        .into_iter()
        .chain(["C:/Windows/Fonts/segoeui.ttf", "C:/Windows/Fonts/arial.ttf"].map(String::from))
    {
        if let Ok(bytes) = std::fs::read(&path) {
            commands.insert_resource(HudFont(fonts.add(Font::from_bytes(bytes))));
            return;
        }
    }
}
