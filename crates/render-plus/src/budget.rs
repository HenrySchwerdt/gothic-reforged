//! Frame-time governor: drops effects while the smoothed frame time is over budget and
//! brings them back when the measured saving fits again. Hysteresis on both sides, so it
//! does not oscillate around the target.
use crate::profile::{Budget, Effect, RenderProfile};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    Dropped(Effect),
    Restored(Effect),
}

#[derive(Debug, Clone)]
pub struct Governor {
    /// Exponential moving average of the frame time (ms).
    pub smoothed: f32,
    /// Dropped effects, newest last: (effect, smoothed frame time before the drop, measured
    /// saving once the drop has settled).
    dropped: Vec<(Effect, f32, Option<f32>)>,
    /// Frames to wait after a change before judging again (new pipelines compile, history
    /// buffers fill).
    settle: u32,
    over: u32,
    under: u32,
    warm: u32,
}

/// EMA weight of a new frame.
const ALPHA: f32 = 0.05;
/// Frames after a change before the next decision.
pub const SETTLE_FRAMES: u32 = 90;
/// Frames over budget (+10 %) before dropping (a fine frame takes two off the count).
pub const OVER_FRAMES: u32 = 30;
/// Consecutive frames with room before restoring.
pub const UNDER_FRAMES: u32 = 180;

impl Default for Governor {
    fn default() -> Self {
        Self {
            smoothed: 0.0,
            dropped: Vec::new(),
            settle: SETTLE_FRAMES,
            over: 0,
            under: 0,
            warm: 0,
        }
    }
}

impl Governor {
    pub fn suppressed(&self) -> Vec<Effect> {
        self.dropped.iter().map(|d| d.0).collect()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Feed one frame time; returns the change to apply, if any.
    pub fn update(&mut self, profile: &RenderProfile, frame_ms: f32) -> Option<Change> {
        let budget: &Budget = &profile.budget;
        if !budget.enabled {
            if self.dropped.is_empty() {
                return None;
            }
            let (e, ..) = self.dropped.pop()?;
            return Some(Change::Restored(e));
        }
        // An effect the user switched off since it was dropped is no longer ours to restore.
        self.dropped.retain(|(e, ..)| profile.is_on(*e));
        if !frame_ms.is_finite() || frame_ms <= 0.0 {
            return None;
        }
        self.smoothed = if self.warm == 0 {
            frame_ms
        } else {
            self.smoothed + (frame_ms - self.smoothed) * ALPHA
        };
        self.warm = self.warm.saturating_add(1);
        if self.settle > 0 {
            self.settle -= 1;
            return None;
        }
        if let Some(last) = self.dropped.last_mut()
            && last.2.is_none()
        {
            last.2 = Some((last.1 - self.smoothed).max(0.0));
        }
        let target = budget.target_ms.max(0.1);
        // Over budget only counts while the raw frames are over too: the average stays high
        // for a while after a hitch (shader compile, streaming) without the scene being heavy.
        if self.smoothed > target * 1.1 && frame_ms > target * 1.1 {
            self.over += 1;
            self.under = 0;
        } else {
            self.over = self.over.saturating_sub(2);
            // Room for the newest dropped effect: with what it cost back on, the frame must
            // stay under 90 % of the target.
            let room = match self.dropped.last() {
                Some(&(_, _, Some(cost))) => self.smoothed + cost < target * 0.9,
                _ => false,
            };
            self.under = if room { self.under + 1 } else { 0 };
        }
        if self.over >= OVER_FRAMES {
            let next = budget
                .shed_order
                .iter()
                .copied()
                .find(|&e| profile.is_on(e) && !self.dropped.iter().any(|d| d.0 == e))?;
            self.dropped.push((next, self.smoothed, None));
            self.changed();
            return Some(Change::Dropped(next));
        }
        if self.under >= UNDER_FRAMES {
            let (e, ..) = self.dropped.pop()?;
            self.changed();
            return Some(Change::Restored(e));
        }
        None
    }

    fn changed(&mut self) {
        self.settle = SETTLE_FRAMES;
        self.over = 0;
        self.under = 0;
    }
}
