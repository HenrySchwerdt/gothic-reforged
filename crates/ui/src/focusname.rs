//! The floating focus name (`gCGUIEntityRenderer::DrawFocusName`, Game:0x10079cb0): the only text
//! the original draws for the thing under the crosshair. It is the entity's name in a coloured
//! bold sans font (Tahoma Bold) centred above the entity, with a black outline made of four
//! offset copies drawn first, faded by the focus time.
//!
//! The host (play mode) computes the screen position and writes [`FocusLabel`] every frame; this
//! module draws it. Hook in `apps/game/src/play/hud.rs`: replace the private `FocusName` node and
//! its `update` by `label.show(&name, attitude, p.x / s, p.y / s)` (`label: ResMut<gothic_ui::FocusLabel>`) (design pixels,
//! `p` the projected head position, `s` the `UiScale`) while a target is focused, and
//! `label.clear()` otherwise.
use crate::assets::UiAssets;
use bevy::prelude::*;

/// Attitude classes that pick the colour (`GetAttitude`, Game:0x1007a0c0, indexes a 3-entry
/// palette). Only the hostile colour was sampled from the shipped game (`Krieger`, 153/23/18);
/// the other two are [I].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FocusAttitude {
    Hostile,
    #[default]
    Neutral,
    Friendly,
}

impl FocusAttitude {
    pub fn color(self) -> Color {
        match self {
            FocusAttitude::Hostile => Color::srgb_u8(153, 23, 18),
            FocusAttitude::Neutral => Color::srgb_u8(224, 213, 178),
            FocusAttitude::Friendly => Color::srgb_u8(164, 214, 120),
        }
    }
}

/// What to draw: the name and where its bottom centre goes (design-space pixels).
#[derive(Resource, Clone, Debug, Default)]
pub struct FocusLabel {
    pub text: String,
    pub at: Vec2,
    pub attitude: FocusAttitude,
    visible: bool,
    /// Fade factor 0..=1 (follows `visible`).
    alpha: f32,
}

/// Fade in and out time of the label in seconds [I].
const FADE_SECS: f32 = 0.15;
/// Font size and the box the text is centred in.
const SIZE: f32 = 16.0;
const BOX_W: f32 = 320.0;
const BOX_H: f32 = 24.0;
/// Offsets of the four black copies drawn under the name (the original's outline passes).
const OUTLINE: [(f32, f32); 4] = [(-1.0, -1.0), (0.0, 2.0), (2.0, 0.0), (0.0, -2.0)];

impl FocusLabel {
    /// Focus something: show `text` above `(x, y)`; the fade keeps running between calls, so the
    /// host may call this every frame.
    pub fn show(&mut self, text: &str, attitude: FocusAttitude, x: f32, y: f32) {
        if self.text != text {
            self.text = text.to_string();
        }
        self.at = Vec2::new(x, y);
        self.attitude = attitude;
        self.visible = true;
    }

    /// Nothing focused: the label fades out.
    pub fn clear(&mut self) {
        self.visible = false;
    }

    /// Opacity right now.
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Advance the fade.
    pub fn tick(&mut self, dt: f32) {
        let step = dt / FADE_SECS;
        self.alpha = if self.visible {
            (self.alpha + step).min(1.0)
        } else {
            (self.alpha - step).max(0.0)
        };
    }
}

#[derive(Component)]
pub(crate) struct FocusLabelRoot;
#[derive(Component)]
pub(crate) struct FocusLabelText(bool);

pub(crate) fn spawn_focus_name(mut commands: Commands, assets: Res<UiAssets>) {
    commands
        .spawn((
            FocusLabelRoot,
            GlobalZIndex(4),
            Node {
                position_type: PositionType::Absolute,
                width: px(BOX_W),
                height: px(BOX_H),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|r| {
            // outline copies first, the coloured name on top
            let passes = OUTLINE.iter().copied().chain(std::iter::once((0.0, 0.0)));
            for (i, (dx, dy)) in passes.enumerate() {
                let top = i == 4;
                r.spawn(Node {
                    position_type: PositionType::Absolute,
                    left: px(dx),
                    top: px(dy),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_child((
                    FocusLabelText(top),
                    Text::new(""),
                    TextFont {
                        font: assets.name_font.clone().into(),
                        font_size: FontSize::Px(SIZE),
                        ..default()
                    },
                    TextColor(Color::BLACK),
                ));
            }
        });
}

pub(crate) fn render_focus_name(
    time: Res<Time>,
    mut label: ResMut<FocusLabel>,
    mut root: Query<(&mut Node, &mut Visibility), With<FocusLabelRoot>>,
    mut texts: Query<(&FocusLabelText, &mut Text, &mut TextColor)>,
) {
    label.tick(time.delta_secs());
    let Ok((mut node, mut vis)) = root.single_mut() else {
        return;
    };
    if label.alpha <= 0.0 {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Inherited;
    node.left = px(label.at.x - BOX_W * 0.5);
    node.top = px(label.at.y - BOX_H);
    for (top, mut t, mut c) in &mut texts {
        if t.0 != label.text {
            t.0 = label.text.clone();
        }
        let base = if top.0 {
            label.attitude.color()
        } else {
            Color::BLACK
        };
        c.0 = base.with_alpha(label.alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_fades_in_and_out() {
        let mut l = FocusLabel::default();
        l.show("Krieger", FocusAttitude::Hostile, 10.0, 20.0);
        assert_eq!(l.alpha(), 0.0);
        l.tick(0.075);
        assert!((l.alpha() - 0.5).abs() < 1e-5);
        l.tick(1.0);
        assert_eq!(l.alpha(), 1.0);
        l.clear();
        l.tick(0.3);
        assert_eq!(l.alpha(), 0.0);
    }

    #[test]
    fn hostile_names_are_the_sampled_red() {
        assert_eq!(FocusAttitude::Hostile.color(), Color::srgb_u8(153, 23, 18));
    }
}
