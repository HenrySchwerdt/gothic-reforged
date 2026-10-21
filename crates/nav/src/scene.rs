//! Authored navigation data of the world documents: `gCNavZone_PS` polygons, `gCNavPath_PS`
//! corridors, `gCNavOffset_PS` approach poses and the `gCNavigation_PS` state of characters.
//!
//! The original compiles these into `NavigationMap.xnav` (`gCNavigationMap::BuildLists`,
//! `WriteLists`); this port reads the authored property sets directly and builds the graph at
//! load time ([`crate::graph::NavGraph`]). Records stay in the document's own (Gothic) space:
//! local points plus the entity's world matrix (row-vector convention, translation in row 3).
use anyhow::Result;
use glam::Vec3;
use gothic_formats::propstream::{DecodeStats, PropValue, PropertySet};
use gothic_formats::world_node::WorldDocument;
use std::collections::HashMap;

/// 20-byte persistent entity id (`bCPropertyID`, trailing four bytes zero).
pub type EntityId = [u8; 20];

/// Decode the lowercase-hex form used by id properties.
pub fn parse_id(hex: &str) -> Option<EntityId> {
    if hex.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Row-vector 4x4 transform of an entity (`world_matrix` of the document).
pub type Matrix = [[f32; 4]; 4];

/// Transform a local point by an entity matrix.
pub fn transform(m: &Matrix, p: [f32; 3]) -> Vec3 {
    Vec3::new(
        p[0] * m[0][0] + p[1] * m[1][0] + p[2] * m[2][0] + m[3][0],
        p[0] * m[0][1] + p[1] * m[1][1] + p[2] * m[2][1] + m[3][1],
        p[0] * m[0][2] + p[1] * m[1][2] + p[2] * m[2][2] + m[3][2],
    )
}

/// `gCNavZone_PS`: a walkable polygon with a vertical tolerance band.
#[derive(Debug, Clone)]
pub struct ZoneRecord {
    pub id: EntityId,
    pub name: String,
    pub matrix: Matrix,
    pub points: Vec<[f32; 3]>,
    pub radius: f32,
    pub ccw: bool,
    pub top_tolerance: f32,
    pub bottom_tolerance: f32,
}

/// `gCNavPath_PS`: a corridor (polyline with a radius per point) between two zones.
#[derive(Debug, Clone)]
pub struct PathRecord {
    pub id: EntityId,
    pub name: String,
    pub matrix: Matrix,
    pub points: Vec<[f32; 3]>,
    pub radii: Vec<f32>,
    pub unlimited_height: bool,
    pub zone_a: Option<EntityId>,
    pub zone_b: Option<EntityId>,
    /// Where the corridor leaves zone A / enters zone B (local space).
    pub a_center: [f32; 3],
    pub b_center: [f32; 3],
    pub top_tolerance: f32,
    pub bottom_tolerance: f32,
}

/// `gCNavOffset_PS`: approach poses of an interaction object (position + rotation in the
/// object's local space) with the animation direction of each.
#[derive(Debug, Clone)]
pub struct OffsetRecord {
    pub id: EntityId,
    pub name: String,
    pub matrix: Matrix,
    pub circle: bool,
    /// Position xyz followed by the rotation quaternion xyzw.
    pub poses: Vec<[f32; 7]>,
    pub directions: Vec<i32>,
}

/// `gCNavigation_PS` of a character: routine points and the last position known to be
/// navigable.
#[derive(Debug, Clone, Default)]
pub struct NavigationRecord {
    pub id: EntityId,
    pub name: String,
    pub matrix: Matrix,
    pub working_points: Vec<EntityId>,
    pub relaxing_points: Vec<EntityId>,
    pub sleeping_points: Vec<EntityId>,
    pub last_position: Option<[f32; 3]>,
    pub last_zone: Option<EntityId>,
    pub last_zone_is_path: bool,
}

/// Name and placement of any entity seen while loading (routine points are plain entities).
#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub name: String,
    pub matrix: Matrix,
}

impl EntityInfo {
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.matrix[3][0], self.matrix[3][1], self.matrix[3][2])
    }
}

#[derive(Debug, Clone, Default)]
pub struct NavScene {
    pub zones: Vec<ZoneRecord>,
    pub paths: Vec<PathRecord>,
    pub offsets: Vec<OffsetRecord>,
    pub navigations: Vec<NavigationRecord>,
    /// Every entity of the documents, by id (only filled when requested).
    pub entities: HashMap<EntityId, EntityInfo>,
}

