//! Bounded decoders that turn the game's audio containers into interleaved PCM.
//!
//! Gothic 3 ships plain Ogg Vorbis (`.ogg`: speech, music, most SFX) and RIFF/WAVE (`.wav`:
//! footsteps and a few voices). The decoders here exist for tooling and verification (duration,
//! sample rate, RMS); live playback hands the original bytes to Bevy's rodio backend.
use anyhow::{Context, Result, bail, ensure};
use std::io::Cursor;

/// Ceiling on decoded samples per file (about 6 minutes of 48 kHz stereo is 35M).
pub const MAX_SAMPLES: usize = 96_000_000;

/// Interleaved signed 16-bit PCM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcmStats {
    pub frames: usize,
    pub duration_secs: f64,
    /// Root mean square over all samples, normalised to 0..1.
    pub rms: f64,
    /// Largest absolute sample, normalised to 0..1.
    pub peak: f64,
}

impl PcmStats {
    pub fn is_silent(&self) -> bool {
        self.rms < 1e-4
    }
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn stats(&self) -> PcmStats {
        let frames = self.frames();
        let (mut sum, mut peak) = (0f64, 0f64);
        for &s in &self.samples {
            let v = f64::from(s) / 32768.0;
            sum += v * v;
            peak = peak.max(v.abs());
        }
        let n = self.samples.len().max(1) as f64;
        PcmStats {
            frames,
            duration_secs: frames as f64 / f64::from(self.sample_rate.max(1)),
            rms: (sum / n).sqrt(),
            peak,
        }
    }
}

/// Container sniffing by magic: `OggS` or `RIFF....WAVE`.
pub fn decode(bytes: &[u8]) -> Result<Pcm> {
    if bytes.starts_with(b"OggS") {
        decode_ogg(bytes)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        decode_wav(bytes)
    } else {
        bail!(
            "unknown audio container (magic {:02x?})",
            &bytes[..bytes.len().min(4)]
        )
    }
}

pub fn decode_ogg(bytes: &[u8]) -> Result<Pcm> {
    use lewton::inside_ogg::OggStreamReader;
    let mut r = OggStreamReader::new(Cursor::new(bytes)).context("ogg vorbis header")?;
    let channels = u16::from(r.ident_hdr.audio_channels);
    let sample_rate = r.ident_hdr.audio_sample_rate;
    ensure!(
        (1..=8).contains(&channels),
        "unsupported channel count {channels}"
    );
    ensure!(sample_rate > 0, "zero sample rate");
    let mut samples = Vec::new();
    while let Some(packet) = r.read_dec_packet_itl().context("vorbis packet")? {
        ensure!(
            samples.len() + packet.len() <= MAX_SAMPLES,
            "decoded audio exceeds ceiling"
        );
        samples.extend(packet);
    }
    Ok(Pcm {
        channels,
        sample_rate,
        samples,
    })
}

/// Duration of an Ogg Vorbis stream from its headers: sample rate from the identification
/// packet and the granule position of the last page. No audio decoding.
pub fn ogg_duration_secs(bytes: &[u8]) -> Option<f32> {
    if !bytes.starts_with(b"OggS") || bytes.len() < 27 {
        return None;
    }
    let nseg = usize::from(bytes[26]);
    let body = 27 + nseg;
    let ident = bytes.get(body..body + 16)?;
    if ident[0] != 1 || &ident[1..7] != b"vorbis" {
        return None;
    }
    let rate = u32::from_le_bytes(ident[12..16].try_into().ok()?);
    if rate == 0 {
        return None;
    }
    let last = bytes.windows(4).rposition(|w| w == b"OggS")?;
    let granule = i64::from_le_bytes(bytes.get(last + 6..last + 14)?.try_into().ok()?);
    (granule > 0).then(|| (granule as f64 / f64::from(rate)) as f32)
}

