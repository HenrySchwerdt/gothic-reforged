//! SOULS-03: the integration hooks of `gothic-combat-souls` (see `docs/SOULS_TUNING.md`) wired
//! into `--play --combat souls`. Nothing here runs without that flag; `souls::register` adds it.
//!
//! * **Tuning**: [`load_config`] starts from `Tuning::default()` and applies a JSON override file
//!   (`--souls-tuning <file>` or `GR_SOULS_TUNING`), see [`apply_json`] for the keys.
//! * **Shake scale** (accessibility): `--souls-shake <0..2>`, `GR_SOULS_SHAKE` or the
//!   `shake_strength` key; 0 turns the camera shake off. It sets `CameraShake::strength`.
//! * **Line of sight** for lock-on: [`los_clear`] over the physics `CollisionWorld` (cm).
//! * **Lock camera occlusion**: [`occlusion`] casts from the render camera to the locked target;
//!   `souls::drive` passes the result to `LockCam::drive_with`.
//! * **Feedback**: the game resolves NPC swings in the simulation, so the core never sees them
//!   as its own hits; [`external_events`] turns the outcomes the glue mirrors (parry, block,
//!   guard break, heavy hurt) into the same `Event::Feedback` the core sends. [`apply_shake`]
//!   adds the plugin's `CameraShake` offset to the render camera.
//! * **Telegraph / commit**: NPC swings come from the sim brain (`brain.strike`, held by the
//!   telegraph variant), not from `Foe::command_attack`; [`TelegraphTracker`] turns a new strike
//!   into `FoeTelegraph` and a fixed share of its lead into `FoeCommit`.
use super::PlayState;
use super::souls::SoulsGate;
use crate::walk::{Collision, Player, PlayerCamera};
use bevy::prelude::*;
use gothic_combat_souls::actions::{ActionId, Hurt};
use gothic_combat_souls::events::{Event, FeedbackCause};
use gothic_combat_souls::hero::Hero;
use gothic_combat_souls::plugin::{CameraShake, SoulsEvent, SoulsRes, to_bevy};
use gothic_combat_souls::tuning::{Impact, Tuning};
use gothic_physics::CollisionWorld;
use gothic_runtime::CoreRes;
use std::collections::BTreeMap;

const CM: f32 = 0.01;
pub const TUNING_FLAG: &str = "--souls-tuning";
pub const TUNING_ENV: &str = "GR_SOULS_TUNING";
pub const SHAKE_FLAG: &str = "--souls-shake";
pub const SHAKE_ENV: &str = "GR_SOULS_SHAKE";
/// Largest accepted shake scale.
pub const MAX_SHAKE: f32 = 2.0;
/// A line-of-sight ray stops this short of its end point (cm), so geometry right at the target
/// (the floor under its feet, a wall it leans on) does not count as a blocker.
pub const LOS_END_MARGIN: f32 = 20.0;
/// Height above the target's feet the camera occlusion ray aims at (cm, chest).
pub const TARGET_CHEST: f32 = 120.0;
/// Share of an NPC wind-up after which the HUD shows the commit cue. Equal to the tracking
/// cut-off of `FoeProfile::telegraphed()`; the sim brain has no tracking cut-off of its own,
/// so in the game this is a timing cue only.
pub const COMMIT_SHARE: f32 = 0.55;

// ---- tuning --------------------------------------------------------------------------------

/// What `--combat souls` is configured with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoulsConfig {
    pub tuning: Tuning,
    /// `CameraShake::strength` (1 = as tuned, 0 = off).
    pub shake_strength: f32,
}

