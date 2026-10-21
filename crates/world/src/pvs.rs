//! Potentially-visible-set prefetch grid and its cell items.
//!
//! The original engine pages world geometry through a 2D grid of square cells (10 000 units, the
//! landscape patch size) around each camera ("frustum database"). The grid rectangle follows a
//! sphere centred on the camera whose radius is the far clip plane; a cell that enters the
//! rectangle gets a [`PvsCellItem`], a cell that leaves it is cached out and deleted once no
//! database references it any more.
//!
//! Units are Gothic units (centimetres); the grid uses the X and Z axes, Y is up.
use std::collections::{BTreeMap, BTreeSet};

/// Offset that keeps all cell indices positive (`eCPVSPrefetcher3::CalculateRectIndices`).
pub const GRID_OFFSET: i64 = 1_000_000;
/// Default cell edge (`Render.PrefetchGridCellSize`, `eCFrustumDatabase::Create`).
pub const MAIN_CELL_SIZE: u32 = 10_000;
/// Cell edge of the low-poly proxy world (`Render.PrefetchGridCellSizeLowPoly`).
pub const LOW_POLY_CELL_SIZE: u32 = 34_000;
/// The rectangle covers the camera plus or minus this fraction of the sphere radius.
pub const RECT_RADIUS_FACTOR: f32 = 0.4;
/// The grid is only recalculated after the camera moved this fraction of the radius ...
pub const MOVE_FACTOR: f32 = 0.2;
/// ... or this one for a camera in the alternate ("special") state.
pub const MOVE_FACTOR_SPECIAL: f32 = 0.4;
/// Lower bound of the movement threshold, in units.
pub const MIN_MOVE: f32 = 1_300.0;
/// Radius scale of the sphere for a camera in the alternate state.
pub const SPECIAL_RADIUS_SCALE: f32 = 0.85;

/// Why the grid refused a camera position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvsError {
    /// A coordinate is not finite or beyond the supported world extent (1 000 000 units); the
    /// original aborts with "illegal pvssphere position detected".
    IllegalPosition,
}

impl std::fmt::Display for PvsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal pvs sphere position")
    }
}
impl std::error::Error for PvsError {}

/// Index of a grid cell: `(world + 1 000 000) / cell_size` on both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PvsCell {
    pub x: i32,
    pub z: i32,
}

impl PvsCell {
    /// The cell containing a world position (floor semantics, indices may be negative only for
    /// positions below the supported extent).
    pub fn of(x: f32, z: f32, cell_size: u32) -> Self {
        let size = i64::from(cell_size.max(1));
        let f = |v: f32| ((i64::from(v.floor() as i32) + GRID_OFFSET).div_euclid(size)) as i32;
        Self { x: f(x), z: f(z) }
    }

    /// World-space lower corner `[x, z]` of the cell.
    pub fn min_corner(self, cell_size: u32) -> [f32; 2] {
        let size = i64::from(cell_size);
        [
            (i64::from(self.x) * size - GRID_OFFSET) as f32,
            (i64::from(self.z) * size - GRID_OFFSET) as f32,
        ]
    }
}

/// Inclusive rectangle of cell indices (`bCRect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
}

impl CellRect {
    pub fn contains(&self, c: PvsCell) -> bool {
        c.x >= self.x0 && c.x <= self.x1 && c.z >= self.z0 && c.z <= self.z1
    }
    /// Every cell of the rectangle (cells below index 0 do not exist).
    pub fn cells(&self) -> impl Iterator<Item = PvsCell> + use<> {
        let r = *self;
        (r.x0.max(0)..=r.x1).flat_map(move |x| (r.z0.max(0)..=r.z1).map(move |z| PvsCell { x, z }))
    }
    /// Does the rectangle intersect the world-space box `[min_x, min_z, max_x, max_z]`?
    pub fn intersects_world(&self, cell_size: u32, b: [f32; 4]) -> bool {
        let lo = PvsCell::of(b[0], b[1], cell_size);
        let hi = PvsCell::of(b[2], b[3], cell_size);
        lo.x <= self.x1 && hi.x >= self.x0 && lo.z <= self.z1 && hi.z >= self.z0
    }
}

