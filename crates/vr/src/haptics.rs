//! Controller haptics: gameplay systems send [`HapticPulse`]s (grab, hit, clash, bow), the
//! [`HapticMixer`] merges them per hand and frame, and the backend plays the result (OpenXR
//! `xrApplyHapticFeedback`; the desktop fallback only records it).
//!
//! Merging: a stronger pulse replaces a weaker one that is still playing, a weaker one is
//! dropped while a stronger one plays (so a burst of contacts does not flatten into noise), and
//! repeated identical ticks (the bow string creaking) are rate-limited.
use crate::input::Hand;
use serde::{Deserialize, Serialize};

/// One vibration on one controller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HapticPulse {
    pub hand: Hand,
    /// 0..1.
    pub amplitude: f32,
    /// Seconds.
    pub duration: f32,
    /// Hz; 0 lets the runtime choose.
    pub frequency: f32,
}

impl HapticPulse {
    pub fn new(hand: Hand, amplitude: f32, duration: f32, frequency: f32) -> Self {
        Self {
            hand,
            amplitude: amplitude.clamp(0.0, 1.0),
            duration: duration.clamp(0.0, 2.0),
            frequency: frequency.max(0.0),
        }
    }

    /// A hand closes on an object: a short, light click.
    pub fn grab(hand: Hand) -> Self {
        Self::new(hand, 0.35, 0.03, 0.0)
    }

    /// A weapon lands on a body; stronger and longer with impact speed (m/s) and damage scale
    /// (0..1, e.g. relative damage).
    pub fn hit(hand: Hand, speed: f32, severity: f32) -> Self {
        let s = (speed / 8.0).clamp(0.0, 1.0);
        Self::new(
            hand,
            0.35 + 0.65 * s.max(severity.clamp(0.0, 1.0)),
            0.05 + 0.07 * s,
            0.0,
        )
    }

    /// Blade on blade (block, parry, clash): sharp and high.
    pub fn clash(hand: Hand, speed: f32) -> Self {
        let s = (speed / 6.0).clamp(0.0, 1.0);
        Self::new(hand, 0.6 + 0.4 * s, 0.06, 320.0)
    }

    /// A weapon got stuck in a body: a dull, longer thud (strong enough to take over from the
    /// hit pulse of the same stab).
    pub fn stuck(hand: Hand) -> Self {
        Self::new(hand, 0.9, 0.12, 80.0)
    }

    /// Pulling a stuck weapon out.
    pub fn pulled_out(hand: Hand) -> Self {
        Self::new(hand, 0.45, 0.05, 120.0)
    }

    /// An arrow clicks onto the string.
    pub fn nock(hand: Hand) -> Self {
        Self::new(hand, 0.4, 0.02, 0.0)
    }

    /// Creak of a drawn string: one tick per [`DRAW_TICK`] of draw, stronger near full draw.
    pub fn draw_tick(hand: Hand, draw: f32) -> Self {
        Self::new(hand, 0.08 + 0.3 * draw.clamp(0.0, 1.0), 0.01, 0.0)
    }

    /// The string snaps forward: a strong kick on the string hand, a lighter one on the bow.
    pub fn release(hand: Hand, draw: f32, string_hand: bool) -> Self {
        let a = if string_hand { 0.6 } else { 0.35 };
        Self::new(hand, a + 0.4 * draw.clamp(0.0, 1.0), 0.06, 0.0)
    }
}

/// Draw distance between two string creaks (fraction of a full draw).
pub const DRAW_TICK: f32 = 0.1;

/// Minimum interval between two pulses of near-equal strength on one hand, seconds.
pub const MIN_INTERVAL: f32 = 0.025;

#[derive(Debug, Clone, Copy, Default)]
struct Playing {
    amplitude: f32,
    until: f32,
    last_start: f32,
}

/// Per-hand merge of the pulses of a frame (see the module docs).
#[derive(Debug, Clone, Default)]
pub struct HapticMixer {
    playing: [Playing; 2],
    pending: [Option<HapticPulse>; 2],
}