fn vec3_array(set: &PropertySet, name: &str) -> Vec<[f32; 3]> {
    match set.get(name) {
        Some(PropValue::Vec3Array(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn vec3(set: &PropertySet, name: &str) -> [f32; 3] {
    match set.get(name) {
        Some(PropValue::Vec3(v)) => *v,
        _ => [0.0; 3],
    }
}

fn id(set: &PropertySet, name: &str) -> Option<EntityId> {
    match set.get(name) {
        Some(PropValue::Id(h)) => parse_id(h),
        _ => None,
    }
}

fn ids(set: &PropertySet, name: &str) -> Vec<EntityId> {
    match set.get(name) {
        Some(PropValue::IdArray(v)) => v.iter().filter_map(|h| parse_id(h)).collect(),
        _ => Vec::new(),
    }
}

impl NavScene {
    /// Read the navigation property sets of one `.node` / `.lrentdat` document. With
    /// `keep_entities` every entity's name and matrix is remembered for id lookups.
    /// Returns the number of records added.
    pub fn add_document(&mut self, bytes: &[u8], keep_entities: bool) -> Result<usize> {
        let mut stats = DecodeStats::default();
        let (doc, sets) = WorldDocument::parse_with_properties(
            bytes,
            &|c| {
                matches!(
                    c,
                    "gCNavZone_PS" | "gCNavPath_PS" | "gCNavOffset_PS" | "gCNavigation_PS"
                )
            },
            &mut stats,
        )?;
        let mut added = 0;
        for (i, entity) in doc.entities.iter().enumerate() {
            if keep_entities {
                self.entities.insert(
                    entity.source_id,
                    EntityInfo {
                        name: entity.name.clone(),
                        matrix: entity.world_matrix,
                    },
                );
            }
            for set in sets.get(i).into_iter().flatten() {
                let (idv, name, m) = (entity.source_id, entity.name.clone(), entity.world_matrix);
                match set.class.as_str() {
                    "gCNavZone_PS" => self.zones.push(ZoneRecord {
                        id: idv,
                        name,
                        matrix: m,
                        points: vec3_array(set, "Point"),
                        radius: set.float("Radius").unwrap_or(0.0),
                        ccw: set.boolean("ZoneIsCCW").unwrap_or(true),
                        top_tolerance: set.float("TopToleranz").unwrap_or(150.0),
                        bottom_tolerance: set.float("BottomToleranz").unwrap_or(100.0),
                    }),
                    "gCNavPath_PS" => self.paths.push(PathRecord {
                        id: idv,
                        name,
                        matrix: m,
                        points: vec3_array(set, "Point"),
                        radii: match set.get("Radius") {
                            Some(PropValue::F32Array(v)) => v.clone(),
                            _ => Vec::new(),
                        },
                        unlimited_height: set.boolean("UnlimitedHeight").unwrap_or(false),
                        zone_a: id(set, "ZoneAEntityID"),
                        zone_b: id(set, "ZoneBEntityID"),
                        a_center: vec3(set, "ZoneAIntersectionCenter"),
                        b_center: vec3(set, "ZoneBIntersectionCenter"),
                        top_tolerance: set.float("TopToleranz").unwrap_or(150.0),
                        bottom_tolerance: set.float("BottomToleranz").unwrap_or(100.0),
                    }),
                    "gCNavOffset_PS" => self.offsets.push(OffsetRecord {
                        id: idv,
                        name,
                        matrix: m,
                        circle: set.boolean("OffsetCircle").unwrap_or(false),
                        poses: match set.get("OffsetPose") {
                            Some(PropValue::MotionArray(v)) => v.clone(),
                            _ => Vec::new(),
                        },
                        directions: match set.get("AniDirection") {
                            Some(PropValue::EnumArray { values, .. }) => values.clone(),
                            _ => Vec::new(),
                        },
                    }),
                    "gCNavigation_PS" => self.navigations.push(NavigationRecord {
                        id: idv,
                        name,
                        matrix: m,
                        working_points: ids(set, "WorkingPoints"),
                        relaxing_points: ids(set, "RelaxingPoints"),
                        sleeping_points: ids(set, "SleepingPoints"),
                        last_position: match set.get("LastUseableNavigationPosition") {
                            Some(PropValue::Vec3(v)) if *v != [0.0; 3] => Some(*v),
                            _ => None,
                        },
                        last_zone: id(set, "LastUseableNavigationZoneID"),
                        last_zone_is_path: set
                            .boolean("LastUseableNavigationZoneIsPath")
                            .unwrap_or(false),
                    }),
                    _ => continue,
                }
                added += 1;
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let h = "18aa6240cc37c5479a6d8d94d7f9e6f800000000";
        let id = parse_id(h).unwrap();
        assert_eq!(id[0], 0x18);
        assert_eq!(id[15], 0xf8);
        assert_eq!(id[16..], [0; 4]);
        assert!(parse_id("zz").is_none());
    }

    #[test]
    fn matrix_uses_row_translation() {
        let mut m: Matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
        ];
        assert_eq!(transform(&m, [1.0, 2.0, 3.0]), Vec3::new(11.0, 22.0, 33.0));
        // Rotation by 90 degrees about Y: local +x goes to world -z in the row convention.
        m[0] = [0.0, 0.0, -1.0, 0.0];
        m[2] = [1.0, 0.0, 0.0, 0.0];
        assert_eq!(transform(&m, [1.0, 0.0, 0.0]), Vec3::new(10.0, 20.0, 29.0));
    }
}
