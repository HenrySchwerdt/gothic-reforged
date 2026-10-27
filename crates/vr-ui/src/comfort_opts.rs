//! The VR comfort options the options page edits. The values are plain data (`Serialize`), the
//! locomotion code of the Quest app reads `snap_turn` / `snap_degrees` / `vignette` / `height_cm`,
//! the HUD reads `right_handed` (main hand) and `subtitles`. Nothing here touches the game's rules.
use gothic_vr::Hand;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VrComfortOptions {
    /// Snap turning (true) or smooth turning (false).
    pub snap_turn: bool,
    /// Snap step, degrees (15, 30, 45, 60).
    pub snap_degrees: f32,
    /// Comfort vignette while moving or turning.
    pub vignette: bool,
    /// Vignette strength 0..1 (steps of 0.25).
    pub vignette_strength: f32,
    /// Standing height correction, centimetres (-30..=30).
    pub height_cm: i32,
    /// Main hand: right (true) or left.
    pub right_handed: bool,
    /// Show subtitles at the speaker.
    pub subtitles: bool,
}

impl Default for VrComfortOptions {
    fn default() -> Self {
        Self {
            snap_turn: true,
            snap_degrees: 30.0,
            vignette: true,
            vignette_strength: 0.5,
            height_cm: 0,
            right_handed: true,
            subtitles: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptId {
    TurnMode,
    SnapDegrees,
    Vignette,
    VignetteStrength,
    Height,
    Handedness,
    Subtitles,
}

impl OptId {
    pub const ALL: [OptId; 7] = [
        OptId::TurnMode,
        OptId::SnapDegrees,
        OptId::Vignette,
        OptId::VignetteStrength,
        OptId::Height,
        OptId::Handedness,
        OptId::Subtitles,
    ];
}

impl VrComfortOptions {
    /// Step an option: `delta` is -1 / +1 (toggles flip on either).
    pub fn apply(&mut self, id: OptId, delta: i32) {
        match id {
            OptId::TurnMode => self.snap_turn = !self.snap_turn,
            OptId::SnapDegrees => {
                const STEPS: [f32; 4] = [15.0, 30.0, 45.0, 60.0];
                let i = STEPS.iter().position(|d| *d == self.snap_degrees).unwrap_or(1) as i32;
                self.snap_degrees = STEPS[(i + delta).clamp(0, 3) as usize];
            }
            OptId::Vignette => self.vignette = !self.vignette,
            OptId::VignetteStrength => {
                self.vignette_strength = (self.vignette_strength + 0.25 * delta as f32).clamp(0.0, 1.0)
            }
            OptId::Height => self.height_cm = (self.height_cm + 5 * delta).clamp(-30, 30),
            OptId::Handedness => self.right_handed = !self.right_handed,
            OptId::Subtitles => self.subtitles = !self.subtitles,
        }
    }

    pub fn main_hand(&self) -> Hand {
        if self.right_handed { Hand::Right } else { Hand::Left }
    }

    /// Value text of an option for the page.
    pub fn value(&self, id: OptId, l: &crate::views::Labels) -> String {
        let onoff = |b: bool| if b { l.on.clone() } else { l.off.clone() };
        match id {
            OptId::TurnMode => if self.snap_turn { l.snap.clone() } else { l.smooth.clone() },
            OptId::SnapDegrees => format!("{}°", self.snap_degrees as i32),
            OptId::Vignette => onoff(self.vignette),
            OptId::VignetteStrength => format!("{} %", (self.vignette_strength * 100.0).round() as i32),
            OptId::Height => format!("{:+} cm", self.height_cm),
            OptId::Handedness => if self.right_handed { l.right.clone() } else { l.left.clone() },
            OptId::Subtitles => onoff(self.subtitles),
        }
    }

    /// Whether the option has any effect right now (snap degrees only with snap turning, the
    /// strength only with the vignette on).
    pub fn active(&self, id: OptId) -> bool {
        match id {
            OptId::SnapDegrees => self.snap_turn,
            OptId::VignetteStrength => self.vignette,
            _ => true,
        }
    }
}
