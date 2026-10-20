//! Tests of the magic port against the in-memory host.

use crate::enums::{Action, AiMode, DamageType, PartyMemberType, Slot, Species, SpellDeity};
use crate::host::{AiCommand, EntityApi, EntityId, InventoryApi, NpcApi, Pool, Pos, StatsApi};
use crate::magic::api::{
    CastType, MagicApi, MagicEvent, PathStyle, SpellDamage, SpellInfo, SpellType,
};
use crate::magic::area::{AreaEffect, AreaKind};
use crate::magic::cast::{self, Phase};
use crate::magic::effects;
use crate::player::commands;
use crate::routine::state::State;
use crate::stats;
use crate::testhost::TestWorld;

fn fireball() -> SpellInfo {
    let mut s = SpellInfo::new("Spl_Fireball", SpellType::Projectile, CastType::Power);
    s.deity = SpellDeity::Innos;
    s.needs_focus = true;
    s.min_mana = 15;
    s.max_mana = 30;
    s.spawn = "Mis_Fireball".into();
    s.damage = Some(SpellDamage {
        damage_type: DamageType::Fire,
        amount: 50,
        mana_mult: 2.0,
        hit_mult: 1.0,
    });
    s
}

fn summon_skeleton() -> SpellInfo {
    let mut s = SpellInfo::new("Spl_SummonSkeleton", SpellType::Summon, CastType::Regular);
    s.deity = SpellDeity::Beliar;
    s.min_mana = 60;
    s.max_mana = 60;
    s.func_on_cast = "MagicInstant".into();
    s.func_on_hit = "MagicSummonSkeleton".into();
    s
}

/// A world with the hero holding a spell item and an orc in front of him.
fn world_with(spell: SpellInfo, category: u32, permanent: bool) -> (TestWorld, EntityId, EntityId) {
    let mut w = TestWorld::new();
    let p = w.player();
    let name = spell.name.clone();
    w.add_spell(spell);
    w.set_spell_slot(p, Slot::RightHand, &name, category);
    w.set_permanent(p, Slot::RightHand, permanent);
    let orc = w.spawn_npc("Orc", Species::Orc, 10, 10);
    w.set_position(orc, Pos::new(500.0, 0.0, 0.0));
    w.set_task_directly(p, State::PsMagic);
    (w, p, orc)
}

fn drive(w: &mut TestWorld, p: EntityId, rounds: usize) -> Vec<AiCommand> {
    let mut all = Vec::new();
    for _ in 0..rounds {
        commands::run(w, p);
        all.extend(w.finish_commands(p));
    }
    all
}

#[test]
fn the_hero_casts_a_fireball_at_an_orc() {
    let (mut w, p, orc) = world_with(fireball(), 9, true);
    drive(&mut w, p, 6);
    assert_eq!(w.current_state(p), Some(State::PsMagicLoop));
    let state = commands::cast_spell(&mut w, p, orc, false).expect("cast");
    assert_eq!(state, State::PsMagicCast, "a short press is a plain cast");
    drive(&mut w, p, 8);
    assert_eq!(w.mg.projectiles.len(), 1);
    let shot = &w.mg.projectiles[0];
    assert_eq!(shot.owner, p);
    assert_eq!(shot.target, orc);
    assert_eq!(shot.template, "Mis_Fireball");
    assert_eq!(shot.damage_type, DamageType::Fire);
    assert_eq!(shot.amount, 50);
    assert_eq!(shot.mana_used, 15, "the load phase pays the minimum");
    assert_eq!(shot.path, PathStyle::Line);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 85);
    assert_eq!(w.current_state(p), Some(State::PsMagicLoop));
    assert!(
        w.magic_events()
            .iter()
            .any(|e| matches!(e, MagicEvent::Cast { spell, .. } if spell == "Spl_Fireball"))
    );
}

