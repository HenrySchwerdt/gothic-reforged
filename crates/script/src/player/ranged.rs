//! `PS_Ranged*` (B16, ranged half): bow and crossbow for the hero, and `Shoot`, the function
//! that turns a released shot into a projectile (NPC archers use it too).
//!
//! A bow draws: the longer the hero holds the string the harder the arrow hits
//! ([`draw_multiplier`], `OnPowerAim_Loop`). A crossbow always shoots at full strength but has to
//! be cocked again after every bolt (`PS_Ranged_Reload`). The damage of the arrow is the
//! ammunition's plus the bow's, scaled by the draw multiplier in the damage pipeline (B07).

use crate::enums::{Action, AiMode, AniState, DamageType, Slot, UseType};
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::magic::api::{PathStyle, ProjectileSpec};
use crate::player::step;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};

/// Seconds of draw after which a bow reaches full strength.
pub const FULL_DRAW_SECS: f32 = 1.95;
/// Draw multiplier of an unstrung (just nocked) bow.
pub const MIN_DRAW: f32 = 0.2;
/// Arrow speed per unit of draw multiplier (cm/s) for the hero's shots.
pub const ARROW_SPEED: f32 = 200.0;

/// Port of the multiplier rule of `OnPowerAim_Loop` (S:84b90): `0.2 + 0.8 * min(1, t / 1.95)`.
pub fn draw_multiplier(held_secs: f32) -> f32 {
    gothic_portcov::hit!("draw_multiplier");
    MIN_DRAW + (1.0 - MIN_DRAW) * (held_secs / FULL_DRAW_SECS).clamp(0.0, 1.0)
}

/// Does the hero hold a crossbow (left hand)?
fn crossbow(host: &dyn ScriptHost, p: EntityId) -> bool {
    host.slot_use_type(p, Slot::LeftHand) == UseType::CrossBow
}

/// Port of `Shoot` (S:86450): spawn the ammunition held in the right hand as a projectile owned
/// by the shooter, with the hit multiplier the draw produced. The hero's arrow is ballistic
/// (path Physics, speed `multiplier * 200`) and aimed at the camera centre — `target` is the
/// focus entity, which only helps the aim; an NPC's flies in a line along its forward axis. The
/// hero uses up one piece of ammunition (NPC archers are re-supplied for free). Returns whether
/// ammunition is left.
pub fn shoot(host: &mut dyn ScriptHost, shooter: EntityId, target: EntityId) -> bool {
    gothic_portcov::hit!("shoot");
    let stack = host.find_stack_index(shooter, Slot::RightHand);
    if stack < 0 {
        return false;
    }
    let (damage_type, ammo) = host
        .slot_item_damage(shooter, Slot::RightHand)
        .unwrap_or((DamageType::Missile, 0));
    let bow = host
        .slot_item_damage(shooter, Slot::LeftHand)
        .map_or(0, |d| d.1);
    let player = shooter == host.player();
    let mult = if player || host.hit_multiplier(shooter) > 0.0 {
        host.hit_multiplier(shooter)
    } else {
        1.0
    };
    let template = host.slot_item_name(shooter, Slot::RightHand);
    let quality = host.slot_item_quality(shooter, Slot::RightHand);
    host.spawn_projectile(ProjectileSpec {
        owner: shooter,
        target,
        spell: String::new(),
        template,
        mana_used: 0,
        hit_mult: mult,
        damage_type,
        amount: ammo + bow,
        path: if player {
            PathStyle::Physics
        } else {
            PathStyle::Line
        },
        velocity: if player { mult * ARROW_SPEED } else { 0.0 },
        deity: crate::enums::SpellDeity::None,
        quality,
    });
    let mut left = host.stack_amount(shooter, stack);
    if player {
        host.delete_stack_items(shooter, stack, 1);
        left -= 1;
    }
    left > 0
}

fn aim_target(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    let t = host.current_target(p);
    if t.is_some() { t } else { host.focus_entity() }
}

