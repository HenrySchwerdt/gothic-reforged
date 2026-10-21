//! Runtime loading of original UI assets from the installation (never copied into the repo):
//! `Gothic3.ttf` and DDS atlases of `Data/gui.pak`. Everything degrades to the built-in font.
use crate::atlas::Pic;
use anyhow::{Result, anyhow, bail, ensure};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gothic_formats::igl::{Igl, bare_key};
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
    /// `g3_hud_it_<group>_00.dds`, index into [`ITEM_GROUPS`] (item icon atlases).
    Item(u8),
    /// `g3_hud_maps_NN.dds` (the world and cave map pictures, 512x512 tiles).
    Map(u8),
}

/// Item icon groups of `g3.igl` (atlas file `g3_hud_it_<name>_00.dds`).
pub const ITEM_GROUPS: [&str; 9] = [
    "alchemy",
    "armor",
    "artefacts",
    "documents",
    "misc",
    "perks",
    "recipe",
    "spells",
    "weapons",
];

/// Page of an `.igl` atlas stem such as `g3_hud_backgrounds_10`, if the UI loads it.
fn page_of_stem(stem: &str) -> Option<Page> {
    let rest = stem.strip_prefix("g3_hud_")?;
    if let Some(n) = rest.strip_prefix("backgrounds_") {
        return n.parse().ok().map(Page::Bg);
    }
    if let Some(n) = rest.strip_prefix("maps_") {
        return n.parse().ok().map(Page::Map);
    }
    if rest == "icons_00" {
        return Some(Page::Icons);
    }
    let g = rest.strip_prefix("it_")?.strip_suffix("_00")?;
    ITEM_GROUPS
        .iter()
        .position(|n| *n == g)
        .map(|i| Page::Item(i as u8))
}

impl Page {
    fn member(self) -> String {
        match self {
            Page::Bg(n) => format!("icons/compiled/g3_hud_backgrounds_{n:02}.dds"),
            Page::Icons => "icons/compiled/g3_hud_icons_00.dds".into(),
            Page::Map(n) => format!("icons/compiled/g3_hud_maps_{n:02}.dds"),
            Page::Theme => "themes/g3/compiled/theme_g3_00.dds".into(),
            Page::Item(i) => format!(
                "icons/compiled/g3_hud_it_{}_00.dds",
                ITEM_GROUPS[i as usize % 9]
            ),
        }
    }
}

/// Text and button colours of the GUI theme (`theme_g3.thm`, element `Button.Button.Back`).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub text: Color,
    pub text_hover: Color,
    pub text_disabled: Color,
    pub button: Color,
    pub button_hover: Color,
    pub button_disabled: Color,
}

impl Default for Palette {
    /// The values of the shipped theme, used until / unless the file is read.
    fn default() -> Self {
        Self {
            text: Color::srgba_u8(0xE9, 0xD9, 0xC7, 210),
            text_hover: Color::srgba_u8(0xFF, 0xEB, 0xDC, 255),
            text_disabled: Color::srgba_u8(0xE9, 0xD9, 0xC7, 128),
            button: Color::srgba_u8(0x3C, 0x28, 0x19, 200),
            button_hover: Color::srgba_u8(0x3C, 0x28, 0x19, 255),
            button_disabled: Color::srgba_u8(0x3C, 0x28, 0x19, 80),
        }
    }
}

impl Palette {
    pub fn from_theme(t: &gothic_formats::theme::Theme) -> Self {
        use gothic_formats::theme::{Rgba, State};
        let c = |v: Rgba| Color::srgba_u8(v.r, v.g, v.b, v.a);
        // element 33 = Button.Button.Back
        let Some(e) = t.elements.get(33) else {
            return Self::default();
        };
        Self {
            text: c(e.font_color(State::Normal)),
            text_hover: c(e.font_color(State::MouseOver)),
            text_disabled: c(e.font_color(State::Disabled)),
            button: c(e.texture_color(State::Normal)),
            button_hover: c(e.texture_color(State::MouseOver)),
            button_disabled: c(e.texture_color(State::Disabled)),
        }
    }
}

