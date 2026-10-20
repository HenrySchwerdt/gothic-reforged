//! Bounded subset of observed GENOMFLE node/lrentdat documents.
//! Source basis vectors and units are preserved; opaque class tails are skipped.
use crate::genome::GenomeFile;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_ENTITIES: usize = 4_000_000;
const MAX_PROPERTIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WorldKind {
    Node,
    Lrentdat,
}

#[derive(Debug, Serialize)]
pub struct WorldDocument {
    pub kind: WorldKind,
    pub entities: Vec<WorldEntity>,
    pub edges: Vec<(u32, u32)>,
}

#[derive(Debug, Serialize)]
pub struct WorldEntity {
    /// Raw 20-byte source property ID, without UUID reinterpretation.
    pub source_id: [u8; 20],
    /// Offset in the decoded document, useful for independent evidence checks.
    pub source_offset: usize,
    /// `eCEntity` Enabled flag of the header (a disabled entity is not instantiated).
    pub enabled: bool,
    /// `eCEntity` ProcessingDisabled flag of the header.
    pub processing_disabled: bool,
    /// Bytes mapped one-to-one to Unicode U+0000..U+00FF; encoding is unspecified.
    pub name: String,
    /// Serialized X/Y/Z basis vectors followed by the translation vector.
    pub world_matrix: [[f32; 4]; 4],
    pub local_matrix: [[f32; 4]; 4],
    pub asset_references: Vec<String>,
    pub property_sets: Vec<String>,
}

/// Byte range of one entity property set inside the document body.
#[derive(Debug, Clone)]
pub struct PropertySetSpan {
    pub class: String,
    pub wrapper_version: u16,
    /// Offset of the object-version `u16` (start of the decodable property object).
    pub start: usize,
    pub end: usize,
}

