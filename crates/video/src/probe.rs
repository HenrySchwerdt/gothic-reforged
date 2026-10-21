//! Stream information through `ffprobe`.
use crate::ffmpeg;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Seconds (0 if unknown).
    pub duration: f64,
    /// ffmpeg codec name (`binkvideo`, `h264`, ...).
    pub codec: String,
    pub audio: Option<AudioInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

fn rate(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/').unwrap_or((s, "1"));
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then(|| n / d)
}

/// Parse `ffprobe -print_format json -show_streams -show_format` output.
pub fn parse(json: &str) -> Result<VideoInfo> {
    let p: Probe = serde_json::from_str(json).context("ffprobe output")?;
    let Some(v) = p
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
    else {
        bail!("no video stream");
    };
    let fps = v
        .avg_frame_rate
        .as_deref()
        .and_then(rate)
        .or_else(|| v.r_frame_rate.as_deref().and_then(rate))
        .unwrap_or(25.0);
    let duration = p
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .or(v.duration.as_deref())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    let audio = p
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .map(|a| AudioInfo {
            codec: a.codec_name.clone().unwrap_or_default(),
            sample_rate: a
                .sample_rate
                .as_deref()
                .and_then(|r| r.parse().ok())
                .unwrap_or(44_100),
            channels: a.channels.unwrap_or(2),
        });
    Ok(VideoInfo {
        width: v.width.context("video width")?,
        height: v.height.context("video height")?,
        fps,
        duration,
        codec: v.codec_name.clone().unwrap_or_default(),
        audio,
    })
}

pub fn probe(path: &Path) -> Result<VideoInfo> {
    ensure_file(path)?;
    let out = ffmpeg::command(ffmpeg::ffprobe())
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .output()
        .with_context(|| {
            format!(
                "running ffprobe ({}); see docs/VIDEO.md for the runtime requirement",
                ffmpeg::ffprobe().display()
            )
        })?;
    if !out.status.success() {
        bail!(
            "ffprobe {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let info = parse(&String::from_utf8_lossy(&out.stdout))
        .with_context(|| format!("probing {}", path.display()))?;
    if info.codec == "binkvideo" && !ffmpeg::supports_bink() {
        bail!("{}: this ffmpeg build has no Bink decoder", path.display());
    }
    Ok(info)
}

fn ensure_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("video {} not found", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_json() {
        let j = r#"{"streams":[
            {"codec_type":"video","codec_name":"binkvideo","width":800,"height":600,"avg_frame_rate":"30000/1001"},
            {"codec_type":"audio","codec_name":"binkaudio_rdft","sample_rate":"22050","channels":2}],
            "format":{"duration":"12.5"}}"#;
        let i = parse(j).unwrap();
        assert_eq!((i.width, i.height), (800, 600));
        assert!((i.fps - 29.97).abs() < 0.01);
        assert_eq!(i.duration, 12.5);
        assert_eq!(i.audio.unwrap().sample_rate, 22050);
        assert!(parse(r#"{"streams":[]}"#).is_err());
    }
}
