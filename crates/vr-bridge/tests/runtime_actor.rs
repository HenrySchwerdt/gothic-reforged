//! The VR hand / melee layer drives an actor spawned by the `gothic-runtime` core (synthetic
//! data only): a physics hand swings a physics sword into the actor's proxy, the hit becomes
//! the core's `Command::Attack`, and the core's own combat rules hurt the actor. Level chunks
//! reach avian, and the core keeps its 30 Hz next to the 90 Hz VR physics.
use bevy::prelude::*;
use gothic_gameplay::{GameDatabase, GameState};
use gothic_physics::Tri;
use gothic_runtime::{Command, Core, CoreCommand, CoreRes, Event};
use gothic_sim::data::NpcRecord;
use gothic_sim::item::ItemDb;
use gothic_sim::types::ActorId;
use gothic_sim::world::Pos;
use gothic_vr::{Hand, Pose};
use gothic_vr_bridge::*;
use gothic_vr_melee::{HitMessage, HitzoneSet, PlayerHitzonesPlugin, VrMeleePlugin};
use gothic_vr_physics::testing::Rig;
use gothic_vr_physics::{VrStrength, props, spawn_prop};
use std::sync::Arc;

fn core() -> Core {
    let text = "[LocAdmin_Languages]\r\nLanguages=English\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
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

#[derive(Resource, Default)]
struct Log {
    hits: Vec<HitMessage>,
    sent: Vec<Command>,
    events: Vec<Event>,
}

fn collect(
    mut log: ResMut<Log>,
    mut h: MessageReader<HitMessage>,
    mut c: MessageReader<CoreCommand>,
    mut e: MessageReader<gothic_runtime::CoreEvent>,
) {
    log.hits.extend(h.read().copied());
    log.sent.extend(c.read().map(|c| c.0.clone()));
    log.events.extend(e.read().map(|e| e.0.clone()));
}

/// A 20 m floor at y = 0 as one level chunk, in centimetres.
fn floor_cm() -> Vec<Tri> {
    let v = |x: f32, z: f32| Vec3::new(x, 0.0, z);
    vec![
        [v(-1000.0, -1000.0), v(-1000.0, 1000.0), v(1000.0, 1000.0)],
        [v(-1000.0, -1000.0), v(1000.0, 1000.0), v(1000.0, -1000.0)],
    ]
}

fn rig() -> Rig {
    let mut rig = Rig::with_plugins(
        |_| {},
        |app| {
            app.add_plugins((
                VrMeleePlugin,
                PlayerHitzonesPlugin,
                VrBridgePlugin::default(),
            ))
            .insert_resource(CoreRes(core()))
            .init_resource::<Log>()
            .add_systems(Last, collect);
        },
    );
    rig.app.insert_resource(VrStrength(100.0));
    rig.app
        .world_mut()
        .write_message(LevelChunkMessage::Loaded {
            id: 1,
            tris: floor_cm(),
        });
    rig.step(2);
    rig
}

fn send(rig: &mut Rig, c: Command) {
    rig.app.world_mut().write_message(CoreCommand(c));
}

/// Spawn an NPC through the core at (x, z) centimetres; returns its id and proxy.
fn spawn_npc(rig: &mut Rig, x: f32, z: f32) -> (ActorId, Entity) {
    send(
        rig,
        Command::SpawnNpc {
            record: Box::new(NpcRecord {
                name: "Fake_Bandit".into(),
                species: "Human".into(),
                level: 8,
                ..Default::default()
            }),
            at: Pos::flat(x, z),
            yaw: std::f32::consts::FRAC_PI_2,
        },
    );
    rig.step(8);
    let actor = rig
        .app
        .world()
        .resource::<Log>()
        .events
        .iter()
        .find_map(|e| match e {
            Event::ActorSpawned { actor, .. } => Some(*actor),
            _ => None,
        })
        .expect("core spawned the actor");
    let proxy = *rig
        .app
        .world()
        .resource::<ActorProxies>()
        .0
        .get(&actor)
        .expect("bridge spawned a proxy");
    (actor, proxy)
}

fn edge_left() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

#[test]
fn a_vr_sword_hit_on_a_runtime_actor_reaches_the_core() {
    let mut rig = rig();
    send(
        &mut rig,
        Command::HeroTransform {
            pos: Pos::flat(0.0, 0.0),
            yaw: -std::f32::consts::FRAC_PI_2,
        },
    );
    let (npc, proxy) = spawn_npc(&mut rig, 0.0, -110.0);
    // The proxy stands where the core put the actor (metres).
    let t = rig.transform(proxy);
    assert!((t.translation.z + 1.1).abs() < 1e-3, "{:?}", t.translation);
    let hp_before = rig.app.world().resource::<CoreRes>().0.sim.actor(npc).hp;
    assert!(hp_before > 0);

    // Grab a sword and swing it through the actor.
    let start = Pose::new(Vec3::new(0.7, 1.25, -0.35), edge_left());
    rig.set_hand(Hand::Right, start);
    rig.step(30);
    rig.spawn(|c| spawn_prop(c, &props::sword(), start));
    rig.grip(Hand::Right, true);
    rig.step(30);
    let n = (0.18 * 90.0) as usize;
    for i in 0..=n {
        let x = 0.7 - 1.4 * i as f32 / n as f32;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(x, 1.25, -0.35), edge_left()),
        );
        rig.step(1);
    }
    rig.step(15);

    let log = rig.app.world().resource::<Log>();
    assert!(
        log.hits.iter().any(|h| h.victim == proxy),
        "melee hit the proxy: {:?}",
        log.hits
    );
    assert!(
        log.sent
            .iter()
            .any(|c| matches!(c, Command::Attack { target } if *target == npc)),
        "bridge sent Attack: {:?}",
        log.sent
    );
    let hp_after = rig.app.world().resource::<CoreRes>().0.sim.actor(npc).hp;
    assert!(
        hp_after < hp_before,
        "core applied damage: {hp_before} -> {hp_after}"
    );
}