impl HapticMixer {
    /// Offer a pulse at time `now` (seconds).
    pub fn push(&mut self, pulse: HapticPulse, now: f32) {
        if pulse.amplitude <= 0.0 || pulse.duration <= 0.0 {
            return;
        }
        let i = pulse.hand.index();
        let p = &self.playing[i];
        let active = now < p.until;
        if active && pulse.amplitude < p.amplitude * 0.9 {
            return;
        }
        if now - p.last_start < MIN_INTERVAL && pulse.amplitude <= p.amplitude * 1.1 {
            return;
        }
        match &mut self.pending[i] {
            Some(q) if q.amplitude >= pulse.amplitude => {}
            slot => *slot = Some(pulse),
        }
    }

    /// The pulses to start this frame (at most one per hand); marks them as playing.
    pub fn take(&mut self, now: f32) -> Vec<HapticPulse> {
        let mut out = Vec::new();
        for i in 0..2 {
            if let Some(p) = self.pending[i].take() {
                self.playing[i] = Playing {
                    amplitude: p.amplitude,
                    until: now + p.duration,
                    last_start: now,
                };
                out.push(p);
            }
        }
        out
    }
}

/// Draw ticks: emits a [`HapticPulse::draw_tick`] each time the draw crosses another
/// [`DRAW_TICK`] step (outwards only), and resets when the string relaxes.
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawTicker {
    step: i32,
}

impl DrawTicker {
    pub fn update(&mut self, hand: Hand, draw: f32) -> Option<HapticPulse> {
        let step = (draw.clamp(0.0, 1.0) / DRAW_TICK).floor() as i32;
        if step > self.step {
            self.step = step;
            return Some(HapticPulse::draw_tick(hand, draw));
        }
        if step < self.step - 1 || draw <= 0.0 {
            self.step = step;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronger_pulse_wins_and_weak_ones_wait() {
        let mut m = HapticMixer::default();
        m.push(HapticPulse::hit(Hand::Right, 2.0, 0.0), 0.0);
        m.push(HapticPulse::hit(Hand::Right, 8.0, 0.0), 0.0);
        m.push(HapticPulse::grab(Hand::Left), 0.0);
        let out = m.take(0.0);
        assert_eq!(out.len(), 2);
        let right = out.iter().find(|p| p.hand == Hand::Right).unwrap();
        assert!(right.amplitude > 0.95);
        // While the strong hit plays, a light touch is dropped ...
        m.push(HapticPulse::grab(Hand::Right), 0.05);
        assert!(m.take(0.05).is_empty());
        // ... a clash on top is not.
        m.push(HapticPulse::clash(Hand::Right, 6.0), 0.06);
        assert_eq!(m.take(0.06).len(), 1);
        // Equal pulses are rate-limited.
        m.push(HapticPulse::clash(Hand::Right, 6.0), 0.07);
        assert!(m.take(0.07).is_empty());
        m.push(HapticPulse::clash(Hand::Right, 6.0), 0.2);
        assert_eq!(m.take(0.2).len(), 1);
    }

    #[test]
    fn hits_scale_with_speed() {
        let slow = HapticPulse::hit(Hand::Left, 1.0, 0.0);
        let fast = HapticPulse::hit(Hand::Left, 7.0, 0.0);
        assert!(fast.amplitude > slow.amplitude && fast.duration > slow.duration);
        assert!(fast.amplitude <= 1.0);
    }

    #[test]
    fn draw_ticks_once_per_step() {
        let mut t = DrawTicker::default();
        let ticks = (0..=100)
            .filter_map(|i| t.update(Hand::Right, i as f32 / 100.0))
            .count();
        assert_eq!(ticks, 10);
        // Holding still or easing back a little does not tick again.
        assert!(t.update(Hand::Right, 0.97).is_none());
        // Letting go and drawing again ticks again.
        assert!(t.update(Hand::Right, 0.0).is_none());
        assert!(t.update(Hand::Right, 0.15).is_some());
    }
}
