//! Option pages of the menu (E32): the hub (`IDD_MENU_OPTIONS_2`), audio, controls, custom key
//! list and difficulty, laid out at the control rectangles of `g3.gui.res` (1024x768 design
//! space; the resource stores left / top / right / bottom, the helpers here take x, y, w, h).
use crate::atlas::*;
use crate::optpages::{Axis, OptAction};
use crate::screens::{Box4, Cx, body, heading};
use crate::screens_menu::menu_btn;
use crate::settings::*;
use crate::style::*;
use crate::*;
use gothic_input::physical_name;

fn opt(a: OptAction) -> Option<Action> {
    Some(Action::Opt(a))
}

/// Column layout of a slider row (label, slider track, value caption).
#[derive(Clone, Copy)]
struct Cols {
    label: (f32, f32),
    track_x: f32,
    caption: (f32, f32),
}

const AUDIO_COLS: Cols = Cols {
    label: (24.0, 366.0),
    track_x: 416.0,
    caption: (640.0, 304.0),
};
const CONTROL_COLS: Cols = Cols {
    label: (112.0, 282.0),
    track_x: 420.0,
    caption: (644.0, 304.0),
};
const TRACK_W: f32 = 200.0;

/// One slider row: label, `<` track `>` and the value caption. `pos` is 0..=1.
#[allow(clippy::too_many_arguments)]
fn slider_row(
    p: &mut P,
    cx: &Cx,
    c: Cols,
    y: f32,
    text: &str,
    pos: f32,
    caption: String,
    left: Option<OptAction>,
    right: Option<OptAction>,
) {
    let pal = cx.a.palette;
    body(
        p,
        cx,
        text,
        (c.label.0, y, c.label.1, 25.0),
        Al::Left,
        pal.text,
    );
    menu_btn(
        p,
        cx,
        "<",
        (c.track_x - 26.0, y, 22.0, 25.0),
        left.and_then(opt),
    );
    let mut track = abs(c.track_x, y + 8.0, TRACK_W, 9.0);
    track.border = UiRect::all(px(1));
    p.spawn((
        track,
        BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.9)),
        BorderColor::all(BORDER),
    ));
    let x = c.track_x + pos.clamp(0.0, 1.0) * TRACK_W;
    p.spawn((
        abs(x - 4.0, y + 2.0, 8.0, 21.0),
        BackgroundColor(pal.text_hover),
    ));
    menu_btn(
        p,
        cx,
        ">",
        (c.track_x + TRACK_W + 4.0, y, 22.0, 25.0),
        right.and_then(opt),
    );
    body(
        p,
        cx,
        caption,
        (c.caption.0, y, c.caption.1, 25.0),
        Al::Left,
        pal.text,
    );
}

/// An on / off row drawn as a two-position slider.
fn toggle_row(p: &mut P, cx: &Cx, c: Cols, y: f32, text: &str, on: bool, toggle: OptAction) {
    slider_row(
        p,
        cx,
        c,
        y,
        text,
        if on { 1.0 } else { 0.0 },
        if on { "An" } else { "Aus" }.into(),
        on.then(|| toggle.clone()),
        (!on).then_some(toggle),
    );
}

/// A radio button (round) or check box (square) with its text.
fn choice(p: &mut P, cx: &Cx, r: Box4, text: &str, on: bool, round: bool, action: Action) {
    let pal = cx.a.palette;
    let mut hit = abs(r.0, r.1, r.2, r.3);
    hit.align_items = AlignItems::Center;
    hit.column_gap = px(10);
    p.spawn((button(action, Color::NONE, SEL), hit))
        .with_children(|b| {
            let mut mark = Node {
                width: px(18),
                height: px(18),
                border: UiRect::all(px(2)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            };
            if round {
                mark.border_radius = BorderRadius::MAX;
            }
            b.spawn((
                mark,
                BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.9)),
                BorderColor::all(BORDER),
            ))
            .with_children(|m| {
                if on {
                    let mut dot = Node {
                        width: px(8),
                        height: px(8),
                        ..default()
                    };
                    if round {
                        dot.border_radius = BorderRadius::MAX;
                    }
                    m.spawn((dot, BackgroundColor(pal.text_hover)));
                }
            });
            b.spawn(txt(
                cx.font(),
                text,
                16.0,
                if on { pal.text_hover } else { pal.text },
            ));
        });
}

