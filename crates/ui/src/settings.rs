//! The settings behind the option pages of the menu (E32): audio, difficulty and controls.
//!
//! Every page edits a draft of one of these structs and writes it back with the key spelling of
//! the real `UserOptions.ini` (`[Options.Audio]`, `[Options.Difficulty]`, `[Options.Controls]`,
//! see `ENGINE_MAP.md` 11.2), so a file from the original game loads and saves. The structs own
//! no engine state: the host reads the resources and applies them (`ApplyAudio`,
//! `ApplyControls`, `ApplyDifficulty` tell it when).
use crate::userini::OptionsWrite;
use bevy::prelude::*;
use gothic_input::keys::DEFAULT_BINDINGS;
use gothic_input::{ActionMapper, Binding, ControlOptions, PhysicalKey, SessionKey};

// ------------------------------------------------------------------------------- audio
pub const AUDIO_SECTION: &str = "Options.Audio";

/// The five volume sliders of the audio page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Volume {
    Master,
    Voice,
    Music,
    Fx,
    Ambient,
}

/// Languages the text slider cycles through (the two speech packs of the install).
pub const LANGUAGES: [&str; 2] = ["German", "English"];

/// Speaker set-ups of the speaker slider (range 1..=7, default 2 = stereo; the names follow the
/// audio system's speaker modes [I: the name table is a string array of the Game DLL]).
pub const SPEAKER_MODES: [&str; 7] = [
    "Mono",
    "Stereo",
    "Quadro",
    "Surround",
    "5.1",
    "7.1",
    "5.1 Matrix",
];

/// `[Options.Audio]` of `UserOptions.ini`. Volumes are stored 0..=255 and shown as 0..=100 %.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct AudioOptions {
    pub master: u8,
    pub voice: u8,
    pub music: u8,
    pub fx: u8,
    pub ambient: u8,
    /// 1..=7, index + 1 into [`SPEAKER_MODES`].
    pub speaker: u8,
    pub voice_language: String,
    pub text_language: String,
    pub subtitle: bool,
}

impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            master: 255,
            voice: 255,
            music: 255,
            fx: 255,
            ambient: 255,
            speaker: 2,
            voice_language: "German".into(),
            text_language: "German".into(),
            subtitle: true,
        }
    }
}

impl AudioOptions {
    fn slot(&mut self, v: Volume) -> &mut u8 {
        match v {
            Volume::Master => &mut self.master,
            Volume::Voice => &mut self.voice,
            Volume::Music => &mut self.music,
            Volume::Fx => &mut self.fx,
            Volume::Ambient => &mut self.ambient,
        }
    }

    pub fn volume(&self, v: Volume) -> u8 {
        match v {
            Volume::Master => self.master,
            Volume::Voice => self.voice,
            Volume::Music => self.music,
            Volume::Fx => self.fx,
            Volume::Ambient => self.ambient,
        }
    }

    /// The slider position (percent) of a stored volume.
    pub fn percent(&self, v: Volume) -> u8 {
        (f32::from(self.volume(v)) * 100.0 / 255.0).round() as u8
    }

    /// Move a volume slider to `percent` (clamped to 0..=100).
    pub fn set_percent(&mut self, v: Volume, percent: i32) {
        let p = percent.clamp(0, 100) as f32;
        *self.slot(v) = (p * 255.0 / 100.0).round() as u8;
    }

    /// Linear amplitude 0..=1 of a volume for the audio crate.
    pub fn amplitude(&self, v: Volume) -> f32 {
        f32::from(self.volume(v)) / 255.0
    }

    pub fn speaker_name(&self) -> &'static str {
        SPEAKER_MODES[usize::from(self.speaker.clamp(1, 7)) - 1]
    }

    pub fn step_speaker(&mut self, d: i32) {
        self.speaker = (i32::from(self.speaker) + d).clamp(1, 7) as u8;
    }

    /// Step the text language through [`LANGUAGES`] (an unlisted current one counts as the first).
    pub fn step_text_language(&mut self, d: i32) {
        let i = LANGUAGES
            .iter()
            .position(|l| l.eq_ignore_ascii_case(&self.text_language))
            .unwrap_or(0) as i32;
        let to = (i + d).clamp(0, LANGUAGES.len() as i32 - 1) as usize;
        self.text_language = LANGUAGES[to].to_string();
    }

    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let s = |k: &str, v: String| (k.to_string(), v);
        vec![
            s("MasterVolume", self.master.to_string()),
            s("VoiceVolume", self.voice.to_string()),
            s("MusicVolume", self.music.to_string()),
            s("FXVolume", self.fx.to_string()),
            s("AmbientVolume", self.ambient.to_string()),
            s("Speaker", self.speaker.to_string()),
            s("VoiceLanguage", self.voice_language.clone()),
            s("SubtitleLanguage", self.text_language.clone()),
            s("Subtitle", self.subtitle.to_string()),
        ]
    }

    pub fn write(&self) -> OptionsWrite {
        OptionsWrite {
            section: AUDIO_SECTION,
            pairs: self.to_pairs(),
        }
    }

    /// Read the section; missing or malformed keys keep the default.
    pub fn from_ini(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let vol = |k: &str, def: u8| get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(def);
        let text = |k: &str, def: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| def.to_string())
        };
        Self {
            master: vol("MasterVolume", d.master),
            voice: vol("VoiceVolume", d.voice),
            music: vol("MusicVolume", d.music),
            fx: vol("FXVolume", d.fx),
            ambient: vol("AmbientVolume", d.ambient),
            speaker: vol("Speaker", d.speaker).clamp(1, 7),
            voice_language: text("VoiceLanguage", &d.voice_language),
            text_language: text("SubtitleLanguage", &d.text_language),
            subtitle: get("Subtitle").map_or(d.subtitle, |v| v.trim().eq_ignore_ascii_case("true")),
        }
    }
}

