//! World sound emitters: the `eCAudioEmitter_PS` property set and its per-frame state machine
//! (ENGINE_MAP C7, batch E35), plus the effect command `PlaySound` selection rules (C9, E37).
use crate::rng::Rng;
use crate::sfx::EmitterDef;

/// `eEAudioEmitterMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawningMode {
    /// Plays once.
    Once,
    /// Plays a looping sound.
    Loop,
    /// Plays again after a random pause, with a repeat probability.
    Repeat,
}

impl SpawningMode {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Once,
            1 => Self::Loop,
            _ => Self::Repeat,
        }
    }
}

/// The properties of `eCAudioEmitter_PS` with the defaults of `ApplyDefaults`
/// (Engine:0x10058140).
#[derive(Debug, Clone, PartialEq)]
pub struct EmitterConfig {
    pub sample: String,
    pub enabled: bool,
    pub auto_reset: bool,
    pub auto_kill: bool,
    pub use_stream: bool,
    pub volume: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub mode: SpawningMode,
    pub seconds_between_repeats: [f32; 2],
    pub repeat_probability: [f32; 2],
    pub use_max_repeats: bool,
    pub max_repeats: [f32; 2],
}

impl Default for EmitterConfig {
    fn default() -> Self {
        Self {
            sample: String::new(),
            enabled: true,
            auto_reset: true,
            auto_kill: false,
            use_stream: false,
            volume: 1.0,
            min_distance: 1000.0,
            max_distance: 1_000_000.0,
            mode: SpawningMode::Repeat,
            seconds_between_repeats: [1.0, 5.0],
            repeat_probability: [1.0, 1.0],
            use_max_repeats: false,
            max_repeats: [0.0, 5.0],
        }
    }
}

impl EmitterConfig {
    /// Config of a template definition; values the template does not set keep their defaults.
    pub fn from_def(def: &EmitterDef) -> Self {
        let d = Self::default();
        Self {
            sample: def.sample.clone(),
            volume: def.volume,
            min_distance: if def.min_distance > 0.0 {
                def.min_distance
            } else {
                d.min_distance
            },
            max_distance: if def.max_distance > 0.0 {
                def.max_distance
            } else {
                d.max_distance
            },
            use_stream: def.use_stream,
            mode: SpawningMode::from_index(def.spawning_mode),
            seconds_between_repeats: if def.seconds_between_repeats == [0.0, 0.0] {
                d.seconds_between_repeats
            } else {
                def.seconds_between_repeats
            },
            ..d
        }
    }
}

/// What the emitter wants the host to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitterAction {
    None,
    /// Start the sample (`looped`: loop count -1).
    Spawn {
        looped: bool,
    },
    /// Remove the emitter's entity (auto kill).
    Kill,
}

#[derive(Debug, Clone, Default)]
pub struct EmitterState {
    timer: Option<f32>,
    spawned: u32,
    max_repeats: Option<u32>,
}

impl EmitterState {
    pub fn spawned(&self) -> u32 {
        self.spawned
    }

    /// Port of eCAudioEmitter_PS::OnProcess (Engine:0x10057ef0), decision part. `playing` tells
    /// whether the emitter's channel still sounds; nothing new starts before it stopped.
    pub fn step(
        &mut self,
        cfg: &EmitterConfig,
        dt: f32,
        playing: bool,
        rng: &mut Rng,
    ) -> EmitterAction {
        if !cfg.enabled || playing {
            return EmitterAction::None;
        }
        match cfg.mode {
            SpawningMode::Once => {
                if self.spawned == 0 {
                    self.spawned += 1;
                    EmitterAction::Spawn { looped: false }
                } else if cfg.auto_kill {
                    EmitterAction::Kill
                } else {
                    if cfg.auto_reset {
                        *self = Self::default();
                    }
                    EmitterAction::None
                }
            }
            SpawningMode::Loop => {
                self.spawned += 1;
                EmitterAction::Spawn { looped: true }
            }
            SpawningMode::Repeat => self.step_repeat(cfg, dt, rng),
        }
    }

    fn step_repeat(&mut self, cfg: &EmitterConfig, dt: f32, rng: &mut Rng) -> EmitterAction {
        let limit = *self.max_repeats.get_or_insert_with(|| {
            rng.range(cfg.max_repeats[0], cfg.max_repeats[1])
                .round()
                .max(0.0) as u32
        });
        if cfg.use_max_repeats && self.spawned >= limit {
            return if cfg.auto_kill {
                EmitterAction::Kill
            } else {
                if cfg.auto_reset {
                    *self = Self::default();
                }
                EmitterAction::None
            };
        }
        let pause = cfg.seconds_between_repeats;
        let timer = self
            .timer
            .get_or_insert_with(|| rng.range(pause[0], pause[1]));
        *timer -= dt;
        if *timer > 0.0 {
            return EmitterAction::None;
        }
        // The pause is over: roll the repeat probability, then re-arm the timer either way.
        let p = rng.range(cfg.repeat_probability[0], cfg.repeat_probability[1]);
        self.timer = Some(rng.range(pause[0], pause[1]));
        if rng.unit() <= p {
            self.spawned += 1;
            EmitterAction::Spawn { looped: false }
        } else {
            EmitterAction::None
        }
    }
}

