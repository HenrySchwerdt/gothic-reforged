//! Bridge from physical VR hits (`vr-melee` `HitMessage`, `vr-archery` `ArrowHitMessage`) to the current
//! `gothic-sim` ported combat pipeline (`Sim::resolve_ported`). Items and the orc record come from the user's own
//! extracted data (`items.json`, `orcs.json` in the data pack).
use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use gothic_sim::actor::Actor;
use gothic_sim::combat::{Attack, HitOutcome};
use gothic_sim::config::SimConfig;
use gothic_sim::data::{NpcFile, default_weapon};
use gothic_sim::item::ItemDb;
use gothic_sim::sim::Sim;
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};
use gothic_vr::VrPlayer;
use gothic_vr_archery::ArrowHitMessage;
use gothic_vr_melee::{HitMessage, PlayerAvatar};
use gothic_vr_npc::VrNpc;
use gothic_vr_npc::ragdoll::Stance;

#[derive(Component)]
pub struct DeadOrc;

#[derive(Resource)]
pub struct GothicCombat {
    pub items: ItemDb,
    pub hero: Actor,
    pub actors: HashMap<Entity, Actor>,
    sim: Sim<Grid2dWorld>,
    pub orc_template: Option<gothic_sim::data::NpcRecord>,
    pub kills: u32,
    pub hero_deaths: u32,
    next_id: u32,
}

impl GothicCombat {
    pub fn load(dir: &Path) -> Option<Self> {
        let items = ItemDb::from_json(&std::fs::read_to_string(dir.join("items.json")).ok()?).ok()?;
        let npcs = NpcFile::from_json(&std::fs::read_to_string(dir.join("orcs.json")).ok()?).ok()?;
        let cfg = SimConfig::default();
        let mut hero = Actor::hero(ActorId(0), &cfg);
        if let Some(sword) = items.get("It_1H_OrcSlayer_01") {
            hero.equipment.equip(sword.clone());
        }
        hero.action = Action::Attack;
        hero.drawn = true;
        let mut sim = Sim::new(cfg, Grid2dWorld::new(), items.clone());
        let hero_id = sim.spawn(hero.clone(), Pos::default());
        sim.hero = Some(hero_id);
        let hero = sim.actor(hero_id).clone();
        Some(Self {
            items,
            hero,
            actors: HashMap::new(),
            sim,
            orc_template: npcs.entities.into_iter().next(),
            kills: 0,
            hero_deaths: 0,
            next_id: 1,
        })
    }

