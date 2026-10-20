//! Fixed-timestep frame capture (`--record DIR`): every rendered frame advances the game by
//! exactly 1/30 s and is written as `frame_00001.png`, ... so the video is smooth no matter
//! how long capturing a frame takes.
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::AsyncComputeTaskPool;
use bevy::time::TimeUpdateStrategy;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub const FPS: u32 = 30;
/// Frames whose PNG is still being written.
pub static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

#[derive(Resource)]
pub struct Recorder {
    pub dir: PathBuf,
    pub active: bool,
    pub frame: u32,
    /// Extra named screenshots (`.local/play-NAME.png`) taken from the next recorded frame
    /// (a second screenshot in the same frame would be dropped and leave a gap).
    pub names: Vec<String>,
}

pub fn register(app: &mut App, dir: PathBuf) {
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("frame_") && n.ends_with(".png") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / f64::from(FPS),
    )))
    .insert_resource(Recorder {
        dir,
        active: false,
        frame: 0,
        names: Vec::new(),
    })
    .add_systems(Last, capture);
}

fn capture(mut commands: Commands, mut rec: ResMut<Recorder>) {
    if !rec.active {
        return;
    }
    rec.frame += 1;
    let path = rec.dir.join(format!("frame_{:05}.png", rec.frame));
    let extra: Vec<PathBuf> = std::mem::take(&mut rec.names)
        .into_iter()
        .map(|n| PathBuf::from(format!(".local/play-{n}.png")))
        .collect();
    commands
        .spawn(Screenshot::primary_window())
        .observe(move |cap: On<ScreenshotCaptured>| {
            let image = cap.image.clone();
            let path = path.clone();
            let extra = extra.clone();
            IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
            AsyncComputeTaskPool::get()
                .spawn(async move {
                    if let Ok(dynamic) = image.try_into_dynamic() {
                        let rgb = dynamic.to_rgb8();
                        for p in std::iter::once(&path).chain(&extra) {
                            if let Err(e) = rgb.save(p) {
                                eprintln!("record: {}: {e}", p.display());
                            }
                        }
                    }
                    IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
                })
                .detach();
        });
}
