//! The focus name (drawn by `gothic_ui::FocusLabel`): like the original (`gCGUIEntityRenderer::DrawFocusName`) the only text the
//! game draws for the thing you look at is its name above it; there is no help overlay and no
//! key hint. Outline and colour: the original draws four black copies and the coloured name on
//! top, the attitude palette is not recovered [U], so the name is plain light text.
use super::interact::Interaction;
use super::objects::WorldObjects;
use super::{Mode, PlayState, Roster, Target};
use crate::walk::PlayerCamera;
use bevy::prelude::*;
use gothic_runtime::CoreRes;
use gothic_ui::{FocusAttitude, UiState};

pub fn register(app: &mut App) {
    app.add_systems(Update, (update, sync_gui_open));
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
    mut label: ResMut<gothic_ui::FocusLabel>,
) {
    let Some(target) = state.target else {
        return label.clear();
    };
    if state.mode != Mode::Explore || ui.is_open() || inter.window.is_some() || state.drawn {
        return label.clear();
    }
    let mut attitude = FocusAttitude::Neutral;
    let (name, at) = match target {
        Target::Npc(id) | Target::Body(id) => {
            let name = &core.0.sim.actor(id).name;
            let shown = roster.display.get(name).unwrap_or(name).clone();
            let Some((p, _)) = core.0.sim.world.actors.get(&id) else {
                return label.clear();
            };
            if matches!(target, Target::Npc(_)) {
                use gothic_sim::types::Attitude;
                attitude = match core.0.sim.attitude(id, core.0.hero) {
                    Attitude::Friendly => FocusAttitude::Friendly,
                    Attitude::Neutral => FocusAttitude::Neutral,
                    _ => FocusAttitude::Hostile,
                };
            }
            (shown, Vec3::new(p.x, p.y + 200.0, p.z))
        }
        Target::Object(i) | Target::Item(i) => {
            let o = &objects.list[i];
            if o.def.focus_name == "Disable" {
                return label.clear();
            }
            (objects.display(i), o.pos_cm + Vec3::Y * 90.0)
        }
    };
    let Ok((camera, gt)) = cam.single() else {
        return label.clear();
    };
    let Ok(p) = camera.world_to_viewport(gt, at * 0.01) else {
        return label.clear();
    };
    let s = scale.0.max(0.01);
    label.show(&name, attitude, p.x / s, p.y / s);
}
