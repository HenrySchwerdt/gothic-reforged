//! Tunable numbers of the Souls flow. Everything here is a timing, distance or resource rule of
//! the *flow*; damage numbers never appear (they come from the Gothic pipeline, see [`crate::sink`]).
//! All values are estimates chosen for feel and are meant to be changed through profiles.

/// Simulation tick of the core.
pub const TICK_HZ: f32 = 60.0;
pub const DT: f32 = 1.0 / TICK_HZ;
/// Action timings are written in frames of 30 fps animation; one tick is half a frame.
pub const ANIM_FPS: f32 = 30.0;
pub const DF: f32 = ANIM_FPS * DT;

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
        }
    }
}
