//! Typed decoder for Genome property streams (templates, world nodes, lrentdat).
//!
//! A *property set* (class instance) is serialized as
//! `u16 object_version (0x53)`, `u32 size`, `u16 list_version`, `u32 count`, `count` properties,
//! then a class-specific *tail* (`u16 class_version` plus extra data) up to `size` bytes.
//! Every property is `u16 name_idx`, `u16 type_idx`, `u16 magic`, `u32 length`, payload; the
//! payload layout depends only on the type string, so values decode without a per-class schema
//! and a failed decode can never desynchronise the stream (the length frames each property).
//!
//! A decode is accepted only when it consumes the payload *exactly*; anything else is kept as
//! [`PropValue::Opaque`] and is counted by [`DecodeStats`]. Unit and field names come from
//! the file itself (string table); nothing here embeds original content.
use anyhow::{Context, Result, bail, ensure};
use serde::{Serialize, Serializer};

const MAX_PROPERTIES: usize = 10_000;
const MAX_ARRAY: usize = 1 << 20;
const MAX_DEPTH: usize = 6;
const OBJECT_VERSION: u16 = 0x53;

/// Bounded little-endian reader over a byte slice.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
    pub limit: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            limit: bytes.len(),
        }
    }
    pub fn with_range(bytes: &'a [u8], pos: usize, limit: usize) -> Result<Self> {
        ensure!(pos <= limit && limit <= bytes.len(), "cursor range invalid");
        Ok(Self { bytes, pos, limit })
    }
    pub fn remaining(&self) -> usize {
        self.limit - self.pos
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.remaining(), "truncated field at {}", self.pos);
        let out = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?))
    }
    /// Strict boolean: only 0 and 1 are accepted.
    pub fn flag(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => bail!("invalid boolean byte {v}"),
        }
    }
    pub fn guid(&mut self) -> Result<[u8; 20]> {
        Ok(self.take(20)?.try_into()?)
    }
    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
}

/// String table of a Genome document (after any index remapping), as raw bytes.
#[derive(Debug, Clone, Copy)]
pub struct Strings<'a> {
    pub entries: &'a [&'a [u8]],
}

impl<'a> Strings<'a> {
    pub fn new(entries: &'a [&'a [u8]]) -> Self {
        Self { entries }
    }
    pub fn bytes(&self, index: u16) -> Result<&'a [u8]> {
        self.entries
            .get(usize::from(index))
            .copied()
            .with_context(|| format!("string index {index} out of bounds"))
    }
    pub fn get(&self, index: u16) -> Result<String> {
        Ok(latin1(self.bytes(index)?))
    }
    pub fn entry(&self, cur: &mut Cursor) -> Result<String> {
        self.get(cur.u16()?)
    }
}

pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn ser_hex<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex(bytes))
}