impl Default for SoulsConfig {
    fn default() -> Self {
        Self {
            tuning: Tuning::default(),
            shake_strength: 1.0,
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .cloned()
}

/// Builds the configuration from the command line (`args`) and the environment (`env`). A flag
/// wins over its environment variable. Errors leave the defaults (or the values applied before
/// the error) in place and are returned as log lines.
pub fn load_config(
    args: &[String],
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&str) -> std::io::Result<String>,
) -> (SoulsConfig, Vec<String>) {
    let mut cfg = SoulsConfig::default();
    let mut log = Vec::new();
    if let Some(path) = flag(args, TUNING_FLAG).or_else(|| env(TUNING_ENV)) {
        match read(&path).map_err(|e| e.to_string()).and_then(|text| {
            let mut next = cfg;
            let n = apply_json(&mut next, &text)?;
            Ok((next, n))
        }) {
            Ok((next, n)) => {
                cfg = next;
                log.push(format!("souls tuning: {n} override(s) from {path}"));
            }
            Err(e) => log.push(format!("souls tuning: {path}: {e}; using the defaults")),
        }
    }
    if let Some(s) = flag(args, SHAKE_FLAG).or_else(|| env(SHAKE_ENV)) {
        match s.trim().parse::<f32>() {
            Ok(v) if v.is_finite() && v >= 0.0 => cfg.shake_strength = v.min(MAX_SHAKE),
            _ => log.push(format!("souls shake: {s:?} is not a number >= 0; ignored")),
        }
    }
    (cfg, log)
}

/// Applies a JSON object of overrides; returns how many values were set. Nested objects are
/// joined with dots, so `{"feedback": {"parry": {"hit_stop": 0.1}}}` and
/// `{"feedback.parry.hit_stop": 0.1}` mean the same. Keys starting with `_` are comments.
///
/// Keys: every numeric field of `Tuning` by name (`roll_cost`, `lock_los_grace`, ...),
/// `parry_window` (`[from, to]` frames), `lock_retarget_on_kill` (bool),
/// `feedback.<parry|block|guard_break|hurt_heavy|stagger|riposte>.hit_stop` and
/// `feedback.<cause>.shake.<amplitude|duration|frequency>`, and `shake_strength`.
/// An unknown key or a bad value fails the whole file (nothing is applied).
pub fn apply_json(cfg: &mut SoulsConfig, text: &str) -> Result<usize, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("the file must hold a JSON object".into());
    }
    let mut flat = Vec::new();
    flatten("", &v, &mut flat);
    let mut next = *cfg;
    for (k, val) in &flat {
        set_key(&mut next, k, val).map_err(|e| format!("{k}: {e}"))?;
    }
    validate(&next.tuning)?;
    *cfg = next;
    Ok(flat.len())
}

fn flatten(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                if k.starts_with('_') {
                    continue;
                }
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, x, out);
            }
        }
        _ => out.push((prefix.to_string(), v.clone())),
    }
}

fn num(v: &serde_json::Value) -> Result<f32, String> {
    match v.as_f64() {
        Some(x) if x.is_finite() && x >= 0.0 => Ok(x as f32),
        _ => Err(format!("expected a number >= 0, got {v}")),
    }
}

macro_rules! tuning_f32 {
    ($t:expr, $key:expr, $val:expr; $($field:ident),* $(,)?) => {
        match $key {
            $(stringify!($field) => { $t.$field = $val; true })*
            _ => false,
        }
    };
}

fn set_key(cfg: &mut SoulsConfig, key: &str, v: &serde_json::Value) -> Result<(), String> {
    let t = &mut cfg.tuning;
    match key {
        "shake_strength" => {
            cfg.shake_strength = num(v)?.min(MAX_SHAKE);
            return Ok(());
        }
        "lock_retarget_on_kill" => {
            t.lock_retarget_on_kill = v.as_bool().ok_or("expected true or false")?;
            return Ok(());
        }
        "parry_window" => {
            let a = v
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("expected [from, to]")?;
            t.parry_window = (num(&a[0])?, num(&a[1])?);
            return Ok(());
        }
        _ => {}
    }
    if let Some(rest) = key.strip_prefix("feedback.") {
        let (cause, field) = rest
            .split_once('.')
            .ok_or("expected feedback.<cause>.<field>")?;
        let imp: &mut Impact = match cause {
            "parry" => &mut t.feedback.parry,
            "block" => &mut t.feedback.block,
            "guard_break" => &mut t.feedback.guard_break,
            "hurt_heavy" => &mut t.feedback.hurt_heavy,
            "stagger" => &mut t.feedback.stagger,
            "riposte" => &mut t.feedback.riposte,
            _ => return Err(format!("unknown feedback cause {cause:?}")),
        };
        let x = num(v)?;
        match field {
            "hit_stop" => imp.hit_stop = x,
            "shake.amplitude" => imp.shake.amplitude = x,
            "shake.duration" => imp.shake.duration = x,
            "shake.frequency" => imp.shake.frequency = x,
            _ => return Err(format!("unknown feedback field {field:?}")),
        }
        return Ok(());
    }
    let x = num(v)?;
    let known = tuning_f32!(t, key, x;
        max_stamina, stamina_regen, stamina_delay, guard_regen_mult, sprint_drain, roll_cost,
        backstep_cost, guard_stamina_ratio, sprint_hold_frames, guard_raise_frames,
        guard_counter_window, guard_arc_deg, max_poise, poise_reset, hit_stop_scale, buffer_secs,
        walk_speed, run_speed, strafe_speed, back_speed, sprint_speed, turn_free_deg,
        turn_locked_deg, turn_action_deg, charge_frames, charge_max_frames, lock_range,
        lock_break_range, weapon_reach, hero_radius, riposte_reach, lock_los_grace,
        lock_switch_max_deg, buffer_dodge_secs, stamina_regen_start, stamina_ramp,
        exhausted_delay,
    );
    if known {
        Ok(())
    } else {
        Err("unknown key".into())
    }
}

