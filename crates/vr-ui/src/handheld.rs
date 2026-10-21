//! Things held in the off hand: the journal as an open book (quest rows from
//! `gothic_ui::journal::QuestRow`, page flips by a swipe of the other hand) and the map as a
//! sheet (markers placed with `gothic_ui::map::MapSheet::calc_uv`, the laser shows labels).
use crate::comfort;
use crate::panel::{Panel, wrap};
use glam::{Quat, Vec2, Vec3};
use gothic_ui::journal::QuestRow;
use gothic_ui::map::{MapMarker, MapSheet};
use gothic_vr::{Hand, Pose};

/// Open book: two pages side by side, m.
pub const PAGE_SIZE: Vec2 = Vec2::new(0.16, 0.22);
/// Lines per page at body text size from about 40 cm.
pub const LINES_PER_PAGE: usize = 14;
pub const LINE_CHARS: usize = 28;
/// Map sheet, m.
pub const MAP_SIZE: Vec2 = Vec2::new(0.3, 0.3);

/// The held object's pose: in front of the grip, tilted up towards the eyes like a book in
/// the hand (the face, +Z, looks up and back).
pub fn held_pose(grip: &Pose, hand: Hand, size: Vec2) -> Pose {
    let local = Pose::new(
        Vec3::new(hand.sign() * -size.x * 0.5, 0.06, -0.08),
        Quat::from_rotation_x(-0.9),
    );
    grip.mul(&local)
}

/// Text lines of the journal: each quest title, wrapped.
pub fn journal_lines(rows: &[QuestRow]) -> Vec<String> {
    rows.iter()
        .flat_map(|r| wrap(&r.title, LINE_CHARS))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    /// Left page index of the spread (even).
    pub spread: usize,
    pub lines: Vec<String>,
    /// Swipe tracking: the other hand's side of the spine last frame.
    last_side: Option<f32>,
}

impl Book {
    pub fn new(lines: Vec<String>) -> Self {
        Self {
            spread: 0,
            lines,
            last_side: None,
        }
    }

    pub fn pages(&self) -> usize {
        self.lines.len().div_ceil(LINES_PER_PAGE).max(1)
    }

    /// Lines of page `p`.
    pub fn page(&self, p: usize) -> &[String] {
        let a = (p * LINES_PER_PAGE).min(self.lines.len());
        let b = (a + LINES_PER_PAGE).min(self.lines.len());
        &self.lines[a..b]
    }

    /// The two pages (left, right) as panels around the spine at `pose`.
    pub fn panels(pose: &Pose) -> [Panel; 2] {
        let off = |x: f32| pose.mul(&Pose::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY));
        [
            Panel::new(off(-PAGE_SIZE.x * 0.5), PAGE_SIZE),
            Panel::new(off(PAGE_SIZE.x * 0.5), PAGE_SIZE),
        ]
    }

    /// Track the other hand; a swipe across the spine above the pages turns the spread
    /// (right to left = forward). Returns whether it turned.
    pub fn swipe(&mut self, pose: &Pose, other_hand: Vec3) -> bool {
        let local = pose.inverse().transform_point(other_hand);
        let over = local.y.abs() < PAGE_SIZE.y * 0.6 && (0.0..0.12).contains(&local.z);
        if !over || local.x.abs() > PAGE_SIZE.x {
            self.last_side = None;
            return false;
        }
        let side = if local.x > 0.03 {
            1.0
        } else if local.x < -0.03 {
            -1.0
        } else {
            return false;
        };
        match self.last_side.replace(side) {
            Some(prev) if prev > 0.0 && side < 0.0 => {
                let ok = self.spread + 2 < self.pages();
                if ok {
                    self.spread += 2;
                }
                ok
            }
            Some(prev) if prev < 0.0 && side > 0.0 => {
                let ok = self.spread >= 2;
                if ok {
                    self.spread -= 2;
                }
                ok
            }
            _ => false,
        }
    }
}

/// The held map: markers and the player on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldMap {
    pub sheet: MapSheet,
    /// (label, uv) of the visible markers on the sheet.
    pub markers: Vec<(String, Vec2)>,
    pub player: Option<Vec2>,
}

impl HeldMap {
    pub fn new(sheet: MapSheet, markers: &[MapMarker], player: Vec2) -> Self {
        let on = |p: Vec2| {
            sheet
                .contains(p.x, p.y)
                .then(|| sheet.calc_uv(p.x, p.y))
                .flatten()
        };
        Self {
            markers: markers
                .iter()
                .filter(|m| m.visible)
                .filter_map(|m| on(m.pos).map(|uv| (m.label.clone(), uv)))
                .collect(),
            player: on(player),
            sheet,
        }
    }

    /// The marker under the laser on the sheet panel (within 4 % of the sheet).
    pub fn label_at(&self, panel: &Panel, from: Vec3, dir: Vec3) -> Option<&str> {
        let uv = panel.ray(from, dir)?;
        self.markers
            .iter()
            .map(|(l, m)| (l, m.distance(uv)))
            .filter(|(_, d)| *d < 0.04)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l.as_str())
    }
}

/// Text height on a held page seen from `eye`, in dmm (for checks and tuning).
pub fn page_text_dmm(pose: &Pose, eye: Vec3) -> f32 {
    let line = PAGE_SIZE.y / LINES_PER_PAGE as f32 * 0.8;
    comfort::to_dmm(line, pose.position.distance(eye))
}
