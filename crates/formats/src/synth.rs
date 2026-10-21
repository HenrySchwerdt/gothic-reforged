//! Robustness harness shared by the libFuzzer targets (`crates/formats/fuzz`) and the
//! property tests (`tests/robustness.rs`): one entry point per parser, a mapping from
//! arbitrary bytes to structurally plausible inputs (valid envelopes and magics around fuzzed
//! payloads), and seed inputs. Everything here is synthetic; no game data. See
//! `docs/FUZZING.md`.
use crate::propstream::{CLASS_ACCESSOR, Cursor, DecodeStats, Strings};
use std::io::Write;

/// Every fuzzed parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Pak,
    Genome,
    Ximg,
    Xcmsh,
    Xlmsh,
    Xact,
    Xmot,
    Xnvmsh,
    Xshmat,
    Xlmp,
    TemplateSummary,
    TemplateDocument,
    WorldNode,
    WorldProps,
    Vegetation,
    Spt,
    Igl,
    Theme,
    StringTable,
    Quest,
    Info,
    Gdini,
    RemasterMesh,
    Dds,
    PropStream,
    Emfx,
    Weather,
    TerrainName,
}

impl Target {
    pub const ALL: [Target; 28] = [
        Target::Pak,
        Target::Genome,
        Target::Ximg,
        Target::Xcmsh,
        Target::Xlmsh,
        Target::Xact,
        Target::Xmot,
        Target::Xnvmsh,
        Target::Xshmat,
        Target::Xlmp,
        Target::TemplateSummary,
        Target::TemplateDocument,
        Target::WorldNode,
        Target::WorldProps,
        Target::Vegetation,
        Target::Spt,
        Target::Igl,
        Target::Theme,
        Target::StringTable,
        Target::Quest,
        Target::Info,
        Target::Gdini,
        Target::RemasterMesh,
        Target::Dds,
        Target::PropStream,
        Target::Emfx,
        Target::Weather,
        Target::TerrainName,
    ];