#[test]
fn a_power_cast_pays_the_maximum_and_homes_in() {
    let (mut w, p, orc) = world_with(fireball(), 9, true);
    drive(&mut w, p, 6);
    assert_eq!(cast::get_cast_action(&w, p), Action::PowerCast);
    let state = commands::cast_spell(&mut w, p, orc, true).expect("cast");
    assert_eq!(state, State::PsMagicPowerCast);
    drive(&mut w, p, 8);
    let shot = &w.mg.projectiles[0];
    assert_eq!(shot.mana_used, 30);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 70);
}

#[test]
fn too_little_mana_downgrades_the_power_cast_and_fizzles_a_cast() {
    let (mut w, p, orc) = world_with(fireball(), 9, true);
    w.pm_set_pool(Pool::Mana, 20);
    assert_eq!(
        cast::get_cast_action(&w, p),
        Action::Cast,
        "20 mana is short of the 30 a power cast needs"
    );
    w.pm_set_pool(Pool::Mana, 5);
    drive(&mut w, p, 6);
    commands::cast_spell(&mut w, p, orc, false).expect("cast");
    drive(&mut w, p, 6);
    assert!(w.mg.projectiles.is_empty());
    assert_eq!(stats::pool(&w, p, Pool::Mana), 5);
    assert!(
        w.magic_events()
            .iter()
            .any(|e| matches!(e, MagicEvent::Effect { name, .. } if name == "eff_magic_fizzle_01"))
    );
    assert_eq!(w.current_state(p), Some(State::PsMagicLoop));
}

#[test]
fn a_scroll_costs_mana_and_is_used_up_but_a_potion_is_free() {
    let (mut w, p, orc) = world_with(fireball(), 3, false);
    drive(&mut w, p, 6);
    commands::cast_spell(&mut w, p, orc, false).expect("cast");
    drive(&mut w, p, 8);
    assert_eq!(w.mg.projectiles.len(), 1);
    assert_eq!(
        stats::pool(&w, p, Pool::Mana),
        85,
        "scrolls pay the load cost"
    );
    assert_eq!(
        w.find_stack_index(p, Slot::RightHand),
        -1,
        "the scroll is gone"
    );
    assert_eq!(
        w.current_task(p),
        Some(State::PsNormal),
        "without a spell the hero is back to normal"
    );
    // A potion (category 4) is cast without paying mana.
    let (mut w, p, orc) = world_with(fireball(), cast::CATEGORY_ALCHEMY, false);
    drive(&mut w, p, 6);
    commands::cast_spell(&mut w, p, orc, false).expect("cast");
    drive(&mut w, p, 8);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 100);
}

#[test]
fn aborting_a_cast_returns_the_mana() {
    let (mut w, p, _orc) = world_with(fireball(), 9, true);
    let stack = w.find_stack_index(p, Slot::RightHand);
    cast::start_cast_phase(&mut w, p, EntityId::NONE, stack, Phase::Load);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 85);
    cast::start_cast_phase(&mut w, p, EntityId::NONE, stack, Phase::Abort);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 100);
    assert_eq!(w.mana_used(p), 0);
}

#[test]
fn a_summon_spell_brings_a_skeleton_into_the_party() {
    let (mut w, p, _orc) = world_with(summon_skeleton(), 9, true);
    drive(&mut w, p, 6);
    assert_eq!(cast::get_cast_action(&w, p), Action::Summon);
    commands::cast_spell(&mut w, p, EntityId::NONE, false).expect("cast");
    drive(&mut w, p, 8);
    assert_eq!(w.mg.spawned.len(), 1);
    let (template, npc) = w.mg.spawned[0].clone();
    assert_eq!(template, "Skeleton_Sword");
    assert_eq!(w.party_leader(npc), p);
    assert_eq!(w.party_member_type(npc), PartyMemberType::Summoned);
    assert_eq!(w.current_task(npc), Some(State::FollowPlayer));
    assert_eq!(stats::pool(&w, p, Pool::Mana), 40);
    assert!(
        w.magic_events()
            .iter()
            .any(|e| matches!(e, MagicEvent::Summoned { owner, .. } if *owner == p))
    );
}

