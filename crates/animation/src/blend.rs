//! Motion-instance semantics of the original animation stack (E03).
//!
//! Pure data and rules, no ECS and no clip data: the [`Animator`](crate::animator::Animator)
//! owns the clips and drives these instances. What is modelled:
//!
//! * [`AniBlendings`]: `G3_Ani_Blendings.txt` (intro/extro fade of a clip family) and the
//!   variation maximum of a clip family (the `eCAnimationAdmin` tables).
//! * [`Instance`]: one running motion with its weight ramp, loop counter, play mode,
//!   priority and additive flag, including the wrapper rules `SwitchFadeOut`, `StopAtLoopEnd`,
//!   `SetTargetWeight` and the stop prevention that holds the last pose.
//! * [`stacked_weights`] / [`contributions`]: how the layer system combines instances
//!   (each instance is lerped over the result of everything below it with its own weight).
//! * [`plan_transition`]: the call sequence the mixers and the script start use to replace the
//!   primary clip.
use std::collections::HashMap;

/// `eCAnimationAdmin` standard fade time (`GetStandardAniFadeTime`), seconds.
pub const STANDARD_FADE: f32 = 0.2;
/// Fade of the old primary clip when the idle mixer takes over from a non-idle clip.
pub const IDLE_TAKEOVER_FADE: f32 = 0.5;
/// Fade used by `sAIPlayAniStart` for `_Begin_` clips.
pub const BEGIN_INTRO_FADE: f32 = 0.1;
/// Blend time EMotionFX uses when a motion is played without a descriptor.
pub const ENGINE_DEFAULT_BLEND: f32 = 0.3;
/// Priority `SwitchFadeOut` gives the fading instance: it sorts above every normal instance.
pub const FADING_PRIORITY: i32 = 5;
/// The blending table stores bytes in units of 1/15 s.
pub const BLEND_TABLE_UNIT: f32 = 15.0;
/// Number of motion slots (`eEMotionType`).
pub const MOTION_SLOTS: usize = 8;
/// Time left on a held last pose (the stop prevention keeps the clock this far from the end).
const END_EPSILON: f32 = 1e-4;

/// Motion slot numbers (`eEMotionType`).
pub mod slot {
    pub const PRIMARY: u8 = 0;
    pub const SECONDARY: u8 = 1;
    pub const THIRD: u8 = 2;
    pub const FOURTH: u8 = 3;
    pub const OVERLAY_1: u8 = 4;
    pub const OVERLAY_2: u8 = 5;
    pub const FACIAL: u8 = 7;
}

/// Slot owner values (`eEMotionOwner`), the cooperative lock between the systems.
pub mod owner {
    /// Free or just stopped.
    pub const FREE: u8 = 2;
    /// Transitional states of the locomotion mixer (8 is rewritten to 3 at once).
    pub const TRANSITION: u8 = 3;
    /// Owned by a script animation.
    pub const SCRIPT: u8 = 4;
    /// Idle / stand state of the mixer.
    pub const IDLE: u8 = 9;
    /// Directional locomotion.
    pub const DIRECTIONAL: u8 = 10;
    /// Turn in place.
    pub const TURN: u8 = 11;
}

// ---------------------------------------------------------------------------------------------
// eCAnimationAdmin tables
// ---------------------------------------------------------------------------------------------

/// Cuts a clip name at the first `%` and upper-cases it: the lookup key of the admin tables.
pub fn blending_key(name: &str) -> String {
    let stem = name.split('%').next().unwrap_or(name);
    stem.to_ascii_uppercase()
}

/// Intro/extro blend table and per-family variation maxima of `eCAnimationAdmin`.
#[derive(Debug, Default, Clone)]
pub struct AniBlendings {
    blends: HashMap<String, (u8, u8)>,
    variation_max: HashMap<String, u32>,
}

