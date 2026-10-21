//! One enhanced voice: mono source in, stereo out. Distance gain (from the faithful FMOD
//! rolloff, computed by the host) -> occlusion low-pass and gain -> HRTF (or plain equal-power
//! pan when HRTF is off) + reverb send. Parameters are set per block and smoothed per sample.
use crate::dsp::{OnePole, Smoothed, equal_power};
use crate::hrtf::{Direction, Hrtf};
use crate::occlusion::{Blocking, OcclusionTuning};
use crate::reverb::{Reverb, ReverbParams};
use serde::{Deserialize, Serialize};

/// The opt-in switches. `enabled: false` is the faithful mix (equal-power pan, no filters).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnhancedSettings {
    pub enabled: bool,
    pub hrtf: bool,
    pub occlusion: bool,
    pub reverb: bool,
    pub occlusion_tuning: OcclusionTuning,
}

impl Default for EnhancedSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            hrtf: true,
            occlusion: true,
            reverb: true,
            occlusion_tuning: OcclusionTuning::default(),
        }
    }
}

impl EnhancedSettings {
    pub fn all_on() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// Per-block inputs of a voice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceParams {
    /// Source direction in listener space (x right, y up, z forward; any length).
    pub direction: [f32; 3],
    /// Distance attenuation (0..1), e.g. `gothic_audio::mix::inverse_gain`.
    pub distance_gain: f32,
    pub blocking: Blocking,
    pub reverb: ReverbParams,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            direction: [0.0, 0.0, 1.0],
            distance_gain: 1.0,
            blocking: Blocking::default(),
            reverb: ReverbParams::NONE,
        }
    }
}

pub struct EnhancedVoice {
    sample_rate: f32,
    hrtf: Hrtf,
    lowpass: OnePole,
    reverb: Reverb,
    gain: Smoothed,
    send: Smoothed,
    pan_l: Smoothed,
    pan_r: Smoothed,
    cutoff: f32,
}

impl EnhancedVoice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            hrtf: Hrtf::new(sample_rate),
            lowpass: OnePole::new(20_000.0, sample_rate),
            reverb: Reverb::new(sample_rate),
            gain: Smoothed::new(1.0, 0.02, sample_rate),
            send: Smoothed::new(0.0, 0.05, sample_rate),
            pan_l: Smoothed::new(1.0, 0.02, sample_rate),
            pan_r: Smoothed::new(1.0, 0.02, sample_rate),
            cutoff: 20_000.0,
        }
    }

    /// Process `input` (mono) into `out` (interleaved stereo, `2 * input.len()`).
    pub fn process(
        &mut self,
        input: &[f32],
        out: &mut [f32],
        params: &VoiceParams,
        settings: &EnhancedSettings,
    ) {
        assert_eq!(out.len(), input.len() * 2, "stereo output");
        let dir = Direction::new(
            params.direction[0],
            params.direction[1],
            params.direction[2],
        );
        let on = settings.enabled;
        let (block_gain, cutoff, send) = if on && settings.occlusion {
            settings.occlusion_tuning.apply(params.blocking)
        } else {
            (1.0, 20_000.0, 1.0)
        };
        if (cutoff - self.cutoff).abs() > 1.0 {
            self.cutoff = cutoff;
            self.lowpass.set_cutoff(cutoff, self.sample_rate);
        }
        let hrtf = on && settings.hrtf;
        let reverb = on && settings.reverb;
        if hrtf {
            self.hrtf.set_direction(dir);
        }
        let (pl, pr) = equal_power(dir.x);
        self.reverb.set_params(if reverb {
            params.reverb
        } else {
            ReverbParams::NONE
        });
        let target_gain = params.distance_gain.max(0.0) * block_gain;
        let target_send = if reverb { send } else { 0.0 };
        for (i, &x) in input.iter().enumerate() {
            let g = self.gain.next(target_gain);
            let s = self.send.next(target_send);
            // The filter keeps running so switching it in is click-free; fully open it is
            // bypassed (the faithful path stays bit-exact).
            let lp = self.lowpass.process(x);
            let dry = if self.cutoff >= 19_999.0 { x } else { lp } * g;
            let (l, r) = if hrtf {
                self.hrtf.process(dry)
            } else {
                (dry * self.pan_l.next(pl), dry * self.pan_r.next(pr))
            };
            // The reverb hears the unfiltered source (the room is excited even when the direct
            // path is obstructed), scaled by distance and the occlusion send.
            let (wl, wr) = if reverb {
                self.reverb.process(x * params.distance_gain * s)
            } else {
                (0.0, 0.0)
            };
            out[2 * i] = l + wl;
            out[2 * i + 1] = r + wr;
        }
    }
}
