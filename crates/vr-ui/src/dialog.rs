//! Dialog options floating next to the speaking character: a column of rows beside the NPC's
//! head, facing the player, at laser distance; the selection logic is `gothic_ui`'s
//! [`OptionList`] (visible window, scrolling), driven by the laser or a fingertip.
use crate::comfort;
use crate::panel::{Panel, row_at};
use glam::{Vec2, Vec3};
use gothic_ui::OptionList;
use gothic_vr::{Hand, Pose};

/// Rows shown at once (the rest scroll).
pub const VISIBLE_ROWS: usize = 6;
/// Panel width, dmm (about 30 characters of body text per row).
pub const WIDTH_DMM: f32 = 380.0;
/// Row height, dmm (over the minimum target size so rows are easy to hit with the laser).
pub const ROW_DMM: f32 = 36.0;

#[derive(Debug, Clone)]
pub struct DialogPanel {
    pub panel: Panel,
    pub list: OptionList,
    pub options: Vec<String>,
    /// Row (absolute index) under the laser or finger.
    pub hovered: Option<usize>,
}

/// Where the options go: beside the NPC's head on the player's `side` (clear of the body by
/// half the panel width), a little lower and pulled towards the player so the panel never
/// cuts into the speaker, kept inside the laser distance range and facing the eyes.
pub fn place(npc_head: Vec3, head: &Pose, side: Hand) -> Pose {
    let eye = head.position;
    let to_npc = Vec3::new(npc_head.x - eye.x, 0.0, npc_head.z - eye.z).normalize_or(Vec3::NEG_Z);
    let right = Vec3::new(-to_npc.z, 0.0, to_npc.x);
    let half = comfort::dmm(WIDTH_DMM, eye.distance(npc_head)) * 0.5;
    let raw = npc_head + right * side.sign() * (0.25 + half) - Vec3::Y * 0.2 - to_npc * 0.3;
    let pos = comfort::clamp_distance(eye, raw, &comfort::POINT_DISTANCE);
    Pose::new(pos, comfort::facing(pos, eye))
}

impl DialogPanel {
    pub fn new(options: Vec<String>, npc_head: Vec3, head: &Pose, side: Hand) -> Self {
        let pose = place(npc_head, head, side);
        let d = pose.position.distance(head.position);
        let rows = options.len().clamp(1, VISIBLE_ROWS);
        let mut list = OptionList::new(VISIBLE_ROWS);
        list.reset(options.len());
        Self {
            panel: Panel::new(
                pose,
                Vec2::new(
                    comfort::dmm(WIDTH_DMM, d),
                    comfort::dmm(ROW_DMM, d) * rows as f32,
                ),
            ),
            list,
            options,
            hovered: None,
        }
    }

    /// Rows on show (absolute indices) in panel order.
    pub fn rows(&self) -> std::ops::Range<usize> {
        self.list.visible_rows()
    }

    /// Centre of visible row `k` (0 = top).
    pub fn row_center(&self, k: usize) -> Vec3 {
        let n = self.rows().len().max(1);
        self.panel
            .point(Vec2::new(0.5, (k as f32 + 0.5) / n as f32))
    }

    fn row_of(&self, uv: Vec2) -> Option<usize> {
        let rows = self.rows();
        row_at(uv, rows.len()).map(|k| rows.start + k)
    }

    /// Laser hover; the hovered row becomes the list's selection.
    pub fn point(&mut self, from: Vec3, dir: Vec3) -> Option<usize> {
        self.hovered = self.panel.ray(from, dir).and_then(|uv| self.row_of(uv));
        if let Some(i) = self.hovered {
            self.list.select(i);
        }
        self.hovered
    }

    /// A fingertip pressing a row chooses it at once.
    pub fn touch(&mut self, tip: Vec3) -> Option<usize> {
        let i = self
            .panel
            .touch(tip, 0.015)
            .and_then(|uv| self.row_of(uv))?;
        self.list.select(i);
        Some(i)
    }

    /// Thumbstick scroll (rows, positive = down) without moving the selection.
    pub fn scroll(&mut self, rows: i32) {
        self.list.scroll(rows);
    }

    /// The trigger: the selected option index (for `ChooseOption`).
    pub fn confirm(&self) -> Option<usize> {
        self.hovered.and(self.list.selected())
    }
}
