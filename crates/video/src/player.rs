//! Bevy cutscene player: a full-screen video texture with letterboxing, the PCM stream as a
//! `bevy_audio` source (its position is the master clock), skip input and a subtitles hook.
//!
//! Send [`PlayCutscene`]; the player answers with [`CutsceneFinished`]. While it runs,
//! [`cutscene_playing`] is true (use it as a run condition to pause gameplay).
use crate::decode::{DecodeOptions, PcmBuffer, Video};
use crate::subtitles::Subtitles;
use crate::sync::FrameSync;
use bevy::asset::RenderAssetUsages;
use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct CutscenePlugin {
    pub decode: DecodeOptions,
    /// Draw subtitles (otherwise only [`CutsceneSubtitle`] messages are sent).
    pub show_subtitles: bool,
    pub skip_keys: Vec<KeyCode>,
    /// The left mouse button skips too.
    pub skip_on_click: bool,
}

impl Default for CutscenePlugin {
    fn default() -> Self {
        Self {
            decode: DecodeOptions::default(),
            show_subtitles: true,
            skip_keys: vec![KeyCode::Escape, KeyCode::Space, KeyCode::Enter],
            skip_on_click: true,
        }
    }
}

/// Play a video full screen.
#[derive(Message, Clone, Debug)]
pub struct PlayCutscene {
    pub path: PathBuf,
    pub subtitles: Option<Subtitles>,
    pub skippable: bool,
}

impl PlayCutscene {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            subtitles: None,
            skippable: true,
        }
    }
}

/// Stop the running cutscene (as if skipped).
#[derive(Message, Clone, Debug)]
pub struct SkipCutscene;

#[derive(Message, Clone, Debug, PartialEq)]
pub struct CutsceneFinished {
    pub path: PathBuf,
    pub skipped: bool,
    /// The video could not be played (missing file, no ffmpeg, no Bink decoder, ...).
    pub error: Option<String>,
    pub frames_shown: u64,
    pub frames_dropped: u64,
}

/// The subtitle changed (`None`: no subtitle now). Sent even when the player draws them.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct CutsceneSubtitle {
    pub text: Option<String>,
}

#[derive(Resource)]
struct Settings {
    decode: DecodeOptions,
    show_subtitles: bool,
    skip_keys: Vec<KeyCode>,
    skip_on_click: bool,
}

/// The running cutscene.
#[derive(Resource)]
pub struct ActiveCutscene {
    pub path: PathBuf,
    video: Mutex<Video>,
    sync: FrameSync,
    image: Handle<Image>,
    root: Entity,
    subtitle_text: Option<Entity>,
    audio: Option<(Entity, Arc<PcmBuffer>)>,
    subtitles: Option<Subtitles>,
    current_subtitle: Option<String>,
    skippable: bool,
    /// Playback time (seconds).
    pub time: f64,
    last_audio: f64,
    audio_stall: f32,
}

/// Run condition: a cutscene is on screen.
pub fn cutscene_playing(c: Option<Res<ActiveCutscene>>) -> bool {
    c.is_some()
}

impl Plugin for CutscenePlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<PcmStream>()
            .insert_resource(Settings {
                decode: self.decode.clone(),
                show_subtitles: self.show_subtitles,
                skip_keys: self.skip_keys.clone(),
                skip_on_click: self.skip_on_click,
            })
            .add_message::<PlayCutscene>()
            .add_message::<SkipCutscene>()
            .add_message::<CutsceneFinished>()
            .add_message::<CutsceneSubtitle>()
            .add_systems(Update, (start, play).chain());
    }
}

/// The audio of a cutscene as a `bevy_audio` source.
#[derive(Asset, TypePath, Clone)]
pub struct PcmStream(pub Arc<PcmBuffer>);

impl Decodable for PcmStream {
    type Decoder = PcmSource;

    fn decoder(&self) -> PcmSource {
        PcmSource {
            buf: self.0.clone(),
            chunk: vec![0.0; 1024],
            pos: 0,
            len: 0,
        }
    }
}

pub struct PcmSource {
    buf: Arc<PcmBuffer>,
    chunk: Vec<f32>,
    pos: usize,
    len: usize,
}

impl Iterator for PcmSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos == self.len {
            self.len = self.buf.pop(&mut self.chunk)?;
            self.pos = 0;
        }
        let s = self.chunk[self.pos];
        self.pos += 1;
        Some(s)
    }
}

impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(self.buf.channels).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.buf.sample_rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn start(
    mut commands: Commands,
    mut requests: MessageReader<PlayCutscene>,
    settings: Res<Settings>,
    active: Option<Res<ActiveCutscene>>,
    mut images: ResMut<Assets<Image>>,
    mut streams: ResMut<Assets<PcmStream>>,
    mut finished: MessageWriter<CutsceneFinished>,
) {
    let Some(req) = requests.read().last().cloned() else {
        return;
    };
    if active.is_some() {
        warn!(
            "cutscene {} requested while another plays; ignored",
            req.path.display()
        );
        return;
    }
    let video = match Video::open(&req.path, &settings.decode) {
        Ok(v) => v,
        Err(e) => {
            warn!("cutscene {}: {e:#}", req.path.display());
            finished.write(CutsceneFinished {
                path: req.path,
                skipped: false,
                error: Some(format!("{e:#}")),
                frames_shown: 0,
                frames_dropped: 0,
            });
            return;
        }
    };
    let (w, h) = (video.video.width, video.video.height);
    let image = images.add(Image::new_fill(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let mut subtitle_text = None;
    let root = commands
        .spawn((
            Name::new("Cutscene"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(i32::MAX - 16),
        ))
        .with_children(|p| {
            p.spawn((
                ImageNode::new(image.clone()),
                Node {
                    max_width: percent(100),
                    max_height: percent(100),
                    width: percent(100),
                    aspect_ratio: Some(w as f32 / h as f32),
                    ..default()
                },
            ));
            if settings.show_subtitles {
                subtitle_text = Some(
                    p.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(26.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TextLayout::justify(Justify::Center),
                        Node {
                            position_type: PositionType::Absolute,
                            bottom: percent(7),
                            ..default()
                        },
                    ))
                    .id(),
                );
            }
        })
        .id();
    let audio = video.audio.as_ref().map(|a| {
        let buf = a.buffer.clone();
        let e = commands
            .spawn((
                AudioPlayer(streams.add(PcmStream(buf.clone()))),
                PlaybackSettings::DESPAWN,
            ))
            .id();
        (e, buf)
    });
    info!(
        "cutscene {}: {}x{} {:.2} fps {} ({:.1} s)",
        req.path.display(),
        w,
        h,
        video.info.fps,
        video.info.codec,
        video.info.duration
    );
    commands.insert_resource(ActiveCutscene {
        path: req.path,
        video: Mutex::new(video),
        sync: FrameSync::default(),
        image,
        root,
        subtitle_text,
        audio,
        subtitles: req.subtitles,
        current_subtitle: None,
        skippable: req.skippable,
        time: 0.0,
        last_audio: 0.0,
        audio_stall: 0.0,
    });
}

#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut skips: MessageReader<SkipCutscene>,
    active: Option<ResMut<ActiveCutscene>>,
    mut images: ResMut<Assets<Image>>,
    mut texts: Query<&mut Text>,
    mut subtitle_out: MessageWriter<CutsceneSubtitle>,
    mut finished: MessageWriter<CutsceneFinished>,
) {
    let Some(mut c) = active else {
        skips.clear();
        return;
    };
    let c = &mut *c;
    let pressed = keys
        .as_ref()
        .is_some_and(|k| settings.skip_keys.iter().any(|key| k.just_pressed(*key)))
        || (settings.skip_on_click
            && mouse
                .as_ref()
                .is_some_and(|m| m.just_pressed(MouseButton::Left)));
    let skipped = skips.read().count() > 0 || (c.skippable && pressed);

    // Clock: the audio position while audio plays, wall time otherwise (or when the audio
    // output is stalled, e.g. no audio device).
    let dt = f64::from(time.delta_secs());
    let mut audio_done = true;
    if let Some((_, buf)) = &c.audio {
        let pos = buf.position();
        audio_done = buf.is_finished();
        if pos > c.last_audio {
            c.last_audio = pos;
            c.audio_stall = 0.0;
            c.time = pos;
        } else {
            c.audio_stall += time.delta_secs();
            if c.audio_stall > 0.3 {
                audio_done = true;
                c.time += dt;
            }
        }
    } else {
        c.time += dt;
    }

    let video_done = {
        let mut v = c.video.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(frame) = c.sync.advance(c.time, || v.video.try_next())
            && let Some(mut img) = images.get_mut(&c.image)
        {
            img.data = Some(frame.rgba);
        }
        v.video.is_finished() && !c.sync.has_pending()
    };

    let sub = c
        .subtitles
        .as_ref()
        .and_then(|s| s.at(c.time))
        .map(str::to_string);
    if sub != c.current_subtitle {
        c.current_subtitle = sub.clone();
        if let Some(e) = c.subtitle_text
            && let Ok(mut t) = texts.get_mut(e)
        {
            t.0 = sub.clone().unwrap_or_default();
        }
        subtitle_out.write(CutsceneSubtitle { text: sub });
    }

    if skipped || (video_done && audio_done) {
        commands.entity(c.root).despawn();
        if let Some((e, _)) = &c.audio {
            commands.entity(*e).try_despawn();
        }
        if c.current_subtitle.is_some() {
            subtitle_out.write(CutsceneSubtitle { text: None });
        }
        finished.write(CutsceneFinished {
            path: c.path.clone(),
            skipped,
            error: None,
            frames_shown: c.sync.shown,
            frames_dropped: c.sync.dropped,
        });
        commands.remove_resource::<ActiveCutscene>();
    }
}
