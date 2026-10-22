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

/// Is there ammunition in the hand slot? (`FUN_10002af0`, the next ammunition stack: a negative
/// index ends the ranged mode with `LastOneGone`.)
fn ammo_left(host: &dyn ScriptHost, p: EntityId) -> bool {
    let stack = host.find_stack_index(p, Slot::RightHand);
    stack >= 0 && host.stack_amount(p, stack) > 0
}

fn aim_target(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    let t = host.current_target(p);
    if t.is_some() { t } else { host.focus_entity() }
}

/// `gEAction` ids of the ranged loops (the jump table of `PS_Ranged_Walk_Loop` S:84080,
/// `PS_Ranged_Aim_Loop` S:84d60 and `OnPowerAim_Loop` S:84b90): the abort (39), the aim (40), the
/// shot (41), the reload (42), the weapon key (68), the jump (57), the sneak (77), quick slots
/// (131..140) and potions (141..143).
const ID_ABORT: u32 = 39;
const ID_AIM: u32 = 40;
const ID_SHOOT: u32 = 41;
const ID_RELOAD: u32 = 42;
const ID_JUMP: u32 = 57;
const ID_HOLD: u32 = 68;
const ID_SNEAK: u32 = 77;
const ID_QUICK_SLOT_BASE: u32 = 131;
const ID_POTION_BASE: u32 = 141;

/// The ids every ranged loop treats alike: the weapon key puts the weapon away
/// (`OnPlayerSelectWeapon`, S:63cc0), quick slots and potion keys go to the game layer. Returns
/// whether `id` was one of them.
fn common_action(host: &mut dyn ScriptHost, p: EntityId, id: u32) -> bool {
    match id {
        ID_HOLD => crate::player::select_weapon::holster_from_melee(host, p),
        n if (ID_QUICK_SLOT_BASE..ID_QUICK_SLOT_BASE + 10).contains(&n) => {
            host.emit(crate::player::api::PlayerEvent::QuickSlot(
                n - ID_QUICK_SLOT_BASE,
            ));
        }
        n if (ID_POTION_BASE..ID_POTION_BASE + 3).contains(&n) => {
            host.emit(crate::player::api::PlayerEvent::PotionKey(
                n - ID_POTION_BASE,
            ));
        }
        _ => return false,
    }
    true
}

/// `PSCharacterControl::SetMovementConstraints` of the helper `sub_20b0` (S:20b0) that `PS_Ranged`
/// and the magic loop call: a humanoid hero gets 0, which lets `ProcessMovements` align the body
/// with the camera direction every frame (`SetAlignmentLookAt`, G:ddd40).
pub const FOLLOW_CAMERA_CONSTRAINT: f32 = 0.0;

/// `PSFocus.CurrentMode` of `PS_Ranged` (`gECombatMode` ranged = 3, `GetFocusEntity(.., 3)`).
pub const FOCUS_MODE_RANGED: u32 = 3;