/// Port of `eCPVSPrefetcher3::CalculateRectIndices` (Engine:0x101aa690): the cell rectangle of a
/// sphere. The centre is truncated to whole units, the half extent is `0.4 * radius` truncated,
/// the result is widened by one cell on every side.
pub fn calculate_rect_indices(
    centre_x: f32,
    centre_z: f32,
    radius: f32,
    cell_size: u32,
) -> Result<CellRect, PvsError> {
    if !(centre_x.is_finite() && centre_z.is_finite() && radius.is_finite()) || cell_size == 0 {
        return Err(PvsError::IllegalPosition);
    }
    let half = i64::from((radius * RECT_RADIUS_FACTOR) as i32);
    let (cx, cz) = (i64::from(centre_x as i32), i64::from(centre_z as i32));
    let size = i64::from(cell_size);
    let lo_x = cx + GRID_OFFSET - half;
    let lo_z = cz + GRID_OFFSET - half;
    // The original divides unsigned values; a negative numerator is outside its world.
    if lo_x < 0 || lo_z < 0 {
        return Err(PvsError::IllegalPosition);
    }
    Ok(CellRect {
        x0: (lo_x / size - 1) as i32,
        z0: (lo_z / size - 1) as i32,
        x1: ((cx + GRID_OFFSET + half) / size + 1) as i32,
        z1: ((cz + GRID_OFFSET + half) / size + 1) as i32,
    })
}

/// Sphere the prefetcher builds from the camera (`eCPVSPrefetcher3::Process`, Engine:0x101aabe0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvsSphere {
    pub centre: [f32; 3],
    pub radius: f32,
}

impl PvsSphere {
    /// Sphere of a camera: radius is the far clip, scaled for the alternate camera state.
    pub fn from_camera(position: [f32; 3], far_clip: f32, special: bool) -> Self {
        Self {
            centre: position,
            radius: if special {
                far_clip * SPECIAL_RADIUS_SCALE
            } else {
                far_clip
            },
        }
    }
}

/// Entity identifier used by the scene model (stable, never reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

/// Port of `eCPVSCellItem` bookkeeping: the entities cached into one cell and the set of
/// databases (bit per database, up to 32) that reference it.
#[derive(Debug, Clone, Default)]
pub struct PvsCellItem {
    /// Static entities that live in this cell.
    pub entities: BTreeSet<EntityId>,
    databases: u32,
    /// Running payload size (`GetNodeFileSize` of the cached-in contexts).
    pub weight: u64,
}

impl PvsCellItem {
    pub fn enable_database(&mut self, db: u8) {
        self.databases |= 1 << (db & 31);
    }
    pub fn disable_database(&mut self, db: u8) {
        self.databases &= !(1 << (db & 31));
    }
    pub fn used_by(&self, db: u8) -> bool {
        self.databases & (1 << (db & 31)) != 0
    }
    pub fn unreferenced(&self) -> bool {
        self.databases == 0
    }
}

/// One frustum database: the grid of one camera (`eCFrustumDatabase` fields used by the
/// prefetcher: cell size +0x1b8, rect +0x1d0, last centre +0x1e0, index +0x21c, force flag +0x224).
#[derive(Debug, Clone)]
pub struct FrustumDatabase {
    pub cell_size: u32,
    pub index: u8,
    rect: CellRect,
    last_centre: [f32; 3],
    forced: bool,
}

impl FrustumDatabase {
    pub fn new(index: u8, cell_size: u32) -> Self {
        Self {
            cell_size,
            index,
            rect: CellRect::default(),
            last_centre: [0.0; 3],
            // A new database must be evaluated at once.
            forced: true,
        }
    }
    pub fn rect(&self) -> CellRect {
        self.rect
    }
}

/// Cells that appeared or vanished in one grid update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GridDelta {
    pub added: Vec<PvsCell>,
    /// Cells cached out of this database. A cell shared with another database is only listed in
    /// `deleted` once the last reference is gone.
    pub removed: Vec<PvsCell>,
    pub deleted: Vec<PvsCell>,
}