#[test]
fn army_spells_scale_with_intelligence() {
    let mut w = TestWorld::new();
    let p = w.player();
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 100);
    effects::magic_summon_army_of_cuteness(&mut w, p);
    assert_eq!(w.mg.spawned.len(), 6);
    w.mg.spawned.clear();
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 260);
    effects::magic_summon_army_of_darkness(&mut w, p);
    assert_eq!(w.mg.spawned.len(), 8, "four skeletons and four zombies");
    w.mg.spawned.clear();
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 320);
    effects::magic_summon_army_of_cuteness(&mut w, p);
    assert_eq!(w.mg.spawned.len(), 8);
}

#[test]
fn status_spells_set_and_clear_the_bits() {
    let mut w = TestWorld::new();
    let p = w.player();
    let orc = w.spawn_npc("Orc", Species::Orc, 10, 10);
    effects::magic_burn(&mut w, p, orc);
    effects::magic_freeze(&mut w, p, orc);
    assert!(w.has_status(orc, effects::STATUS_BURNING | effects::STATUS_FROZEN));
    effects::magic_heal_other(&mut w, orc);
    assert!(!w.has_status(orc, effects::STATUS_BURNING));
    assert!(!w.has_status(orc, effects::STATUS_FROZEN));
    assert_eq!(
        stats::pool(&w, orc, Pool::Hit),
        stats::pool_max(&w, orc, Pool::Hit)
    );
    // The poison needs a victim that can be poisoned; the victim turns on the caster either way.
    effects::magic_poison(&mut w, p, orc);
    assert!(w.has_status(orc, effects::STATUS_POISONED));
    assert_eq!(w.current_task(orc), Some(State::Attack));
    effects::magic_cure_poison(&mut w, orc);
    assert!(!w.has_status(orc, effects::STATUS_POISONED));
    // Cure and refresh fill the hero's pools.
    w.pm_set_pool(Pool::Hit, 20);
    w.pm_set_pool(Pool::Mana, 1);
    effects::magic_cure(&mut w, p);
    effects::magic_refresh(&mut w, p);
    assert_eq!(stats::pool(&w, p, Pool::Hit), 200);
    assert_eq!(stats::pool(&w, p, Pool::Mana), 100);
}

#[test]
fn stun_takes_the_nearest_fighters_but_spares_the_party_and_the_guards() {
    let mut w = TestWorld::new();
    let p = w.player();
    let near = w.spawn_npc("Near", Species::Human, 5, 5);
    let ally = w.spawn_npc("Ally", Species::Human, 5, 5);
    let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
    let far = w.spawn_npc("Far", Species::Human, 5, 5);
    w.set_position(near, Pos::new(300.0, 0.0, 0.0));
    w.set_position(ally, Pos::new(400.0, 0.0, 0.0));
    w.set_position(guard, Pos::new(500.0, 0.0, 0.0));
    w.set_position(far, Pos::new(2000.0, 0.0, 0.0));
    w.set_leader(ally, p, PartyMemberType::Party, false);
    w.set_task_directly(guard, State::Guard);
    effects::magic_stun(&mut w, p);
    assert_eq!(w.current_task(near), Some(State::Stunned));
    assert_ne!(w.current_task(ally), Some(State::Stunned));
    assert_ne!(w.current_task(guard), Some(State::Stunned));
    assert_ne!(w.current_task(far), Some(State::Stunned));
}

#[test]
fn the_stun_state_runs_eight_seconds_and_stands_up() {
    let mut w = TestWorld::new();
    let orc = w.spawn_npc("Orc", Species::Orc, 5, 5);
    w.set_task_directly(orc, State::Stunned);
    for _ in 0..3 {
        crate::routine::work::run_state(&mut w, orc);
        w.finish_commands(orc);
    }
    assert_eq!(w.current_state(orc), Some(State::StunnedLoop));
    w.set_task_time_value(orc, 8.5);
    for _ in 0..4 {
        crate::routine::work::run_state(&mut w, orc);
        w.finish_commands(orc);
    }
    assert_ne!(w.current_state(orc), Some(State::StunnedLoop));
}

