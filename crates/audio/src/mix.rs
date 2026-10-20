//! The original mix model (FMOD Ex), measured by hooking `fmodex.dll` in the running game.
//!
//! Evidence and numbers: `docs/research/AUDIO_FORMATS.md`, section "Original mix". In short the
//! engine calls `System::init(64, ..)`, `set3DSettings(doppler 1, distancefactor 100, rolloff 1)`
//! (so every distance is in centimetres), creates the channel groups `Master > {Voice, Music, FX,
//! Ambient}`, and per sound `setPriority`, `set3DMinMaxDistance`, `setVolume`. Rolloff is FMOD's
//! default *inverse* curve; voices use min 500 cm / max 1 000 000 cm, priority 1.
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Mutex;

/// `System::init(maxchannels = 64)`.
pub const MAX_CHANNELS: usize = 64;

/// FMOD channel group a sound belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Voice,
    Music,
    Fx,
    Ambient,
}

impl Category {
    /// FMOD `Channel::setPriority` seen in the capture (0 = most important). Music is not
    /// 3D and never competes, it gets 0.
    pub fn priority(self) -> u8 {
        match self {
            Self::Music => 0,
            Self::Voice => 1,
            Self::Fx => 3,
            Self::Ambient => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::Music => "music",
            Self::Fx => "fx",
            Self::Ambient => "ambient",
        }
    }
}

/// FMOD Ex `FMOD_3D_INVERSEROLLOFF` (the default when a sound sets no rolloff mode): full volume
/// inside `min`, then `min / (min + rolloff * (d - min))`; beyond `max` the attenuation stops
/// (it does not cut off). Distances in any one unit.
pub fn inverse_gain(dist: f32, min: f32, max: f32, rolloff: f32) -> f32 {
    let min = min.max(1e-4);
    let d = dist.min(max.max(min));
    if d <= min {
        1.0
    } else {
        min / (min + rolloff * (d - min))
    }
}

/// FMOD `FMOD_3D_LINEARROLLOFF`: 1 at `min`, 0 at `max` (used by a few ambient loops).
pub fn linear_gain(dist: f32, min: f32, max: f32) -> f32 {
    if dist <= min {
        1.0
    } else if dist >= max || max <= min {
        0.0
    } else {
        (max - dist) / (max - min)
    }
}

/// One playing channel as seen by the voice limiter.
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub priority: u8,
    /// Linear loudness (volume x rolloff); the quietest of equal priority is stolen first.
    pub audibility: f32,
}

/// With `limit` channels in use, which one makes room for a new sound? `None` = the new sound
/// is the least important and must be dropped (FMOD would virtualise it).
pub fn steal_victim(active: &[Slot], new: Slot, limit: usize) -> Option<Option<usize>> {
    if active.len() < limit {
        return Some(None);
    }
    let worst = active
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            (a.priority, -a.audibility)
                .partial_cmp(&(b.priority, -b.audibility))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, s)| (i, *s))?;
    let (i, w) = worst;
    let new_better =
        new.priority < w.priority || (new.priority == w.priority && new.audibility > w.audibility);
    new_better.then_some(Some(i))
}

/// Debug tap (`GR_AUDIO_TAP=path`): one TSV line per audio event, for offline mixing and for
/// comparing against the original capture. Columns: `t kind id name gain pan extra`.
pub struct Tap(Mutex<std::io::BufWriter<std::fs::File>>);

impl Tap {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("GR_AUDIO_TAP")?;
        let f = std::fs::File::create(path).ok()?;
        Some(Self(Mutex::new(std::io::BufWriter::new(f))))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn log(&self, t: f32, kind: &str, id: u64, name: &str, gain: f32, pan: f32, extra: &str) {
        let mut l = String::new();
        let _ = writeln!(
            l,
            "{t:.3}\t{kind}\t{id}\t{name}\t{gain:.4}\t{pan:.3}\t{extra}"
        );
        if let Ok(mut w) = self.0.lock() {
            let _ = w.write_all(l.as_bytes());
            let _ = w.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_matches_fmod_curve() {
        // voices: min 500 cm, max 1e6 cm, rolloff 1
        assert_eq!(inverse_gain(300.0, 500.0, 1e6, 1.0), 1.0);
        assert!((inverse_gain(1000.0, 500.0, 1e6, 1.0) - 500.0 / 1000.0).abs() < 1e-6);
        assert!((inverse_gain(2500.0, 500.0, 1e6, 1.0) - 0.2).abs() < 1e-6);
        // beyond max the gain stays at the max-distance value
        let g = inverse_gain(5e6, 500.0, 1e6, 1.0);
        assert!((g - 500.0 / (500.0 + 1e6 - 500.0)).abs() < 1e-9);
        // footsteps: min 150 cm
        assert!((inverse_gain(300.0, 150.0, 1e12, 1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn linear_rolloff() {
        assert_eq!(linear_gain(10.0, 25.0, 25000.0), 1.0);
        assert!((linear_gain(12512.5, 25.0, 25000.0) - 0.5).abs() < 1e-4);
        assert_eq!(linear_gain(30000.0, 25.0, 25000.0), 0.0);
    }

    #[test]
    fn limiter_steals_lowest_priority_quietest() {
        let s = |p, a| Slot {
            priority: p,
            audibility: a,
        };
        let active = [s(1, 1.0), s(4, 0.5), s(4, 0.1), s(3, 0.9)];
        assert_eq!(steal_victim(&active, s(3, 1.0), 8), Some(None));
        // full: ambient (4) quietest goes first
        assert_eq!(steal_victim(&active, s(3, 0.2), 4), Some(Some(2)));
        // a new ambient quieter than everything is dropped
        assert_eq!(steal_victim(&active, s(4, 0.05), 4), None);
        // a voice always makes room
        assert_eq!(steal_victim(&active, s(1, 0.3), 4), Some(Some(2)));
    }
}
