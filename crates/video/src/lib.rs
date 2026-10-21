//! Cutscene video for Gothic Reforged: Bink (`.bik`) and anything else ffmpeg can decode.
//!
//! Decoding runs in `ffmpeg` subprocesses (no native linking): RGBA frames and f32 PCM are
//! read from pipes on worker threads with bounded buffers. ffmpeg/ffprobe must be present at
//! runtime (see `docs/VIDEO.md`); [`ffmpeg::supports_bink`] tells whether the build has the
//! Bink decoder.
//!
//! With the `bevy` feature, [`player::CutscenePlugin`] plays a video full screen with audio
//! (audio is the master clock), skip and subtitles.
pub mod decode;
pub mod ffmpeg;
#[cfg(feature = "bevy")]
pub mod player;
pub mod probe;
pub mod subtitles;
pub mod sync;

pub use decode::{AudioDecoder, DecodeOptions, Frame, PcmBuffer, Video, VideoDecoder};
pub use probe::{AudioInfo, VideoInfo, probe};
pub use subtitles::{Cue, Subtitles};
pub use sync::FrameSync;

/// Generate a synthetic test video with ffmpeg's `testsrc2` pattern and a sine tone
/// (`codec` e.g. `mpeg4`, container from the extension). For tests and the example.
pub fn generate_test_video(
    path: &std::path::Path,
    seconds: f32,
    width: u32,
    height: u32,
    fps: u32,
    codec: &str,
) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    let out = ffmpeg::command(ffmpeg::ffmpeg())
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={width}x{height}:rate={fps}"))
        .args(["-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000")
        .args([
            "-t",
            &seconds.to_string(),
            "-c:v",
            codec,
            "-pix_fmt",
            "yuv420p",
            "-c:a",
        ])
        .arg(
            if path.extension().is_some_and(|e| e == "mkv" || e == "avi") {
                "pcm_s16le"
            } else {
                "aac"
            },
        )
        .arg(path)
        .output()
        .context("running ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
