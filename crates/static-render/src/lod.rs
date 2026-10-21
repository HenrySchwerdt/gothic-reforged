//! Level-of-detail ranges: each level is drawn from the previous level's end distance to its
//! own, with a cross-fade margin.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LodRanges {
    /// End distance of each level, increasing; the last one is the draw distance.
    pub ends: Vec<f32>,
    /// Cross-fade width (world units) at each switch.
    pub margin: f32,
}

impl LodRanges {
    pub fn new(ends: Vec<f32>, margin: f32) -> Self {
        let mut ends = ends;
        ends.retain(|e| e.is_finite() && *e > 0.0);
        ends.sort_by(f32::total_cmp);
        ends.dedup();
        Self {
            ends,
            margin: margin.max(0.0),
        }
    }

    pub fn levels(&self) -> usize {
        self.ends.len()
    }

    pub fn draw_distance(&self) -> f32 {
        self.ends.last().copied().unwrap_or(0.0)
    }

    /// The level drawn at `distance` (`None` beyond the draw distance).
    pub fn level_at(&self, distance: f32) -> Option<usize> {
        self.ends.iter().position(|e| distance < *e)
    }

    /// `(start_margin, end_margin)` of level `i` as Bevy `VisibilityRange` wants them: the
    /// level fades in over `start_margin` and out over `end_margin`.
    pub fn visibility_range(&self, i: usize) -> (std::ops::Range<f32>, std::ops::Range<f32>) {
        let h = self.margin * 0.5;
        let start = if i == 0 { 0.0 } else { self.ends[i - 1] };
        let end = self.ends[i];
        let s = if i == 0 {
            0.0..0.0
        } else {
            (start - h).max(0.0)..start + h
        };
        let e = if i + 1 == self.ends.len() {
            end..end
        } else {
            end - h..end + h
        };
        (s, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_ranges() {
        let l = LodRanges::new(vec![120.0, 30.0, 60.0], 4.0);
        assert_eq!(l.ends, [30.0, 60.0, 120.0]);
        assert_eq!(l.level_at(0.0), Some(0));
        assert_eq!(l.level_at(45.0), Some(1));
        assert_eq!(l.level_at(119.0), Some(2));
        assert_eq!(l.level_at(120.0), None);
        assert_eq!(l.visibility_range(0), (0.0..0.0, 28.0..32.0));
        assert_eq!(l.visibility_range(1), (28.0..32.0, 58.0..62.0));
        assert_eq!(l.visibility_range(2), (58.0..62.0, 120.0..120.0));
    }
}
