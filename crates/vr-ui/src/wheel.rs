//! Quick-slot wheel: press and hold, the slots (`gothic_ui::QuickSlots`, ten by default)
//! appear on a ring around where the hand was, in the plane facing the eyes; move the hand
//! towards a slot and release to use it. Pressed at the hip it is the belt.
use crate::comfort;
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};

/// Ring radius, m (a 10-slot ring keeps targets ~4.7 cm apart, far above the touch minimum
/// at arm's length).
pub const WHEEL_RADIUS: f32 = 0.12;
/// Inside this distance from the centre nothing is selected (release cancels).
pub const DEADZONE: f32 = 0.045;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickWheel {
    /// Centre and orientation (+Z towards the eyes), `None` while closed.
    pub open: Option<Pose>,
    pub slots: usize,
    pub hovered: Option<usize>,
    /// Opened at the hip.
    pub belt: bool,
}

impl QuickWheel {
    pub fn new(slots: usize) -> Self {
        Self {
            open: None,
            slots,
            hovered: None,
            belt: false,
        }
    }

    /// Open at the hand, facing the eyes.
    pub fn open_at(&mut self, hand: Vec3, head: &Pose) {
        let hip = head.position.y - hand.y > 0.55;
        self.belt = hip;
        self.open = Some(Pose::new(hand, comfort::facing(hand, head.position)));
        self.hovered = None;
    }

    /// World position of slot `i`: slot 0 on top, clockwise as seen by the player.
    pub fn slot_position(&self, i: usize) -> Option<Vec3> {
        let c = self.open?;
        let a = i as f32 / self.slots.max(1) as f32 * std::f32::consts::TAU;
        Some(c.transform_point(Vec3::new(a.sin(), a.cos(), 0.0) * WHEEL_RADIUS))
    }

    /// Track the hand; returns the hovered slot.
    pub fn update(&mut self, hand: Vec3) -> Option<usize> {
        let c = self.open?;
        let local = c.inverse().transform_point(hand);
        let flat = Vec3::new(local.x, local.y, 0.0);
        self.hovered = (flat.length() >= DEADZONE && self.slots > 0).then(|| {
            // Angle from the top, clockwise.
            let a = local.x.atan2(local.y).rem_euclid(std::f32::consts::TAU);
            (a / std::f32::consts::TAU * self.slots as f32).round() as usize % self.slots
        });
        self.hovered
    }

    /// Release: the chosen slot (if any) and close.
    pub fn release(&mut self) -> Option<usize> {
        self.open = None;
        self.hovered.take()
    }

    /// Rotation of the ring (for drawing).
    pub fn rotation(&self) -> Quat {
        self.open.map_or(Quat::IDENTITY, |p| p.rotation)
    }
}
