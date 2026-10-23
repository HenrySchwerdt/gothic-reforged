# Enhanced audio (opt-in): `crates/audio-plus`

`gothic-audio` reproduces the original FMOD mix (equal-power pan, inverse rolloff, no
filters) and stays the default. `gothic-audio-plus` adds three switchable features on top:

| Feature | What it does | Module |
|---|---|---|
| HRTF | Spherical-head model: interaural time difference (Woodworth), head shadow (Brown–Duda one-pole/zero), a pinna notch that moves with elevation, and a high shelf for sources behind the listener. Synthetic: no measured HRIR set is shipped. | `hrtf` |
| Occlusion / obstruction | A direct ray plus a ring of offset rays (`probe`) give `obstruction` (direct line blocked) and `occlusion` (the paths around it blocked too). `OcclusionTuning` turns them into gain, a log-interpolated low-pass cutoff (20 kHz → 700 Hz) and a reverb send cut. | `occlusion` |
| Reverb zones | Freeverb-style reverb (8 combs + 4 allpasses per side, pre-delay, width). Presets Outdoor / Forest / Indoor / Hall / Cave; `ReverbZone` (sphere or box, border fade, priority) blended by `zone_params` at the listener. | `reverb` |

`EnhancedVoice` chains them for one mono source into stereo. Parameters change once per
block and are smoothed per sample, so moving sources and toggling features are click-free.
With `EnhancedSettings::enabled == false` (the default) the voice is the faithful equal-power
pan and nothing else: the low-pass is bypassed bit-exactly when fully open and the reverb is
not run.

The presets and tuning values are placeholders chosen by ear on synthetic signals; they are
not taken from the original game and should be tuned against real levels.

## Bevy integration (`--features plugin`)

```rust
app.add_plugins(gothic_audio_plus::plugin::EnhancedAudioPlugin);
app.insert_resource(OcclusionProbe(Box::new(|a, b| physics_ray_blocked(a, b))));
app.world_mut().resource_mut::<EnhancedAudio>().settings = EnhancedSettings::all_on();

let sound = sounds.add(EnhancedSound::decode(&bytes, true)?.instance());
commands.spawn((AudioPlayer(sound), EnhancedEmitter::default(), Transform::from_xyz(..)));
```

* `EnhancedSound` is a `bevy_audio` `Decodable`: it decodes Ogg/WAV through
  `gothic_audio::pcm::decode`, mixes to mono and plays through `EnhancedVoice` in
  256-sample blocks on the audio thread.
* `update_voices` (PostUpdate) computes per emitter the listener-space direction (from the
  `SpatialListener`), the faithful distance gain `gothic_audio::mix::inverse_gain`, the
  occlusion probe and the zone reverb at the listener, and hands them to the voice.
* The voice parameters live on the asset: give every simultaneously playing emitter its own
  `EnhancedSound::instance()` (the samples are shared through an `Arc`).
* Without an `OcclusionProbe` resource nothing is occluded.

### Bridge from `gothic-audio` (`bridge::GothicAudioBridgePlugin`)

Positional channels `gothic-audio` spawns (sound effects, ambient shots and emitters: an
`Emitter3d` plus `AudioPlayer<AudioSource>`, no `VoiceLine`) are switched to enhanced voices
before `bevy_audio` starts them: the sound is decoded once per source and cached, each play
gets its own `instance()`, the channel's base volume becomes the playback volume, and the
template distances (in world units via `AudioSettings::units_per_meter`) the
`EnhancedEmitter`. Channel budget and priorities stay with `gothic-audio`. Dialogue voices
stay faithful. `EmitterBlocking` on an emitter carries a host-computed probe result in place
of the `OcclusionProbe` closure (for hosts whose collision lives in a resource).

### In the game (`--audio-plus`)

`gothic-reforged --play --audio-plus` (or `GR_AUDIO_PLUS=1`) adds `EnhancedAudioPlugin` and
the bridge with every feature on (`apps/game/src/walk/plus.rs`):
* occlusion: `probe` against the world `CollisionWorld` (centimetres), every sixth frame per
  emitter, staggered; a source mounted on a wall is not behind it (25 cm grace);
* reverb: no zones in the level data yet, so a ray up from the listener picks the room
  (no ceiling within 9 m: outdoor; under 4.5 m: indoor; else hall), four times a second.

Not verified here (no game data in CI): how it sounds in real scenes, the occlusion probe's
cost with many emitters, and the main-thread decode hitch on a sound's first play.

## Demo and cost

`cargo run --release -p gothic-audio-plus --example scene -- --out <dir>` renders a
12 s synthetic scene without an audio device: a click-and-noise source orbits the listener at
5 m, passes behind a wall, then the listener walks into a cave zone. It writes
`faithful.wav`, `enhanced.wav` and `cost.json` (best of three runs, one voice at 48 kHz).
Measured in the CI container (shared CPU, other builds running, so read as an order of
magnitude):

| Run | ns / sample |
|---|---|
| faithful | 22 |
| HRTF only | 44 |
| occlusion only (filter + probe per block) | 15 |
| reverb only | 63 |
| all enhanced | 89 |

About 90 ns per sample is 4.3 ms of CPU per second of audio per voice, i.e. one core carries
roughly 200 enhanced voices at 48 kHz. The reverb dominates; a shared reverb bus per zone
instead of one per voice is the obvious next step when many voices play at once.

In the rendered files the occluded stretch (about 0.6–1.1 s) loses most of its high-frequency
energy, sources behind and to the left come out duller through head shadow and the back
shelf, and the last seconds carry the cave tail.

## Tests

`cargo test -p gothic-audio-plus --features plugin`: the bridge switches positional channels
(volume and distances kept, one decode per sound, own voice parameters per play), leaves
undecodable sounds and the disabled path faithful, and host blocking reaches the voice. ITD follows the side, head shadow is frequency dependent,
sources behind are duller, occlusion is monotonically quieter and duller, the probe tells
obstruction from occlusion, bigger presets ring longer, zones blend smoothly and priority
wins, the disabled voice is exactly the equal-power pan, and long noise stays finite and
deterministic.
