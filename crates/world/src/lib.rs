//! Independent spatial residency planner. Grid coordinates are development units,
//! not yet mapped to Genome coordinates or original world sectors.
use bevy::prelude::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Component)]
pub struct CellId {
    pub x: i32,
    pub z: i32,
}

#[derive(Debug, Resource)]
pub struct StreamingSettings {
    pub cell_size: f32,
    pub load_radius: i32,
    pub retain_radius: i32,
    pub operations_per_frame: usize,
}
impl Default for StreamingSettings {
    fn default() -> Self {
        Self {
            cell_size: 64.0,
            load_radius: 3,
            retain_radius: 4,
            operations_per_frame: 4,
        }
    }
}

impl StreamingSettings {
    pub fn validate(&self) -> bool {
        self.cell_size.is_finite()
            && self.cell_size > 0.0
            && self.load_radius >= 0
            && self.retain_radius >= self.load_radius
            && self.retain_radius <= 32
            && self.operations_per_frame > 0
    }
    pub fn cell_at(&self, position: Vec3) -> CellId {
        CellId {
            x: (position.x / self.cell_size).floor() as i32,
            z: (position.z / self.cell_size).floor() as i32,
        }
    }
}

#[derive(Debug, Resource, Default)]
pub struct Residency {
    pub cells: BTreeSet<CellId>,
}

#[derive(Debug, Default)]
pub struct StreamingDelta {
    pub load: Vec<CellId>,
    pub unload: Vec<CellId>,
}

/// Deterministic nearest-first loading and farthest-first eviction with hysteresis.
/// At most operations_per_frame combined changes. No filesystem I/O on this path.
pub fn plan(
    settings: &StreamingSettings,
    resident: &BTreeSet<CellId>,
    center: CellId,
) -> StreamingDelta {
    if !settings.validate() {
        return StreamingDelta::default();
    }
    let distance = |c: &CellId| {
        (i64::from(c.x) - i64::from(center.x))
            .abs()
            .max((i64::from(c.z) - i64::from(center.z)).abs())
    };
    let mut unload: Vec<_> = resident
        .iter()
        .filter(|c| distance(c) > i64::from(settings.retain_radius))
        .copied()
        .collect();
    unload.sort_by_key(|c| (std::cmp::Reverse(distance(c)), *c));
    unload.truncate(settings.operations_per_frame);
    let mut load = Vec::new();
    for z in -settings.load_radius..=settings.load_radius {
        for x in -settings.load_radius..=settings.load_radius {
            if let (Some(x), Some(z)) = (center.x.checked_add(x), center.z.checked_add(z)) {
                let c = CellId { x, z };
                if !resident.contains(&c) {
                    load.push(c);
                }
            }
        }
    }
    load.sort_by_key(|c| (distance(c), *c));
    load.truncate(settings.operations_per_frame - unload.len());
    StreamingDelta { load, unload }
}

#[derive(Component)]
pub struct StreamingObserver;

pub struct WorldStreamingPlugin;
impl Plugin for WorldStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreamingSettings>()
            .init_resource::<Residency>()
            .add_systems(Update, update_residency);
    }
}
fn update_residency(
    settings: Res<StreamingSettings>,
    mut residency: ResMut<Residency>,
    observers: Query<&Transform, With<StreamingObserver>>,
) {
    let Ok(transform) = observers.single() else {
        return;
    };
    if !transform.translation.is_finite() {
        return;
    }
    let delta = plan(
        &settings,
        &residency.cells,
        settings.cell_at(transform.translation),
    );
    if delta.load.is_empty() && delta.unload.is_empty() {
        return;
    }
    for cell in delta.unload {
        residency.cells.remove(&cell);
    }
    residency.cells.extend(delta.load);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floors_negative_coordinates() {
        assert_eq!(
            StreamingSettings::default().cell_at(Vec3::new(-0.1, 0.0, -64.1)),
            CellId { x: -1, z: -2 }
        );
    }
    #[test]
    fn bounded_convergent_and_hysteretic() {
        let s = StreamingSettings::default();
        let mut resident = BTreeSet::new();
        let center = CellId { x: 0, z: 0 };
        for _ in 0..20 {
            let d = plan(&s, &resident, center);
            assert!(d.load.len() + d.unload.len() <= 4);
            resident.extend(d.load);
        }
        assert_eq!(resident.len(), 49);
        assert!(plan(&s, &resident, center).load.is_empty());
        assert!(plan(&s, &resident, CellId { x: 1, z: 0 }).unload.is_empty());
        let d = plan(&s, &resident, CellId { x: 100, z: 100 });
        assert_eq!(d.unload.len(), 4);
        assert!(d.load.is_empty());
    }
    #[test]
    fn headless_plugin_converges() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, WorldStreamingPlugin));
        app.world_mut()
            .spawn((StreamingObserver, Transform::default()));
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(app.world().resource::<Residency>().cells.len(), 49);
    }
}
