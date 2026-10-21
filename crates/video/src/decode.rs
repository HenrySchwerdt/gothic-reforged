//! Decoding through ffmpeg subprocesses: one for RGBA video frames, one for interleaved f32
//! PCM. Both run on their own threads with bounded buffers (back-pressure), and are killed
//! when the decoder is dropped.
use crate::ffmpeg;
use crate::probe::{VideoInfo, probe};
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};

/// One decoded picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub index: u64,
    /// Presentation time in seconds from the start.
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, sRGB RGBA.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodeOptions {
    /// Downscale to at most this width (keeps the aspect ratio; even sizes).
    pub max_width: Option<u32>,
    /// Output sample rate and channel count of the audio.
    pub sample_rate: u32,
    pub channels: u16,
    /// Decode audio (if the file has any).
    pub audio: bool,
    /// Decoded frames buffered ahead.
    pub frame_queue: usize,
    /// Seconds of audio buffered ahead.
    pub audio_buffer_seconds: f32,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            max_width: None,
            sample_rate: 48_000,
            channels: 2,
            audio: true,
            frame_queue: 8,
            audio_buffer_seconds: 1.0,
        }
    }
}

struct Kill(Arc<Mutex<Option<Child>>>);

impl Drop for Kill {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn spawn(path: &Path, args: &[String]) -> Result<(Arc<Mutex<Option<Child>>>, ChildStdout)> {
    let mut child = ffmpeg::command(ffmpeg::ffmpeg())
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(args)
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "starting ffmpeg ({}); see docs/VIDEO.md for the runtime requirement",
                ffmpeg::ffmpeg().display()
            )
        })?;
    let out = child.stdout.take().context("ffmpeg stdout")?;
    Ok((Arc::new(Mutex::new(Some(child))), out))
}

/// Output size for `max_width` (even numbers, as most scalers want).
pub fn output_size(w: u32, h: u32, max_width: Option<u32>) -> (u32, u32) {
    match max_width {
        Some(m) if m < w => {
            let nw = (m.max(2)) & !1;
            let nh = ((u64::from(h) * u64::from(nw) / u64::from(w)) as u32).max(2) & !1;
            (nw, nh)
        }
        _ => (w, h),
    }
}

pub struct VideoDecoder {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    rx: Receiver<Frame>,
    done: bool,
    _kill: Kill,
}

impl VideoDecoder {
    pub fn open(path: &Path, info: &VideoInfo, opts: &DecodeOptions) -> Result<Self> {
        let (width, height) = output_size(info.width, info.height, opts.max_width);
        let mut args: Vec<String> = vec!["-an".into(), "-sn".into()];
        if (width, height) != (info.width, info.height) {
            args.extend(["-vf".into(), format!("scale={width}:{height}")]);
        }
        args.extend(["-f", "rawvideo", "-pix_fmt", "rgba"].map(String::from));
        let (child, mut out) = spawn(path, &args)?;
        let (tx, rx) = sync_channel(opts.frame_queue.max(1));
        let fps = info.fps;
        let size = width as usize * height as usize * 4;
        std::thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || {
                let mut index = 0u64;
                loop {
                    let mut rgba = vec![0u8; size];
                    if out.read_exact(&mut rgba).is_err() {
                        break;
                    }
                    let f = Frame {
                        index,
                        pts: index as f64 / fps,
                        width,
                        height,
                        rgba,
                    };
                    if tx.send(f).is_err() {
                        break;
                    }
                    index += 1;
                }
            })?;
        Ok(Self {
            width,
            height,
            fps,
            rx,
            done: false,
            _kill: Kill(child),
        })
    }

    /// The next decoded frame if one is ready.
    pub fn try_next(&mut self) -> Option<Frame> {
        match self.rx.try_recv() {
            Ok(f) => Some(f),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.done = true;
                None
            }
        }
    }

    /// Block for the next frame (`None` at the end).
    pub fn next_blocking(&mut self) -> Option<Frame> {
        let f = self.rx.recv().ok();
        self.done |= f.is_none();
        f
    }

    /// The stream ended and every frame was taken.
    pub fn is_finished(&self) -> bool {
        self.done
    }
}

struct PcmInner {
    samples: VecDeque<f32>,
    eof: bool,
    /// Samples handed to the audio output so far.
    consumed: u64,
    cancelled: bool,
}

/// Shared PCM queue between the audio decoder thread and the audio output.
pub struct PcmBuffer {
    inner: Mutex<PcmInner>,
    cv: Condvar,
    capacity: usize,
    pub sample_rate: u32,
    pub channels: u16,
}

