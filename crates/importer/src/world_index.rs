//! Spatial index over every placed mesh entity of the world documents.
//! Positions are source units (matrices are preserved untouched); queries use
//! the world-matrix translation (X, Y, Z) as the entity origin. The grid is
//! horizontal (X/Z); height is only applied as a distance filter.
use anyhow::Result;
use gothic_formats::{vfs::VirtualFileSystem, world_node::WorldDocument};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

/// Grid cell edge in source units (the SysDyn cells are 10 000 wide).
const CELL: f32 = 2_500.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldEntityRecord {
    /// Hex of the raw 20-byte source ID.
    pub entity_id: String,
    pub name: String,
    /// Lowercase archive path of the world document.
    pub document: Arc<str>,
    /// Mesh resource name as serialized (basename, usually).
    pub resource: Arc<str>,
    /// Serialized X/Y/Z basis vectors followed by translation (row-vector layout).
    pub world_matrix: [[f32; 4]; 4],
}

impl WorldEntityRecord {
    pub fn position(&self) -> [f32; 3] {
        let t = self.world_matrix[3];
        [t[0], t[1], t[2]]
    }
}

pub fn hex_id(id: &[u8; 20]) -> String {
    use std::fmt::Write;
    id.iter().fold(String::with_capacity(40), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[derive(Default)]
pub struct WorldIndex {
    records: Vec<WorldEntityRecord>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    interned: HashMap<String, Arc<str>>,
    pub documents: usize,
    pub failed_documents: Vec<(String, String)>,
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    // `as` saturates for out-of-range floats; positions are validated finite.
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

impl WorldIndex {
    pub fn new() -> Self {
        Self::default()
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(v) = self.interned.get(s) {
            return v.clone();
        }
        let v: Arc<str> = Arc::from(s);
        self.interned.insert(s.to_owned(), v.clone());
        v
    }

    /// Add all mesh-bearing entities of a parsed document.
    pub fn add_document(&mut self, document: &str, doc: &WorldDocument) {
        self.documents += 1;
        let document = self.intern(document);
        for entity in &doc.entities {
            for asset in &entity.asset_references {
                let lower = asset.to_ascii_lowercase();
                if !(lower.ends_with(".xcmsh") || lower.ends_with(".xlmsh")) {
                    continue;
                }
                let resource = self.intern(asset);
                self.push(WorldEntityRecord {
                    entity_id: hex_id(&entity.source_id),
                    name: entity.name.clone(),
                    document: document.clone(),
                    resource,
                    world_matrix: entity.world_matrix,
                });
            }
        }
    }

    pub fn push(&mut self, record: WorldEntityRecord) {
        let id = u32::try_from(self.records.len()).expect("fewer than 2^32 world records");
        let p = record.position();
        self.cells.entry(cell_of(p[0], p[2])).or_default().push(id);
        self.records.push(record);
    }

    /// Parse every world document in the VFS (documents that fail are listed).
    pub fn build(vfs: &mut VirtualFileSystem) -> Result<Self> {
        let mut docs: Vec<_> = vfs
            .iter()
            .filter(|(p, _)| crate::survey::is_world_document(p))
            .map(|(p, s)| (p.to_owned(), s))
            .collect();
        docs.sort();
        let mut index = Self::new();
        for (path, slot) in docs {
            match vfs.read(slot).and_then(|b| WorldDocument::parse(&b)) {
                Ok(doc) => index.add_document(&path, &doc),
                Err(e) => index.failed_documents.push((path, format!("{e:#}"))),
            }
        }
        Ok(index)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn records(&self) -> &[WorldEntityRecord] {
        &self.records
    }

    /// Entities whose origin lies within `radius` (3D, source units) of `center`.
    /// Results are ordered by insertion order (deterministic). Non-finite or
    /// negative inputs yield no results.
    pub fn query(&self, center: [f32; 3], radius: f32) -> Vec<&WorldEntityRecord> {
        self.query_with(center, radius, false)
    }

    /// Same as [`query`](Self::query) but ignores height (Y); suited to streaming.
    pub fn query_xz(&self, center: [f32; 3], radius: f32) -> Vec<&WorldEntityRecord> {
        self.query_with(center, radius, true)
    }

    fn query_with(&self, center: [f32; 3], radius: f32, flat: bool) -> Vec<&WorldEntityRecord> {
        if !radius.is_finite() || radius < 0.0 || !center.iter().all(|v| v.is_finite()) {
            return Vec::new();
        }
        let lo = cell_of(center[0] - radius, center[2] - radius);
        let hi = cell_of(center[0] + radius, center[2] + radius);
        let width = (i64::from(hi.0) - i64::from(lo.0) + 1).max(0);
        let depth = (i64::from(hi.1) - i64::from(lo.1) + 1).max(0);
        let mut ids: Vec<u32> = Vec::new();
        if width.saturating_mul(depth) > self.cells.len() as i64 {
            // Large radius: scanning occupied cells is cheaper than the box.
            for (&(x, z), list) in &self.cells {
                if (lo.0..=hi.0).contains(&x) && (lo.1..=hi.1).contains(&z) {
                    ids.extend(list);
                }
            }
        } else {
            for x in lo.0..=hi.0 {
                for z in lo.1..=hi.1 {
                    if let Some(list) = self.cells.get(&(x, z)) {
                        ids.extend(list);
                    }
                }
            }
        }
        ids.sort_unstable();
        let r2 = radius * radius;
        ids.into_iter()
            .map(|i| &self.records[i as usize])
            .filter(|r| {
                let p = r.position();
                let dy = if flat { 0.0 } else { p[1] - center[1] };
                let (dx, dz) = (p[0] - center[0], p[2] - center[2]);
                dx * dx + dy * dy + dz * dz <= r2
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, x: f32, y: f32, z: f32) -> WorldEntityRecord {
        let mut m = [[0.0; 4]; 4];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        m[2][2] = 1.0;
        m[3] = [x, y, z, 1.0];
        WorldEntityRecord {
            entity_id: id.into(),
            name: String::new(),
            document: Arc::from("doc.node"),
            resource: Arc::from("a.xcmsh"),
            world_matrix: m,
        }
    }

    #[test]
    fn radius_queries_match_brute_force() {
        let mut index = WorldIndex::new();
        let mut seed = 12345u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        for i in 0..2000 {
            let (x, y, z) = (
                next() * 40_000.0 - 20_000.0,
                next() * 6_000.0,
                next() * 40_000.0 - 20_000.0,
            );
            index.push(rec(&i.to_string(), x, y, z));
        }
        for (c, r, flat) in [
            ([0.0, 1000.0, 0.0], 3000.0, false),
            ([-15_000.0, 0.0, 12_000.0], 9000.0, true),
            ([0.0, 0.0, 0.0], 1.0e9, false),
            ([5.0, 5.0, 5.0], 0.0, false),
        ] {
            let found = if flat {
                index.query_xz(c, r)
            } else {
                index.query(c, r)
            };
            let got: Vec<_> = found.iter().map(|r| r.entity_id.clone()).collect();
            let want: Vec<_> = index
                .records()
                .iter()
                .filter(|q| {
                    let p = q.position();
                    let dy = if flat { 0.0 } else { p[1] - c[1] };
                    (p[0] - c[0]).powi(2) + dy * dy + (p[2] - c[2]).powi(2) <= r * r
                })
                .map(|q| q.entity_id.clone())
                .collect();
            assert_eq!(got, want, "center {c:?} radius {r}");
        }
    }

    #[test]
    fn documents_contribute_only_mesh_resources_with_matrices() {
        use gothic_formats::world_node::{WorldDocument, WorldEntity, WorldKind};
        let entity = |id: u8, x: f32, assets: &[&str]| WorldEntity {
            source_id: [id; 20],
            source_offset: 0,
            name: format!("e{id}"),
            world_matrix: rec("", x, 0.0, 0.0).world_matrix,
            local_matrix: [[0.0; 4]; 4],
            asset_references: assets.iter().map(ToString::to_string).collect(),
            property_sets: Vec::new(),
        };
        let doc = WorldDocument {
            kind: WorldKind::Node,
            entities: vec![
                entity(1, 10.0, &[]),
                entity(2, 20.0, &["Tree.XCMSH", "Lod.xlmsh", "fx.fxa", "t.ximg"]),
            ],
            edges: Vec::new(),
        };
        let mut index = WorldIndex::new();
        index.add_document("a/doc_cstat.node", &doc);
        assert_eq!(index.documents, 1);
        assert_eq!(index.len(), 2);
        let hits = index.query([20.0, 0.0, 0.0], 1.0);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entity_id, "02".repeat(20));
        assert_eq!(&*hits[0].document, "a/doc_cstat.node");
        assert_eq!(&*hits[0].resource, "Tree.XCMSH");
        assert_eq!(hits[0].position(), [20.0, 0.0, 0.0]);
        // Resource strings are interned across records.
        assert!(Arc::ptr_eq(&hits[0].document, &hits[1].document));
    }

    #[test]
    fn rejects_nonfinite_queries_and_roundtrips_json() {
        let mut index = WorldIndex::new();
        index.push(rec("a", 1.0, 2.0, 3.0));
        assert!(index.query([f32::NAN, 0.0, 0.0], 10.0).is_empty());
        assert!(index.query([0.0; 3], f32::INFINITY).is_empty());
        assert!(index.query([0.0; 3], -1.0).is_empty());
        let json = serde_json::to_string(&index.records()[0]).unwrap();
        let back: WorldEntityRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, index.records()[0]);
        assert_eq!(hex_id(&[0xab; 20]).len(), 40);
    }
}