fn validate(t: &Tuning) -> Result<(), String> {
    if t.max_stamina <= 0.0 || t.max_poise <= 0.0 {
        return Err("max_stamina and max_poise must be > 0".into());
    }
    if t.parry_window.0 > t.parry_window.1 {
        return Err("parry_window: from > to".into());
    }
    if t.lock_break_range < t.lock_range {
        return Err("lock_break_range must be >= lock_range".into());
    }
    Ok(())
}

// ---- line of sight -------------------------------------------------------------------------

/// `Env::line_of_sight` over the world collision. Both points in centimetres (the physics and
/// the Souls core share Gothic units; only the render transforms are metres).
pub fn los_clear(world: &CollisionWorld, from: Vec3, to: Vec3) -> bool {
    let d = to - from;
    let len = d.length();
    let max = len - LOS_END_MARGIN;
    if !len.is_finite() || max <= 0.0 {
        return true;
    }
    world.raycast(from, d / len, max).is_none()
}

/// The render camera (`eye`, cm) cannot see the target standing at `target` (feet, cm).
pub fn camera_occluded(world: &CollisionWorld, eye: Vec3, target: Vec3) -> bool {
    !los_clear(world, eye, target + Vec3::Y * TARGET_CHEST)
}

/// Lock camera occlusion of the current frame and a few counters for the QA scenarios.
#[derive(Resource, Default, Debug)]
pub struct LockView {
    pub occluded: bool,
    pub occluded_frames: u32,
    /// Largest camera shake offset applied so far (cm).
    pub max_shake_cm: f32,
    pub shaken_frames: u32,
}

/// Casts from the render camera to the locked target (runs before `souls::drive`, so the lock
/// camera reacts to the view of the frame before).
pub fn occlusion(
    gate: Res<SoulsGate>,
    res: Res<SoulsRes>,
    collision: Res<Collision>,
    cam: Query<&Transform, With<PlayerCamera>>,
    mut view: ResMut<LockView>,
) {
    let occluded = gate.active
        && match (res.world.lock_pos(), cam.single()) {
            (Some(t), Ok(tf)) => camera_occluded(&collision.0, tf.translation / CM, t),
            _ => false,
        };
    view.occluded = occluded;
    if occluded {
        view.occluded_frames += 1;
    }
}

// ---- feedback ------------------------------------------------------------------------------

/// The outcome of an NPC swing that the simulation resolved against the hero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum External {
    Parried,
    Blocked,
    GuardBroken,
    Hurt(Hurt),
}

impl External {
    /// Block outcome after `Hero::apply_external_block` ran (the bar ran out: guard break).
    pub fn block_of(hero: &Hero) -> Self {
        if hero.action() == Some(ActionId::GuardBreak) {
            Self::GuardBroken
        } else {
            Self::Blocked
        }
    }
}

/// The feedback the core gives for the same outcome of its own swings (`SoulsWorld::foe_attacks`).
pub fn external_feedback(o: External, t: &Tuning) -> Option<Event> {
    let f = &t.feedback;
    let (cause, imp) = match o {
        External::Parried => (FeedbackCause::Parry, f.parry),
        External::Blocked => (FeedbackCause::Block, f.block),
        External::GuardBroken => (FeedbackCause::GuardBreak, f.guard_break),
        External::Hurt(Hurt::Large | Hurt::Knockdown) => (FeedbackCause::HurtHeavy, f.hurt_heavy),
        External::Hurt(_) => return None,
    };
    Some(Event::Feedback {
        cause,
        hit_stop: imp.hit_stop,
        shake: imp.shake,
    })
}