/// UI -> host: the player applied the audio page.
#[derive(Message, Clone, Debug)]
pub struct ApplyAudio(pub AudioOptions);

// ---------------------------------------------------------------------------- difficulty
pub const DIFFICULTY_SECTION: &str = "Options.Difficulty";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    pub fn ini(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        }
    }
}

/// `[Options.Difficulty]`: the difficulty radio buttons and the "Alternative A.I." box.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct DifficultyOptions {
    pub level: Difficulty,
    pub extended_content: bool,
    pub alternative_ai: bool,
}

impl Default for DifficultyOptions {
    fn default() -> Self {
        Self {
            level: Difficulty::Medium,
            extended_content: true,
            alternative_ai: true,
        }
    }
}

impl DifficultyOptions {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("Difficulty".into(), self.level.ini().into()),
            ("ExtendedContent".into(), self.extended_content.to_string()),
            ("AIMode".into(), self.alternative_ai.to_string()),
        ]
    }

    pub fn write(&self) -> OptionsWrite {
        OptionsWrite {
            section: DIFFICULTY_SECTION,
            pairs: self.to_pairs(),
        }
    }

    pub fn from_ini(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let flag =
            |k: &str, def: bool| get(k).map_or(def, |v| v.trim().eq_ignore_ascii_case("true"));
        Self {
            level: get("Difficulty")
                .and_then(|v| {
                    Difficulty::ALL
                        .into_iter()
                        .find(|l| l.ini().eq_ignore_ascii_case(v.trim()))
                })
                .unwrap_or(d.level),
            extended_content: flag("ExtendedContent", d.extended_content),
            alternative_ai: flag("AIMode", d.alternative_ai),
        }
    }
}

/// UI -> host: the player applied the difficulty page.
#[derive(Message, Clone, Debug)]
pub struct ApplyDifficulty(pub DifficultyOptions);

// ----------------------------------------------------------------------------- controls
pub const CONTROLS_SECTION: &str = "Options.Controls";

/// `[Options.Controls]` plus the custom key table.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ControlSettings {
    pub options: ControlOptions,
    /// The `SessionKey.<Name>.*` table (one row per key the INI can name). Used when
    /// `options.custom`; the standard table is used otherwise.
    pub custom: Vec<Binding>,
}

impl Default for ControlSettings {
    fn default() -> Self {
        Self {
            options: ControlOptions::default(),
            custom: standard_rows(),
        }
    }
}

/// The standard table cut to the keys the INI can name, in `SessionKey` order.
pub fn standard_rows() -> Vec<Binding> {
    SessionKey::ALL
        .iter()
        .filter(|k| k.ini_name().is_some())
        .map(|k| {
            DEFAULT_BINDINGS
                .iter()
                .find(|b| b.key == *k)
                .copied()
                .unwrap_or(Binding {
                    key: *k,
                    label: "",
                    physical: [None, None],
                })
        })
        .collect()
}

