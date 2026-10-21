//! Story-state sectors and triggers in the core: `SetSectorStatus` switches whole layers of
//! actors, and the trigger counters persist through a save.
use gothic_gameplay::{GameDatabase, GameState};
use gothic_runtime::{Command, Core, Event};
use gothic_sim::data::NpcRecord;
use gothic_sim::item::ItemDb;
use gothic_sim::world::Pos;
use gothic_world::entity::EntityTree;
use gothic_world::scene::SceneAdmin;
use gothic_world::trigger::{Contact, TriggerDef, TriggerEnv, TriggerOutput};
use std::sync::Arc;

const NPC_01: &str =
    "G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_01/G3_Myrtana_01_Ardea_NPC_01.lrentdat";
const NPC_02: &str =
    "G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_02/G3_Myrtana_01_Ardea_NPC_02.lrentdat";

fn core() -> Core {
    let text = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nQUEST_X=Quest;;Quest;\r\n[LocAdmin_Revisions]\r\n";
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    let strings = gothic_formats::stringtable::StringTable::parse(&b).unwrap();
    let db = Arc::new(GameDatabase::from_parts(vec![], vec![], strings, "English").unwrap());
    let mut c = Core::new(db, ItemDb::default(), GameState::new(), 7);
    c.guess_weapons = false;
    c
}

fn spawn(c: &mut Core, name: &str, document: &str, x: f32) -> gothic_sim::types::ActorId {
    let record = NpcRecord {
        name: name.into(),
        species: "Human".into(),
        document: document.into(),
        ..Default::default()
    };
    let evs = c.tick(
        0.0,
        &[Command::SpawnNpc {
            record: Box::new(record),
            at: Pos::flat(x, 0.0),
            yaw: 0.0,
        }],
    );
    evs.iter()
        .find_map(|e| match e {
            Event::ActorSpawned { actor, .. } => Some(*actor),
            _ => None,
        })
        .expect("spawned")
}

#[test]
fn liberating_ardea_swaps_the_npc_layers() {
    let mut c = core();
    c.tick(
        0.0,
        &[
            Command::RegisterSector {
                name: "G3_Myrtana_01_Ardea_NPC_01".into(),
                enabled: true,
            },
            Command::RegisterSector {
                name: "G3_Myrtana_01_Ardea_NPC_02".into(),
                enabled: false,
            },
        ],
    );
    assert!(c.document_enabled(NPC_01) && !c.document_enabled(NPC_02));
    let occupied = spawn(&mut c, "Old_Guard", NPC_01, 500.0);
    let liberated = spawn(&mut c, "Free_Citizen", NPC_02, 600.0);
    c.tick(0.05, &[]);
    let inside = c.sim.processing.clone().expect("range on");
    assert!(inside.contains(&occupied) && !inside.contains(&liberated));

    // What the info command `SetSectorStatus` writes.
    c.sim
        .state
        .sectors
        .insert("G3_Myrtana_01_Ardea_NPC_01".into(), false);
    c.sim
        .state
        .sectors
        .insert("G3_Myrtana_01_Ardea_NPC_02".into(), true);
    let evs = c.tick(0.05, &[]);
    let switched: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            Event::SectorSwitched { sector, enabled } => Some((sector.as_str(), *enabled)),
            _ => None,
        })
        .collect();
    assert_eq!(
        switched,
        vec![
            ("G3_Myrtana_01_Ardea_NPC_01", false),
            ("G3_Myrtana_01_Ardea_NPC_02", true)
        ]
    );
    let inside = c.sim.processing.clone().expect("range on");
    assert!(!inside.contains(&occupied) && inside.contains(&liberated));
    assert!(!c.document_enabled(NPC_01) && c.document_enabled(NPC_02));

    // Loading an older save without the flags restores the authored state.
    c.sim.state.sectors.clear();
    c.tick(0.05, &[]);
    assert!(c.document_enabled(NPC_01) && !c.document_enabled(NPC_02));
    let inside = c.sim.processing.clone().expect("range on");
    assert!(inside.contains(&occupied) && !inside.contains(&liberated));
}

#[test]
fn trigger_counters_survive_a_save() {
    let mut tree = EntityTree::new();
    let mut scene = SceneAdmin::new();
    let mut mk = |n: &str, dynamic: bool| {
        let id = tree.create(n, dynamic);
        scene.register(id, n);
        id
    };
    let volume = mk("Loc_Ardea", false);
    let _target = mk("Music", true);
    let hero = mk("PC_Hero", true);
    let mut def = TriggerDef::new("Music");
    def.max_activations = 1;

    let mut a = core();
    a.triggers.add(volume, "loc_ardea", def.clone());
    let env = TriggerEnv::new(&tree, &scene);
    a.triggers
        .on_touch(&env, a.clock, volume, hero, &Contact::default());
    assert!(matches!(
        a.triggers.take_output()[..],
        [TriggerOutput::Delivered { .. }]
    ));
    let body = a.capture();
    assert!(body.host.contains_key("world.triggers"));

    let mut b = core();
    b.triggers.add(volume, "loc_ardea", def);
    b.restore(&body);
    b.triggers
        .on_touch(&env, b.clock, volume, hero, &Contact::default());
    assert!(
        b.triggers.take_output().is_empty(),
        "one-shot trigger stays used up"
    );
    // The key is internal: it does not leak into the host map.
    assert!(!b.host.contains_key("world.triggers"));
}
