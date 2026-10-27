//! Backpack inventory: reach over the shoulder and grip to open the inventory panel at
//! touching distance in front, a grid of the hero's stacks (`gothic_ui::PlayerPanel`), paged.
use crate::comfort;
use crate::panel::{Panel, cell_at};
use glam::{Vec2, Vec3};
use gothic_ui::{PageRow, PlayerPanel};
use gothic_vr::Pose;

pub const COLS: usize = 5;
pub const ROWS: usize = 4;
/// Cell edge, m (6 cm: ~100 dmm at touching distance).
pub const CELL: f32 = 0.06;
/// Height of the paging strip under the grid, m.
pub const STRIP: f32 = 0.05;
const PANEL_DISTANCE: f32 = 0.45;
const PANEL_PITCH: f32 = 25.0 * std::f32::consts::PI / 180.0;

/// The hand is behind the head and around shoulder height: reaching into the backpack.
pub fn over_shoulder(head: &Pose, hand: Vec3) -> bool {
    let yaw = head.yaw();
    let back = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let d = hand - head.position;
    d.dot(back) > 0.05 && (-0.35..=0.25).contains(&d.y) && d.length() < 0.5
}

/// What a press on the panel hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelHit {
    /// A stack: its `PageRow::key`.
    Item(usize),
    PrevPage,
    NextPage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Backpack {
    pub panel: Panel,
    pub page: usize,
    pub hovered: Option<PanelHit>,
    /// What the fingertip touches (a touch presses once on arrival).
    pub touching: Option<PanelHit>,
}

impl Backpack {
    /// Open in front of the head, below the eye line, facing the eyes.
    pub fn open(head: &Pose) -> Self {
        let pos = comfort::in_front(head, PANEL_DISTANCE, PANEL_PITCH);
        let size = Vec2::new(COLS as f32 * CELL, ROWS as f32 * CELL + STRIP);
        Self {
            panel: Panel::new(Pose::new(pos, comfort::facing(pos, head.position)), size),
            page: 0,
            hovered: None,
            touching: None,
        }
    }

    pub fn pages(rows: &[PageRow]) -> usize {
        rows.len().div_ceil(COLS * ROWS).max(1)
    }

    /// The stacks on the current page with their grid cells.
    pub fn visible<'a>(&self, inv: &'a PlayerPanel) -> impl Iterator<Item = (usize, &'a PageRow)> {
        inv.rows
            .iter()
            .skip(self.page * COLS * ROWS)
            .take(COLS * ROWS)
            .enumerate()
    }

    /// Centre of cell `i` (row-major) on the panel.
    pub fn cell_center(&self, i: usize) -> Vec3 {
        let gh = ROWS as f32 * CELL / self.panel.size.y;
        let uv = Vec2::new(
            ((i % COLS) as f32 + 0.5) / COLS as f32,
            ((i / COLS) as f32 + 0.5) / ROWS as f32 * gh,
        );
        self.panel.point(uv)
    }

    /// What is under a panel coordinate.
    pub fn hit(&self, uv: Vec2, inv: &PlayerPanel) -> Option<PanelHit> {
        let gh = ROWS as f32 * CELL / self.panel.size.y;
        if uv.y >= gh {
            // Paging strip: left half back, right half forward.
            return (uv.y <= 1.0).then_some(if uv.x < 0.5 {
                PanelHit::PrevPage
            } else {
                PanelHit::NextPage
            });
        }
        let (c, r) = cell_at(Vec2::new(uv.x, uv.y / gh), COLS, ROWS)?;
        self.visible(inv)
            .nth(r * COLS + c)
            .map(|(_, row)| PanelHit::Item(row.key))
    }

    /// Apply a press: paging is handled here, an item is returned to the host.
    pub fn press(&mut self, hit: PanelHit, inv: &PlayerPanel) -> Option<usize> {
        match hit {
            PanelHit::PrevPage => {
                self.page = self.page.saturating_sub(1);
                None
            }
            PanelHit::NextPage => {
                self.page = (self.page + 1).min(Self::pages(&inv.rows) - 1);
                None
            }
            PanelHit::Item(key) => Some(key),
        }
    }
}
