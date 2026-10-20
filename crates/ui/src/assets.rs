//! Runtime loading of original UI assets from the installation (never copied into the repo):
//! `Gothic3.ttf` and DDS atlases of `Data/gui.pak`. Everything degrades to the built-in font.
use anyhow::{Result, anyhow, bail, ensure};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gothic_formats::{PakArchive, normalize_path};
use std::path::Path;

#[derive(Resource, Default, Clone)]
pub struct UiAssets {
    pub font: Handle<Font>,
    /// `themes/g3/compiled/theme_g3_00.dds` (2048x2048 widget atlas).
    pub theme: Option<Handle<Image>>,
    pub original_font: bool,
}

/// Decode an uncompressed-mip DXT1/3/5 DDS (the only variant shipped in gui.pak).
pub fn dds_image(bytes: &[u8]) -> Result<Image> {
    ensure!(bytes.get(..4) == Some(b"DDS "), "not a DDS file");
    let word = |o: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes.get(o..o + 4).ok_or_else(|| anyhow!("short DDS"))?.try_into()?,
        ))
    };
    let (h, w) = (word(12)?, word(16)?);
    let format = match bytes.get(84..88) {
        Some(b"DXT1") => TextureFormat::Bc1RgbaUnormSrgb,
        Some(b"DXT3") => TextureFormat::Bc2RgbaUnormSrgb,
        Some(b"DXT5") => TextureFormat::Bc3RgbaUnormSrgb,
        other => bail!("unsupported DDS format {other:?}"),
    };
    let block = if format == TextureFormat::Bc1RgbaUnormSrgb { 8 } else { 16 };
    let size = (w.div_ceil(4) * h.div_ceil(4)) as usize * block;
    let data = bytes.get(128..128 + size).ok_or_else(|| anyhow!("short DDS payload"))?.to_vec();
    Ok(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

fn read_pak_member(pak: &mut PakArchive, member: &str) -> Result<Vec<u8>> {
    let member = normalize_path(member)?;
    let i = pak
        .entries
        .iter()
        .position(|e| e.path.eq_ignore_ascii_case(&member))
        .ok_or_else(|| anyhow!("{member} not in archive"))?;
    pak.read_payload(i)
}

/// Load what is available; returns assets that fall back to defaults on any failure.
pub fn load_install_assets(
    install: &Path,
    fonts: &mut Assets<Font>,
    images: &mut Assets<Image>,
) -> UiAssets {
    let mut out = UiAssets::default();
    match std::fs::read(install.join("Gothic3.ttf")) {
        Ok(b) => {
            out.font = fonts.add(Font::from_bytes(b));
            out.original_font = true;
        }
        Err(e) => warn!("UI: original font unavailable ({e}); using fallback"),
    }
    let theme = PakArchive::open(install.join("Data").join("gui.pak"))
        .and_then(|mut p| read_pak_member(&mut p, "themes/g3/compiled/theme_g3_00.dds"))
        .and_then(|b| dds_image(&b));
    match theme {
        Ok(img) => out.theme = Some(images.add(img)),
        Err(e) => warn!("UI: theme atlas unavailable ({e:#}); using flat style"),
    }
    out
}
