//! World-space text tags: the focus name above the thing you look or point at, and the floating
//! texts (XP gain, damage) that rise from where they happened. The original draws the focus name
//! as the only text over the crosshair target (`DrawFocusName`, see `gothic_ui::focusname`) and
//! floating texts as `gCTextEffect` rows; in VR both live *in the world*, never head-locked
//! (Meta: no overlay that is "behind" scene objects, reticles and names drawn on the target).
//!
//! Pure layout: [`Tag`]s with a pose, text height and alpha. The renderer draws them.
use crate::comfort;
use glam::Vec3;
use gothic_ui::FocusAttitude;
use gothic_vr::Pose;

/// Smallest comfortable name height, dmm, and the largest (a name never grows with distance).
pub const NAME_DMM: f32 = 24.0;
pub const NAME_MAX_M: f32 = 0.16;

#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub text: String,
    /// sRGB colour.
    pub color: [f32; 3],
    pub pose: Pose,
    /// Text height, m.
    pub height: f32,
    pub alpha: f32,
    /// Health bar under the name, 0..1.
    pub bar: Option<f32>,
}

/// What the host knows about the focused character.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusData {
    pub name: String,
    pub attitude: FocusAttitude,
    /// Health fraction when it is an enemy in view of the fight.
    pub health: Option<f32>,
    /// The character's head, world metres.
    pub head: Vec3,
}

/// The attitude colours of the original (hostile red sampled from the shipped game).
pub fn attitude_rgb(a: FocusAttitude) -> [f32; 3] {
    let u = |r: u8, g: u8, b: u8| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
    match a {
        FocusAttitude::Hostile => u(153, 23, 18),
        FocusAttitude::Neutral => u(224, 213, 178),
        FocusAttitude::Friendly => u(164, 214, 120),
    }
}

/// The name tag above the head, facing the eyes upright, `NAME_DMM` high at its distance (but at
/// most `NAME_MAX_M` so it does not balloon next to the player). `alpha` is the host's fade.
pub fn focus_tag(f: &FocusData, eye: &Pose, alpha: f32) -> Tag {
    let above = f.head + Vec3::Y * 0.28;
    let d = eye.position.distance(above).max(0.5);
    Tag {
        text: f.name.clone(),
        color: attitude_rgb(f.attitude),
        pose: Pose::new(above, comfort::facing(above, eye.position)),
        height: comfort::dmm(NAME_DMM, d).min(NAME_MAX_M),
        alpha,
        bar: f.health,
    }
}

/// One rising text.
#[derive(Debug, Clone, PartialEq)]
pub struct Floater {
    pub text: String,
    pub color: [f32; 3],
    pub origin: Vec3,
    pub age: f32,
    pub life: f32,
}

/// Rising speed, m/s.
pub const RISE: f32 = 0.35;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Floaters {
    pub items: Vec<Floater>,
}

impl Floaters {
    /// `Event::XpGained` / hits: a text at a world position (the hero's chest for XP).
    pub fn push(&mut self, text: impl Into<String>, color: [f32; 3], at: Vec3, life: f32) {
        // At most six at a time: the oldest goes first.
        if self.items.len() >= 6 {
            self.items.remove(0);
        }
        self.items.push(Floater {
            text: text.into(),
            color,
            origin: at,
            age: 0.0,
            life: life.max(0.2),
        });
    }

    pub fn tick(&mut self, dt: f32) {
        for f in &mut self.items {
            f.age += dt;
        }
        self.items.retain(|f| f.age < f.life);
    }

    /// Tags for the renderer: rising, fading in the last third, facing the eyes.
    pub fn tags(&self, eye: &Pose) -> Vec<Tag> {
        self.items
            .iter()
            .map(|f| {
                let at = f.origin + Vec3::Y * (RISE * f.age);
                let left = 1.0 - f.age / f.life;
                let d = eye.position.distance(at).max(0.5);
                Tag {
                    text: f.text.clone(),
                    color: f.color,
                    pose: Pose::new(at, comfort::facing(at, eye.position)),
                    height: comfort::dmm(NAME_DMM * 0.9, d).min(NAME_MAX_M),
                    alpha: (left * 3.0).clamp(0.0, 1.0),
                    bar: None,
                }
            })
            .collect()
    }
}
