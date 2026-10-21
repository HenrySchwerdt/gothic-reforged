//! Tunable numbers of the Souls flow. Everything here is a timing, distance or resource rule of
//! the *flow*; damage numbers never appear (they come from the Gothic pipeline, see [`crate::sink`]).
//! All values are estimates chosen for feel and are meant to be changed through profiles.

/// Simulation tick of the core.
pub const TICK_HZ: f32 = 60.0;
pub const DT: f32 = 1.0 / TICK_HZ;
/// Action timings are written in frames of 30 fps animation; one tick is half a frame.
pub const ANIM_FPS: f32 = 30.0;
pub const DF: f32 = ANIM_FPS * DT;

/// A camera shake: presentation only.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Shake {
    /// Peak offset of the camera (cm).
    pub amplitude: f32,
    /// Seconds until it has died out.
    pub duration: f32,
    /// Oscillations per second.
    pub frequency: f32,
}

impl Shake {
    pub const fn new(amplitude: f32, duration: f32, frequency: f32) -> Self {
        Self {
            amplitude,
            duration,
            frequency,
        }
    }

    /// Offset factor at `t` seconds (decaying sine, deterministic): multiply with
    /// `amplitude` and a direction.
    pub fn at(&self, t: f32) -> f32 {
        if t < 0.0 || t >= self.duration || self.duration <= 0.0 {
            return 0.0;
        }
        let decay = 1.0 - t / self.duration;
        (t * self.frequency * std::f32::consts::TAU).sin() * decay * decay
    }
}

/// Hit-stop (seconds the hero freezes) and camera shake per kind of impact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub hit_stop: f32,
    pub shake: Shake,
}

/// Feedback of the defensive moments and big hits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeedbackTuning {
    pub parry: Impact,
    pub block: Impact,
    pub guard_break: Impact,
    /// The hero takes a heavy (knock-down level) hit.
    pub hurt_heavy: Impact,
    /// The hero breaks a foe's poise.
    pub stagger: Impact,
    pub riposte: Impact,
}

impl Default for FeedbackTuning {
    fn default() -> Self {
        Self {
            parry: Impact {
                hit_stop: 0.12,
                shake: Shake::new(3.0, 0.18, 26.0),
            },
            block: Impact {
                hit_stop: 0.05,
                shake: Shake::new(1.5, 0.12, 30.0),
            },
            guard_break: Impact {
                hit_stop: 0.15,
                shake: Shake::new(9.0, 0.4, 16.0),
            },
            hurt_heavy: Impact {
                hit_stop: 0.08,
                shake: Shake::new(7.0, 0.35, 18.0),
            },
            stagger: Impact {
                hit_stop: 0.0,
                shake: Shake::new(2.5, 0.2, 22.0),
            },
            riposte: Impact {
                hit_stop: 0.1,
                shake: Shake::new(5.0, 0.3, 20.0),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub max_stamina: f32,
    /// Stamina per second once the regen delay has passed.
    pub stamina_regen: f32,
    /// Seconds after spending stamina before it refills.
    pub stamina_delay: f32,
    /// Regen multiplier while the guard is up.
    pub guard_regen_mult: f32,
    pub sprint_drain: f32,
    pub roll_cost: f32,
    pub backstep_cost: f32,
    /// Share of a blocked hit's damage taken from stamina.
    pub guard_stamina_ratio: f32,
    /// Frames of a held dodge that turn a tap (roll on release) into a sprint.
    pub sprint_hold_frames: f32,
    /// Frames after the guard press during which a hit is parried.
    pub parry_window: (f32, f32),
    /// Frames the guard needs to come up.
    pub guard_raise_frames: f32,
    /// Frames after a block in which a heavy becomes a guard counter.
    pub guard_counter_window: f32,
    /// Half-angle of the arc in front a raised guard covers.
    pub guard_arc_deg: f32,
    pub max_poise: f32,
    /// Seconds without being hit before poise refills.
    pub poise_reset: f32,
    /// Share of an attack's hit-stop time applied.
    pub hit_stop_scale: f32,
    /// Seconds the input buffer remembers a press.
    pub buffer_secs: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub strafe_speed: f32,
    pub back_speed: f32,
    pub sprint_speed: f32,
    pub turn_free_deg: f32,
    pub turn_locked_deg: f32,
    /// Turn rate inside an action before its hit window.
    pub turn_action_deg: f32,
    /// Heavy attack: frames the button must be held at the charge point to count as charged.
    pub charge_frames: f32,
    /// Longest the charge is held before it releases on its own.
    pub charge_max_frames: f32,
    pub lock_range: f32,
    pub lock_break_range: f32,
    /// Reach of the player's weapon in cm (added to the foe radius).
    pub weapon_reach: f32,
    pub hero_radius: f32,
    /// Distance within which a riposte can be started on a staggered enemy.
    pub riposte_reach: f32,
    /// Seconds the lock survives without line of sight (a pillar between hero and target).
    pub lock_los_grace: f32,
    /// When the locked foe dies, lock onto the next one in view instead of dropping the lock.
    pub lock_retarget_on_kill: bool,
    /// Largest angle (from the current target, as seen by the hero) a lock switch may jump.
    pub lock_switch_max_deg: f32,
    /// Seconds a buffered roll stays valid (attacks use `buffer_secs`): shorter, so a late
    /// press does not roll you out of a fight after the moment has passed.
    pub buffer_dodge_secs: f32,
    /// Regen starts at this share of `stamina_regen` and reaches full rate after
    /// `stamina_ramp` seconds.
    pub stamina_regen_start: f32,
    pub stamina_ramp: f32,
    /// Extra seconds before regen when the bar was emptied.
    pub exhausted_delay: f32,
    /// Hit-stop and camera shake per impact kind.
    pub feedback: FeedbackTuning,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            max_stamina: 100.0,
            stamina_regen: 48.0,
            stamina_delay: 0.6,
            guard_regen_mult: 0.5,
            sprint_drain: 10.0,
            roll_cost: 14.0,
            backstep_cost: 9.0,
            guard_stamina_ratio: 0.45,
            sprint_hold_frames: 10.0,
            parry_window: (2.0, 9.0),
            guard_raise_frames: 4.0,
            guard_counter_window: 20.0,
            guard_arc_deg: 80.0,
            max_poise: 40.0,
            poise_reset: 6.0,
            hit_stop_scale: 0.5,
            buffer_secs: 0.4,
            walk_speed: 150.0,
            run_speed: 400.0,
            strafe_speed: 280.0,
            back_speed: 280.0,
            sprint_speed: 600.0,
            turn_free_deg: 900.0,
            turn_locked_deg: 700.0,
            turn_action_deg: 360.0,
            charge_frames: 12.0,
            charge_max_frames: 40.0,
            lock_range: 1500.0,
            lock_break_range: 2300.0,
            weapon_reach: 190.0,
            hero_radius: 35.0,
            riposte_reach: 230.0,
            lock_los_grace: 1.0,
            lock_retarget_on_kill: true,
            lock_switch_max_deg: 100.0,
            buffer_dodge_secs: 0.25,
            stamina_regen_start: 0.35,
            stamina_ramp: 0.5,
            exhausted_delay: 0.9,
            feedback: FeedbackTuning::default(),
        }
    }
}
