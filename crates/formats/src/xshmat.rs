//! Bounded, independent XSHMAT (eCResourceShaderMaterial_PS) reader.
//!
//! Two storage variants exist in the installed game: a GENOMFLE-wrapped one whose
//! property names/types/strings are u16 indices into a trailing string table, and a
//! raw one (no envelope) whose strings are inline `u16 length + bytes`. In both, a
//! property record is `name, type, u16 version(30), u32 payload size, payload`.
//! The full object tree is not decoded. Instead the body is scanned for property
//! records with known names; each candidate must match name, type, version and a
//! payload size that fits the body, so unrelated bytes are very unlikely to match.
//! See `docs/research/XSHMAT_FORMAT.md` for evidence and limits.
pub mod graph;

use crate::genome::GenomeFile;
use anyhow::{Result, ensure};
use serde::Serialize;

const MAX_TEXTURES: usize = 64;
const PROPERTY_VERSION: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TextureRole {
    Diffuse,
    Normal,
    Specular,
    Detail,
    Environment,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextureRef {
    /// Source path as authored, usually a `.tga` file name, sometimes with a directory.
    pub path: String,
    pub role: TextureRole,
}

impl TextureRef {
    /// Final path component without extension, e.g. `G3_Architecture_Wood_08_Diffuse_01`.
    pub fn stem(&self) -> &str {
        let name = self.path.rsplit(['/', '\\']).next().unwrap_or_default();
        name.rsplit_once('.').map_or(name, |(stem, _)| stem)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Xshmat {
    /// Samplers in file order.
    pub textures: Vec<TextureRef>,
    /// `eCShaderWater` found as a bounded object class in the material tree. This identifies the
    /// authored shader family; it does not imply a visible surface by itself.
    pub water_shader: bool,
    /// Raw `BlendMode` enum value (eEShaderMaterialBlendMode); symbolic names unverified.
    pub blend_mode: Option<u32>,
    /// Raw `MaskReference` byte (alpha-test reference).
    pub mask_reference: Option<u8>,
    pub enable_specular: Option<bool>,
    pub disable_lighting: Option<bool>,
}

impl Xshmat {
    pub fn texture(&self, role: TextureRole) -> Option<&TextureRef> {
        self.textures.iter().find(|t| t.role == role)
    }
    pub fn diffuse(&self) -> Option<&TextureRef> {
        self.texture(TextureRole::Diffuse)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 4 * 1024 * 1024,
            "XSHMAT exceeds 4 MiB ceiling"
        );
        let genome = if bytes.starts_with(b"GENOMFLE") {
            Some(GenomeFile::parse(bytes)?)
        } else {
            None
        };
        let (body, table) = match &genome {
            Some(g) => (g.body, Some(g.strings.as_slice())),
            None => (bytes, None),
        };
        let mut scanner = Scanner {
            body,
            table,
            out: Xshmat::default(),
            seen_class: false,
        };
        scanner.check_class()?;
        scanner.out.water_shader =
            graph::contains_object_class(body, scanner.table, b"eCShaderWater");
        scanner.scan()?;
        scanner.out.assign_roles();
        Ok(scanner.out)
    }

    fn assign_roles(&mut self) {
        let mut have_diffuse = false;
        for texture in &mut self.textures {
            let name = texture.stem().to_ascii_lowercase();
            texture.role = if name.contains("normal") {
                TextureRole::Normal
            } else if name.contains("specular") {
                TextureRole::Specular
            } else if name.contains("detail") {
                TextureRole::Detail
            } else if name.contains("cubemap") {
                TextureRole::Environment
            } else if !have_diffuse {
                have_diffuse = true;
                TextureRole::Diffuse
            } else {
                TextureRole::Other
            };
            if texture.role == TextureRole::Diffuse {
                have_diffuse = true;
            }
        }
    }
}

struct Scanner<'a> {
    body: &'a [u8],
    table: Option<&'a [&'a [u8]]>,
    out: Xshmat,
    seen_class: bool,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Known {
    ImageFilePath,
    BlendMode,
    MaskReference,
    EnableSpecular,
    DisableLighting,
}

const KNOWN: [(&[u8], &[u8], Known); 5] = [
    (
        b"ImageFilePath",
        b"bCImageResourceString",
        Known::ImageFilePath,
    ),
    (
        b"BlendMode",
        b"bTPropertyContainer<enum eEShaderMaterialBlendMode>",
        Known::BlendMode,
    ),
    (b"MaskReference", b"char", Known::MaskReference),
    (b"EnableSpecular", b"bool", Known::EnableSpecular),
    (b"DisableLighting", b"bool", Known::DisableLighting),
];

impl Scanner<'_> {
    fn u16_at(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            self.body.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    }
    fn u32_at(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.body.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    }

    fn check_class(&mut self) -> Result<()> {
        const CLASS: &[u8] = b"eCResourceShaderMaterial_PS";
        let found = match self.table {
            Some(table) => table.contains(&CLASS),
            None => self
                .body
                .windows(CLASS.len())
                .take(64)
                .any(|window| window == CLASS),
        };
        ensure!(found, "not an eCResourceShaderMaterial_PS resource");
        self.seen_class = true;
        Ok(())
    }

    /// Resolve a string reference at `at`; returns (bytes, next offset).
    fn string_at(&self, at: usize) -> Option<(&[u8], usize)> {
        let value = usize::from(self.u16_at(at)?);
        match self.table {
            Some(table) => Some((*table.get(value)?, at + 2)),
            None => {
                let end = (at + 2).checked_add(value)?;
                Some((self.body.get(at + 2..end)?, end))
            }
        }
    }

    fn scan(&mut self) -> Result<()> {
        for start in 0..self.body.len() {
            let Some((name, after_name)) = self.string_at(start) else {
                continue;
            };
            let Some(&(_, type_name, kind)) = KNOWN.iter().find(|(n, _, _)| *n == name) else {
                continue;
            };
            let Some((ty, after_type)) = self.string_at(after_name) else {
                continue;
            };
            if ty != type_name || self.u16_at(after_type) != Some(PROPERTY_VERSION) {
                continue;
            }
            let Some(size) = self.u32_at(after_type + 2) else {
                continue;
            };
            let payload_start = after_type + 6;
            let Some(payload) = self
                .body
                .get(payload_start..payload_start.saturating_add(size as usize))
            else {
                continue;
            };
            self.accept(kind, payload)?;
        }
        Ok(())
    }

    fn accept(&mut self, kind: Known, payload: &[u8]) -> Result<()> {
        match kind {
            Known::ImageFilePath => {
                // The payload is itself a string reference (index or inline).
                let sub = Scanner {
                    body: payload,
                    table: self.table,
                    out: Xshmat::default(),
                    seen_class: true,
                };
                if let Some((text, end)) = sub.string_at(0)
                    && end == payload.len()
                    && !text.is_empty()
                {
                    ensure!(
                        self.out.textures.len() < MAX_TEXTURES,
                        "XSHMAT has too many samplers"
                    );
                    self.out.textures.push(TextureRef {
                        path: text.iter().map(|&b| char::from(b)).collect(),
                        role: TextureRole::Other,
                    });
                }
            }
            Known::BlendMode => {
                if payload.len() == 6 {
                    self.out.blend_mode = Some(u32::from_le_bytes(
                        payload[2..6].try_into().unwrap_or_default(),
                    ));
                }
            }
            Known::MaskReference => {
                if let [value] = payload {
                    self.out.mask_reference = Some(*value);
                }
            }
            Known::EnableSpecular => {
                if let [value @ (0 | 1)] = payload {
                    self.out.enable_specular = Some(*value == 1);
                }
            }
            Known::DisableLighting => {
                if let [value @ (0 | 1)] = payload {
                    self.out.disable_lighting = Some(*value == 1);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_string(out: &mut Vec<u8>, text: &[u8]) {
        out.extend((text.len() as u16).to_le_bytes());
        out.extend(text);
    }
    fn raw_property(out: &mut Vec<u8>, name: &[u8], ty: &[u8], payload: &[u8]) {
        raw_string(out, name);
        raw_string(out, ty);
        out.extend(PROPERTY_VERSION.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(payload);
    }
    fn sampler(out: &mut Vec<u8>, path: &[u8]) {
        let mut payload = Vec::new();
        raw_string(&mut payload, path);
        raw_property(out, b"ImageFilePath", b"bCImageResourceString", &payload);
        out.extend([0, 0, 7, 7]); // unrelated bytes between records
    }
    fn raw_material() -> Vec<u8> {
        let mut b = vec![1, 0, 1, 1, 0, 1];
        raw_string(&mut b, b"eCResourceShaderMaterial_PS");
        raw_property(&mut b, b"DisableLighting", b"bool", &[0]);
        raw_property(&mut b, b"MaskReference", b"char", &[128]);
        raw_property(
            &mut b,
            b"BlendMode",
            b"bTPropertyContainer<enum eEShaderMaterialBlendMode>",
            &[1, 0, 2, 0, 0, 0],
        );
        sampler(&mut b, b"Data/Materials/Images/Test_Wall_Diffuse_01.tga");
        sampler(&mut b, b"Test_Wall_Normal_01.tga");
        b
    }

    #[test]
    fn parses_raw_variant_with_roles() -> Result<()> {
        let m = Xshmat::parse(&raw_material())?;
        assert_eq!(m.textures.len(), 2);
        let d = m.diffuse().unwrap();
        assert_eq!(d.stem(), "Test_Wall_Diffuse_01");
        assert_eq!(
            m.texture(TextureRole::Normal).unwrap().stem(),
            "Test_Wall_Normal_01"
        );
        assert_eq!(m.blend_mode, Some(2));
        assert_eq!(m.mask_reference, Some(128));
        assert_eq!(m.disable_lighting, Some(false));
        assert_eq!(m.enable_specular, None);
        Ok(())
    }

    #[test]
    fn parses_genome_variant_via_string_table() -> Result<()> {
        let strings: [&[u8]; 6] = [
            b"eCResourceShaderMaterial_PS",
            b"ImageFilePath",
            b"bCImageResourceString",
            b"Synthetic_Diffuse_01.tga",
            b"DisableLighting",
            b"bool",
        ];
        let mut body = vec![1, 0, 1, 1, 0, 1, 0, 0];
        for (name, ty, payload) in [(1u16, 2u16, &3u16.to_le_bytes()[..]), (4, 5, &[1u8][..])] {
            body.extend(name.to_le_bytes());
            body.extend(ty.to_le_bytes());
            body.extend(PROPERTY_VERSION.to_le_bytes());
            body.extend((payload.len() as u32).to_le_bytes());
            body.extend(payload);
        }
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        let table = 14 + body.len();
        b.extend((table as u32).to_le_bytes());
        b.extend(&body);
        b.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s);
        }
        let m = Xshmat::parse(&b)?;
        assert_eq!(m.diffuse().unwrap().path, "Synthetic_Diffuse_01.tga");
        assert_eq!(m.disable_lighting, Some(true));
        Ok(())
    }

    #[test]
    fn rejects_other_resources_and_survives_truncation() {
        assert!(Xshmat::parse(b"").is_err());
        assert!(Xshmat::parse(&[0u8; 200]).is_err());
        let b = raw_material();
        for n in 0..b.len() {
            let _ = Xshmat::parse(&b[..n]); // must not panic
        }
    }

    #[test]
    fn second_unclassified_sampler_is_not_diffuse() -> Result<()> {
        let mut b = vec![1, 0, 1, 1, 0, 1];
        raw_string(&mut b, b"eCResourceShaderMaterial_PS");
        sampler(&mut b, b"A_01.tga");
        sampler(&mut b, b"B_01.tga");
        let m = Xshmat::parse(&b)?;
        assert_eq!(m.textures[0].role, TextureRole::Diffuse);
        assert_eq!(m.textures[1].role, TextureRole::Other);
        Ok(())
    }

    #[test]
    fn identifies_exact_water_shader_without_inventing_a_sampler() -> Result<()> {
        let mut depth_material = vec![1, 0, 1, 1, 0, 1];
        raw_string(&mut depth_material, b"eCResourceShaderMaterial_PS");
        depth_material.extend([1, 0, 1, 1, 0, 1]);
        raw_string(&mut depth_material, b"eCShaderWater");
        depth_material.extend([0; 5]);
        depth_material.extend(0x53u16.to_le_bytes());
        depth_material.extend(6u32.to_le_bytes());
        depth_material.extend(30u16.to_le_bytes());
        depth_material.extend(0u32.to_le_bytes());
        let parsed = Xshmat::parse(&depth_material)?;
        assert!(parsed.water_shader);
        assert!(parsed.diffuse().is_none());
        assert!(parsed.texture(TextureRole::Normal).is_none());

        let mut unrelated = vec![1, 0, 1, 1, 0, 1];
        raw_string(&mut unrelated, b"eCResourceShaderMaterial_PS");
        raw_string(&mut unrelated, b"eCShaderWaterLike");
        assert!(!Xshmat::parse(&unrelated)?.water_shader);
        Ok(())
    }
}

#[cfg(test)]
mod survey {
    use super::*;
    /// Real-data survey; run with `GOTHIC_MATERIAL_PAK=<path> cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn survey_installed_materials() {
        let Some(path) = std::env::var_os("GOTHIC_MATERIAL_PAK") else {
            return;
        };
        let mut pak = crate::PakArchive::open(path).unwrap();
        let (mut ok, mut failed, mut no_diffuse) = (0, 0, 0);
        let mut blends = std::collections::BTreeMap::<Option<u32>, usize>::new();
        let mut masks = std::collections::BTreeMap::<Option<u8>, usize>::new();
        for index in 0..pak.entries.len() {
            let entry = &pak.entries[index];
            if !entry.path.to_ascii_lowercase().ends_with(".xshmat") {
                continue;
            }
            let name = entry.path.clone();
            let bytes = pak.read_payload(index).unwrap();
            match Xshmat::parse(&bytes) {
                Ok(m) => {
                    ok += 1;
                    if m.diffuse().is_none() {
                        no_diffuse += 1;
                        println!("NO DIFFUSE {name}: {:?}", m.textures);
                    }
                    let seen = blends.entry(m.blend_mode).or_default();
                    *seen += 1;
                    if *seen <= 5 && m.blend_mode != Some(0) {
                        println!(
                            "BLEND {:?} mask {:?} {name}",
                            m.blend_mode, m.mask_reference
                        );
                    }
                    *masks.entry(m.mask_reference).or_default() += 1;
                    if name.contains("Plant_Herb_13") || name.contains("Wood_08_A") {
                        println!("{name}: {m:?}");
                    }
                }
                Err(e) => {
                    failed += 1;
                    println!("FAIL {name}: {e:#}");
                }
            }
        }
        println!(
            "ok {ok} failed {failed} no_diffuse {no_diffuse}\nblend {blends:?}\nmask {masks:?}"
        );
    }
}