/// Feedback events of the outcomes, with the hit-stop applied to the hero like the core does.
pub fn external_events(outcomes: &[External], hero: &mut Hero, t: &Tuning) -> Vec<Event> {
    let mut out = Vec::new();
    for o in outcomes {
        if let Some(e) = external_feedback(*o, t) {
            if let Event::Feedback { hit_stop, .. } = e {
                hero.hit_stop = hero.hit_stop.max(hit_stop);
            }
            out.push(e);
        }
    }
    out
}

/// Camera shake offset (cm, crate glam) as a render-space translation (m).
pub fn shake_offset_m(offset_cm: Vec3) -> Vec3 {
    offset_cm * CM
}

/// Adds the plugin's `CameraShake` offset to the render camera after `cam::play_camera` set it
/// (PostUpdate, before the transforms propagate). Skips the frames in which `play_camera` does
/// not write the camera (fly mode, cinematic, not placed) so the offset never accumulates.
pub fn apply_shake(
    shake: Res<CameraShake>,
    player: Res<Player>,
    state: Res<PlayState>,
    mut view: ResMut<LockView>,
    mut cam: Query<&mut Transform, With<PlayerCamera>>,
) {
    let off = to_bevy(shake.offset);
    if off == Vec3::ZERO || player.fly || !player.placed || state.cinematic.is_some() {
        return;
    }
    if let Ok(mut tf) = cam.single_mut() {
        tf.translation += shake_offset_m(off);
        view.max_shake_cm = view.max_shake_cm.max(off.length());
        view.shaken_frames += 1;
    }
}

// ---- telegraphs ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Pending {
    start_ms: u64,
    at_ms: u64,
    committed: bool,
}

/// Turns the sim brain's held NPC swings into `FoeTelegraph` / `FoeCommit` events.
#[derive(Resource, Default, Debug)]
pub struct TelegraphTracker {
    live: BTreeMap<u32, Pending>,
}

impl TelegraphTracker {
    /// `strikes`: (Souls foe id, sim time the swing lands) of every swing aimed at the hero now.
    pub fn update(&mut self, strikes: &[(u32, u64)], now_ms: u64) -> Vec<Event> {
        let mut out = Vec::new();
        for &(foe, at_ms) in strikes {
            let fresh = self.live.get(&foe).is_none_or(|p| p.at_ms != at_ms);
            if fresh {
                let lead = at_ms.saturating_sub(now_ms) as f32 / 1000.0;
                out.push(Event::FoeTelegraph {
                    foe,
                    attack: 0,
                    lead,
                    track: lead * COMMIT_SHARE,
                    // [U] the sim does not tell which swings can be parried; the original
                    // parry rules decide when it lands.
                    parryable: true,
                });
                self.live.insert(
                    foe,
                    Pending {
                        start_ms: now_ms,
                        at_ms,
                        committed: false,
                    },
                );
                continue;
            }
            let Some(p) = self.live.get_mut(&foe) else {
                continue;
            };
            let span = p.at_ms.saturating_sub(p.start_ms);
            let commit_at = p.start_ms + (span as f32 * COMMIT_SHARE) as u64;
            if !p.committed && commit_at < p.at_ms && now_ms >= commit_at {
                p.committed = true;
                out.push(Event::FoeCommit { foe, attack: 0 });
            }
        }
        self.live.retain(|id, _| strikes.iter().any(|s| s.0 == *id));
        out
    }

    pub fn clear(&mut self) {
        self.live.clear();
    }
}

/// Runs after `souls::drive`: telegraph / commit events of the NPC swings aimed at the hero.
pub fn telegraphs(
    gate: Res<SoulsGate>,
    core: Res<CoreRes>,
    mut tracker: ResMut<TelegraphTracker>,
    mut out: MessageWriter<SoulsEvent>,
) {
    if !gate.active {
        tracker.clear();
        return;
    }
    let hero = core.0.hero;
    let strikes: Vec<(u32, u64)> = core
        .0
        .sim
        .actors
        .iter()
        .filter(|a| a.id != hero)
        .filter_map(|a| {
            a.brain
                .strike
                .filter(|s| s.target == hero)
                .map(|s| (a.id.0 + 1, s.at_ms))
        })
        .collect();
    for e in tracker.update(&strikes, core.0.sim.now_ms()) {
        out.write(SoulsEvent(e));
    }
}

