//! Independent reader for Gothic 3 `.xmot` motion resources: GENOMFLE envelope around a
//! chunked "LMA " container with one pose record per animated node followed by optional
//! linear key tracks. See `docs/research/ACTOR_ANIMATION_FORMAT.md`.
use crate::{
    emfx::{Reader, read_container},
    genome::GenomeFile,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

const MAX_NODES: usize = 8192;
const MAX_KEYS: usize = 200_000;
const CHUNK_POSE: u32 = 1;
const CHUNK_TRACK: u32 = 2;
const TAG_POSITION: u16 = 0x504c; // "LP"
const TAG_ROTATION: u16 = 0x524c; // "LR"
const TAG_SCALE: u16 = 0x534c; // "LS"

#[derive(Debug, Clone, Serialize, Default)]
pub struct Track<T> {
    /// Key time in seconds and value, ascending in time.
    pub keys: Vec<(f32, T)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotionNode {
    pub name: String,
    /// Static pose stored in the node record.
    pub position: [f32; 3],
    /// Quaternion x, y, z, w.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub positions: Track<[f32; 3]>,
    pub rotations: Track<[f32; 4]>,
    pub scales: Track<[f32; 3]>,
    /// Keys skipped because a value was not finite.
    pub dropped_keys: usize,
}

#[derive(Debug, Serialize)]
pub struct Motion {
    pub nodes: Vec<MotionNode>,
    /// Latest key time over all tracks, in seconds (0 for pose-only motions).
    pub duration: f32,
    pub key_count: usize,
    /// Smallest positive spacing between consecutive keys, in seconds.
    pub min_key_interval: f32,
}

impl Motion {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let genome = GenomeFile::parse(bytes).context("invalid XMOT GENOMFLE envelope")?;
        let container = read_container(genome.body, b"LMA ", 3, false)?;
        ensure!(container.leftover == 0, "unconsumed LMA bytes");
        let mut nodes: Vec<MotionNode> = Vec::new();
        for chunk in &container.chunks {
            match chunk.id {
                CHUNK_POSE => {
                    ensure!(chunk.version == 3, "unsupported motion pose version");
                    ensure!(nodes.len() < MAX_NODES, "too many motion nodes");
                    nodes.push(parse_pose(chunk.data)?);
                }
                CHUNK_TRACK => {
                    ensure!(chunk.version == 1, "unsupported track version");
                    let node = nodes.last_mut().context("track before any node record")?;
                    parse_track(chunk.data, node)?;
                }
                other => anyhow::bail!("unsupported LMA chunk {other}"),
            }
        }
        ensure!(!nodes.is_empty(), "motion has no nodes");
        let mut duration = 0f32;
        let mut key_count = 0usize;
        let mut min_key_interval = f32::INFINITY;
        for node in &nodes {
            track_stats(
                &node.positions.keys,
                &mut duration,
                &mut key_count,
                &mut min_key_interval,
            );
            track_stats(
                &node.rotations.keys,
                &mut duration,
                &mut key_count,
                &mut min_key_interval,
            );
            track_stats(
                &node.scales.keys,
                &mut duration,
                &mut key_count,
                &mut min_key_interval,
            );
        }
        Ok(Self {
            nodes,
            duration,
            key_count,
            min_key_interval: if min_key_interval.is_finite() {
                min_key_interval
            } else {
                0.0
            },
        })
    }
}

fn track_stats<T>(keys: &[(f32, T)], duration: &mut f32, count: &mut usize, min_dt: &mut f32) {
    *count += keys.len();
    if let Some((t, _)) = keys.last() {
        *duration = duration.max(*t);
    }
    for pair in keys.windows(2) {
        let dt = pair[1].0 - pair[0].0;
        if dt > 1e-6 {
            *min_dt = min_dt.min(dt);
        }
    }
}

fn parse_pose(data: &[u8]) -> Result<MotionNode> {
    let mut r = Reader::new(data);
    let position = r.vec3()?;
    let rotation = r.quat()?;
    let scale = r.vec3()?;
    // 40 bytes of uninitialised exporter memory (pointer-like words) follow.
    r.take(40)?;
    let name = r.string()?;
    ensure!(r.len() == 0, "trailing pose bytes");
    ensure!(!name.is_empty(), "unnamed motion node");
    Ok(MotionNode {
        name,
        position,
        rotation,
        scale,
        positions: Track::default(),
        rotations: Track::default(),
        scales: Track::default(),
        dropped_keys: 0,
    })
}

fn parse_track(data: &[u8], node: &mut MotionNode) -> Result<()> {
    let mut r = Reader::new(data);
    let count = r.u32()? as usize;
    // Low 16 bits identify the track type; the high half is exporter garbage.
    let tag = (r.u32()? & 0xffff) as u16;
    ensure!(count <= MAX_KEYS, "key count exceeds ceiling");
    let stride = match tag {
        TAG_POSITION | TAG_SCALE => 16,
        TAG_ROTATION => 20,
        other => anyhow::bail!("unsupported track tag {other:#06x}"),
    };
    ensure!(
        r.len() == count * stride,
        "track size differs from key count"
    );
    let mut last = f32::NEG_INFINITY;
    let mut dropped = 0usize;
    for _ in 0..count {
        let t = r.f32()?;
        ensure!(t >= last && t >= 0.0, "key times not ascending");
        last = t;
        let mut value = [0f32; 4];
        let width = if tag == TAG_ROTATION { 4 } else { 3 };
        for v in value.iter_mut().take(width) {
            *v = r.f32_lenient_flag(&mut dropped)?;
        }
        // Observed in a few Fat_/Orc_ motions: uninitialised (NaN/Inf) key values. The
        // key is dropped; neighbouring keys still bracket the time.
        if dropped > 0 {
            dropped = 0;
            node.dropped_keys += 1;
            continue;
        }
        match tag {
            TAG_POSITION => node
                .positions
                .keys
                .push((t, [value[0], value[1], value[2]])),
            TAG_SCALE => node.scales.keys.push((t, [value[0], value[1], value[2]])),
            _ => node.rotations.keys.push((t, value)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32b(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn f32s(b: &mut Vec<u8>, v: &[f32]) {
        for x in v {
            b.extend(x.to_le_bytes());
        }
    }
    fn chunk(out: &mut Vec<u8>, id: u32, version: u32, data: &[u8]) {
        u32b(out, id);
        u32b(out, data.len() as u32);
        u32b(out, version);
        out.extend(data);
    }
    fn pose(name: &str) -> Vec<u8> {
        let mut d = Vec::new();
        f32s(&mut d, &[1., 2., 3.]);
        f32s(&mut d, &[0., 0., 0., 1.]);
        f32s(&mut d, &[1., 1., 1.]);
        d.extend([0xab; 40]);
        u32b(&mut d, name.len() as u32);
        d.extend(name.as_bytes());
        d
    }
    fn track(tag: u32, keys: &[(f32, Vec<f32>)]) -> Vec<u8> {
        let mut d = Vec::new();
        u32b(&mut d, keys.len() as u32);
        u32b(&mut d, tag);
        for (t, v) in keys {
            f32s(&mut d, &[*t]);
            f32s(&mut d, v);
        }
        d
    }
    fn motion_bytes(extra: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut chunks = Vec::new();
        chunk(&mut chunks, 1, 3, &pose("Hero_ROOT"));
        chunk(
            &mut chunks,
            2,
            1,
            &track(
                0x00c6_504c,
                &[(0., vec![0., 0., 0.]), (0.04, vec![0., 1., 0.])],
            ),
        );
        chunk(
            &mut chunks,
            2,
            1,
            &track(
                0x23f_524c,
                &[(0., vec![0., 0., 0., 1.]), (0.08, vec![0., 1., 0., 0.])],
            ),
        );
        chunk(&mut chunks, 1, 3, &pose("Static"));
        extra(&mut chunks);
        let mut body = vec![5, 0, 0, 0, 0, 0, 1, 2, 3, 4];
        body.extend(b"LMA ");
        body.extend([1, 1, 0]);
        body.extend(chunks);
        let mut out = b"GENOMFLE".to_vec();
        out.extend(1u16.to_le_bytes());
        out.extend(((14 + body.len()) as u32).to_le_bytes());
        out.extend(&body);
        out.extend([0xef, 0xbe, 0xad, 0xde, 1, 0, 0, 0, 0]);
        out
    }

    #[test]
    fn parses_synthetic_motion() -> Result<()> {
        let m = Motion::parse(&motion_bytes(|_| {}))?;
        assert_eq!(m.nodes.len(), 2);
        assert_eq!(m.nodes[0].name, "Hero_ROOT");
        assert_eq!(m.nodes[0].position, [1., 2., 3.]);
        assert_eq!(m.nodes[0].positions.keys.len(), 2);
        assert_eq!(m.nodes[0].rotations.keys[1].1, [0., 1., 0., 0.]);
        assert!(m.nodes[1].positions.keys.is_empty());
        assert!((m.duration - 0.08).abs() < 1e-6);
        assert!((m.min_key_interval - 0.04).abs() < 1e-6);
        assert_eq!(m.key_count, 4);
        Ok(())
    }

    #[test]
    fn rejects_bad_tracks() {
        // Track before node, wrong size, descending times, unknown tag, unknown chunk.
        let mut c = Vec::new();
        chunk(&mut c, 2, 1, &track(0x504c, &[(0., vec![0.; 3])]));
        let mut b = motion_bytes(|_| {});
        assert!(Motion::parse(&b).is_ok());
        let bad_tag = motion_bytes(|c| chunk(c, 2, 1, &track(0x1234, &[(0., vec![0.; 3])])));
        assert!(Motion::parse(&bad_tag).is_err());
        let descending = motion_bytes(|c| {
            chunk(
                c,
                2,
                1,
                &track(0x504c, &[(0.5, vec![0.; 3]), (0.1, vec![0.; 3])]),
            )
        });
        assert!(Motion::parse(&descending).is_err());
        let mut wrong = track(0x504c, &[(0., vec![0.; 3])]);
        wrong.truncate(wrong.len() - 4);
        let wrong = motion_bytes(|c| chunk(c, 2, 1, &wrong));
        assert!(Motion::parse(&wrong).is_err());
        let unknown = motion_bytes(|c| chunk(c, 9, 1, &[0; 4]));
        assert!(Motion::parse(&unknown).is_err());
        b.truncate(30);
        assert!(Motion::parse(&b).is_err());
    }

    #[test]
    fn truncation_never_panics() {
        let bytes = motion_bytes(|_| {});
        for n in 0..bytes.len() {
            let _ = Motion::parse(&bytes[..n]);
        }
    }
}
