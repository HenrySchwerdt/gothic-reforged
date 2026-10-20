# Audio: archives, formats, voice/music/sfx data

Recorded 2026-10-20 against Gothic 3 1.75.20108.14 (read-only install, VFS with patch overlays applied). Code: crate `gothic-audio`
(`crates/audio`: `pcm`, `speech`, `music`, `mul`, `sfx`, `library`, `plugin`). Reproduce the numbers with
`cargo run --release -p gothic-audio --example audit` (writes nothing; output shown below was captured to the gitignored
`.local/audio-audit.txt`). No original audio is committed; tests use synthetic fixtures.

Evidence legend as in `PROPERTY_STREAMS.md`: **[V]** verified over the whole installation, **[I]** inferred, **[U]** unknown.

## 1. Inventory [V]

There is **no proprietary audio container**: every sound is plain Ogg Vorbis or RIFF/WAVE inside the normal `.pak` archives.

| Archive (+ patch) | Folder | Format | Files |
| --- | --- | --- | ---: |
| `Speech_German.pak` (+`.p00`) | `german_ogg/` | Ogg Vorbis, 44.1 kHz mono | 24 289 (+22 in p00) = 24 308 visible |
| `Speech_German.pak` (+`.p00`) | `german_lipsyncdata/` | `.xlip` lip-sync | 24 295 (+16) |
| `Speech_English.pak` (+`.p00`) | `english_ogg/` | Ogg Vorbis, 44.1 kHz mono | 24 234 (+16) = 24 250 visible |
| `Speech_English.pak` (+`.p00`) | `english_lipsyncdata/` | `.xlip` | 24 238 (+12) |
| `Music.pak` (+`.p00`) | `fragments/` | Ogg Vorbis, 48 kHz stereo | 60 (+2) = 62 files, 61 used by the table |
| `Music.pak` | `transitions/` | Ogg Vorbis | 42 |
| `Sound.pak` | `creature/ environment/ foley/ magic/ gui/ inventory/ event/ voice/` | 1 611 Ogg + 269 WAV (PCM, 44.1 kHz mono) | 1 880 |
| `Library.pak` | `g3.mul` | music table (Genome property stream) | 1 |
| `Templates.pak` | `sfx/**/*.tple` | sound emitter templates | 44 usable (+ deleted copies) |

`Sound.pak` by folder: creature 838 (836 Ogg), foley 680 (footsteps are the 257 WAVs, per surface and gait), environment 272 (birds 197, water 39,
wind 10, ambience 7, ...), magic 56, inventory 14, voice 10 (WAV, fight grunts), gui 6, event 3, plus `nosound.ogg`.
Music fragments total 7 570 s (2 h 6 min); every shipped speech file decodes (sampled: 487 files at every 100th, 0 failures, 0 silent).
Only German and English audio is shipped; the other eight text languages exist only as subtitles (`Strings.pak`).

Decoders (`gothic_audio::pcm`): Ogg via `lewton` (pure Rust), WAV parsed by hand (PCM 8/16 bit, chunk walk, truncated `data` clamped, other
format tags rejected); sample ceiling 96 M. Playback hands the original bytes to Bevy `AudioSource` (rodio decodes Ogg/WAV).

## 2. Voice-over: key to file [V]

Files are named after the string-table key (case-insensitive, all lowercase on disk):

* Dialog lines: `<lang>_ogg/info_<speaker>_<key tail>_<lang>.ogg` for string key `INFO_<KEY TAIL>`. `<speaker>` is `<voice class>_<npc>`
  (`hum_fat_bearish_pedar`, `gorn_gorn`, `ass_elite_proud_kapdun_assguard`) or `player_pc_hero`. `_0`, `_1`, ... suffixes are part of the key
  (`INFO_BPANKRATZ320060214102844_0`). Several files can share a key when different voices say the same line (`INFO_FILLER325`):
  `SpeechIndex::lookup(key, speaker_hint)` picks by speaker substring.
  Example: `INFO_BPANKRATZ320060306120442` = `german_ogg/info_gorn_gorn_bpankratz320060306120442_german.ogg`, subtitle "Ich bin nicht wirklich scharf drauf, wieder da reinzugehen."
