//! Headless scenarios for the gameplay camera.
use glam::{Vec2, Vec3};
use gothic_camera::{
    CameraConfig, CameraInput, CameraMode, CameraState, CameraWorld, DialogActor, HeroView,
    NoWorld, PresetTable,
};

const DT: f32 = 1.0 / 60.0;

fn camera() -> CameraState {
    CameraState::new(CameraConfig::default(), PresetTable::default())
}

fn hero_at(p: Vec3, moving: bool) -> HeroView {
    HeroView {
        position: p,
        box_height: 200.0,
        moving,
        speed: if moving { 400.0 } else { 0.0 },
        ..HeroView::default()
    }
}

fn frame(hero: HeroView) -> CameraInput {
    CameraInput {
        dt: DT,
        hero,
        ..CameraInput::default()
    }
}

/// Wall at z = `z` blocking rays that travel toward +z.
struct Wall(f32);
impl CameraWorld for Wall {
    fn ray_nearest(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32> {
        if dir.z <= 1e-4 {
            return None;
        }
        let t = (self.0 - from.z) / dir.z;
        (t > 0.0 && t <= len).then_some(t)
    }
}

#[test]
fn camera_trails_a_running_hero_and_settles_at_the_move_distance() {
    let mut cam = camera();
    let mut hero = hero_at(Vec3::ZERO, true);
    cam.reset(&hero, Vec3::NEG_Z);
    let mut out = cam.update(&frame(hero), &NoWorld);
    for _ in 0..(6 * 60) {
        hero.position.z -= 400.0 * DT;
        out = cam.update(&frame(hero), &NoWorld);
    }
    let (dist, zoom) = cam.distances();
    assert_eq!(zoom, 150.0);
    assert!((dist - 300.0).abs() < 8.0, "boom {dist}");
    let head = hero.position + Vec3::Y * 200.0;
    let rel = out.position - head;
    // Behind the hero (+z), above the head by sin(30 deg) * boom, trailing by the position filter.
    assert!(rel.z > 250.0, "{rel}");
    assert!(rel.y > 100.0 && rel.y < 180.0, "{rel}");
    // The camera looks forward along the run direction.
    let fwd = out.rotation * Vec3::NEG_Z;
    assert!(fwd.z < -0.7, "{fwd}");
}

#[test]
fn stopping_snaps_the_boom_back_to_the_rest_distance() {
    let mut cam = camera();
    let mut hero = hero_at(Vec3::ZERO, true);
    cam.reset(&hero, Vec3::NEG_Z);
    for _ in 0..300 {
        hero.position.z -= 400.0 * DT;
        cam.update(&frame(hero), &NoWorld);
    }
    assert!(cam.distances().0 > 250.0);
    hero.moving = false;
    cam.update(&frame(hero), &NoWorld);
    assert_eq!(cam.distances().0, 150.0);
}

#[test]
fn pitch_is_clamped_to_89_degrees() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    for _ in 0..600 {
        cam.update(
            &CameraInput {
                look: Vec2::new(0.0, 300.0),
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    let (_, e) = cam.orbit_degrees();
    assert!((e - 89.0).abs() < 0.01, "{e}");
    for _ in 0..1200 {
        cam.update(
            &CameraInput {
                look: Vec2::new(0.0, -300.0),
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    let (_, e) = cam.orbit_degrees();
    assert!((e + 89.0).abs() < 0.01, "{e}");
}

#[test]
fn horizontal_look_orbits_the_hero_and_keeps_the_boom() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    let (a0, _) = cam.orbit_degrees();
    for _ in 0..30 {
        cam.update(
            &CameraInput {
                look: Vec2::new(40.0, 0.0),
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    for _ in 0..240 {
        cam.update(&frame(hero), &NoWorld);
    }
    let (a1, _) = cam.orbit_degrees();
    assert!(
        a1 < a0 - 10.0,
        "mouse right must orbit the camera, {a0} -> {a1}"
    );
    let out = cam.update(&frame(hero), &NoWorld);
    let d = (out.position - (hero.position + Vec3::Y * 200.0)).length();
    assert!((d - 150.0).abs() < 2.0, "{d}");
}

#[test]
fn zoom_is_clamped_and_wheel_overshoot_enters_first_person_and_back() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    for _ in 0..40 {
        cam.update(
            &CameraInput {
                zoom: -1.0,
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    assert_eq!(cam.distances().1, 800.0);
    for _ in 0..14 {
        cam.update(
            &CameraInput {
                zoom: 1.0,
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    assert_eq!(cam.distances().1, 100.0);
    assert_eq!(cam.mode(), CameraMode::Normal);
    // 200 units past the minimum: 4 notches of 50.
    let mut out = cam.update(
        &CameraInput {
            zoom: 1.0,
            ..frame(hero)
        },
        &NoWorld,
    );
    for _ in 0..4 {
        out = cam.update(
            &CameraInput {
                zoom: 1.0,
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    assert_eq!(cam.mode(), CameraMode::FirstPerson);
    assert_eq!(out.z_near, 13.0);
    assert_eq!(out.focus_mode, 1);
    assert!(out.hide_hero);
    let head = hero.position + Vec3::Y * 200.0;
    for _ in 0..120 {
        out = cam.update(&frame(hero), &NoWorld);
    }
    assert!((out.position - head).length() < 1.0);
    assert!((out.fov_deg - 70.0).abs() < 0.5);
    for _ in 0..6 {
        out = cam.update(
            &CameraInput {
                zoom: -1.0,
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    assert_eq!(cam.mode(), CameraMode::Normal);
    assert_eq!(out.z_near, 25.0);
}

#[test]
fn wall_behind_the_hero_pulls_the_camera_in_and_it_relaxes_back() {
    let mut cam = camera();
    let mut hero = hero_at(Vec3::ZERO, true);
    cam.reset(&hero, Vec3::NEG_Z);
    let wall = Wall(220.0);
    let head_z = hero.position.z;
    for _ in 0..300 {
        let out = cam.update(&frame(hero), &wall);
        assert!(
            out.position.z <= 220.0 - 20.0 + 1.0,
            "camera behind wall: {}",
            out.position.z
        );
        let _ = head_z;
    }
    let (d, _) = cam.distances();
    assert!(d < 260.0, "{d}");
    // Without the wall the boom relaxes out to the moving distance again.
    hero.moving = true;
    for _ in 0..400 {
        cam.update(&frame(hero), &NoWorld);
    }
    assert!((cam.distances().0 - 300.0).abs() < 8.0);
}

#[test]
fn fov_follows_the_distance_law_and_the_mode_presets() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    let mut out = cam.update(&frame(hero), &NoWorld);
    for _ in 0..240 {
        out = cam.update(&frame(hero), &NoWorld);
    }
    assert!((out.fov_deg - 64.9).abs() < 0.15, "{}", out.fov_deg);
    cam.set_mode(CameraMode::Magic, false);
    for _ in 0..400 {
        out = cam.update(&frame(hero), &NoWorld);
    }
    assert!((out.fov_deg - 45.0).abs() < 0.2, "{}", out.fov_deg);
    assert_eq!(cam.distances().1, 150.0);
    cam.set_mode(CameraMode::Melee, false);
    assert_eq!(cam.distances().1, 450.0);
}

#[test]
fn hero_fades_when_the_camera_comes_close() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    // A wall right behind the hero squeezes the boom to the minimum.
    let wall = Wall(60.0);
    let mut out = cam.update(&frame(hero), &wall);
    for _ in 0..120 {
        out = cam.update(&frame(hero), &wall);
    }
    assert!(out.hero_alpha < 0.5, "{}", out.hero_alpha);
    let mut open = camera();
    open.reset(&hero, Vec3::NEG_Z);
    let far = open.update(&frame(hero), &NoWorld);
    assert!(far.hero_alpha > 0.99);
}

fn actor(id: u64, p: Vec3) -> DialogActor {
    DialogActor {
        id,
        origin: p,
        head: p + Vec3::Y * 170.0,
        volume: 100_000.0,
        box_height: 200.0,
    }
}

#[test]
fn dialog_camera_frames_both_speakers() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    let a = actor(1, Vec3::ZERO);
    let b = actor(2, Vec3::new(0.0, 0.0, -300.0));
    cam.seed(3);
    assert!(cam.set_dialog_roles(a, b, &NoWorld));
    assert!(cam.dialog_active());
    let mut out = cam.update(
        &CameraInput {
            dialog: Some((a, b)),
            ..frame(hero)
        },
        &NoWorld,
    );
    for _ in 0..400 {
        out = cam.update(
            &CameraInput {
                dialog: Some((a, b)),
                ..frame(hero)
            },
            &NoWorld,
        );
    }
    assert_eq!(out.mode, CameraMode::Dialog);
    assert!((out.fov_deg - 60.0).abs() < 0.3, "{}", out.fov_deg);
    // Both heads are in front of the camera and inside a 60 degree cone.
    let fwd = out.rotation * Vec3::NEG_Z;
    for t in [a.head, b.head] {
        let to = (t - out.position).normalize();
        let angle = fwd.dot(to).clamp(-1.0, 1.0).acos().to_degrees();
        assert!(angle < 60.0, "{t}: {angle}");
    }
    assert!(out.speaker_point.is_some());
    cam.stop_dialog();
    assert!(!cam.dialog_active());
    assert_eq!(cam.mode(), CameraMode::Normal);
}

#[test]
fn dialog_recut_alternates_sides_and_is_deterministic() {
    let run = || {
        let mut cam = camera();
        cam.reset(&hero_at(Vec3::ZERO, false), Vec3::NEG_Z);
        cam.seed(99);
        let a = actor(1, Vec3::ZERO);
        let b = actor(2, Vec3::new(0.0, 0.0, -300.0));
        let mut keys = Vec::new();
        for i in 0..4 {
            let (l, s) = if i % 2 == 0 { (a, b) } else { (b, a) };
            cam.set_dialog_roles(l, s, &NoWorld);
            keys.push(cam.active_preset_key());
            // Move past the 18 s short-shot guard so the next call really re-cuts.
            for _ in 0..(19 * 60) {
                cam.update(
                    &CameraInput {
                        dialog: Some((l, s)),
                        ..frame(hero_at(Vec3::ZERO, false))
                    },
                    &NoWorld,
                );
            }
        }
        keys
    };
    assert_eq!(run(), run());
    let keys = run();
    assert!(keys.windows(2).all(|w| w[0] != w[1]), "{keys:?}");
}

#[test]
fn teleport_snaps_the_camera_to_the_new_position() {
    let mut cam = camera();
    let mut hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    hero.position = Vec3::new(50_000.0, 0.0, 0.0);
    cam.notify_teleport(&hero);
    let out = cam.update(&frame(hero), &NoWorld);
    assert!((out.position - (hero.position + Vec3::Y * 200.0)).length() < 300.0);
}

#[test]
fn tremor_shakes_and_dies_out() {
    let mut cam = camera();
    let hero = hero_at(Vec3::ZERO, false);
    cam.reset(&hero, Vec3::NEG_Z);
    for _ in 0..120 {
        cam.update(&frame(hero), &NoWorld);
    }
    let calm = cam.update(&frame(hero), &NoWorld).position;
    cam.add_tremor(calm, 1000.0, Vec3::splat(10.0), 1.0);
    let shaken = cam.update(&frame(hero), &NoWorld).position;
    assert!((shaken - calm).length() > 0.5);
    for _ in 0..90 {
        cam.update(&frame(hero), &NoWorld);
    }
    let after = cam.update(&frame(hero), &NoWorld).position;
    assert!((after - calm).length() < 0.2);
    // Out of range: no shake.
    cam.add_tremor(calm + Vec3::X * 5000.0, 1000.0, Vec3::splat(10.0), 1.0);
    let still = cam.update(&frame(hero), &NoWorld).position;
    assert!((still - after).length() < 0.2);
}

#[test]
fn disabled_camera_ignores_mode_requests() {
    let mut cam = camera();
    cam.set_enabled(false);
    cam.set_mode(CameraMode::Melee, false);
    assert_eq!(cam.mode(), CameraMode::Normal);
}
