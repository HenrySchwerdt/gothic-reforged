//! NPC combat AI: weapon selection (`_AI_SelectWeapon`, S:49600), casters, archers and the
//! `_AI_*` instruction plan the host's clips drive.
mod common;
use common::*;
use gothic_script::enums as se;
use gothic_script::host::MovePhase;
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::item::{DamageDef, ItemDef, WeaponMode};
use gothic_sim::sim::{EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};

const SPELLS: &str = r#"[
 {"name":"Spl_Fireball","spell_type":0,"deity":2,"min_mana":15,"max_mana":30,"target":1,"cast_type":1,
  "func_on_cast":"","func_on_hit":"","anim_cast":"","anim_power":"",
  "spawn":{"template":"Mis_Fireball","damage":{"DamageType":4,"DamageAmount":50,"DamageManaMultiplier":2.0,"DamageHitMultiplier":1.0},
           "projectile":{"ShootVelocity":100.0}}},
 {"name":"Spl_Icelance","spell_type":0,"deity":0,"min_mana":15,"max_mana":30,"target":1,"cast_type":1,
  "func_on_cast":"","func_on_hit":"","anim_cast":"","anim_power":"",
  "spawn":{"template":"Mis_Icelance","damage":{"DamageType":5,"DamageAmount":40,"DamageManaMultiplier":2.0,"DamageHitMultiplier":1.0},
           "projectile":{"ShootVelocity":100.0}}}
]"#;

fn items() -> gothic_sim::item::ItemDb {
    let mut db = synthetic_items();
    let cast = |name: &str, spell: &str| ItemDef {
        name: name.into(),
        category: "Spellbook".into(),
        use_type: Some("Cast".into()),
        spell: Some(spell.into()),
        permanent: true,
        ..ItemDef::default()
    };
    db.insert(cast("It_Spell_Fireball", "Spl_Fireball"));
    db.insert(cast("It_Spell_Icelance", "Spl_Icelance"));
    db.insert(ItemDef::weapon("It_Staff_Test", "Staff", "Blade", 70));
    db.insert(ItemDef {
        name: "It_Bow_Test".into(),
        category: "Weapon".into(),
        use_type: Some("Bow".into()),
        damage: Some(DamageDef::new("Missile", 30)),
        ..ItemDef::default()
    });
    db
}

fn world() -> (Sim<Grid2dWorld>, ActorId) {
    let mut sim = Sim::new(SimConfig::default(), open_world(), items());
    sim.load_spells(SPELLS).expect("spells");
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    (sim, hero)
}

#[test]
fn a_mage_readies_a_spell_and_casts_it_at_range() {
    let (mut sim, hero) = world();
    let mut m = Actor::npc(ActorId(0), "Orc_Shaman", Species::Orc, 10, 30);
    m.alignment = Alignment::Orc;
    m.class = NpcClass::Mage;
    m.base_attitude_to_player = Attitude::None;
    m.refresh(&SimConfig::default());
    let staff = sim.items.get("It_Staff_Test").cloned().unwrap();
    m.equipment.equip(staff);
    let shaman = sim.spawn(m, Pos::flat(1400.0, 0.0));
    sim.world.set_facing(shaman, std::f32::consts::PI);
    sim.start_attack(shaman, hero, AttackReason::Enemy);
    let mut cast = false;
    for _ in 0..900 {
        sim.step();
        for e in sim.drain_events() {
            if matches!(e.kind, EventKind::Projectile { spell: true, .. }) {
                cast = true;
            }
        }
    }
    assert!(cast, "the shaman never cast: {:?}", sim.script.npc(shaman));
    let eq = &sim.actor(shaman).equipment;
    assert_eq!(eq.weapon_mode(), WeaponMode::Magic, "{eq:?}");
    let spell = eq.right.as_ref().map(|d| d.name.as_str());
    assert!(
        matches!(spell, Some("It_Spell_Fireball" | "It_Spell_Icelance")),
        "{spell:?}"
    );
    assert!(sim.actor(hero).hp < 200, "the missile hurt the hero");
}

#[test]
fn an_archer_draws_the_bow_at_range_and_the_sword_up_close() {
    let (mut sim, hero) = world();
    let mut a = Actor::npc(ActorId(0), "Reb_Warrior", Species::Human, 10, 30);
    a.alignment = Alignment::Reb;
    a.refresh(&SimConfig::default());
    let sword = sim.items.get("It_1H_Sword_01").cloned().unwrap();
    a.equipment.equip(sword);
    a.inventory.insert("It_Bow_Test", 1);
    a.inventory.insert("Arrow", 20);
    let id = sim.spawn(a, Pos::flat(1600.0, 0.0));
    sim.world.set_facing(id, std::f32::consts::PI);
    {
        let h = sim.actor_mut(hero);
        h.ai_controlled = false;
    }
    sim.start_attack(id, hero, AttackReason::Enemy);
    let mut arrows = 0;
    for _ in 0..400 {
        sim.step();
        for e in sim.drain_events() {
            if matches!(e.kind, EventKind::Projectile { spell: false, .. }) {
                arrows += 1;
            }
        }
        sim.actor_mut(hero).hp = 200;
    }
    assert_eq!(sim.actor(id).equipment.weapon_mode(), WeaponMode::Ranged);
    assert!(arrows >= 1, "no arrow flew");
    // The hero steps up: under 7 m the archer takes the sword again.
    gothic_sim::world::WorldQuery::teleport(&mut sim.world, hero, Pos::flat(1400.0, 0.0));
    sim.run_ticks(200);
    let eq = &sim.actor(id).equipment;
    assert_eq!(eq.weapon_mode(), WeaponMode::Melee, "{eq:?}");
    assert_eq!(
        eq.right.as_ref().map(|d| d.name.as_str()),
        Some("It_1H_Sword_01")
    );
}