    /// Fuzz target name (`fuzz/fuzz_targets/<name>.rs`).
    pub fn name(self) -> &'static str {
        match self {
            Target::Pak => "pak",
            Target::Genome => "genome",
            Target::Ximg => "ximg",
            Target::Xcmsh => "xcmsh",
            Target::Xlmsh => "xlmsh",
            Target::Xact => "xact",
            Target::Xmot => "xmot",
            Target::Xnvmsh => "xnvmsh",
            Target::Xshmat => "xshmat",
            Target::Xlmp => "xlmp",
            Target::TemplateSummary => "template_summary",
            Target::TemplateDocument => "template_document",
            Target::WorldNode => "world_node",
            Target::WorldProps => "world_props",
            Target::Vegetation => "vegetation",
            Target::Spt => "spt",
            Target::Igl => "igl",
            Target::Theme => "theme",
            Target::StringTable => "stringtable",
            Target::Quest => "quest",
            Target::Info => "info",
            Target::Gdini => "gdini",
            Target::RemasterMesh => "remaster_mesh",
            Target::Dds => "dds",
            Target::PropStream => "propstream",
            Target::Emfx => "emfx",
            Target::Weather => "weather",
            Target::TerrainName => "terrain_name",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Run a parser on `data` (and the follow-up calls a loader makes on success). Errors are
/// expected; a panic, a hang or an unbounded allocation is a bug.
pub fn run(target: Target, data: &[u8]) {
    use crate::*;
    match target {
        Target::Pak => {
            if let Ok(mut pak) = PakArchive::read(std::io::Cursor::new(data)) {
                for i in 0..pak.entries.len().min(8) {
                    let _ = pak.read_payload(i);
                }
            }
        }
        Target::Genome => {
            let _ = genome::GenomeFile::parse(data);
        }
        Target::Ximg => {
            if let Ok(x) = ximg::Ximg::parse(data) {
                let _ = x.to_dds();
            }
        }
        Target::Xcmsh => {
            let _ = xcmsh::Xcmsh::parse(data);
        }
        Target::Xlmsh => {
            let _ = xlmsh::Xlmsh::parse(data);
        }
        Target::Xact => {
            let _ = xact::Actor::parse(data);
        }
        Target::Xmot => {
            let _ = xmot::Motion::parse(data);
        }
        Target::Xnvmsh => {
            if let Ok(x) = xnvmsh::Xnvmsh::parse(data) {
                let _ = x.triangles_cm();
            }
        }
        Target::Xshmat => {
            let _ = xshmat::Xshmat::parse(data);
        }
        Target::Xlmp => {
            let _ = xlmp::Xlmp::parse_opts(data, true);
        }
        Target::TemplateSummary => {
            let _ = template::TemplateSummary::parse(data);
        }
        Target::TemplateDocument => {
            let _ = template::TemplateDocument::parse(data, &mut DecodeStats::default());
        }
        Target::WorldNode => {
            let _ = world_node::WorldDocument::parse(data);
        }
        Target::WorldProps => {
            let _ = world_node::WorldDocument::parse_with_properties(
                data,
                &|_| true,
                &mut DecodeStats::default(),
            );
        }
        Target::Vegetation => {
            let _ = terrain::vegetation::extract(data);
        }
        Target::Spt => {
            if let Ok(s) = terrain::spt::Spt::parse(data) {
                let _ = s.trunk_capsule_cm();
                let _ = terrain::spt_gen::generate(&s, 0, Default::default());
            }
        }
        Target::Igl => {
            let _ = igl::Igl::parse(data);
        }
        Target::Theme => {
            let _ = theme::Theme::parse(data);
        }
        Target::StringTable => {
            let _ = stringtable::StringTable::parse(data);
        }
        Target::Quest => {
            let _ = quest::Quest::parse(data);
        }
        Target::Info => {
            let _ = info::Info::parse(data);
        }
        Target::Gdini => {
            let _ = gdini::parse_record(data, "Quest");
        }
        Target::RemasterMesh => {
            if let Ok(m) = remaster::OverrideMesh::parse(data) {
                let _ = m.to_bytes();
            }
        }
        Target::Dds => {
            if let Ok(d) = remaster::DdsTexture::parse(data) {
                let _ = d.to_bytes();
            }
        }
        Target::PropStream => {
            const STRINGS: [&[u8]; 12] = [
                b"gCInventory_PS",
                b"Name",
                b"bCString",
                b"int",
                b"float",
                b"bool",
                b"bCVector",
                b"bCGuid",
                b"gEDirection",
                b"bTObjArray<class bCString>",
                b"eCEntityProxy",
                b"bCMatrix",
            ];
            let strings = Strings::new(&STRINGS);
            let mut stats = DecodeStats::default();
            let mut cur = Cursor::new(data);
            let _ = propstream::read_sub_class(&mut cur, &strings, 0, &mut stats);
            for ty in [
                "int",
                "float",
                "bCVector",
                "bTObjArray<class bCString>",
                "bCMatrix",
            ] {
                let _ = propstream::decode_value(ty, data, &strings, 0, &mut stats);
            }
        }
        Target::Emfx => {
            let _ = emfx::read_container(data, b"FXA ", 2, true);
            let _ = emfx::read_container(data, b"LMA ", 3, false);
        }
        Target::Weather => {
            if let Ok(w) = weather::parse(data) {
                for st in &w.states {
                    for t in [-1.0, 0.0, 0.37, 1.0, 2.0, f32::NAN] {
                        let _ = st.scales.sun_diffuse.at(t);
                        let _ = st.scales.fog_end.at(t);
                    }
                }
                for e in &w.environments {
                    let _ = e.entries.first().and_then(|x| w.state(&x.state));
                }
            }
        }
        Target::TerrainName => {
            let _ = terrain::parse_cell_name(&String::from_utf8_lossy(data));
        }
    }
}

/// Fuzzer bytes as a stream of small numbers (wraps around; zeros when empty).
struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        if self.data.is_empty() {
            return 0;
        }
        let b = self.data[self.pos % self.data.len()];
        self.pos += 1;
        b
    }
    fn below(&mut self, n: u8) -> u8 {
        self.u8() % n.max(1)
    }
    fn f32(&mut self) -> f32 {
        f32::from(self.u8()) / 64.0 - 1.0
    }
}