/// Resources and the systems that need no ordering against `souls::drive`. `souls::register`
/// orders [`occlusion`] before and [`telegraphs`] after it.
pub fn register(app: &mut App, cfg: &SoulsConfig) {
    app.world_mut().resource_mut::<CameraShake>().strength = Some(cfg.shake_strength);
    app.init_resource::<LockView>()
        .init_resource::<TelegraphTracker>()
        .add_systems(
            PostUpdate,
            apply_shake.before(bevy::transform::TransformSystems::Propagate),
        );
    super::souls_hud::register(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_combat_souls::tuning::Shake;

    fn wall_at_z(z: f32) -> CollisionWorld {
        // A 400 x 400 cm wall in the XY plane at depth `z`.
        let (a, b, c, d) = (
            Vec3::new(-200.0, 0.0, z),
            Vec3::new(200.0, 0.0, z),
            Vec3::new(200.0, 400.0, z),
            Vec3::new(-200.0, 400.0, z),
        );
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, vec![[a, b, c], [a, c, d]]);
        w
    }

    #[test]
    fn los_is_blocked_by_a_wall_between() {
        let w = wall_at_z(-300.0);
        let from = Vec3::new(0.0, 150.0, 0.0);
        assert!(!los_clear(&w, from, Vec3::new(0.0, 150.0, -600.0)));
        // Beside the wall, and the open sky above it.
        assert!(los_clear(&w, from, Vec3::new(600.0, 150.0, -600.0)));
        assert!(los_clear(
            &w,
            Vec3::new(0.0, 500.0, 0.0),
            Vec3::new(0.0, 500.0, -600.0)
        ));
    }

    #[test]
    fn los_ignores_geometry_at_the_end_point_and_short_rays() {
        let w = wall_at_z(-300.0);
        // The wall is 10 cm before the target: inside the end margin.
        assert!(los_clear(
            &w,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::new(0.0, 150.0, -310.0)
        ));
        assert!(los_clear(
            &w,
            Vec3::new(0.0, 150.0, -299.0),
            Vec3::new(0.0, 150.0, -301.0)
        ));
        assert!(los_clear(&w, Vec3::ZERO, Vec3::ZERO));
        assert!(los_clear(
            &CollisionWorld::new(),
            Vec3::ZERO,
            Vec3::splat(1e4)
        ));
    }

    #[test]
    fn camera_occlusion_aims_at_the_chest() {
        let w = wall_at_z(-300.0);
        let eye = Vec3::new(0.0, 250.0, 200.0);
        assert!(camera_occluded(&w, eye, Vec3::new(0.0, 0.0, -600.0)));
        assert!(!camera_occluded(&w, eye, Vec3::new(0.0, 0.0, -200.0)));
        // From high above, the ray clears the 4 m wall.
        assert!(!camera_occluded(
            &w,
            Vec3::new(0.0, 2000.0, 0.0),
            Vec3::new(0.0, 0.0, -400.0)
        ));
    }

    #[test]
    fn external_outcomes_map_to_the_tuned_feedback() {
        let t = Tuning::default();
        let e = external_feedback(External::Parried, &t).unwrap();
        assert_eq!(
            e,
            Event::Feedback {
                cause: FeedbackCause::Parry,
                hit_stop: t.feedback.parry.hit_stop,
                shake: t.feedback.parry.shake,
            }
        );
        let cause = |o| match external_feedback(o, &t) {
            Some(Event::Feedback { cause, .. }) => Some(cause),
            _ => None,
        };
        assert_eq!(cause(External::Blocked), Some(FeedbackCause::Block));
        assert_eq!(
            cause(External::GuardBroken),
            Some(FeedbackCause::GuardBreak)
        );
        assert_eq!(
            cause(External::Hurt(Hurt::Large)),
            Some(FeedbackCause::HurtHeavy)
        );
        assert_eq!(
            cause(External::Hurt(Hurt::Knockdown)),
            Some(FeedbackCause::HurtHeavy)
        );
        assert_eq!(cause(External::Hurt(Hurt::Small)), None);
        assert_eq!(cause(External::Hurt(Hurt::Middle)), None);
    }

    #[test]
    fn external_events_apply_the_longest_hit_stop() {
        let t = Tuning::default();
        let mut hero = Hero::new(0, Vec3::ZERO, 100.0, &t);
        let ev = external_events(
            &[
                External::Blocked,
                External::GuardBroken,
                External::Hurt(Hurt::Small),
            ],
            &mut hero,
            &t,
        );
        assert_eq!(ev.len(), 2);
        assert!((hero.hit_stop - t.feedback.guard_break.hit_stop).abs() < 1e-6);
    }

    #[test]
    fn a_blocked_hit_that_empties_the_bar_is_a_guard_break() {
        let t = Tuning::default();
        let mut hero = Hero::new(0, Vec3::ZERO, 100.0, &t);
        hero.apply_external_block(5.0, &t);
        assert_eq!(External::block_of(&hero), External::Blocked);
        hero.apply_external_block(1000.0, &t);
        assert_eq!(External::block_of(&hero), External::GuardBroken);
    }

    #[test]
    fn shake_offsets_become_metres_and_scale_to_zero() {
        assert_eq!(
            shake_offset_m(Vec3::new(100.0, -50.0, 3.0)),
            Vec3::new(1.0, -0.5, 0.03)
        );
        let run = |strength| {
            let mut s = CameraShake::default();
            s.strength = Some(strength);
            s.add(Shake::new(9.0, 0.4, 16.0));
            let mut peak = 0.0f32;
            for _ in 0..30 {
                s.advance(1.0 / 60.0);
                peak = peak.max(to_bevy(s.offset).length());
            }
            peak
        };
        assert!(run(1.0) > 1.0);
        assert!((run(0.5) - run(1.0) * 0.5).abs() < 1e-3);
        assert_eq!(run(0.0), 0.0);
    }

    #[test]
    fn telegraph_then_commit_then_forget() {
        let mut tr = TelegraphTracker::default();
        let ev = tr.update(&[(7, 1650)], 1000);
        assert_eq!(ev.len(), 1);
        let Event::FoeTelegraph {
            foe, lead, track, ..
        } = ev[0]
        else {
            panic!("{ev:?}");
        };
        assert_eq!(foe, 7);
        assert!((lead - 0.65).abs() < 1e-4);
        assert!((track - 0.65 * COMMIT_SHARE).abs() < 1e-4);
        // The same strike: nothing until the commit point (1000 + 0.55 * 650 = 1357).
        assert!(tr.update(&[(7, 1650)], 1300).is_empty());
        assert_eq!(
            tr.update(&[(7, 1650)], 1360),
            vec![Event::FoeCommit { foe: 7, attack: 0 }]
        );
        assert!(tr.update(&[(7, 1650)], 1500).is_empty());
        // Swing landed and gone; the next one telegraphs again.
        assert!(tr.update(&[], 1700).is_empty());
        assert_eq!(tr.update(&[(7, 3000)], 2400).len(), 1);
    }

    #[test]
    fn a_swing_due_now_has_no_commit_cue() {
        let mut tr = TelegraphTracker::default();
        let ev = tr.update(&[(3, 500)], 500);
        assert!(matches!(ev[0], Event::FoeTelegraph { lead, .. } if lead == 0.0));
        assert!(tr.update(&[(3, 500)], 600).is_empty());
    }

    #[test]
    fn several_foes_are_tracked_apart() {
        let mut tr = TelegraphTracker::default();
        assert_eq!(tr.update(&[(1, 2000), (2, 2500)], 1000).len(), 2);
        // Foe 2 retargets to a new swing time: a fresh telegraph.
        let ev = tr.update(&[(1, 2000), (2, 2800)], 1100);
        assert_eq!(ev.len(), 1);
        assert!(matches!(ev[0], Event::FoeTelegraph { foe: 2, .. }));
    }

    #[test]
    fn json_overrides_set_fields_and_reject_unknown_keys() {
        let mut cfg = SoulsConfig::default();
        let n = apply_json(
            &mut cfg,
            r#"{
                "_comment": "test",
                "roll_cost": 12,
                "lock_los_grace": 1.5,
                "lock_retarget_on_kill": false,
                "parry_window": [1, 8],
                "feedback": {"parry": {"hit_stop": 0.2, "shake": {"amplitude": 4}}},
                "feedback.block.shake.frequency": 12,
                "shake_strength": 0.25
            }"#,
        )
        .unwrap();
        assert_eq!(n, 8);
        let t = cfg.tuning;
        assert_eq!(t.roll_cost, 12.0);
        assert_eq!(t.lock_los_grace, 1.5);
        assert!(!t.lock_retarget_on_kill);
        assert_eq!(t.parry_window, (1.0, 8.0));
        assert_eq!(t.feedback.parry.hit_stop, 0.2);
        assert_eq!(t.feedback.parry.shake.amplitude, 4.0);
        assert_eq!(
            t.feedback.parry.shake.duration,
            Tuning::default().feedback.parry.shake.duration
        );
        assert_eq!(t.feedback.block.shake.frequency, 12.0);
        assert_eq!(cfg.shake_strength, 0.25);

        // A bad file changes nothing.
        let before = cfg;
        for bad in [
            r#"{"roll_cost": 3, "no_such_field": 1}"#,
            r#"{"roll_cost": -1}"#,
            r#"{"feedback": {"nope": {"hit_stop": 1}}}"#,
            r#"{"parry_window": [9, 2]}"#,
            r#"{"lock_range": 3000}"#,
            r#"[1, 2]"#,
            "not json",
        ] {
            assert!(apply_json(&mut cfg, bad).is_err(), "{bad}");
            assert_eq!(cfg, before, "{bad}");
        }
    }

    #[test]
    fn config_from_flags_and_environment() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let file = |_: &str| Ok(r#"{"roll_cost": 20, "shake_strength": 0.5}"#.to_string());
        let none = |_: &str| None;
        let (cfg, log) = load_config(&args(&["--play"]), none, file);
        assert_eq!(cfg, SoulsConfig::default());
        assert!(log.is_empty());

        let (cfg, log) = load_config(&args(&["--souls-tuning", "t.json"]), none, file);
        assert_eq!(cfg.tuning.roll_cost, 20.0);
        assert_eq!(cfg.shake_strength, 0.5);
        assert_eq!(log.len(), 1);

        // The flag beats the file's value and the environment.
        let env = |k: &str| (k == SHAKE_ENV).then(|| "0.75".to_string());
        let (cfg, _) = load_config(
            &args(&["--souls-tuning", "t.json", "--souls-shake", "0"]),
            env,
            file,
        );
        assert_eq!(cfg.shake_strength, 0.0);
        let (cfg, _) = load_config(&args(&[]), env, file);
        assert_eq!(cfg.shake_strength, 0.75);
        let (cfg, _) = load_config(&args(&["--souls-shake", "9"]), none, file);
        assert_eq!(cfg.shake_strength, MAX_SHAKE);
        let (cfg, log) = load_config(&args(&["--souls-shake", "loud"]), none, file);
        assert_eq!(cfg.shake_strength, 1.0);
        assert_eq!(log.len(), 1);

        // The environment names the file; a missing file keeps the defaults.
        let env = |k: &str| (k == TUNING_ENV).then(|| "missing.json".to_string());
        let missing = |_: &str| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        let (cfg, log) = load_config(&args(&[]), env, missing);
        assert_eq!(cfg, SoulsConfig::default());
        assert!(log[0].contains("missing.json"), "{log:?}");
    }
}

