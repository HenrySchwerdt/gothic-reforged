//! Cutscene video (`gothic-video`) on start: `--video FILE` plays a file, `GR_VIDEO=1` the
//! original intro (`G3_Intro.bik`, which `gCSession::Start` plays). Decoding runs in ffmpeg
//! subprocesses; without ffmpeg or its Bink decoder the player answers at once with an error
//! and the game continues as if the video had been skipped.
//!
//! The world keeps loading and ticking underneath (gameplay is not paused); the walk / play
//! input still sees the skip keys (Esc, Space, Enter, left click).
use super::VideoRequest;
use bevy::prelude::*;
use gothic_video::player::{CutsceneFinished, CutscenePlugin, PlayCutscene};
use std::path::{Path, PathBuf};

/// File name of the original intro.
pub(crate) const INTRO: &str = "g3_intro.bik";

/// Look for the intro (case-insensitive) in `data_dir`, `data_dir/Video`, and the
/// installation's `Video` / `Data/Video` folders.
pub(crate) fn find_intro(data_dir: &Path) -> Option<PathBuf> {
    let mut dirs = vec![data_dir.to_path_buf(), data_dir.join("Video")];
    if let Some(install) = data_dir.parent() {
        dirs.push(install.join("Video"));
        dirs.push(install.join("Data").join("Video"));
    }
    dirs.iter().find_map(|d| {
        std::fs::read_dir(d).ok()?.find_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            (name == INTRO && e.path().is_file()).then(|| e.path())
        })
    })
}

#[derive(Resource)]
struct StartVideo(PathBuf);

pub(crate) fn install(app: &mut App, request: &VideoRequest, data_dir: &Path) {
    let path = match request {
        VideoRequest::File(p) => p.clone(),
        VideoRequest::Intro => match find_intro(data_dir) {
            Some(p) => p,
            None => {
                eprintln!(
                    "video: {INTRO} not found under {}; no intro",
                    data_dir.display()
                );
                return;
            }
        },
    };
    eprintln!(
        "video: {} (ffmpeg {}, Bink decoder {})",
        path.display(),
        if gothic_video::ffmpeg::available() {
            "found"
        } else {
            "missing"
        },
        if gothic_video::ffmpeg::supports_bink() {
            "present"
        } else {
            "missing"
        },
    );
    app.add_plugins(CutscenePlugin::default())
        .insert_resource(StartVideo(path))
        .add_systems(Startup, start)
        .add_systems(Update, report);
}

fn start(video: Res<StartVideo>, mut play: MessageWriter<PlayCutscene>) {
    play.write(PlayCutscene::new(video.0.clone()));
}

fn report(mut finished: MessageReader<CutsceneFinished>) {
    for f in finished.read() {
        match &f.error {
            Some(e) => eprintln!("video: {} not played: {e}", f.path.display()),
            None => eprintln!(
                "video: {} {} ({} frames shown, {} dropped)",
                f.path.display(),
                if f.skipped { "skipped" } else { "finished" },
                f.frames_shown,
                f.frames_dropped
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_video_finds_intro_case_insensitive() {
        let root = std::env::temp_dir().join(format!("gr-cloud-video-{}", std::process::id()));
        let data = root.join("Data");
        let video = data.join("Video");
        std::fs::create_dir_all(&video).unwrap();
        assert_eq!(find_intro(&data), None);
        // Synthetic stand-in: only the name matters.
        std::fs::write(video.join("G3_Intro.BIK"), b"not a video").unwrap();
        assert_eq!(find_intro(&data), Some(video.join("G3_Intro.BIK")));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