fn apply_button(p: &mut P, cx: &Cx, enabled: bool, action: OptAction, x: f32) {
    menu_btn(
        p,
        cx,
        "Übernehmen",
        (x, 682.0, 120.0, 32.0),
        enabled.then_some(Action::Opt(action)),
    );
}

fn back_button(p: &mut P, cx: &Cx) {
    menu_btn(
        p,
        cx,
        "Zurück",
        (462.0, 682.0, 100.0, 32.0),
        Some(Action::MenuBack),
    );
}

// ------------------------------------------------------------------------------------ hub
pub(crate) fn hub_page(p: &mut P, cx: &Cx) {
    spawn_pic(p, cx.a, MENU_OPTIONS_BG, 0.0, 0.0, None);
    heading(p, cx, "Einstellungen", (271.0, 30.0, 482.0, 40.0));
    // top to bottom as in the resource: performance, video, audio, controls, difficulty
    let rows: [(&str, f32, Option<Action>); 5] = [
        ("Leistung", 444.0, None),
        ("Video", 481.0, Some(Action::Open(Screen::Video))),
        ("Audio", 518.0, Some(Action::Open(Screen::Audio))),
        ("Steuerung", 555.0, Some(Action::Open(Screen::Controls))),
        (
            "Schwierigkeit",
            592.0,
            Some(Action::Open(Screen::Difficulty)),
        ),
    ];
    for (text, y, a) in rows {
        menu_btn(p, cx, text, (386.0, y, 252.0, 32.0), a);
    }
    back_button(p, cx);
}

// ---------------------------------------------------------------------------------- audio
pub(crate) fn audio_page(p: &mut P, cx: &Cx) {
    let o = &cx.ui.opts;
    let a = &o.audio;
    spawn_pic(p, cx.a, MENU_AUDIO_BG, 0.0, 0.0, None);
    heading(p, cx, "Audiooptionen", (271.0, 30.0, 482.0, 40.0));
    let vol = |p: &mut P, y: f32, text: &str, v: Volume| {
        slider_row(
            p,
            cx,
            AUDIO_COLS,
            y,
            text,
            f32::from(a.percent(v)) / 100.0,
            format!("{}%", a.percent(v)),
            Some(OptAction::AudioVolume(v, -1)),
            Some(OptAction::AudioVolume(v, 1)),
        );
    };
    vol(p, 396.0, "Gesamtlautstärke", Volume::Master);
    vol(p, 424.0, "Sprachlautstärke", Volume::Voice);
    vol(p, 452.0, "Musiklautstärke", Volume::Music);
    vol(p, 480.0, "Effektlautstärke", Volume::Fx);
    vol(p, 508.0, "Umgebungslautstärke", Volume::Ambient);
    let lang = LANGUAGES
        .iter()
        .position(|l| l.eq_ignore_ascii_case(&a.text_language))
        .unwrap_or(0);
    slider_row(
        p,
        cx,
        AUDIO_COLS,
        564.0,
        "Textsprache",
        lang as f32 / (LANGUAGES.len() - 1) as f32,
        match a.text_language.as_str() {
            "German" => "Deutsch".to_string(),
            "English" => "Englisch".to_string(),
            other => other.to_string(),
        },
        Some(OptAction::AudioLanguage(-1)),
        Some(OptAction::AudioLanguage(1)),
    );
    toggle_row(
        p,
        cx,
        AUDIO_COLS,
        592.0,
        "Untertitel",
        a.subtitle,
        OptAction::AudioSubtitle,
    );
    slider_row(
        p,
        cx,
        AUDIO_COLS,
        620.0,
        &cx.t("HUD_Audio_Speaker_Setup", "Lautsprecher"),
        f32::from(a.speaker - 1) / 6.0,
        a.speaker_name().into(),
        Some(OptAction::AudioSpeaker(-1)),
        Some(OptAction::AudioSpeaker(1)),
    );
    body(
        p,
        cx,
        cx.t(
            "HUD_Audio_ResetWarning",
            "Die Lautsprecher-Einstellung gilt nach dem Übernehmen sofort.",
        ),
        (24.0, 650.0, 976.0, 26.0),
        Al::Center,
        cx.a.palette.text_disabled,
    );
    apply_button(p, cx, o.audio_changed(), OptAction::AudioApply, 332.0);
    back_button(p, cx);
}