/// `eCPVSPrefetcher3`: owns the cell items of all databases (keyed by cell index and size, like
/// the original's 967-bucket hash table) and recalculates a database's rectangle on demand.
#[derive(Debug, Default)]
pub struct PvsPrefetcher {
    databases: Vec<FrustumDatabase>,
    cells: BTreeMap<(i32, i32, u32), PvsCellItem>,
    enabled: bool,
}

impl PvsPrefetcher {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// `EnablePrefetcher` (ini `Render.EnablePVSPrefetcherThread`).
    pub fn enable(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Register a database and return its index (at most 32).
    pub fn register_database(&mut self, cell_size: u32) -> Option<u8> {
        if self.databases.len() >= 32 || cell_size == 0 {
            return None;
        }
        let index = self.databases.len() as u8;
        self.databases.push(FrustumDatabase::new(index, cell_size));
        Some(index)
    }

    pub fn database(&self, db: u8) -> Option<&FrustumDatabase> {
        self.databases.get(usize::from(db))
    }

    /// `ForcePrefetch` / `Invalidate`: the next [`Self::process`] re-evaluates unconditionally.
    pub fn force(&mut self, db: u8) {
        if let Some(d) = self.databases.get_mut(usize::from(db)) {
            d.forced = true;
        }
    }

    /// Port of `eCPVSPrefetcher3::Process` + `RecalculateGrid` (Engine:0x101aabe0, 0x101aa750).
    ///
    /// Returns `Ok(None)` when nothing was re-evaluated: the prefetcher is disabled, the
    /// rectangle is unchanged, or the camera moved less than `max(1300, factor * radius)` since
    /// the last evaluation (`factor` 0.2, or 0.4 for the alternate camera state).
    pub fn process(
        &mut self,
        db: u8,
        sphere: PvsSphere,
        special: bool,
    ) -> Result<Option<GridDelta>, PvsError> {
        if !self.enabled {
            return Ok(None);
        }
        if sphere
            .centre
            .iter()
            .any(|c| !c.is_finite() || c.abs() > 1.0e6)
        {
            return Err(PvsError::IllegalPosition);
        }
        let Some(d) = self.databases.get(usize::from(db)) else {
            return Ok(None);
        };
        let (cell_size, old_rect, forced) = (d.cell_size, d.rect, d.forced);
        let new_rect =
            calculate_rect_indices(sphere.centre[0], sphere.centre[2], sphere.radius, cell_size)?;
        if !forced {
            if new_rect == old_rect {
                return Ok(None);
            }
            let moved = sphere
                .centre
                .iter()
                .zip(d.last_centre)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f32>()
                .sqrt();
            let factor = if special {
                MOVE_FACTOR_SPECIAL
            } else {
                MOVE_FACTOR
            };
            if moved < (sphere.radius * factor).max(MIN_MOVE) {
                return Ok(None);
            }
        }
        let mut delta = GridDelta::default();
        for cell in new_rect.cells() {
            let key = (cell.x, cell.z, cell_size);
            let item = self.cells.entry(key).or_default();
            if !item.used_by(db) {
                item.enable_database(db);
                delta.added.push(cell);
            }
        }
        for cell in old_rect.cells() {
            if new_rect.contains(cell) {
                continue;
            }
            let key = (cell.x, cell.z, cell_size);
            if let Some(item) = self.cells.get_mut(&key) {
                item.disable_database(db);
                delta.removed.push(cell);
                if item.unreferenced() {
                    self.cells.remove(&key);
                    delta.deleted.push(cell);
                }
            }
        }
        let d = &mut self.databases[usize::from(db)];
        d.rect = new_rect;
        d.last_centre = sphere.centre;
        d.forced = false;
        Ok(Some(delta))
    }

    /// `GetValidCells`: the cells currently referenced by a database.
    pub fn valid_cells(&self, db: u8) -> Vec<PvsCell> {
        match self.databases.get(usize::from(db)) {
            Some(d) => d
                .rect
                .cells()
                .filter(|c| self.cells.contains_key(&(c.x, c.z, d.cell_size)))
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn cell(&self, db: u8, cell: PvsCell) -> Option<&PvsCellItem> {
        let d = self.databases.get(usize::from(db))?;
        self.cells.get(&(cell.x, cell.z, d.cell_size))
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// `eCPVSCellItem::CacheInEntity`: register a static entity in the cell holding `position`.
    pub fn add_entity(&mut self, db: u8, id: EntityId, position: [f32; 3], weight: u64) -> bool {
        let Some(d) = self.databases.get(usize::from(db)) else {
            return false;
        };
        let cell = PvsCell::of(position[0], position[2], d.cell_size);
        let key = (cell.x, cell.z, d.cell_size);
        let item = self.cells.entry(key).or_default();
        item.weight += weight;
        item.entities.insert(id)
    }

    /// `eCPVSCellItem::CacheOutEntity`.
    pub fn remove_entity(&mut self, db: u8, id: EntityId, position: [f32; 3]) -> bool {
        let Some(d) = self.databases.get(usize::from(db)) else {
            return false;
        };
        let cell = PvsCell::of(position[0], position[2], d.cell_size);
        self.cells
            .get_mut(&(cell.x, cell.z, d.cell_size))
            .is_some_and(|i| i.entities.remove(&id))
    }

    /// Port of `eCPVSPrefetcher3::Recalibrate` (Engine:0x101a9470): an entity moved, so it
    /// changes cell if the new position lies in another one.
    pub fn recalibrate(&mut self, db: u8, id: EntityId, old: [f32; 3], new: [f32; 3]) {
        let Some(d) = self.databases.get(usize::from(db)) else {
            return;
        };
        let size = d.cell_size;
        if PvsCell::of(old[0], old[2], size) == PvsCell::of(new[0], new[2], size) {
            return;
        }
        if self.remove_entity(db, id, old) {
            self.add_entity(db, id, new, 0);
        }
    }

    /// Static entities of every valid cell of a database, cell by cell.
    pub fn entities_in_valid_cells(&self, db: u8) -> Vec<EntityId> {
        self.valid_cells(db)
            .into_iter()
            .filter_map(|c| self.cell(db, c))
            .flat_map(|i| i.entities.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_matches_documented_3x3_block_at_far_10000() {
        // Far clip 10 000: +-4000 around the camera plus one cell margin = 3 x 3 cells.
        let r = calculate_rect_indices(5_000.0, 5_000.0, 10_000.0, MAIN_CELL_SIZE).unwrap();
        assert_eq!(
            r,
            CellRect {
                x0: 99,
                z0: 99,
                x1: 101,
                z1: 101
            }
        );
        assert_eq!(r.cells().count(), 9);
        // At far 6000 it is the same block.
        let r6 = calculate_rect_indices(5_000.0, 5_000.0, 6_000.0, MAIN_CELL_SIZE).unwrap();
        assert_eq!(r6, r);
    }

    #[test]
    fn low_poly_grid_is_5x5_at_far_100000() {
        let r = calculate_rect_indices(0.0, 0.0, 100_000.0, LOW_POLY_CELL_SIZE).unwrap();
        // +-40 000 around the camera, cells of 34 000: 1 000 000 / 34 000 = 29.4
        assert_eq!(r.x1 - r.x0 + 1, 5);
    }

    #[test]
    fn rect_rejects_positions_outside_the_world() {
        assert!(calculate_rect_indices(-1.1e6, 0.0, 1000.0, MAIN_CELL_SIZE).is_err());
        assert!(calculate_rect_indices(f32::NAN, 0.0, 1000.0, MAIN_CELL_SIZE).is_err());
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        let s = PvsSphere::from_camera([2.0e6, 0.0, 0.0], 10_000.0, false);
        assert_eq!(p.process(db, s, false), Err(PvsError::IllegalPosition));
    }

    #[test]
    fn hysteresis_needs_rect_change_and_movement() {
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        let far = 10_000.0;
        let at = |x: f32| PvsSphere::from_camera([x, 0.0, 5_000.0], far, false);
        let first = p.process(db, at(5_000.0), false).unwrap().unwrap();
        assert_eq!(first.added.len(), 9);
        assert!(first.removed.is_empty());
        // Same rectangle: nothing to do.
        assert_eq!(p.process(db, at(5_500.0), false).unwrap(), None);
        // The rectangle changed (crossing x = 6000 shifts the +4000 edge) but the camera moved
        // only 1500 < max(1300, 0.2 * 10000) = 2000: still no re-evaluation.
        assert_eq!(p.process(db, at(6_500.0), false).unwrap(), None);
        // Moved 2500 > 2000 and the rectangle differs: re-evaluated.
        let d = p.process(db, at(7_500.0), false).unwrap().unwrap();
        assert_eq!(d.added.len(), 3);
        assert!(d.removed.is_empty());
        // A forced database is always evaluated, even without a change.
        p.force(db);
        let d = p.process(db, at(7_500.0), false).unwrap().unwrap();
        assert!(d.added.is_empty() && d.removed.is_empty());
    }

    #[test]
    fn leaving_cells_are_removed_and_deleted() {
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        let at = |x: f32| PvsSphere::from_camera([x, 0.0, 5_000.0], 10_000.0, false);
        p.process(db, at(5_000.0), false).unwrap();
        let d = p.process(db, at(105_000.0), false).unwrap().unwrap();
        assert_eq!(d.removed.len(), 9);
        assert_eq!(d.deleted.len(), 9);
        assert_eq!(p.cell_count(), 9);
        assert_eq!(p.valid_cells(db).len(), 9);
    }

    #[test]
    fn shared_cells_survive_until_the_last_database_leaves() {
        let mut p = PvsPrefetcher::new();
        let a = p.register_database(MAIN_CELL_SIZE).unwrap();
        let b = p.register_database(MAIN_CELL_SIZE).unwrap();
        let at = |x: f32| PvsSphere::from_camera([x, 0.0, 5_000.0], 10_000.0, false);
        p.process(a, at(5_000.0), false).unwrap();
        p.process(b, at(5_000.0), false).unwrap();
        assert_eq!(p.cell_count(), 9);
        let d = p.process(a, at(105_000.0), false).unwrap().unwrap();
        assert!(d.deleted.is_empty());
        assert_eq!(d.removed.len(), 9);
        let d = p.process(b, at(105_000.0), false).unwrap().unwrap();
        assert_eq!(d.deleted.len(), 9);
        assert_eq!(p.cell_count(), 9);
    }

    #[test]
    fn disabled_prefetcher_does_nothing() {
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        p.enable(false);
        let s = PvsSphere::from_camera([0.0; 3], 10_000.0, false);
        assert_eq!(p.process(db, s, false).unwrap(), None);
        assert!(p.valid_cells(db).is_empty());
    }

    #[test]
    fn entities_move_between_cells() {
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        let s = PvsSphere::from_camera([5_000.0, 0.0, 5_000.0], 10_000.0, false);
        p.process(db, s, false).unwrap();
        let e = EntityId(7);
        assert!(p.add_entity(db, e, [5_000.0, 0.0, 5_000.0], 10));
        assert_eq!(p.entities_in_valid_cells(db), vec![e]);
        p.recalibrate(db, e, [5_000.0, 0.0, 5_000.0], [15_000.0, 0.0, 5_000.0]);
        let moved = PvsCell::of(15_000.0, 5_000.0, MAIN_CELL_SIZE);
        assert!(p.cell(db, moved).unwrap().entities.contains(&e));
        // Cached out when the cell leaves the rectangle.
        let far = PvsSphere::from_camera([500_000.0, 0.0, 5_000.0], 10_000.0, false);
        p.process(db, far, false).unwrap();
        assert!(p.entities_in_valid_cells(db).is_empty());
    }

    #[test]
    fn cell_corner_roundtrip() {
        let c = PvsCell::of(-1_234.0, 25_000.0, MAIN_CELL_SIZE);
        let [x, z] = c.min_corner(MAIN_CELL_SIZE);
        assert!(x <= -1_234.0 && x + 10_000.0 > -1_234.0);
        assert!(z <= 25_000.0 && z + 10_000.0 > 25_000.0);
    }
}
