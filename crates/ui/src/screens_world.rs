//! Map screens (E34): the map document page (`IDD_VIEW_MAP_2`, `gCHUDStaticMap`) and the location
//! buttons of the quest page's world view (`IDD_VIEW_WORLD_2`).
use crate::atlas::*;
use crate::map::*;
use crate::screens::{Cx, body, heading};
use crate::screens_menu::menu_btn;
use crate::style::*;
use crate::*;

/// Client size of the map control (`IDD_VIEW_MAP_2`: static `(0,0)-(576,568)`) and its place on
/// the page.
const VIEW: Vec2 = Vec2::new(576.0, 568.0);
const VIEW_AT: (f32, f32) = (36.0, 51.0);

/// Atlas rectangle of a UV window of a map picture.
fn crop(pic: Pic, w: Rect) -> Rect {
    Rect::new(
        pic.x + w.min.x * pic.w,
        pic.y + w.min.y * pic.h,
        pic.x + w.max.x * pic.w,
        pic.y + w.max.y * pic.h,
    )
}

pub(crate) fn map_page(p: &mut P, cx: &Cx) {
    spawn_pic(p, cx.a, MAP_BACK, 14.0, 16.0, None);
    let sheet = cx.map.sheet.as_ref();
    heading(
        p,
        cx,
        sheet.map_or_else(|| "Karte".to_string(), |s| s.name.clone()),
        (46.0, 22.0, 556.0, 28.0),
    );
    let zoom = cx.ui.map_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let pic = sheet.and_then(|s| cx.a.named_pic(&s.bitmap));
    let (Some(sheet), Some(pic)) = (sheet, pic) else {
        body(
            p,
            cx,
            "Keine Karte vorhanden.",
            (VIEW_AT.0, VIEW_AT.1, VIEW.x, VIEW.y),
            Al::Center,
            TEXT_DIM,
        );
        return;
    };
    let player_uv = sheet
        .calc_uv(cx.map.player.x, cx.map.player.y)
        .unwrap_or(Vec2::splat(0.5));
    let window = view_window(player_uv, VIEW, Vec2::new(pic.w, pic.h), zoom);
    if let Some(img) = cx.a.page(pic.page) {
        p.spawn((
            abs(VIEW_AT.0, VIEW_AT.1, VIEW.x, VIEW.y),
            ImageNode {
                image: img.clone(),
                rect: Some(crop(pic, window)),
                ..default()
            },
        ));
    }
    let at = |uv: Vec2| to_view(uv, window, VIEW).map(|v| v + Vec2::new(VIEW_AT.0, VIEW_AT.1));
    for m in cx.map.markers.iter().filter(|m| m.visible) {
        let Some(pos) = sheet.calc_uv(m.pos.x, m.pos.y).and_then(at) else {
            continue;
        };
        let mut dot = abs(pos.x - 4.0, pos.y - 4.0, 8.0, 8.0);
        dot.border = UiRect::all(px(1));
        p.spawn((
            dot,
            BackgroundColor(Color::srgb(0.8, 0.15, 0.1)),
            BorderColor::all(Color::BLACK),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
        ));
        if zoom >= 1.5 {
            body(
                p,
                cx,
                m.label.clone(),
                (pos.x + 6.0, pos.y - 10.0, 120.0, 20.0),
                Al::Left,
                TEXT_HI,
            );
        }
    }
    // the player's arrow, turned by the camera yaw
    if let Some(pos) = at(player_uv) {
        let size = 26.0;
        let arrow = cx.a.named_pic("g3_hud_arrow.tga").unwrap_or(ARROW);
        let mut e = spawn_pic(
            p,
            cx.a,
            arrow,
            pos.x - size / 2.0,
            pos.y - size / 2.0,
            Some((size, size)),
        );
        e.insert(UiTransform::from_rotation(Rot2::radians(-cx.heading)));
    }
    body(
        p,
        cx,
        format!("{}x", zoom),
        (VIEW_AT.0 + 70.0, VIEW_AT.1 + 8.0, 50.0, 24.0),
        Al::Left,
        TEXT_HI,
    );
    menu_btn(
        p,
        cx,
        "+",
        (VIEW_AT.0 + 8.0, VIEW_AT.1 + 8.0, 26.0, 24.0),
        (zoom < ZOOM_MAX).then_some(Action::MapZoom(1)),
    );
    menu_btn(
        p,
        cx,
        "-",
        (VIEW_AT.0 + 38.0, VIEW_AT.1 + 8.0, 26.0, 24.0),
        (zoom > ZOOM_MIN).then_some(Action::MapZoom(-1)),
    );
}

/// The world view of the quest page: the painted map (`G3_HUD_View_World_Back`, 580x599, shown
/// at 0.886 and shifted so the compass rose sits where the original puts it) clipped to the
/// 442x392 control at `(52,62)`, then the location buttons.
pub(crate) fn world_view(p: &mut P, cx: &Cx) {
    let mut e = p.spawn(Node {
        overflow: Overflow::clip(),
        ..abs(52.0, 62.0, 442.0, 392.0)
    });
    e.with_children(|c| {
        let s = 0.886;
        spawn_pic(
            c,
            cx.a,
            WORLD_MAP,
            -35.0,
            -9.0,
            Some((WORLD_MAP.w * s, WORLD_MAP.h * s)),
        );
    });
    world_locations(p, cx);
}

/// The location buttons and the Haupt / Alle radios over the world view of the quest page,
/// whose control starts at `(52,62)` (`IDD_VIEW_WORLD_2`).
fn world_locations(p: &mut P, cx: &Cx) {
    let (ox, oy) = (52.0, 62.0);
    for (name, x, y) in cx.locations.shown() {
        let Some(pic) = cx.a.named_pic(&format!("g3_hud_icon_location_{name}.tga")) else {
            continue;
        };
        spawn_pic(p, cx.a, pic, ox + x, oy + y, Some((18.0, 18.0)));
    }
    for (name, y, text, all) in [
        ("main", 318.0, "Haupt", false),
        ("world", 342.0, "Alle", true),
    ] {
        let on = cx.locations.all == all;
        let mut hit = abs(ox + 300.0, oy + y - 4.0, 120.0, 26.0);
        hit.align_items = AlignItems::Center;
        hit.column_gap = px(6);
        p.spawn((button(Action::WorldAll(all), Color::NONE, SEL), hit))
            .with_children(|b| {
                if let Some(pic) = cx.a.named_pic(&format!("g3_hud_icon_location_{name}.tga")) {
                    b.spawn(Node {
                        width: px(18),
                        height: px(18),
                        ..default()
                    })
                    .with_children(|i| {
                        spawn_pic(i, cx.a, pic, 0.0, 0.0, Some((18.0, 18.0)));
                    });
                }
                b.spawn(txt(cx.font(), text, 15.0, if on { TEXT_HI } else { TEXT }));
            });
    }
}
