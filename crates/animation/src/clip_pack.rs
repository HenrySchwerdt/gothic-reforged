//! Local clip pack: retargeted clips (plus optional timing metadata) in one small binary file.
//!
//! The pack is produced offline by a user-run tool from animation data the user owns and is
//! never part of the repository (it lives under the gitignored `.local/`). The runtime treats
//! it like any optional resource: when the file is absent nothing changes and the stock Gothic 3
//! motions play. Clip names are free-form (`ER_Roll_Front`, ...); they are registered in the
//! [`crate::ActorLibrary`] next to the `.xmot` motions and bound to rigs by node name, so a clip
//! only drives the skeleton family it was retargeted for.
//!
//! Layout (little endian):
//! ```text
//! "GRCP" u32 version=1 u32 clipCount
//! clip: str name, f32 duration, f32 fps, u8 looping, u32 nodeCount, nodes..., u8 hasTiming, timing?
//! node: str name, rest t[3] q[4] s[3], u32 posKeys, u32 rotKeys, pos[3]*, rot[4]*   (uniform 1/fps)
//! timing: f32 total input_from cancel_light cancel_heavy cancel_dodge cancel_jump cancel_guard
//!         cancel_move iframes[2], u32 hits, (from,to,stop)*, u8 hasCharge, charge[2],
//!         u8 hasTravel, travel[3] (from, to, distance cm)
//! str: u16 length + UTF-8
//! ```
use crate::{
    clip::{Clip, ClipNode, Track},
    pose::Trs,
};
use anyhow::{Result, bail};
use bevy::math::{Quat, Vec3};
use std::collections::HashMap;

pub const MAGIC: &[u8; 4] = b"GRCP";

/// Action timing in 30 fps animation frames, as authored with the clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipTiming {
    pub total: f32,
    pub input_from: f32,
    pub cancel_light: f32,
    pub cancel_heavy: f32,
    pub cancel_dodge: f32,
    pub cancel_jump: f32,
    pub cancel_guard: f32,
    pub cancel_move: f32,
    pub iframes: (f32, f32),
    /// Hit windows `(from, to, hit-stop seconds)`.
    pub hits: Vec<(f32, f32, f32)>,
    pub charge: Option<(f32, f32)>,
    /// Ground travel `(from frame, to frame, distance in cm; negative is backwards)`.
    pub travel: Option<(f32, f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct PackClip {
    pub clip: Clip,
    pub fps: f32,
    pub timing: Option<ClipTiming>,
}

#[derive(Clone, Debug, Default)]
pub struct ClipPack {
    pub clips: Vec<PackClip>,
}

struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.0.extend_from_slice(&(s.len() as u16).to_le_bytes());
        self.0.extend_from_slice(s.as_bytes());
    }
}

struct R<'a>(&'a [u8], usize);
impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.1.checked_add(n).filter(|&e| e <= self.0.len());
        let Some(end) = end else {
            bail!("clip pack truncated at {}", self.1)
        };
        let s = &self.0[self.1..end];
        self.1 = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn str(&mut self) -> Result<String> {
        let n = u16::from_le_bytes(self.take(2)?.try_into()?) as usize;
        Ok(String::from_utf8(self.take(n)?.to_vec())?)
    }
    fn v3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn q(&mut self) -> Result<Quat> {
        let q = Quat::from_xyzw(self.f32()?, self.f32()?, self.f32()?, self.f32()?);
        Ok(if q.length_squared() > 1e-12 && q.is_finite() {
            q.normalize()
        } else {
            Quat::IDENTITY
        })
    }
}

