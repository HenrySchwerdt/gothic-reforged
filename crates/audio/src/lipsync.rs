//! Lip-sync data: the `.xlip` file next to every voice recording and the phoneme table
//! (ENGINE_MAP B14, batch E38).
//!
//! `eCVisualAnimationLipSync::Read` (Engine:0x10027690): `u16 version` (1), `u32 count`, then
//! `count` pairs `{f32 seconds, u32 viseme}`. The viseme indexes the 14 key positions of the
//! head's `_LipSync` source motion (see `gothic_animation::face`).
use anyhow::{Result, bail, ensure};

/// Number of visemes (key positions of the source motion).
pub const VISEMES: usize = 14;
/// The rest viseme (`x`, also the answer for unknown phoneme labels).
pub const REST: u32 = 1;

const MAX_FRAMES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub time: f32,
    pub viseme: u32,
}

/// A decoded `.xlip` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LipSync {
    pub frames: Vec<Frame>,
}

impl LipSync {
    /// Reads both framings found in the archives: the bare layout of the German files and the
    /// `GENOMFLE`-wrapped one of the English files (same `u16 1, u32 count` header after the
    /// 14-byte container header, then the same pairs and a `DEADBEEF` trailer).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let body = if bytes.starts_with(b"GENOMFLE") {
            ensure!(bytes.len() >= 14, "xlip too short");
            &bytes[14..]
        } else {
            bytes
        };
        ensure!(body.len() >= 6, "xlip too short");
        let version = u16::from_le_bytes([body[0], body[1]]);
        if version != 1 {
            bail!("unsupported xlip version {version}");
        }
        let n = u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
        ensure!(n <= MAX_FRAMES, "xlip frame count {n} too large");
        ensure!(body.len() >= 6 + 8 * n, "xlip truncated");
        let frames = body[6..6 + 8 * n]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| Frame {
                time: f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                viseme: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            })
            .collect::<Vec<_>>();
        ensure!(
            frames.iter().all(|f| f.time.is_finite() && f.time >= 0.0),
            "xlip has an invalid time"
        );
        ensure!(
            frames.windows(2).all(|w| w[0].time <= w[1].time),
            "xlip times are not sorted"
        );
        Ok(Self { frames })
    }

    /// Time of the last frame, seconds.
    pub fn duration(&self) -> f32 {
        self.frames.last().map_or(0.0, |f| f.time)
    }

    /// Viseme weights at `t`: the two neighbouring frames and the blend factor between them.
    /// Before the first frame the first viseme holds, after the last the last one (the
    /// facial motion is clamped the same way).
    pub fn sample(&self, t: f32) -> Option<(u32, u32, f32)> {
        let first = self.frames.first()?;
        let last = self.frames.last()?;
        if t <= first.time {
            return Some((first.viseme, first.viseme, 0.0));
        }
        if t >= last.time {
            return Some((last.viseme, last.viseme, 0.0));
        }
        let i = self.frames.partition_point(|f| f.time <= t) - 1;
        let (a, b) = (self.frames[i], self.frames[i + 1]);
        let span = (b.time - a.time).max(1e-6);
        Some((a.viseme, b.viseme, ((t - a.time) / span).clamp(0.0, 1.0)))
    }

    /// `count` moments (seconds into the line) with different visemes, at least 0.3 s apart and
    /// after the first 0.4 s: frames for checking that the mouth moves.
    pub fn distinct_mouth_times(&self, count: usize) -> Vec<f32> {
        let mut chosen: Vec<(f32, u32)> = Vec::new();
        for f in &self.frames {
            if f.time < 0.4 || chosen.iter().any(|c| c.1 == f.viseme) {
                continue;
            }
            if chosen.last().is_none_or(|c| f.time - c.0 >= 0.3) {
                chosen.push((f.time, f.viseme));
            }
            if chosen.len() == count {
                break;
            }
        }
        chosen.iter().map(|c| c.0 + 0.02).collect()
    }

    /// Weight of every viseme at `t` (sums to 1).
    pub fn weights(&self, t: f32) -> [f32; VISEMES] {
        let mut w = [0.0; VISEMES];
        match self.sample(t) {
            Some((a, b, f)) => {
                w[(a as usize).min(VISEMES - 1)] += 1.0 - f;
                w[(b as usize).min(VISEMES - 1)] += f;
            }
            None => w[REST as usize] = 1.0,
        }
        w
    }
}