#[test]
fn taming_and_transforming_have_species_gates() {
    let mut w = TestWorld::new();
    let p = w.player();
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 50);
    let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
    let ripper = w.spawn_npc("Ripper", Species::Ripper, 5, 5);
    let orc = w.spawn_npc("Orc", Species::Orc, 5, 5);
    let pig = w.spawn_npc("Pig", Species::Pig, 1, 1);
    assert!(effects::can_tame_animal(&mut w, p, wolf));
    assert!(
        !effects::can_tame_animal(&mut w, p, ripper),
        "a ripper needs Intelligence 100"
    );
    assert!(!effects::can_tame_animal(&mut w, p, orc), "not an animal");
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 100);
    assert!(effects::can_tame_animal(&mut w, p, ripper));
    assert!(effects::can_transform_into(&mut w, p, wolf));
    assert!(!effects::can_transform_into(&mut w, p, pig), "never a pig");
    let keys: Vec<_> = w
        .magic_events()
        .iter()
        .filter_map(|e| match e {
            MagicEvent::Message { key, .. } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(keys, ["TooHard", "Impossible", "Impossible"]);
    effects::magic_tame_animal(&mut w, p, wolf);
    assert_eq!(w.party_member_type(wolf), PartyMemberType::Controlled);
    assert_eq!(w.party_leader(wolf), p);
}

#[test]
fn transformation_lasts_by_intelligence_perk_and_size() {
    let mut w = TestWorld::new();
    let p = w.player();
    let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
    let ripper = w.spawn_npc("Ripper", Species::Ripper, 5, 5);
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 100);
    assert_eq!(effects::transform_duration(&w, p, wolf), 100.0);
    assert_eq!(
        effects::transform_duration(&w, p, ripper),
        50.0,
        "large form"
    );
    w.grant_skill(p, "Perk_Druid");
    assert_eq!(effects::transform_duration(&w, p, wolf), 200.0);
    w.grant_skill(p, "Perk_Watermage");
    assert_eq!(effects::transform_duration(&w, p, wolf), 250.0);
}

#[test]
fn transform_uses_the_spell_spawn_and_ends_with_transform_back() {
    let mut w = TestWorld::new();
    let p = w.player();
    let mut s = SpellInfo::new("Spl_Transform_Wolf", SpellType::Target, CastType::Regular);
    s.func_on_hit = "MagicTransform".into();
    s.spawn = "Wolf".into();
    w.add_spell(s);
    w.pm_set_attribute(crate::host::Attribute::Intelligence, 60);
    w.set_cast_spell(p, "Spl_Transform_Wolf");
    assert!(effects::call_spell_function(
        &mut w,
        "MagicTransform",
        p,
        EntityId::NONE,
        0
    ));
    assert_eq!(w.mg.transforms.len(), 1);
    assert_eq!(w.mg.transforms[0].2, 60.0);
    assert!(w.is_transformed(p));
    effects::transform_back(&mut w, p);
    assert!(!w.is_transformed(p));
    assert_eq!(w.mg.transforms_ended, vec![p]);
}

#[test]
fn unknown_spell_functions_do_nothing() {
    let mut w = TestWorld::new();
    let p = w.player();
    assert!(!effects::call_spell_function(
        &mut w,
        "MagicResurrect",
        p,
        EntityId::NONE,
        0
    ));
    assert!(effects::call_spell_function(
        &mut w,
        "MagicTimeBubble",
        p,
        EntityId::NONE,
        0
    ));
}