/// Seconds of digital near-silence (|sample| <= `threshold`) at the start of an Ogg Vorbis
/// stream, decoding only as far as the first audible sample (at most `max_secs`).
/// Every shipped music fragment starts with about 10 s of silence.
pub fn leading_silence_secs(bytes: &[u8], threshold: i16, max_secs: f64) -> Result<f64> {
    use lewton::inside_ogg::OggStreamReader;
    let mut r = OggStreamReader::new(Cursor::new(bytes)).context("ogg vorbis header")?;
    let ch = usize::from(r.ident_hdr.audio_channels).max(1);
    let rate = f64::from(r.ident_hdr.audio_sample_rate.max(1));
    let mut frames = 0usize;
    while let Some(packet) = r.read_dec_packet_itl().context("vorbis packet")? {
        if let Some(i) = packet.iter().position(|&s| s.abs() > threshold) {
            return Ok((frames + i / ch) as f64 / rate);
        }
        frames += packet.len() / ch;
        if frames as f64 / rate > max_secs {
            break;
        }
    }
    Ok(frames as f64 / rate)
}

/// `(format tag, channels, sample rate, bits per sample)` of a `fmt ` chunk.
type WavFormat = (u16, u16, u32, u16);

/// RIFF/WAVE with PCM (format 1) 8 or 16 bit; other format tags are reported, not guessed.
pub fn decode_wav(bytes: &[u8]) -> Result<Pcm> {
    ensure!(
        bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "not RIFF/WAVE"
    );
    let mut fmt: Option<WavFormat> = None;
    let mut data: Option<&[u8]> = None;
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into()?) as usize;
        let body_start = pos + 8;
        // Truncated final chunk: clamp to the available bytes (seen in the wild).
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];
        match id {
            b"fmt " => {
                ensure!(body.len() >= 16, "short fmt chunk");
                let tag = u16::from_le_bytes([body[0], body[1]]);
                let ch = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into()?);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                fmt = Some((tag, ch, rate, bits));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos = body_start.saturating_add(size).saturating_add(size & 1);
    }
    let (tag, channels, sample_rate, bits) = fmt.context("missing fmt chunk")?;
    let data = data.context("missing data chunk")?;
    ensure!(tag == 1, "unsupported WAVE format tag {tag} (only PCM)");
    ensure!(
        (1..=8).contains(&channels) && sample_rate > 0,
        "bad channel count or rate"
    );
    let samples: Vec<i16> = match bits {
        16 => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect(),
        8 => data.iter().map(|&b| (i16::from(b) - 128) << 8).collect(),
        other => bail!("unsupported PCM bit depth {other}"),
    };
    ensure!(
        samples.len() <= MAX_SAMPLES,
        "decoded audio exceeds ceiling"
    );
    Ok(Pcm {
        channels,
        sample_rate,
        samples,
    })
}

#[cfg(test)]
pub(crate) fn synth_wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(channels.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * u32::from(channels) * 2).to_le_bytes());
    b.extend((channels * 2).to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for s in samples {
        b.extend(s.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip_and_stats() {
        let tone: Vec<i16> = (0..2000)
            .flat_map(|i| {
                let v = ((i as f32 * 0.1).sin() * 16000.0) as i16;
                [v, v]
            })
            .collect();
        let pcm = decode(&synth_wav(2, 8000, &tone)).unwrap();
        assert_eq!(
            (pcm.channels, pcm.sample_rate, pcm.frames()),
            (2, 8000, 2000)
        );
        let s = pcm.stats();
        assert!((s.duration_secs - 0.25).abs() < 1e-9);
        assert!(!s.is_silent() && s.rms > 0.2 && s.rms < 0.5, "{s:?}");
        assert!(
            decode(&synth_wav(1, 8000, &[0; 100]))
                .unwrap()
                .stats()
                .is_silent()
        );
    }

    #[test]
    fn rejects_garbage_and_non_pcm() {
        assert!(decode(b"nope").is_err());
        assert!(decode(b"OggS\0\0\0\0").is_err());
        let mut w = synth_wav(1, 8000, &[1, 2, 3]);
        w[20] = 2; // ADPCM tag
        assert!(decode(&w).is_err());
        assert!(decode(&w[..30]).is_err());
    }

    #[test]
    fn truncated_data_chunk_is_clamped() {
        let w = synth_wav(1, 8000, &[100; 50]);
        let pcm = decode_wav(&w[..w.len() - 11]).unwrap();
        assert_eq!(pcm.samples.len(), 44);
    }
}