#[derive(Resource, Default, Clone)]
pub struct UiAssets {
    /// Colours read from the compiled theme.
    pub palette: Palette,
    /// Body font (Times New Roman Bold when installed, else `Gothic3.ttf`).
    pub font: Handle<Font>,
    /// Heading font (Times New Roman Bold Italic, else the body font).
    pub heading: Handle<Font>,
    /// Font of the floating focus names: Tahoma Bold, which the shipped game's names match
    /// (the install's `Gothic3.ttf` is only the display face of the logo); else the body font.
    pub name_font: Handle<Font>,
    pub pages: std::collections::HashMap<Page, Handle<Image>>,
    pub original_font: bool,
    /// `g3.igl` lookup (lower-cased bare image name -> page + rectangle), the
    /// `eCImageAdmin::FindImageIndex` of the original.
    pub igl: std::collections::HashMap<String, Pic>,
}

impl UiAssets {
    /// Replace a hand-recovered picture by the `g3.igl` rectangle when its key resolves and the
    /// atlas page is loaded.
    pub fn resolve(&self, pic: Pic) -> Pic {
        pic.key
            .and_then(|k| self.igl.get(k))
            .filter(|r| self.pages.contains_key(&r.page))
            .copied()
            .unwrap_or(pic)
    }

    /// Inventory icon of an item template's icon string (`gCInventoryStack::GetIconFileNames`);
    /// `grey` selects the `_Grey` variant (not-yet-learned skills/recipes) when one exists.
    pub fn item_pic(&self, icon: &str, grey: bool) -> Option<Pic> {
        let key = bare_key(icon);
        let hit = |k: &str| {
            self.igl
                .get(k)
                .filter(|p| matches!(p.page, Page::Item(_)) && self.pages.contains_key(&p.page))
                .copied()
        };
        if grey
            && let Some(stem) = key.strip_suffix(".tga")
            && let Some(p) = hit(&format!("{stem}_grey.tga"))
        {
            return Some(p);
        }
        hit(&key)
    }

    /// An `.igl` picture by file name (`g3_map_world.tga`, `g3_hud_arrow.tga`) when its atlas is
    /// loaded.
    pub fn named_pic(&self, name: &str) -> Option<Pic> {
        self.igl
            .get(&bare_key(name))
            .filter(|p| self.pages.contains_key(&p.page))
            .copied()
    }

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
    out.name_font = load(win.join("tahomabd.ttf"))
        .or_else(|| load(install.join("Gothic3.ttf")))
        .map(|f| fonts.add(f))
        .unwrap_or_else(|| out.font.clone());
    out.heading = load(win.join("timesbi.ttf"))
        .map(|f| fonts.add(f))
        .unwrap_or_else(|| out.font.clone());
    out.igl = load_igl(install);
    match PakArchive::open(install.join("Data").join("gui.pak")) {
        Ok(mut pak) => {
            if let Ok(b) = read_pak_member(&mut pak, "themes/g3/compiled/theme_g3.thm") {
                match gothic_formats::theme::Theme::parse(&b) {
                    Ok(t) => out.palette = Palette::from_theme(&t),
                    Err(e) => warn!("UI: theme_g3.thm unreadable ({e:#})"),
                }
            }
            let mut pages = (1..=6).chain(8..=17).map(Page::Bg).collect::<Vec<_>>();
            pages.extend([Page::Icons, Page::Theme]);
            pages.extend((0..4).map(Page::Map));
            pages.extend((0..ITEM_GROUPS.len() as u8).map(Page::Item));
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

/// Parse `g3.igl` of `Library.pak` into the lookup map (empty on failure).
pub fn load_igl(install: &Path) -> std::collections::HashMap<String, Pic> {
    let read = || -> Result<Igl> {
        let mut pak = PakArchive::open(install.join("Data").join("Library.pak"))?;
        Igl::parse(&read_pak_member(&mut pak, "g3.igl")?)
    };
    match read() {
        Ok(igl) => igl_pics(&igl),
        Err(e) => {
            warn!("UI: g3.igl unavailable ({e:#}); using hand-recovered rectangles");
            Default::default()
        }
    }
}

/// Map every tile whose atlas the UI can load to a [`Pic`]. Duplicated names keep the first.
pub fn igl_pics(igl: &Igl) -> std::collections::HashMap<String, Pic> {
    let mut m = std::collections::HashMap::new();
    for g in &igl.groups {
        for t in &g.tiles {
            let Some(page) = page_of_stem(&t.texture_stem()) else {
                continue;
            };
            let [l, tp, r, b] = t.rect.map(|v| v as f32);
            m.entry(t.key()).or_insert(Pic {
                page,
                x: l,
                y: tp,
                w: r - l,
                h: b - tp,
                key: None,
            });
        }
    }
    m
}