#[test]
fn the_flame_sword_has_its_own_animation_and_the_dusk_spell_needs_darkness() {
    let mut s = SpellInfo::new("Spl_Flamesword", SpellType::Target, CastType::Regular);
    s.min_mana = 10;
    let (w, p, _) = world_with(s, 9, true);
    assert_eq!(cast::get_cast_action(&w, p), Action::FlameSword);

    let mut s = SpellInfo::new("Spl_DuskToDawn", SpellType::Summon, CastType::Regular);
    s.min_mana = 10;
    let (mut w, p, _) = world_with(s, 9, true);
    w.hour = 12.0;
    assert!(
        !cast::can_cast(&mut w, p, EntityId::NONE),
        "not in daylight"
    );
    w.hour = 23.0;
    assert!(cast::can_cast(&mut w, p, EntityId::NONE));
}

#[test]
fn destroy_evil_needs_an_evil_target() {
    let mut s = SpellInfo::new(
        "Spl_DestroyEvil",
        SpellType::Projectile,
        CastType::Immediate,
    );
    s.min_mana = 10;
    s.needs_focus = true;
    let (mut w, p, orc) = world_with(s, 9, true);
    let skeleton = w.spawn_npc("Skel", Species::Skeleton, 5, 5);
    assert!(!cast::can_cast(&mut w, p, orc));
    assert!(cast::can_cast(&mut w, p, skeleton));
    assert!(!cast::can_cast(&mut w, p, EntityId::NONE));
}

// ---- area spells ------------------------------------------------------------------------------------

#[test]
fn a_sweep_wave_hits_each_target_once_with_falling_damage() {
    let mut w = TestWorld::new();
    let p = w.player();
    let near = w.spawn_npc("Near", Species::Human, 5, 5);
    let far = w.spawn_npc("Far", Species::Human, 5, 5);
    let out = w.spawn_npc("Out", Species::Human, 5, 5);
    w.set_position(near, Pos::new(0.0, 0.0, 100.0));
    w.set_position(far, Pos::new(0.0, 0.0, 500.0));
    w.set_position(out, Pos::new(0.0, 0.0, 1500.0));
    let mut fx = AreaEffect::new(
        &mut w,
        AreaKind::Heatwave,
        p,
        "Spl_HeatWave",
        Pos::default(),
        0,
    );
    let mut alive = true;
    for _ in 0..10 {
        if !fx.tick(&mut w, 16) {
            alive = false;
            break;
        }
    }
    assert!(!alive, "the wave ends when nothing is left to hit");
    let hits = &w.mg.hits;
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].1, near);
    assert_eq!(hits[1].1, far);
    // 400 at the centre falling linearly to 1 at 1000: 400 - 0.1 * 399 and 400 - 0.5 * 399.
    assert_eq!(hits[0].2.amount, 360);
    assert_eq!(hits[1].2.amount, 201);
    assert_eq!(hits[0].2.damage_type, DamageType::Fire);
}

#[test]
fn meteor_waits_half_a_second_and_the_rain_has_a_hit_chance() {
    let mut w = TestWorld::new();
    let p = w.player();
    let victim = w.spawn_npc("Victim", Species::Human, 5, 5);
    w.set_position(victim, Pos::new(0.0, 0.0, 200.0));
    let mut fx = AreaEffect::new(&mut w, AreaKind::Meteor, p, "Spl_Meteor", Pos::default(), 0);
    fx.tick(&mut w, 400);
    assert!(w.mg.hits.is_empty(), "the meteor is still falling");
    fx.tick(&mut w, 200);
    assert_eq!(w.mg.hits.len(), 1);
    assert_eq!(
        w.mg.hits[0].2.amount, 600,
        "1000 at the centre, 1 at 500: 200 cm out"
    );

    // Firerain: one candidate gives 33 + 5 percent; a roll of 40 misses, a roll of 38 hits.
    w.mg.hits.clear();
    w.rolls = vec![3, 40, 0, 7];
    let mut rain = AreaEffect::new(
        &mut w,
        AreaKind::Firerain,
        p,
        "Spl_Firerain",
        Pos::default(),
        20,
    );
    assert_eq!(rain.duration_ms, 11_000, "8 s plus the roll of 3");
    rain.tick(&mut w, 400);
    assert!(w.mg.hits.is_empty(), "40 is above 38");
    w.rolls = vec![38, 0, 7];
    rain.tick(&mut w, 400);
    assert_eq!(w.mg.hits.len(), 1);
    let hit = &w.mg.hits[0].2;
    assert_eq!(hit.amount, 57, "50 + rand(50)");
    assert_eq!(hit.mana_used, 20);
    rain.tick(&mut w, 10_000);
    assert!(
        !rain.tick(&mut w, 1_000),
        "after eleven seconds the rain is over"
    );
}

