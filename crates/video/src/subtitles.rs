//! Subtitles: SubRip (`.srt`) cues looked up by playback time.
use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Subtitles {
    pub cues: Vec<Cue>,
}

/// `HH:MM:SS,mmm` (or `.mmm`).
fn time(s: &str) -> Result<f64> {
    let s = s.trim();
    let (hms, ms) = s.split_once([',', '.']).unwrap_or((s, "0"));
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.len() != 3 {
        bail!("bad time {s:?}");
    }
    let n = |p: &str| {
        p.trim()
            .parse::<f64>()
            .with_context(|| format!("bad time {s:?}"))
    };
    Ok(n(parts[0])? * 3600.0 + n(parts[1])? * 60.0 + n(parts[2])? + n(ms)? / 1000.0)
}

impl Subtitles {
    pub fn parse_srt(text: &str) -> Result<Self> {
        let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
        let mut cues = Vec::new();
        for block in text.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
            let mut lines = block.lines();
            let mut first = lines.next().unwrap_or_default();
            if !first.contains("-->") {
                first = lines.next().unwrap_or_default();
            }
            let Some((a, b)) = first.split_once("-->") else {
                bail!("cue without timing: {block:?}");
            };
            let text = lines.collect::<Vec<_>>().join("\n");
            cues.push(Cue {
                start: time(a)?,
                end: time(b)?,
                text,
            });
        }
        cues.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(Self { cues })
    }

    /// The text at `t` seconds (the latest cue that covers it).
    pub fn at(&self, t: f64) -> Option<&str> {
        self.cues
            .iter()
            .rev()
            .find(|c| c.start <= t && t < c.end)
            .map(|c| c.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let s = Subtitles::parse_srt(
            "\u{feff}1\r\n00:00:00,500 --> 00:00:01,250\r\nHello\r\nthere\r\n\r\n2\r\n00:00:02.000 --> 00:01:00.000\r\nBye\r\n",
        )
        .unwrap();
        assert_eq!(s.cues.len(), 2);
        assert_eq!(s.at(0.4), None);
        assert_eq!(s.at(0.5), Some("Hello\nthere"));
        assert_eq!(s.at(1.3), None);
        assert_eq!(s.at(59.0), Some("Bye"));
        assert!(Subtitles::parse_srt("1\nno timing\n").is_err());
    }
}
