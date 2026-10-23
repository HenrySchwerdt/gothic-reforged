//! Live tuning of the Blade & Sorcery style combat feel through `debug.gr.*` system properties (see
//! `docs/research/VR_COMBAT_BNS.md`): `setprop debug.gr.<name> <value>`, picked up within half a second (no rebuild, no
//! relaunch). Defaults are the shipped values; `[R]` marks the research note's recommendations, which are unsourced
//! starting points.
//!
//! | prop | default | meaning |
//! |---|---|---|
//! | `pd_lin_hz`, `pd_lin_damp` | 12, 1.0 | hand position PD: natural frequency (Hz) and damping ratio (spring/damper feel) |
//! | `pd_ang_hz`, `pd_ang_damp` | 10, 1.0 | hand rotation PD |
//! | `strength` | 60 | arm strength (Gothic STR): force and torque caps (`arm_limits`); low = heavy/laggy hands |
//! | `weapon_mass` | 1.0 | multiplier on every holstered weapon's mass (weight lag) |
//! | `hit_min_speed`, `hit_full_speed` | 0 = profile | blade impact speed (m/s) below which a hit does nothing / at which it counts fully |
//! | `hit_impulse` | 0.9 | ragdoll impulse per (m/s x kg) of a hit |
//! | `stagger_impulse`, `fall_impulse` | 12, 60 | impulse (N s) that makes an orc stagger / fall |
//! | `stagger_strength`, `stagger_time` | 0.3, 0.7 | muscle strength (0..1) and duration (s) of a stagger |
//! | `haptic` | 1.0 | multiplier on every controller vibration (0 = off) |
//! | `vignette` | 0.6 | max comfort vignette opacity (0 = off) |
//! | `snap_deg`, `smooth_turn` | 30, 0 | snap-turn step in degrees; `smooth_turn` > 0 switches to smooth turning (deg/s) |
//! | `hand_stagger_max` | 2.0 | caps the seconds a blocked hand stays weakened (0 = never weakened; the weakness depth `STAGGER_STRENGTH` and `WOUND_DRAG` are crate consts, see docs) |
//! | `orc_substeps`, `orc_iter` | 2, 4 | ragdoll solver cost |
use bevy::prelude::*;
use gothic_vr_melee::MeleeWeapon;
use gothic_vr_npc::VrNpc;
use gothic_vr_physics::avian3d::prelude::Mass;
use gothic_vr_physics::pd::PdGains;
use gothic_vr_physics::{PhysicsHand, Prop, VrStrength};
use gothic_xr_quest::knob_f32;

static STAG_MAX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x40000000);

pub struct TunePlugin;

impl Plugin for TunePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_tuning).add_systems(Update, cap_stagger);
    }
}

#[allow(clippy::type_complexity)]
fn apply_tuning(
    time: Res<Time<Real>>,
    mut last: Local<f32>,
    mut strength: ResMut<VrStrength>,
    mut hands: Query<&mut PhysicsHand>,
    mut weapons: Query<(Entity, &Prop, Option<&mut MeleeWeapon>), Without<VrNpc>>,
    mut npcs: Query<&mut VrNpc>,
    mut commands: Commands,
    mut player: ResMut<gothic_vr::VrPlayer>,
    mut mass_seen: Local<std::collections::HashMap<Entity, f32>>,
) {
    if time.elapsed_secs() - *last < 0.5 {
        return;
    }
    *last = time.elapsed_secs();
    strength.0 = knob_f32("strength", 60.0);
    let lin = PdGains {
        frequency_hz: knob_f32("pd_lin_hz", 12.0),
        damping_ratio: knob_f32("pd_lin_damp", 1.0),
    };
    let ang = PdGains {
        frequency_hz: knob_f32("pd_ang_hz", 10.0),
        damping_ratio: knob_f32("pd_ang_damp", 1.0),
    };
    let stag_max = knob_f32("hand_stagger_max", 2.0);
    for mut h in &mut hands {
        h.linear = lin;
        h.angular = ang;
    }
    {
        let c = &mut player.loco.settings;
        c.vignette_strength = knob_f32("vignette", 0.6).clamp(0.0, 1.0);
        let smooth = knob_f32("smooth_turn", 0.0);
        c.turn = if smooth > 0.0 {
            gothic_vr::locomotion::TurnMode::Smooth { degrees_per_second: smooth }
        } else {
            gothic_vr::locomotion::TurnMode::Snap { degrees: knob_f32("snap_deg", 30.0).max(5.0) }
        };
    }
    STAG_MAX.store(stag_max.to_bits(), std::sync::atomic::Ordering::Relaxed);
    let mass_mul = knob_f32("weapon_mass", 1.0);
    let (min_s, full_s) = (
        knob_f32("hit_min_speed", 0.0),
        knob_f32("hit_full_speed", 0.0),
    );
    for (e, prop, melee) in &mut weapons {
        if mass_seen.get(&e).copied() != Some(mass_mul) {
            commands.entity(e).insert(Mass(prop.0.mass * mass_mul));
            mass_seen.insert(e, mass_mul);
        }
        if let Some(mut m) = melee {
            for s in &mut m.profile.strikers {
                if min_s > 0.0 {
                    s.min_speed = min_s;
                }
                if full_s > 0.0 {
                    s.full_speed = full_s;
                }
            }
        }
    }
    for mut n in &mut npcs {
        let s = &mut n.ragdoll.settings;
        s.stagger_impulse = knob_f32("stagger_impulse", 12.0);
        s.fall_impulse = knob_f32("fall_impulse", 60.0);
        s.stagger_strength = knob_f32("stagger_strength", 0.3);
        s.stagger_time = knob_f32("stagger_time", 0.7);
        s.substeps = knob_f32("orc_substeps", 2.0).max(1.0) as u32;
        s.iterations = knob_f32("orc_iter", 4.0).max(1.0) as u32;
    }
}

/// Applies `hand_stagger_max` every frame (the stagger timer is set by melee clashes at any time).
fn cap_stagger(mut hands: Query<&mut PhysicsHand>) {
    let max = f32::from_bits(STAG_MAX.load(std::sync::atomic::Ordering::Relaxed));
    if max >= 2.0 {
        return;
    }
    for mut h in &mut hands {
        if h.staggered > max {
            h.staggered = max;
        }
    }
}
