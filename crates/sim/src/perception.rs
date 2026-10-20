//! Perception: sight ranges, field of view, hearing [CA 10.1]. The original has no radius in
//! the `SendPerception*` broadcasts; each `Assess*` handler applies its own distance, FOV and
//! LOS test, which is what [`can_perceive`] and the hearing helpers model.
use crate::actor::Actor;
use crate::config::SimConfig;
use crate::types::*;
use crate::world::{WorldQuery, in_fov};

/// Maximum range at which `AssessActivePerception` (S:295f0) notices `target` [CA 10.1]:
/// the player 2000; a target already in Combat/Arena 4000; dead or down 1000; otherwise by the
/// observer's attitude to it: Friendly/Neutral 700, Angry 1000, Hostile/Predator/Panic 2000.
pub fn perception_range(target: &Actor, attitude: Attitude) -> f32 {
    if target.is_player {
        return 2000.0;
    }
    if target.is_down_or_dead() {
        return 1000.0;
    }
    if matches!(target.ai_mode, AiMode::Combat | AiMode::Arena) {
        return 4000.0;
    }
    match attitude {
        Attitude::Friendly | Attitude::Neutral | Attitude::None => 700.0,
        Attitude::Angry => 1000.0,
        Attitude::Hostile | Attitude::Predator | Attitude::Panic => 2000.0,
    }
}

/// Fight-sound hearing radius: 2000 around attacker or victim, 4000 while the listener's
/// enclave is in Attack status (`AssessFightSound`, S:2a510) [CA 10.2].
pub fn fight_hearing_range(enclave_attacking: bool) -> f32 {
    if enclave_attacking { 4000.0 } else { 2000.0 }
}

/// `CanReceivePerceptionFrom` (S:31c80) [CA 10.1].
pub fn can_receive_perception(receiver: &Actor, sender: &Actor) -> bool {
    !(receiver.is_player
        || receiver.id == sender.id
        || matches!(
            receiver.ai_mode,
            AiMode::Dead | AiMode::Down | AiMode::Talk | AiMode::Abandoned
        )
        || receiver.status & (status::INVULNERABLE | status::MISTED) != 0)
}

/// Can `observer` perceive `target` through sight? Applies the range by attitude, the 1000
/// vertical cut-off, the sneaking-player rule, FOV and LOS (skipped outdoors) [CA 10.1].
pub fn can_perceive(
    cfg: &SimConfig,
    world: &dyn WorldQuery,
    observer: &Actor,
    target: &Actor,
    attitude: Attitude,
) -> bool {
    let (Some(op), Some(tp)) = (world.position(observer.id), world.position(target.id)) else {
        return false;
    };
    let dist = op.dist(tp);
    if dist > perception_range(target, attitude) {
        return false;
    }
    if (tp.y - op.y).abs() > 1000.0 {
        return false;
    }
    if matches!(target.ani_state, AniState::Swim | AniState::Dive) {
        return false;
    }
    // The poll skips a sneaking player closer than 1000; `AssessPlayerSneaking` handles it.
    if target.is_player && target.ani_state == AniState::Sneak && dist < 1000.0 {
        return false;
    }
    if !in_fov(world, observer.id, tp, cfg.fov_degrees) {
        return false;
    }
    world.is_outdoors(op) || world.line_of_sight(op, tp)
}

/// Sight test with a fixed range (witness checks such as theft within 1000, weapon within 700).
pub fn sees_within(
    cfg: &SimConfig,
    world: &dyn WorldQuery,
    observer: &Actor,
    target: &Actor,
    range: f32,
) -> bool {
    let (Some(op), Some(tp)) = (world.position(observer.id), world.position(target.id)) else {
        return false;
    };
    op.dist(tp) <= range
        && in_fov(world, observer.id, tp, cfg.fov_degrees)
        && (world.is_outdoors(op) || world.line_of_sight(op, tp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Grid2dWorld, Pos, Rect};

    fn setup() -> (SimConfig, Grid2dWorld, Actor, Actor) {
        let cfg = SimConfig::default();
        let mut w = Grid2dWorld::new();
        let hero = Actor::hero(ActorId(0), &cfg);
        let mut guard = Actor::npc(ActorId(1), "Guard", Species::Human, 10, 10);
        guard.refresh(&cfg);
        w.place(hero.id, Pos::flat(1500.0, 0.0));
        w.place(guard.id, Pos::flat(0.0, 0.0));
        w.set_facing(guard.id, 0.0);
        (cfg, w, hero, guard)
    }

    #[test]
    fn range_depends_on_target_and_attitude() {
        let (cfg, _, hero, mut npc) = setup();
        assert_eq!(perception_range(&hero, Attitude::Neutral), 2000.0);
        assert_eq!(perception_range(&npc, Attitude::Neutral), 700.0);
        assert_eq!(perception_range(&npc, Attitude::Angry), 1000.0);
        assert_eq!(perception_range(&npc, Attitude::Hostile), 2000.0);
        npc.ai_mode = AiMode::Combat;
        assert_eq!(perception_range(&npc, Attitude::Neutral), 4000.0);
        npc.ai_mode = AiMode::Down;
        assert_eq!(perception_range(&npc, Attitude::Hostile), 1000.0);
        assert_eq!(fight_hearing_range(true), 4000.0);
        let _ = cfg;
    }

    #[test]
    fn sight_needs_fov_range_and_los() {
        let (cfg, mut w, hero, guard) = setup();
        assert!(can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
        // Behind the guard: outside the field of view.
        w.set_facing(guard.id, std::f32::consts::PI);
        assert!(!can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
        w.set_facing(guard.id, 0.0);
        // Wall in between.
        w.add_wall(Rect::new(700.0, -200.0, 800.0, 200.0));
        assert!(!can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
        // Outdoors skips LOS.
        w.outdoors = true;
        assert!(can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
        // Out of range.
        w.teleport(hero.id, Pos::flat(2500.0, 0.0));
        assert!(!can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
    }

    #[test]
    fn sneaking_player_hidden_from_poll_within_1000() {
        let (cfg, mut w, mut hero, guard) = setup();
        w.teleport(hero.id, Pos::flat(800.0, 0.0));
        hero.ani_state = AniState::Sneak;
        assert!(!can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
        assert!(sees_within(&cfg, &w, &guard, &hero, 1000.0));
        w.teleport(hero.id, Pos::flat(1200.0, 0.0));
        assert!(can_perceive(&cfg, &w, &guard, &hero, Attitude::Neutral));
    }

    #[test]
    fn receive_filter() {
        let (cfg, _, hero, mut guard) = setup();
        let other = Actor::npc(ActorId(2), "O", Species::Human, 1, 1);
        assert!(can_receive_perception(&guard, &other));
        assert!(!can_receive_perception(&hero, &other));
        assert!(!can_receive_perception(&guard, &guard.clone()));
        guard.ai_mode = AiMode::Dead;
        assert!(!can_receive_perception(&guard, &other));
        let _ = cfg;
    }
}
