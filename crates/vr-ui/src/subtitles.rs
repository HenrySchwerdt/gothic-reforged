//! World-locked subtitles near the speaker, from `gothic_ui`'s [`Subtitle`] state (text,
//! fade). Inside the comfortable view the line hangs above the speaker's head facing the
//! player; when the speaker is out of view it falls back to a lazily following line in front
//! with a marker on the speaker's side.
use crate::comfort;
use crate::panel::wrap;
use glam::Vec3;
use gothic_ui::Subtitle;
use gothic_vr::Pose;

/// Lines of at most this many characters.
pub const LINE_CHARS: usize = 42;
/// Fallback distance in front, m.
pub const FALLBACK_DISTANCE: f32 = 1.4;
/// Speakers further than this off the view direction use the fallback, radians.
pub const OFF_VIEW: f32 = 50.0 * std::f32::consts::PI / 180.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleView {
    pub pose: Pose,
    /// Text height, m (body text size at that distance).
    pub text_height: f32,
    pub lines: Vec<String>,
    pub alpha: f32,
    /// Speaker out of view: on which side.
    pub off_view: Option<Side>,
}

/// Lay out the current line, `None` while nothing is shown.
pub fn layout(sub: &Subtitle, speaker_head: Option<Vec3>, head: &Pose) -> Option<SubtitleView> {
    if !sub.visible() || sub.text.is_empty() {
        return None;
    }
    let lines = wrap(&sub.text, LINE_CHARS);
    let eye = head.position;
    let (pos, off_view) = match speaker_head {
        Some(s) => {
            let (yaw, _) = comfort::view_angles(head, s);
            if yaw.abs() <= OFF_VIEW {
                // Above the head, a little towards the player so it never clips the speaker.
                let raw = s + Vec3::Y * 0.3 + (eye - s).normalize_or_zero() * 0.15;
                (
                    comfort::clamp_distance(eye, raw, &comfort::POINT_DISTANCE),
                    None,
                )
            } else {
                let side = if yaw > 0.0 { Side::Left } else { Side::Right };
                (comfort::in_front(head, FALLBACK_DISTANCE, 0.25), Some(side))
            }
        }
        // Narrator / off-screen voice.
        None => (comfort::in_front(head, FALLBACK_DISTANCE, 0.25), None),
    };
    let d = pos.distance(eye);
    Some(SubtitleView {
        pose: Pose::new(pos, comfort::facing(pos, eye)),
        text_height: comfort::dmm(comfort::BODY_TEXT_DMM, d),
        lines,
        alpha: sub.alpha,
        off_view,
    })
}
