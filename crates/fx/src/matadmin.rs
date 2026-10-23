//! Shader-material admin rules that run per frame: the texture-coordinate nodes of a material
//! graph (`eCTexCoordSrcScroller`, `...Oscillator`, `...Rotator`), refreshed by
//! `eCShaderMaterialAdmin::Process` (Engine:0x102388b0) through `SetTime`, and the sampler's
//! flip-book animation (`eCColorSrcSampler.AnimationSpeed`).
//!
//! The engine bakes the result into a 3x3 matrix per node; here the three nodes collapse into
//! one affine map `uv' = A * uv + b` that the surface shader applies.
use gothic_formats::xshmat::graph::UvAnim;

/// `uv' = [[a, b], [c, d]] * uv + [tx, ty]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvTransform {
    pub m: [f32; 4],
    pub t: [f32; 2],
}

impl UvTransform {
    pub const IDENTITY: Self = Self {
        m: [1.0, 0.0, 0.0, 1.0],
        t: [0.0, 0.0],
    };

    pub fn apply(&self, uv: [f32; 2]) -> [f32; 2] {
        [
            self.m[0] * uv[0] + self.m[1] * uv[1] + self.t[0],
            self.m[2] * uv[0] + self.m[3] * uv[1] + self.t[1],
        ]
    }

    /// `self` applied after `first`.
    fn then(&self, first: &Self) -> Self {
        let m = [
            self.m[0] * first.m[0] + self.m[1] * first.m[2],
            self.m[0] * first.m[1] + self.m[1] * first.m[3],
            self.m[2] * first.m[0] + self.m[3] * first.m[2],
            self.m[2] * first.m[1] + self.m[3] * first.m[3],
        ];
        let t = self.apply(first.t);
        Self { m, t }
    }
}

fn frac(x: f32) -> f32 {
    x - x.floor()
}

/// Port of eCTexCoordSrcScroller::SetTime (Engine:0x102628f0): the offset is the
/// fractional part of `direction * rate * time` (wrapping keeps float precision over long
/// sessions), added to the authored `ScrollOffset`.
pub fn scroller(offset: [f32; 2], direction: [f32; 2], rate: f32, time: f32) -> [f32; 2] {
    gothic_portcov::hit!("scroller");
    [
        offset[0] + frac(direction[0] * rate * time),
        offset[1] + frac(direction[1] * rate * time),
    ]
}

/// Pan oscillation of eCTexCoordSrcOscillator::SetTime (Engine:0x1025d4d0, type 0): a sine of `time * rate + phase` scaled
/// by the amplitude, plus the authored offset. [I] the stretch and jitter types are not modelled
/// (they leave the coordinate unchanged).
pub fn oscillator_pan(
    types: [u32; 2],
    amplitude: [f32; 2],
    rate: [f32; 2],
    phase: [f32; 2],
    offset: [f32; 2],
    time: f32,
) -> [f32; 2] {
    let axis = |i: usize| {
        if types[i] == 0 {
            offset[i] + (time * rate[i] + phase[i]).sin() * amplitude[i]
        } else {
            0.0
        }
    };
    [axis(0), axis(1)]
}

/// Constant rotation of eCTexCoordSrcRotator::SetTime (Engine:0x1025f910, kind 1): the roll angle grows with time around the
/// authored centre. [I] other kinds keep the authored angle.
pub fn rotator(kind: u32, angle: [f32; 3], centre: [f32; 2], time: f32) -> UvTransform {
    let roll = if kind == 1 { angle[2] * time } else { angle[2] };
    let (s, c) = roll.sin_cos();
    // Rotate around the centre: uv' = R (uv - centre) + centre.
    let m = [c, -s, s, c];
    let t = [
        centre[0] - (m[0] * centre[0] + m[1] * centre[1]),
        centre[1] - (m[2] * centre[0] + m[3] * centre[1]),
    ];
    UvTransform { m, t }
}

/// The combined transform of a sampler's animation nodes at `time` (seconds).
pub fn uv_transform(anim: &UvAnim, time: f32) -> UvTransform {
    let mut out = UvTransform::IDENTITY;
    if let Some(r) = &anim.rotate {
        out = rotator(r.kind, r.angle, r.offset, time).then(&out);
    }
    if let Some(o) = &anim.oscillate {
        let d = oscillator_pan(o.types, o.amplitude, o.rate, o.phase, o.offset, time);
        out = UvTransform {
            m: [1.0, 0.0, 0.0, 1.0],
            t: d,
        }
        .then(&out);
    }
    if let Some(s) = &anim.scroll {
        let d = scroller(s.offset, s.direction, s.rate, time);
        out = UvTransform {
            m: [1.0, 0.0, 0.0, 1.0],
            t: d,
        }
        .then(&out);
    }
    out
}

/// What the surface shader needs to animate one sampler (it evaluates the transform per pixel
/// from `globals.time`, so the material never changes after creation):
/// `scroll = [velocity.x, velocity.y, offset.x, offset.y]` in uv per second and
/// `rotate = [rate, base angle, centre.x, centre.y]` in radians. [I] oscillators are skipped.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ShaderAnim {
    pub scroll: [f32; 4],
    pub rotate: [f32; 4],
}

