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
Spatial attenuation: rodio spatial audio has no min/max distance, so `MinDistance/MaxDistance` are parsed but not applied.

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