* Stock voice lines: `<lang>_ogg/<svm key>_<lang>.ogg` with key `SVM_<VoiceClass>_<Event>` verbatim (`svm_hum_warrior_hard_thisway_german.ogg`).
  The info script command `SaySVM` stores only the event in `ID1` (`ThoughtSo`, `Teach_Axe_3_01`); the voice class comes from the speaking NPC's
  `Voice`. `SpeechIndex::svm(voice, event)` and `PlayVoice::svm` implement exact match, then same race prefix, then any voice.
* `SaySound` (6 steps), `PlayMusic` (5 steps, argument = music trigger [I]) exist in info scripts; not wired yet.
* `.xlip` (lip-sync, one per `.ogg`, same stem): `u16 version (1)`, `u32 n`, `n x (f32 seconds, u32 viseme id 0..12)`; total size matches
  `6 + 8n` for the sample inspected (302 bytes, n = 37) [V frame, I meaning]. Not decoded further.

### Coverage (string keys that have text in the language and a voice file)

| | German | English |
| --- | ---: | ---: |
| `INFO_*` keys with text | 12 217 | 12 217 |
| ... with a voice file | 11 884 (**97.3 %**) | 11 881 (**97.2 %**) |
| `SVM_*` keys with text | 4 791 | 4 345 |
| ... with a voice file | 4 296 (**89.7 %**) | 4 285 (**98.6 %**) |
| Distinct keys spoken by info scripts (`Say*` steps with text) | 11 976 | 11 976 |
| ... with a voice file | 11 858 (**99.0 %**) | 11 856 (**99.0 %**) |
| `Say` steps with a voice file | 12 428 / 12 547 | 12 426 / 12 547 |
| Voice files not reached by any string key | 8 022 of 24 308 | 8 034 of 24 250 |

Misses are real data gaps, not lookup errors: e.g. no file at all contains `20060217173051` (`INFO_BPANKRATZ320060217173051`), and
`SVM_Ass_Elite_Hard_AdanosTeacher_00` has no recording in the `ass_elite_hard` voice (only `_insidious`/`_proud`). The unreached files are almost all
`svm_*` recordings of voice classes that have no string key of their own (7 893 of 8 022 German; they reuse the text of a sibling voice class) plus
132 `info_*` files whose key is absent from the string table.

## 3. Music [V table, I runtime semantics]

`Library.pak/g3.mul` is a `GENOMFLE` file: `u16 version`, `u32 size`, `u16 2`, then groups of `u16 1`, `u32 count`, `count` class instances
(accessor + class name + object, the shared Genome grammar of `PROPERTY_STREAMS.md`), terminated by `0xDEADBEEF`, `u8 1`, `u32 n` and a
`u16`-length-prefixed string table that ends exactly at end of file. All 1 416 properties decode typed. Contents: 8 `gCMusicTrigger`,
62 `gCMusicFragment`, 42 `gCMusicTransition`.

* `gCMusicFragment`: `Location` (35 values: `myrtana`, `varant`, `dungeon`, `trelis`, `vengard`, `thedig`, `xardasindoor`, `orcmine01`, `locationidyllic`, ...),
  `Situation` (`explore`, `fight`; Myrtana story cues `beliar`, `dead`, `extro`, `innos`, `revolution`, `revolutionwon`, `showdown`, `soultravel`, `title`),
  `DayTime` (`day`/`night`; night exists for myrtana, ominouswoods, trelis, varant, vengard), `Variation`/`NextVariation` (alternates chain:
  dungeon, locationspooky, varant `0 -> 1 -> 0`; empty = repeat itself; `-1` = one-shot cue), `Sample` (authoring name `x.wav`, shipped as
  `fragments/x.ogg`), `Volume`, `LeadInEnd`, `LeadOutBegin`, `Duration` (ms). `Duration` equals the decoded Ogg length to the millisecond
  (Myrtana explore 329.083 s). `LeadInEnd` is 0 and `LeadOutBegin == Duration` for all 62 entries. One entry (`trelis_explore_night_01_new`) has no file.
* **Every fragment begins with 10.0 s of digital silence** (1.1 s for `myrtana_title`, 10 to 13.8 s elsewhere); the runtime skips it by default
  (`AudioSettings::skip_lead_silence`). Whether the original engine plays through it is [U].
* `gCMusicTransition`: stingers in `transitions/` with `ConnectIn/ConnectOut` ids (0..30; the pairing to fragments is [U]), `FadeOutBegin/End`,
  `FadeInBegin/End`, `Offset`, `Timing` (enum, always 3), `Duration`. Not used by the runtime yet (plain equal-power cross-fade instead).
