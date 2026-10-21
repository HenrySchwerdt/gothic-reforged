//! Start the OpenXR input backend against the system runtime and print a few frames.
//!
//! `cargo run -p gothic-vr --features openxr --example openxr_probe [seconds]`: exit code 0
//! once the session runs and a head pose arrived, 1 otherwise (no runtime, no headless
//! support, no tracking). See docs/vr/OPENXR.md for running it against Monado without a
//! headset.
use gothic_vr::XrFrame;
use gothic_vr::openxr::OpenXrInput;
use std::time::{Duration, Instant};

fn main() -> std::process::ExitCode {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let mut xr = match OpenXrInput::new("Gothic Reforged probe") {
        Ok(xr) => xr,
        Err(e) => {
            eprintln!("openxr_probe: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("openxr_probe: {}", xr.description);
    let start = Instant::now();
    let fallback = XrFrame::default();
    let mut frames = 0;
    while start.elapsed() < Duration::from_secs(secs) {
        match xr.frame(&fallback) {
            Ok(Some(f)) => {
                frames += 1;
                if frames % 30 == 1 {
                    let hands: Vec<_> =
                        f.hands.iter().map(|h| h.pose.map(|p| p.position)).collect();
                    println!(
                        "frame {frames}: head {:.3} hands {hands:?}",
                        f.head.position
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("openxr_probe: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
        std::thread::sleep(Duration::from_millis(11));
    }
    println!("openxr_probe: state {:?}, {frames} frames", xr.state());
    if frames > 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
