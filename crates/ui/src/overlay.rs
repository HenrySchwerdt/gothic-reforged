//! On-screen layers fed by the models of E33 / E34: the HUD log (`gCHUDLog`), floating text
//! effects (`gCHUDTextEffect`) and the centred screen output (`gCScreenOutputCtrl`). The models
//! own all rules; this file only mirrors them into text entities.
use crate::assets::UiAssets;
use crate::log::{HudLog, LogSettings};
use crate::style::*;
use crate::texteffect::*;
use crate::*;

/// Parent of the log lines (the "Log" static, `(15,219)-(884,471)`).
#[derive(Component)]
pub(crate) struct LogLayer;
/// Parent of the floating effect lines.
#[derive(Component)]
pub(crate) struct EffectLayer;
/// Parent of the centred screen output lines.
#[derive(Component)]
pub(crate) struct OutputLayer;

/// Top of the effect rows and the screen output window [I: the controls are created in code,
/// not in `g3.gui.res`; both sit in the upper third of the screen].
const EFFECT_TOP: f32 = 150.0;
const OUTPUT_TOP: f32 = 250.0;
const LOG_AREA: (f32, f32, f32, f32) = (15.0, 219.0, 869.0, 252.0);

pub(crate) fn spawn_overlays(mut commands: Commands) {
    let (x, y, w, h) = LOG_AREA;
    commands.spawn((
        LogLayer,
        GlobalZIndex(6),
        Node {
            overflow: Overflow::clip(),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            ..abs(x, y, w, h)
        },
    ));
    let centred = |top: f32| Node {
        position_type: PositionType::Absolute,
        left: px(0),
        width: percent(100),
        top: px(top),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        ..default()
    };
    commands.spawn((EffectLayer, GlobalZIndex(21), centred(EFFECT_TOP)));
    commands.spawn((OutputLayer, GlobalZIndex(21), centred(OUTPUT_TOP)));
}

/// Host messages into the models, and the clocks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn feed_overlays(
    time: Res<Time>,
    settings: Res<LogSettings>,
    mut log: ResMut<HudLog>,
    mut effects: ResMut<TextEffects>,
    mut output: ResMut<ScreenOutput>,
    mut prints: MessageReader<PrintScreen>,
    mut ends: MessageReader<PrintScreenEnd>,
    mut floats: MessageReader<FloatingText>,
) {
    for p in prints.read() {
        output.print(&p.text, p.color, p.seconds);
    }
    if ends.read().count() > 0 {
        output.print_end();
    }
    for f in floats.read() {
        effects.push(f.row, f.text.clone(), f.color, f.flags, f.seconds);
    }
    let dt = time.delta_secs();
    log.tick(&settings, dt);
    effects.tick(dt);
    output.tick(dt);
}

fn clear(commands: &mut Commands, kids: Option<&Children>) {
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
}

fn text_node(font: &Handle<Font>, text: String, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextShadow::default(),
    )
}

/// Rebuild the layers from the models (a handful of lines, so no diffing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_overlays(
    mut commands: Commands,
    assets: Res<UiAssets>,
    ui: Res<UiState>,
    settings: Res<LogSettings>,
    log: Res<HudLog>,
    effects: Res<TextEffects>,
    output: Res<ScreenOutput>,
    log_layer: Query<(Entity, Option<&Children>), With<LogLayer>>,
    effect_layer: Query<(Entity, Option<&Children>), With<EffectLayer>>,
    output_layer: Query<(Entity, Option<&Children>), With<OutputLayer>>,
) {
    let font = &assets.font;
    if let Ok((e, kids)) = log_layer.single() {
        clear(&mut commands, kids);
        if ui.gui.flags().log_a || ui.gui.flags().log_b {
            let rows = (LOG_AREA.3 / HudLog::LINE_H) as usize;
            let (lines, shift) = log.visible(rows);
            for l in lines {
                let c = settings
                    .color(l.category)
                    .with_alpha(HudLog::alpha(&settings, l));
                let t = commands
                    .spawn(text_node(font, l.text.clone(), 15.0, c))
                    .id();
                commands.entity(e).add_child(t);
            }
            if shift > 0.0 {
                commands
                    .entity(e)
                    .insert(UiTransform::from_translation(Val2::px(0.0, shift)));
            } else {
                commands.entity(e).insert(UiTransform::default());
            }
        }
    }
    if let Ok((e, kids)) = effect_layer.single() {
        clear(&mut commands, kids);
        if !ui.gui.hud_hidden {
            for d in effects.draws() {
                let mut n = Node {
                    position_type: PositionType::Absolute,
                    top: px(d.row as f32 * TextEffects::ROW_H + d.shake.y),
                    ..default()
                };
                match d.align {
                    EffectAlign::Left => n.left = px(40.0 + d.shake.x),
                    EffectAlign::Right => n.right = px(40.0 - d.shake.x),
                    EffectAlign::Center => {
                        n.align_self = AlignSelf::Center;
                        n.position_type = PositionType::Relative;
                        n.margin.top = px(d.row as f32 * TextEffects::ROW_H);
                        n.top = px(d.shake.y);
                        n.left = px(d.shake.x);
                    }
                }
                let t = commands
                    .spawn((n, text_node(font, d.text, 22.0, d.color)))
                    .id();
                commands.entity(e).add_child(t);
            }
        }
    }
    if let Ok((e, kids)) = output_layer.single() {
        clear(&mut commands, kids);
        if !ui.gui.hud_hidden {
            for l in output.lines() {
                let t = commands
                    .spawn(text_node(font, l.text.clone(), 20.0, l.color))
                    .id();
                commands.entity(e).add_child(t);
            }
        }
    }
}
