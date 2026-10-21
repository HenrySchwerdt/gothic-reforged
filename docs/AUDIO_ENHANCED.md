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

### Integration hooks (not done in this PR)

* `apps/game`: an "Enhanced audio" option that inserts the plugin and sets
  `EnhancedAudio::settings` (the struct is serde, so it can live in the settings file).
* The game's sound emitters spawn `AudioPlayer<EnhancedSound>` instead of the faithful
  player when the option is on; reverb zones come from level data or a hand-made table.
* `OcclusionProbe` wraps the physics raycast against static level geometry.

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

`cargo test -p gothic-audio-plus`: ITD follows the side, head shadow is frequency dependent,
sources behind are duller, occlusion is monotonically quieter and duller, the probe tells
obstruction from occlusion, bigger presets ring longer, zones blend smoothly and priority
wins, the disabled voice is exactly the equal-power pan, and long noise stays finite and
deterministic.
