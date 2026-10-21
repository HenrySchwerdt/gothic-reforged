//! Bounded subset of observed GENOMFLE node/lrentdat documents.
//! Source basis vectors and units are preserved; opaque class tails are skipped.
use crate::genome::GenomeFile;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_ENTITIES: usize = 4_000_000;
/// Largest table-less layer that is still just a context (the shipped ones are 124 bytes).
const TABLELESS_EMPTY_MAX: usize = 256;

/// `GENOMFLE` envelope whose table offset is 0 (no string table at all).
fn is_tableless(bytes: &[u8]) -> bool {
    bytes.get(..8) == Some(b"GENOMFLE")
        && bytes.get(8..10) == Some(&[1, 0])
        && bytes.get(10..14) == Some(&[0, 0, 0, 0])
}
const MAX_PROPERTIES: usize = 10_000;

/// A `GENOMFLE` layer whose writer never finished: the header's table offset is still the
/// placeholder 0 and no string table follows. The engine cannot load such a file (see
/// [`WorldDocument::parse`] and `docs/research/WORLD_SURVEY.md`), so it is never referenced.
/// Returns true for those layers that carry content (the 124-byte context-only ones are
/// parsed as empty documents instead).
pub fn is_unfinalized_layer(bytes: &[u8]) -> bool {
    is_tableless(bytes) && bytes.len() >= TABLELESS_EMPTY_MAX
}