impl ControlSettings {
    /// Read `[Options.Controls]` (all keys of the section, `SessionKey.*` included).
    pub fn from_ini<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)> + Clone) -> Self {
        let options = ControlOptions::from_pairs(pairs.clone());
        let parsed = gothic_input::keys::parse_custom_bindings(pairs);
        let has_custom = parsed
            .iter()
            .any(|b| b.physical.iter().any(Option::is_some));
        let mut custom = standard_rows();
        if has_custom {
            for row in &mut custom {
                if let Some(p) = parsed.iter().find(|b| b.key == row.key) {
                    row.physical = p.physical;
                }
            }
        }
        Self { options, custom }
    }

    /// The table the session keys use.
    pub fn bindings(&self) -> Vec<Binding> {
        if self.options.custom {
            self.custom.clone()
        } else {
            DEFAULT_BINDINGS.to_vec()
        }
    }

    /// An action mapper with the effective table.
    pub fn mapper(&self) -> ActionMapper {
        ActionMapper::from_bindings(&self.bindings())
    }

    /// Key / value pairs of the section: the options, then (for `Controls=Custom`) the table.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .options
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        if self.options.custom {
            out.extend(gothic_input::custom_bindings_to_ini(&self.custom));
        }
        out
    }

    pub fn write(&self) -> OptionsWrite {
        OptionsWrite {
            section: CONTROLS_SECTION,
            pairs: self.to_pairs(),
        }
    }
}

/// UI -> host: the player applied the controls pages.
#[derive(Message, Clone, Debug)]
pub struct ApplyControls(pub ControlSettings);

/// Option value 0..=1 (sensitivity, smoothing) as slider percent.
pub fn unit_percent(v: f32) -> i32 {
    (v.clamp(0.0, 1.0) * 100.0).round() as i32
}

pub fn from_percent(p: i32) -> f32 {
    p.clamp(0, 100) as f32 / 100.0
}

// --------------------------------------------------------------------------- key mapping
/// A cell that lost its key to a new assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Displaced {
    pub key: SessionKey,
    pub slot: usize,
}

/// Editing state of the custom key list (`gCHUDMenuOptionsControlsCustom2`): the table rows, the
/// selected cell and the "press a key" prompt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyMapEdit {
    pub rows: Vec<Binding>,
    /// Selected row and slot (0 or 1).
    pub selected: Option<(usize, usize)>,
    /// Waiting for the next physical key for the selected cell.
    pub capturing: bool,
    /// Last assignment note (the action that lost its key), shown under the list.
    pub note: Option<String>,
}

impl KeyMapEdit {
    pub fn new(rows: Vec<Binding>) -> Self {
        Self {
            rows,
            ..Self::default()
        }
    }

    pub fn label(row: &Binding) -> String {
        if row.label.is_empty() {
            row.key.ini_name().unwrap_or("?").to_string()
        } else {
            row.label.to_string()
        }
    }

    pub fn select(&mut self, row: usize, slot: usize) {
        if row < self.rows.len() && slot < 2 {
            self.selected = Some((row, slot));
            self.capturing = false;
        }
    }

    /// Start waiting for a key for the selected cell.
    pub fn begin_capture(&mut self) -> bool {
        self.capturing = self.selected.is_some();
        self.capturing
    }

    pub fn cancel_capture(&mut self) {
        self.capturing = false;
    }

    /// Bind `key` to the selected cell. A physical key drives one action only: if another cell
    /// held it, that cell is cleared and reported [I: the original's conflict rule is not in the
    /// decompiled list code; clearing the older binding keeps the table unambiguous].
    pub fn assign(&mut self, key: PhysicalKey) -> Option<Displaced> {
        let (r, s) = self.selected?;
        self.capturing = false;
        let mut displaced = None;
        for (i, row) in self.rows.iter_mut().enumerate() {
            for (j, cell) in row.physical.iter_mut().enumerate() {
                if (i, j) != (r, s) && *cell == Some(key) {
                    *cell = None;
                    displaced = Some(Displaced {
                        key: row.key,
                        slot: j,
                    });
                }
            }
        }
        self.rows[r].physical[s] = Some(key);
        self.note = displaced.as_ref().and_then(|d| {
            self.rows.iter().find(|b| b.key == d.key).map(|b| {
                format!(
                    "{} wurde von {} entfernt",
                    gothic_input::physical_name(key),
                    Self::label(b)
                )
            })
        });
        displaced
    }

    /// Unbind the selected cell.
    pub fn clear_selected(&mut self) {
        if let Some((r, s)) = self.selected {
            self.rows[r].physical[s] = None;
            self.note = None;
        }
    }