/// A `WeatherStates.g3ws` file whose counts, property picks and key values come from `data`
/// (valid layout throughout, so the fuzzer explores the decoding, not the magics).
fn weather_file(data: &[u8]) -> Vec<u8> {
    const PROPS: [&[u8]; 6] = [
        b"ParticlesPerSecond",
        b"SpeedMinMax",
        b"LoopSample",
        b"SplashSizeScale",
        b"LoopVolume",
        b"Unknown",
    ];
    // Strings: properties, then state and climate names.
    let mut strings: Vec<&[u8]> = PROPS.to_vec();
    strings.extend([&b"Clear"[..], b"Rain\0junk", b"Forest", b"Myrtana"]);
    let nstr = strings.len() as u16;
    let mut b = Bytes { data, pos: 0 };
    let mut body = b"GEWS".to_vec();
    body.extend(2u16.to_le_bytes());
    let states = b.below(4);
    body.extend(u32::from(states).to_le_bytes());
    // Scale kinds in file order (true = colour).
    const COLOUR: [bool; 16] = [
        true, true, false, true, true, false, true, false, true, true, false, false, true, false,
        false, false,
    ];
    for _ in 0..states {
        body.extend([1, 0, 1, 1, 0, 1]);
        body.extend(7u16.to_le_bytes());
        body.extend([1, 0, 0, 1, 0]);
        body.extend(0x53u16.to_le_bytes());
        let mut obj = 30u16.to_le_bytes().to_vec();
        let props = b.below(5);
        obj.extend(u32::from(props).to_le_bytes());
        for _ in 0..props {
            let prop = b.below(PROPS.len() as u8);
            obj.extend(u16::from(prop).to_le_bytes());
            obj.extend(0u16.to_le_bytes());
            obj.extend(30u16.to_le_bytes());
            if PROPS[usize::from(prop)] == b"LoopSample" {
                // A two-byte string index.
                obj.extend(2u32.to_le_bytes());
                obj.extend(u16::from(b.below(nstr as u8)).to_le_bytes());
                continue;
            }
            let len = [0usize, 2, 4, 8][usize::from(b.below(4))];
            obj.extend((len as u32).to_le_bytes());
            for _ in 0..len {
                obj.push(b.u8());
            }
        }
        obj.extend(7u16.to_le_bytes());
        obj.extend((PROPS.len() as u16 + u16::from(b.below(2))).to_le_bytes());
        for colour in COLOUR {
            obj.extend(1u32.to_le_bytes());
            let keys = u32::from(b.below(4));
            let entry = if colour { 20 } else { 8 };
            obj.extend((keys * entry).to_le_bytes());
            if keys > 0 {
                obj.extend(keys.to_le_bytes());
                for _ in 0..keys * entry / 4 {
                    obj.extend(b.f32().to_le_bytes());
                }
            }
        }
        body.extend((obj.len() as u32).to_le_bytes());
        body.extend(obj);
    }
    let envs = b.below(3);
    body.extend(u32::from(envs).to_le_bytes());
    for _ in 0..envs {
        body.extend(3u16.to_le_bytes());
        body.extend((nstr - 2).to_le_bytes());
        body.push(1);
        let k = b.below(4);
        body.extend(u32::from(k).to_le_bytes());
        for _ in 0..k {
            body.extend((PROPS.len() as u16 + u16::from(b.below(3))).to_le_bytes());
            for _ in 0..3 {
                body.extend(b.f32().to_le_bytes());
            }
        }
        body.extend((nstr - 1).to_le_bytes());
        body.extend(i32::from(b.u8()).to_le_bytes());
    }
    genome(&body, &strings)
}

/// An `.igl` image list: groups of tiles with string-table paths, counts and rectangles from
/// `data`.
fn igl_file(data: &[u8], strings: &[&[u8]]) -> Vec<u8> {
    let mut b = Bytes { data, pos: 0 };
    let nstr = strings.len().max(1) as u8;
    let groups = 1 + b.below(3);
    let mut body = u32::from(groups).to_le_bytes().to_vec();
    body.extend([0u8; 26]); // header up to the first group at 0x2c
    for _ in 0..groups {
        body.extend(2u32.to_le_bytes());
        for _ in 0..4 {
            body.extend(u16::from(b.below(nstr)).to_le_bytes());
        }
        body.extend(u32::from(b.u8()).to_le_bytes());
        body.extend(u32::from(b.u8()).to_le_bytes());
        body.extend([0u8; 36]);
        let tiles = b.below(4);
        body.extend(u32::from(tiles).to_le_bytes());
        for _ in 0..tiles {
            body.extend(u16::from(b.below(nstr)).to_le_bytes());
            body.extend(u16::from(b.below(nstr)).to_le_bytes());
            for _ in 0..4 {
                body.extend(u32::from(b.u8()).to_le_bytes());
            }
        }
    }
    genome(&body, strings)
}