/// How strings are stored in a document (decided by its envelope).
#[derive(Clone, Copy, PartialEq, Eq)]
enum StringMode {
    /// `u16` index into a table (GENOMFLE footer, or the string list heading a raw v56 layer).
    Table,
    /// `u16` length + bytes in place (raw legacy nodes without an envelope).
    Inline,
}

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
            // Legacy layouts (inline strings / property-stream lists) have no typed side table.
            let empty = doc.entities.iter().map(|_| Vec::new()).collect();
            return Ok((doc, empty));
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

    /// The `{GUID}.lrentdat` layers whose `GENOMFLE` header still holds the placeholder table
    /// offset 0: the writer (`eCArchiveFile::Close` 0x102a7500 appends the string table and
    /// patches the offset) never finished, so every string of the body is an index into a table
    /// the file does not contain. A layer that holds only its context (no entity count follows,
    /// 124 bytes in the shipped data) is an empty document. Larger ones are unrecoverable and
    /// unreachable: `eCProcessibleElement::LoadData` 0x1005d2f0 treats offset 0 as a corrupt
    /// archive (the marker read at offset 0 is `GENO`, not 0xDEADBEEF, a fatal error) and no sector
    /// lists them (see `docs/research/WORLD_SURVEY.md`). They are reported, never guessed.
    fn parse_tableless(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() < TABLELESS_EMPTY_MAX && bytes.get(14..22) == Some(b"GENOMEDL"),
            "world layer has no string table (GENOMFLE table offset 0, {} bytes of indexed strings;              never finalized by its writer and never loaded by the engine)",
            bytes.len()
        );
        Ok(Self {
            kind: WorldKind::Lrentdat,
            entities: Vec::new(),
            edges: Vec::new(),
        })
    }

    /// `bCPOStream::ReadFile` (0x10057b50) header at the reader position: `u16` stream version 1,
    /// a flag byte, a `u32` string count with inline strings and a `u32` payload size that must
    /// match the rest of the file. Payload strings are `u16` indices into the returned list.
    fn property_stream<'a>(head: &mut Reader<'a>) -> Result<Vec<&'a [u8]>> {
        ensure!(head.u16()? == 1, "unsupported property-stream version");
        head.take(1)?;
        let count = head.u32()? as usize;
        ensure!(
            count <= 65536 && count <= head.remaining() / 2,
            "property-stream string count exceeds bounds"
        );
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            let len = head.u16()? as usize;
            ensure!(len <= 16384, "property-stream string exceeds ceiling");
            strings.push(head.take(len)?);
        }
        let size = head.u32()? as usize;
        ensure!(
            size == head.remaining(),
            "property-stream size does not match the file"
        );
        Ok(strings)
    }

    /// Raw `GENOMEDL` dynamic layer of the legacy generation (version <= 56), read through a
    /// property stream; entities use the pre-83 `eCEntity::ReadV63` layout.
    fn parse_raw_layer(bytes: &[u8]) -> Result<(Self, Vec<Vec<PropertySetSpan>>)> {
        let mut head = Reader::new(bytes, Vec::new(), StringMode::Inline, false, 8);
        let version = head.u16()?;
        ensure!(
            version <= 0x38,
            "unsupported raw GENOMEDL layer version {version}"
        );
        let strings = Self::property_stream(&mut head)?;
        let mut reader = Reader::new(bytes, strings, StringMode::Table, true, head.pos);
        reader.document(WorldKind::Lrentdat)
    }

    /// Oldest dynamic layer form: no magic, just the accessor-wrapped context with inline
    /// strings (`gCDynamicLayer::DoLoadData` 0x100c5a50, non-`GENOMEDL` branch).
    fn parse_raw_accessor_layer(bytes: &[u8]) -> Result<(Self, Vec<Vec<PropertySetSpan>>)> {
        let mut reader = Reader::new(bytes, Vec::new(), StringMode::Inline, true, 0);
        reader.document(WorldKind::Lrentdat)
    }

    /// Raw legacy spatial document (no envelope): `u16` version, `u32` entity count and entities
    /// in the `eCGeometrySpatialContext::ReadNodesOld` (0x1021b9c0, version 30..=55, inline
    /// strings) or `ReadNodesV56` (0x1021bc30, version 56..=82, property stream) layout.
    fn parse_raw_node(bytes: &[u8]) -> Result<(Self, Vec<Vec<PropertySetSpan>>)> {
        let mut head = Reader::new(bytes, Vec::new(), StringMode::Inline, true, 0);
        let version = head.u16()?;
        if (0x1e..0x38).contains(&version) {
            return head.document(WorldKind::Node);
        }
        ensure!(
            (0x38..0x53).contains(&version),
            "unsupported raw spatial document version {version}"
        );
        let strings = Self::property_stream(&mut head)?;
        let mut reader = Reader::new(bytes, strings, StringMode::Table, true, head.pos);
        reader.document(WorldKind::Node)
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
        if is_tableless(bytes) {
            return Self::parse_tableless(bytes).map(|doc| (doc, Vec::new()));
        }
        if bytes.starts_with(b"GENOMEDL") {
            return Self::parse_raw_layer(bytes);
        }
        if !bytes.starts_with(b"GENOMFLE") {
            if bytes.starts_with(&[1, 0, 1, 1, 0, 1]) {
                return Self::parse_raw_accessor_layer(bytes);
            }
            return Self::parse_raw_node(bytes);
        }
        let genome = GenomeFile::parse(bytes)?;
        let kind = if genome.body.starts_with(b"GENOMEDL") {
            WorldKind::Lrentdat
        } else {
            WorldKind::Node
        };
        let mut reader = Reader::new(bytes, genome.strings, StringMode::Table, false, 14);
        reader.limit = genome.table_offset;
        if kind == WorldKind::Lrentdat {
            reader.take(8)?;
        }
        ensure!(reader.u16()? == 83, "unsupported world document version");
        reader.document(kind)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    strings: Vec<&'a [u8]>,
    mode: StringMode,
    /// Pre-83 entity layout (`eCEntity::ReadV63`, `eCDynamicEntity` below version 83).
    legacy: bool,
    pos: usize,
    limit: usize,
    // Caps expansion from repeatedly referenced large string-table entries.
    text_budget: usize,
    /// Property-set spans per entity, in entity order (GENOMFLE layout only).
    spans: Vec<Vec<PropertySetSpan>>,
}

struct ClassSummary {
    name: String,
    assets: Vec<String>,
    wrapper_version: u16,
    start: usize,
    end: usize,
}

/// Everything after the entity-kind specific header (`eCEntity` and its property sets).
struct Common {
    source_id: [u8; 20],
    enabled: bool,
    processing_disabled: bool,
    name: String,
    world_matrix: [[f32; 4]; 4],
    local_matrix: [[f32; 4]; 4],
    assets: BTreeSet<String>,
    property_sets: Vec<String>,
    spans: Vec<PropertySetSpan>,
}