* `gCMusicTrigger`: `Loc_Arena`, `Loc_Braga`, `Loc_Myrtana`, `Loc_Myrtana_Now`, `Loc_Varant` (location `faring`!), `Sit_Explore`, `Sit_Fight`, `Stop`
  with `Exit`/`Entrance` enums (`gEMusicTriggerTime`, `gEMusicFragmentPosition`, values unnamed) and `IsStopTrigger`. World data has music zone
  geometry (`Projects_compiled.pak/.../g3_myrtana_01_landscape_musiczones_01`, one sector) and templates carry `MusicLocation`/`MusicPlayed`
  properties (editor test templates, `Music_Reddock_Dungeon`); the zone-to-trigger mapping is [U], so locations are driven by name today.

Runtime state machine (`MusicState`, `MusicCommand`): location + situation + night; resolution falls back requested daytime -> day, requested
situation -> `explore`, then the default region (`myrtana`). A change cross-fades (3 s equal-power); single fragments loop, variation chains and
`-1` cues play once and then continue/return to exploring.

## 4. Sound effects and ambience [V]

Sound definitions are ordinary templates under `Templates.pak/sfx/` whose reference entity has an `eCAudioEmitter_PS` property set:
`Sound` (sample name, authoring `.wav` extension even when the shipped file is `.ogg`), `Enabled`, `AutoReset`, `AutoKill`, `UseStream`, `Volume`,
`MinDistance`/`MaxDistance` (world units; 100 / 100000 for waterfalls), `SpawningMode` (`eEAudioEmitterMode`, 1 = repeating [I]),
`SecondsBetweenRepeats`, `RepeatProbability`, `UseMaxRepeats`, `MaxNumRepeats`. 44 non-deleted templates define emitters (water/waterfall/wind/ambience
`SFX_Env_*`, magic `SFX_Magic_*`, animation/collision foley); 69 templates anywhere carry the property set (placed in the world as ambient emitters).
Other audio-adjacent properties: `CollisionSound*`/`SpawningSound*` (enum `eECollisionSound`/`eESpawningSound`, 471 / 476 templates; the enum-to-sample
tables live in code, not data [U]), NPC `Voice`, `gEAmbientAction`.
`SoundIndex` resolves a name by stem (extension ignored, Ogg preferred) and falls back to numbered variants (`foo` -> `foo_01..foo_07`).
Spatial attenuation: see section 7 (FMOD inverse rolloff in centimetres); template distances are used for `PlaySfx` when the name is a template.

## 5. Plugin API (`gothic_audio`)

* `AudioLibrary::open(data_dir, "German"|"English")` indexes everything once (speech, sounds, music table, sfx templates, string table); `GothicAudioPlugin::new(lib)`; headless tools use `headless_app`.
* Messages in: `PlayVoice { key, speaker_entity, speaker_hint, spatial, volume }` (`PlayVoice::new / spatial / svm`), `PlaySfx { name, position, volume }`,
  `MusicCommand::{Trigger(id), SetLocation, SetSituation, SetNight, Stop}`. Messages out: `VoiceStarted` (subtitle, path, lip-sync path, duration),
  `VoiceFinished`, `VoiceMissing`. Resources: `AudioSettings` (volumes, `units_per_meter` = 100 because Gothic units are centimetres, cross-fade),
  `MusicState` (observable `now_playing`). Spatial voices become children of the speaker entity (so they follow it) and need a `SpatialListener`.
* Examples: `say <KEY> [--lang English] [--speaker HINT] [--spatial] [--no-play]`, `music [--secs N] [--location L] [--night] [--fight-at S]`, `audit`.

## 6. Open items

