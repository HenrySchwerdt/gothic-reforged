//! `VrPlugin { backend: OpenXr }` with the `openxr` feature: with a runtime (set
//! `GOTHIC_VR_EXPECT_RUNTIME=1`, e.g. Monado's simulated HMD in CI) the backend reports OpenXR
//! and head poses arrive in `VrInputRes`; without one it falls back to the desktop backend
//! with the reason in `XrBackend::note`.
use bevy::prelude::*;
use gothic_vr::{
    Hand, HapticMessage, HapticOut, HapticPulse, VrInputRes, VrPlugin, XrBackend, XrBackendKind,
};

#[test]
fn openxr_backend_runs_or_falls_back() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        VrPlugin {
            backend: XrBackendKind::OpenXr,
        },
    ));
    let backend = app.world().resource::<XrBackend>().clone();
    let expect_runtime = std::env::var("GOTHIC_VR_EXPECT_RUNTIME").is_ok_and(|v| v == "1");
    if !expect_runtime {
        if backend.kind == XrBackendKind::Desktop {
            assert!(
                backend.note.contains("OpenXR unavailable"),
                "{}",
                backend.note
            );
        }
        return;
    }
    assert_eq!(backend.kind, XrBackendKind::OpenXr, "{}", backend.note);
    let mut heads = Vec::new();
    for _ in 0..300 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let input = &app.world().resource::<VrInputRes>().0;
        if input.frames > 0 {
            heads.push(input.head().position);
        }
    }
    assert!(heads.len() > 10, "frames from the runtime: {}", heads.len());
    // Stage space: the head is above the floor, not at the desktop default.
    let h = heads.last().unwrap();
    assert!(h.y > 1.0 && h.y < 2.2, "{h}");
    // Haptics reach the runtime (the simulated HMD accepts them without a device).
    let pulse = HapticPulse::clash(Hand::Right, 5.0);
    let r = app
        .world_mut()
        .non_send_mut::<gothic_vr::openxr::OpenXrInput>()
        .vibrate(&pulse);
    assert!(r.is_ok(), "{r:?}");
    app.world_mut().write_message(HapticMessage(pulse));
    app.update();
    assert!(!app.world().resource::<HapticOut>().played.is_empty());
}
