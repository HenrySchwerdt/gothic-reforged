//! Mods against a synthetic base database (no game data).
use gothic_formats::{info::Info, quest::Quest, stringtable::StringTable};
use gothic_gameplay::Engine;
use gothic_gameplay::GameState;
use gothic_gameplay::model::QuestStatus;
use gothic_mods::{
    HotReload, LoadOrder, LoadedMod, Manifest, ModData, OrderProblem, PatchFile, RecordKind, apply,
    diff, resolve,
};
use gothic_sim::item::ItemDef;
use serde_json::json;
use std::path::{Path, PathBuf};

fn strings() -> StringTable {
    let mut keys = String::new();
    for (k, en, de) in [
        ("T_HELLO", "Hello.", "Hallo."),
        ("T_WEATHER", "Nice weather.", "Schoenes Wetter."),
    ] {
        keys.push_str(&format!("{k}={en};;{de};\r\n"));
    }
    let text = format!(
        "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n{keys}[LocAdmin_Revisions]\r\n"
    );
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    StringTable::parse(&b).unwrap()
}

fn base() -> ModData {
    let quests = vec![
        Quest::parse(b"[Quest]\r\nName=Q_Letter\r\nType=1\r\nDeliveryEntities=Bob\r\nDeliveryAmounts=1\r\nExperiencePoints=50\r\n").unwrap(),
    ];
    let infos = vec![
        Info::parse(b"[Info]\r\nName=A_Intro\r\nSortID=1\r\nOwner=Alice\r\nConditionType=3\r\nInfoType=3\r\nInfoScript_Commands=Say\r\nInfoScript_Entities1=npc;\r\nInfoScript_Entities2=;\r\nInfoScript_IDs1=;\r\nInfoScript_IDs2=;\r\nInfoScript_Texts=T_HELLO;\r\n").unwrap(),
        Info::parse(b"[Info]\r\nName=B_Smalltalk\r\nSortID=2\r\nOwner=Bob\r\nConditionType=3\r\nInfoType=3\r\nInfoScript_Commands=Say\r\nInfoScript_Entities1=npc;\r\nInfoScript_Entities2=;\r\nInfoScript_IDs1=;\r\nInfoScript_IDs2=;\r\nInfoScript_Texts=T_WEATHER;\r\n").unwrap(),
    ];
    let items = vec![
        ItemDef {
            name: "It_Herb".into(),
            category: "Plant".into(),
            value: 5,
            ..ItemDef::default()
        },
        ItemDef {
            name: "It_Apple".into(),
            category: "Food".into(),
            value: 2,
            ..ItemDef::default()
        },
    ];
    let npcs = vec![
        json!({"name": "Alice", "species": "Human", "level": 3}),
        json!({"name": "Bob", "species": "Human", "level": 4}),
    ];
    ModData::new(&quests, &infos, strings(), &items, npcs)
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/examples")
}

/// The example data mod alone (`mods/examples` also holds script-only example mods).
fn hermit() -> LoadOrder {
    resolve(vec![
        LoadedMod::load(&examples().join("forest_hermit")).unwrap(),
    ])
}