impl AniBlendings {
    /// Port of eCAnimationAdmin::Initialize (Engine:0x10020a20), the `G3_Ani_Blendings.txt`
    /// part. The text must contain `*Version 1`; every `*Entry->` is followed by a clip key and
    /// two integers (intro, extro) stored as bytes. A text without the version marker yields
    /// an empty table (the engine then uses the standard fade everywhere).
    pub fn parse(text: &str) -> Self {
        let mut table = Self::default();
        if !text.contains("*Version 1") {
            return table;
        }
        for part in text.split("*Entry->").skip(1) {
            let mut tokens = part
                .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                .filter(|t| !t.is_empty());
            let (Some(name), Some(intro), Some(extro)) =
                (tokens.next(), tokens.next(), tokens.next())
            else {
                continue;
            };
            let (Ok(intro), Ok(extro)) = (intro.parse::<i32>(), extro.parse::<i32>()) else {
                continue;
            };
            table
                .blends
                .insert(blending_key(name), (intro as u8, extro as u8));
        }
        table
    }

    pub fn len(&self) -> usize {
        self.blends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blends.is_empty()
    }

    /// Port of eCAnimationAdmin::GetAniIntroBlending (Engine:0x100213a0): stored byte / 15 s,
    /// the standard fade when the key is unknown.
    pub fn intro(&self, name: &str) -> f32 {
        self.blends
            .get(&blending_key(name))
            .map_or(STANDARD_FADE, |&(i, _)| f32::from(i) / BLEND_TABLE_UNIT)
    }

    /// Port of eCAnimationAdmin::GetAniExtroBlending (Engine:0x10021450).
    pub fn extro(&self, name: &str) -> f32 {
        self.blends
            .get(&blending_key(name))
            .map_or(STANDARD_FADE, |&(_, e)| f32::from(e) / BLEND_TABLE_UNIT)
    }

    /// Port of eCAnimationAdmin::SetAniVariationMax (Engine:0x100211c0): on first sight of a
    /// clip family, word 11 of the underscore-separated file name is its variation maximum.
    pub fn register_variation(&mut self, file_name: &str) {
        let key = blending_key(file_name);
        if self.variation_max.contains_key(&key) {
            return;
        }
        let max = file_name
            .split('_')
            .nth(11)
            .and_then(|w| w.parse::<u32>().ok())
            .unwrap_or(0);
        self.variation_max.insert(key, max);
    }

    pub fn variation_max(&self, name: &str) -> Option<u32> {
        self.variation_max.get(&blending_key(name)).copied()
    }

    /// Port of eCAnimationAdmin::GetAniVariation (Engine:0x10021290): with a known maximum the
    /// two characters before the trailing underscore of the key become a random number in
    /// `0..=max` (`rand` is the caller's random value). Keys without a maximum are unchanged.
    pub fn with_variation(&self, key: &str, rand: u32) -> String {
        let Some(max) = self.variation_max(key) else {
            return key.to_owned();
        };
        let bytes = key.as_bytes();
        if bytes.len() < 3 || !key.is_char_boundary(bytes.len() - 3) {
            return key.to_owned();
        }
        let head = &key[..bytes.len() - 3];
        format!("{head}{:02}_", rand % (max + 1))
    }
}

/// Fade the old primary clip gets when a script clip replaces it (`sAIPlayAniStart`,
/// Game:0x1016a360): `_Begin_` clips use 0.1 s, others the smaller of the old clip's extro and
/// the new clip's intro.
pub fn script_fade(table: &AniBlendings, old_clip: &str, new_clip: &str) -> f32 {
    if new_clip.contains("_Begin_") {
        BEGIN_INTRO_FADE
    } else {
        table.extro(old_clip).min(table.intro(new_clip))
    }
}

/// Fade of the old primary clip when a locomotion mixer replaces it: the idle mixer uses
/// [`IDLE_TAKEOVER_FADE`] unless the old clip was an idle one, every other mixer the standard
/// fade (FUN_100d9940, FUN_100d9d80, FUN_100da480).
pub fn mixer_fade(to_idle: bool, old_owner: u8) -> f32 {
    if to_idle && old_owner != owner::IDLE {
        IDLE_TAKEOVER_FADE
    } else {
        STANDARD_FADE
    }
}

// ---------------------------------------------------------------------------------------------
// Motion instances
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayMode {
    #[default]
    Forward,
    Backward,
    PingPong,
}