impl WorldDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Ok(Self::parse_inner(bytes)?.0)
    }

    /// Parse and additionally decode the property sets whose class passes `filter` into typed
    /// [`crate::propstream::PropertySet`]s. The result is a side table: `sets[i]` belongs to
    /// `entities[i]`. A property set that fails to decode is skipped and counted in
    /// `stats.tail_errors`.
    pub fn parse_with_properties(
        bytes: &[u8],
        filter: &dyn Fn(&str) -> bool,
        stats: &mut crate::propstream::DecodeStats,
    ) -> Result<(Self, Vec<Vec<crate::propstream::PropertySet>>)> {
        use crate::propstream::{Cursor, Strings, read_object_body};
        let (doc, spans) = Self::parse_inner(bytes)?;
        if spans.is_empty() {
            return Ok((doc, Vec::new()));
        }
        let genome = GenomeFile::parse(bytes)?;
        let strings = Strings::new(&genome.strings);
        let mut sets = Vec::with_capacity(spans.len());
        for entity in spans {
            let mut decoded = Vec::new();
            for span in entity.into_iter().filter(|s| filter(&s.class)) {
                let result = Cursor::with_range(bytes, span.start, span.end).and_then(|mut cur| {
                    read_object_body(
                        &mut cur,
                        &strings,
                        &span.class,
                        Some(span.wrapper_version),
                        0,
                        stats,
                    )
                });
                match result {
                    Ok(set) => decoded.push(set),
                    Err(e) => {
                        let key = format!("world {}: {e:#}", span.class);
                        *stats.tail_errors.entry(key).or_default() += 1;
                    }
                }
            }
            sets.push(decoded);
        }
        Ok((doc, sets))
    }

    /// Parse and return the byte ranges of every entity's property sets (entity order), so
    /// callers can decode class tails this module skips.
    pub fn parse_with_spans(bytes: &[u8]) -> Result<(Self, Vec<Vec<PropertySetSpan>>)> {
        Self::parse_inner(bytes)
    }

    fn parse_inner(bytes: &[u8]) -> Result<(Self, Vec<Vec<PropertySetSpan>>)> {
        // Observed empty placeholder: raw "GENOMEDL" + u16 version, nothing else
        // (ten bytes, e.g. GUID-named .lrentdat stubs). Represents zero entities.
        if bytes.len() == 10 && bytes.starts_with(b"GENOMEDL") {
            return Ok((
                Self {
                    kind: WorldKind::Lrentdat,
                    entities: Vec::new(),
                    edges: Vec::new(),
                },
                Vec::new(),
            ));
        }
        let genome = GenomeFile::parse(bytes)?;
        let kind = if genome.body.starts_with(b"GENOMEDL") {
            WorldKind::Lrentdat
        } else {
            WorldKind::Node
        };
        let mut reader = Reader {
            bytes,
            strings: genome.strings,
            pos: 14,
            limit: genome.table_offset,
            // Only retained strings are charged (names, classes, resources).
            text_budget: 64 * 1024 * 1024 + bytes.len().saturating_mul(2),
            spans: Vec::new(),
        };
        if kind == WorldKind::Lrentdat {
            reader.take(8)?;
        }
        ensure!(reader.u16()? == 83, "unsupported world document version");
        if kind == WorldKind::Lrentdat {
            let context = reader.class(false, true)?;
            ensure!(
                context.name == "eCEntityDynamicContext",
                "unexpected lrentdat context"
            );
        }
        let count = reader.u32()? as usize;
        ensure!(
            count > 0 && count <= MAX_ENTITIES && count <= reader.remaining() / 298,
            "world entity count exceeds bounds"
        );
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            // Duplicate source IDs occur in shipped data (one CStat cell);
            // identity is therefore (document, index), not the ID alone.
            entities.push(reader.entity(kind)?);
        }
        let mut edges = Vec::new();
        let mut parents = vec![None; count];
        loop {
            let parent = reader.u32()?;
            let child = reader.u32()?;
            if parent == u32::MAX {
                ensure!(child == u32::MAX, "invalid graph terminator");
                break;
            }
            ensure!(
                (parent as usize) < count && (child as usize) < count && parent != child,
                "invalid entity graph index"
            );
            ensure!(
                parents[child as usize].replace(parent).is_none(),
                "entity has multiple parents"
            );
            edges.push((parent, child));
            ensure!(edges.len() < count, "entity graph has too many edges");
        }
        ensure!(
            reader.remaining() == 0,
            "unexpected world document body tail"
        );
        // Linear-time parent-chain validation; supports forests without inventing a root.
        let mut marks = vec![0u8; count];
        for start in 0..count {
            if marks[start] != 0 {
                continue;
            }
            let mut cursor = Some(start);
            let mut chain = Vec::new();
            while let Some(index) = cursor {
                ensure!(marks[index] != 1, "cycle in entity graph");
                if marks[index] == 2 {
                    break;
                }
                marks[index] = 1;
                chain.push(index);
                cursor = parents[index].map(|parent| parent as usize);
            }
            for index in chain {
                marks[index] = 2;
            }
        }
        Ok((
            Self {
                kind,
                entities,
                edges,
            },
            std::mem::take(&mut reader.spans),
        ))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    strings: Vec<&'a [u8]>,
    pos: usize,
    limit: usize,
    // Caps expansion from repeatedly referenced large string-table entries.
    text_budget: usize,
    /// Property-set spans per entity, in entity order.
    spans: Vec<Vec<PropertySetSpan>>,
}

