//! Unit and convention conversions: the game works in centimetres with the sim's yaw
//! (`Pos::yaw_to`: 0 looks along +X, angle = atan2(dz, dx)); Bevy / avian / VR in metres with
//! Bevy's yaw (0 looks along -Z, positive turns left).
use glam::Vec3;
use gothic_physics::Tri;
use gothic_sim::world::Pos;
use gothic_vr::CM_PER_M;

pub fn pos_to_m(p: Pos) -> Vec3 {
    Vec3::new(p.x, p.y, p.z) / CM_PER_M
}

pub fn m_to_pos(v: Vec3) -> Pos {
    let c = v * CM_PER_M;
    Pos::new(c.x, c.y, c.z)
}

pub fn tri_to_m(t: &Tri) -> [Vec3; 3] {
    t.map(|v| v / CM_PER_M)
}

/// Bevy yaw of a sim yaw (same facing direction).
pub fn sim_yaw_to_bevy(sim: f32) -> f32 {
    // Facing (x, z) = (cos, sin); Bevy forward (-sin b, -cos b).
    (-sim.cos()).atan2(-sim.sin())
}

/// Sim yaw of a Bevy yaw (the convention of `gothic_vr::bindings::hero_transform`).
pub fn bevy_yaw_to_sim(bevy: f32) -> f32 {
    (-bevy.cos()).atan2(-bevy.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn positions_and_yaws_round_trip() {
        let p = Pos::new(120.0, -35.0, 410.0);
        let back = m_to_pos(pos_to_m(p));
        assert!((back.x - p.x).abs() < 1e-3 && (back.z - p.z).abs() < 1e-3);
        for sim in [0.0f32, 0.7, -2.0, 3.0] {
            let b = sim_yaw_to_bevy(sim);
            let f = Quat::from_rotation_y(b) * Vec3::NEG_Z;
            assert!((f.x - sim.cos()).abs() < 1e-5 && (f.z - sim.sin()).abs() < 1e-5);
            let again = bevy_yaw_to_sim(b);
            assert!(
                (again - sim)
                    .rem_euclid(std::f32::consts::TAU)
                    .min((sim - again).rem_euclid(std::f32::consts::TAU))
                    < 1e-4
            );
        }
        // Same as the VR command mapper's hero transform.
        let gothic_runtime::Command::HeroTransform { yaw, .. } =
            gothic_vr::bindings::hero_transform(Vec3::ZERO, 0.4)
        else {
            panic!()
        };
        assert!((yaw - bevy_yaw_to_sim(0.4)).abs() < 1e-6);
    }
}