impl ClipPack {
    pub fn get(&self, name: &str) -> Option<&PackClip> {
        self.clips.iter().find(|c| c.clip.name == name)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.0.extend_from_slice(MAGIC);
        w.u32(1);
        w.u32(self.clips.len() as u32);
        for pc in &self.clips {
            let c = &pc.clip;
            w.str(&c.name);
            w.f32(c.duration);
            w.f32(pc.fps);
            w.u8(u8::from(c.looping));
            w.u32(c.nodes.len() as u32);
            for n in &c.nodes {
                w.str(&n.name);
                for v in [n.rest.t.x, n.rest.t.y, n.rest.t.z] {
                    w.f32(v);
                }
                for v in n.rest.r.to_array() {
                    w.f32(v);
                }
                for v in [n.rest.s.x, n.rest.s.y, n.rest.s.z] {
                    w.f32(v);
                }
                w.u32(n.pos.len() as u32);
                w.u32(n.rot.len() as u32);
                for p in n.pos.values() {
                    for v in p.to_array() {
                        w.f32(v);
                    }
                }
                for q in n.rot.values() {
                    for v in q.to_array() {
                        w.f32(v);
                    }
                }
            }
            match &pc.timing {
                None => w.u8(0),
                Some(t) => {
                    w.u8(1);
                    for v in [
                        t.total,
                        t.input_from,
                        t.cancel_light,
                        t.cancel_heavy,
                        t.cancel_dodge,
                        t.cancel_jump,
                        t.cancel_guard,
                        t.cancel_move,
                        t.iframes.0,
                        t.iframes.1,
                    ] {
                        w.f32(v);
                    }
                    w.u32(t.hits.len() as u32);
                    for h in &t.hits {
                        w.f32(h.0);
                        w.f32(h.1);
                        w.f32(h.2);
                    }
                    match t.charge {
                        Some(c) => {
                            w.u8(1);
                            w.f32(c.0);
                            w.f32(c.1);
                        }
                        None => w.u8(0),
                    }
                    match t.travel {
                        Some(c) => {
                            w.u8(1);
                            w.f32(c.0);
                            w.f32(c.1);
                            w.f32(c.2);
                        }
                        None => w.u8(0),
                    }
                }
            }
        }
        w.0
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = R(data, 0);
        if r.take(4)? != MAGIC {
            bail!("not a clip pack (bad magic)");
        }
        let ver = r.u32()?;
        if ver != 1 {
            bail!("unsupported clip pack version {ver}");
        }
        let count = r.u32()? as usize;
        let mut clips = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            let name = r.str()?;
            let duration = r.f32()?;
            let fps = r.f32()?;
            if !(1.0..=480.0).contains(&fps) {
                bail!("clip {name}: bad frame rate {fps}");
            }
            let looping = r.u8()? != 0;
            let nn = r.u32()? as usize;
            let mut nodes = Vec::with_capacity(nn.min(1024));
            let mut by_name = HashMap::new();
            for i in 0..nn {
                let nname = r.str()?;
                let rest = Trs {
                    t: r.v3()?,
                    r: r.q()?,
                    s: r.v3()?,
                };
                let (np, nr) = (r.u32()? as usize, r.u32()? as usize);
                if np > 100_000 || nr > 100_000 {
                    bail!("clip {name}: implausible key count");
                }
                let dt = 1.0 / fps;
                let mut pos = Vec::with_capacity(np);
                for k in 0..np {
                    pos.push((k as f32 * dt, r.v3()?));
                }
                let mut rot = Vec::with_capacity(nr);
                for k in 0..nr {
                    rot.push((k as f32 * dt, r.q()?));
                }
                by_name.insert(nname.clone(), i as u16);
                nodes.push(ClipNode {
                    name: nname,
                    pos: Track::new(pos),
                    rot: Track::new(rot),
                    scale: Track::new(Vec::new()),
                    rest,
                });
            }
            let timing = if r.u8()? != 0 {
                let mut f = [0.0f32; 10];
                for v in &mut f {
                    *v = r.f32()?;
                }
                let nh = r.u32()? as usize;
                if nh > 64 {
                    bail!("clip {name}: implausible hit count");
                }
                let mut hits = Vec::new();
                for _ in 0..nh {
                    hits.push((r.f32()?, r.f32()?, r.f32()?));
                }
                let charge = if r.u8()? != 0 {
                    Some((r.f32()?, r.f32()?))
                } else {
                    None
                };
                let travel = if r.u8()? != 0 {
                    Some((r.f32()?, r.f32()?, r.f32()?))
                } else {
                    None
                };
                Some(ClipTiming {
                    total: f[0],
                    input_from: f[1],
                    cancel_light: f[2],
                    cancel_heavy: f[3],
                    cancel_dodge: f[4],
                    cancel_jump: f[5],
                    cancel_guard: f[6],
                    cancel_move: f[7],
                    iframes: (f[8], f[9]),
                    hits,
                    charge,
                    travel,
                })
            } else {
                None
            };
            clips.push(PackClip {
                clip: Clip {
                    name,
                    duration,
                    nodes,
                    looping,
                    nominal_speed: 0.0,
                    implied_dir: Vec3::ZERO,
                    implied_turn: 0.0,
                    by_name,
                },
                fps,
                timing,
            });
        }
        Ok(Self { clips })
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }
}

