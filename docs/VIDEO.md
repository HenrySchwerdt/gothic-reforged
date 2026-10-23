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
If you ship ffmpeg binaries, include their license and source offer (prefer an LGPL build; see `THIRD_PARTY_NOTICES.md`). Do not switch to linking libav* in-process (static or dynamic) without a licence review: that would make the LGPL/GPL linking terms apply to the game binary.

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

## In the game (`--video FILE`, `GR_VIDEO=1`)

Opt-in; without it no player is added. Glue: `apps/game/src/walk/cloud/video.rs`.

```sh
# a file, on start of --walk or --play
cargo run -p gothic-reforged --features viewer -- --play --video "D:/SteamLibrary/steamapps/common/Gothic 3/Data/Video/G3_Intro.bik"
# the original intro: G3_Intro.bik, looked up (case-insensitively) in the data directory,
# Data/Video, and the installation's Video folder
GR_VIDEO=1 cargo run -p gothic-reforged --features viewer -- --play
```

* The start log says whether ffmpeg and its Bink decoder were found. Without them the player
  answers at once with `CutsceneFinished { error }`, the log says `video: ... not played: ...`
  and the game goes on.
* No new Cargo feature: ffmpeg runs as a subprocess, so the game links nothing new.

Not wired / not verified:

* **Real `.bik` playback was not verified** (no game data here). To verify, with a Bink-capable
  ffmpeg on `PATH` or in `GOTHIC_FFMPEG` / `GOTHIC_FFPROBE`:
  `ffmpeg -hide_banner -decoders | grep -i bink` (expect `binkvideo` and `binkaudio_*`), then
  `cargo run -p gothic-video --example cutscene --features demo -- "<Gothic 3>/Data/Video/G3_Intro.bik"`
  and the game command above. The intro's location is an assumption (`gCSession::Start` plays
  `G3_Intro.bik`; the folder was not checked against the installation).
* Gameplay is **not paused** while the video plays (the world streams and the core ticks
  underneath), and the walk / play input also sees the skip keys (Esc, Space, Enter, click).
* Story cutscenes (`EndGame` credits etc.) are not triggered from the game yet; only the start
  video is.

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
