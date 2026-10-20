//! Cooked textures (`.gtex`): header, mip table, one data blob. Block-compressed payloads
//! are uploadable as-is (BC1/2/3 on desktop, ASTC 6x6 for the quest profile).
use crate::container::{Container, Kind, Writer};
use crate::ids::{Profile, TextureMode};
use crate::mmap::Blob;
use anyhow::{Result, bail, ensure};
use bytemuck::{Pod, Zeroable};
use gothic_formats::ximg::Ximg;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TexFormat {
    Bc1 = 1,
    Bc2 = 2,
    Bc3 = 3,
    Astc6x6 = 10,
    Rgba8 = 20,
}

impl TexFormat {
    fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            1 => Self::Bc1,
            2 => Self::Bc2,
            3 => Self::Bc3,
            10 => Self::Astc6x6,
            20 => Self::Rgba8,
            _ => return None,
        })
    }
}

pub const TEX_SRGB: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct TexHeader {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub mip_count: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub flags: u32,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MipRec {
    pub offset: u32,
    pub len: u32,
    pub width: u32,
    pub height: u32,
}

pub struct CookedTexture {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Cook one XIMG payload for `profile`. `srgb` selects the colour space for ASTC encoding.
pub fn cook_texture(src: &[u8], profile: &Profile, srgb: bool, key: u64) -> Result<CookedTexture> {
    let img = Ximg::parse(src)?;
    let (src_fmt, block) = match &img.fourcc {
        b"DXT1" => (TexFormat::Bc1, 8usize),
        b"DXT3" => (TexFormat::Bc2, 16),
        b"DXT5" => (TexFormat::Bc3, 16),
        other => bail!("unsupported fourcc {other:?}"),
    };
    let mips = img.mips();
    let dims = |level: usize| ((img.width >> level).max(1), (img.height >> level).max(1));
    let cap = match profile.texture {
        TextureMode::Bc { max_dim } => max_dim,
        TextureMode::Astc6x6 { max_dim } => Some(max_dim),
    };
    let mut first = 0usize;
    if let Some(cap) = cap {
        while first + 1 < mips.len() && {
            let (w, h) = dims(first);
            w.max(h) > cap
        } {
            first += 1;
        }
    }
    let mut data: Vec<u8> = Vec::new();
    let mut recs: Vec<MipRec> = Vec::new();
    let format;
    match profile.texture {
        TextureMode::Bc { .. } => {
            format = src_fmt;
            for (level, m) in mips.iter().enumerate().skip(first) {
                let (w, h) = dims(level);
                ensure!(
                    m.len() == (w.div_ceil(4) * h.div_ceil(4)) as usize * block,
                    "mip size"
                );
                recs.push(MipRec {
                    offset: data.len() as u32,
                    len: m.len() as u32,
                    width: w,
                    height: h,
                });
                data.extend_from_slice(m);
            }
        }
        TextureMode::Astc6x6 { .. } => {
            format = TexFormat::Astc6x6;
            let mut enc = astc::Encoder::new(srgb)?;
            for (level, m) in mips.iter().enumerate().skip(first) {
                let (w, h) = dims(level);
                let rgba = decode_bc(src_fmt, m, w as usize, h as usize)?;
                let blocks = enc.encode(&rgba, w, h)?;
                recs.push(MipRec {
                    offset: data.len() as u32,
                    len: blocks.len() as u32,
                    width: w,
                    height: h,
                });
                data.extend_from_slice(&blocks);
            }
        }
    }
    if profile.remaster_hook {
        crate::remaster::on_texture(&img, &mut data);
    }
    let (w, h) = dims(first);
    let header = TexHeader {
        width: w,
        height: h,
        format: format as u32,
        mip_count: recs.len() as u32,
        source_width: img.width,
        source_height: img.height,
        flags: if srgb { TEX_SRGB } else { 0 },
        _pad: 0,
    };
    let mut wr = Writer::new();
    wr.one(&header).slice(&recs).raw(data);
    Ok(CookedTexture {
        bytes: wr.finish(Kind::Texture, key),
        width: w,
        height: h,
    })
}

/// Decode BC1/2/3 blocks to tightly packed RGBA8.
pub fn decode_bc(fmt: TexFormat, blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    let mut px = vec![0u32; w * h];
    match fmt {
        TexFormat::Bc1 => texture2ddecoder::decode_bc1(blocks, w, h, &mut px),
        TexFormat::Bc2 => texture2ddecoder::decode_bc2(blocks, w, h, &mut px),
        TexFormat::Bc3 => texture2ddecoder::decode_bc3(blocks, w, h, &mut px),
        _ => bail!("not a BC format"),
    }
    .map_err(|e| anyhow::anyhow!("decode: {e}"))?;
    // texture2ddecoder packs B,G,R,A little-endian.
    Ok(px
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect())
}

#[cfg(feature = "astc")]
mod astc {
    use super::*;
    use ctt_astcenc::{Context, Flags, Preset, Profile as P, Swizzle, bindings};

    pub struct Encoder {
        ctx: Context,
    }
    impl Encoder {
        pub fn new(srgb: bool) -> Result<Self> {
            #[cfg(all(windows, target_env = "gnu"))]
            std::hint::black_box(crate::shim::keep_linked());
            let cfg = ctt_astcenc::config_init(
                if srgb { P::LdrSrgb } else { P::Ldr },
                6,
                6,
                1,
                Preset::Fast,
                Flags::empty(),
            )
            .map_err(|e| anyhow::anyhow!("astcenc config: {e:?}"))?;
            let ctx = Context::new(&cfg).map_err(|e| anyhow::anyhow!("astcenc context: {e:?}"))?;
            Ok(Self { ctx })
        }
        pub fn encode(&mut self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
            let mut buf = rgba.to_vec();
            let mut slice_ptr: *mut std::ffi::c_void = buf.as_mut_ptr().cast();
            let mut image = bindings::astcenc_image {
                dim_x: w,
                dim_y: h,
                dim_z: 1,
                data_type: bindings::astcenc_type_ASTCENC_TYPE_U8,
                data: &mut slice_ptr,
            };
            let mut out = vec![0u8; (w.div_ceil(6) * h.div_ceil(6)) as usize * 16];
            self.ctx
                .compress(&mut image, Swizzle::IDENTITY, &mut out)
                .map_err(|e| anyhow::anyhow!("astcenc compress: {e:?}"))?;
            self.ctx
                .compress_reset()
                .map_err(|e| anyhow::anyhow!("astcenc reset: {e:?}"))?;
            Ok(out)
        }
    }
}

#[cfg(not(feature = "astc"))]
mod astc {
    use anyhow::{Result, bail};
    pub struct Encoder;
    impl Encoder {
        pub fn new(_srgb: bool) -> Result<Self> {
            bail!("the quest texture profile needs gothic-cook's `astc` feature (ctt-astcenc)")
        }
        pub fn encode(&mut self, _: &[u8], _: u32, _: u32) -> Result<Vec<u8>> {
            unreachable!()
        }
    }
}

/// Zero-copy view over a cooked texture file.
#[derive(Clone)]
pub struct TextureData {
    c: Container,
}

impl TextureData {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Texture)?;
        ensure!(c.section_count() == 3, "texture section count");
        let t = Self { c };
        let h = t.header();
        ensure!(
            TexFormat::from_u32(h.format).is_some() && t.mips().len() == h.mip_count as usize,
            "texture header"
        );
        for m in t.mips() {
            ensure!(
                (m.offset as usize + m.len as usize) <= t.c.raw(2).len(),
                "mip out of range"
            );
        }
        Ok(t)
    }
    pub fn header(&self) -> TexHeader {
        self.c.one(0).unwrap_or_else(Zeroable::zeroed)
    }
    pub fn format(&self) -> TexFormat {
        TexFormat::from_u32(self.header().format).unwrap_or(TexFormat::Rgba8)
    }
    pub fn mips(&self) -> &[MipRec] {
        self.c.slice(1)
    }
    pub fn mip_data(&self, level: usize) -> Option<&[u8]> {
        let m = self.mips().get(level)?;
        self.c
            .raw(2)
            .get(m.offset as usize..(m.offset + m.len) as usize)
    }
    /// All mip data back to back (what a GPU upload with a mip table consumes).
    pub fn data(&self) -> &[u8] {
        self.c.raw(2)
    }
}
