//! The focus name: like the original (`gCGUIEntityRenderer::DrawFocusName`) the only text the
//! game draws for the thing you look at is its name above it; there is no help overlay and no
//! key hint. Outline and colour: the original draws four black copies and the coloured name on
//! top, the attitude palette is not recovered [U], so the name is plain light text.
use super::interact::Interaction;
use super::objects::WorldObjects;
use super::{Mode, PlayState, Roster, Target};
use crate::walk::PlayerCamera;
use bevy::prelude::*;
use gothic_runtime::CoreRes;
use gothic_ui::{UiAssets, UiState};

#[derive(Component)]
struct FocusName;

#[derive(Component)]
struct FocusText;

const BOX_W: f32 = 320.0;

pub fn register(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (update, sync_gui_open));
}

/// While a full screen or an interaction window is up the hero cannot be hurt
/// (`gui_open` of the combat context, as the original's HUD pages set it).
fn sync_gui_open(
    ui: Res<UiState>,
    inter: Res<Interaction>,
    mut cmd: MessageWriter<gothic_runtime::CoreCommand>,
    mut last: Local<bool>,
) {
    let want = ui.is_open() || inter.window.is_some();
    if want != *last {
        *last = want;
        cmd.write(gothic_runtime::CoreCommand(
            gothic_runtime::Command::GuiOpen(want),
        ));
    }
}

fn spawn(mut commands: Commands, assets: Option<Res<UiAssets>>) {
    let font = assets.map(|a| a.font.clone());
    commands
        .spawn((
            FocusName,
            Node {
                position_type: PositionType::Absolute,
                width: px(BOX_W),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                FocusText,
                Text::new(""),
                TextFont {
                    font: font.map(Into::into).unwrap_or_default(),
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.9, 0.7)),
                TextShadow::default(),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn update(
    state: Res<PlayState>,
    core: Res<CoreRes>,
    roster: Res<Roster>,
    objects: Res<WorldObjects>,
    inter: Res<Interaction>,
    ui: Res<UiState>,
    scale: Res<UiScale>,
    cam: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut root: Query<(&mut Node, &mut Visibility), With<FocusName>>,
    mut text: Query<&mut Text, With<FocusText>>,
) {
    let (Ok((mut node, mut vis)), Ok(mut t)) = (root.single_mut(), text.single_mut()) else {
        return;
    };
    let hide = |vis: &mut Visibility| {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
    };
    let Some(target) = state.target else {
        return hide(&mut vis);
    };
    if state.mode != Mode::Explore || ui.is_open() || inter.window.is_some() || state.drawn {
        return hide(&mut vis);
    }
    let (name, at) = match target {
        Target::Npc(id) | Target::Body(id) => {
            let name = &core.0.sim.actor(id).name;
            let shown = roster.display.get(name).unwrap_or(name).clone();
            let Some((p, _)) = core.0.sim.world.actors.get(&id) else {
                return hide(&mut vis);
            };
            (shown, Vec3::new(p.x, p.y + 200.0, p.z))
        }
        Target::Object(i) | Target::Item(i) => {
            let o = &objects.list[i];
            if o.def.focus_name == "Disable" {
                return hide(&mut vis);
            }
            (objects.display(i), o.pos_cm + Vec3::Y * 90.0)
        }
    };
    let Ok((camera, gt)) = cam.single() else {
        return hide(&mut vis);
    };
    let Ok(p) = camera.world_to_viewport(gt, at * 0.01) else {
        return hide(&mut vis);
    };
    let s = scale.0.max(0.01);
    node.left = px(p.x / s - BOX_W * 0.5);
    node.top = px(p.y / s - 30.0);
    if t.0 != name {
        t.0 = name;
    }
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
}