/// The wiring systems in a headless Bevy app over a synthetic core (no game data): catches
/// system parameter conflicts and checks the systems end to end.
#[cfg(test)]
mod app_tests {
    use super::super::Roster;
    use super::super::souls_hud::SoulsHudModel;
    use super::*;
    use gothic_combat_souls::SoulsWorld;
    use gothic_combat_souls::foe::FoeProfile;
    use gothic_combat_souls::tuning::Shake;
    use gothic_gameplay::{GameDatabase, GameState};
    use gothic_physics::{CharacterController, ControllerConfig};
    use gothic_sim::actor::Actor;
    use gothic_sim::item::ItemDb;
    use gothic_sim::types::{ActorId, Species};
    use gothic_sim::world::Pos;
    use std::sync::Arc;

    fn strings() -> gothic_formats::stringtable::StringTable {
        let text = "[LocAdmin_Languages]\r\nLanguages=English\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
        let mut b = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        gothic_formats::stringtable::StringTable::parse(&b).unwrap()
    }

    /// A core with the hero and one orc 600 cm ahead; returns the orc's Souls foe id.
    fn core() -> (CoreRes, u32) {
        let db = Arc::new(GameDatabase::from_parts(vec![], vec![], strings(), "English").unwrap());
        let mut c = gothic_runtime::Core::new(db, ItemDb::default(), GameState::new(), 7);
        c.guess_weapons = false;
        let orc = Actor::npc(ActorId(0), "Orc_Test", Species::Orc, 10, 10);
        let id = c.sim.spawn(orc, Pos::flat(0.0, -600.0));
        (CoreRes(c), id.0 + 1)
    }

