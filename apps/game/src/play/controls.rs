//! Input of the play mode: the original session keys (`gothic-input`, engine port E50).
//!
//! The shipped player keys are compiled into `Game.dll` (`Controls=Standard`, the 45-entry table
//! of `gothic_input::keys::DEFAULT_BINDINGS`): W A S D move relative to the camera, **Q / E rotate
//! the camera**, Shift is *walk while held*, Ctrl ducks, Alt jumps, Space and the middle button
//! draw / put away the weapon, 1..0 are the quick slots, R / End lock the target, T first person,
//! the left button is the **use key** outside combat modes and the primary action (attack, aim,
//! cast) in them, the right button the secondary one (guard, abort). There is no double-tap
//! dodge in the player layer.
//!
//! This module owns the per-frame pipeline up to the messages: Bevy device state -> `RawInput`
//! -> `ActionMapper` -> `GameKeyMessage`s (with press edge and duration), the movement keys of
//! `CharacterControl`, the camera look / zoom counts and the options of
//! `UserOptions.ini [Options.Controls]`. `hero.rs` routes the messages into player actions
//! (`PlayerInput::route`); `cam.rs` feeds the look counts to the camera. The few UI-level keys
//! (window digits, Enter inside windows, F5..F10) stay on Bevy key codes.
use super::interact::Interaction;
use super::{Mode, PlayState, ScriptInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_input::control::{look_counts, zoom_notches};
use gothic_input::{
    ActionMapper, CharacterControl, ControlOptions, GameKeyMessage, Look, PlayerInput, RawInput,
    SessionKey, dik,
};

/// Keys of the UI-level shortcuts that are not session keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Inventory,
    QuickSave,
    QuickLoad,
    Save,
    Load,
    Confirm,
}

fn keys_of(a: Act) -> &'static [KeyCode] {
    match a {
        Act::Inventory => &[KeyCode::KeyI],
        Act::QuickSave => &[KeyCode::F5],
        Act::QuickLoad => &[KeyCode::F9],
        Act::Save => &[KeyCode::F6],
        Act::Load => &[KeyCode::F10],
        Act::Confirm => &[KeyCode::Enter, KeyCode::NumpadEnter],
    }
}

pub fn just_pressed(keys: &ButtonInput<KeyCode>, a: Act) -> bool {
    keys.any_just_pressed(keys_of(a).iter().copied())
}

/// The quick slot (0-based) whose number key went down this frame (`1` = slot 0, `0` = slot 9);
/// only used for the digit shortcuts of open windows.
pub fn quick_slot_pressed(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    DIGITS.iter().position(|k| keys.just_pressed(*k))
}

/// The state of the session keys of one frame.
#[derive(Resource)]
pub struct InputFrame {
    mapper: ActionMapper,
    /// `PSCharacterControl`: the four movement keys, jump, duck.
    pub control: CharacterControl,
    /// The static latches of the script's key router.
    pub router: PlayerInput,
    /// This frame's messages (one per logical key that is on or was on the frame before).
    pub msgs: Vec<GameKeyMessage>,
    /// Camera look in mouse counts (x right, y down), inverted per option.
    pub look: Look,
    /// Wheel notches (positive zooms in).
    pub zoom: f32,
    pub options: ControlOptions,
    /// The look-back key is held.
    pub look_back: bool,
    /// The first-person key went down this frame.
    pub first_person: bool,
    /// The camera reset key went down this frame.
    pub reset_camera: bool,
    /// Walk gait: the walk key held, or the caps-lock style toggle.
    pub walk: bool,
    walk_toggled: bool,
    /// Keys are not delivered (a page, a dialog, a window or the death screen is up).
    pub blocked: bool,
    /// The mouse is captured by the game.
    pub grabbed: bool,
    /// The smoke script holds the left / right mouse button until this time.
    pub script_left_until: Option<f32>,
    pub script_right: bool,
    pub paused: bool,
    app_ms: u64,
    /// Bumped when the options changed (the camera re-reads sensitivity).
    pub rev: u32,
}

impl Default for InputFrame {
    fn default() -> Self {
        Self::new(ControlOptions::default(), ActionMapper::standard())
    }
}

impl InputFrame {
    pub fn new(options: ControlOptions, mapper: ActionMapper) -> Self {
        Self {
            mapper,
            control: CharacterControl::default(),
            router: PlayerInput::default(),
            msgs: Vec::new(),
            look: Look::default(),
            zoom: 0.0,
            options,
            look_back: false,
            first_person: false,
            reset_camera: false,
            walk: false,
            walk_toggled: false,
            blocked: false,
            grabbed: false,
            script_left_until: None,
            script_right: false,
            paused: false,
            app_ms: 0,
            rev: 0,
        }
    }

    /// The message of a logical key this frame, if it is on or went off.
    pub fn msg(&self, key: SessionKey) -> Option<&GameKeyMessage> {
        self.msgs.iter().find(|m| m.key == key)
    }

    /// The logical key is down.
    pub fn down(&self, key: SessionKey) -> bool {
        self.msg(key).is_some_and(GameKeyMessage::is_pressed)
    }

    /// The logical key went down this frame.
    pub fn pressed_edge(&self, key: SessionKey) -> bool {
        self.msg(key).is_some_and(GameKeyMessage::is_press_edge)
    }

    /// The logical key went up this frame.
    pub fn released_edge(&self, key: SessionKey) -> bool {
        self.msg(key).is_some_and(GameKeyMessage::is_release_edge)
    }