// ------------------------------------------------------------------------------- controls
pub(crate) fn controls_page(p: &mut P, cx: &Cx) {
    let o = &cx.ui.opts;
    let c = &o.controls.options;
    spawn_pic(p, cx.a, MENU_OPTIONS_BG, 0.0, 0.0, None);
    heading(p, cx, "Steuerung", (271.0, 30.0, 482.0, 40.0));
    heading(p, cx, "Maus", (362.0, 141.0, 300.0, 25.0));
    let pct = |p: &mut P, y: f32, text: &str, v: f32, l: OptAction, r: OptAction| {
        slider_row(
            p,
            cx,
            CONTROL_COLS,
            y,
            text,
            v,
            format!("{}%", unit_percent(v)),
            Some(l),
            Some(r),
        );
    };
    for (axis, name, y) in [(Axis::X, "X-Achse", 184.0), (Axis::Y, "Y-Achse", 212.0)] {
        let i = usize::from(axis == Axis::Y);
        pct(
            p,
            y,
            &format!("Empfindlichkeit {name}"),
            c.sensitivity[i],
            OptAction::Sensitivity(axis, -1),
            OptAction::Sensitivity(axis, 1),
        );
    }
    for (axis, name, y) in [(Axis::X, "X-Achse", 256.0), (Axis::Y, "Y-Achse", 284.0)] {
        let i = usize::from(axis == Axis::Y);
        toggle_row(
            p,
            cx,
            CONTROL_COLS,
            y,
            &format!("{name} umkehren"),
            c.invert[i],
            OptAction::Invert(axis),
        );
    }
    for (axis, name, y) in [(Axis::X, "X-Achse", 324.0), (Axis::Y, "Y-Achse", 352.0)] {
        let i = usize::from(axis == Axis::Y);
        pct(
            p,
            y,
            &format!("Glättung {name}"),
            c.smoothing[i],
            OptAction::Smoothing(axis, -1),
            OptAction::Smoothing(axis, 1),
        );
    }
    toggle_row(
        p,
        cx,
        CONTROL_COLS,
        396.0,
        "Alternative Kamera",
        c.alt_camera,
        OptAction::AltCamera,
    );
    toggle_row(
        p,
        cx,
        CONTROL_COLS,
        424.0,
        "Kamerablick umkehren",
        c.lookaround_inverse,
        OptAction::LookaroundInverse,
    );
    heading(p, cx, "Tastatur", (356.0, 472.0, 312.0, 25.0));
    choice(
        p,
        cx,
        (408.0, 532.0, 204.0, 25.0),
        "Standard",
        !c.custom,
        true,
        Action::Opt(OptAction::CustomKeys(false)),
    );
    choice(
        p,
        cx,
        (408.0, 568.0, 204.0, 25.0),
        "Eigene Belegung",
        c.custom,
        true,
        Action::Open(Screen::ControlsCustom),
    );
    apply_button(p, cx, o.controls_changed(), OptAction::ControlsApply, 332.0);
    back_button(p, cx);
}

// -------------------------------------------------------------------------- custom keys
const KEY_ROW_H: f32 = 28.0;

/// The caption of one key cell.
fn cell_text(k: Option<gothic_input::PhysicalKey>) -> String {
    k.map_or_else(|| "-".to_string(), physical_name)
}