#[test]
fn example_mod_adds_item_dialog_line_and_quest() {
    let order = hermit();
    let (data, report) = apply(base(), &order);
    assert!(report.is_clean(), "{}", report.to_text());
    assert_eq!(report.applied, ["forest_hermit@1.0.0"]);
    // New item, NPC, quest, infos and strings.
    let items = data.items_typed().unwrap();
    let tea = items.iter().find(|i| i.name == "It_HermitTea").unwrap();
    assert_eq!((tea.value, tea.category.as_str()), (25, "Potion"));
    let npcs = data.npcs_typed().unwrap();
    assert_eq!(npcs.iter().find(|n| n.name == "Hermit").unwrap().level, 5);
    let db = data.to_database("English").unwrap();
    let q = db.quest_by_name("Q_HermitTea").unwrap();
    assert_eq!(q.delivery_entities, ["It_Herb"]);
    assert_eq!(
        db.text("MOD_ALICE_HERMIT").unwrap(),
        "An old hermit. He brews tea."
    );
    // The new dialog line is offered by Alice, with its new text as the label.
    let engine = Engine::new(&db);
    let mut st = GameState::new();
    let (session, _) = engine.start(&mut st, "Alice");
    let labels: Vec<String> = engine
        .menu(&st, &session)
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert!(
        labels.iter().any(|l| l == "Who lives in the forest?"),
        "{labels:?}"
    );
    // The new quest plays: the hermit's activator starts it.
    let (_, out) = engine.start(&mut st, "Hermit");
    assert!(
        out.outputs.iter().any(|o| matches!(
            o,
            gothic_gameplay::script::Output::QuestChanged {
                to: QuestStatus::Running,
                ..
            }
        )),
        "{out:?}"
    );
    // German texts from the RON patch.
    let de = data.to_database("German").unwrap();
    assert_eq!(
        de.text("MOD_HERMIT_QUEST").unwrap(),
        "Tee fuer den Einsiedler"
    );
}

#[test]
fn applied_data_round_trips_through_a_diff_patch() {
    let order = hermit();
    let (modded, _) = apply(base(), &order);
    let patch = diff(&base(), &modded);
    // The diff survives TOML and RON serialisation ...
    let toml = patch.to_toml().unwrap();
    assert_eq!(PatchFile::from_toml(&toml).unwrap(), patch);
    assert_eq!(
        PatchFile::from_ron(&patch.to_ron().unwrap()).unwrap(),
        patch
    );
    // ... and re-creates the modded data from the base.
    let m = LoadedMod {
        manifest: Manifest::parse("[mod]\nid = \"export\"\nversion = \"0.1.0\"\n").unwrap(),
        dir: PathBuf::new(),
        patches: vec![("diff.toml".into(), PatchFile::from_toml(&toml).unwrap())],
    };
    let (again, report) = apply(base(), &resolve(vec![m]));
    assert!(report.is_clean(), "{}", report.to_text());
    assert_eq!(again, modded);
    // No difference, no patch.
    assert!(diff(&modded, &modded).is_empty());
}

fn inline_mod(id: &str, version: &str, extra: &str, patch: &str) -> LoadedMod {
    LoadedMod {
        manifest: Manifest::parse(&format!(
            "[mod]\nid = \"{id}\"\nversion = \"{version}\"\n{extra}"
        ))
        .unwrap(),
        dir: PathBuf::new(),
        patches: vec![("p.toml".into(), PatchFile::from_toml(patch).unwrap())],
    }
}

#[test]
fn conflicts_are_detected_and_load_order_decides() {
    let a = inline_mod(
        "balance",
        "1.0.0",
        "",
        "[[items.patch]]\nname = \"It_Herb\"\nset = { value = 10 }\n[[quests.patch]]\nname = \"Q_Letter\"\nset = { experience_points = 80 }\n",
    );
    let b = inline_mod(
        "economy",
        "1.0.0",
        "[load]\nafter = [\"balance\"]\n",
        "[[items.patch]]\nname = \"It_Herb\"\nset = { value = 20 }\n[[items.delete]]\nname = \"It_Apple\"\n[[strings.set]]\nkey = \"T_HELLO\"\ntexts = { German = \"Gruess dich.\" }\n",
    );
    let (data, report) = apply(base(), &resolve(vec![b, a]));
    assert_eq!(report.applied, ["balance@1.0.0", "economy@1.0.0"]);
    let herb = data
        .items_typed()
        .unwrap()
        .into_iter()
        .find(|i| i.name == "It_Herb")
        .unwrap();
    assert_eq!(herb.value, 20, "the later mod wins");
    assert_eq!(report.conflicts.len(), 1, "{}", report.to_text());
    let c = &report.conflicts[0];
    assert_eq!(
        (c.kind, c.record.as_str(), c.field.as_str()),
        (RecordKind::Item, "It_Herb", "value")
    );
    assert_eq!(
        (c.first.as_str(), c.winner.as_str()),
        ("balance", "economy")
    );
    assert!(data.find(RecordKind::Item, "It_Apple").is_none());
    // Only the German text changed.
    let en = data.to_database("English").unwrap();
    let de = data.to_database("German").unwrap();
    assert_eq!(
        (en.text("T_HELLO").unwrap(), de.text("T_HELLO").unwrap()),
        ("Hello.".into(), "Gruess dich.".into())
    );
    assert!(
        report
            .to_text()
            .contains("CONFLICT item It_Herb field value: economy overrides balance")
    );
}

