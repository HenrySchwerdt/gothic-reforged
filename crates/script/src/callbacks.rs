//! B14: engine callbacks and engine queries of `Engine_Callback` and `Magic_Barrier`.
//!
//! The engine calls these at fixed events (an NPC enters the processing range, a barrier is
//! touched) and asks them for numbers (the character-controller radius, the animation state an
//! action leaves the character in). Most callbacks that only wire the engine to the simulation
//! (`OnTick` regeneration, fall damage, respawning) live natively in `gothic-sim`; the ones
//! here are the pieces of script logic that stand on their own.

use crate::enums::{Action, AiMode, AniState, Species};
use crate::host::{EntityId, Pool, ScriptHost};
use crate::magic::api::MagicEvent;
use crate::predicates::is_ambient_creature;
use crate::routine::state::State;
use crate::stats;

/// Name of the barrier's runtime effect.
pub const BARRIER_EFFECT: &str = "eff_magic_barriere";
/// Name of the effect a touched barrier plays on the victim.
pub const BARRIER_TOUCH_EFFECT: &str = "eff_touch_magic_barriere";

/// Port of `GetAniStateByAction` (S:aa030): the pose an action leaves the character in.
/// `current` is the pose before; a death keeps the lying family of a knocked-down character
/// (sitting and lying become the straight dead pose). Unknown actions keep the pose.
pub fn ani_state_by_action(action: Action, current: AniState) -> AniState {
    gothic_portcov::hit!("ani_state_by_action");
    use AniState as P;
    match action {
        Action::Parade => P::Parade,
        Action::SitKnockDown => P::SitKnockDown,
        Action::LieKnockDown => P::LieKnockDown,
        Action::LieKnockOut => P::LieKnockOut,
        Action::Die => match current {
            P::SitKnockDown | P::LieKnockDown => P::LieStraightDead,
            other => other,
        },
        Action::LieDead => P::LieDead,
        Action::LiePiercedKO => P::LiePiercedKO,
        Action::LiePiercedDead => P::LiePiercedDead,
        Action::Stand => P::Stand,
        Action::Sneak => P::Sneak,
        Action::Kneel => P::Kneel,
        Action::SitGround => P::SitGround,
        Action::SitStool => P::SitStool,
        Action::SitBench => P::SitBench,
        Action::SitThrone => P::SitThrone,
        Action::SleepBed => P::SleepBed,
        Action::SleepGround => P::SleepGround,
        Action::TiltOrcBoulder => P::TiltOrcBoulder,
        Action::HoldOrcBoulder => P::HoldOrcBoulder,
        Action::LiftOrcBoulder => P::LiftOrcBoulder,
        _ => current,
    }
}

/// Port of `GetDCCRadius` (S:aa3a0): the radius of the character-controller capsule in cm:
/// trolls 120, ambient creatures 20, everybody else 60.
pub fn dcc_radius(host: &dyn ScriptHost, e: EntityId) -> f32 {
    gothic_portcov::hit!("dcc_radius");
    if host.species(e) == Species::Troll {
        120.0
    } else if is_ambient_creature(host, e) {
        20.0
    } else {
        60.0
    }
}

/// Port of `IsFirstPersonCameraAllowed` (S:aaef0): not while down or dead, and not in the
/// knock-down tasks.
pub fn first_person_camera_allowed(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("first_person_camera_allowed");
    if matches!(host.ai_mode(e), AiMode::Down | AiMode::Dead) {
        return false;
    }
    !matches!(
        host.current_task(e),
        Some(State::LieKnockDown | State::SitKnockDown)
    )
}

/// Port of `RefreshHitPoints` (S:ab840): stamina and hit points back to their maxima (the
/// maxima themselves are re-clamped by the setters).
pub fn refresh_hit_points(host: &mut dyn ScriptHost, e: EntityId) {
    gothic_portcov::hit!("refresh_hit_points");
    let sta = stats::pool_max(host, e, Pool::Stamina);
    stats::set_pool_max(host, e, Pool::Stamina, sta);
    stats::set_pool(host, e, Pool::Stamina, sta);
    let hp = stats::pool_max(host, e, Pool::Hit);
    stats::set_pool_max(host, e, Pool::Hit, hp);
    stats::set_pool(host, e, Pool::Hit, hp);
}

