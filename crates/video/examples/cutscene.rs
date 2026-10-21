//! Plays a video full screen with the cutscene player. Without an argument it generates a
//! synthetic test video (ffmpeg `testsrc2` + a sine tone) and subtitles first.
//!
//! `cargo run -p gothic-video --example cutscene --features demo [-- VIDEO [SUBS.srt]]`
//! Esc / Space / Enter / click skips. `--screenshots DIR` saves two frames and runs headless
//! checks (works under `xvfb-run`). The app exits when the cutscene ends.
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_video::player::{
    CutsceneFinished, CutscenePlugin, CutsceneSubtitle, PlayCutscene, cutscene_playing,
};
use gothic_video::{Subtitles, ffmpeg, generate_test_video};
use std::path::PathBuf;

const SUBS: &str = "1\n00:00:00,300 --> 00:00:01,800\nThe orcs have taken Myrtana.\n\n2\n00:00:02,000 --> 00:00:03,800\n(synthetic test video, no game content)\n";

#[derive(Resource)]
struct Shots(Option<PathBuf>, u32);

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let shots = args.iter().position(|a| a == "--screenshots").map(|i| {
        let dir = PathBuf::from(args.remove(i + 1));
        args.remove(i);
        dir
    });
    println!(
        "ffmpeg available: {}, Bink decoder: {}",
        ffmpeg::available(),
        ffmpeg::supports_bink()
    );
    let (video, subs) = match args.first() {
        Some(v) => {
            let subs = match args.get(1) {
                Some(s) => Some(Subtitles::parse_srt(&std::fs::read_to_string(s)?)?),
                None => None,
            };
            (PathBuf::from(v), subs)
        }
        None => {
            let path = std::env::temp_dir().join("gothic-video-demo.avi");
            generate_test_video(&path, 4.0, 640, 360, 30, "mpeg4")?;
            (path, Some(Subtitles::parse_srt(SUBS)?))
        }
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "gothic-video cutscene".into(),
                resolution: (960, 600).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(CutscenePlugin::default())
        .insert_resource(Shots(shots, 0))
        .add_systems(
            Startup,
            move |mut commands: Commands, mut play: MessageWriter<PlayCutscene>| {
                commands.spawn(Camera2d);
                play.write(PlayCutscene {
                    path: video.clone(),
                    subtitles: subs.clone(),
                    skippable: true,
                });
            },
        )
        .add_systems(Update, (report, capture.run_if(cutscene_playing)))
        .run();
    Ok(())
}

fn report(
    mut subs: MessageReader<CutsceneSubtitle>,
    mut done: MessageReader<CutsceneFinished>,
    mut exit: MessageWriter<AppExit>,
) {
    for s in subs.read() {
        println!("subtitle: {:?}", s.text);
    }
    for d in done.read() {
        println!(
            "finished {} skipped={} shown={} dropped={} error={:?}",
            d.path.display(),
            d.skipped,
            d.frames_shown,
            d.frames_dropped,
            d.error
        );
        exit.write(if d.error.is_some() {
            AppExit::error()
        } else {
            AppExit::Success
        });
    }
}

fn capture(mut commands: Commands, mut shots: ResMut<Shots>) {
    let Some(dir) = shots.0.clone() else {
        return;
    };
    shots.1 += 1;
    if shots.1 == 40 || shots.1 == 100 {
        let _ = std::fs::create_dir_all(&dir);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(dir.join(format!("cutscene_{}.png", shots.1))));
    }
}