/// Port of `PS_Ranged` (S:837e0): the bow or crossbow is readied. The weapon mode must still be
/// ranged, otherwise `PS_Normal`. Step 0 clears the focus unless the hero already fought, runs
/// `_AI_HoldInventoryItems` and gives the body to the camera; the last step records the weapon
/// configuration and sets the hit multiplier, then a crossbow aims at once (`PS_Ranged_Aim_Loop`,
/// crosshair on) and a bow walks (`PS_Ranged_Walk`).
fn ps_ranged(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Ranged {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            if host.ai_mode(p) != AiMode::Combat {
                host.set_focus_entity(EntityId::NONE);
            }
            host.set_ai_mode(p, AiMode::Talk);
            host.set_last_weapon_config(1);
            host.set_focus_current_mode(FOCUS_MODE_RANGED);
            host.set_focus_range_rating(true);
            host.set_look_at(p, EntityId::NONE);
            host.set_alignment_target(p, EntityId::NONE);
            host.set_movement_constraints(p, FOLLOW_CAMERA_CONSTRAINT);
            host.set_current_action(p, Action::None);
            let right = host.find_stack_index(p, Slot::RightHand);
            let left = host.find_stack_index(p, Slot::LeftHand);
            host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
            Flow::Yield
        }
        _ => {
            host.set_ai_mode(p, AiMode::Combat);
            if crossbow(host, p) {
                host.set_hit_multiplier(p, 1.0);
                host.set_state(p, State::PsRangedAimLoop);
            } else {
                host.set_hit_multiplier(p, MIN_DRAW);
                host.set_state(p, State::PsRangedWalk);
            }
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Walk` (S:83f80): crosshair off, auto-aim off (the host derives both from the
/// state), then the loop.
fn ps_ranged_walk(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_walk");
    host.set_state(p, State::PsRangedWalkLoop);
    Flow::Done
}

/// The weapon was taken out of the hands from outside the loops (the inventory, a quick slot):
/// the task ends [I].
fn weapon_gone(host: &mut dyn ScriptHost, p: EntityId) -> bool {
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Ranged {
        host.set_task(p, State::PsNormal);
        return true;
    }
    false
}

/// Finish one pass of a loop: either the state it chose, or wait for the next tick.
fn leave_or_wait(host: &mut dyn ScriptHost, p: EntityId, next: Option<State>) -> Flow {
    match next {
        Some(state) => {
            host.set_state(p, state);
            Flow::Done
        }
        None => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 20 });
            Flow::Yield
        }
    }
}

/// Port of `PS_Ranged_Walk_Loop` (S:84080): the hero walks with the weapon ready. Every pass
/// clears the action and reads the head of the action queue: the abort and the reload key lower
/// the draw (`PS_Ranged_Reload`), the aim key aims, the shot key shoots a crossbow and aims a bow,
/// then jump and sneak, the weapon key, quick slots and potions.
fn ps_ranged_walk_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_walk_loop");
    host.set_current_action(p, Action::None);
    let Some((id, _)) = host.action_queue_head() else {
        if weapon_gone(host, p) {
            return Flow::Done;
        }
        return leave_or_wait(host, p, None);
    };
    host.action_queue_pop();
    let next = match id {
        ID_ABORT | ID_RELOAD => Some(State::PsRangedReload),
        ID_AIM => Some(State::PsRangedAim),
        ID_SHOOT if crossbow(host, p) => Some(State::PsRangedShoot),
        ID_SHOOT => Some(State::PsRangedAim),
        ID_JUMP => Some(State::PsRangedJump),
        ID_SNEAK => Some(State::PsRangedSneak),
        other => {
            common_action(host, p, other);
            None
        }
    };
    leave_or_wait(host, p, next)
}

/// Port of `PS_Ranged_Aim` (S:844a0): action `Aim`, standing. Without an item in the slot the hero
/// says `MissingItem` and goes back to walking; otherwise the aim clip plays (crosshair, auto-aim,
/// the host's overlay) and the crossbow waits in `PS_Ranged_Aim_Loop`, the bow draws in
/// `PS_Ranged_PowerAim`.
fn ps_ranged_aim(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_aim");
    match step(host, p) {
        0 => {
            host.set_current_action(p, Action::Aim);
            host.set_ani_state(p, AniState::Stand);
            if host.find_stack_index(p, Slot::RightHand) < 0 {
                crate::speech::say(host, p, "MissingItem");
                host.set_state(p, State::PsRangedWalk);
                return Flow::Done;
            }
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

/// Port of `PS_Ranged_Aim_Loop` (S:84d60): the weapon is up. The abort puts the draw down
/// (`PS_Ranged_Reload`), the shot key shoots; a crossbow can sneak; the weapon key, quick slots
/// and potions as in the walk loop.
fn ps_ranged_aim_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_aim_loop");
    let Some((id, _)) = host.action_queue_head() else {
        if weapon_gone(host, p) {
            return Flow::Done;
        }
        return leave_or_wait(host, p, None);
    };
    host.action_queue_pop();
    let next = match id {
        ID_ABORT => Some(State::PsRangedReload),
        ID_SHOOT => Some(State::PsRangedShoot),
        ID_SNEAK if crossbow(host, p) => Some(State::PsRangedSneak),
        other => {
            common_action(host, p, other);
            None
        }
    };
    leave_or_wait(host, p, next)
}

/// Seconds the bow's `PowerRaise` clip runs before the draw is held in `PS_Ranged_Aim_Loop`
/// (`gothic_projectile::aim::clips::bow::POWER_RAISE`, the clip length in the animation data).
pub const POWER_RAISE_SECS: f32 = 2.0;

/// Port of `PS_Ranged_PowerAim` (S:84830) with its loop callback `OnPowerAim_Loop` (S:84b90): the
/// bow is drawn for the length of the `PowerRaise` clip. Every tick the hit multiplier follows the
/// time held and the head of the action queue is read: the abort (`FullStop`, `PS_Ranged_Reload`)
/// and the shot (`FullStop`, `PS_Ranged_Shoot`). After the clip the state is `PS_Ranged_Aim_Loop`.
fn ps_ranged_power_aim(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_power_aim");
    let held = host.state_time(p);
    host.set_hit_multiplier(p, draw_multiplier(held));
    if let Some((id, _)) = host.action_queue_head() {
        let next = match id {
            ID_ABORT => Some(State::PsRangedReload),
            ID_SHOOT => Some(State::PsRangedShoot),
            _ => None,
        };
        if let Some(state) = next {
            host.action_queue_pop();
            host.full_stop(p);
            host.set_state(p, state);
            return Flow::Done;
        }
    }
    if held >= POWER_RAISE_SECS {
        host.set_state(p, State::PsRangedAimLoop);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 50 });
    Flow::Yield
}

/// Port of `PS_Ranged_Shoot` (S:850e0): the shot clip, `Shoot` looses the projectile
/// ([`shoot`]), the recovery clip follows (`Aim_Recover`). Then: with no ammunition left the hero
/// says `LastOneGone`, forgets the ranged configuration and the task is `PS_Normal`; a crossbow
/// walks (it is reloaded with the key), a bow re-nocks in `PS_Ranged_Aim_Reload`.
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
        1 => {
            let target = aim_target(host, p);
            shoot(host, p, target);
            host.set_current_target(p, EntityId::NONE);
            // `Aim_Recover`: the weapon goes down after the shot before the next move.
            host.queue_ai(
                p,
                AiCommand::WaitMs {
                    ms: if crossbow(host, p) { 600 } else { 400 },
                },
            );
            Flow::Yield
        }
        _ => {
            if !ammo_left(host, p) {
                crate::speech::say(host, p, "LastOneGone");
                host.set_last_weapon_config(0);
                host.set_task(p, State::PsNormal);
                return Flow::Done;
            }
            if crossbow(host, p) {
                host.set_state(p, State::PsRangedWalk);
            } else {
                host.set_state(p, State::PsRangedAimReload);
            }
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Reload` (S:859a0): after an abort or the reload key. A crossbow cocks
/// (`Cock`, 2.8 s) and aims again; a bow has nothing to cock and goes back to walking with the
/// string relaxed. The ammunition is held again (`_AI_HoldInventoryItems`).
fn ps_ranged_reload(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_reload");
    if host.find_stack_index(p, Slot::RightHand) < 0 {
        host.set_state(p, State::PsRangedWalk);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            host.set_ani_state(p, AniState::Stand);
            host.set_current_action(p, Action::Reload);
            if crossbow(host, p) {
                host.queue_ai(
                    p,
                    AiCommand::Action {
                        action: Action::Cock,
                        secs: 1,
                    },
                );
            } else {
                host.queue_ai(p, AiCommand::WaitMs { ms: 0 });
            }
            Flow::Yield
        }
        1 => {
            let right = host.find_stack_index(p, Slot::RightHand);
            let left = host.find_stack_index(p, Slot::LeftHand);
            host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
            Flow::Yield
        }
        _ => {
            if crossbow(host, p) {
                host.set_hit_multiplier(p, 1.0);
                host.set_state(p, State::PsRangedAim);
            } else {
                host.set_hit_multiplier(p, MIN_DRAW);
                host.set_state(p, State::PsRangedWalk);
            }
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Aim_Reload` (S:85550): after a bow shot the next arrow is nocked (the
/// `Reload` clips, `Begin`, `Raise`, `End`: 0.28 + 0.4 + 0.28 s). Then the head of the action
/// queue decides: the aim key draws again (`PS_Ranged_PowerAim`), the shot key shoots, anything
/// else walks. Without ammunition the state hands over to `PS_Ranged_Reload`.
fn ps_ranged_aim_reload(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_aim_reload");
    match step(host, p) {
        0 => {
            if !ammo_left(host, p) {
                host.set_state(p, State::PsRangedReload);
                return Flow::Done;
            }
            host.set_ani_state(p, AniState::Stand);
            host.set_current_action(p, Action::Reload);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Reload,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_hit_multiplier(p, MIN_DRAW);
            let next = match host.action_queue_head().map(|h| h.0) {
                Some(ID_AIM) => {
                    host.action_queue_pop();
                    State::PsRangedPowerAim
                }
                Some(ID_SHOOT) => {
                    host.action_queue_pop();
                    State::PsRangedShoot
                }
                _ => State::PsRangedWalk,
            };
            host.set_state_time(p, 0.0);
            host.set_state(p, next);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Jump`: `_AI_Jump`, then the walk loop again (the task default re-readies the
/// weapon; nothing of it is lost).
fn ps_ranged_jump(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_jump");
    match step(host, p) {
        0 => {
            crate::combat::commands::start_jump(host, p, false);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Jump,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsRangedWalkLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Ranged_Sneak` (S:860c0): the sneak toggle. A crossbow hides the crosshair and sneaks
/// in `PS_Ranged_CrossBowSneak_Loop`, a bow goes back to `PS_Ranged_Walk_Loop`.
fn ps_ranged_sneak(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_sneak");
    let sneaking = host.ani_state(p) == AniState::Sneak;
    host.set_ani_state(
        p,
        if sneaking {
            AniState::Stand
        } else {
            AniState::Sneak
        },
    );
    let next = if crossbow(host, p) {
        State::PsRangedCrossBowSneakLoop
    } else {
        State::PsRangedWalkLoop
    };
    host.set_state(p, next);
    Flow::Done
}

/// Port of `PS_Ranged_CrossBowSneak_Loop` (S:86320): sneaking with a crossbow reads the same
/// queue as the walk loop.
fn ps_ranged_crossbow_sneak(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ranged_crossbow_sneak");
    ps_ranged_walk_loop(host, p)
}

/// Definitions of the ranged states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsRanged => custom(ps_ranged),
        S::PsRangedWalk => custom(ps_ranged_walk),
        S::PsRangedWalkLoop => custom(ps_ranged_walk_loop),
        S::PsRangedAim => custom(ps_ranged_aim),
        S::PsRangedAimLoop => custom(ps_ranged_aim_loop),
        S::PsRangedPowerAim => custom(ps_ranged_power_aim),
        S::PsRangedShoot => custom(ps_ranged_shoot),
        S::PsRangedReload => custom(ps_ranged_reload),
        S::PsRangedAimReload => custom(ps_ranged_aim_reload),
        S::PsRangedJump => custom(ps_ranged_jump),
        S::PsRangedSneak => custom(ps_ranged_sneak),
        S::PsRangedCrossBowSneakLoop => custom(ps_ranged_crossbow_sneak),
        _ => None,
    }
}