/// Port of `PS_Ranged` (S:837e0): with the bow or crossbow up (the weapon mode must still be
/// ranged, otherwise `PS_Normal`) the hero readies it: combat mode, hands filled, and the hit
/// multiplier of a crossbow at 1, of a bow at the undrawn 0.2.
fn ps_ranged(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Ranged {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            host.set_ai_mode(p, AiMode::Combat);
            host.set_current_action(p, Action::None);
            let m = if crossbow(host, p) { 1.0 } else { MIN_DRAW };
            host.set_hit_multiplier(p, m);
            let right = host.find_stack_index(p, Slot::RightHand);
            let left = host.find_stack_index(p, Slot::LeftHand);
            host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsRangedWalk);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Walk` / `PS_Ranged_Walk_Loop` (S:84080): move about with the weapon ready;
/// the loop waits for the aim key and ends the mode when the weapon is put away.
fn ps_ranged_walk(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_walk");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Ranged {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_Ranged_Aim` (S:844a0): take aim (action `Aim`, standing); a crossbow waits in
/// `PS_Ranged_Aim_Loop`, a bow starts drawing in `PS_Ranged_PowerAim`.
fn ps_ranged_aim(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_aim");
    match step(host, p) {
        0 => {
            host.set_current_action(p, Action::Aim);
            host.set_ani_state(p, AniState::Stand);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Aim,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            let next = if crossbow(host, p) {
                State::PsRangedAimLoop
            } else {
                State::PsRangedPowerAim
            };
            host.set_state_time(p, 0.0);
            host.set_state(p, next);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Aim_Loop` (S:84d60): hold the aim and wait for shoot or reload.
fn ps_ranged_aim_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_aim_loop");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Ranged {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_Ranged_PowerAim` (S:84830) with its loop callback `OnPowerAim_Loop` (S:84b90): the
/// bow is drawn; every tick the hit multiplier follows the time held.
fn ps_ranged_power_aim(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_power_aim");
    let held = host.state_time(p);
    host.set_hit_multiplier(p, draw_multiplier(held));
    host.queue_ai(p, AiCommand::WaitMs { ms: 50 });
    Flow::Yield
}

/// Port of `PS_Ranged_Shoot` (S:850e0): play the shot (`Shoot`), loose the projectile with
/// [`shoot`], then reload: a crossbow cocks the next bolt, a bow goes back to walking.
fn ps_ranged_shoot(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_shoot");
    match step(host, p) {
        0 => {
            host.set_current_action(p, Action::Shoot);
            host.set_ani_state(p, AniState::Stand);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Shoot,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            let target = aim_target(host, p);
            let more = shoot(host, p, target);
            host.set_current_target(p, EntityId::NONE);
            // `Aim_Recover`: the weapon goes down after the shot before the next move.
            host.queue_ai(
                p,
                AiCommand::WaitMs {
                    ms: if crossbow(host, p) { 600 } else { 400 },
                },
            );
            let next = if !more {
                State::PsRangedAimReload
            } else if crossbow(host, p) {
                State::PsRangedReload
            } else {
                host.set_hit_multiplier(p, MIN_DRAW);
                State::PsRangedWalk
            };
            host.set_state(p, next);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Reload` (S:859a0) and `PS_Ranged_Aim_Reload` (S:85550): cock the crossbow
/// (`Cock` overlay) or pull a fresh arrow, hold the ammunition again and aim (crossbow) or walk
/// (bow). Without ammunition the hero just lowers the weapon.
fn ps_ranged_reload(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_reload");
    let stack = host.find_stack_index(p, Slot::RightHand);
    if stack < 0 {
        host.set_state(p, State::PsRangedWalk);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            host.set_ani_state(p, AniState::Stand);
            host.set_current_action(p, Action::Reload);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: if host.current_state(p) == Some(State::PsRangedReload) {
                        Action::Cock
                    } else {
                        Action::Reload
                    },
                    secs: 1,
                },
            );
            Flow::Yield
        }
        1 => {
            let right = host.find_stack_index(p, Slot::RightHand);
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right,
                    left: host.find_stack_index(p, Slot::LeftHand),
                },
            );
            Flow::Yield
        }
        _ => {
            let next = if crossbow(host, p) {
                host.set_hit_multiplier(p, 1.0);
                State::PsRangedAim
            } else {
                host.set_hit_multiplier(p, MIN_DRAW);
                State::PsRangedWalk
            };
            host.set_state(p, next);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_CrossBowSneak_Loop` (S:86320): sneaking with a crossbow is the walk loop.
fn ps_ranged_crossbow_sneak(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_crossbow_sneak");
    ps_ranged_walk(host, p)
}

/// Definitions of the ranged states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsRanged => custom(ps_ranged),
        S::PsRangedWalk | S::PsRangedWalkLoop => custom(ps_ranged_walk),
        S::PsRangedAim => custom(ps_ranged_aim),
        S::PsRangedAimLoop => custom(ps_ranged_aim_loop),
        S::PsRangedPowerAim => custom(ps_ranged_power_aim),
        S::PsRangedShoot => custom(ps_ranged_shoot),
        S::PsRangedReload | S::PsRangedAimReload => custom(ps_ranged_reload),
        S::PsRangedCrossBowSneakLoop => custom(ps_ranged_crossbow_sneak),
        _ => None,
    }
}