impl<'a> Reader<'a> {
    fn new(
        bytes: &'a [u8],
        strings: Vec<&'a [u8]>,
        mode: StringMode,
        legacy: bool,
        pos: usize,
    ) -> Self {
        Self {
            bytes,
            strings,
            mode,
            legacy,
            pos,
            limit: bytes.len(),
            // Only retained strings are charged (names, classes, resources).
            text_budget: 64 * 1024 * 1024 + bytes.len().saturating_mul(2),
            spans: Vec::new(),
        }
    }
    fn remaining(&self) -> usize {
        self.limit - self.pos
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .context("world field range overflow")?;
        ensure!(end <= self.limit, "truncated world field at {}", self.pos);
        let bytes: &'a [u8] = self.bytes;
        let slice = &bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
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
    /// Borrowed string (table entry or inline bytes); not charged against the text budget.
    fn text_bytes(&mut self) -> Result<&'a [u8]> {
        match self.mode {
            StringMode::Table => {
                let index = self.u16()? as usize;
                self.strings
                    .get(index)
                    .copied()
                    .context("world string index out of bounds")
            }
            StringMode::Inline => {
                let len = self.u16()? as usize;
                self.take(len)
            }
        }
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

    /// Context (lrentdat), entity count, entities and the parent/child graph, shared by every
    /// layout. The caller has consumed the envelope and the document version.
    fn document(&mut self, kind: WorldKind) -> Result<(WorldDocument, Vec<Vec<PropertySetSpan>>)> {
        if kind == WorldKind::Lrentdat {
            let context = self.class(false, true)?;
            ensure!(
                context.name == "eCEntityDynamicContext",
                "unexpected lrentdat context"
            );
        }
        let count = self.u32()? as usize;
        let min_entity = if self.legacy { 200 } else { 298 };
        ensure!(
            count > 0 && count <= MAX_ENTITIES && count <= self.remaining() / min_entity,
            "world entity count exceeds bounds"
        );
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            // Duplicate source IDs occur in shipped data (one CStat cell);
            // identity is therefore (document, index), not the ID alone.
            entities.push(self.entity(kind)?);
        }
        let mut edges = Vec::new();
        let mut parents = vec![None; count];
        loop {
            let parent = self.u32()?;
            let child = self.u32()?;
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
        ensure!(self.remaining() == 0, "unexpected world document body tail");
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
            WorldDocument {
                kind,
                entities,
                edges,
            },
            std::mem::take(&mut self.spans),
        ))
    }

    /// Property list of a property object: `u16` list version, `u32` count, then
    /// name, type, `u16` version, `u32` length and value per property. Returns the mesh/asset
    /// references among the values. The caller bounds `self.limit` to the object.
    fn property_list(&mut self) -> Result<BTreeSet<String>> {
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
                let end = self
                    .pos
                    .checked_add(length)
                    .context("asset length overflow")?;
                ensure!(end <= self.limit, "truncated asset reference");
                // Table strings fill exactly one index; inline strings are `u16 length + bytes`.
                let asset = self.text()?;
                ensure!(self.pos == end, "unsupported resource reference payload");
                if !asset.is_empty() {
                    assets.insert(asset);
                }
            } else {
                self.take(length)?;
            }
        }
        Ok(assets)
    }

    /// One accessor-wrapped property object: six accessor bytes, class name, five factory bytes,
    /// `u16` object version, `u32` size, then the object. Version 83 objects start with the
    /// property list; legacy (version 1) objects carry their class name and a 20-byte property id
    /// first. Returns the summary with `start` at the object-version `u16`.
    fn object(&mut self, wrapper_version: u16, context: bool) -> Result<ClassSummary> {
        ensure!(
            self.take(6)? == [1, 0, 1, 1, 0, 1],
            "unsupported class accessor"
        );
        let name = self.text()?;
        self.take(5)?; // Factory metadata is not interpreted.
        let start = self.pos;
        let object_version = self.u16()?;
        ensure!(
            object_version == 83 || (self.legacy && (1..=83).contains(&object_version)),
            "unsupported property object version"
        );
        let size = self.u32()? as usize;
        let end = self.pos.checked_add(size).context("class size overflow")?;
        ensure!(end <= self.limit, "class exceeds world document body");
        let previous_limit = self.limit;
        self.limit = end;
        // bCPropertyObjectBase::ReadData 0x10056ca0: version 1 stores a string, versions up to 81
        // a 20-byte property id; 82 and 83 store neither.
        if object_version == 1 {
            self.text_bytes()?;
        }
        if object_version <= 0x51 {
            self.take(20)?;
        }
        let assets = self.property_list()?;
        if context {
            ensure!(
                name == "eCEntityDynamicContext",
                "unsupported world context"
            );
            self.u16()?;
            self.take(33)?;
        } else {
            // Opaque property-set tails are not decoded as gameplay or geometry.
            self.pos = end;
        }
        self.limit = previous_limit;
        Ok(ClassSummary {
            name,
            assets: assets.into_iter().collect(),
            wrapper_version,
            start,
            end,
        })
    }

    fn class(&mut self, wrapped: bool, context: bool) -> Result<ClassSummary> {
        ensure!(!(context && wrapped), "unsupported world context");
        let wrapper_version = if wrapped { self.u16()? } else { 0 };
        let summary = self.object(wrapper_version, context)?;
        if wrapped {
            ensure!(self.u32()? == 0xdeadc0de, "invalid property-set terminator");
        }
        Ok(summary)
    }

    /// Pre-83 property set (`eCEntity::ReadV63`, entity version >= 42): `u16` set version,
    /// `u32` size of everything after the name, class name, the accessor object, terminator.
    fn legacy_set(&mut self) -> Result<ClassSummary> {
        let wrapper_version = self.u16()?;
        let size = self.u32()? as usize;
        let name = self.text()?;
        let end = self
            .pos
            .checked_add(size)
            .context("property-set size overflow")?;
        ensure!(
            end <= self.limit,
            "property set exceeds world document body"
        );
        let previous_limit = self.limit;
        self.limit = end;
        let inner = self.object(wrapper_version, false)?;
        ensure!(inner.name == name, "property-set class name mismatch");
        self.limit = previous_limit;
        self.pos = end;
        ensure!(self.u32()? == 0xdeadc0de, "invalid property-set terminator");
        Ok(inner)
    }

    fn entity(&mut self, kind: WorldKind) -> Result<WorldEntity> {
        let source_offset = self.pos;
        let mut trailing_creator = false;
        match kind {
            WorldKind::Node => {
                let has_creator = self.flag()?;
                let disable_patch = self.flag()?;
                trailing_creator = has_creator && !disable_patch;
                let version = self.u16()?;
                if self.legacy {
                    // eCSpatialEntity::Read 0x10228d50: 0x1e has neither template flag nor boxes.
                    ensure!(
                        (0x1e..=0x23).contains(&version),
                        "unsupported spatial entity version"
                    );
                    if version != 0x1e {
                        if self.flag()? {
                            self.take(20)?;
                        }
                        if version > 0x22 {
                            self.take(84)?;
                        }
                    }
                } else {
                    ensure!(version == 0x23, "unsupported spatial entity version");
                    if self.flag()? {
                        self.take(20)?;
                    }
                    self.take(84)?;
                }
            }
            WorldKind::Lrentdat => {
                // Derived-class version: 0x40 in v83 layers, other values in legacy ones.
                let derived = self.u16()?;
                ensure!(
                    derived == 0x40 || self.legacy,
                    "unsupported dynamic entity version"
                );
                // eCDynamicEntity::Read 0x102295c0: below version 83 an extra bool precedes.
                let version = self.u16()?;
                ensure!(
                    version == 0x53 || (self.legacy && version < 0x53),
                    "unsupported dynamic entity version"
                );
                if version < 0x53 {
                    self.flag()?;
                }
                if self.flag()? {
                    self.take(20)?;
                }
            }
        }
        let common = if self.legacy {
            self.common_v63()?
        } else {
            self.common_v83()?
        };
        if !self.legacy {
            self.spans.push(common.spans);
        }
        if trailing_creator {
            self.take(20)?;
        }
        Ok(WorldEntity {
            source_id: common.source_id,
            source_offset,
            enabled: common.enabled,
            processing_disabled: common.processing_disabled,
            name: common.name,
            world_matrix: common.world_matrix,
            local_matrix: common.local_matrix,
            asset_references: common.assets.into_iter().collect(),
            property_sets: common.property_sets,
        })
    }

    fn common_v83(&mut self) -> Result<Common> {
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
        Ok(Common {
            source_id,
            enabled: header_flags[0],
            processing_disabled: header_flags[2],
            name,
            world_matrix,
            local_matrix,
            assets,
            property_sets,
            spans,
        })
    }

    /// `eCEntity::ReadV63` 0x10225840 for entity versions 42..=82 (31..=41 predate the
    /// versioned property-set wrapper and are rejected).
    fn common_v63(&mut self) -> Result<Common> {
        let version = self.u16()?;
        ensure!(
            (0x2a..0x53).contains(&version),
            "unsupported legacy entity version {version}"
        );
        self.u16()?; // eCNode::Read version.
        let source_id = self.take(20)?.try_into()?;
        // Enabled, RenderingEnabled, ProcessingDisabled, two flag-word bits.
        let mut header_flags = [false; 5];
        for f in &mut header_flags {
            *f = self.flag()?;
        }
        if version < 0x3f {
            self.flag()?;
        }
        self.flag()?; // Pickable
        self.flag()?; // CollisionEnabled
        self.flag()?;
        self.take(4)?; // alpha f32
        self.take(3)?; // insertion u16, render priority u8.
        self.flag()?; // Locked
        if version > 0x39 {
            self.flag()?;
        }
        self.flag()?; // version >= 0x28
        let name = self.text()?;
        let world_matrix = self.matrix()?;
        let local_matrix = self.matrix()?;
        self.take(3 * 24)?; // three boxes
        self.take(2 * 16)?; // two spheres (version >= 0x29)
        self.take(4)?; // f32, version > 0x21
        self.flag()?; // version >= 0x26
        self.take(4)?; // f32, version > 0x26
        if version > 0x37 {
            self.take(4)?; // u32
        }
        if version >= 0x3d {
            self.take(4)?; // uniform scaling
        }
        if version >= 0x3e {
            self.flag()?;
            self.flag()?;
        }
        let count = self.u32()? as usize;
        ensure!(
            count <= MAX_PROPERTIES && count <= self.remaining() / 21,
            "property-set count exceeds bounds"
        );
        let mut property_sets = Vec::with_capacity(count);
        let mut assets = BTreeSet::new();
        for _ in 0..count {
            let class = self.legacy_set()?;
            property_sets.push(class.name);
            assets.extend(class.assets);
        }
        Ok(Common {
            source_id,
            enabled: header_flags[0],
            processing_disabled: header_flags[2],
            name,
            world_matrix,
            local_matrix,
            assets,
            property_sets,
            spans: Vec::new(),
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
        // GENOMFLE header with table offset 0 (observed in the unfinalized GUID .lrentdat files).
        let mut bytes = b"GENOMFLE".to_vec();
        bytes.extend([1, 0, 0, 0, 0, 0]);
        bytes.extend(b"GENOMEDL");
        bytes.extend(83u16.to_le_bytes());
        bytes.extend([0; 400]);
        let error = WorldDocument::parse(&bytes).unwrap_err();
        assert!(format!("{error:#}").contains("no string table"));
        assert!(is_unfinalized_layer(&bytes));
        // A context-only layer of the same shape (124 bytes in the shipped data) is empty.
        let mut small = b"GENOMFLE".to_vec();
        small.extend([1, 0, 0, 0, 0, 0]);
        small.extend(b"GENOMEDL");
        small.extend(83u16.to_le_bytes());
        small.extend([0; 100]);
        assert!(!is_unfinalized_layer(&small));
        let doc = WorldDocument::parse(&small).expect("context-only layer");
        assert_eq!(doc.kind, WorldKind::Lrentdat);
        assert!(doc.entities.is_empty());
        // Content without a table is never guessed.
        small.extend([0; 400]);
        assert!(WorldDocument::parse(&small).is_err());
    }

    // ---- legacy (pre-83) layouts -------------------------------------------------------

    fn inline(text: &str) -> Vec<u8> {
        let mut bytes = (text.len() as u16).to_le_bytes().to_vec();
        bytes.extend(text.as_bytes());
        bytes
    }

    /// Accessor-wrapped legacy property object; `name` is the encoded class name (inline text or
    /// a property-stream index, depending on the document).
    fn legacy_object(name: &[u8], object_version: u16, props: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        if object_version == 1 {
            body.extend(name); // class name again
        }
        if object_version <= 0x51 {
            body.extend([7u8; 20]); // property id
        }
        body.extend(30u16.to_le_bytes());
        body.extend(props);
        body.extend(tail);
        let mut bytes = vec![1, 0, 1, 1, 0, 1];
        bytes.extend(name);
        bytes.extend([1, 0, 0, 1, 0]);
        bytes.extend(object_version.to_le_bytes());
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    /// `eCEntity::ReadV63` entity (version 56) with the given encoded property sets.
    fn legacy_entity_body(id: u8, name: &[u8], sets: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = 56u16.to_le_bytes().to_vec();
        bytes.extend(1u16.to_le_bytes());
        bytes.extend([id; 20]);
        bytes.extend([1, 1, 0, 1, 0]); // five header flags
        bytes.extend([0, 0, 1, 0]); // extra (<0x3f), picking, collision, flag
        bytes.extend([0; 4 + 3]); // alpha, insertion, priority
        bytes.extend([0, 0]); // locked, bit (>=0x28)
        bytes.extend(name);
        bytes.extend(matrix(10.0));
        bytes.extend(matrix(4.0));
        bytes.extend([0; 3 * 24 + 2 * 16 + 4 + 1 + 4 + 4]);
        bytes.extend((sets.len() as u32).to_le_bytes());
        for set in sets {
            bytes.extend(set);
        }
        bytes
    }

    /// A legacy property set record: version, size of everything after the name, name, object.
    fn legacy_set(name: &[u8], object: &[u8]) -> Vec<u8> {
        let mut bytes = 50u16.to_le_bytes().to_vec();
        bytes.extend((object.len() as u32).to_le_bytes());
        bytes.extend(name);
        bytes.extend(object);
        bytes.extend(0xdeadc0deu32.to_le_bytes());
        bytes
    }

    fn mesh_props_inline() -> Vec<u8> {
        let mut props = 1u32.to_le_bytes().to_vec();
        props.extend(inline("ResourceFileName"));
        props.extend(inline("bCMeshResourceString"));
        props.extend(30u16.to_le_bytes());
        let value = inline("legacy.xcmsh");
        props.extend((value.len() as u32).to_le_bytes());
        props.extend(value);
        props
    }

    fn raw_node() -> Vec<u8> {
        let name = inline("eCVisualMeshStatic_PS");
        let object = legacy_object(&name, 1, &mesh_props_inline(), &[0xaa, 0xbb]);
        let mut doc = 30u16.to_le_bytes().to_vec();
        doc.extend(2u32.to_le_bytes());
        for (id, entity_name, sets) in [
            (1u8, "Root", vec![]),
            (2, "mesh", vec![legacy_set(&name, &object)]),
        ] {
            doc.extend([0, 0]); // no template, patching flag
            doc.extend(0x22u16.to_le_bytes());
            doc.push(0); // no template id
            doc.extend(legacy_entity_body(id, &inline(entity_name), &sets));
        }
        for value in [0u32, 1, u32::MAX, u32::MAX] {
            doc.extend(value.to_le_bytes());
        }
        doc
    }

    #[test]
    fn raw_legacy_node_with_inline_strings_parses() -> Result<()> {
        let bytes = raw_node();
        let doc = WorldDocument::parse(&bytes)?;
        assert_eq!(doc.kind, WorldKind::Node);
        assert_eq!(doc.edges, vec![(0, 1)]);
        assert_eq!(doc.entities[0].name, "Root");
        assert_eq!(doc.entities[1].name, "mesh");
        assert_eq!(doc.entities[1].world_matrix[3], [10.0, 2.0, -3.0, 1.0]);
        assert_eq!(doc.entities[1].source_id, [2; 20]);
        assert_eq!(doc.entities[1].property_sets, ["eCVisualMeshStatic_PS"]);
        assert_eq!(doc.entities[1].asset_references, ["legacy.xcmsh"]);
        for n in 0..bytes.len() {
            assert!(WorldDocument::parse(&bytes[..n]).is_err(), "truncated {n}");
        }
        // The side-table API stays aligned with the entities for layouts it cannot decode.
        let mut stats = crate::propstream::DecodeStats::default();
        let (_, sets) = WorldDocument::parse_with_properties(&bytes, &|_| true, &mut stats)?;
        assert_eq!(sets.len(), 2);
        let mut bad = bytes.clone();
        bad[0] = 0x80; // unknown version
        assert!(WorldDocument::parse(&bad).is_err());
        Ok(())
    }

    /// Property stream header: version 1, flag, string list, payload size.
    fn property_stream(strings: &[&str], payload: &[u8]) -> Vec<u8> {
        let mut bytes = 1u16.to_le_bytes().to_vec();
        bytes.push(1);
        bytes.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            bytes.extend(inline(s));
        }
        bytes.extend((payload.len() as u32).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn raw_legacy_layer_with_property_stream_parses() -> Result<()> {
        // strings: 0 context class, 1 context box property, 2 its type, 3 entity name
        let strings = ["eCEntityDynamicContext", "ContextBox", "bCBox", "unit"];
        let mut props = 1u32.to_le_bytes().to_vec();
        for value in [1u16, 2, 30] {
            props.extend(value.to_le_bytes());
        }
        props.extend(24u32.to_le_bytes());
        props.extend([0; 24]);
        let mut tail = 40u16.to_le_bytes().to_vec();
        tail.extend([0; 33]);
        // Version 81 object: property id, no class name; entities follow the context tail.
        let mut context = legacy_object(&0u16.to_le_bytes(), 81, &props, &tail);
        let mut entities = 1u32.to_le_bytes().to_vec();
        entities.extend(0x40u16.to_le_bytes()); // derived class version
        entities.extend(1u16.to_le_bytes()); // eCDynamicEntity version
        entities.extend([0, 0]); // legacy bool, no creator
        entities.extend(legacy_entity_body(5, &3u16.to_le_bytes(), &[]));
        for value in [u32::MAX, u32::MAX] {
            entities.extend(value.to_le_bytes());
        }
        // The context size field spans the entities (as in the shipped layers).
        let size_pos = 6 + 2 + 5 + 2;
        let spanning = (context.len() - size_pos - 4 + entities.len()) as u32;
        context[size_pos..size_pos + 4].copy_from_slice(&spanning.to_le_bytes());
        context.extend(&entities);
        let mut bytes = b"GENOMEDL".to_vec();
        bytes.extend(56u16.to_le_bytes());
        bytes.extend(property_stream(&strings, &context));
        let doc = WorldDocument::parse(&bytes)?;
        assert_eq!(doc.kind, WorldKind::Lrentdat);
        assert_eq!(doc.entities.len(), 1);
        assert_eq!(doc.entities[0].name, "unit");
        assert_eq!(doc.entities[0].source_id, [5; 20]);
        for n in (0..bytes.len()).filter(|&n| n != 10) {
            // (ten bytes is the shipped empty-layer stub)
            assert!(WorldDocument::parse(&bytes[..n]).is_err(), "truncated {n}");
        }
        // Payload size must match the file, string indices must stay in range.
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(WorldDocument::parse(&bad).is_err());
        let mut bad = b"GENOMEDL".to_vec();
        bad.extend(56u16.to_le_bytes());
        bad.extend(property_stream(&strings[..3], &context));
        assert!(WorldDocument::parse(&bad).is_err());
        Ok(())
    }

    #[test]
    fn raw_node_with_property_stream_and_headerless_layer_parse() -> Result<()> {
        // Version >= 56 spatial documents use the property stream with string indices.
        let mut entity = vec![0u8, 0];
        entity.extend(0x22u16.to_le_bytes());
        entity.push(0);
        entity.extend(legacy_entity_body(9, &0u16.to_le_bytes(), &[]));
        let mut payload = 1u32.to_le_bytes().to_vec();
        payload.extend(entity);
        for value in [u32::MAX, u32::MAX] {
            payload.extend(value.to_le_bytes());
        }
        let mut node = 72u16.to_le_bytes().to_vec();
        node.extend(property_stream(&["only"], &payload));
        let doc = WorldDocument::parse(&node)?;
        assert_eq!((doc.kind, doc.entities.len()), (WorldKind::Node, 1));
        assert_eq!(doc.entities[0].name, "only");

        // A dynamic layer without any magic: the accessor-wrapped context starts the file.
        let props = 0u32.to_le_bytes().to_vec();
        let mut tail = 40u16.to_le_bytes().to_vec();
        tail.extend([0; 33]);
        let mut layer = legacy_object(&inline("eCEntityDynamicContext"), 1, &props, &tail);
        let mut entities = 1u32.to_le_bytes().to_vec();
        entities.extend(0x24u16.to_le_bytes());
        entities.extend(1u16.to_le_bytes());
        entities.extend([0, 0]);
        entities.extend(legacy_entity_body(6, &inline("npc"), &[]));
        for value in [u32::MAX, u32::MAX] {
            entities.extend(value.to_le_bytes());
        }
        layer.extend(entities);
        let doc = WorldDocument::parse(&layer)?;
        assert_eq!(doc.kind, WorldKind::Lrentdat);
        assert_eq!(doc.entities[0].name, "npc");
        for n in 0..layer.len() {
            assert!(WorldDocument::parse(&layer[..n]).is_err(), "truncated {n}");
        }
        Ok(())
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
