//! Asset-side residency driven by the PVS grid.
//!
//! The engine pages geometry per PVS cell. A host renders and loads in its own tiles (the game
//! uses 2500-unit terrain tiles), so [`TileStreamer`] turns the cells that the prefetch grid
//! keeps valid into the set of tiles that must be resident, and hands loads (nearest first) and
//! unloads (farthest first) to the host under a per-frame budget. No file I/O happens here.
use crate::pvs::{FrustumDatabase, MAIN_CELL_SIZE, PvsCell, PvsError, PvsPrefetcher, PvsSphere};
use bevy::prelude::*;
use std::collections::{BTreeSet, HashMap};

/// Index of a host tile: `floor(position / tile_size)` on X and Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct CellId {
    pub x: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Resource)]
pub struct StreamingSettings {
    /// Edge of a host tile in world units (`cell_at` and the tile keys use it).
    pub cell_size: f32,
    /// Edge of a PVS cell (`Render.PrefetchGridCellSize`).
    pub pvs_cell_size: u32,
    /// Far clip plane of the observing camera; the PVS sphere radius.
    pub far: f32,
    /// Loads and unloads handed out per frame (each).
    pub operations_per_frame: usize,
}

impl Default for StreamingSettings {
    fn default() -> Self {
        Self {
            cell_size: 2_500.0,
            pvs_cell_size: MAIN_CELL_SIZE,
            far: 10_000.0,
            operations_per_frame: 4,
        }
    }
}

impl StreamingSettings {
    pub fn validate(&self) -> bool {
        self.cell_size.is_finite()
            && self.cell_size > 0.0
            && self.far.is_finite()
            && self.far > 0.0
            && self.pvs_cell_size > 0
            && self.operations_per_frame > 0
    }

    /// Tile containing a world position.
    pub fn cell_at(&self, position: Vec3) -> CellId {
        CellId {
            x: (position.x / self.cell_size).floor() as i32,
            z: (position.z / self.cell_size).floor() as i32,
        }
    }

    /// The far clip that makes the grid reach at least `extent` units from the camera in every
    /// direction: the rectangle covers `0.4 * far` plus a full cell margin.
    pub fn far_for_extent(extent: f32, pvs_cell_size: u32) -> f32 {
        ((extent - pvs_cell_size as f32) / crate::pvs::RECT_RADIUS_FACTOR).max(1.0)
    }
}

/// Tiles of one PVS cell.
fn tiles_of_cell(cell: PvsCell, pvs: u32, tile: f32) -> impl Iterator<Item = CellId> {
    let [x0, z0] = cell.min_corner(pvs);
    let span = pvs as f32;
    let lo = |v: f32| (v / tile).floor() as i32;
    let hi = |v: f32| ((v + span - 1.0) / tile).floor() as i32;
    let (tx0, tx1, tz0, tz1) = (lo(x0), hi(x0), lo(z0), hi(z0));
    (tx0..=tx1).flat_map(move |x| (tz0..=tz1).map(move |z| CellId { x, z }))
}

/// Residency of host tiles under the PVS rules.
pub struct TileStreamer {
    settings: StreamingSettings,
    prefetch: PvsPrefetcher,
    db: u8,
    wanted: BTreeSet<CellId>,
    resident: BTreeSet<CellId>,
    weights: HashMap<(i32, i32), u32>,
}

impl TileStreamer {
    pub fn new(settings: StreamingSettings) -> Self {
        let mut prefetch = PvsPrefetcher::new();
        let db = prefetch
            .register_database(settings.pvs_cell_size.max(1))
            .expect("a fresh prefetcher has room for a database");
        Self {
            settings,
            prefetch,
            db,
            wanted: BTreeSet::new(),
            resident: BTreeSet::new(),
            weights: HashMap::new(),
        }
    }

    pub fn settings(&self) -> &StreamingSettings {
        &self.settings
    }
    pub fn database(&self) -> Option<&FrustumDatabase> {
        self.prefetch.database(self.db)
    }
    pub fn resident(&self) -> &BTreeSet<CellId> {
        &self.resident
    }
    pub fn wanted(&self) -> &BTreeSet<CellId> {
        &self.wanted
    }
    pub fn is_resident(&self, t: CellId) -> bool {
        self.resident.contains(&t)
    }

    /// Provide the world data's weight (placed meshes) per PVS cell, keyed by cell index.
    pub fn set_cell_weights(&mut self, weights: impl IntoIterator<Item = ((i32, i32), u32)>) {
        self.weights = weights.into_iter().collect();
    }

    /// Weight of the PVS cells the grid currently keeps valid.
    pub fn wanted_weight(&self) -> u64 {
        self.prefetch
            .valid_cells(self.db)
            .iter()
            .map(|c| u64::from(self.weights.get(&(c.x, c.z)).copied().unwrap_or(0)))
            .sum()
    }

    /// PVS cells that are currently valid.
    pub fn valid_cells(&self) -> Vec<PvsCell> {
        self.prefetch.valid_cells(self.db)
    }