/// Port of `OnEnterMagicBarrier` (S:5ebb0): a living character that enters the barrier gets its
/// effect.
pub fn on_enter_magic_barrier(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("on_enter_magic_barrier");
    if host.exists(who) && host.ai_mode(who) != AiMode::Dead {
        host.emit_magic(MagicEvent::Effect {
            name: BARRIER_EFFECT.into(),
            on: who,
        });
    }
}

/// Port of `OnLeaveMagicBarrier` (S:5ec90): the barrier effect ends (reported as the effect
/// name with a leading minus).
pub fn on_leave_magic_barrier(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("on_leave_magic_barrier");
    if host.exists(who) {
        host.emit_magic(MagicEvent::Effect {
            name: format!("-{BARRIER_EFFECT}"),
            on: who,
        });
    }
}

/// Port of `OnToucheBarrier` (S:5ed60): a living character that runs into the barrier stops,
/// forgets who is hitting it (the current attacker becomes the last one) and stumbles back.
pub fn on_touch_barrier(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("on_touch_barrier");
    if !host.exists(who) || host.ai_mode(who) == AiMode::Dead {
        return;
    }
    host.full_stop(who);
    let attacker = host.current_attacker(who);
    host.set_last_attacker(who, attacker);
    host.set_current_attacker(who, EntityId::NONE);
    host.set_task(who, State::Stumble);
    host.emit_magic(MagicEvent::Effect {
        name: BARRIER_TOUCH_EFFECT.into(),
        on: who,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityApi, NpcApi};
    use crate::testhost::TestWorld;

    #[test]
    fn actions_map_to_poses() {
        assert_eq!(
            ani_state_by_action(Action::SitStool, AniState::Stand),
            AniState::SitStool
        );
        assert_eq!(
            ani_state_by_action(Action::TiltOrcBoulder, AniState::Stand),
            AniState::TiltOrcBoulder
        );
        assert_eq!(
            ani_state_by_action(Action::Die, AniState::LieKnockDown),
            AniState::LieStraightDead
        );
        assert_eq!(
            ani_state_by_action(Action::Attack, AniState::Sneak),
            AniState::Sneak,
            "a swing does not change the pose"
        );
    }

    #[test]
    fn capsule_radius_and_camera_rules() {
        let mut w = TestWorld::new();
        let troll = w.spawn_npc("Troll", Species::Troll, 20, 20);
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        let rat = w.spawn_npc("Rat", Species::Rabbit, 1, 1);
        assert_eq!(dcc_radius(&w, troll), 120.0);
        assert_eq!(dcc_radius(&w, man), 60.0);
        assert_eq!(dcc_radius(&w, rat), 20.0);
        assert!(first_person_camera_allowed(&w, man));
        w.set_ai_mode(man, AiMode::Down);
        assert!(!first_person_camera_allowed(&w, man));
        let p = w.player();
        w.set_task_directly(p, State::SitKnockDown);
        assert!(!first_person_camera_allowed(&w, p));
    }

    #[test]
    fn refreshing_restores_hit_points_and_stamina() {
        let mut w = TestWorld::new();
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        stats::set_pool(&mut w, man, Pool::Hit, 3);
        stats::set_pool(&mut w, man, Pool::Stamina, 1);
        refresh_hit_points(&mut w, man);
        assert_eq!(
            stats::pool(&w, man, Pool::Hit),
            stats::pool_max(&w, man, Pool::Hit)
        );
        assert_eq!(
            stats::pool(&w, man, Pool::Stamina),
            stats::pool_max(&w, man, Pool::Stamina)
        );
    }

    #[test]
    fn the_barrier_stops_and_stumbles_the_living() {
        let mut w = TestWorld::new();
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        let foe = w.spawn_npc("Foe", Species::Orc, 5, 5);
        w.set_current_attacker(man, foe);
        on_enter_magic_barrier(&mut w, man);
        on_touch_barrier(&mut w, man);
        assert_eq!(w.current_task(man), Some(State::Stumble));
        assert_eq!(w.last_attacker(man), foe);
        assert_eq!(w.current_attacker(man), EntityId::NONE);
        w.set_ai_mode(man, AiMode::Dead);
        w.mg.events.clear();
        on_enter_magic_barrier(&mut w, man);
        assert!(w.mg.events.is_empty(), "the dead feel nothing");
        on_leave_magic_barrier(&mut w, foe);
        assert_eq!(w.mg.events.len(), 1);
    }
}
