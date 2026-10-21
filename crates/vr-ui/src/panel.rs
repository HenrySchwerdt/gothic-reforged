//! A flat panel in the world and the hit tests on it.
use glam::{Vec2, Vec3};
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};

/// A rectangle in the world: centred on `pose.position`, local X right, Y up, the front faces
/// local +Z. Panel coordinates (`uv`) run 0..1 left to right and top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub pose: Pose,
    /// Width, height in metres.
    pub size: Vec2,
}

impl Panel {
    pub fn new(pose: Pose, size: Vec2) -> Self {
        Self { pose, size }
    }

    pub fn normal(&self) -> Vec3 {
        self.pose.rotation * Vec3::Z
    }

    /// World position of a panel coordinate.
    pub fn point(&self, uv: Vec2) -> Vec3 {
        self.pose.transform_point(Vec3::new(
            (uv.x - 0.5) * self.size.x,
            (0.5 - uv.y) * self.size.y,
            0.0,
        ))
    }

    /// Panel coordinate of a world point projected onto the panel (may be outside 0..1) and
    /// its height above the front face.
    pub fn project(&self, p: Vec3) -> (Vec2, f32) {
        let local = self.pose.inverse().transform_point(p);
        (
            Vec2::new(local.x / self.size.x + 0.5, 0.5 - local.y / self.size.y),
            local.z,
        )
    }

    fn inside(uv: Vec2) -> bool {
        (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)
    }

    /// Where a ray from `from` along `dir` hits the front of the panel.
    pub fn ray(&self, from: Vec3, dir: Vec3) -> Option<Vec2> {
        let n = self.normal();
        let denom = dir.dot(n);
        // Only from the front, never along the panel.
        if denom > -1e-4 {
            return None;
        }
        let t = (self.pose.position - from).dot(n) / denom;
        if t < 0.0 {
            return None;
        }
        let (uv, _) = self.project(from + dir * t);
        Self::inside(uv).then_some(uv)
    }

    /// A fingertip touching the panel: within `depth` in front of (or slightly behind) it.
    pub fn touch(&self, tip: Vec3, depth: f32) -> Option<Vec2> {
        let (uv, h) = self.project(tip);
        (Self::inside(uv) && (-0.02..=depth).contains(&h)).then_some(uv)
    }
}

/// Row `0..n` under `uv.y` for `n` equal rows filling the panel height.
pub fn row_at(uv: Vec2, n: usize) -> Option<usize> {
    if n == 0 || !(0.0..1.0).contains(&uv.y) {
        return None;
    }
    Some(((uv.y * n as f32) as usize).min(n - 1))
}

/// Cell (column, row) of a `cols` x `rows` grid filling the panel.
pub fn cell_at(uv: Vec2, cols: usize, rows: usize) -> Option<(usize, usize)> {
    if cols == 0 || rows == 0 || !(0.0..1.0).contains(&uv.x) || !(0.0..1.0).contains(&uv.y) {
        return None;
    }
    Some((
        ((uv.x * cols as f32) as usize).min(cols - 1),
        ((uv.y * rows as f32) as usize).min(rows - 1),
    ))
}

/// Break `text` into lines of at most `max_chars` characters at spaces (long words are cut).
pub fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > max {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let cut: String = word.chars().take(max).collect();
            word = word.chars().skip(max).collect();
            lines.push(cut);
        }
        let need = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
        if need > max && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