#[test]
fn proxies_follow_the_core_and_leave_with_the_actor() {
    let mut rig = rig();
    let (npc, proxy) = spawn_npc(&mut rig, 300.0, -200.0);
    {
        let mut core = rig.app.world_mut().resource_mut::<CoreRes>();
        let entry = core.0.sim.world.actors.get_mut(&npc).unwrap();
        entry.0 = Pos::new(-150.0, 20.0, -400.0);
        // Sim yaw 0 looks along +X.
        entry.1 = 0.0;
    }
    rig.step(3);
    let t = rig.transform(proxy);
    assert!((t.translation - Vec3::new(-1.5, 0.2 + 0.9, -4.0)).length() < 1e-3);
    let facing = t.rotation * Vec3::NEG_Z;
    assert!((facing - Vec3::X).length() < 1e-4, "{facing}");
    let zones = rig
        .app
        .world()
        .get::<HitzoneSet>(proxy)
        .unwrap()
        .zones
        .clone();
    assert!(
        zones
            .iter()
            .all(|z| z.pose.position.distance(Vec3::new(-1.5, 1.0, -4.0)) < 1.2)
    );
    rig.app
        .world_mut()
        .resource_mut::<CoreRes>()
        .0
        .sim
        .world
        .actors
        .remove(&npc);
    rig.step(3);
    assert!(
        rig.app.world().get_entity(proxy).is_err(),
        "proxy despawned"
    );
}

#[test]
fn level_chunks_reach_avian_and_can_be_unloaded() {
    let mut rig = rig();
    let at = Pose::new(Vec3::new(1.0, 0.6, -1.0), Quat::IDENTITY);
    let crate_ = rig.spawn(|c| spawn_prop(c, &props::crate_box(Vec3::splat(0.2), 5.0), at));
    rig.step(120);
    let y = rig.transform(crate_).translation.y;
    assert!(y > 0.0 && y < 0.6, "rests on the chunk floor: {y}");
    rig.app
        .world_mut()
        .write_message(LevelChunkMessage::Unloaded { id: 1 });
    rig.step(90);
    assert!(
        rig.transform(crate_).translation.y < -0.5,
        "falls once the chunk is gone"
    );
    // The same triangles feed the game's collision world.
    let mut world = gothic_physics::CollisionWorld::new();
    assert!(gothic_vr_bridge::level::load_both(&mut world, 7, floor_cm()).is_some());
    assert_eq!(world.triangle_count(), 2);
    assert!(gothic_vr_bridge::level::trimesh_from_cm(&[]).is_none());
}

#[test]
fn the_core_ticks_at_30_hz_while_physics_keeps_90() {
    let mut rig = rig();
    let t0 = rig.app.world().resource::<CoreRes>().0.sim.now_ms();
    rig.step(90);
    let t1 = rig.app.world().resource::<CoreRes>().0.sim.now_ms();
    let hz = rig
        .app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f64();
    assert!((hz - 1.0 / 90.0).abs() < 1e-6, "fixed step stays at 90 Hz");
    assert!(
        (900..=1100).contains(&(t1 - t0)),
        "one second of core time: {}",
        t1 - t0
    );
}