impl PcmBuffer {
    pub fn new(sample_rate: u32, channels: u16, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(PcmInner {
                samples: VecDeque::new(),
                eof: false,
                consumed: 0,
                cancelled: false,
            }),
            cv: Condvar::new(),
            capacity: capacity.max(1024),
            sample_rate,
            channels,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PcmInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Producer side: blocks while the buffer is full. False once cancelled.
    pub fn push(&self, data: &[f32]) -> bool {
        let mut g = self.lock();
        while g.samples.len() >= self.capacity && !g.cancelled {
            g = self.cv.wait(g).unwrap_or_else(|p| p.into_inner());
        }
        if g.cancelled {
            return false;
        }
        g.samples.extend(data.iter().copied());
        true
    }

    pub fn finish(&self) {
        self.lock().eof = true;
        self.cv.notify_all();
    }

    /// Stop the producer (it is woken and ends).
    pub fn cancel(&self) {
        let mut g = self.lock();
        g.cancelled = true;
        g.eof = true;
        g.samples.clear();
        self.cv.notify_all();
    }

    /// Consumer side: take up to `out.len()` samples. Returns the count taken, or `None` at
    /// the end of the stream. An underrun (decoder behind) yields silence instead of ending.
    pub fn pop(&self, out: &mut [f32]) -> Option<usize> {
        let mut g = self.lock();
        if g.samples.is_empty() && g.eof {
            return None;
        }
        let n = out.len().min(g.samples.len());
        for (o, s) in out.iter_mut().zip(g.samples.drain(..n)) {
            *o = s;
        }
        if n == 0 {
            // Underrun: play one block of silence.
            out.fill(0.0);
        }
        let taken = out.len();
        g.consumed += taken as u64;
        drop(g);
        self.cv.notify_all();
        Some(taken)
    }

    /// Seconds of audio handed to the output (the audio clock).
    pub fn position(&self) -> f64 {
        self.lock().consumed as f64 / (f64::from(self.sample_rate) * f64::from(self.channels))
    }

    /// The producer finished and everything was consumed.
    pub fn is_finished(&self) -> bool {
        let g = self.lock();
        g.eof && g.samples.is_empty()
    }

    pub fn buffered(&self) -> usize {
        self.lock().samples.len()
    }
}

pub struct AudioDecoder {
    pub buffer: Arc<PcmBuffer>,
    _kill: Kill,
}

impl AudioDecoder {
    pub fn open(path: &Path, opts: &DecodeOptions) -> Result<Self> {
        let args: Vec<String> = vec![
            "-vn".into(),
            "-sn".into(),
            "-f".into(),
            "f32le".into(),
            "-ac".into(),
            opts.channels.to_string(),
            "-ar".into(),
            opts.sample_rate.to_string(),
        ];
        let (child, mut out) = spawn(path, &args)?;
        let capacity = (opts.audio_buffer_seconds.max(0.1)
            * opts.sample_rate as f32
            * f32::from(opts.channels)) as usize;
        let buffer = PcmBuffer::new(opts.sample_rate, opts.channels, capacity);
        let b = buffer.clone();
        std::thread::Builder::new()
            .name("audio-decode".into())
            .spawn(move || {
                let mut bytes = vec![0u8; 16 * 1024];
                let mut carry: Vec<u8> = Vec::new();
                loop {
                    let n = match out.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    carry.extend_from_slice(&bytes[..n]);
                    let whole = carry.len() / 4 * 4;
                    let samples: Vec<f32> = carry[..whole]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c))
                        .collect();
                    carry.drain(..whole);
                    if !b.push(&samples) {
                        return;
                    }
                }
                b.finish();
            })?;
        Ok(Self {
            buffer,
            _kill: Kill(child),
        })
    }
}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        self.buffer.cancel();
    }
}

/// A file opened for playback.
pub struct Video {
    pub path: PathBuf,
    pub info: VideoInfo,
    pub video: VideoDecoder,
    pub audio: Option<AudioDecoder>,
}

impl Video {
    pub fn open(path: &Path, opts: &DecodeOptions) -> Result<Self> {
        let info = probe(path)?;
        let video = VideoDecoder::open(path, &info, opts)?;
        let audio = if opts.audio && info.audio.is_some() {
            Some(AudioDecoder::open(path, opts)?)
        } else {
            None
        };
        Ok(Self {
            path: path.to_path_buf(),
            info,
            video,
            audio,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_to_even_sizes() {
        assert_eq!(output_size(800, 600, None), (800, 600));
        assert_eq!(output_size(800, 600, Some(1024)), (800, 600));
        assert_eq!(output_size(800, 600, Some(401)), (400, 300));
        assert_eq!(output_size(640, 273, Some(320)), (320, 136));
    }

    #[test]
    fn pcm_buffer_clock_underrun_and_end() {
        let b = PcmBuffer::new(4, 2, 1024);
        assert!(b.push(&[1.0; 8]));
        let mut out = [0.0; 4];
        assert_eq!(b.pop(&mut out), Some(4));
        assert_eq!(out, [1.0; 4]);
        assert_eq!(b.position(), 0.5);
        assert_eq!(b.pop(&mut out), Some(4));
        // Underrun before the end: silence, the clock keeps running.
        assert_eq!(b.pop(&mut out), Some(4));
        assert_eq!(out, [0.0; 4]);
        assert_eq!(b.position(), 1.5);
        b.finish();
        assert_eq!(b.pop(&mut out), None);
        assert!(b.is_finished());
    }
}