#[test]
fn bad_operations_are_reported_and_skipped() {
    let m = inline_mod(
        "broken",
        "1.0.0",
        "",
        r#"
[[items.patch]]
name = "It_Herb"
set = { valu = 3 }

[[items.patch]]
name = "It_Herb"
set = { value = "cheap" }

[[quests.patch]]
name = "Q_Nope"
set = { experience_points = 1 }

[[quests.add]]
name = "Q_Letter"

[[infos.patch]]
name = "A_Intro"
set = { name = "A_Renamed" }

[[strings.set]]
key = "T_X"
texts = { Klingon = "Qapla'" }

[[items.patch]]
name = "It_Apple"
set = { value = 3 }
"#,
    );
    let (data, report) = apply(base(), &resolve(vec![m]));
    let msgs: Vec<String> = report
        .errors
        .iter()
        .map(|e| format!("{} {}", e.record, e.message))
        .collect();
    assert_eq!(report.errors.len(), 6, "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("unknown field \"valu\"")));
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("It_Herb") && m.contains("invalid type")),
        "{msgs:#?}"
    );
    assert!(msgs.iter().any(|m| m == "Q_Nope no such record"));
    assert!(msgs.iter().any(|m| m.contains("already exists")));
    assert!(msgs.iter().any(|m| m.contains("cannot rename")));
    assert!(msgs.iter().any(|m| m.contains("Klingon")));
    // The good operation still applied.
    let apple = data
        .items_typed()
        .unwrap()
        .into_iter()
        .find(|i| i.name == "It_Apple")
        .unwrap();
    assert_eq!(apple.value, 3);
    assert_eq!(report.operations, 1);
}

#[test]
fn dependencies_gate_mods() {
    let needs = inline_mod("addon", "1.0.0", "[dependencies]\nbase_fix = \">=2\"\n", "");
    let dep = inline_mod("base_fix", "1.4.0", "", "");
    let (_, report) = apply(base(), &resolve(vec![needs, dep]));
    assert_eq!(report.applied, ["base_fix@1.4.0"]);
    assert!(
        matches!(&report.order_problems[0], OrderProblem::VersionMismatch { id, .. } if id == "addon")
    );
}

#[test]
fn hot_reload_picks_up_changes() {
    let dir = std::env::temp_dir().join(format!("gothic-mods-hot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let m = dir.join("tweak");
    std::fs::create_dir_all(m.join("patches")).unwrap();
    std::fs::write(
        m.join("mod.toml"),
        "[mod]\nid = \"tweak\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let patch = m.join("patches/a.toml");
    std::fs::write(
        &patch,
        "[[items.patch]]\nname = \"It_Herb\"\nset = { value = 7 }\n",
    )
    .unwrap();
    let mut hot = HotReload::new(base(), &dir);
    let value = |d: &ModData| {
        d.items_typed()
            .unwrap()
            .into_iter()
            .find(|i| i.name == "It_Herb")
            .unwrap()
            .value
    };
    let (d, _) = hot.load().unwrap();
    assert_eq!(value(&d), 7);
    assert!(hot.poll().is_none(), "nothing changed");
    // Different length, so the change is seen even within the same mtime tick.
    std::fs::write(
        &patch,
        "[[items.patch]]\nname = \"It_Herb\"\nset = { value = 1234 }\n",
    )
    .unwrap();
    let (d, report) = hot.poll().expect("changed").unwrap();
    assert!(report.is_clean());
    assert_eq!(value(&d), 1234);
    let _ = std::fs::remove_dir_all(&dir);
}
