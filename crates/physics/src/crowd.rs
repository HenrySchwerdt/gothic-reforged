//! Character-versus-character collision.
//!
//! Characters are vertical capsules; for movement only the horizontal footprint and the
//! vertical overlap matter. The original uses PhysX shapes in collision groups `Player` / `NPC`
//! (`eECollisionGroup`) and the movement flag `CanBePushedWhileIdle` (V: false on every
//! shipped template): an idle character is not displaced by a mover, the mover is deflected
//! instead. This module reproduces that with a position-based separation pass.
use glam::Vec3;
use std::collections::HashMap;

/// One character for the separation pass.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    /// Feet position.
    pub position: Vec3,
    pub radius: f32,
    pub height: f32,
    /// Moved by its own controller this frame (idle characters are immovable unless
    /// `pushable_when_idle`).
    pub moving: bool,
    /// V: `CanBePushedWhileIdle` (false in all shipped templates).
    pub pushable_when_idle: bool,
}

impl Body {
    pub fn new(position: Vec3, radius: f32, height: f32) -> Self {
        Self {
            position,
            radius,
            height,
            moving: true,
            pushable_when_idle: false,
        }
    }

    fn immovable(&self) -> bool {
        !self.moving && !self.pushable_when_idle
    }
}

/// Horizontal displacement for every body so that no two overlap (up to `iterations`
/// relaxation passes). Bodies separated vertically (one standing on the other's head height
/// range) do not interact.
pub fn separate(bodies: &[Body], iterations: usize) -> Vec<Vec3> {
    let n = bodies.len();
    let mut pos: Vec<Vec3> = bodies.iter().map(|b| b.position).collect();
    if n < 2 {
        return vec![Vec3::ZERO; n];
    }
    let max_r = bodies.iter().map(|b| b.radius).fold(0.0, f32::max);
    let cell = (2.0 * max_r).max(1.0);
    for _ in 0..iterations.max(1) {
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, p) in pos.iter().enumerate() {
            grid.entry(key(*p, cell)).or_default().push(i);
        }
        let mut any = false;
        for i in 0..n {
            let (cx, cz) = key(pos[i], cell);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = grid.get(&(cx + dx, cz + dz)) else {
                        continue;
                    };
                    for &j in list {
                        if j <= i {
                            continue;
                        }
                        any |= push_pair(bodies, &mut pos, i, j);
                    }
                }
            }
        }
        if !any {
            break;
        }
    }
    pos.iter()
        .zip(bodies)
        .map(|(p, b)| *p - b.position)
        .collect()
}

fn key(p: Vec3, cell: f32) -> (i32, i32) {
    ((p.x / cell).floor() as i32, (p.z / cell).floor() as i32)
}

fn push_pair(bodies: &[Body], pos: &mut [Vec3], i: usize, j: usize) -> bool {
    let (a, b) = (&bodies[i], &bodies[j]);
    // Vertical overlap of the two capsules.
    if pos[i].y > pos[j].y + b.height || pos[j].y > pos[i].y + a.height {
        return false;
    }
    let d = Vec3::new(pos[j].x - pos[i].x, 0.0, pos[j].z - pos[i].z);
    let dist = d.length();
    let min = a.radius + b.radius;
    if dist >= min {
        return false;
    }
    let n = if dist > 1.0e-4 {
        d / dist
    } else {
        // Exactly stacked: separate along a deterministic direction.
        let ang = (i * 31 + j * 17) as f32;
        Vec3::new(ang.cos(), 0.0, ang.sin())
    };
    let pen = min - dist;
    let (wa, wb) = match (a.immovable(), b.immovable()) {
        (true, true) => return false,
        (true, false) => (0.0, 1.0),
        (false, true) => (1.0, 0.0),
        // Both move (or both are pushable): share equally (all masses are equal, V: both
        // rigid bodies of 80 for humans).
        (false, false) => (0.5, 0.5),
    };
    pos[i] -= n * pen * wa;
    pos[j] += n * pen * wb;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_movers_are_pushed_apart_symmetrically() {
        let b = [
            Body::new(Vec3::new(0.0, 0.0, 0.0), 35.0, 200.0),
            Body::new(Vec3::new(40.0, 0.0, 0.0), 35.0, 200.0),
        ];
        let d = separate(&b, 4);
        let a = b[0].position + d[0];
        let c = b[1].position + d[1];
        assert!((c.x - a.x - 70.0).abs() < 0.1);
        assert!((d[0].x + d[1].x).abs() < 0.01, "symmetric");
    }

    #[test]
    fn idle_character_is_not_displaced() {
        let mut idle = Body::new(Vec3::ZERO, 35.0, 200.0);
        idle.moving = false;
        let b = [idle, Body::new(Vec3::new(30.0, 0.0, 0.0), 35.0, 200.0)];
        let d = separate(&b, 4);
        assert_eq!(d[0], Vec3::ZERO);
        assert!((b[1].position.x + d[1].x - 70.0).abs() < 0.1);
    }

    #[test]
    fn crowd_of_two_hundred_ends_without_overlap() {
        let mut bodies = Vec::new();
        for i in 0..200 {
            let a = i as f32 * 2.399;
            let r = 40.0 + (i as f32).sqrt() * 70.0;
            bodies.push(Body::new(
                Vec3::new(a.cos() * r, 0.0, a.sin() * r),
                35.0,
                200.0,
            ));
        }
        for _ in 0..8 {
            let d = separate(&bodies, 6);
            for (b, d) in bodies.iter_mut().zip(d) {
                b.position += d;
            }
        }
        let mut worst = 0.0f32;
        for i in 0..bodies.len() {
            for j in i + 1..bodies.len() {
                let d = bodies[i].position.distance(bodies[j].position);
                worst = worst.max(70.0 - d);
            }
        }
        assert!(worst < 3.0, "worst overlap {worst} cm");
    }

    #[test]
    fn different_heights_do_not_interact() {
        let b = [
            Body::new(Vec3::new(0.0, 0.0, 0.0), 35.0, 200.0),
            Body::new(Vec3::new(10.0, 500.0, 0.0), 35.0, 200.0),
        ];
        assert_eq!(separate(&b, 2), vec![Vec3::ZERO; 2]);
    }
}
