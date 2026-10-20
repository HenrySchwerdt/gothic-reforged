//! Runtime loading of original UI assets from the installation (never copied into the repo):
//! `Gothic3.ttf` and DDS atlases of `Data/gui.pak`. Everything degrades to the built-in font.
use anyhow::{Result, anyhow, bail, ensure};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gothic_formats::{PakArchive, normalize_path};
use std::path::Path;

/// Atlas pages of `gui.pak` used by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    /// `g3_hud_backgrounds_NN.dds` (view panels, 9..=17 are the in-game ones).
    Bg(u8),
    /// `g3_hud_icons_00.dds` (compass, tab glyphs, map markers).
    Icons,
    /// `theme_g3_00.dds` (generic widget theme, cursor).
    Theme,
}

impl Page {
    fn member(self) -> String {
        match self {
            Page::Bg(n) => format!("icons/compiled/g3_hud_backgrounds_{n:02}.dds"),
            Page::Icons => "icons/compiled/g3_hud_icons_00.dds".into(),
            Page::Theme => "themes/g3/compiled/theme_g3_00.dds".into(),
        }
    }
}

#[derive(Resource, Default, Clone)]
pub struct UiAssets {
    /// Body font (Times New Roman Bold when installed, else `Gothic3.ttf`).
    pub font: Handle<Font>,
    /// Heading font (Times New Roman Bold Italic, else the body font).
    pub heading: Handle<Font>,
    pub pages: std::collections::HashMap<Page, Handle<Image>>,
    pub original_font: bool,
}

impl UiAssets {
    pub fn page(&self, p: Page) -> Option<&Handle<Image>> {
        self.pages.get(&p)
    }
}

/// Decode an uncompressed-mip DXT1/3/5 DDS (the only variant shipped in gui.pak).
pub fn dds_image(bytes: &[u8]) -> Result<Image> {
    ensure!(bytes.get(..4) == Some(b"DDS "), "not a DDS file");
    let word = |o: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(o..o + 4)
                .ok_or_else(|| anyhow!("short DDS"))?
                .try_into()?,
        ))
    };
    let (h, w) = (word(12)?, word(16)?);
    let format = match bytes.get(84..88) {
        Some(b"DXT1") => TextureFormat::Bc1RgbaUnormSrgb,
        Some(b"DXT3") => TextureFormat::Bc2RgbaUnormSrgb,
        Some(b"DXT5") => TextureFormat::Bc3RgbaUnormSrgb,
        other => bail!("unsupported DDS format {other:?}"),
    };
    let block = if format == TextureFormat::Bc1RgbaUnormSrgb {
        8
    } else {
        16
    };
    let size = (w.div_ceil(4) * h.div_ceil(4)) as usize * block;
    let data = bytes
        .get(128..128 + size)
        .ok_or_else(|| anyhow!("short DDS payload"))?
        .to_vec();
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
    // The original UI resources ask for Times New Roman (GUI.DefaultFont in ge3.INI is
    // user-configurable); use the system copy when present, else the installation's font.
    let win = Path::new("C:/Windows/Fonts");
    let load = |p: std::path::PathBuf| std::fs::read(p).ok().map(Font::from_bytes);
    let body = load(win.join("timesbd.ttf")).or_else(|| load(install.join("Gothic3.ttf")));
    match body {
        Some(f) => {
            out.font = fonts.add(f);
            out.original_font = true;
        }
        None => warn!("UI: no TrueType font available; using the built-in fallback"),
    }
    out.heading = load(win.join("timesbi.ttf"))
        .map(|f| fonts.add(f))
        .unwrap_or_else(|| out.font.clone());
    match PakArchive::open(install.join("Data").join("gui.pak")) {
        Ok(mut pak) => {
            let mut pages = (9..=17).map(Page::Bg).collect::<Vec<_>>();
            pages.extend([Page::Icons, Page::Theme]);
            for p in pages {
                match read_pak_member(&mut pak, &p.member()).and_then(|b| dds_image(&b)) {
                    Ok(img) => {
                        out.pages.insert(p, images.add(img));
                    }
                    Err(e) => warn!("UI: {p:?} unavailable ({e:#})"),
                }
            }
        }
        Err(e) => warn!("UI: gui.pak unavailable ({e:#}); using flat style"),
    }
    out
}