/// The phoneme table (Engine:0x105e7d68): 40 labels and their viseme.
pub const PHONEMES: [(&str, u32); 40] = [
    ("x", 1),
    ("IY", 5),
    ("IH", 10),
    ("EH", 11),
    ("AE", 10),
    ("AH", 11),
    ("UW", 9),
    ("UH", 9),
    ("AA", 10),
    ("AO", 11),
    ("EY", 11),
    ("AY", 11),
    ("OY", 13),
    ("AW", 9),
    ("OW", 13),
    ("l", 12),
    ("r", 8),
    ("y", 5),
    ("w", 9),
    ("ER", 8),
    ("m", 1),
    ("n", 7),
    ("NG", 4),
    ("CH", 3),
    ("j", 3),
    ("DH", 7),
    ("b", 1),
    ("d", 7),
    ("g", 4),
    ("p", 1),
    ("t", 7),
    ("k", 4),
    ("z", 6),
    ("ZH", 3),
    ("v", 2),
    ("f", 2),
    ("TH", 7),
    ("s", 6),
    ("SH", 3),
    ("h", 4),
];

/// Port of eCVisualAnimationLipSync::FindePhoneByLabelString (Engine:0x10027650):
/// case-insensitive lookup; unknown labels give the rest viseme.
pub fn viseme_for_label(label: &str) -> u32 {
    gothic_portcov::hit!("viseme_for_label");
    PHONEMES
        .iter()
        .find(|(l, _)| l.eq_ignore_ascii_case(label))
        .map_or(REST, |&(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(frames: &[(f32, u32)]) -> Vec<u8> {
        let mut b = 1u16.to_le_bytes().to_vec();
        b.extend((frames.len() as u32).to_le_bytes());
        for (t, v) in frames {
            b.extend(t.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_and_samples_between_frames() {
        let l = LipSync::parse(&file(&[(0.0, 1), (0.2, 10), (0.6, 5)])).unwrap();
        assert_eq!(l.frames.len(), 3);
        assert!((l.duration() - 0.6).abs() < 1e-6);
        assert_eq!(l.sample(-1.0), Some((1, 1, 0.0)));
        let (a, b, f) = l.sample(0.4).unwrap();
        assert_eq!((a, b), (10, 5));
        assert!((f - 0.5).abs() < 1e-5);
        assert_eq!(l.sample(9.0), Some((5, 5, 0.0)));
        let w = l.weights(0.1);
        assert!((w[1] - 0.5).abs() < 1e-6 && (w[10] - 0.5).abs() < 1e-6);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reads_the_genome_wrapped_english_framing() {
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(file(&[(0.0, 4), (0.08, 10)]));
        b.extend([0xEF, 0xBE, 0xAD, 0xDE, 1, 0, 0, 0, 0]);
        let l = LipSync::parse(&b).unwrap();
        assert_eq!(
            l.frames[1],
            Frame {
                time: 0.08,
                viseme: 10
            }
        );
    }

    #[test]
    fn picks_distinct_mouth_shapes() {
        let l = LipSync::parse(&file(&[
            (0.0, 1),
            (0.5, 11),
            (0.6, 11),
            (0.9, 9),
            (1.0, 1),
            (1.3, 7),
        ]))
        .unwrap();
        // 0.5 (11), 0.9 (9) is 0.4 later, 1.0 is the rest viseme (1) but only 0.1 later -> skipped
        // in favour of 1.3 (7) which is 0.4 after 0.9.
        let t = l.distinct_mouth_times(3);
        assert_eq!(t.len(), 3);
        assert!((t[0] - 0.52).abs() < 1e-4 && (t[1] - 0.92).abs() < 1e-4);
    }

    #[test]
    fn rejects_damaged_input_without_panicking() {
        let good = file(&[(0.0, 1), (0.5, 2)]);
        assert!(LipSync::parse(&good[..good.len() - 1]).is_err());
        assert!(LipSync::parse(&[]).is_err());
        let mut v2 = good.clone();
        v2[0] = 2;
        assert!(LipSync::parse(&v2).is_err());
        assert!(LipSync::parse(&file(&[(0.5, 1), (0.1, 2)])).is_err());
        assert!(LipSync::parse(&file(&[(f32::NAN, 1)])).is_err());
        let mut huge = good;
        huge[2..6].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(LipSync::parse(&huge).is_err());
        let bytes = file(&[(0.0, 1), (0.5, 2)]);
        for n in 0..=bytes.len() {
            let _ = LipSync::parse(&bytes[..n]);
        }
    }

    #[test]
    fn phoneme_table_matches_the_engine() {
        assert_eq!(viseme_for_label("AA"), 10);
        assert_eq!(viseme_for_label("aa"), 10);
        assert_eq!(viseme_for_label("OW"), 13);
        assert_eq!(viseme_for_label("sh"), 3);
        assert_eq!(viseme_for_label("???"), REST);
        assert_eq!(PHONEMES.iter().map(|p| p.1).max(), Some(13));
    }
}
