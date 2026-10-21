# Cutscene video (`crates/video`)

`gothic-video` plays the game's Bink cutscenes (`.bik`), and anything else ffmpeg decodes, in
Bevy. Decoding runs in **ffmpeg subprocesses**, so nothing is linked natively and the build
needs no ffmpeg headers or libraries:

* video: `ffmpeg -i FILE -an -f rawvideo -pix_fmt rgba -` (optionally `-vf scale=W:H`), read
  frame by frame on a worker thread into a bounded queue;
* audio: `ffmpeg -i FILE -vn -f f32le -ac 2 -ar 48000 -`, read into a bounded PCM buffer;
* stream info: `ffprobe -print_format json -show_streams -show_format`.

The processes are killed when playback ends or is skipped.

## Runtime requirement: ffmpeg

The player needs `ffmpeg` and `ffprobe` at runtime, built with the **Bink decoders**
(`binkvideo`, `binkaudio_rdft` / `binkaudio_dct`). These are part of standard ffmpeg builds:
all common Linux distributions, Homebrew, and the Windows builds from gyan.dev or BtbN.
The executables are looked up in this order:

1. `GOTHIC_FFMPEG` / `GOTHIC_FFPROBE` environment variables (full paths);
2. `ffmpeg[.exe]` / `ffprobe[.exe]` next to the game executable (the place to ship them);
3. `PATH`.

`gothic_video::ffmpeg::available()` and `supports_bink()` report what was found. Without
ffmpeg, or without a Bink decoder, `PlayCutscene` immediately answers with
`CutsceneFinished { error: Some(..) }` and the game continues as if the video had been
skipped.

Licensing: ffmpeg runs as a separate program, so its LGPL/GPL terms don't extend to the game.
If you ship ffmpeg binaries, include their license and source offer.

## Player (`--features bevy`)

```rust
app.add_plugins(CutscenePlugin::default());            // decode options, skip keys, subtitles
play.write(PlayCutscene { path, subtitles: Some(Subtitles::parse_srt(&srt)?), skippable: true });
// later
for f in finished.read() { /* f.skipped, f.error, f.frames_shown, f.frames_dropped */ }
app.add_systems(Update, gameplay.run_if(not(cutscene_playing)));
```

* Full-screen black UI layer with the video letterboxed to its aspect ratio.
* **Sync:** the audio output's playback position is the master clock. Frames whose time has
  passed are shown; frames that are already late by the next tick are dropped. Without audio,
  or when the audio output stalls (no device), wall time drives playback.
* **Skip:** Esc, Space, Enter or a left click (configurable), or a `SkipCutscene` message.
* **Subtitles hook:** every change is sent as `CutsceneSubtitle { text }`, so the game's own
  subtitle UI can show it. The player also draws them unless `show_subtitles` is `false`.
  Cues come from `.srt` or are built from the game's string table as `Subtitles { cues }`.

## Tests and demo

* `cargo test -p gothic-video`. The tests generate their videos with ffmpeg (`testsrc2` +
  sine as MPEG-4/PCM AVI, solid colour as FFV1 MKV). They check:
  - frame count, timestamps and pixel values;
  - audio length and level;
  - downscaling and frame dropping;
  - that Bink is gated on the decoder being present.
  ffmpeg cannot encode Bink, so the Bink path is the same generic path with a codec check.
  When ffmpeg is missing, the tests print `skipped` and pass.
* `cargo run -p gothic-video --example cutscene --features demo` generates a 4-second test
  video with subtitles and plays it. Pass `-- path/to/video.bik [subs.srt]` to play a real
  file.