#[test]
fn area_effects_skip_the_owner_and_his_party() {
    let mut w = TestWorld::new();
    let p = w.player();
    let ally = w.spawn_npc("Ally", Species::Human, 5, 5);
    w.set_leader(ally, p, PartyMemberType::Party, false);
    w.set_position(ally, Pos::new(0.0, 0.0, 50.0));
    let mut fx = AreaEffect::new(
        &mut w,
        AreaKind::Frostwave,
        p,
        "Spl_FrostWave",
        Pos::default(),
        0,
    );
    assert!(!fx.tick(&mut w, 16));
    assert!(w.mg.hits.is_empty());
}

#[test]
fn light_and_the_time_bubble_last_eighty_to_one_hundred_ten_seconds() {
    let mut w = TestWorld::new();
    let p = w.player();
    w.rolls = vec![2];
    let fx = AreaEffect::new(&mut w, AreaKind::Light, p, "Spl_Light", Pos::default(), 0);
    assert_eq!(fx.duration_ms, 100_000);
    w.grant_skill(p, "Perk_Firemage");
    let mut s = SpellInfo::new("Spl_Light", SpellType::Summon, CastType::Regular);
    s.deity = SpellDeity::Innos;
    w.add_spell(s);
    w.rolls = vec![3];
    let fx = AreaEffect::new(&mut w, AreaKind::Light, p, "Spl_Light", Pos::default(), 0);
    assert_eq!(
        fx.duration_ms, 165_000,
        "110 s plus the school bonus of half"
    );
}

#[test]
fn freezing_holds_the_victim_until_the_ice_melts() {
    let mut w = TestWorld::new();
    let orc = w.spawn_npc("Orc", Species::Orc, 5, 5);
    w.set_task_directly(orc, State::Freeze);
    w.set_task_position(orc, 60);
    crate::routine::work::run_state(&mut w, orc);
    assert!(w.has_status(orc, effects::STATUS_FROZEN));
    assert_eq!(w.ai_mode(orc), AiMode::Sleep);
    crate::routine::work::run_state(&mut w, orc);
    assert!(
        w.has_status(orc, effects::STATUS_FROZEN),
        "six seconds have not passed"
    );
    w.set_task_time_value(orc, 6.1);
    crate::routine::work::run_state(&mut w, orc);
    assert!(!w.has_status(orc, effects::STATUS_FROZEN));
}

#[test]
fn telekinesis_and_teleport_reach_the_host() {
    let mut w = TestWorld::new();
    let p = w.player();
    let companion = w.spawn_npc("Friend", Species::Human, 5, 5);
    w.set_leader(companion, p, PartyMemberType::Party, false);
    w.set_cast_spell(p, "Spl_Teleport_KapDun");
    effects::magic_teleport(&mut w, p);
    assert!(w.mg.teleports.contains(&(p, "Teleport_KapDun".into())));
    assert!(
        w.mg.teleports
            .contains(&(companion, "Teleport_KapDun".into()))
    );
    let other = w.spawn_npc("Thing", Species::Human, 5, 5);
    effects::magic_telekinesis(&mut w, p, other);
    assert_eq!(w.mg.telekinesis, vec![(p, other)]);
}