#[test]
fn a_host_driven_swing_lands_when_the_clip_says_so() {
    let (mut sim, hero) = world();
    let mut o = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
    o.alignment = Alignment::Orc;
    o.refresh(&SimConfig::default());
    o.base_attitude_to_player = Attitude::None;
    let w = gothic_sim::data::default_weapon(&sim.items, o.species).unwrap();
    o.equipment.equip(w);
    let orc = sim.spawn(o, Pos::flat(300.0, 0.0));
    sim.world.set_facing(orc, std::f32::consts::PI);
    sim.actor_mut(hero).ai_controlled = false;
    sim.set_host_drives_npc(orc, true);
    sim.start_attack(orc, hero, AttackReason::Enemy);
    // The host: a Hit clip lasts 1 s, the blow comes at 0.6 s, the other clips 0.3 s.
    let mut pending: Vec<(u32, MovePhase, u64)> = Vec::new();
    let mut moves: Vec<(se::Action, MovePhase, f32)> = Vec::new();
    let mut first_hit_tick = None;
    let mut first_blow_tick = None;
    let mut started_at = None;
    for t in 0..1200u64 {
        sim.step();
        for e in sim.drain_events() {
            match e.kind {
                EventKind::NpcMove {
                    actor,
                    seq,
                    action,
                    phase,
                    speed,
                    ..
                } if actor == orc => {
                    moves.push((action, phase, speed));
                    if phase == MovePhase::Hit && started_at.is_none() {
                        started_at = Some(t);
                    }
                    pending.push((seq, phase, t));
                }
                EventKind::Hit { attacker, .. } if attacker == orc => {
                    first_hit_tick.get_or_insert(t);
                }
                _ => {}
            }
        }
        let tick_ms = u64::from(sim.cfg.tick_ms);
        let mut keep = Vec::new();
        for (seq, phase, t0) in pending.drain(..) {
            let age = (t - t0) * tick_ms;
            let len = if phase == MovePhase::Hit { 1000 } else { 300 };
            if phase == MovePhase::Hit && age >= 600 {
                sim.npc_move_hit(orc, seq);
                first_blow_tick.get_or_insert(t);
            }
            if age >= len {
                sim.npc_exec_done(orc, seq);
            } else if phase == MovePhase::Hit && age < 600 {
                keep.push((seq, phase, t0));
            } else {
                keep.push((seq, phase, t0));
            }
        }
        pending = keep;
        if first_hit_tick.is_some() {
            break;
        }
    }
    let hit = first_hit_tick.expect("the orc never hit");
    let blow = first_blow_tick.expect("the host never reported a blow");
    // The event of the blow is drained one step after the host reported it.
    assert_eq!(
        hit,
        blow + 1,
        "the damage lands with the host's blow, not before"
    );
    // The first move asked is a clip of an attack, played at the ported speed: the halberd
    // of an orc is a two-hander (0.7 for the plain attack, S:42a0).
    let (a, _, speed) = moves
        .iter()
        .copied()
        .find(|m| m.1 == MovePhase::Hit && m.0 != se::Action::JumpBack)
        .expect("a Hit clip");
    assert!(
        matches!(
            a,
            se::Action::Attack
                | se::Action::PowerAttack
                | se::Action::QuickAttackL
                | se::Action::QuickAttackR
                | se::Action::SimpleWhirl
                | se::Action::WhirlAttack
                | se::Action::HackAttack
        ),
        "{a:?} {moves:?}"
    );
    assert!(speed > 0.0 && speed <= 1.5, "{a:?} at {speed}");
    let _ = started_at;
}

#[test]
fn a_hit_makes_a_host_driven_victim_play_the_stumble_of_the_result() {
    let (mut sim, hero) = world();
    sim.state.level = 1;
    let mut o = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
    o.alignment = Alignment::Orc;
    o.refresh(&SimConfig::default());
    o.base_attitude_to_player = Attitude::None;
    let w = gothic_sim::data::default_weapon(&sim.items, o.species).unwrap();
    o.equipment.equip(w);
    let orc = sim.spawn(o, Pos::flat(150.0, 0.0));
    sim.actor_mut(hero).ai_controlled = false;
    sim.set_host_drives_npc(orc, true);
    sim.equip(hero, "It_1H_Sword_01").ok();
    sim.drain_events();
    let out = sim.melee_hit(hero, orc);
    let ev = sim.drain_events();
    let stumble = ev.iter().find_map(|e| match &e.kind {
        EventKind::NpcMove {
            actor,
            action,
            phase,
            ..
        } if *actor == orc => Some((*action, *phase)),
        _ => None,
    });
    if out.result.is_parry() || out.damage == 0 {
        return;
    }
    // The plan starts with the next tick of the victim.
    sim.run_ticks(2);
    let ev2 = sim.drain_events();
    let found = stumble.or_else(|| {
        ev2.iter().find_map(|e| match &e.kind {
            EventKind::NpcMove {
                actor,
                action,
                phase,
                ..
            } if *actor == orc => Some((*action, *phase)),
            _ => None,
        })
    });
    let (action, phase) = found.expect("the victim plays a reaction clip");
    assert_eq!(phase, MovePhase::Hit);
    assert!(
        matches!(
            action,
            se::Action::QuickStumble
                | se::Action::StumbleL
                | se::Action::StumbleR
                | se::Action::PierceStumble
                | se::Action::ParadeStumbleL
                | se::Action::ParadeStumbleR
                | se::Action::HeavyParadeStumble
        ),
        "{action:?} for {:?}",
        out.result
    );
}
