//! Logic of the option pages (E32): the page drafts, what each button does and what "apply"
//! hands to the host. The screens (`screens_options.rs`) only read this state; Bevy glue lives
//! in `logic.rs`.
use crate::settings::*;
use gothic_input::PhysicalKey;

/// The mouse axes of the controls page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    fn i(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
        }
    }
}

/// Percent step of the `<` / `>` slider buttons.
pub const SLIDER_STEP: i32 = 5;

/// A button of an option page.
#[derive(Clone, Debug, PartialEq)]
pub enum OptAction {
    AudioVolume(Volume, i32),
    AudioSpeaker(i32),
    AudioLanguage(i32),
    AudioSubtitle,
    AudioApply,
    Difficulty(Difficulty),
    DifficultyAi,
    DifficultyApply,
    Sensitivity(Axis, i32),
    Smoothing(Axis, i32),
    Invert(Axis),
    AltCamera,
    LookaroundInverse,
    /// The Standard / Custom radio buttons.
    CustomKeys(bool),
    ControlsApply,
    KeySelect(usize, usize),
    /// Wait for the next key for the selected cell.
    KeyCapture,
    KeyClear,
    KeyReset,
}

/// What an apply button hands to the host.
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    Audio(AudioOptions),
    Difficulty(DifficultyOptions),
    Controls(ControlSettings),
}

/// Drafts of the option pages; `*_base` is what the host runs with now (the apply button is
/// enabled while they differ).
#[derive(Clone, Debug, Default)]
pub struct OptionsUi {
    pub audio: AudioOptions,
    pub audio_base: AudioOptions,
    pub difficulty: DifficultyOptions,
    pub difficulty_base: DifficultyOptions,
    pub controls: ControlSettings,
    pub controls_base: ControlSettings,
    pub keymap: KeyMapEdit,
}

impl OptionsUi {
    pub fn open_audio(&mut self, now: &AudioOptions) {
        self.audio = now.clone();
        self.audio_base = now.clone();
    }

    pub fn open_difficulty(&mut self, now: &DifficultyOptions) {
        self.difficulty = now.clone();
        self.difficulty_base = now.clone();
    }

    pub fn open_controls(&mut self, now: &ControlSettings) {
        self.controls = now.clone();
        self.controls_base = now.clone();
        self.keymap = KeyMapEdit::default();
    }

    /// Enter the custom key list with the draft's table.
    pub fn open_keymap(&mut self) {
        self.keymap = KeyMapEdit::new(self.controls.custom.clone());
    }

    pub fn audio_changed(&self) -> bool {
        self.audio != self.audio_base
    }

    pub fn difficulty_changed(&self) -> bool {
        self.difficulty != self.difficulty_base
    }

    pub fn controls_changed(&self) -> bool {
        self.controls != self.controls_base || self.keymap_dirty()
    }

    fn keymap_dirty(&self) -> bool {
        !self.keymap.rows.is_empty() && self.keymap.rows != self.controls.custom
    }

    /// Take the key list edits into the controls draft.
    pub fn commit_keymap(&mut self) {
        if !self.keymap.rows.is_empty() {
            self.controls.custom = self.keymap.rows.clone();
        }
    }

