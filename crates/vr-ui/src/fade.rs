//! The screen fade of sleeping and long waits. Comfort: the world fades to black *before* time
//! jumps and back after, so the sky and shadows never snap in view (Meta: avoid sudden
//! full-field changes). Driven by the core's events: `PlayerEvent::Slept { hours }` starts the
//! fade, `TimeScale` changes are applied by the host while the screen is dark.
//!
//! Pure state: [`SleepFade::alpha`] is the black overlay opacity the renderer draws as a
//! ring/vignette around the eyes (or a full dark quad once it passes 0.95).

/// Seconds to fade out, stay dark and fade in.
pub const OUT: f32 = 1.0;
pub const HOLD: f32 = 0.8;
pub const IN: f32 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Phase {
    #[default]
    Awake,
    Out,
    Dark,
    In,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SleepFade {
    phase: Phase,
    t: f32,
    /// Hours the host must advance when the screen is dark (once).
    pending_hours: Option<f32>,
}

impl SleepFade {
    pub fn start(&mut self, hours: f32) {
        self.phase = Phase::Out;
        self.t = 0.0;
        self.pending_hours = Some(hours);
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn alpha(&self) -> f32 {
        match self.phase {
            Phase::Awake => 0.0,
            Phase::Out => (self.t / OUT).min(1.0),
            Phase::Dark => 1.0,
            Phase::In => (1.0 - self.t / IN).max(0.0),
        }
    }

    /// Advance; returns the hours to skip once, at the moment the screen is fully dark.
    pub fn tick(&mut self, dt: f32) -> Option<f32> {
        let mut skip = None;
        self.t += dt;
        match self.phase {
            Phase::Awake => {}
            Phase::Out if self.t >= OUT => {
                self.phase = Phase::Dark;
                self.t = 0.0;
                skip = self.pending_hours.take();
            }
            Phase::Dark if self.t >= HOLD => {
                self.phase = Phase::In;
                self.t = 0.0;
            }
            Phase::In if self.t >= IN => {
                self.phase = Phase::Awake;
                self.t = 0.0;
            }
            _ => {}
        }
        skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_out_skips_time_in_the_dark_and_fades_in() {
        let mut f = SleepFade::default();
        assert_eq!(f.alpha(), 0.0);
        f.start(8.0);
        assert!(f.tick(0.5).is_none());
        assert!((f.alpha() - 0.5).abs() < 1e-5);
        assert_eq!(f.tick(0.6), Some(8.0), "time passes only when dark");
        assert_eq!(f.alpha(), 1.0);
        assert!(f.tick(0.9).is_none());
        assert_eq!(f.phase(), Phase::In);
        f.tick(IN + 0.1);
        assert_eq!(f.phase(), Phase::Awake);
        assert_eq!(f.alpha(), 0.0);
    }
}
