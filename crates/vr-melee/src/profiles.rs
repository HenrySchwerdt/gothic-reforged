//! Melee profiles of the procedural test props (`gothic_vr_physics::props`).
use crate::contact::{MeleeProfile, Striker, StrikerKind};
use glam::Vec3;
use gothic_vr_physics::props::PropKind;

fn edge(a: Vec3, b: Vec3, radius: f32) -> Striker {
    Striker {
        kind: StrikerKind::Edge {
            normal: Vec3::Y,
            double: true,
        },
        a,
        b,
        radius,
        min_speed: 2.5,
        full_speed: 9.0,
    }
}

fn point(a: Vec3, b: Vec3) -> Striker {
    Striker {
        kind: StrikerKind::Point,
        a,
        b,
        radius: 0.012,
        min_speed: 1.5,
        full_speed: 6.0,
    }
}

fn blunt(a: Vec3, b: Vec3, radius: f32, min_speed: f32) -> Striker {
    Striker {
        kind: StrikerKind::Blunt,
        a,
        b,
        radius,
        min_speed,
        full_speed: 9.0,
    }
}

/// Melee profile of a prop kind (`None` for non-melee props).
pub fn profile_for(kind: PropKind) -> Option<MeleeProfile> {
    let z = |v: f32| Vec3::new(0.0, 0.0, v);
    match kind {
        PropKind::Sword => Some(MeleeProfile {
            strikers: vec![edge(z(-0.08), z(-0.79), 0.025), point(z(-0.6), z(-0.81))],
            guard: (z(0.13), z(-0.81), 0.03),
            max_penetration: 0.4,
        }),
        PropKind::TwoHandedSword => Some(MeleeProfile {
            strikers: vec![edge(z(-0.08), z(-1.08), 0.03), point(z(-0.85), z(-1.1))],
            guard: (z(0.3), z(-1.1), 0.035),
            max_penetration: 0.5,
        }),
        PropKind::Axe => Some(MeleeProfile {
            strikers: vec![
                Striker {
                    kind: StrikerKind::Edge {
                        normal: Vec3::Y,
                        double: false,
                    },
                    a: Vec3::new(0.0, 0.17, -0.51),
                    b: Vec3::new(0.0, 0.17, -0.65),
                    radius: 0.02,
                    min_speed: 2.5,
                    full_speed: 9.0,
                },
                // Back of the head and the shaft: blunt.
                blunt(
                    Vec3::new(0.0, -0.02, -0.52),
                    Vec3::new(0.0, -0.02, -0.64),
                    0.03,
                    3.0,
                ),
            ],
            guard: (z(0.12), z(-0.65), 0.04),
            max_penetration: 0.1,
        }),
        PropKind::Mace => Some(MeleeProfile {
            strikers: vec![blunt(z(-0.47), z(-0.57), 0.06, 2.5)],
            guard: (z(0.1), z(-0.57), 0.04),
            max_penetration: 0.0,
        }),
        PropKind::Arrow => Some(MeleeProfile {
            strikers: vec![point(z(-0.2), z(-0.38))],
            guard: (z(0.37), z(-0.38), 0.008),
            max_penetration: 0.3,
        }),
        PropKind::Bow | PropKind::Crate => None,
    }
}