    /// Run the prefetcher for the observer. Returns whether the wanted tile set changed.
    pub fn observe(&mut self, position: Vec3) -> Result<bool, PvsError> {
        if !self.settings.validate() {
            return Ok(false);
        }
        let sphere = PvsSphere::from_camera(position.to_array(), self.settings.far, false);
        let Some(_delta) = self.prefetch.process(self.db, sphere, false)? else {
            return Ok(false);
        };
        let pvs = self.settings.pvs_cell_size;
        let tile = self.settings.cell_size;
        self.wanted = self
            .prefetch
            .valid_cells(self.db)
            .into_iter()
            .flat_map(|c| tiles_of_cell(c, pvs, tile))
            .collect();
        Ok(true)
    }

    fn distance(&self, t: CellId, p: Vec3) -> f32 {
        let s = self.settings.cell_size;
        let cx = (t.x as f32 + 0.5) * s;
        let cz = (t.z as f32 + 0.5) * s;
        (cx - p.x).abs().max((cz - p.z).abs())
    }

    /// Up to `limit` resident tiles that are no longer wanted, farthest from `at` first. They
    /// leave the resident set.
    pub fn take_unloads(&mut self, at: Vec3, limit: usize) -> Vec<CellId> {
        let mut v: Vec<CellId> = self.resident.difference(&self.wanted).copied().collect();
        v.sort_by(|a, b| {
            self.distance(*b, at)
                .total_cmp(&self.distance(*a, at))
                .then(a.cmp(b))
        });
        v.truncate(limit);
        for t in &v {
            self.resident.remove(t);
        }
        v
    }

    /// Up to `limit` wanted tiles that are not resident yet, nearest to `at` first. They join
    /// the resident set (the host is now loading them).
    pub fn next_loads(&mut self, at: Vec3, limit: usize) -> Vec<CellId> {
        let mut v: Vec<CellId> = self.wanted.difference(&self.resident).copied().collect();
        v.sort_by(|a, b| {
            self.distance(*a, at)
                .total_cmp(&self.distance(*b, at))
                .then(a.cmp(b))
        });
        v.truncate(limit);
        self.resident.extend(v.iter().copied());
        v
    }

    /// One frame for hosts that load synchronously: observe, then apply the budget. Returns
    /// `(loads, unloads)`.
    pub fn step(&mut self, at: Vec3) -> Result<(Vec<CellId>, Vec<CellId>), PvsError> {
        self.observe(at)?;
        let n = self.settings.operations_per_frame;
        let unload = self.take_unloads(at, n);
        let load = self.next_loads(at, n);
        Ok((load, unload))
    }
}

/// Tiles resident around the observer of the headless / debug viewer.
#[derive(Debug, Resource, Default)]
pub struct Residency {
    pub cells: BTreeSet<CellId>,
}

#[derive(Component)]
pub struct StreamingObserver;

#[derive(Resource)]
pub struct Streamer(pub TileStreamer);

pub struct WorldStreamingPlugin;
impl Plugin for WorldStreamingPlugin {
    fn build(&self, app: &mut App) {
        let settings = app
            .world()
            .get_resource::<StreamingSettings>()
            .cloned()
            .unwrap_or_default();
        app.insert_resource(Streamer(TileStreamer::new(settings.clone())))
            .insert_resource(settings)
            .init_resource::<Residency>()
            .add_systems(Update, update_residency);
    }
}