    /// Back to the standard table.
    pub fn reset(&mut self) {
        self.rows = standard_rows();
        self.capturing = false;
        self.note = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::read_section;
    use gothic_input::dik;
    use gothic_input::keys::mouse;

    const FILE: &str = "[Options.Audio]\nMasterVolume=255\nMusicVolume=56\nFXVolume=205\nSpeaker=2\nVoiceLanguage=English\nSubtitleLanguage=German\nSubtitle=false\n[Options.Difficulty]\nDifficulty=hard\nExtendedContent=true\nAIMode=false\n";

    #[test]
    fn audio_reads_the_user_file_and_writes_it_back() {
        let a = AudioOptions::from_ini(read_section(FILE, AUDIO_SECTION));
        assert_eq!((a.music, a.fx, a.voice), (56, 205, 255));
        assert_eq!(a.voice_language, "English");
        assert!(!a.subtitle);
        assert_eq!(a.percent(Volume::Music), 22);
        let mut text = format!("[{AUDIO_SECTION}]\n");
        for (k, v) in a.to_pairs() {
            text.push_str(&format!("{k}={v}\n"));
        }
        assert_eq!(
            AudioOptions::from_ini(read_section(&text, AUDIO_SECTION)),
            a
        );
    }

    #[test]
    fn volume_sliders_round_trip_and_clamp() {
        let mut a = AudioOptions::default();
        a.set_percent(Volume::Fx, 50);
        assert_eq!(a.fx, 128);
        a.set_percent(Volume::Fx, 500);
        assert_eq!(a.fx, 255);
        a.set_percent(Volume::Fx, -3);
        assert_eq!(a.fx, 0);
        assert!((AudioOptions::default().amplitude(Volume::Master) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn speaker_and_language_steps_stay_in_range() {
        let mut a = AudioOptions::default();
        assert_eq!(a.speaker_name(), "Stereo");
        a.step_speaker(-5);
        assert_eq!(a.speaker, 1);
        a.step_speaker(50);
        assert_eq!(a.speaker, 7);
        a.step_text_language(1);
        assert_eq!(a.text_language, "English");
        a.step_text_language(1);
        assert_eq!(a.text_language, "English");
    }

    #[test]
    fn difficulty_parses_case_insensitively() {
        let d = DifficultyOptions::from_ini(read_section(FILE, DIFFICULTY_SECTION));
        assert_eq!(d.level, Difficulty::Hard);
        assert!(!d.alternative_ai);
        assert_eq!(
            d.to_pairs()[0],
            ("Difficulty".to_string(), "Hard".to_string())
        );
    }

    #[test]
    fn standard_controls_use_the_default_table_custom_uses_the_rows() {
        let mut c = ControlSettings::default();
        assert_eq!(c.bindings().len(), DEFAULT_BINDINGS.len());
        assert!(
            !c.to_pairs()
                .iter()
                .any(|(k, _)| k.starts_with("SessionKey"))
        );
        c.options.custom = true;
        c.custom[0].physical = [Some(PhysicalKey::key(dik::T)), None];
        assert_eq!(
            c.mapper().physical_keys(c.custom[0].key),
            &[PhysicalKey::key(dik::T)]
        );
        let pairs = c.to_pairs();
        assert!(pairs.contains(&("Controls".to_string(), "Custom".to_string())));
        assert!(pairs.contains(&(
            "SessionKey.Forward.Key1.Offset".to_string(),
            u32::from(dik::T).to_string()
        )));
    }

    #[test]
    fn controls_ini_round_trip_keeps_custom_rows() {
        let mut c = ControlSettings::default();
        c.options.custom = true;
        c.custom[1].physical = [Some(PhysicalKey::mouse(mouse::MIDDLE)), None];
        let pairs = c.to_pairs();
        let back = ControlSettings::from_ini(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        assert_eq!(back.custom, c.custom);
        assert!(back.options.custom);
    }

    #[test]
    fn a_plain_standard_file_leaves_the_custom_table_at_the_standard() {
        let c = ControlSettings::from_ini([("Controls", "Standard"), ("AltCamera", "true")]);
        assert!(c.options.alt_camera && !c.options.custom);
        assert_eq!(c.custom, standard_rows());
    }

    #[test]
    fn assigning_a_taken_key_clears_the_older_binding() {
        let mut m = KeyMapEdit::new(standard_rows());
        let fwd = m
            .rows
            .iter()
            .position(|b| b.key == SessionKey::Forward)
            .unwrap();
        let back = m
            .rows
            .iter()
            .position(|b| b.key == SessionKey::Backward)
            .unwrap();
        m.select(back, 0);
        assert!(m.begin_capture());
        let w = PhysicalKey::key(dik::W);
        let d = m.assign(w).expect("W was Forward's");
        assert_eq!(d.key, SessionKey::Forward);
        assert_eq!(m.rows[fwd].physical[1], None);
        assert_eq!(m.rows[back].physical[0], Some(w));
        assert!(!m.capturing);
        assert!(m.note.as_deref().unwrap().contains("Vorwärts"));
    }

    #[test]
    fn capture_needs_a_selection_and_clear_unbinds() {
        let mut m = KeyMapEdit::new(standard_rows());
        assert!(!m.begin_capture());
        assert!(m.assign(PhysicalKey::key(dik::T)).is_none());
        m.select(0, 1);
        m.clear_selected();
        assert_eq!(m.rows[0].physical[1], None);
        m.reset();
        assert_eq!(m.rows, standard_rows());
    }
}