/// A GENOMFLE v1 envelope: header, body, `0xDEADBEEF` table with `strings`.
pub fn genome(body: &[u8], strings: &[&[u8]]) -> Vec<u8> {
    let mut out = b"GENOMFLE".to_vec();
    out.extend(1u16.to_le_bytes());
    out.extend(((14 + body.len()) as u32).to_le_bytes());
    out.extend(body);
    out.extend([0xef, 0xbe, 0xad, 0xde, 1]);
    out.extend((strings.len() as u32).to_le_bytes());
    for s in strings {
        out.extend((s.len() as u16).to_le_bytes());
        out.extend(*s);
    }
    out
}

/// Class and type names the structured inputs put into string tables, so string indices
/// in fuzzed bodies resolve to names the parsers look for.
fn dictionary(target: Target) -> &'static [&'static [u8]] {
    match target {
        Target::Xcmsh => &[b"eCResourceMeshComplex_PS", b"bCMeshResourceString"],
        Target::Xlmsh => &[b"eCResourceMeshLoD_PS", b"float", b"LoDScale", b"bCBox"],
        Target::Xshmat => &[
            b"eCResourceShaderMaterial_PS",
            b"ImageFilePath",
            b"bCImageResourceString",
            b"BlendMode",
            b"MaskReference",
            b"char",
            b"bool",
        ],
        Target::Xlmp => &[b"eCResourceLightmap_PS"],
        Target::Vegetation => &[b"eCVegetation_Mesh", b"eCVegetation_PS"],
        Target::WorldNode | Target::WorldProps => &[
            b"eCEntity",
            b"ResourceFileName",
            b"bCMeshResourceString",
            b"int",
        ],
        Target::TemplateSummary | Target::TemplateDocument => {
            &[b"eCTemplateEntity", b"gCInventory_PS", b"Name", b"bCString"]
        }
        Target::Igl => &[
            b"G3_HUD_Items",
            b"Data\\gui\\icons\\compiled\\",
            b"#G3:\\Data\\gui\\icons\\IT_Apple.tga",
            b"Data\\gui\\icons\\compiled\\/g3_hud_it_00.dds",
        ],
        _ => &[b"Name", b"int"],
    }
}

