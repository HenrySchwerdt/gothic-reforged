//! Picking the frame to show for a playback time (frames late by more than one frame are
//! dropped, early ones wait).

use crate::decode::Frame;

#[derive(Debug, Default)]
pub struct FrameSync {
    pending: Option<Frame>,
    /// Frames shown / skipped because they were already late.
    pub shown: u64,
    pub dropped: u64,
}

impl FrameSync {
    /// The newest frame due at time `t` (seconds), pulling from `next` as needed. `None` if
    /// the frame on screen is still current.
    pub fn advance(&mut self, t: f64, mut next: impl FnMut() -> Option<Frame>) -> Option<Frame> {
        let mut show: Option<Frame> = None;
        loop {
            let f = match self.pending.take() {
                Some(f) => f,
                None => match next() {
                    Some(f) => f,
                    None => break,
                },
            };
            if f.pts <= t {
                if show.replace(f).is_some() {
                    self.dropped += 1;
                }
            } else {
                self.pending = Some(f);
                break;
            }
        }
        if show.is_some() {
            self.shown += 1;
        }
        show
    }

    /// A frame is waiting for its time.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: u64, fps: f64) -> impl FnMut() -> Option<Frame> {
        let mut i = 0;
        move || {
            (i < n).then(|| {
                i += 1;
                Frame {
                    index: i - 1,
                    pts: (i - 1) as f64 / fps,
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                }
            })
        }
    }

    #[test]
    fn shows_due_frames_and_drops_late_ones() {
        let mut src = frames(10, 10.0);
        let mut s = FrameSync::default();
        assert_eq!(s.advance(0.0, &mut src).unwrap().index, 0);
        assert!(s.advance(0.05, &mut src).is_none(), "frame 1 is due at 0.1");
        assert_eq!(s.advance(0.1, &mut src).unwrap().index, 1);
        // A hitch: at 0.45 frames 2, 3 and 4 are due; 4 is shown, 2 and 3 dropped.
        assert_eq!(s.advance(0.45, &mut src).unwrap().index, 4);
        assert_eq!(s.dropped, 2);
        assert_eq!(s.advance(10.0, &mut src).unwrap().index, 9);
        assert!(s.advance(11.0, &mut src).is_none());
        assert!(!s.has_pending());
    }
}