/// Parses the line-based timing text the extraction tools write next to a source set:
/// `Name key=value ...` with keys `total input_from cancel_light cancel_heavy cancel_dodge
/// cancel_jump cancel_guard cancel_move` (frames), `iframes=a,b`, `hits=from:to:stop;...`,
/// `charge=a,b`, `travel=from,to,cm`. Unknown keys and malformed lines are skipped.
pub fn parse_timing(text: &str) -> HashMap<String, ClipTiming> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(name) = it.next() else { continue };
        let mut t = ClipTiming::default();
        for kv in it {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let nums = |s: &str, sep: char| -> Vec<f32> {
                s.split(sep).filter_map(|x| x.parse().ok()).collect()
            };
            match k {
                "total" => t.total = v.parse().unwrap_or(0.0),
                "input_from" => t.input_from = v.parse().unwrap_or(0.0),
                "cancel_light" => t.cancel_light = v.parse().unwrap_or(0.0),
                "cancel_heavy" => t.cancel_heavy = v.parse().unwrap_or(0.0),
                "cancel_dodge" => t.cancel_dodge = v.parse().unwrap_or(0.0),
                "cancel_jump" => t.cancel_jump = v.parse().unwrap_or(0.0),
                "cancel_guard" => t.cancel_guard = v.parse().unwrap_or(0.0),
                "cancel_move" => t.cancel_move = v.parse().unwrap_or(0.0),
                "iframes" => {
                    if let [a, b] = nums(v, ',')[..] {
                        t.iframes = (a, b);
                    }
                }
                "charge" => {
                    if let [a, b] = nums(v, ',')[..] {
                        t.charge = Some((a, b));
                    }
                }
                "travel" => {
                    if let [a, b, c] = nums(v, ',')[..] {
                        t.travel = Some((a, b, c));
                    }
                }
                "hits" => {
                    for h in v.split(';') {
                        if let [a, b, c] = nums(h, ':')[..] {
                            t.hits.push((a, b, c));
                        }
                    }
                }
                _ => {}
            }
        }
        if t.total > 0.0 {
            out.insert(name.to_string(), t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ClipPack {
        let rot: Vec<(f32, Quat)> = (0..4)
            .map(|i| (i as f32 / 30.0, Quat::from_rotation_y(i as f32 * 0.1)))
            .collect();
        let pos: Vec<(f32, Vec3)> = (0..4)
            .map(|i| (i as f32 / 30.0, Vec3::splat(i as f32)))
            .collect();
        let node = ClipNode {
            name: "Test_Hip_1".into(),
            pos: Track::new(pos),
            rot: Track::new(rot),
            scale: Track::new(vec![]),
            rest: Trs::IDENTITY,
        };
        let mut by_name = HashMap::new();
        by_name.insert("Test_Hip_1".to_string(), 0u16);
        ClipPack {
            clips: vec![PackClip {
                clip: Clip {
                    name: "Synthetic_Roll".into(),
                    duration: 3.0 / 30.0,
                    nodes: vec![node],
                    looping: false,
                    nominal_speed: 0.0,
                    implied_dir: Vec3::ZERO,
                    implied_turn: 0.0,
                    by_name,
                },
                fps: 30.0,
                timing: Some(ClipTiming {
                    total: 50.0,
                    iframes: (0.0, 13.0),
                    hits: vec![(10.0, 12.0, 0.08)],
                    charge: Some((2.0, 20.0)),
                    travel: Some((1.0, 30.0, 250.0)),
                    ..Default::default()
                }),
            }],
        }
    }

    #[test]
    fn round_trips() {
        let pack = sample();
        let back = ClipPack::from_bytes(&pack.to_bytes()).unwrap();
        assert_eq!(back.clips.len(), 1);
        let (a, b) = (&pack.clips[0], &back.clips[0]);
        assert_eq!(a.clip.name, b.clip.name);
        assert_eq!(a.timing, b.timing);
        let (qa, qb) = (
            a.clip.nodes[0].rot.sample(0.05).unwrap(),
            b.clip.nodes[0].rot.sample(0.05).unwrap(),
        );
        assert!(qa.dot(qb) > 0.9999);
        assert_eq!(b.clip.nodes[0].pos.len(), 4);
    }

    #[test]
    fn parses_timing_lines() {
        let m = parse_timing(
            "Roll_Front total=50 input_from=15 iframes=0,13 hits=10:12:0.08;20:22:0.1 charge=2,20 travel=1,30,250 bogus=1
bad
Zero total=0
",
        );
        assert_eq!(m.len(), 1);
        let t = &m["Roll_Front"];
        assert_eq!(t.total, 50.0);
        assert_eq!(t.iframes, (0.0, 13.0));
        assert_eq!(t.hits.len(), 2);
        assert_eq!(t.travel, Some((1.0, 30.0, 250.0)));
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(ClipPack::from_bytes(b"nope").is_err());
        let bytes = sample().to_bytes();
        for n in 0..bytes.len() {
            assert!(ClipPack::from_bytes(&bytes[..n]).is_err(), "len {n}");
        }
    }
}