struct ClassSummary {
    name: String,
    assets: Vec<String>,
    wrapper_version: u16,
    start: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.limit - self.pos
    }
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let end = self
            .pos
            .checked_add(count)
            .context("world field range overflow")?;
        ensure!(end <= self.limit, "truncated world field at {}", self.pos);
        let bytes = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn flag(&mut self) -> Result<bool> {
        let value = self.take(1)?[0];
        ensure!(value <= 1, "invalid world boolean");
        Ok(value == 1)
    }
    /// Borrowed string-table entry; not charged against the text budget.
    fn text_bytes(&mut self) -> Result<&'a [u8]> {
        let index = self.u16()? as usize;
        self.strings
            .get(index)
            .copied()
            .context("world string index out of bounds")
    }
    fn text(&mut self) -> Result<String> {
        let bytes = self.text_bytes()?;
        let cost = bytes
            .len()
            .checked_mul(2)
            .context("world text size overflow")?;
        self.text_budget = self
            .text_budget
            .checked_sub(cost)
            .context("world text allocation budget exceeded")?;
        Ok(bytes.iter().copied().map(char::from).collect())
    }
    fn matrix(&mut self) -> Result<[[f32; 4]; 4]> {
        let mut matrix = [[0.0; 4]; 4];
        for vector in &mut matrix {
            for value in vector {
                *value = f32::from_le_bytes(self.take(4)?.try_into()?);
                ensure!(value.is_finite(), "non-finite world matrix");
            }
        }
        Ok(matrix)
    }
    fn class(&mut self, wrapped: bool, context: bool) -> Result<ClassSummary> {
        let wrapper_version = if wrapped { self.u16()? } else { 0 };
        ensure!(
            self.take(6)? == [1, 0, 1, 1, 0, 1],
            "unsupported class accessor"
        );
        let name = self.text()?;
        self.take(5)?; // Factory metadata is not interpreted.
        let start = self.pos;
        ensure!(self.u16()? == 83, "unsupported property object version");
        let size = self.u32()? as usize;
        let end = self.pos.checked_add(size).context("class size overflow")?;
        ensure!(end <= self.limit, "class exceeds world document body");
        let previous_limit = self.limit;
        self.limit = end;
        self.u16()?; // Property-list version.
        let count = self.u32()? as usize;
        ensure!(
            count <= MAX_PROPERTIES && count <= self.remaining() / 10,
            "property count exceeds bounds"
        );
        let mut assets = BTreeSet::new();
        for _ in 0..count {
            let property = self.text_bytes()?;
            let property_type = self.text_bytes()?;
            self.u16()?;
            let length = self.u32()? as usize;
            if matches!(property, b"ResourceFileName" | b"ResourceFilePath")
                && matches!(
                    property_type,
                    b"bCMeshResourceString"
                        | b"bCAnimationResourceString"
                        | b"bCImageResourceString"
                        | b"bCString"
                )
            {
                ensure!(length == 2, "unsupported resource reference payload");
                let asset = self.text()?;
                if !asset.is_empty() {
                    assets.insert(asset);
                }
            } else {
                self.take(length)?;
            }
        }
        if context {
            ensure!(
                !wrapped && name == "eCEntityDynamicContext",
                "unsupported world context"
            );
            self.u16()?;
            self.take(33)?;
        } else {
            // Opaque property-set tails are not decoded as gameplay or geometry.
            self.pos = end;
        }
        self.limit = previous_limit;
        if wrapped {
            ensure!(self.u32()? == 0xdeadc0de, "invalid property-set terminator");
        }
        Ok(ClassSummary {
            name,
            assets: assets.into_iter().collect(),
            wrapper_version,
            start,
            end,
        })
    }
    fn entity(&mut self, kind: WorldKind) -> Result<WorldEntity> {
        let source_offset = self.pos;
        let mut trailing_creator = false;
        match kind {
            WorldKind::Node => {
                let has_creator = self.flag()?;
                let disable_patch = self.flag()?;
                trailing_creator = has_creator && !disable_patch;
                ensure!(self.u16()? == 0x23, "unsupported spatial entity version");
                if self.flag()? {
                    self.take(20)?;
                }
                self.take(84)?;
            }
            WorldKind::Lrentdat => {
                ensure!(
                    self.u16()? == 0x40 && self.u16()? == 0x53,
                    "unsupported dynamic entity version"
                );
                if self.flag()? {
                    self.take(20)?;
                }
            }
        }
        ensure!(
            self.u16()? == 0x53 && self.u16()? == 1,
            "unsupported common entity version"
        );
        let source_id = self.take(20)?.try_into()?;
        // eCEntity header flags: Enabled, RenderingEnabled, ProcessingDisabled,
        // DeactivationEnabled, (unknown), Pickable, CollisionEnabled.
        let mut header_flags = [false; 7];
        for f in &mut header_flags {
            *f = self.flag()?;
        }
        self.take(7)?; // alpha f32, insertion u16, render priority u8.
        for _ in 0..3 {
            self.flag()?;
        }
        let name = self.text()?;
        let world_matrix = self.matrix()?;
        let local_matrix = self.matrix()?;
        self.take(3 * 24 + 2 * 16 + 4)?;
        self.flag()?;
        self.take(12)?; // culling f32, timestamp u32, scaling f32.
        self.flag()?;
        self.flag()?;
        let count = self.u32()? as usize;
        ensure!(
            count <= MAX_PROPERTIES && count <= self.remaining() / 21,
            "property-set count exceeds bounds"
        );
        let mut property_sets = Vec::with_capacity(count);
        let mut assets = BTreeSet::new();
        let mut spans = Vec::with_capacity(count);
        for _ in 0..count {
            let class = self.class(true, false)?;
            spans.push(PropertySetSpan {
                class: class.name.clone(),
                wrapper_version: class.wrapper_version,
                start: class.start,
                end: class.end,
            });
            property_sets.push(class.name);
            assets.extend(class.assets);
        }
        self.spans.push(spans);
        if trailing_creator {
            self.take(20)?;
        }
        Ok(WorldEntity {
            source_id,
            source_offset,
            enabled: header_flags[0],
            processing_disabled: header_flags[2],
            name,
            world_matrix,
            local_matrix,
            asset_references: assets.into_iter().collect(),
            property_sets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope(body: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut bytes = b"GENOMFLE\x01\x00".to_vec();
        bytes.extend((14u32 + body.len() as u32).to_le_bytes());
        bytes.extend(body);
        bytes.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        bytes.extend((strings.len() as u32).to_le_bytes());
        for string in strings {
            bytes.extend((string.len() as u16).to_le_bytes());
            bytes.extend(string.as_bytes());
        }
        bytes
    }
    fn matrix(x: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [
            1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, 2.0, -3.0, 1.0,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }
    fn class(name: u16, data: &[u8], wrapped: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if wrapped {
            bytes.extend(1u16.to_le_bytes());
        }
        bytes.extend([1, 0, 1, 1, 0, 1]);
        bytes.extend(name.to_le_bytes());
        bytes.extend([1, 0, 0, 83, 0]);
        bytes.extend(83u16.to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        if wrapped {
            bytes.extend(0xdeadc0deu32.to_le_bytes());
        }
        bytes
    }
    fn entity(kind: WorldKind, id: u8, classes: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        if kind == WorldKind::Node {
            bytes.extend([0, 0, 0x23, 0, 0]);
            bytes.extend([0; 84]);
        } else {
            bytes.extend([0x40, 0, 0x53, 0, 0]);
        }
        bytes.extend([0x53, 0, 1, 0]);
        bytes.extend([id; 20]);
        bytes.extend([0; 7 + 7 + 3]);
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(matrix(10.0));
        bytes.extend(matrix(4.0));
        bytes.extend([0; 3 * 24 + 2 * 16 + 4 + 1 + 12 + 2]);
        bytes.extend((classes.len() as u32).to_le_bytes());
        for class in classes {
            bytes.extend(class);
        }
        bytes
    }
    fn node() -> Vec<u8> {
        let mut props = Vec::new();
        props.extend(30u16.to_le_bytes());
        props.extend(1u32.to_le_bytes());
        for value in [2u16, 3, 30] {
            props.extend(value.to_le_bytes());
        }
        props.extend(2u32.to_le_bytes());
        props.extend(4u16.to_le_bytes());
        props.extend([0xaa, 0xbb]); // opaque subclass tail.
        let mut body = 83u16.to_le_bytes().to_vec();
        body.extend(2u32.to_le_bytes());
        body.extend(entity(WorldKind::Node, 1, &[]));
        body.extend(entity(WorldKind::Node, 2, &[class(1, &props, true)]));
        for value in [0u32, 1, u32::MAX, u32::MAX] {
            body.extend(value.to_le_bytes());
        }
        envelope(
            &body,
            &[
                "entity",
                "eCVisualMeshStatic_PS",
                "ResourceFileName",
                "bCMeshResourceString",
                "fixture.xcmsh",
            ],
        )
    }
    #[test]
    fn preserves_world_and_local_basis_and_resource_relationships() -> Result<()> {
        let parsed = WorldDocument::parse(&node())?;
        assert_eq!(parsed.kind, WorldKind::Node);
        assert_eq!(parsed.edges, vec![(0, 1)]);
        assert_eq!(parsed.entities[1].world_matrix[3], [10.0, 2.0, -3.0, 1.0]);
        assert_eq!(parsed.entities[1].local_matrix[3], [4.0, 2.0, -3.0, 1.0]);
        assert_eq!(parsed.entities[1].source_id, [2; 20]);
        assert_eq!(parsed.entities[1].asset_references, ["fixture.xcmsh"]);
        Ok(())
    }
    #[test]
    fn rejects_truncation_invalid_graph_and_nonfinite_matrix() {
        let bytes = node();
        for n in 0..bytes.len() {
            assert!(WorldDocument::parse(&bytes[..n]).is_err(), "{n}");
        }
        let mut bad = bytes.clone();
        let table = u32::from_le_bytes(bad[10..14].try_into().unwrap()) as usize;
        bad[table - 12..table - 8].copy_from_slice(&99u32.to_le_bytes());
        assert!(WorldDocument::parse(&bad).is_err());
        let mut bad = bytes;
        bad[152..156].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(WorldDocument::parse(&bad).is_err());
    }
    #[test]
    fn empty_lrentdat_stub_and_duplicate_ids_are_accepted() -> Result<()> {
        let mut stub = b"GENOMEDL".to_vec();
        stub.extend(56u16.to_le_bytes());
        let doc = WorldDocument::parse(&stub)?;
        assert_eq!(doc.kind, WorldKind::Lrentdat);
        assert!(doc.entities.is_empty() && doc.edges.is_empty());
        // Other raw GENOMEDL payloads (legacy v56 layout) remain unsupported.
        stub.push(0);
        assert!(WorldDocument::parse(&stub).is_err());
        assert!(WorldDocument::parse(&stub[..9]).is_err());

        // One shipped CStat cell repeats a source ID: identity is the index.
        let mut body = 83u16.to_le_bytes().to_vec();
        body.extend(2u32.to_le_bytes());
        body.extend(entity(WorldKind::Node, 7, &[]));
        body.extend(entity(WorldKind::Node, 7, &[]));
        for value in [u32::MAX, u32::MAX] {
            body.extend(value.to_le_bytes());
        }
        let doc = WorldDocument::parse(&envelope(&body, &["entity"]))?;
        assert_eq!(doc.entities.len(), 2);
        assert_eq!(doc.entities[0].source_id, doc.entities[1].source_id);
        Ok(())
    }

    #[test]
    fn missing_string_table_is_reported_not_guessed() {
        // GENOMFLE header with table offset 0 (observed in eight GUID .lrentdat files).
        let mut bytes = b"GENOMFLE     ".to_vec();
        bytes.extend(b"GENOMEDL");
        bytes.extend(83u16.to_le_bytes());
        bytes.extend([0; 64]);
        let error = WorldDocument::parse(&bytes).unwrap_err();
        assert!(format!("{error:#}").contains("table offset"));
    }

    #[test]
    fn lrentdat_context_size_can_include_the_following_entities() -> Result<()> {
        let mut context = 30u16.to_le_bytes().to_vec();
        context.extend(0u32.to_le_bytes());
        context.extend(1u16.to_le_bytes());
        context.extend([0; 33]);
        let mut body = b"GENOMEDL".to_vec();
        body.extend(83u16.to_le_bytes());
        let context_start = body.len();
        body.extend(class(1, &context, false));
        body.extend(1u32.to_le_bytes());
        body.extend(entity(WorldKind::Lrentdat, 3, &[]));
        body.extend([0xff; 8]);
        let size_pos = context_start + 15;
        let spanning_size = body.len() - size_pos - 4;
        body[size_pos..size_pos + 4].copy_from_slice(&(spanning_size as u32).to_le_bytes());
        let parsed = WorldDocument::parse(&envelope(&body, &["entity", "eCEntityDynamicContext"]))?;
        assert_eq!(parsed.kind, WorldKind::Lrentdat);
        assert_eq!(parsed.entities.len(), 1);
        assert_eq!(parsed.entities[0].source_id, [3; 20]);
        Ok(())
    }

    #[test]
    fn property_side_table_decodes_typed_values_without_changing_entities() -> Result<()> {
        use crate::propstream::{DecodeStats, PropValue};
        let bytes = node();
        let plain = WorldDocument::parse(&bytes)?;
        let mut stats = DecodeStats::default();
        let (doc, sets) = WorldDocument::parse_with_properties(&bytes, &|_| true, &mut stats)?;
        assert_eq!(doc.entities.len(), plain.entities.len());
        assert_eq!(
            doc.entities[1].property_sets,
            plain.entities[1].property_sets
        );
        assert!(sets[0].is_empty());
        assert_eq!(sets[1].len(), 1);
        let set = &sets[1][0];
        assert_eq!(set.class, "eCVisualMeshStatic_PS");
        assert_eq!(set.wrapper_version, Some(1));
        assert_eq!(
            set.get("ResourceFileName"),
            Some(&PropValue::Str("fixture.xcmsh".into()))
        );
        assert_eq!((stats.properties, stats.typed), (1, 1));
        // a class filter skips decoding but keeps the per-entity alignment
        let (_, none) =
            WorldDocument::parse_with_properties(&bytes, &|c| c == "other", &mut stats)?;
        assert!(none.iter().all(Vec::is_empty) && none.len() == 2);
        // truncation behaves like plain parsing
        for n in 0..bytes.len() {
            assert!(
                WorldDocument::parse_with_properties(&bytes[..n], &|_| true, &mut stats).is_err()
            );
        }
        Ok(())
    }
}
