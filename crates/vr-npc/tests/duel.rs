//! Headless greybox duel: a ragdoll NPC with a physical sword against the player.
use bevy::prelude::*;
use gothic_vr::{Hand, Pose};
use gothic_vr_melee::{
    ClashMessage, HitMessage, PlayerAvatar, PlayerHitzonesPlugin, VrMeleePlugin,
};
use gothic_vr_npc::ai::{AttackDir, Phase};
use gothic_vr_npc::ragdoll::Stance;
use gothic_vr_npc::{VrNpc, VrNpcPlugin, spawn_npc};
use gothic_vr_physics::testing::Rig;
use gothic_vr_physics::{VrStrength, props, spawn_prop, static_box};

#[derive(Resource, Default)]
struct Log {
    hits: Vec<HitMessage>,
    clashes: Vec<ClashMessage>,
    phases: Vec<Phase>,
    stances: Vec<Stance>,
}

fn collect(
    mut log: ResMut<Log>,
    mut h: MessageReader<HitMessage>,
    mut c: MessageReader<ClashMessage>,
    npcs: Query<&VrNpc>,
) {
    log.hits.extend(h.read().copied());
    log.clashes.extend(c.read().copied());
    for n in &npcs {
        log.phases.push(n.ai.phase);
        log.stances.push(n.ragdoll.stance());
    }
}

fn duel(pattern: Option<AttackDir>, cooldown: f32) -> (Rig, Entity, Entity) {
    duel_with(pattern, cooldown, true)
}

fn duel_with(pattern: Option<AttackDir>, cooldown: f32, armed: bool) -> (Rig, Entity, Entity) {
    let mut rig = Rig::with_plugins(
        |c| {
            c.spawn(static_box(
                Vec3::new(0.0, -0.5, 0.0),
                Vec3::new(10.0, 0.5, 10.0),
                Quat::IDENTITY,
            ));
        },
        |app| {
            app.add_plugins((VrMeleePlugin, PlayerHitzonesPlugin, VrNpcPlugin))
                .init_resource::<Log>()
                .add_systems(FixedUpdate, collect.after(gothic_vr_melee::MeleeSet));
        },
    );
    let npc = rig.spawn(|c| {
        spawn_npc(
            c,
            Vec3::new(0.0, 0.0, -1.5),
            std::f32::consts::PI,
            armed.then(props::sword).as_ref(),
            1,
        )
    });
    {
        let mut n = rig.app.world_mut().get_mut::<VrNpc>(npc).unwrap();
        n.ai.settings.attack_pattern = pattern;
        n.ai.cooldown = cooldown;
    }
    let mut q = rig
        .app
        .world_mut()
        .query_filtered::<Entity, With<PlayerAvatar>>();
    let avatar = q.iter(rig.app.world()).next().unwrap();
    (rig, npc, avatar)
}

#[test]
fn npc_attacks_and_hits_an_idle_player() {
    let (mut rig, npc, avatar) = duel(None, 0.5);
    rig.step(90 * 8);
    let log = rig.app.world().resource::<Log>();
    let n = rig.app.world().get::<VrNpc>(npc).unwrap();
    assert!(
        log.phases.iter().any(|p| matches!(p, Phase::Telegraph(..))),
        "never attacked: last {:?} root {} cooldown {} stance {:?} hits {:?}",
        log.phases.last(),
        n.ai.root,
        n.ai.cooldown,
        log.stances.iter().find(|s| **s != Stance::Standing),
        log.hits
    );
    let on_player: Vec<_> = log.hits.iter().filter(|h| h.victim == avatar).collect();
    assert!(
        !on_player.is_empty(),
        "no hit on the player: {:?}",
        log.hits
    );
    assert!(on_player.iter().all(|h| h.attacker == Some(npc)));
    // The NPC kept its feet doing so.
    assert!(log.stances.iter().all(|s| *s == Stance::Standing));
}

#[test]
fn a_hard_player_hit_staggers_or_fells_the_npc() {
    let (mut rig, npc, _) = duel_with(None, 1000.0, false);
    {
        // A passive, unarmed sparring partner: stands still, does not block.
        let mut n = rig.app.world_mut().get_mut::<VrNpc>(npc).unwrap();
        n.ai.settings.circle_speed = 0.0;
        n.ai.settings.threat_range = 0.0;
    }
    rig.app.insert_resource(VrStrength(120.0));
    rig.step(30);
    // Player picks up a sword (spawned in the hand) and swings it through the NPC's chest.
    let start = Pose::new(
        Vec3::new(0.7, 1.3, -0.75),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    );
    rig.set_hand(Hand::Right, start);
    rig.step(40);
    rig.spawn(|c| spawn_prop(c, &props::sword(), start));
    rig.grip(Hand::Right, true);
    rig.step(30);
    for i in 0..=16 {
        let x = 0.7 - 1.4 * i as f32 / 16.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(x, 1.3, -0.75), start.rotation),
        );
        rig.step(1);
    }
    rig.step(90);
    let log = rig.app.world().resource::<Log>();
    assert!(log.hits.iter().any(|h| h.victim == npc), "{:?}", log.hits);
    assert!(
        log.stances.iter().any(|s| *s != Stance::Standing),
        "the hit moved the ragdoll off balance"
    );
    assert!(log.phases.contains(&Phase::Disabled));
}

#[test]
fn blocking_an_overhead_blow_makes_the_npc_recoil() {
    let (mut rig, _, avatar) = duel(Some(AttackDir::Overhead), 1.0);
    rig.app.insert_resource(VrStrength(150.0));
    rig.step(10);
    // Hold a sword horizontally above the head, blade across (pointing +X), in front.
    let guard = Pose::new(
        Vec3::new(-0.45, 1.75, -0.45),
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
    );
    rig.set_hand(Hand::Right, guard);
    rig.step(40);
    rig.spawn(|c| spawn_prop(c, &props::sword(), guard));
    rig.grip(Hand::Right, true);
    rig.step(90 * 6);
    let log = rig.app.world().resource::<Log>();
    assert!(!log.clashes.is_empty(), "no clash; hits: {:?}", log.hits);
    assert!(log.phases.iter().any(|p| matches!(p, Phase::Recoil(_))));
    let head_hits = log
        .hits
        .iter()
        .filter(|h| h.victim == avatar && h.bone == gothic_vr_melee::Bone::Head)
        .count();
    assert_eq!(head_hits, 0, "the guard protects the head: {:?}", log.hits);
}
