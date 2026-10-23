//! Procedural test props (no game assets): weapons and boxes described by simple shapes,
//! masses and grip points.
//!
//! Weapon frame convention: the primary grip sits at the origin with the identity rotation,
//! i.e. a weapon held there has its frame equal to the hand's grip frame. A blade points along
//! -Z (out of the thumb side of the fist, the OpenXR grip-pose convention), the pommel along
//! +Z; an axe or blade edge faces +Y ("up" when the hand is level); a bow's own frame has the stave along
//! Y and shoots toward -Z, but its grip rotation turns that into the fist frame (stave along the fist tube).
use crate::grab::{GripKind, GripPoint};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PartShape {
    /// Half extents.
    Box(Vec3),
    /// Capsule along local Y: radius, half length of the cylinder part.
    Capsule { radius: f32, half_length: f32 },
}

impl PartShape {
    pub fn volume(&self) -> f32 {
        match *self {
            PartShape::Box(h) => 8.0 * h.x * h.y * h.z,
            PartShape::Capsule {
                radius,
                half_length,
            } => {
                std::f32::consts::PI * radius * radius * (2.0 * half_length)
                    + 4.0 / 3.0 * std::f32::consts::PI * radius.powi(3)
            }
        }
    }
}

/// One collider part of a prop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropPart {
    pub shape: PartShape,
    pub offset: Vec3,
    pub rotation: Quat,
    /// Debug render colour (sRGB).
    pub color: [f32; 3],
}

/// What a prop is (other plugins attach their mechanics by kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropKind {
    Sword,
    Axe,
    TwoHandedSword,
    Mace,
    Bow,
    Arrow,
    Crate,
}

/// A prop definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropSpec {
    pub kind: PropKind,
    /// kg.
    pub mass: f32,
    pub parts: Vec<PropPart>,
    pub grips: Vec<GripPoint>,
    /// Objects without grip points can be grabbed anywhere within this distance of their
    /// centre (keeps the relative pose).
    pub free_grab_radius: Option<f32>,
}

impl PropSpec {
    pub fn volume(&self) -> f32 {
        self.parts.iter().map(|p| p.shape.volume()).sum()
    }

    /// Density that gives the parts the prop's mass.
    pub fn density(&self) -> f32 {
        let v = self.volume();
        if v > 0.0 { self.mass / v } else { 1.0 }
    }
}

const STEEL: [f32; 3] = [0.75, 0.77, 0.8];
const WOOD: [f32; 3] = [0.45, 0.3, 0.18];
const LEATHER: [f32; 3] = [0.3, 0.2, 0.15];

fn along_z(radius: f32, from: f32, to: f32, color: [f32; 3]) -> PropPart {
    let half = ((to - from).abs() / 2.0 - radius).max(0.001);
    PropPart {
        shape: PartShape::Capsule {
            radius,
            half_length: half,
        },
        offset: Vec3::new(0.0, 0.0, (from + to) / 2.0),
        rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        color,
    }
}

fn block(half: Vec3, offset: Vec3, color: [f32; 3]) -> PropPart {
    PropPart {
        shape: PartShape::Box(half),
        offset,
        rotation: Quat::IDENTITY,
        color,
    }
}

fn handle(from: f32, to: f32, kind: GripKind) -> GripPoint {
    GripPoint {
        a: Vec3::new(0.0, 0.0, from),
        b: Vec3::new(0.0, 0.0, to),
        rotation: Quat::IDENTITY,
        kind,
        radius: 0.09,
    }
}

/// One-handed sword: 1.4 kg, 0.95 m.
pub fn sword() -> PropSpec {
    PropSpec {
        kind: PropKind::Sword,
        mass: 1.4,
        parts: vec![
            along_z(0.018, 0.13, -0.02, LEATHER),
            block(
                Vec3::new(0.09, 0.015, 0.012),
                Vec3::new(0.0, 0.0, -0.035),
                STEEL,
            ),
            block(
                Vec3::new(0.004, 0.025, 0.38),
                Vec3::new(0.0, 0.0, -0.43),
                STEEL,
            ),
        ],
        grips: vec![
            handle(0.0, 0.0, GripKind::Primary),
            handle(0.03, 0.11, GripKind::Secondary),
        ],
        free_grab_radius: None,
    }
}

/// Bearded axe: 2.4 kg, head-heavy.
pub fn axe() -> PropSpec {
    PropSpec {
        kind: PropKind::Axe,
        mass: 2.4,
        parts: vec![
            along_z(0.02, 0.12, -0.62, WOOD),
            block(
                Vec3::new(0.012, 0.11, 0.07),
                Vec3::new(0.0, 0.07, -0.58),
                STEEL,
            ),
        ],
        grips: vec![
            handle(0.0, 0.0, GripKind::Primary),
            handle(0.02, -0.4, GripKind::Secondary),
        ],
        free_grab_radius: None,
    }
}