#[test]
fn the_mist_befriends_friends_and_angers_strangers() {
    let mut w = TestWorld::new();
    let p = w.player();
    let friend = w.spawn_npc("Friend", Species::Human, 5, 5);
    let stranger = w.spawn_npc("Stranger", Species::Orc, 5, 5);
    let far = w.spawn_npc("Far", Species::Orc, 5, 5);
    w.set_position(friend, Pos::new(100.0, 0.0, 0.0));
    w.set_position(stranger, Pos::new(0.0, 0.0, 500.0));
    w.set_position(far, Pos::new(0.0, 0.0, 5000.0));
    w.set_leader(friend, p, PartyMemberType::Party, false);
    let mut fx = AreaEffect::new(
        &mut w,
        AreaKind::Mist,
        p,
        "Spl_SummonMist",
        Pos::default(),
        0,
    );
    assert_eq!(fx.duration_ms, 20_000);
    fx.tick(&mut w, 1000);
    assert!(w.has_status(stranger, effects::STATUS_CONTROLLED));
    assert!(w.has_status(friend, effects::STATUS_CONTROLLED));
    assert!(!w.has_status(far, effects::STATUS_CONTROLLED));
    assert_eq!(
        w.current_task(stranger),
        Some(State::Attack),
        "strangers attack the caster"
    );
    // The mist ends: everybody is freed.
    assert!(!fx.tick(&mut w, 25_000));
    assert!(!w.has_status(stranger, effects::STATUS_CONTROLLED));
}

#[test]
fn the_no_aggro_mist_leaves_strangers_alone() {
    let mut w = TestWorld::new();
    let p = w.player();
    let stranger = w.spawn_npc("Stranger", Species::Orc, 5, 5);
    w.set_position(stranger, Pos::new(0.0, 0.0, 500.0));
    let mut fx = AreaEffect::new(
        &mut w,
        AreaKind::MistNoAggro,
        p,
        "Spl_SummonMist",
        Pos::default(),
        0,
    );
    fx.tick(&mut w, 1000);
    assert_ne!(w.current_task(stranger), Some(State::Attack));
}

#[test]
fn a_transformation_hands_the_enemies_over_to_the_animal_and_back() {
    let mut w = TestWorld::new();
    let p = w.player();
    let orc = w.spawn_npc("Orc", Species::Orc, 5, 5);
    w.set_target(orc, p);
    let mut s = SpellInfo::new("Spl_Transform_Wolf", SpellType::Target, CastType::Regular);
    s.spawn = "Wolf".into();
    w.add_spell(s);
    w.set_cast_spell(p, "Spl_Transform_Wolf");
    effects::magic_transform(&mut w, p);
    let wolf = w.mg.spawned[0].1;
    assert_eq!(w.current_target(orc), wolf);
    effects::transform_back(&mut w, p);
    assert_eq!(w.current_target(orc), p);
}

#[test]
fn soul_travel_leaves_the_body_and_the_ghost_state_waits() {
    let mut s = SpellInfo::new("Spl_SoulTravel", SpellType::Summon, CastType::Regular);
    s.min_mana = 20;
    s.max_mana = 20;
    s.func_on_cast = "MagicInstant".into();
    s.func_on_hit = "MagicSoulTravel".into();
    let (mut w, p, _orc) = world_with(s, 9, true);
    drive(&mut w, p, 6);
    commands::cast_spell(&mut w, p, EntityId::NONE, false).expect("cast");
    drive(&mut w, p, 10);
    assert!(w.has_status(p, effects::STATUS_GHOST | effects::STATUS_OUT_OF_BODY));
    assert_eq!(w.current_task(p), Some(State::PsGhost));
    assert_eq!(w.current_state(p), Some(State::PsGhostLoop));
    effects::return_to_body(&mut w, p);
    assert!(!w.has_status(p, effects::STATUS_GHOST));
}