    /// Press a button. Returns the settings an apply button hands to the host.
    pub fn act(&mut self, a: OptAction) -> Option<Applied> {
        match a {
            OptAction::AudioVolume(v, steps) => {
                let p = i32::from(self.audio.percent(v)) + steps * SLIDER_STEP;
                self.audio.set_percent(v, p);
            }
            OptAction::AudioSpeaker(d) => self.audio.step_speaker(d),
            OptAction::AudioLanguage(d) => self.audio.step_text_language(d),
            OptAction::AudioSubtitle => self.audio.subtitle = !self.audio.subtitle,
            OptAction::AudioApply => {
                self.audio_base = self.audio.clone();
                return Some(Applied::Audio(self.audio.clone()));
            }
            OptAction::Difficulty(l) => self.difficulty.level = l,
            OptAction::DifficultyAi => self.difficulty.alternative_ai ^= true,
            OptAction::DifficultyApply => {
                self.difficulty_base = self.difficulty.clone();
                return Some(Applied::Difficulty(self.difficulty.clone()));
            }
            OptAction::Sensitivity(axis, steps) => {
                let o = &mut self.controls.options.sensitivity[axis.i()];
                *o = from_percent(unit_percent(*o) + steps * SLIDER_STEP);
            }
            OptAction::Smoothing(axis, steps) => {
                let o = &mut self.controls.options.smoothing[axis.i()];
                *o = from_percent(unit_percent(*o) + steps * SLIDER_STEP);
            }
            OptAction::Invert(axis) => self.controls.options.invert[axis.i()] ^= true,
            OptAction::AltCamera => self.controls.options.alt_camera ^= true,
            OptAction::LookaroundInverse => self.controls.options.lookaround_inverse ^= true,
            OptAction::CustomKeys(on) => self.controls.options.custom = on,
            OptAction::ControlsApply => {
                self.commit_keymap();
                self.controls_base = self.controls.clone();
                return Some(Applied::Controls(self.controls.clone()));
            }
            OptAction::KeySelect(r, s) => {
                self.keymap.select(r, s);
                self.keymap.begin_capture();
            }
            OptAction::KeyCapture => {
                self.keymap.begin_capture();
            }
            OptAction::KeyClear => self.keymap.clear_selected(),
            OptAction::KeyReset => self.keymap.reset(),
        }
        None
    }

    /// A key arrived while the "press a key" prompt is up.
    pub fn captured(&mut self, key: PhysicalKey) {
        self.keymap.assign(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_input::{SessionKey, dik};

    #[test]
    fn audio_buttons_step_and_apply_once() {
        let mut o = OptionsUi::default();
        assert!(!o.audio_changed());
        o.act(OptAction::AudioVolume(Volume::Music, -4));
        assert_eq!(o.audio.percent(Volume::Music), 80);
        o.act(OptAction::AudioSubtitle);
        assert!(o.audio_changed());
        let a = o.act(OptAction::AudioApply).expect("apply hands over");
        assert_eq!(a, Applied::Audio(o.audio.clone()));
        assert!(!o.audio_changed());
    }

    #[test]
    fn mouse_sliders_clamp_at_the_ends() {
        let mut o = OptionsUi::default();
        for _ in 0..30 {
            o.act(OptAction::Sensitivity(Axis::X, 1));
            o.act(OptAction::Smoothing(Axis::Y, -1));
        }
        assert_eq!(o.controls.options.sensitivity[0], 1.0);
        assert_eq!(o.controls.options.smoothing[1], 0.0);
        assert_eq!(
            o.controls.options.sensitivity[1], 0.5,
            "the other axis is untouched"
        );
    }

    #[test]
    fn rebinding_flows_from_capture_to_the_applied_table() {
        let mut o = OptionsUi::default();
        o.act(OptAction::CustomKeys(true));
        o.open_keymap();
        let fwd = o
            .keymap
            .rows
            .iter()
            .position(|b| b.key == SessionKey::Forward)
            .unwrap();
        o.act(OptAction::KeySelect(fwd, 0));
        o.act(OptAction::KeyCapture);
        assert!(o.keymap.capturing);
        o.captured(PhysicalKey::key(dik::T));
        assert!(o.controls_changed(), "the key list differs from the draft");
        let Some(Applied::Controls(c)) = o.act(OptAction::ControlsApply) else {
            panic!("apply hands the controls over");
        };
        assert!(c.options.custom);
        assert_eq!(c.custom[fwd].physical[0], Some(PhysicalKey::key(dik::T)));
        assert!(!o.controls_changed());
        let pairs = c.to_pairs();
        assert!(pairs.contains(&(
            "SessionKey.Forward.Key1.Offset".into(),
            u32::from(dik::T).to_string()
        )));
    }

    #[test]
    fn difficulty_radio_selects_one_level() {
        let mut o = OptionsUi::default();
        o.act(OptAction::Difficulty(Difficulty::Hard));
        assert!(o.difficulty_changed());
        o.act(OptAction::Difficulty(Difficulty::Medium));
        assert!(!o.difficulty_changed());
    }
}