/// Port of eCResourceSoundList::SelectSample (Engine:0x101bbb20) and
/// gCEffectCommandPlaySound::SelectSample (Game:0x10057f40): entry `i` is drawn with
/// probability `w_i / (N * max_w)`, so the remainder of the unit interval is "no sound"
/// (`None`).
pub fn select_sample(weights: &[f32], rng: &mut Rng) -> Option<usize> {
    let max = weights.iter().copied().fold(0.0f32, f32::max);
    if weights.is_empty() || max <= 0.0 {
        return None;
    }
    let roll = rng.unit();
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += w / (weights.len() as f32 * max);
        if roll < acc {
            return Some(i);
        }
    }
    None
}

/// Port of gCEffectCommandPlayVoice::Execute path building (Game:0x10059030): the literal token
/// `(voice)` is replaced by the speaker's `Voice` string and the whole path is lower-cased.
pub fn voice_path(template: &str, voice: &str) -> String {
    template.replace("(voice)", voice).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cfg: &EmitterConfig, secs: f32, sound_secs: f32, seed: u64) -> (u32, u32) {
        let mut st = EmitterState::default();
        let mut rng = Rng::new(seed);
        let (mut t, mut until, mut spawns, mut kills) = (0.0f32, 0.0f32, 0, 0);
        while t < secs {
            match st.step(cfg, 0.05, t < until, &mut rng) {
                EmitterAction::Spawn { looped } => {
                    spawns += 1;
                    until = if looped {
                        f32::INFINITY
                    } else {
                        t + sound_secs
                    };
                }
                EmitterAction::Kill => {
                    kills += 1;
                    break;
                }
                EmitterAction::None => {}
            }
            t += 0.05;
        }
        (spawns, kills)
    }

    #[test]
    fn defaults_follow_apply_defaults() {
        let d = EmitterConfig::default();
        assert_eq!(d.min_distance, 1000.0);
        assert_eq!(d.max_distance, 1_000_000.0);
        assert_eq!(d.mode, SpawningMode::Repeat);
        assert_eq!(d.seconds_between_repeats, [1.0, 5.0]);
        assert!(d.enabled && d.auto_reset && !d.auto_kill && !d.use_max_repeats);
    }

    #[test]
    fn loop_starts_once_and_keeps_playing() {
        let cfg = EmitterConfig {
            mode: SpawningMode::Loop,
            ..EmitterConfig::default()
        };
        assert_eq!(run(&cfg, 30.0, 1.0, 1), (1, 0));
    }

    #[test]
    fn once_with_auto_kill_removes_the_entity() {
        let cfg = EmitterConfig {
            mode: SpawningMode::Once,
            auto_kill: true,
            ..EmitterConfig::default()
        };
        assert_eq!(run(&cfg, 30.0, 1.0, 1), (1, 1));
    }

    #[test]
    fn repeat_spawns_after_random_pauses() {
        let cfg = EmitterConfig::default(); // 1..5 s pauses, probability 1
        let (spawns, kills) = run(&cfg, 300.0, 0.5, 4);
        // mean cycle = 3 s pause + 0.5 s sound
        assert!((60..=110).contains(&spawns) && kills == 0, "{spawns}");
    }

    #[test]
    fn repeat_probability_thins_the_spawns_and_max_repeats_ends_them() {
        let thin = EmitterConfig {
            repeat_probability: [0.25, 0.25],
            seconds_between_repeats: [1.0, 1.0],
            ..EmitterConfig::default()
        };
        let (spawns, _) = run(&thin, 400.0, 0.1, 2);
        assert!((70..=130).contains(&spawns), "{spawns}");
        let limited = EmitterConfig {
            use_max_repeats: true,
            max_repeats: [3.0, 3.0],
            auto_kill: true,
            seconds_between_repeats: [1.0, 1.0],
            ..EmitterConfig::default()
        };
        assert_eq!(run(&limited, 100.0, 0.1, 2), (3, 1));
    }

    #[test]
    fn disabled_emitter_is_silent() {
        let cfg = EmitterConfig {
            enabled: false,
            ..EmitterConfig::default()
        };
        assert_eq!(run(&cfg, 60.0, 1.0, 1), (0, 0));
    }

    #[test]
    fn sample_selection_has_a_silent_remainder() {
        // weights 1 and 0.5: P = 1/(2*1) = 0.5 and 0.25; the rest (0.25) is silence.
        let mut rng = Rng::new(11);
        let mut hits = [0u32; 3];
        for _ in 0..20_000 {
            match select_sample(&[1.0, 0.5], &mut rng) {
                Some(i) => hits[i] += 1,
                None => hits[2] += 1,
            }
        }
        let f = |n: u32| n as f32 / 20_000.0;
        assert!((f(hits[0]) - 0.5).abs() < 0.02, "{hits:?}");
        assert!((f(hits[1]) - 0.25).abs() < 0.02);
        assert!((f(hits[2]) - 0.25).abs() < 0.02);
        // equal weights never go silent
        assert!((0..1000).all(|_| select_sample(&[2.0, 2.0], &mut rng).is_some()));
        assert_eq!(select_sample(&[], &mut rng), None);
    }

    #[test]
    fn voice_token_is_replaced_and_lowercased() {
        assert_eq!(
            voice_path("Sounds/(voice).wav", "Hum_Fat_Bearish"),
            "sounds/hum_fat_bearish.wav"
        );
    }
}
