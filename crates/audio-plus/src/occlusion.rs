//! Occlusion (the path is blocked: quieter and duller, the reverb too) and obstruction (the
//! direct path is blocked but the room still carries the sound: duller direct sound only),
//! from ray probes against the level geometry.

/// Probe result, both 0 (free) to 1 (fully blocked).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Blocking {
    /// The direct line is blocked.
    pub obstruction: f32,
    /// The paths around the direct line are blocked too.
    pub occlusion: f32,
}

/// Rays: the direct one plus a ring of offset rays around it (`spread` m) on both ends.
pub fn probe(
    listener: [f32; 3],
    emitter: [f32; 3],
    spread: f32,
    blocked: &dyn Fn([f32; 3], [f32; 3]) -> bool,
) -> Blocking {
    let direct = blocked(listener, emitter);
    // Offsets perpendicular-ish to the path (fixed set: deterministic).
    let d = [
        emitter[0] - listener[0],
        emitter[1] - listener[1],
        emitter[2] - listener[2],
    ];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    let up = if dir[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = cross(dir, up);
    let side = normalize(side);
    let up = cross(side, dir);
    let offsets = [
        [side[0], side[1], side[2]],
        [-side[0], -side[1], -side[2]],
        [up[0], up[1], up[2]],
        [-up[0] * 0.5, -up[1] * 0.5, -up[2] * 0.5],
    ];
    let mut hits = 0;
    for o in offsets {
        let a = [
            listener[0] + o[0] * spread,
            listener[1] + o[1] * spread,
            listener[2] + o[2] * spread,
        ];
        let b = [
            emitter[0] + o[0] * spread,
            emitter[1] + o[1] * spread,
            emitter[2] + o[2] * spread,
        ];
        hits += usize::from(blocked(a, b));
    }
    let around = hits as f32 / offsets.len() as f32;
    Blocking {
        obstruction: if direct { 1.0 } else { around * 0.5 },
        occlusion: if direct { around } else { 0.0 },
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// What blocking does to the sound.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OcclusionTuning {
    /// Direct-path attenuation at full occlusion / full obstruction (dB, negative).
    pub occlusion_db: f32,
    pub obstruction_db: f32,
    /// Low-pass cutoff of the direct path when fully blocked (Hz).
    pub blocked_cutoff: f32,
    /// Reverb send attenuation at full occlusion (dB).
    pub occlusion_reverb_db: f32,
}

impl Default for OcclusionTuning {
    fn default() -> Self {
        Self {
            occlusion_db: -18.0,
            obstruction_db: -6.0,
            blocked_cutoff: 700.0,
            occlusion_reverb_db: -10.0,
        }
    }
}

impl OcclusionTuning {
    /// (direct gain, direct low-pass cutoff Hz, reverb send gain).
    pub fn apply(&self, b: Blocking) -> (f32, f32, f32) {
        let db = self.occlusion_db * b.occlusion + self.obstruction_db * b.obstruction;
        let dull = b.occlusion.max(b.obstruction).clamp(0.0, 1.0);
        // Log-interpolate the cutoff from 20 kHz down.
        let cutoff = (20_000f32.ln() * (1.0 - dull) + self.blocked_cutoff.ln() * dull).exp();
        let send = crate::dsp::db_to_gain(self.occlusion_reverb_db * b.occlusion);
        (crate::dsp::db_to_gain(db), cutoff, send)
    }
}