1. Zone to music-trigger mapping from world data, and `PlayMusic` argument semantics.
2. Transition stinger selection (`ConnectIn/ConnectOut`), `Entrance`/`Exit` enum meanings, whether the 10 s silent lead-in is played.
3. `.xlip` viseme table and how it maps to head morph targets.
4. Emitter attenuation curves and `SpawningMode` values; `eECollisionSound`/`eESpawningSound` tables (code).
5. Text for `svm_*` recordings of voice classes without a string key (fall back to a sibling class's text).

## 7. Original mix (FMOD Ex runtime capture) [V]

Method: `.local/research/audio/fmod_dbg.py` launches `Gothic3Final.exe` under a ctypes debugger (REA's Frida provider is not installed here), sets INT3
breakpoints on the `fmodex.dll` C++ exports that `Engine.dll` imports (76 `?name@Class@FMOD@@QAG..` imports: `System::init/playSound/createSound/set3DSettings/
set3DListenerAttributes/createChannelGroup`, `Channel::setVolume/setPaused/setPriority/set3DAttributes/set3DMinMaxDistance/setChannelGroup/setLoopCount/setMode`,
`ChannelGroup::setVolume/addGroup/setMute`; `__stdcall` members take `this` as the first stack argument) and logs every call (`fmod-trace.jsonl`, 31 835 calls,
Ardea new game: 68 s of menu, then 173 s in the world including the opening brawl). Analysis: `analyze.py`, `summ.py`, `orig_stats.py` (durations from
`crates/audio/examples/durations.rs`). All kept under the gitignored `.local/research/audio/`.

| Item | Captured value |
| --- | --- |
| `System::setSpeakerMode` / `init` | stereo (2) / `maxchannels 64`, flags 0 |
| `System::set3DSettings` | doppler 1.0, **distancefactor 100.0** (all distances are centimetres), rolloff scale 1.0 |
| Channel groups | `Master` > `Voice`, `Music`, `FX`, `Ambient` |
| Group volumes (set at 5.8 s and again from the options) | Master 1.0, Voice 1.0, **Music 0.2196 (= 56/255)**, **FX 0.8039 (= 205/255)**, Ambient 1.0: exactly `Sound.*Volume`/255 of `ge3.INI`; `ChannelGroup::setMute` toggles Voice/FX/Ambient on menu <-> game |
| Voice channel | group Voice, **priority 1**, min 500 cm, max 1 000 000 cm, volume 1.0, 3D stream (`mode 0x20000d0`), one `createSound` per play |
| FX channel (steps, hits, sword) | group FX, **priority 3**, min 150 cm (steps, 877 of 2 027), 80 (sword draw/whoosh), 90/75/100 (impacts), max unbounded (1e12), a few 90/30 000; volume 1.0 (creature steps 0.4-0.7), mode `0x2004150` (3D sample) |
| Ambient channel | group Ambient, **priority 4**, birds min 500 / max 1 000 000, volume 0.2 (0.3), fire loops min 25 / max 25 000 (loop -1), coast water 2D linear loop |
| Music channel | group Music, 2D stream, volume 1.0 (the group carries the level), no fades via `Channel::setVolume`; a `transition_NN` stinger starts 28 ms before/after the new fragment and plays over it; the story cue `myrtana_revolution_day_01` restarted via `transition_00` after 72 s |
| Listener | Gothic coordinates passed unchanged (left-handed, FMOD default), position = camera, forward/up from the camera |
| Rolloff mode | no rolloff flag in the create/setMode values (`0x...d0/150`) = FMOD default **inverse**: `gain = min / (min + rolloff * (d - min))` for `d > min`, 1 inside `min`, attenuation stops at `max` |

Activity in the 173 s world window (2 630 `playSound` calls):

| Group | plays | per min | max concurrent | mean concurrent |
| --- | ---: | ---: | ---: | ---: |
| Voice (`SVM_*_AARGH/DOWN/DEAD/DieEnemy` in the brawl) | 348 | 121 | 9 | 1.9 |
| FX | 2 177 | 755 | 25 | 9.2 |
| Ambient | 99 | 34 | 11 | 7.8 |
| Music (fragment + stinger) | 6 | 2 | 3 | 1.5 |

Takeaways: the original mix is **music 0.22 / fx 0.80 / voice 1.0 / ambient 1.0 and 0.2 per bird**; barks never exceed 9 at once; group maxima sum to 48 channels, below the 64 budget. The old plugin defaults (music 0.6, no FX/ambient groups) put the music 2.7x too loud relative to speech.

## 8. QA (2026-10-20, "Ton ist cursed")

Tools: `examples/qa_decode.rs`, `examples/tap_mix.rs`, `GR_AUDIO_TAP=path` (plugin tap, TSV: `t kind id file gain pan extra`).

### 8.1 Decode correctness (ffmpeg as ground truth)

25 files (20 voices, 5 music fragments) + 10 sfx (Ogg and WAV): our `pcm::decode` (i16) and the playback decoder (Bevy `AudioSource` -> rodio lewton/hound, f32)
match `ffmpeg -f s16le/f32le` in sample rate, channel count and frame count exactly, max sample difference 1 LSB (rounding) / 0.00000 (f32). Rates are
44.1 kHz mono (voice, sfx), 48 kHz or 44.1 kHz stereo (music; `locationgothic1theme` is 44.1 kHz). Resampling to the device rate is rodio's; no
rate-dependent bug found. **No decode bug.**

### 8.2 Bugs found and fixed

| # | Finding (measurement) | Fix |
| --- | --- | --- |
| 1 | Music default 0.6 vs original 0.2196; FX/ambient groups absent | `AudioSettings` defaults from `ge3.INI` /255 (`music 56/255`, `sfx 205/255`, `ambient 1.0`) |
| 2 | Spatial attenuation = rodio 1/d^2 (clamped at 1 unit) with a 100 u/m scale: a voice 3 m away played at 0.11, 6 m at 0.03, i.e. NPC dialog inaudible outside ~1 m; original is inverse with 5 m full volume (`mix::inverse_gain`, 0.5 at 10 m) | rodio unit scaled to 1 km so its term stays 1; per-frame `rolloff_3d` sets `volume = base x FMOD inverse(distance, min, max, rolloff)`; voices min 5 m / max 10 km, sfx from template or footstep default 1.5 m |
| 3 | **Left/right swapped**: rodio 0.22 `Spatial` gives the ear *farther* from the source the larger gain (`left_diff_modifier` grows with `left_dist - right_dist`) | `fix_listener_ears` swaps the ear offsets of every `SpatialListener` (no change to `play/cam.rs` needed) |
| 4 | Spawn frame: a new child voice's `GlobalTransform` is origin until propagation, tap showed 0.07 gain for one frame | tap logs one frame later; rolloff applied after propagation |
| 5 | Play mode never started music (no `SetLocation`; only `SetSituation(fight)` on an attack) and fight music never ended; no footsteps / SFX wired | `play/audio.rs`: `SetLocation("myrtana")` at start, fight -> explore after 12 s without target change, hero footsteps via `PlaySfx` |
| 6 | No channel budget / group priorities; unlimited SVM barks would overlap | `mix.rs`: 64 channels with priority (voice 1 < fx 3 < ambient 4) then quietest stealing; barks (`SVM_*`) limited to 1 per speaker and 9 total (original peak 9) |
| 7 | Transition stingers (`transition_NN`, played over the new fragment in the original) are not played; plain 3 s equal-power cross-fade | open: `ConnectIn/Out` pairing [U] |

Not bugs (checked): ~10 s silent fragment lead-in is cut (`skip_lead_silence`; whether the original plays the silence is still [U]); lines for keys without a recording emit `VoiceMissing` (no sound, no stall); dialog line sequencing has 15-30 ms gaps; no re-triggering per frame.

### 8.3 Our tap vs the original (scripted `--play --smoke`, 56.7 s of game time)

Offline render of `GR_AUDIO_TAP` (`examples/tap_mix.rs`, also `.local/audio-tap.mp3`; raw log `.local/research/audio/our-tap.tsv`):

| Measure | Ours (after fixes) | Original (brawl capture) |
| --- | --- | --- |
| Peak / clipped samples | 0.709 / 0 | not measurable (FMOD mixes to the device) |
| Music vs voice RMS | -40 vs -22 dBFS (music 18 dB under speech; group 0.2196, fragment `Volume` 1.0) | groups 0.2196 vs 1.0 |
| Simultaneous voices | 1 (dialog is sequential, 15-30 ms gaps) | 1.9 mean / 9 max (barks) |
| Voice gain at dialog distance 3 m / 5.4 m | 1.00 / 0.92 (inverse, min 5 m) | same law |
| Footstep gain at 3.45 m | 0.35 = 0.8039 (FX) x 1.5/(1.5+1.95) | FX group 0.8039, min 150 cm |
| Voice re-triggers per frame | none (each key started once, `VoiceFinished` follows the sink end) | n/a |
| Pan sign | speaker left of the camera -> pan -0.34, rodio ear factors left 0.83 / right 0.67 after the ear swap | FMOD stereo pan |

Remaining gaps (not wired in play mode, so nothing can spam or be wrong yet): NPC footsteps and combat hits/whooshes (the original plays ~755 FX/min in the
brawl), ambient emitters (birds, water, fire), SVM barks from the script layer (limits are ready: 1 per speaker, 9 total), music transition stingers, night
music, and the speaking cooldown inside the script runtime (not ported; the bark limit is the audio-side guard).