/// `None` when the sampler is static.
pub fn shader_anim(anim: &UvAnim) -> Option<ShaderAnim> {
    let mut out = ShaderAnim::default();
    let mut any = false;
    if let Some(s) = &anim.scroll {
        out.scroll = [
            s.direction[0] * s.rate,
            s.direction[1] * s.rate,
            s.offset[0],
            s.offset[1],
        ];
        any = true;
    }
    if let Some(r) = &anim.rotate {
        let roll = r.angle[2];
        let (rate, base) = if r.kind == 1 {
            (roll, 0.0)
        } else {
            (0.0, roll)
        };
        if rate != 0.0 || base != 0.0 {
            out.rotate = [rate, base, r.offset[0], r.offset[1]];
            any = true;
        }
    }
    any.then_some(out)
}

/// Frame of a flip-book sampler (`eCColorSrcSampler.AnimationSpeed`, `SwitchRepeat`): frames per
/// second over `frames` images; repeat 0 wraps, 1 clamps on the last frame, 2 ping-pongs.
/// [I] the speed unit (frames per second) is inferred.
pub fn flipbook_frame(frames: u32, speed: f32, repeat: u32, time: f32) -> u32 {
    if frames <= 1 {
        return 0;
    }
    let n = frames as i64;
    let step = (time * speed).floor() as i64;
    match repeat {
        1 => step.clamp(0, n - 1) as u32,
        2 => {
            let period = 2 * (n - 1);
            let k = step.rem_euclid(period);
            (if k < n { k } else { period - k }) as u32
        }
        _ => step.rem_euclid(n) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_formats::xshmat::graph::{Rotate, Scroll};

    #[test]
    fn scroller_wraps_every_second_at_unit_rate() {
        let d = scroller([0.0; 2], [1.0, 0.5], 1.0, 2.25);
        assert!((d[0] - 0.25).abs() < 1e-6);
        assert!((d[1] - 0.125).abs() < 1e-6);
        let e = scroller([0.1, 0.2], [0.0; 2], 3.0, 9.0);
        assert!((e[0] - 0.1).abs() < 1e-6 && (e[1] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn negative_scroll_stays_in_range() {
        let d = scroller([0.0; 2], [-1.0, 0.0], 0.1, 1.0);
        assert!((d[0] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn empty_animation_is_the_identity() {
        assert_eq!(uv_transform(&UvAnim::default(), 5.0), UvTransform::IDENTITY);
    }

    #[test]
    fn scroll_translates_the_coordinate() {
        let anim = UvAnim {
            scroll: Some(Scroll {
                offset: [0.0; 2],
                direction: [0.0, 1.0],
                rate: 0.25,
            }),
            ..UvAnim::default()
        };
        let uv = uv_transform(&anim, 2.0).apply([0.5, 0.5]);
        assert!((uv[0] - 0.5).abs() < 1e-6 && (uv[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn constant_rotation_keeps_the_centre_fixed() {
        let anim = UvAnim {
            rotate: Some(Rotate {
                kind: 1,
                angle: [0.0, 0.0, std::f32::consts::FRAC_PI_2],
                offset: [0.5, 0.5],
            }),
            ..UvAnim::default()
        };
        let x = uv_transform(&anim, 1.0);
        let c = x.apply([0.5, 0.5]);
        assert!((c[0] - 0.5).abs() < 1e-5 && (c[1] - 0.5).abs() < 1e-5);
        let p = x.apply([1.0, 0.5]);
        assert!((p[0] - 0.5).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shader_anim_packs_scroll_and_rotation() {
        let anim = UvAnim {
            scroll: Some(Scroll {
                offset: [0.1, 0.2],
                direction: [-0.02, -0.9],
                rate: 0.55,
            }),
            rotate: Some(Rotate {
                kind: 1,
                angle: [0.0, 0.0, 0.5],
                offset: [0.5, 0.5],
            }),
            ..UvAnim::default()
        };
        let a = shader_anim(&anim).expect("animated");
        assert!((a.scroll[1] + 0.495).abs() < 1e-6);
        assert_eq!(a.scroll[2..], [0.1, 0.2]);
        assert_eq!(a.rotate, [0.5, 0.0, 0.5, 0.5]);
        assert!(shader_anim(&UvAnim::default()).is_none());
    }

    #[test]
    fn flipbook_modes() {
        assert_eq!(flipbook_frame(4, 2.0, 0, 3.0), 2);
        assert_eq!(flipbook_frame(4, 2.0, 1, 100.0), 3);
        let seq: Vec<u32> = (0..8)
            .map(|i| flipbook_frame(3, 1.0, 2, i as f32))
            .collect();
        assert_eq!(seq, [0, 1, 2, 1, 0, 1, 2, 1]);
        assert_eq!(flipbook_frame(1, 5.0, 0, 9.0), 0);
    }
}