    /// Creates the Gothic actor behind an orc entity.
    pub fn new_orc(&mut self, e: Entity) {
        let Some(rec) = &self.orc_template else { return };
        let id = ActorId(self.next_id);
        self.next_id += 1;
        let mut a = rec.to_actor(id, &self.sim.cfg, &self.items);
        if let Some(w) = default_weapon(&self.items, a.species) {
            a.equipment.equip(w);
        }
        a.action = Action::Attack;
        a.ai_mode = AiMode::Combat;
        a.drawn = true;
        let id = self.sim.spawn(a.clone(), Pos::default());
        a = self.sim.actor(id).clone();
        self.actors.insert(e, a);
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (apply_melee, apply_arrows, keep_dead_down));
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_melee(
    mut hits: MessageReader<HitMessage>,
    mut gc: Option<ResMut<GothicCombat>>,
    avatar: Query<Entity, With<PlayerAvatar>>,
    mut commands: Commands,
    mut npcs: Query<&mut VrNpc>,
    mut player: ResMut<VrPlayer>,
    mut fade: ResMut<gothic_xr_quest::XrFade>,
    time: Res<Time>,
    mut last_hit: Local<HashMap<Entity, f32>>,
) {
    let Some(gc) = gc.as_mut() else {
        hits.clear();
        return;
    };
    let me = avatar.iter().next();
    let now = time.elapsed_secs();
    for h in hits.read() {
        let gc = &mut **gc;
        // Haptics: a blow you land (scaled by impact speed) or take.
        let amp = if Some(h.victim) == me { 1.0 } else { (0.25 + h.impact_speed / 8.0).clamp(0.25, 1.0) };
        gothic_xr_quest::haptic_pulse(0, amp);
        gothic_xr_quest::haptic_pulse(1, amp);
        if Some(h.victim) == me {
            // An orc hit the hero.
            let Some(att_e) = h.attacker else { continue };
            if last_hit.get(&att_e).is_some_and(|t| now - t < 0.6) {
                continue;
            }
            last_hit.insert(att_e, now);
            let Some(orc) = gc.actors.get(&att_e).cloned() else { continue };
            let attack = Attack::melee(&orc);
            let out = resolve_ported(&mut gc.sim, &orc, &mut gc.hero, &attack);
            info!("[combat] orc hits hero: damage {} hp {} result {:?}", out.damage, gc.hero.hp, out.result);
            if gc.hero.hp <= 0 {
                gc.hero_deaths += 1;
                gc.hero.refresh(&gc.sim.cfg);
                gc.hero.ai_mode = AiMode::Routine;
                let p = player.ctrl.position;
                player.ctrl.teleport(p + Vec3::Y * 50.0);
                fade.0 = 1.0;
                info!("[combat] the hero fell and got back up (deaths {})", gc.hero_deaths);
            }
        } else if h.attacker == me {
            // The hero hit something.
            if last_hit.get(&h.victim).is_some_and(|t| now - t < 0.35) {
                continue;
            }
            let Some(victim) = gc.actors.get_mut(&h.victim) else { continue };
            last_hit.insert(h.victim, now);
            let mut attack = Attack::melee(&gc.hero);
            attack.head_hit = matches!(h.bone, gothic_vr_melee::Bone::Head | gothic_vr_melee::Bone::Neck);
            let hero = gc.hero.clone();
            let out = resolve_ported(&mut gc.sim, &hero, victim, &attack);
            info!("[combat] hero hits orc: damage {} hp {} result {:?}", out.damage, victim.hp, out.result);
            if (victim.hp <= 0 || out.killed() || out.defeated()) && let Ok(mut n) = npcs.get_mut(h.victim) {
                n.ragdoll.set_stance(Stance::Fallen);
                n.ai.cooldown = f32::INFINITY;
                victim.ai_mode = AiMode::Dead;
                commands.entity(h.victim).insert(DeadOrc);
                gc.kills += 1;
            }
        }
    }
}

fn apply_arrows(
    mut hits: MessageReader<ArrowHitMessage>,
    mut gc: Option<ResMut<GothicCombat>>,
    mut commands: Commands,
    mut npcs: Query<&mut VrNpc>,
) {
    let Some(gc) = gc.as_mut() else {
        hits.clear();
        return;
    };
    for h in hits.read() {
        let gc = &mut **gc;
        let Some(victim) = gc.actors.get_mut(&h.victim) else { continue };
        // Gothic: arrow damage scales with the draw time (0.2..1.0); the hunting bow's damage is the ammo figure.
        let mut ammo = gc.items.get("Arrow").cloned().unwrap_or_default();
        if let Some(bow) = gc.items.get("It_Bow_Hunting") {
            ammo.damage = bow.damage.clone();
        }
        let mult = 0.2 + 0.8 * h.draw_factor.clamp(0.0, 1.0);
        let attack = Attack::projectile(&ammo, mult);
        let hero = gc.hero.clone();
        let out = resolve_ported(&mut gc.sim, &hero, victim, &attack);
        info!("[combat] arrow hits orc: damage {} hp {} (draw {:.2})", out.damage, victim.hp, h.draw_factor);
        if (victim.hp <= 0 || out.killed() || out.defeated()) && let Ok(mut n) = npcs.get_mut(h.victim) {
            n.ragdoll.set_stance(Stance::Fallen);
            n.ai.cooldown = f32::INFINITY;
            victim.ai_mode = AiMode::Dead;
            commands.entity(h.victim).insert(DeadOrc);
            gc.kills += 1;
        }
    }
}

/// Keep the Quest-facing actor copies synchronized with the Sim-owned state while delegating
/// damage rules to the current scripted pipeline. `resolve_ported` deliberately does not own
/// the VR entity's death/ragdoll aftermath; the callers below retain that presentation work.
fn resolve_ported(
    sim: &mut Sim<Grid2dWorld>,
    attacker: &Actor,
    victim: &mut Actor,
    attack: &Attack,
) -> HitOutcome {
    let mut attacker_state = attacker.clone();
    if !attacker_state.is_player {
        // The collision message names the actual victim. Feed that confirmed target to the
        // Script-backed DoLogicalDamage gate, which reads Npc.current_target from Sim state.
        attacker_state.target = Some(victim.id);
    }
    *sim.actor_mut(attacker.id) = attacker_state;
    *sim.actor_mut(victim.id) = victim.clone();
    let out = sim.resolve_ported(attacker.id, victim.id, attack);
    *victim = sim.actor(victim.id).clone();
    out
}

/// Dead orcs stay down.
fn keep_dead_down(mut q: Query<&mut VrNpc, With<DeadOrc>>) {
    for mut n in &mut q {
        if n.ragdoll.stance() != Stance::Fallen {
            n.ragdoll.set_stance(Stance::Fallen);
        }
        n.ai.cooldown = f32::INFINITY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_bridge(npc_attacks_hero: bool) -> (Sim<Grid2dWorld>, Actor, Actor, Attack) {
        // Follow crates/sim/tests/common::{sim_with,armed_hero}: the player identity,
        // progression perks, and equipped weapon are part of a valid Sim damage fixture.
        let mut items = ItemDb::default();
        let mut sword = gothic_sim::item::ItemDef::weapon("It_1H_Sword_01", "1H", "Blade", 50);
        sword.requirements.push(gothic_sim::item::Requirement {
            tag: "STR".into(),
            op: 0,
            value: 100,
        });
        sword.required_skills.push("Perk_1H_1".into());
        items.insert(sword);

        let cfg = SimConfig::default();
        let mut sim = Sim::new(cfg, Grid2dWorld::new(), items);
        sim.state.level = 1;
        sim.state.xp = 500;
        sim.state.skills.insert("perk_1h_1".into());
        sim.player_alignment = Alignment::Reb;
        let hero_id = sim.spawn_hero(Pos::flat(0.0, 0.0));
        sim.equip(hero_id, "It_1H_Sword_01")
            .expect("synthetic hero equipment");
        sim.actor_mut(hero_id).action = Action::Attack;
        sim.actor_mut(hero_id).drawn = true;

        let mut orc = Actor::npc(ActorId(0), "Synthetic Orc", Species::Orc, 10, 10);
        orc.alignment = Alignment::Orc;
        orc.action = Action::Attack;
        orc.ai_mode = AiMode::Combat;
        orc.drawn = true;
        orc.equipment.equip(gothic_sim::item::ItemDef::weapon(
            "It_Orc_Test_Sword",
            "1H",
            "Blade",
            40,
        ));
        let orc_id = sim.spawn(orc, Pos::flat(100.0, 0.0));
        let (attacker_id, victim_id) = if npc_attacks_hero {
            (orc_id, hero_id)
        } else {
            (hero_id, orc_id)
        };
        let attacker = sim.actor(attacker_id).clone();
        let victim = sim.actor(victim_id).clone();
        let attack = Attack::melee(&attacker);
        (sim, attacker, victim, attack)
    }

    #[test]
    fn hero_hit_copies_ported_hp_mutation_to_visible_orc_once() {
        let (mut sim, attacker, mut visible_victim, attack) = synthetic_bridge(false);
        let before = visible_victim.hp;

        let out = resolve_ported(&mut sim, &attacker, &mut visible_victim, &attack);

        assert!(out.hp_lost > 0, "synthetic hero melee hit should apply damage");
        assert_eq!(before - visible_victim.hp, out.hp_lost);
        assert_eq!(visible_victim.hp, sim.actor(visible_victim.id).hp);
    }

    #[test]
    fn npc_hits_hero_and_repeated_bridge_hits_apply_one_delta_each() {
        let (mut sim, attacker, mut visible_victim, attack) = synthetic_bridge(true);
        let initial = visible_victim.hp;
        let mut total_loss = 0;

        for _ in 0..2 {
            let before = visible_victim.hp;
            let out = resolve_ported(&mut sim, &attacker, &mut visible_victim, &attack);
            assert!(out.hp_lost > 0, "each synthetic orc melee hit should apply damage");
            assert_eq!(before - visible_victim.hp, out.hp_lost);
            assert_eq!(visible_victim.hp, sim.actor(visible_victim.id).hp);
            total_loss += out.hp_lost;
        }

        assert_eq!(sim.actor(attacker.id).target, Some(visible_victim.id));
        assert_eq!(initial - visible_victim.hp, total_loss);
    }
}