pub(crate) fn controls_custom_page(p: &mut P, cx: &Cx) {
    let o = &cx.ui.opts;
    let km = &o.keymap;
    let pal = cx.a.palette;
    spawn_pic(p, cx.a, MENU_CUSTOM_BG, 0.0, 0.0, None);
    heading(p, cx, "Tastenbelegung", (172.0, 96.0, 676.0, 25.0));
    // column heads
    for (text, x, w) in [
        ("Aktion", 184.0, 260.0),
        ("Taste 1", 460.0, 190.0),
        ("Taste 2", 660.0, 190.0),
    ] {
        body(p, cx, text, (x, 132.0, w, 22.0), Al::Left, TEXT_DIM);
    }
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..abs(172.0, 156.0, 680.0, 476.0)
        },
        scroll_area("keymap", cx.scroll_y("keymap")),
    ))
    .with_children(|l| {
        for (i, row) in km.rows.iter().enumerate() {
            l.spawn(Node {
                height: px(KEY_ROW_H),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn(Node {
                    width: px(280),
                    padding: UiRect::left(px(12)),
                    ..default()
                })
                .with_child(txt(
                    cx.font(),
                    KeyMapEdit::label(row),
                    16.0,
                    pal.text,
                ));
                for slot in 0..2 {
                    let sel = km.selected == Some((i, slot));
                    let waiting = sel && km.capturing;
                    let text = if waiting {
                        "Taste drücken ...".to_string()
                    } else {
                        cell_text(row.physical[slot])
                    };
                    r.spawn((
                        button(
                            Action::Opt(OptAction::KeySelect(i, slot)),
                            if sel { SEL } else { Color::NONE },
                            SEL,
                        ),
                        Node {
                            width: px(190),
                            height: px(KEY_ROW_H - 4.0),
                            margin: UiRect::right(px(10)),
                            padding: UiRect::horizontal(px(8)),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BorderColor::all(if sel { BORDER } else { Color::NONE }),
                    ))
                    .with_child(txt(
                        cx.font(),
                        text,
                        15.0,
                        if waiting {
                            pal.text_hover
                        } else if row.physical[slot].is_some() {
                            pal.text
                        } else {
                            TEXT_DIM
                        },
                    ));
                }
            });
        }
    });
    let hint = if km.capturing {
        "Taste oder Maustaste drücken - Esc bricht ab.".to_string()
    } else if let Some(n) = &km.note {
        n.clone()
    } else {
        "Eine Zelle anklicken, dann die neue Taste drücken.".to_string()
    };
    body(
        p,
        cx,
        hint,
        (172.0, 638.0, 680.0, 30.0),
        Al::Center,
        pal.text_disabled,
    );
    menu_btn(
        p,
        cx,
        "Löschen",
        (172.0, 682.0, 130.0, 32.0),
        km.selected.map(|_| Action::Opt(OptAction::KeyClear)),
    );
    menu_btn(
        p,
        cx,
        "Standard",
        (312.0, 682.0, 140.0, 32.0),
        opt(OptAction::KeyReset),
    );
    apply_button(p, cx, o.controls_changed(), OptAction::ControlsApply, 572.0);
    back_button(p, cx);
}

// ----------------------------------------------------------------------------- difficulty
pub(crate) fn difficulty_page(p: &mut P, cx: &Cx) {
    let o = &cx.ui.opts;
    spawn_pic(p, cx.a, MENU_DIFFICULTY_BG, 0.0, 0.0, None);
    heading(p, cx, "Schwierigkeitsgrad", (271.0, 30.0, 482.0, 40.0));
    for (level, text, y) in [
        (Difficulty::Easy, "Leicht", 459.0),
        (Difficulty::Medium, "Mittel", 497.0),
        (Difficulty::Hard, "Schwer", 535.0),
    ] {
        choice(
            p,
            cx,
            (397.0, y, 230.0, 25.0),
            text,
            o.difficulty.level == level,
            true,
            Action::Opt(OptAction::Difficulty(level)),
        );
    }
    choice(
        p,
        cx,
        (397.0, 580.0, 230.0, 25.0),
        "Alternative K.I.",
        o.difficulty.alternative_ai,
        false,
        Action::Opt(OptAction::DifficultyAi),
    );
    apply_button(
        p,
        cx,
        o.difficulty_changed(),
        OptAction::DifficultyApply,
        332.0,
    );
    back_button(p, cx);
}