/// Motion descriptor of `PlayMotion` (`eSMotionDesc`, Engine:0x10030860).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayDesc {
    /// Fade-in seconds; 0 starts at full weight.
    pub fade_in: f32,
    pub mode: PlayMode,
    pub speed: f32,
    /// -1 loops forever, 1 plays once.
    pub max_loops: i32,
    pub weight: f32,
    /// Field +0x14: 0.1 s for `_Begin_`/`_End_` script clips, consumer unknown.
    pub extra_blend: f32,
}

impl Default for PlayDesc {
    /// What the mixers pass: no fade-in, forward, speed 1, endless loop, weight 1.
    fn default() -> Self {
        Self {
            fade_in: 0.0,
            mode: PlayMode::Forward,
            speed: 1.0,
            max_loops: -1,
            weight: 1.0,
            extra_blend: 0.0,
        }
    }
}

impl PlayDesc {
    /// The facial call: forward, once, full weight.
    pub fn once() -> Self {
        Self {
            max_loops: 1,
            ..Self::default()
        }
    }
}

/// One playing motion (the EMotionFX motion instance block that the wrapper manipulates).
#[derive(Clone, Debug)]
pub struct Instance {
    /// Current play time in seconds.
    pub time: f32,
    pub speed: f32,
    /// Motion duration in seconds.
    pub duration: f32,
    /// Weight the layer system uses right now.
    pub weight: f32,
    pub target_weight: f32,
    /// Weight change per second while `ramping`.
    pub weight_rate: f32,
    pub ramping: bool,
    pub loop_index: i32,
    pub max_loops: i32,
    pub paused: bool,
    pub mode: PlayMode,
    /// Sort key of the layer system; [`FADING_PRIORITY`] once the instance fades out.
    pub priority: i32,
    /// Mix flag (blend mode 2): the instance is added on top of what is below it.
    pub additive: bool,
    /// Set when the instance faded out completely and may be dropped.
    pub expired: bool,
    /// True once the last loop ended without stop prevention (non-primary slots).
    pub ended: bool,
    reversed: bool,
}

impl Instance {
    /// Starts an instance. Port of the EMotionFX add-instance path (Engine:0x102ed2b0): the
    /// target weight is clamped to 0..1; without a fade-in the weight is the target at once,
    /// otherwise it ramps linearly from 0 over `fade_in`.
    pub fn new(duration: f32, desc: &PlayDesc, start_time: f32) -> Self {
        let target = desc.weight.clamp(0.0, 1.0);
        let (weight, rate, ramping) = if desc.fade_in <= 0.0 {
            (target, 0.0, false)
        } else {
            (0.0, target / desc.fade_in, true)
        };
        let backward = desc.mode == PlayMode::Backward;
        Self {
            time: if start_time > 0.0 {
                start_time
            } else if backward {
                duration
            } else {
                0.0
            },
            speed: desc.speed,
            duration,
            weight,
            target_weight: target,
            weight_rate: rate,
            ramping,
            loop_index: 0,
            max_loops: desc.max_loops,
            paused: false,
            mode: desc.mode,
            priority: 0,
            additive: false,
            expired: false,
            ended: false,
            reversed: backward,
        }
    }

    /// Port of eCWrapper_emfx2Actor::IsActorPlaying (Engine:0x1004a040) for an existing
    /// instance: not past its maximum loops and not paused.
    pub fn is_playing(&self) -> bool {
        !self.paused && (self.max_loops == -1 || self.loop_index < self.max_loops)
    }

    /// The last loop has been played to its end (held by stop prevention or finished).
    pub fn at_end(&self) -> bool {
        self.ended
            || (self.max_loops != -1
                && self.loop_index + 1 >= self.max_loops
                && self.speed == 0.0
                && self.time >= self.duration - 2.0 * END_EPSILON)
    }

    /// Port of eCWrapper_emfx2Actor::SetTargetWeight (Engine:0x10049ca0): an instant change of
    /// current and target weight.
    pub fn set_target_weight(&mut self, weight: f32) {
        let w = weight.clamp(0.0, 1.0);
        self.weight = w;
        self.target_weight = w;
        self.weight_rate = 0.0;
        self.ramping = false;
    }

