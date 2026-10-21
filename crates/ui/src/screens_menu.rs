//! Menu pages: the save / load list (`IDD_MENU_FILES_2`) and the video options
//! (`IDD_MENU_OPTIONS_VIDEO_2`), laid out at the control rectangles of `g3.gui.res` in the
//! 1024x768 design space and coloured with the compiled theme.
use crate::atlas::*;
use crate::files::{self, Buttons};
use crate::options::Slider;
use crate::screens::{Box4, Cx, body, heading};
use crate::style::*;
use crate::*;

/// Themed text button of the menu pages; `action: None` is disabled.
pub(crate) fn menu_btn(p: &mut P, cx: &Cx, text: &str, r: Box4, action: Option<Action>) {
    let pal = cx.a.palette;
    let mut n = abs(r.0, r.1, r.2, r.3);
    n.border = UiRect::all(px(2));
    n.align_items = AlignItems::Center;
    n.justify_content = JustifyContent::Center;
    match action {
        Some(a) => {
            p.spawn((
                button(a, pal.button, pal.button_hover),
                n,
                BorderColor::all(BORDER),
                children![txt(cx.font(), text, 18.0, pal.text)],
            ));
        }
        None => {
            p.spawn((
                n,
                BackgroundColor(pal.button_disabled),
                BorderColor::all(Color::srgba(0.4, 0.34, 0.25, 0.5)),
                children![txt(cx.font(), text, 18.0, pal.text_disabled)],
            ));
        }
    }
}

// ---------------------------------------------------------------------------------- files
const ROW_H: f32 = 48.0;

pub(crate) fn files_page(p: &mut P, cx: &Cx) {
    let f = &cx.ui.files;
    spawn_pic(p, cx.a, MENU_FILES_BG, 0.0, 0.0, None);
    let title = if f.save_mode {
        "Spiel speichern"
    } else {
        "Spiel laden"
    };
    heading(p, cx, title, (271.0, 30.0, 482.0, 40.0));

    let selected = f.selected.and_then(|i| f.rows.get(i));
    let Buttons { load, save, delete } = files::buttons(selected, f.can_save);
    let main = if f.save_mode {
        ("Speichern", save.then_some(Action::FileDo))
    } else {
        ("Laden", load.then_some(Action::FileDo))
    };
    menu_btn(p, cx, main.0, (93.0, 271.0, 172.0, 32.0), main.1);
    menu_btn(
        p,
        cx,
        "Löschen",
        (93.0, 319.0, 172.0, 32.0),
        delete.then_some(Action::FileDelete),
    );
    menu_btn(
        p,
        cx,
        "Zurück",
        (462.0, 682.0, 100.0, 32.0),
        Some(Action::MenuBack),
    );

    let quick = cx.t("HUD_QuickSave", "Schnellspeicherung");
    let clock = cx.clock_format();
    let list = (337.0, 89.0, 552.0, 552.0);
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..abs(list.0, list.1, list.2, list.3)
        },
        scroll_area("files", cx.scroll_y("files")),
    ))
    .with_children(|l| {
        let row = |l: &mut P, sel: bool, action: Action, lines: [(String, Color); 2]| {
            let [(a, ca), (b, cb)] = lines;
            l.spawn((
                button(action, if sel { SEL } else { Color::NONE }, SEL),
                Node {
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                    height: px(ROW_H),
                    padding: UiRect::horizontal(px(12)),
                    ..default()
                },
            ))
            .with_children(|r| {
                r.spawn(txt(cx.font(), a, 18.0, ca));
                r.spawn(txt(cx.font(), b, 14.0, cb));
            });
        };
        if f.save_mode {
            row(
                l,
                f.selected.is_none(),
                Action::FileSelect(None),
                [
                    ("Neuer Spielstand".into(), cx.a.palette.text_hover),
                    (String::new(), TEXT_DIM),
                ],
            );
        }
        for (i, r) in f.rows.iter().enumerate() {
            let (name, when) = files::caption(r, &quick, &clock);
            let detail = format!(
                "{when}   Stufe {}   Spielzeit {}",
                r.level,
                files::play_time(r.play_ms)
            );
            let sel = f.selected == Some(i);
            row(
                l,
                sel,
                Action::FileSelect(Some(i)),
                [
                    (
                        name,
                        if sel {
                            cx.a.palette.text_hover
                        } else {
                            cx.a.palette.text
                        },
                    ),
                    (detail, TEXT_DIM),
                ],
            );
        }
        if f.rows.is_empty() && !f.save_mode {
            l.spawn(txt(
                cx.font(),
                "Keine Spielstände vorhanden.",
                16.0,
                TEXT_DIM,
            ));
        }
    });
}

