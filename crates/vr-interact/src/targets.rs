//! Interaction targets the host registered near the hero and the queries on them.
//!
//! Positions are world metres in the same space as [`crate::Tracked`] poses. Entity ids are
//! the raw script ids the core uses (`Event::ObjectRegistered` / `ItemRegistered`; actor `n`
//! is focus `n + 1`).
use crate::lockpick::LockSpec;
use glam::Vec3;
use gothic_sim::types::ActorId;
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TargetKind {
    /// A living character: point + trigger talks (`Confirm`).
    Npc { actor: ActorId },
    /// A dead or unconscious body: grab loots everything (`QuickLoot`).
    Body { entity: u32 },
    /// A loose item: grab picks it up (`PickUp`).
    Item { entity: u32, template: String },
    /// An interact object (chest, door, work station, bed): grab the handle to use it
    /// (`UseInteractObject`), or play its lock first.
    Object { entity: u32, lock: Option<LockSpec> },
    /// A recipe on a work station's board: point + trigger crafts it (`Craft`).
    Recipe { recipe: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub kind: TargetKind,
    /// Centre for pointing.
    pub pos: Vec3,
    /// Pointing / grabbing radius, m.
    pub radius: f32,
    /// Where a hand grabs it (door handle, chest lid); `pos` when `None`.
    pub handle: Option<Vec3>,
}

impl Target {
    pub fn new(kind: TargetKind, pos: Vec3, radius: f32) -> Self {
        Self {
            kind,
            pos,
            radius,
            handle: None,
        }
    }

    pub fn with_handle(mut self, handle: Vec3) -> Self {
        self.handle = Some(handle);
        self
    }

    pub fn grab_point(&self) -> Vec3 {
        self.handle.unwrap_or(self.pos)
    }

    /// The raw script entity id the confirm key focuses.
    pub fn focus_id(&self) -> Option<u32> {
        match &self.kind {
            TargetKind::Npc { actor } => Some(actor.0 + 1),
            TargetKind::Body { entity }
            | TargetKind::Item { entity, .. }
            | TargetKind::Object { entity, .. } => Some(*entity),
            TargetKind::Recipe { .. } => None,
        }
    }
}

/// Distance along the ray and miss distance of `pos` for a ray from `from` along `dir`.
fn ray_offset(from: Vec3, dir: Vec3, pos: Vec3) -> (f32, f32) {
    let d = pos - from;
    let along = d.dot(dir);
    (along, (d - dir * along).length())
}

/// The target the ray from `hand` (its -Z) points at: the nearest along the ray whose sphere,
/// widened by `slack` radians of aim tolerance, the ray passes, within `range` metres.
pub fn pointed<'a>(
    targets: &'a [Target],
    hand: &Pose,
    range: f32,
    slack: f32,
    filter: impl Fn(&Target) -> bool,
) -> Option<&'a Target> {
    let dir = hand.forward().normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    targets
        .iter()
        .filter(|t| filter(t))
        .filter_map(|t| {
            let (along, miss) = ray_offset(hand.position, dir, t.pos);
            (along > 0.0 && along <= range && miss <= t.radius + along * slack.tan())
                .then_some((along, t))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

/// The target whose grab point is within `reach` (plus its radius) of the hand, nearest first.
pub fn grabbable(targets: &[Target], hand: Vec3, reach: f32) -> Option<&Target> {
    targets
        .iter()
        .filter(|t| !matches!(t.kind, TargetKind::Npc { .. } | TargetKind::Recipe { .. }))
        .map(|t| (t.grab_point().distance(hand) - t.radius, t))
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

/// The character aimed at along `dir` from `from` (spells, arrows).
pub fn aimed_actor(
    targets: &[Target],
    from: Vec3,
    dir: Vec3,
    range: f32,
    cone: f32,
) -> Option<ActorId> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    targets
        .iter()
        .filter_map(|t| match t.kind {
            TargetKind::Npc { actor } => {
                let (along, miss) = ray_offset(from, dir, t.pos);
                (along > 0.0 && along <= range && miss <= t.radius + along * cone.tan())
                    .then_some((miss / along.max(0.1), actor))
            }
            _ => None,
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, a)| a)
}