    /// Port of eCWrapper_emfx2Actor::SetBlendMode (Engine:0x10049d10): only mode 2 sets the
    /// mix flag.
    pub fn set_blend_mode(&mut self, mode: i32) {
        self.additive = mode == 2;
    }

    /// Port of eCWrapper_emfx2Actor::StopAtLoopEnd (Engine:0x100479c0): a running instance gets
    /// its maximum loops set to the current loop plus one.
    pub fn stop_at_loop_end(&mut self) {
        if self.is_playing() {
            self.max_loops = self.loop_index + 1;
        }
    }

    /// Port of eCWrapper_emfx2Actor::SwitchFadeOut (Engine:0x10049dd0) applied to the instance
    /// that was moved out of its slot. Returns false (instance untouched) when it has already
    /// run out of loops or is paused.
    ///
    /// The instance gets [`FADING_PRIORITY`] (so it sorts above the clip that replaces it),
    /// target weight 0 and a linear ramp over `seconds` (instant for `seconds <= 0`). A finite
    /// clip whose remaining play time is shorter than the fade is slowed so that it ends with
    /// the fade.
    pub fn switch_fade_out(&mut self, seconds: f32) -> bool {
        if !self.is_playing() {
            return false;
        }
        self.priority = FADING_PRIORITY;
        self.target_weight = 0.0;
        if seconds <= 0.0 {
            self.weight = 0.0;
            self.weight_rate = 0.0;
            self.ramping = false;
            self.expired = true;
        } else {
            self.ramping = true;
            self.weight_rate = -self.weight / seconds;
        }
        if self.max_loops != -1 {
            let mut left = if self.mode == PlayMode::Backward {
                self.time
            } else {
                self.duration - self.time
            };
            if self.speed != 0.0 {
                left /= self.speed;
            }
            if left.abs() < seconds.abs() {
                self.speed = left.abs() / seconds.abs();
            }
        }
        true
    }

    /// Port of eCWrapper_emfx2Actor::RunStopPrevention (Engine:0x10047600). Applies to the
    /// primary slot: when its final loop would end within this frame, the clock is parked just
    /// before the end (just after the start for backward play) and the speed becomes 0, so the
    /// last pose is held instead of wrapping or popping.
    pub fn run_stop_prevention(&mut self, dt: f32) {
        if !self.is_playing()
            || self.speed == 0.0
            || self.max_loops == -1
            || self.max_loops != self.loop_index + 1
        {
            return;
        }
        let left = if self.mode == PlayMode::Backward {
            self.time
        } else {
            self.duration - self.time
        };
        if (left / self.speed).abs() < dt.abs() {
            self.speed = 0.0;
            self.time = if self.mode == PlayMode::Backward {
                END_EPSILON
            } else {
                self.duration - END_EPSILON
            };
        }
    }

    /// Advances the weight ramp and the clock by `dt` seconds (already scaled by the entity's
    /// time scale). Loops are counted per pass; a finished instance clamps to its last pose.
    pub fn advance(&mut self, dt: f32) {
        if self.ramping {
            self.weight += self.weight_rate * dt;
            let done = if self.weight_rate < 0.0 {
                self.weight <= self.target_weight
            } else {
                self.weight >= self.target_weight
            };
            if done {
                self.weight = self.target_weight;
                self.ramping = false;
                self.weight_rate = 0.0;
                if self.target_weight <= 0.0 {
                    self.expired = true;
                }
            }
        }
        if self.paused || self.ended || self.duration <= 1e-6 {
            return;
        }
        let step = dt * self.speed;
        match self.mode {
            PlayMode::Forward => self.step_forward(step),
            PlayMode::Backward => self.step_backward(step),
            PlayMode::PingPong => {
                if self.reversed {
                    self.step_backward(step);
                } else {
                    self.step_forward(step);
                }
            }
        }
    }

    fn pass_done(&mut self) -> bool {
        self.loop_index += 1;
        self.max_loops != -1 && self.loop_index >= self.max_loops
    }

    fn step_forward(&mut self, step: f32) {
        self.time += step;
        while self.time >= self.duration {
            if self.pass_done() {
                self.time = self.duration;
                self.ended = true;
                return;
            }
            if self.mode == PlayMode::PingPong {
                self.time = 2.0 * self.duration - self.time;
                self.reversed = true;
                return;
            }
            self.time -= self.duration;
        }
    }

