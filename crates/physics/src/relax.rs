//! The engine's "relax" smoothing filter (camera distance/angles/position, ground height,
//! animation velocity, FOV all use it).
use glam::Vec3;

/// One relax unit in seconds: a relax factor `r` gives a time constant of `r * 0.025 s`.
pub const RELAX_UNIT: f32 = 0.025;

/// Error-dependent speed-up: large errors are chased up to four times faster.
///
/// Port of the boost term shared by `gCCameraAI_PS::UpdateDistance` (GameDll:0x10026860),
/// `gCCharacterMovement_PS::PutToGround` (0x100e94f0) and `CalcAniVelocity` (0x100e1cd0).
pub fn boost(error_sq: f32) -> f32 {
    let e = error_sq / 100_000.0;
    (4.0 * e + 1.0) / (e + 1.0)
}

fn gain(error_sq: f32, dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 {
        f32::INFINITY
    } else {
        boost(error_sq) * dt / tau
    }
}

/// Relax a scalar toward `target` with time constant `tau` seconds
/// (`new = (a * target + current) / (a + 1)`, `a = boost * dt / tau`). A non-positive time
/// constant means "unfiltered".
pub fn relax_tau(current: f32, target: f32, dt: f32, tau: f32) -> f32 {
    let d = target - current;
    let a = gain(d * d, dt, tau);
    if a.is_finite() {
        (a * target + current) / (a + 1.0)
    } else {
        target
    }
}

/// [`relax_tau`] with the original `relax factor * 0.025 s` time constant.
pub fn relax(current: f32, target: f32, dt: f32, relax_factor: f32) -> f32 {
    relax_tau(current, target, dt, relax_factor * RELAX_UNIT)
}

/// Vector relax: the boost depends on the squared length of the whole error vector.
pub fn relax_vec3(current: Vec3, target: Vec3, dt: f32, relax_factor: f32) -> Vec3 {
    let a = gain(
        (target - current).length_squared(),
        dt,
        relax_factor * RELAX_UNIT,
    );
    if a.is_finite() {
        (target * a + current) / (a + 1.0)
    } else {
        target
    }
}