/// Split fuzzer bytes into a few short strings and a body.
fn split(data: &[u8]) -> (Vec<&[u8]>, &[u8]) {
    let Some((&n, mut rest)) = data.split_first() else {
        return (Vec::new(), data);
    };
    let mut strings = Vec::new();
    for _ in 0..(n % 6) {
        let Some((&len, tail)) = rest.split_first() else {
            break;
        };
        let len = usize::from(len % 24).min(tail.len());
        strings.push(&tail[..len]);
        rest = &tail[len..];
    }
    (strings, rest)
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// Map arbitrary bytes to an input shaped like `target`'s format (valid outer layers, fuzzed
/// inner payload), so mutations reach past the magic and envelope checks.
pub fn structured(target: Target, data: &[u8]) -> Vec<u8> {
    let (fuzz_strings, body) = split(data);
    let mut strings: Vec<&[u8]> = dictionary(target).to_vec();
    strings.extend(fuzz_strings);
    let text = || String::from_utf8_lossy(data).into_owned();
    match target {
        Target::Pak => {
            let mut out = Vec::new();
            out.extend(le32(0));
            out.extend(le32(0x3056_3347));
            out.extend([0u8; 16]); // revision, encryption, compression, reserved
            let table = 48u64 + 16;
            out.extend(table.to_le_bytes());
            out.extend(table.to_le_bytes());
            out.extend(table.to_le_bytes());
            out.extend(b"payload_bytes!!!");
            out.extend(body);
            out
        }
        Target::Genome => genome(body, &strings),
        Target::Ximg => {
            // Header fields relative to the file: G3IMG at 38, width 47, height 51,
            // mip count 63, FourCC 71, data from 87.
            let w = 1 + u32::from(body.first().copied().unwrap_or(3) % 64);
            let h = 1 + u32::from(body.get(1).copied().unwrap_or(3) % 64);
            let mips = 1 + u32::from(body.get(2).copied().unwrap_or(0) % 4);
            let fourcc: &[u8; 4] = match body.get(3).copied().unwrap_or(0) % 3 {
                0 => b"DXT1",
                1 => b"DXT3",
                _ => b"DXT5",
            };
            let mut b = vec![0u8; 24];
            b.extend(b"G3IMG");
            b.extend([0u8; 4]);
            b.extend(le32(w));
            b.extend(le32(h));
            b.extend([0u8; 8]);
            b.extend(le32(mips));
            b.extend([0u8; 4]);
            b.extend(fourcc);
            b.extend([0u8; 12]);
            b.extend(body.get(4..).unwrap_or_default());
            genome(&b, &strings)
        }
        Target::Xact => {
            let mut b = b"gena".to_vec();
            b.extend(1u16.to_le_bytes());
            b.extend(le32(body.len() as u32 + 6));
            b.extend(b"FXA ");
            b.extend([1, 0]);
            b.extend(body);
            genome(&b, &strings)
        }
        Target::Xmot => {
            let mut b = b"LMA ".to_vec();
            b.extend([1, 0, 0]);
            b.extend(body);
            genome(&b, &strings)
        }
        Target::Emfx => {
            let mut b = b"gena".to_vec();
            b.extend(1u16.to_le_bytes());
            b.extend(le32(body.len() as u32 + 6));
            b.extend(if data.first().is_some_and(|x| x & 1 == 0) {
                b"FXA "
            } else {
                b"LMA "
            });
            b.extend([1, 0, 0]);
            b.extend(body);
            b
        }
        Target::WorldNode | Target::WorldProps | Target::Vegetation => {
            let mut b = b"GENOMEDL".to_vec();
            b.extend(83u16.to_le_bytes());
            b.extend(body);
            genome(&b, &strings)
        }
        Target::TemplateSummary | Target::TemplateDocument => {
            let mut b = b"GENOMETP".to_vec();
            b.extend(body);
            genome(&b, &strings)
        }
        Target::Spt => {
            let mut b = vec![0u8; 8];
            b.extend(b"__IdvSpt_02_");
            b.extend(body);
            b
        }
        Target::Xcmsh
        | Target::Xlmsh
        | Target::Xnvmsh
        | Target::Xshmat
        | Target::Xlmp
        | Target::Theme => genome(body, &strings),
        Target::Igl => igl_file(body, &strings),
        Target::Weather => weather_file(data),
        Target::TerrainName => {
            let n = data.first().copied().unwrap_or(0);
            let region = ["Myrtana", "Varant", "Nordmar", "", "A_B"][usize::from(n % 5)];
            let ext = ["xcmsh", "XNVMSH", "xlmsh"][usize::from(n % 3)];
            format!(
                "Data\\G3_{region}_Landscape_Cell_{}.{ext}",
                String::from_utf8_lossy(body)
            )
            .into_bytes()
        }
        Target::StringTable => {
            let s = format!(
                "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\n[LocAdmin_Strings]\r\n{}\r\n",
                text()
            );
            let mut out = vec![0xFF, 0xFE];
            for u in s.encode_utf16() {
                out.extend(u.to_le_bytes());
            }
            out
        }
        Target::Quest => format!("[Quest]\r\nName=Q\r\n{}\r\n", text()).into_bytes(),
        Target::Info => format!("[Info]\r\nName=I\r\n{}\r\n", text()).into_bytes(),
        Target::Gdini => format!("[Quest]\r\n{}\r\n", text()).into_bytes(),
        Target::RemasterMesh => {
            let mut b = b"GRMO".to_vec();
            b.extend(body);
            b
        }
        Target::Dds => {
            let mut h = vec![0u8; 128];
            h[0..4].copy_from_slice(b"DDS ");
            h[4..8].copy_from_slice(&le32(124));
            h[12..16].copy_from_slice(&le32(
                4 + u32::from(body.first().copied().unwrap_or(0) % 60),
            ));
            h[16..20].copy_from_slice(&le32(4 + u32::from(body.get(1).copied().unwrap_or(0) % 60)));
            h[28..32].copy_from_slice(&le32(1 + u32::from(body.get(2).copied().unwrap_or(0) % 6)));
            h[76..80].copy_from_slice(&le32(32));
            h[80..84].copy_from_slice(&le32(4));
            h[84..88].copy_from_slice(if body.get(3).is_some_and(|x| x & 1 == 1) {
                b"DXT5"
            } else {
                b"DXT1"
            });
            h.extend(body.get(4..).unwrap_or_default());
            h
        }
        Target::PropStream => {
            let mut b = CLASS_ACCESSOR.to_vec();
            b.extend(body);
            b
        }
    }
}

/// Seed inputs for `target`: structured inputs from a few fixed patterns plus hand-built
/// valid documents where the layout is small enough to write down.
pub fn seeds(target: Target) -> Vec<Vec<u8>> {
    let patterns: [&[u8]; 5] = [
        b"",
        &[0; 64],
        &[0xff; 64],
        b"\x02\x05hello\x03abc\x01\x00\x53\x00\x00\x00\x00\x00\x00\x00\x00",
        &[
            1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0x53, 0, 4, 0, 0, 0, 1, 0, 1, 1, 0, 1,
        ],
    ];
    let mut out: Vec<Vec<u8>> = patterns.iter().map(|p| structured(target, p)).collect();
    match target {
        Target::Pak => out.push(sample_pak()),
        Target::Genome => out.push(genome(b"body", &[b"a", b"bc"])),
        Target::Quest => out.push(
            b"[Quest]\r\nName=Test_Quest\r\nType=2\r\nLogTopic=QUEST_TEST\r\nExperiencePoints=300\r\nDeliveryEntities=It_A;It_B\r\nDeliveryAmounts=1;5\r\n"
                .to_vec(),
        ),
        Target::Info => out.push(
            b"[Info]\r\nName=T1\r\nSortID=7\r\nOwner=Npc\r\nConditionType=3\r\nInfoType=3\r\nInfoScript_Commands=Say;Say\r\nInfoScript_Entities1=player;npc\r\nInfoScript_Texts=INFO_T1;INFO_T1_0\r\n"
                .to_vec(),
        ),
        _ => {}
    }
    out
}

/// A small valid G3V0 archive: one stored member and one zlib member.
pub fn sample_pak() -> Vec<u8> {
    let stored = b"hello stored".to_vec();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(b"hello compressed hello compressed");
    let zipped = z.finish().unwrap_or_default();
    let data_start = 48u64;
    let table = data_start + stored.len() as u64 + zipped.len() as u64;
    let mut out = Vec::new();
    out.extend(le32(0));
    out.extend(le32(0x3056_3347));
    out.extend([0u8; 16]);
    out.extend(table.to_le_bytes());
    out.extend(table.to_le_bytes());
    out.extend(table.to_le_bytes());
    out.extend(&stored);
    out.extend(&zipped);
    let s = |out: &mut Vec<u8>, v: &str| {
        out.extend(le32(v.len() as u32));
        out.extend(v.as_bytes());
        if !v.is_empty() {
            out.push(0);
        }
    };
    // Root directory with two files.
    out.extend([0u8; 32]);
    out.extend(le32(0x10));
    s(&mut out, "root");
    out.extend(le32(2));
    for (name, off, stored_len, size, comp) in [
        (
            "a.txt",
            data_start,
            stored.len() as u64,
            stored.len() as u64,
            0u32,
        ),
        (
            "b.txt",
            data_start + stored.len() as u64,
            zipped.len() as u64,
            33,
            2,
        ),
    ] {
        out.extend([0u8; 32]);
        out.extend(le32(0));
        out.extend(off.to_le_bytes());
        out.extend(stored_len.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(le32(0));
        out.extend(le32(comp));
        s(&mut out, name);
        s(&mut out, "");
    }
    out.extend(le32(0));
    out
}