    fn step_backward(&mut self, step: f32) {
        self.time -= step;
        while self.time <= 0.0 {
            if self.pass_done() {
                self.time = 0.0;
                self.ended = true;
                return;
            }
            if self.mode == PlayMode::PingPong {
                self.time = -self.time;
                self.reversed = false;
                return;
            }
            self.time += self.duration;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Layer combination
// ---------------------------------------------------------------------------------------------

/// Inserts into a priority list the way the layer system does (Engine:0x102ed2b0): the list
/// runs from the top layer down by descending priority; a new instance goes in front of the
/// first entry whose priority is not higher, so among equals the newest is on top. Returns the
/// index (0 = top). Priorities are read through `priority_of`.
pub fn insertion_index<T>(list: &[T], priority: i32, priority_of: impl Fn(&T) -> i32) -> usize {
    list.iter()
        .position(|e| priority_of(e) <= priority)
        .unwrap_or(list.len())
}

/// Converts normalised blend fractions (bottom layer first, summing to 1) into the weights of
/// the layer stack, in which every layer is lerped over everything below it. The bottom layer
/// gets 1 (it is lerped over the bind pose).
pub fn stacked_weights(fractions: &[f32]) -> Vec<f32> {
    let mut sum = 0.0;
    fractions
        .iter()
        .map(|&f| {
            sum += f;
            if sum > 1e-6 {
                (f / sum).clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
        .collect()
}

/// Share each layer (bottom first) ends up contributing to the final pose, plus the share of
/// the bind pose as the last element. The shares sum to 1.
pub fn contributions(weights: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; weights.len() + 1];
    let mut above = 1.0;
    for i in (0..weights.len()).rev() {
        out[i] = weights[i] * above;
        above *= 1.0 - weights[i];
    }
    out[weights.len()] = above;
    out
}

// ---------------------------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------------------------

/// One call on the actor wrapper.
#[derive(Clone, Debug, PartialEq)]
pub enum MotionOp {
    /// `SwitchFadeOut(slot, seconds)`.
    SwitchFadeOut { slot: u8, seconds: f32 },
    /// `StopMotion(slot, fade)`.
    Stop { slot: u8, fade: f32 },
    /// `SetTargetWeight(slot, weight)`.
    SetTargetWeight { slot: u8, weight: f32 },
    /// `SetOwner(slot, owner)`.
    SetOwner { slot: u8, owner: u8 },
    /// `PlayMotion(slot, name, desc)`.
    Play {
        slot: u8,
        name: String,
        desc: PlayDesc,
    },
}

/// Who asks for the new primary clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requester {
    /// The idle mixer; `old_owner` is the slot-0 owner it found.
    IdleMixer { old_owner: u8 },
    /// Directional locomotion or turn-in-place mixer.
    Mixer,
    /// `sAIPlayAniStart`.
    Script,
}

/// Plans the calls that replace the primary clip `old` (if it is running) by `new`, in the
/// order the original issues them: fade the old instance out (SwitchFadeOut), stop and
/// zero-weight the mixer slots 0..3, then start the new clip at full weight in slot 0.
pub fn plan_transition(
    table: &AniBlendings,
    requester: Requester,
    old: Option<&str>,
    new: &str,
    desc: PlayDesc,
) -> Vec<MotionOp> {
    let mut ops = Vec::new();
    let fade = match requester {
        Requester::IdleMixer { old_owner } => mixer_fade(true, old_owner),
        Requester::Mixer => STANDARD_FADE,
        Requester::Script => old.map_or(STANDARD_FADE, |o| script_fade(table, o, new)),
    };
    if old.is_some() {
        ops.push(MotionOp::SwitchFadeOut {
            slot: slot::PRIMARY,
            seconds: fade,
        });
    }
    for s in slot::PRIMARY..=slot::FOURTH {
        ops.push(MotionOp::Stop { slot: s, fade: 0.0 });
        ops.push(MotionOp::SetTargetWeight {
            slot: s,
            weight: 0.0,
        });
    }
    ops.push(MotionOp::Play {
        slot: slot::PRIMARY,
        name: new.to_owned(),
        desc,
    });
    ops.push(MotionOp::SetOwner {
        slot: slot::PRIMARY,
        owner: match requester {
            Requester::IdleMixer { .. } => owner::IDLE,
            Requester::Mixer => owner::DIRECTIONAL,
            Requester::Script => owner::SCRIPT,
        },
    });
    ops
}

/// What the idle mixer does with slot 0 (FUN_100d9940, Game:0x100d9940).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleDecision {
    /// Leave the slot alone.
    Leave,
    /// Fade the old clip out and start a new idle clip.
    TakeOver,
    /// Give the slot back (owner becomes [`owner::FREE`]).
    Release,
}

/// Port of the owner switch of the idle mixer. `running` is `IsMotionRunning(0)` with a
/// non-zero play speed; `remaining` the time to the end of the clip and `duration` its length;
/// `items_changed` says the hand-item combination changed since the idle clip was chosen.
pub fn idle_mixer_decision(
    owner_value: u8,
    running: bool,
    remaining: f32,
    duration: f32,
    items_changed: bool,
) -> IdleDecision {
    match owner_value {
        2 if running => IdleDecision::Leave,
        2 | 3 | 10 | 11 => IdleDecision::TakeOver,
        4 | 5 | 7 | 8 => IdleDecision::Leave,
        6 if running => IdleDecision::Leave,
        6 => IdleDecision::Release,
        9 => {
            let near_end =
                running && remaining <= STANDARD_FADE && duration > 2.0 * STANDARD_FADE + 0.1;
            if !running || near_end || items_changed {
                IdleDecision::TakeOver
            } else {
                IdleDecision::Leave
            }
        }
        _ => IdleDecision::Leave,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "*Version 1\n*Entry-> Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_ 3 6\n\
        *Entry-> G3_Run_%_x 15 0\n";

    #[test]
    fn blend_table_bytes_are_fifteenths_of_a_second() {
        let t = AniBlendings::parse(TABLE);
        assert_eq!(t.len(), 2);
        let walk = "hero_stand_none_none_p0_move_walk_n_fwd_00_%_00_P0_160";
        assert!((t.intro(walk) - 0.2).abs() < 1e-6); // 3 / 15
        assert!((t.extro(walk) - 0.4).abs() < 1e-6); // 6 / 15
        assert!((t.intro("G3_Run_") - 1.0).abs() < 1e-6);
        assert_eq!(t.extro("G3_Run_"), 0.0);
        // Unknown clips use the standard fade; a text without the version marker is empty.
        assert_eq!(t.intro("Nope_"), STANDARD_FADE);
        assert!(AniBlendings::parse("*Entry-> a 1 1").is_empty());
    }

    #[test]
    fn variation_maximum_is_word_eleven() {
        let mut t = AniBlendings::default();
        t.register_variation("Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_03_P0_160");
        assert_eq!(
            t.variation_max("Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_"),
            Some(3)
        );
        let key = "Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_";
        assert_eq!(t.with_variation(key, 6), key.replace("_00_", "_02_"));
        assert_eq!(t.with_variation("Other_00_", 1), "Other_00_");
    }

    #[test]
    fn script_fade_rules() {
        let t = AniBlendings::parse(TABLE);
        let a = "Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_"; // intro 0.2, extro 0.4
        let b = "G3_Run_"; // intro 1.0
        assert_eq!(script_fade(&t, a, "X_Begin_Y"), BEGIN_INTRO_FADE);
        assert!((script_fade(&t, a, b) - 0.4).abs() < 1e-6);
        assert!((script_fade(&t, b, a) - 0.0).abs() < 1e-6);
        assert_eq!(mixer_fade(true, owner::DIRECTIONAL), 0.5);
        assert_eq!(mixer_fade(true, owner::IDLE), 0.2);
        assert_eq!(mixer_fade(false, owner::IDLE), 0.2);
    }

    #[test]
    fn fade_in_ramps_linearly_and_default_starts_full() {
        let full = Instance::new(2.0, &PlayDesc::default(), 0.0);
        assert_eq!(full.weight, 1.0);
        let mut i = Instance::new(
            2.0,
            &PlayDesc {
                fade_in: 0.1,
                ..PlayDesc::default()
            },
            0.0,
        );
        assert_eq!(i.weight, 0.0);
        i.advance(0.05);
        assert!((i.weight - 0.5).abs() < 1e-5);
        i.advance(0.1);
        assert_eq!(i.weight, 1.0);
        assert!(!i.ramping);
    }

    #[test]
    fn switch_fade_out_ramps_to_zero_and_expires() {
        let mut i = Instance::new(2.0, &PlayDesc::default(), 0.5);
        assert!(i.switch_fade_out(0.2));
        assert_eq!(i.priority, FADING_PRIORITY);
        assert_eq!(i.target_weight, 0.0);
        i.advance(0.1);
        assert!((i.weight - 0.5).abs() < 1e-5);
        assert!(!i.expired);
        i.advance(0.15);
        assert_eq!(i.weight, 0.0);
        assert!(i.expired);
        // Zero seconds drops the weight at once.
        let mut j = Instance::new(2.0, &PlayDesc::default(), 0.0);
        j.switch_fade_out(0.0);
        assert_eq!((j.weight, j.expired), (0.0, true));
    }

    #[test]
    fn short_remaining_clip_is_slowed_to_last_the_fade() {
        // Plays once, 0.1 s left, fade 0.2 s: speed becomes 0.1 / 0.2.
        let mut i = Instance::new(1.0, &PlayDesc::once(), 0.9);
        i.switch_fade_out(0.2);
        assert!((i.speed - 0.5).abs() < 1e-5, "{}", i.speed);
        // Endless loops are not slowed.
        let mut l = Instance::new(1.0, &PlayDesc::default(), 0.9);
        l.switch_fade_out(0.2);
        assert_eq!(l.speed, 1.0);
        // An instance that ran out of loops is not faded.
        let mut done = Instance::new(1.0, &PlayDesc::once(), 0.0);
        done.advance(2.0);
        assert!(!done.switch_fade_out(0.2));
    }

    #[test]
    fn stop_prevention_holds_the_last_pose() {
        let mut i = Instance::new(1.0, &PlayDesc::once(), 0.99);
        i.run_stop_prevention(1.0 / 60.0);
        assert_eq!(i.speed, 0.0);
        assert!((i.time - (1.0 - END_EPSILON)).abs() < 1e-6);
        assert!(i.at_end());
        i.advance(1.0);
        assert!((i.time - (1.0 - END_EPSILON)).abs() < 1e-6);
        // Loops forever: nothing to prevent. Early in the clip: nothing either.
        let mut l = Instance::new(1.0, &PlayDesc::default(), 0.99);
        l.run_stop_prevention(1.0);
        assert_eq!(l.speed, 1.0);
        let mut e = Instance::new(1.0, &PlayDesc::once(), 0.1);
        e.run_stop_prevention(1.0 / 60.0);
        assert_eq!(e.speed, 1.0);
    }

    #[test]
    fn loops_are_counted_and_stop_at_loop_end_finishes_the_cycle() {
        let mut i = Instance::new(1.0, &PlayDesc::default(), 0.0);
        i.advance(2.5);
        assert_eq!(i.loop_index, 2);
        assert!((i.time - 0.5).abs() < 1e-5);
        i.stop_at_loop_end();
        assert_eq!(i.max_loops, 3);
        i.advance(0.6);
        assert!(i.ended);
        assert!(!i.is_playing());
        // A paused instance ignores StopAtLoopEnd.
        let mut p = Instance::new(1.0, &PlayDesc::default(), 0.0);
        p.paused = true;
        p.stop_at_loop_end();
        assert_eq!(p.max_loops, -1);
    }

    #[test]
    fn backward_and_ping_pong_play() {
        let mut b = Instance::new(
            2.0,
            &PlayDesc {
                mode: PlayMode::Backward,
                ..PlayDesc::default()
            },
            0.0,
        );
        assert_eq!(b.time, 2.0);
        b.advance(0.5);
        assert!((b.time - 1.5).abs() < 1e-6);
        let mut p = Instance::new(
            1.0,
            &PlayDesc {
                mode: PlayMode::PingPong,
                ..PlayDesc::default()
            },
            0.0,
        );
        p.advance(1.25);
        assert!((p.time - 0.75).abs() < 1e-5, "{}", p.time);
        p.advance(0.5);
        assert!((p.time - 0.25).abs() < 1e-5);
    }

    #[test]
    fn target_weight_is_instant_and_blend_mode_two_is_additive() {
        let mut i = Instance::new(
            1.0,
            &PlayDesc {
                fade_in: 1.0,
                ..PlayDesc::default()
            },
            0.0,
        );
        i.set_target_weight(1.5);
        assert_eq!((i.weight, i.target_weight, i.ramping), (1.0, 1.0, false));
        i.set_blend_mode(2);
        assert!(i.additive);
        i.set_blend_mode(1);
        assert!(!i.additive);
    }

    #[test]
    fn new_instances_go_below_fading_ones_and_above_equals() {
        let mut list = vec![0, 0];
        let at = insertion_index(&list, 0, |&p| p);
        assert_eq!(at, 0, "newest of equal priority is on top");
        list.insert(0, FADING_PRIORITY);
        assert_eq!(insertion_index(&list, 0, |&p| p), 1, "below the fading one");
        assert_eq!(
            insertion_index(&[5, 5], 0, |&p| p),
            2,
            "append when all higher"
        );
    }

    #[test]
    fn stacked_weights_reproduce_normalised_fractions() {
        let fractions = [0.2, 0.3, 0.5];
        let w = stacked_weights(&fractions);
        assert_eq!(w[0], 1.0);
        let c = contributions(&w);
        for (got, want) in c.iter().zip(fractions) {
            assert!((got - want).abs() < 1e-5, "{c:?}");
        }
        assert!(c[3].abs() < 1e-6, "no bind pose left");
        assert!((c.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn crossfade_pose_old_fades_over_the_new_clip() {
        // New clip underneath at weight 1, old clip on top ramping 1 -> 0 over 0.2 s.
        let c = contributions(&[1.0, 0.5]);
        assert!((c[0] - 0.5).abs() < 1e-6 && (c[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn idle_takeover_plan_matches_the_original_call_order() {
        let t = AniBlendings::default();
        let ops = plan_transition(
            &t,
            Requester::IdleMixer {
                old_owner: owner::DIRECTIONAL,
            },
            Some("Walk_"),
            "Idle_",
            PlayDesc::default(),
        );
        assert_eq!(
            ops[0],
            MotionOp::SwitchFadeOut {
                slot: 0,
                seconds: 0.5
            }
        );
        assert_eq!(ops[1], MotionOp::Stop { slot: 0, fade: 0.0 });
        assert!(matches!(
            ops.last(),
            Some(MotionOp::SetOwner { owner: 9, .. })
        ));
        assert!(matches!(
            &ops[ops.len() - 2],
            MotionOp::Play { slot: 0, name, .. } if name == "Idle_"
        ));
        // Nothing to fade without an old clip.
        let first = plan_transition(&t, Requester::Mixer, None, "Walk_", PlayDesc::default());
        assert!(
            !first
                .iter()
                .any(|o| matches!(o, MotionOp::SwitchFadeOut { .. }))
        );
    }

    #[test]
    fn idle_mixer_owner_table() {
        use IdleDecision::*;
        assert_eq!(idle_mixer_decision(2, true, 5.0, 5.0, false), Leave);
        assert_eq!(idle_mixer_decision(2, false, 0.0, 5.0, false), TakeOver);
        assert_eq!(idle_mixer_decision(10, true, 5.0, 5.0, false), TakeOver);
        assert_eq!(idle_mixer_decision(4, false, 0.0, 5.0, false), Leave);
        assert_eq!(idle_mixer_decision(6, false, 0.0, 5.0, false), Release);
        // Idle re-rolls near the end of a long clip or when the items changed.
        assert_eq!(idle_mixer_decision(9, true, 0.1, 3.0, false), TakeOver);
        assert_eq!(idle_mixer_decision(9, true, 0.1, 0.4, false), Leave);
        assert_eq!(idle_mixer_decision(9, true, 2.0, 3.0, true), TakeOver);
        assert_eq!(idle_mixer_decision(9, true, 2.0, 3.0, false), Leave);
    }
}