/// Decoded property payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", content = "v")]
pub enum PropValue {
    Bool(bool),
    Char(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    F32(f32),
    Str(String),
    /// `bTPropertyContainer<enum T>`: enum type name and numeric value.
    Enum {
        ty: String,
        value: i32,
    },
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Euler([f32; 3]),
    ColorRgb([f32; 3]),
    ColorRgba([f32; 4]),
    Range1([f32; 2]),
    Range3([[f32; 3]; 2]),
    Rect([f32; 4]),
    Box3([[f32; 3]; 2]),
    Matrix([f32; 16]),
    /// 20-byte persistent id (`bCGuid`, `bCPropertyID`), lowercase hex.
    Id(String),
    /// `eCEntityProxy` / `eCTemplateEntityProxy`: the referenced entity/template GUID, if any.
    EntityRef(Option<String>),
    /// `eCPropertySetProxy`: property-set class name and entity GUID.
    PropertySetRef {
        set: Option<String>,
        entity: Option<String>,
    },
    StrArray(Vec<String>),
    F32Array(Vec<f32>),
    I32Array(Vec<i32>),
    U32Array(Vec<u32>),
    U16Array(Vec<u16>),
    BoolArray(Vec<bool>),
    Vec3Array(Vec<[f32; 3]>),
    /// `bCMotion` array: position xyz followed by rotation quaternion xyzw.
    MotionArray(Vec<[f32; 7]>),
    IdArray(Vec<String>),
    /// Quest log entries (heading, text).
    StrPairArray(Vec<(String, String)>),
    EntityRefArray(Vec<Option<String>>),
    EnumArray {
        ty: String,
        values: Vec<i32>,
    },
    Object(Box<PropertySet>),
    ObjectArray(Vec<PropertySet>),
    /// Payload whose layout is not (yet) known; kept verbatim.
    Opaque(#[serde(serialize_with = "ser_hex")] Vec<u8>),
}

impl PropValue {
    pub fn is_typed(&self) -> bool {
        !matches!(self, PropValue::Opaque(_))
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            PropValue::I32(v) => Some(*v),
            PropValue::U32(v) => i32::try_from(*v).ok(),
            PropValue::I16(v) => Some(i32::from(*v)),
            PropValue::U16(v) => Some(i32::from(*v)),
            PropValue::Char(v) => Some(i32::from(*v)),
            PropValue::Enum { value, .. } => Some(*value),
            PropValue::Bool(v) => Some(i32::from(*v)),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            PropValue::F32(v) => Some(*v),
            _ => self.as_i32().map(|v| v as f32),
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_ref_guid(&self) -> Option<&str> {
        match self {
            PropValue::EntityRef(Some(g)) => Some(g),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Property {
    pub name: String,
    pub type_name: String,
    pub magic: u16,
    pub value: PropValue,
}

/// One serialized class instance (an entity property set or a nested object).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropertySet {
    pub class: String,
    /// Wrapper version preceding the object (entity property-set version / template class version).
    pub wrapper_version: Option<u16>,
    pub list_version: u16,
    pub properties: Vec<Property>,
    /// `u16` written right after the property list (the class's persistence version).
    pub class_version: Option<u16>,
    pub tail: crate::props::tails::Tail,
}

impl PropertySet {
    pub fn get(&self, name: &str) -> Option<&PropValue> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.value)
    }
    pub fn int(&self, name: &str) -> Option<i32> {
        self.get(name).and_then(PropValue::as_i32)
    }
    pub fn float(&self, name: &str) -> Option<f32> {
        self.get(name).and_then(PropValue::as_f32)
    }
    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(PropValue::as_bool)
    }
    pub fn string(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(PropValue::as_str)
    }
}

/// Per-type coverage counters filled while decoding.
#[derive(Debug, Default, Clone, Serialize)]
pub struct DecodeStats {
    pub properties: u64,
    pub typed: u64,
    /// type name -> (instances, typed instances)
    pub by_type: std::collections::BTreeMap<String, (u64, u64)>,
    /// (type name, payload length) -> count, for opaque payloads only
    pub opaque_shapes: std::collections::BTreeMap<String, u64>,
    pub tails_total: u64,
    pub tails_decoded: u64,
    /// class name -> (property sets, sets whose tail has a dedicated decoder)
    pub tails_by_class: std::collections::BTreeMap<String, (u64, u64)>,
    pub tail_errors: std::collections::BTreeMap<String, u64>,
}

impl DecodeStats {
    pub fn merge(&mut self, other: &DecodeStats) {
        self.properties += other.properties;
        self.typed += other.typed;
        for (k, (a, b)) in &other.by_type {
            let e = self.by_type.entry(k.clone()).or_default();
            e.0 += a;
            e.1 += b;
        }
        for (k, v) in &other.opaque_shapes {
            *self.opaque_shapes.entry(k.clone()).or_default() += v;
        }
        self.tails_total += other.tails_total;
        self.tails_decoded += other.tails_decoded;
        for (k, (a, b)) in &other.tails_by_class {
            let e = self.tails_by_class.entry(k.clone()).or_default();
            e.0 += a;
            e.1 += b;
        }
        for (k, v) in &other.tail_errors {
            *self.tail_errors.entry(k.clone()).or_default() += v;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Value decoding
// ---------------------------------------------------------------------------------------------

fn enum_type(type_name: &str) -> Option<&str> {
    let inner = type_name.strip_prefix("bTPropertyContainer<enum ")?;
    Some(inner.strip_suffix('>')?.trim())
}

fn enum_array_type(type_name: &str) -> Option<&str> {
    let inner = type_name.strip_prefix("bTObjArray<class bTPropertyContainer<enum ")?;
    Some(
        inner
            .strip_suffix('>')?
            .trim_end()
            .strip_suffix('>')?
            .trim(),
    )
}

fn object_class(type_name: &str) -> Option<&str> {
    let inner = type_name.strip_prefix("bTPropertyObject<class ")?;
    Some(inner.split(',').next()?.trim())
}

fn smart_ptr_array_class(type_name: &str) -> Option<&str> {
    let inner = type_name.strip_prefix("bTObjArray<class bTAutoPOSmartPtr<class ")?;
    Some(inner.split('>').next()?.trim())
}

fn array_count(cur: &mut Cursor) -> Result<usize> {
    cur.skip(1)?; // bTArray writes a version byte before the count
    let n = cur.u32()? as usize;
    ensure!(
        n <= MAX_ARRAY && n <= cur.remaining(),
        "array count {n} out of bounds"
    );
    Ok(n)
}

fn opt_guid(cur: &mut Cursor) -> Result<Option<String>> {
    Ok(if cur.flag()? {
        Some(hex(&cur.guid()?))
    } else {
        None
    })
}

fn decode_scalar_or_composite(
    type_name: &str,
    cur: &mut Cursor,
    strings: &Strings,
    depth: usize,
    stats: &mut DecodeStats,
) -> Result<PropValue> {
    if let Some(ty) = enum_type(type_name) {
        let _version = cur.u16()?;
        return Ok(PropValue::Enum {
            ty: ty.to_owned(),
            value: cur.i32()?,
        });
    }
    Ok(match type_name {
        "bool" => PropValue::Bool(cur.flag()?),
        "char" | "unsigned char" => PropValue::Char(cur.u8()?),
        "short" => PropValue::I16(i16::from_le_bytes(cur.take(2)?.try_into()?)),
        "unsigned short" => PropValue::U16(cur.u16()?),
        "int" | "long" => PropValue::I32(cur.i32()?),
        "unsigned int" | "unsigned long" => PropValue::U32(cur.u32()?),
        "float" => PropValue::F32(cur.f32()?),
        "bCString"
        | "bCAnimationResourceString"
        | "bCImageOrMaterialResourceString"
        | "bCImageResourceString"
        | "bCMeshResourceString"
        | "bCScriptString"
        | "bCSpeedTreeResourceString"
        | "eCLocString" => PropValue::Str(strings.entry(cur)?),
        "bCVector" => PropValue::Vec3(cur.floats()?),
        "bCVector2" => PropValue::Vec2(cur.floats()?),
        "bCEulerAngles" => PropValue::Euler(cur.floats()?),
        // Both carry a 4-byte vtable slot (ignored by the engine) before the floats.
        "bCFloatColor" => {
            cur.skip(4)?;
            PropValue::ColorRgb(cur.floats()?)
        }
        "bCFloatAlphaColor" => {
            cur.skip(4)?;
            PropValue::ColorRgba(cur.floats()?)
        }
        "bCRange1" => PropValue::Range1(cur.floats()?),
        "bCRange3" => PropValue::Range3([cur.floats()?, cur.floats()?]),
        "bCRect" => PropValue::Rect(cur.floats()?),
        "bCBox" => PropValue::Box3([cur.floats()?, cur.floats()?]),
        "bCMatrix" => PropValue::Matrix(cur.floats()?),
        "bCGuid" | "bCPropertyID" => PropValue::Id(hex(&cur.guid()?)),
        "eCEntityProxy" | "eCTemplateEntityProxy" => {
            let _version = cur.u16()?;
            PropValue::EntityRef(opt_guid(cur)?)
        }
        "eCPropertySetProxy" => {
            let _version = cur.u16()?;
            let set = if cur.flag()? {
                Some(strings.entry(cur)?)
            } else {
                None
            };
            let _inner = cur.u16()?;
            PropValue::PropertySetRef {
                set,
                entity: opt_guid(cur)?,
            }
        }
        "bTObjArray<class bCString>" => {
            let n = array_count(cur)?;
            PropValue::StrArray((0..n).map(|_| strings.entry(cur)).collect::<Result<_>>()?)
        }
        "bTValArray<float>" => {
            let n = array_count(cur)?;
            PropValue::F32Array((0..n).map(|_| cur.f32()).collect::<Result<_>>()?)
        }
        "bTValArray<long>" | "bTValArray<int>" => {
            let n = array_count(cur)?;
            PropValue::I32Array((0..n).map(|_| cur.i32()).collect::<Result<_>>()?)
        }
        "bTValArray<unsigned long>" | "bTObjArray<unsigned long>" => {
            let n = array_count(cur)?;
            PropValue::U32Array((0..n).map(|_| cur.u32()).collect::<Result<_>>()?)
        }
        "bTValArray<unsigned short>" => {
            let n = array_count(cur)?;
            PropValue::U16Array((0..n).map(|_| cur.u16()).collect::<Result<_>>()?)
        }
        "bTValArray<bool>" => {
            let n = array_count(cur)?;
            PropValue::BoolArray((0..n).map(|_| cur.flag()).collect::<Result<_>>()?)
        }
        "bTValArray<class bCVector>" => {
            let n = array_count(cur)?;
            PropValue::Vec3Array((0..n).map(|_| cur.floats()).collect::<Result<_>>()?)
        }
        "bTValArray<class bCPropertyID>" => {
            let n = array_count(cur)?;
            PropValue::IdArray(
                (0..n)
                    .map(|_| Ok(hex(&cur.guid()?)))
                    .collect::<Result<_>>()?,
            )
        }
        "bTValArray<enum gEDirection>" => {
            let n = array_count(cur)?;
            PropValue::EnumArray {
                ty: "gEDirection".to_owned(),
                values: (0..n).map(|_| cur.i32()).collect::<Result<_>>()?,
            }
        }
        "bTValArray<class bCMotion>" => {
            let n = array_count(cur)?;
            PropValue::MotionArray((0..n).map(|_| cur.floats()).collect::<Result<_>>()?)
        }
        "bTPtrArray<class eCEntityProxy *>" if cur.remaining() == 0 => {
            PropValue::EntityRefArray(Vec::new())
        }
        "bTObjArray<struct gCQuest_PS::SLogEntry>" => {
            let n = array_count(cur)?;
            PropValue::StrPairArray(
                (0..n)
                    .map(|_| {
                        let _version = cur.u16()?;
                        Ok((strings.entry(cur)?, strings.entry(cur)?))
                    })
                    .collect::<Result<_>>()?,
            )
        }
        "bTObjArray<class eCEntityProxy>" | "bTObjArray<class eCTemplateEntityProxy>" => {
            let n = array_count(cur)?;
            PropValue::EntityRefArray(
                (0..n)
                    .map(|_| {
                        let _version = cur.u16()?;
                        opt_guid(cur)
                    })
                    .collect::<Result<_>>()?,
            )
        }
        _ => {
            if let Some(ty) = enum_array_type(type_name) {
                let n = array_count(cur)?;
                let values = (0..n)
                    .map(|_| {
                        let _version = cur.u16()?;
                        cur.i32()
                    })
                    .collect::<Result<_>>()?;
                return Ok(PropValue::EnumArray {
                    ty: ty.to_owned(),
                    values,
                });
            }
            if let Some(class) = object_class(type_name) {
                ensure!(depth < MAX_DEPTH, "nested object depth exceeded");
                let set = read_object_body(cur, strings, class, None, depth + 1, stats)?;
                return Ok(PropValue::Object(Box::new(set)));
            }
            if let Some(class) = smart_ptr_array_class(type_name) {
                ensure!(depth < MAX_DEPTH, "nested object depth exceeded");
                let n = array_count(cur)?;
                let mut out = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    let _version = cur.u16()?;
                    if cur.flag()? {
                        out.push(read_object_body(
                            cur,
                            strings,
                            class,
                            None,
                            depth + 1,
                            stats,
                        )?);
                    }
                }
                return Ok(PropValue::ObjectArray(out));
            }
            bail!("unknown property type");
        }
    })
}

/// Decode one payload; `Opaque` when the type is unknown or the layout does not fit exactly.
pub fn decode_value(
    type_name: &str,
    payload: &[u8],
    strings: &Strings,
    depth: usize,
    stats: &mut DecodeStats,
) -> PropValue {
    let mut cur = Cursor::new(payload);
    match decode_scalar_or_composite(type_name, &mut cur, strings, depth, stats) {
        Ok(v) if cur.remaining() == 0 => v,
        _ => PropValue::Opaque(payload.to_vec()),
    }
}

// ---------------------------------------------------------------------------------------------
// Property sets
// ---------------------------------------------------------------------------------------------

/// Read the `0x53 / size / list` body of a class instance. The cursor must be positioned at the
/// `u16` object version. `wrapper_version` is stored verbatim for provenance. When `decode` is
/// false the properties are skipped (class name and framing are still validated).
pub fn read_object_body(
    cur: &mut Cursor,
    strings: &Strings,
    class: &str,
    wrapper_version: Option<u16>,
    depth: usize,
    stats: &mut DecodeStats,
) -> Result<PropertySet> {
    let version = cur.u16()?;
    ensure!(
        version == 1 || (0x51..=OBJECT_VERSION).contains(&version),
        "unsupported object version {version:#x} in {class}"
    );
    let size = cur.u32()? as usize;
    ensure!(
        size <= cur.remaining(),
        "object {class} exceeds its container"
    );
    let end = cur.pos + size;
    let mut body = Cursor::with_range(cur.bytes, cur.pos, end)?;
    cur.pos = end;
    if version == 1 {
        strings.entry(&mut body)?; // legacy name entry
    }
    if version <= 0x51 {
        body.guid()?; // legacy object id
    }
    let list_version = body.u16()?;
    let count = body.u32()? as usize;
    ensure!(
        count <= MAX_PROPERTIES && count <= body.remaining() / 10,
        "property count {count} out of bounds"
    );
    let mut properties = Vec::with_capacity(count);
    for _ in 0..count {
        let name = strings.entry(&mut body)?;
        let type_name = strings.entry(&mut body)?;
        let magic = body.u16()?;
        let length = body.u32()? as usize;
        let payload = body.take(length)?;
        let value = decode_value(&type_name, payload, strings, depth, stats);
        stats.properties += 1;
        let entry = stats.by_type.entry(type_name.clone()).or_default();
        entry.0 += 1;
        if value.is_typed() {
            stats.typed += 1;
            entry.1 += 1;
        } else {
            *stats
                .opaque_shapes
                .entry(format!("{type_name} [{length}]"))
                .or_default() += 1;
        }
        properties.push(Property {
            name,
            type_name,
            magic,
            value,
        });
    }
    let class_version = if body.remaining() >= 2 {
        Some(body.u16()?)
    } else {
        None
    };
    let tail =
        crate::props::tails::decode_tail(class, class_version, &mut body, strings, depth, stats);
    Ok(PropertySet {
        class: class.to_owned(),
        wrapper_version,
        list_version,
        properties,
        class_version,
        tail,
    })
}

/// Fixed marker preceding every serialized class name: accessor `01 00 01 01 00 01`.
pub const CLASS_ACCESSOR: [u8; 6] = [1, 0, 1, 1, 0, 1];

/// Read a nested class reference: accessor, class name, 5 filler bytes, then the object body.
pub fn read_sub_class(
    cur: &mut Cursor,
    strings: &Strings,
    depth: usize,
    stats: &mut DecodeStats,
) -> Result<PropertySet> {
    ensure!(depth < MAX_DEPTH, "nested class depth exceeded");
    ensure!(cur.take(6)? == CLASS_ACCESSOR, "unsupported class accessor");
    let name = strings.entry(cur)?;
    let filler = cur.take(5)?;
    ensure!(
        filler[..3] == [1, 0, 0] && cur.bytes.get(cur.pos..cur.pos + 2) == Some(&filler[3..5]),
        "unsupported class filler {}",
        hex(filler)
    );
    read_object_body(cur, strings, &name, None, depth + 1, stats)
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Synthetic stream builders shared by the format tests.
    pub fn prop(name: u16, ty: u16, payload: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(name.to_le_bytes());
        b.extend(ty.to_le_bytes());
        b.extend(30u16.to_le_bytes());
        b.extend((payload.len() as u32).to_le_bytes());
        b.extend(payload);
        b
    }
    /// `0x53 / size / list_version / count / props / class_version / tail`
    pub fn object(props: &[Vec<u8>], class_version: u16, tail: &[u8]) -> Vec<u8> {
        let mut inner = Vec::new();
        inner.extend(30u16.to_le_bytes());
        inner.extend((props.len() as u32).to_le_bytes());
        for p in props {
            inner.extend(p);
        }
        inner.extend(class_version.to_le_bytes());
        inner.extend(tail);
        let mut b = Vec::new();
        b.extend(0x53u16.to_le_bytes());
        b.extend((inner.len() as u32).to_le_bytes());
        b.extend(inner);
        b
    }
    /// One template class entry: wrapper version, pre-size, pre-type, accessor, name, filler,
    /// object, terminator.
    pub fn template_class(name: u16, object: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(7u16.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(name.to_le_bytes());
        b.extend(sub_class(name, object));
        b.extend(0xdead_c0deu32.to_le_bytes());
        b
    }
    /// Version-62 template entity header with `classes` (empty `classes` + `reference` makes a
    /// reference-only header).
    pub fn template_header(
        file: u16,
        name: u16,
        guid: u8,
        helper: bool,
        classes: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(62u16.to_le_bytes());
        b.extend(file.to_le_bytes());
        b.extend([guid; 20]);
        b.extend([1, 0, 0, 1, 1, 1, 0]); // seven flags
        b.push(u8::from(helper));
        b.push(0); // no reference template
        b.extend([0; 4 + 2 + 1]); // alpha, insert type, priority
        b.extend(name.to_le_bytes());
        for x in [
            1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0,
        ] {
            b.extend(x.to_le_bytes());
        }
        b.extend([0; 64]); // local matrix
        b.extend([0; 72]); // boxes
        b.extend([0; 4]); // lod factor
        b.extend([0, 0]); // flag, deleted
        b.extend([0; 4]); // cull factor
        b.extend([0; 4]); // time stamp
        b.push(0);
        b.extend([0; 4]); // scaling
        b.extend([0, 0]);
        b.extend((classes.len() as u32).to_le_bytes());
        for c in classes {
            b.extend(c);
        }
        b
    }
    /// A `GENOMFLE` envelope around a `GENOMETP` body with an identity index table.
    pub fn template_file(strings: &[&str], headers: &[Vec<u8>]) -> Vec<u8> {
        let mut body = b"GENOMETP".to_vec();
        body.extend(62u16.to_le_bytes());
        body.extend(1u16.to_le_bytes());
        body.push(1);
        body.extend((strings.len() as u32).to_le_bytes());
        for i in 0..strings.len() {
            body.extend((i as u16).to_le_bytes());
        }
        body.extend(0u32.to_le_bytes());
        body.extend((headers.len() as u32).to_le_bytes());
        for h in headers {
            body.extend(h);
        }
        for v in [u32::MAX, u32::MAX] {
            body.extend(v.to_le_bytes());
        }
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend((14 + body.len() as u32).to_le_bytes());
        b.extend(body);
        b.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b
    }
    /// accessor + class name index + filler + object (the nested `readSubClass` shape).
    pub fn sub_class(name: u16, object: &[u8]) -> Vec<u8> {
        let mut b = vec![1, 0, 1, 1, 0, 1];
        b.extend(name.to_le_bytes());
        b.extend([1, 0, 0, 0x53, 0]);
        b.extend(object);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn table() -> Vec<&'static [u8]> {
        vec![
            b"Amount",
            b"int",
            b"Category",
            b"bTPropertyContainer<enum gEItemCategory>",
            b"Name",
            b"bCString",
            b"hello",
            b"Pos",
            b"bCVector",
            b"Mystery",
            b"bCUnknownThing",
            b"Flag",
            b"bool",
            b"gCItem_PS",
        ]
    }

    #[test]
    fn decodes_scalars_enums_strings_vectors_and_keeps_unknown_opaque() {
        let t = table();
        let strings = Strings::new(&t);
        let mut vec3 = Vec::new();
        for f in [1.0f32, 2.0, -3.5] {
            vec3.extend(f.to_le_bytes());
        }
        let props = vec![
            prop(0, 1, &42i32.to_le_bytes()),
            prop(2, 3, &[1, 0, 7, 0, 0, 0]),
            prop(4, 5, &6u16.to_le_bytes()),
            prop(7, 8, &vec3),
            prop(9, 10, &[1, 2, 3]),
            prop(11, 12, &[1]),
            // wrong size for the declared type must not desynchronise: stays opaque
            prop(0, 1, &[1, 2]),
        ];
        let bytes = object(&props, 7, &[]);
        let mut cur = Cursor::new(&bytes);
        let mut stats = DecodeStats::default();
        let set = read_object_body(&mut cur, &strings, "gCItem_PS", None, 0, &mut stats).unwrap();
        assert_eq!(cur.remaining(), 0);
        assert_eq!(set.class_version, Some(7));
        assert_eq!(set.int("Amount"), Some(42));
        assert_eq!(
            set.get("Category"),
            Some(&PropValue::Enum {
                ty: "gEItemCategory".into(),
                value: 7
            })
        );
        assert_eq!(set.string("Name"), Some("hello"));
        assert_eq!(set.get("Pos"), Some(&PropValue::Vec3([1.0, 2.0, -3.5])));
        assert_eq!(set.get("Mystery"), Some(&PropValue::Opaque(vec![1, 2, 3])));
        assert_eq!(set.boolean("Flag"), Some(true));
        assert!(matches!(set.properties[6].value, PropValue::Opaque(_)));
        assert_eq!((stats.properties, stats.typed), (7, 5));
    }

    #[test]
    fn rejects_truncation_and_oversize_frames() {
        let t = table();
        let strings = Strings::new(&t);
        let bytes = object(&[prop(0, 1, &1i32.to_le_bytes())], 1, &[]);
        for n in 0..bytes.len() {
            let mut cur = Cursor::new(&bytes[..n]);
            let mut stats = DecodeStats::default();
            assert!(
                read_object_body(&mut cur, &strings, "X", None, 0, &mut stats).is_err(),
                "{n}"
            );
        }
        // property length larger than the object
        let mut bad = bytes.clone();
        bad[6 + 2 + 4 + 2 + 2 + 2] = 0xff;
        let mut cur = Cursor::new(&bad);
        let mut stats = DecodeStats::default();
        assert!(read_object_body(&mut cur, &strings, "X", None, 0, &mut stats).is_err());
    }

    #[test]
    fn nested_objects_and_arrays_decode() {
        let mut t = table();
        t.push(b"Inner"); // 14
        t.push(b"bTPropertyObject<class gCItem_PS, class bCObjectRefBase>"); // 15
        t.push(b"Refs"); // 16
        t.push(b"bTObjArray<class eCEntityProxy>"); // 17
        let strings = Strings::new(&t);
        let inner = object(&[prop(0, 1, &5i32.to_le_bytes())], 2, &[]);
        let mut refs = vec![0u8];
        refs.extend(2u32.to_le_bytes());
        refs.extend([1, 0, 1]);
        refs.extend([0xab; 20]);
        refs.extend([1, 0, 0]);
        let bytes = object(&[prop(14, 15, &inner), prop(16, 17, &refs)], 1, &[]);
        let mut cur = Cursor::new(&bytes);
        let mut stats = DecodeStats::default();
        let set = read_object_body(&mut cur, &strings, "gCItem_PS", None, 0, &mut stats).unwrap();
        match set.get("Inner") {
            Some(PropValue::Object(o)) => assert_eq!(o.int("Amount"), Some(5)),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            set.get("Refs"),
            Some(&PropValue::EntityRefArray(vec![
                Some("ab".repeat(20)),
                None
            ]))
        );
    }

    #[test]
    fn colors_direction_arrays_motions_and_log_entries() {
        let mut t = table();
        for x in [
            "Col",
            "bCFloatAlphaColor",
            "Dir",
            "bTValArray<enum gEDirection>",
            "Mot",
            "bTValArray<class bCMotion>",
            "Log",
            "bTObjArray<struct gCQuest_PS::SLogEntry>",
            "Head",
            "Body",
        ] {
            t.push(x.as_bytes());
        }
        let strings = Strings::new(&t);
        let mut color = vec![0u8; 4];
        for f in [0.25f32, 0.5, 0.75, 1.0] {
            color.extend(f.to_le_bytes());
        }
        let mut dirs = vec![0u8];
        dirs.extend(2u32.to_le_bytes());
        dirs.extend(3i32.to_le_bytes());
        dirs.extend(4i32.to_le_bytes());
        let mut motions = vec![0u8];
        motions.extend(1u32.to_le_bytes());
        for f in [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0] {
            motions.extend(f.to_le_bytes());
        }
        let mut log = vec![0u8];
        log.extend(1u32.to_le_bytes());
        log.extend([1, 0, 22, 0, 23, 0]);
        let bytes = object(
            &[
                prop(14, 15, &color),
                prop(16, 17, &dirs),
                prop(18, 19, &motions),
                prop(20, 21, &log),
            ],
            1,
            &[],
        );
        let mut cur = Cursor::new(&bytes);
        let mut stats = DecodeStats::default();
        let set = read_object_body(&mut cur, &strings, "X", None, 0, &mut stats).unwrap();
        assert_eq!(
            set.get("Col"),
            Some(&PropValue::ColorRgba([0.25, 0.5, 0.75, 1.0]))
        );
        assert_eq!(
            set.get("Dir"),
            Some(&PropValue::EnumArray {
                ty: "gEDirection".into(),
                values: vec![3, 4]
            })
        );
        assert_eq!(
            set.get("Mot"),
            Some(&PropValue::MotionArray(vec![[
                1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0
            ]]))
        );
        assert_eq!(
            set.get("Log"),
            Some(&PropValue::StrPairArray(vec![(
                "Head".into(),
                "Body".into()
            )]))
        );
        assert_eq!(stats.typed, stats.properties);
    }

    #[test]
    fn legacy_object_versions_and_bad_filler_are_handled() {
        let t = table();
        let strings = Strings::new(&t);
        // version 0x52: same layout as 0x53
        let mut obj = object(&[prop(0, 1, &9i32.to_le_bytes())], 3, &[]);
        obj[0] = 0x52;
        let mut b = vec![1, 0, 1, 1, 0, 1, 13, 0, 1, 0, 0, 0x52, 0];
        b.extend(&obj);
        let mut cur = Cursor::new(&b);
        let mut stats = DecodeStats::default();
        let set = read_sub_class(&mut cur, &strings, 0, &mut stats).unwrap();
        assert_eq!(
            (set.class.as_str(), set.int("Amount")),
            ("gCItem_PS", Some(9))
        );
        // filler that disagrees with the object version is rejected, not guessed
        let mut bad = b.clone();
        bad[11] = 0x53;
        let mut cur = Cursor::new(&bad);
        assert!(read_sub_class(&mut cur, &strings, 0, &mut stats).is_err());
    }
}