// ---------------------------------------------------------------------------------- video
fn slider_row(p: &mut P, cx: &Cx, label_text: &str, y: f32, slider: Slider) {
    let v = cx.ui.video.slider(slider);
    body(
        p,
        cx,
        label_text,
        (20.0, y, 364.0, 25.0),
        Al::Left,
        cx.a.palette.text,
    );
    menu_btn(
        p,
        cx,
        "<",
        (384.0, y, 22.0, 25.0),
        Some(Action::VideoSlider(slider, -1)),
    );
    // track and thumb
    let mut track = abs(408.0, y + 8.0, 200.0, 9.0);
    track.border = UiRect::all(px(1));
    p.spawn((
        track,
        BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.9)),
        BorderColor::all(BORDER),
    ));
    let x = 408.0 + (v + 1.0) / 2.0 * 200.0;
    p.spawn((
        abs(x - 4.0, y + 2.0, 8.0, 21.0),
        BackgroundColor(cx.a.palette.text_hover),
    ));
    menu_btn(
        p,
        cx,
        ">",
        (610.0, y, 22.0, 25.0),
        Some(Action::VideoSlider(slider, 1)),
    );
    body(
        p,
        cx,
        format!("{}%", ((v + 1.0) * 100.0).round() as i32),
        (642.0, y, 338.0, 25.0),
        Al::Left,
        cx.a.palette.text,
    );
}

pub(crate) fn video_page(p: &mut P, cx: &Cx) {
    let v = &cx.ui.video;
    spawn_pic(p, cx.a, MENU_VIDEO_BG, 0.0, 0.0, None);
    heading(p, cx, "Videooptionen", (271.0, 30.0, 482.0, 40.0));
    let text = cx.a.palette.text;
    body(
        p,
        cx,
        "Auflösung",
        (20.0, 420.0, 364.0, 25.0),
        Al::Left,
        text,
    );
    menu_btn(
        p,
        cx,
        "<",
        (384.0, 420.0, 22.0, 25.0),
        Some(Action::VideoMode(-1)),
    );
    menu_btn(
        p,
        cx,
        ">",
        (610.0, 420.0, 22.0, 25.0),
        Some(Action::VideoMode(1)),
    );
    body(
        p,
        cx,
        format!("{} x {}", v.width, v.height),
        (642.0, 420.0, 338.0, 25.0),
        Al::Left,
        text,
    );
    body(
        p,
        cx,
        "Vertikale Synchronisation",
        (20.0, 448.0, 364.0, 25.0),
        Al::Left,
        text,
    );
    menu_btn(
        p,
        cx,
        if v.vsync { "An" } else { "Aus" },
        (408.0, 448.0, 100.0, 25.0),
        Some(Action::VideoVsync),
    );
    for (label_text, y, s) in [
        ("Helligkeit", 476.0, Slider::Brightness),
        ("Kontrast", 504.0, Slider::Contrast),
        ("Gamma Rot", 532.0, Slider::Red),
        ("Gamma Grün", 560.0, Slider::Green),
        ("Gamma Blau", 588.0, Slider::Blue),
    ] {
        slider_row(p, cx, label_text, y, s);
    }
    body(
        p,
        cx,
        cx.t(
            "HUD_Video_ResetWarning",
            "Änderungen der Auflösung gelten nach dem Übernehmen sofort.",
        ),
        (20.0, 624.0, 980.0, 52.0),
        Al::Center,
        cx.a.palette.text_disabled,
    );
    menu_btn(
        p,
        cx,
        "Übernehmen",
        (332.0, 682.0, 120.0, 32.0),
        (*v != *cx.applied).then_some(Action::VideoApply),
    );
    menu_btn(
        p,
        cx,
        "Zurück",
        (462.0, 682.0, 100.0, 32.0),
        Some(Action::MenuBack),
    );
}