/// Two-handed sword: 3.6 kg, 1.4 m.
pub fn two_handed_sword() -> PropSpec {
    PropSpec {
        kind: PropKind::TwoHandedSword,
        mass: 3.6,
        parts: vec![
            along_z(0.02, 0.3, -0.02, LEATHER),
            block(
                Vec3::new(0.14, 0.02, 0.015),
                Vec3::new(0.0, 0.0, -0.04),
                STEEL,
            ),
            block(
                Vec3::new(0.005, 0.03, 0.52),
                Vec3::new(0.0, 0.0, -0.58),
                STEEL,
            ),
        ],
        grips: vec![
            handle(0.0, 0.0, GripKind::Primary),
            handle(0.08, 0.26, GripKind::Secondary),
        ],
        free_grab_radius: None,
    }
}

/// Mace: 2.0 kg blunt head.
pub fn mace() -> PropSpec {
    PropSpec {
        kind: PropKind::Mace,
        mass: 2.0,
        parts: vec![
            along_z(0.018, 0.1, -0.5, WOOD),
            block(Vec3::splat(0.05), Vec3::new(0.0, 0.0, -0.52), STEEL),
        ],
        grips: vec![
            handle(0.0, 0.0, GripKind::Primary),
            handle(0.02, 0.08, GripKind::Secondary),
        ],
        free_grab_radius: None,
    }
}

/// Short bow: 0.9 kg, stave along Y, shoots toward -Z. Grip at the riser; the string's
/// nocking point rests at `bow_string_rest()`.
pub fn bow() -> PropSpec {
    PropSpec {
        kind: PropKind::Bow,
        mass: 0.9,
        parts: vec![
            block(Vec3::new(0.015, 0.09, 0.02), Vec3::ZERO, WOOD),
            PropPart {
                shape: PartShape::Box(Vec3::new(0.012, 0.32, 0.012)),
                offset: Vec3::new(0.0, 0.38, 0.06),
                rotation: Quat::from_rotation_x(0.25),
                color: WOOD,
            },
            PropPart {
                shape: PartShape::Box(Vec3::new(0.012, 0.32, 0.012)),
                offset: Vec3::new(0.0, -0.38, 0.06),
                rotation: Quat::from_rotation_x(-0.25),
                color: WOOD,
            },
        ],
        // The bow frame (stave Y, shoots -Z) is NOT the hand's grip frame: held, the stave runs along the fist
        // tube (grip -Z, little finger -> thumb, upper limb up) and the shot leaves along the fingers (grip -Y).
        // grip frame = bow frame * rotation, so rotation = Rx(+90 deg): bow Y -> grip -Z, bow -Z -> grip -Y.
        grips: vec![GripPoint {
            a: Vec3::ZERO,
            b: Vec3::ZERO,
            rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            kind: GripKind::Primary,
            radius: 0.1,
        }],
        free_grab_radius: None,
    }
}

/// Where the bow string's nocking point rests (bow frame).
pub fn bow_string_rest() -> Vec3 {
    // Brace height of the original bow mesh (`it_weapon_ranged_bow_hunting_01`: string 27.1 cm behind the grip).
    Vec3::new(0.0, 0.0, 0.271)
}

/// Arrow: 0.03 kg, 0.75 m, tip toward -Z, nock at +Z end.
pub fn arrow() -> PropSpec {
    PropSpec {
        kind: PropKind::Arrow,
        mass: 0.03,
        parts: vec![along_z(0.005, 0.37, -0.38, WOOD)],
        grips: vec![GripPoint {
            a: Vec3::new(0.0, 0.0, 0.3),
            b: Vec3::new(0.0, 0.0, -0.1),
            rotation: Quat::IDENTITY,
            kind: GripKind::Any,
            radius: 0.08,
        }],
        free_grab_radius: None,
    }
}

/// Arrow length (m) and the distance from its centre to the nock.
pub const ARROW_HALF_LENGTH: f32 = 0.375;

/// Wooden crate with `half` extents, grabbable anywhere.
pub fn crate_box(half: Vec3, mass: f32) -> PropSpec {
    PropSpec {
        kind: PropKind::Crate,
        mass,
        parts: vec![block(half, Vec3::ZERO, [0.6, 0.45, 0.25])],
        grips: vec![],
        free_grab_radius: Some(half.max_element() + 0.08),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn densities_give_the_masses() {
        for spec in [sword(), axe(), two_handed_sword(), mace(), bow(), arrow()] {
            let m = spec.density() * spec.volume();
            assert!((m - spec.mass).abs() < 1e-4, "{:?}", spec.kind);
            assert!(!spec.grips.is_empty());
            assert!(matches!(
                spec.grips[0].kind,
                GripKind::Primary | GripKind::Any
            ));
        }
        let c = crate_box(Vec3::splat(0.2), 5.0);
        assert!(c.free_grab_radius.unwrap() > 0.2);
    }
}