    /// The movement keys in the camera's ground frame: `(right, forward)`, each -1, 0 or 1.
    pub fn move_axes(&self) -> (f32, f32) {
        self.control.move_keys().axes()
    }

    /// Toggle the caps-lock style walk gait (`WalkToggle`).
    fn update_walk(&mut self) {
        if self.pressed_edge(SessionKey::WalkToggle) {
            self.walk_toggled = !self.walk_toggled;
        }
        self.walk = self.down(SessionKey::Walk) || self.walk_toggled;
    }
}

/// The user's options file: `Documents\gothic3\UserOptions.ini` (the file the original reads and
/// writes), `GR_USER_OPTIONS` overrides it.
pub fn user_options_path() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("GR_USER_OPTIONS") {
        return Some(p.into());
    }
    let home = std::env::var_os("USERPROFILE")?;
    Some(
        std::path::PathBuf::from(home)
            .join("Documents")
            .join("gothic3")
            .join("UserOptions.ini"),
    )
}

pub fn register(app: &mut App) {
    let text = user_options_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let section = gothic_ui::userini::section_pairs(&text, "Options.Controls");
    let settings =
        gothic_ui::ControlSettings::from_ini(section.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let frame = InputFrame::new(settings.options, settings.mapper());
    app.insert_resource(settings)
        .insert_resource(frame)
        .add_systems(
            Update,
            gather_input
                .before(super::hero::input)
                .before(super::cam::play_camera)
                .after(crate::walk::input_mode),
        );
}

impl InputFrame {
    /// Take new settings (the controls pages applied): the table and the mouse options.
    pub fn apply(&mut self, s: &gothic_ui::ControlSettings) {
        self.mapper = s.mapper();
        self.options = s.options;
        self.rev += 1;
    }
}

/// Bevy device state -> messages. Runs before the hero's input and the camera.
#[allow(clippy::too_many_arguments)]
fn gather_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    ui: Res<gothic_ui::UiState>,
    state: Res<PlayState>,
    inter: Res<Interaction>,
    mut script: ResMut<ScriptInput>,
    time: Res<Time>,
    mut f: ResMut<InputFrame>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    f.app_ms = time.elapsed().as_millis() as u64;
    let grabbed = cursor
        .single()
        .is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    // A click that only captures the cursor is not a game click: the mouse counts from the
    // frame after the capture.
    let mouse_live = grabbed && f.grabbed;
    f.grabbed = grabbed;
    f.blocked =
        ui.is_open() || state.mode == Mode::Dialog || state.hero_down || inter.window.is_some();
    // The legacy smoke flag is a 0.1 s click of the left button.
    if std::mem::take(&mut script.attack) {
        f.script_left_until = Some(now + 0.1);
    }
    for a in script.acts.iter() {
        match a {
            super::ScriptAct::LeftClick(secs) => f.script_left_until = Some(now + *secs),
            super::ScriptAct::Parry(d) => f.script_right = *d,
            _ => {}
        }
    }
    let scripted_left = f.script_left_until.is_some_and(|t| now < t);
    if !scripted_left {
        f.script_left_until = None;
    }

    let mut raw = RawInput::default();
    if !f.blocked {
        for k in keyboard.get_pressed() {
            if let Some(code) = dik::from_key_code_name(&format!("{k:?}")) {
                raw.keys[code as usize] = true;
            }
        }
        if mouse_live {
            raw.buttons[0] = buttons.pressed(MouseButton::Left);
            raw.buttons[1] = buttons.pressed(MouseButton::Right);
            raw.buttons[2] = buttons.pressed(MouseButton::Middle);
            raw.buttons[3] =
                buttons.pressed(MouseButton::Back) || buttons.pressed(MouseButton::Forward);
            raw.delta = [
                motion.delta.x.round() as i32,
                motion.delta.y.round() as i32,
                scroll.delta.y.round() as i32,
            ];
        }
        raw.buttons[0] |= scripted_left;
        raw.buttons[1] |= f.script_right;
    }
    let ms = f.app_ms;
    let msgs = f.mapper.process(&raw, ms);
    if std::env::var_os("GR_INPUT_DEBUG").is_some()
        && (raw.delta != [0, 0, 0]
            || msgs
                .iter()
                .any(|m| m.is_press_edge() || m.is_release_edge()))
    {
        eprintln!(
            "input: t={now:.2} grabbed={grabbed} blocked={} delta={:?} keys={:?}",
            f.blocked,
            raw.delta,
            msgs.iter()
                .map(|m| format!(
                    "{:?}:{}{}",
                    m.key,
                    u8::from(m.is_pressed()),
                    u8::from(m.was_pressed())
                ))
                .collect::<Vec<_>>()
        );
    }
    let paused = f.blocked;
    for m in &msgs {
        f.control.on_action(m, !paused);
    }
    if paused {
        f.control.release_all();
    }
    f.paused = paused;
    f.look = if f.blocked {
        Look::default()
    } else {
        look_counts(&msgs, dt, f.options.invert[0], f.options.invert[1])
    };
    f.zoom = if f.blocked { 0.0 } else { zoom_notches(&msgs) };
    f.msgs = msgs;
    f.look_back = f.down(SessionKey::LookBack);
    f.first_person = f.pressed_edge(SessionKey::FirstPerson);
    f.reset_camera = f.pressed_edge(SessionKey::ResetCamera);
    f.update_walk();
}