    fn build(wall: bool) -> (App, u32) {
        let (core, foe) = core();
        let mut world = SoulsWorld::new(Vec3::ZERO, 100.0, Tuning::default());
        world.add_foe(
            foe,
            Vec3::new(0.0, 0.0, -600.0),
            50.0,
            FoeProfile::telegraphed(),
        );
        world.lock.target = Some(foe);
        let mut collision = CollisionWorld::new();
        if wall {
            let (a, b, c, d) = (
                Vec3::new(-200.0, -100.0, -300.0),
                Vec3::new(200.0, -100.0, -300.0),
                Vec3::new(200.0, 400.0, -300.0),
                Vec3::new(-200.0, 400.0, -300.0),
            );
            collision.insert_chunk(1, vec![[a, b, c], [a, c, d]]);
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SoulsEvent>()
            .insert_resource(core)
            .insert_resource(SoulsRes::new(world))
            .insert_resource(SoulsGate { active: true })
            .insert_resource(Collision(collision))
            .insert_resource(Player {
                ctrl: CharacterController::new(ControllerConfig::default(), Vec3::ZERO),
                yaw: 0.0,
                pitch: 0.0,
                fly: false,
                third: false,
                placed: true,
                fly_pos: Vec3::ZERO,
                scripted_wish: None,
                scripted_gait: None,
                scripted_jump: None,
                command: true,
                face: None,
                report: Default::default(),
                scripted_dive: None,
                scripted_water: None,
                magic_body_trace: None,
            })
            .init_resource::<PlayState>()
            .init_resource::<Roster>()
            .init_resource::<UiScale>()
            .init_resource::<CameraShake>()
            .init_resource::<LockView>()
            .init_resource::<TelegraphTracker>()
            .add_systems(Update, (occlusion, telegraphs).chain())
            .add_systems(PostUpdate, apply_shake);
        super::super::souls_hud::register(&mut app);
        // The render camera 2.5 m above and 2 m behind the hero (metres).
        app.world_mut()
            .spawn((PlayerCamera, Transform::from_xyz(0.0, 2.5, 2.0)));
        (app, foe)
    }

    #[test]
    fn occlusion_follows_the_world_collision() {
        let (mut app, _) = build(true);
        app.update();
        assert!(app.world().resource::<LockView>().occluded);
        let (mut app, _) = build(false);
        app.update();
        assert!(!app.world().resource::<LockView>().occluded);
        // Not driving the hero: never occluded.
        let (mut app, _) = build(true);
        app.world_mut().resource_mut::<SoulsGate>().active = false;
        app.update();
        assert!(!app.world().resource::<LockView>().occluded);
    }

    #[test]
    fn hud_shows_lock_and_telegraph_of_a_sim_strike() {
        let (mut app, foe) = build(false);
        {
            let mut core = app.world_mut().resource_mut::<CoreRes>();
            let hero = core.0.hero;
            let now = core.0.sim.now_ms();
            core.0.sim.actor_mut(ActorId(foe - 1)).brain.strike = Some(gothic_sim::brain::Strike {
                at_ms: now + 650,
                action: gothic_sim::types::Action::Attack,
                target: hero,
            });
        }
        app.update();
        app.update();
        let hud = app.world().resource::<SoulsHudModel>();
        assert_eq!(hud.counts.telegraphs, 1);
        assert_eq!(hud.shown_lock, "LOCK  Orc_Test");
        assert!(
            hud.shown_threat.starts_with("WIND-UP"),
            "{:?}",
            hud.shown_threat
        );
        // Leaving the flow hides the lines.
        app.world_mut().resource_mut::<SoulsGate>().active = false;
        app.update();
        let hud = app.world().resource::<SoulsHudModel>();
        assert!(hud.shown_lock.is_empty() && hud.shown_threat.is_empty());
    }

    #[test]
    fn shake_moves_the_camera_and_strength_zero_does_not() {
        for (strength, moves) in [(1.0, true), (0.0, false)] {
            let (mut app, _) = build(false);
            {
                let mut s = app.world_mut().resource_mut::<CameraShake>();
                s.strength = Some(strength);
                s.add(Shake::new(9.0, 0.4, 16.0));
                // The plugin's PostUpdate system advances it; do it by hand here.
                s.advance(0.01);
            }
            app.update();
            let v = app.world().resource::<LockView>();
            assert_eq!(v.max_shake_cm > 0.0, moves, "strength {strength}: {v:?}");
        }
        // Fly mode: the camera is not ours, nothing is added.
        let (mut app, _) = build(false);
        app.world_mut().resource_mut::<Player>().fly = true;
        {
            let mut s = app.world_mut().resource_mut::<CameraShake>();
            s.add(Shake::new(9.0, 0.4, 16.0));
            s.advance(0.01);
        }
        app.update();
        assert_eq!(app.world().resource::<LockView>().shaken_frames, 0);
    }
}
