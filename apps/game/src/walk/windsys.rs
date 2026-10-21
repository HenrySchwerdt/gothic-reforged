//! Wind systems: the grass response (`Render.GrassResponseRange/Intensity` of `ge3.INI`) needs the
//! hero position in every ground-cover material; the wave itself runs on the GPU from the global
//! time (see `shaders/gothic_vertex.wesl`).
use super::{CM, Player, gmat};
use bevy::prelude::*;

pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, grass_focus);
    }
}

/// Write the hero position into the vegetation materials (only those: touching an asset
/// re-prepares its bind group).
fn grass_focus(
    player: Res<Player>,
    mut materials: ResMut<Assets<gmat::GothicMaterial>>,
    mut last: Local<Option<Vec3>>,
) {
    if std::env::var_os("GR_NOGRASSFOCUS").is_some() {
        return;
    }
    let pos = super::observer_cm(&player) * CM;
    if last.is_some_and(|l| l.distance_squared(pos) < 0.02 * 0.02) {
        return;
    }
    *last = Some(pos);
    if std::env::var_os("GR_WINDLOG").is_some() {
        eprintln!(
            "grass focus update {pos:?}, {} wind materials",
            gmat::wind_materials().len()
        );
    }
    for id in gmat::wind_materials() {
        if let Some(mut m) = materials.get_mut(id)
            && m.params.wind_mat.x == gmat::WindMode::Vegetation as i32 as f32
        {
            m.params.player = pos.extend(1.0);
        }
    }
}