fn update_residency(
    mut streamer: ResMut<Streamer>,
    mut residency: ResMut<Residency>,
    observers: Query<&Transform, With<StreamingObserver>>,
) {
    let Ok(transform) = observers.single() else {
        return;
    };
    if !transform.translation.is_finite() {
        return;
    }
    let Ok((load, unload)) = streamer.0.step(transform.translation) else {
        return;
    };
    if load.is_empty() && unload.is_empty() {
        return;
    }
    for t in unload {
        residency.cells.remove(&t);
    }
    residency.cells.extend(load);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streamer(far: f32) -> TileStreamer {
        TileStreamer::new(StreamingSettings {
            far,
            ..StreamingSettings::default()
        })
    }

    #[test]
    fn floors_negative_coordinates() {
        assert_eq!(
            StreamingSettings::default().cell_at(Vec3::new(-0.1, 0.0, -2_500.1)),
            CellId { x: -1, z: -2 }
        );
    }

    #[test]
    fn a_pvs_cell_covers_four_by_four_tiles() {
        let c = PvsCell::of(5_000.0, 5_000.0, MAIN_CELL_SIZE);
        let tiles: Vec<_> = tiles_of_cell(c, MAIN_CELL_SIZE, 2_500.0).collect();
        assert_eq!(tiles.len(), 16);
        assert!(tiles.contains(&CellId { x: 0, z: 0 }) && tiles.contains(&CellId { x: 3, z: 3 }));
    }

    #[test]
    fn first_observation_wants_the_3x3_block_of_cells() {
        let mut s = streamer(10_000.0);
        assert!(s.observe(Vec3::new(5_000.0, 0.0, 5_000.0)).unwrap());
        assert_eq!(s.wanted().len(), 9 * 16);
        assert!(!s.observe(Vec3::new(5_100.0, 0.0, 5_000.0)).unwrap());
    }

    #[test]
    fn loads_nearest_first_under_budget_and_converge() {
        let mut s = streamer(10_000.0);
        let at = Vec3::new(5_000.0, 0.0, 5_000.0);
        s.observe(at).unwrap();
        let first = s.next_loads(at, 4);
        assert_eq!(first.len(), 4);
        // The first tiles touch the camera (it stands on a corner of four tiles).
        let touching = s.distance(first[0], at);
        assert!(touching <= 1_250.0);
        assert!(first.iter().all(|t| s.distance(*t, at) <= 1_250.0));
        let mut seen = first;
        loop {
            let more = s.next_loads(at, 4);
            if more.is_empty() {
                break;
            }
            assert!(more.len() <= 4);
            seen.extend(more);
        }
        assert_eq!(seen.len(), s.wanted().len());
        assert_eq!(s.resident(), s.wanted());
        assert!(s.take_unloads(at, 4).is_empty());
    }

    #[test]
    fn unloads_are_farthest_first_and_bounded() {
        let mut s = streamer(10_000.0);
        let a = Vec3::new(5_000.0, 0.0, 5_000.0);
        s.observe(a).unwrap();
        while !s.next_loads(a, 64).is_empty() {}
        // Jump far away: everything resident is stale.
        let b = Vec3::new(305_000.0, 0.0, 5_000.0);
        assert!(s.observe(b).unwrap());
        let drop = s.take_unloads(b, 4);
        assert_eq!(drop.len(), 4);
        let d0 = s.distance(drop[0], b);
        let rest = s.take_unloads(b, 1000);
        assert!(rest.iter().all(|t| s.distance(*t, b) <= d0));
        assert!(s.resident().is_empty());
    }

    #[test]
    fn far_for_extent_guarantees_the_requested_reach() {
        let far = StreamingSettings::far_for_extent(25_000.0, MAIN_CELL_SIZE);
        let mut s = streamer(far);
        // Walk along X; right after every re-evaluation all four directions reach 25 000 units.
        let mut x = 5_000.0;
        while x < 80_000.0 {
            let at = Vec3::new(x, 0.0, 5_000.0);
            if s.observe(at).unwrap() {
                let tile = |dx: f32, dz: f32| s.settings().cell_at(at + Vec3::new(dx, 0.0, dz));
                for (dx, dz) in [
                    (24_000.0, 0.0),
                    (-24_000.0, 0.0),
                    (0.0, 24_000.0),
                    (0.0, -24_000.0),
                ] {
                    assert!(s.wanted().contains(&tile(dx, dz)), "x={x} d=({dx},{dz})");
                }
            }
            x += 700.0;
        }
    }

    #[test]
    fn walk_loads_and_unloads_tiles_consistently() {
        let mut s = streamer(StreamingSettings::far_for_extent(15_000.0, MAIN_CELL_SIZE));
        let mut resident: BTreeSet<CellId> = BTreeSet::new();
        let mut peak = 0;
        let mut x = 5_000.0;
        for _ in 0..4_000 {
            let at = Vec3::new(x, 0.0, 5_000.0 + (x * 0.3).sin() * 3_000.0);
            let (load, unload) = s.step(at).unwrap();
            assert!(load.len() <= 4 && unload.len() <= 4);
            for t in unload {
                assert!(resident.remove(&t));
            }
            for t in load {
                assert!(resident.insert(t));
            }
            peak = peak.max(resident.len());
            x += 40.0;
        }
        assert_eq!(&resident, s.resident());
        // Everything far behind is gone (walked 160 000 units).
        assert!(
            resident
                .iter()
                .all(|t| t.x >= (5_000.0 / 2_500.0) as i32 + 40)
        );
        assert!(peak < 1_000, "{peak}");
    }

    #[test]
    fn weights_sum_over_valid_cells() {
        let mut s = streamer(10_000.0);
        s.set_cell_weights([((100, 100), 7), ((101, 100), 5), ((500, 500), 99)]);
        s.observe(Vec3::new(5_000.0, 0.0, 5_000.0)).unwrap();
        assert_eq!(s.wanted_weight(), 12);
    }

    #[test]
    fn invalid_settings_do_nothing() {
        let mut s = TileStreamer::new(StreamingSettings {
            far: f32::NAN,
            ..StreamingSettings::default()
        });
        assert!(!s.observe(Vec3::ZERO).unwrap());
        assert!(s.wanted().is_empty());
    }

    #[test]
    fn headless_plugin_converges() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, WorldStreamingPlugin));
        app.world_mut().spawn((
            StreamingObserver,
            Transform::from_xyz(5_000.0, 0.0, 5_000.0),
        ));
        for _ in 0..80 {
            app.update();
        }
        assert_eq!(app.world().resource::<Residency>().cells.len(), 9 * 16);
    }
}
