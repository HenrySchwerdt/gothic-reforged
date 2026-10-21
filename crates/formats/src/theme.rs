//! Bounded reader for the compiled GUI theme `theme_g3.thm` (`eCThemeAdmin::ReadCompiled`).
//!
//! Layout (see `docs/research/UI_ORIGINAL.md`): a `GENOMFLE` container (`u16` version, `u32`
//! footer offset), a main stream of metrics, cursors and elements, and a footer string table
//! (`0xDEADBEEF`, flag byte, `u32` count, `u16 len` + bytes per string). Only the numbers a
//! port needs are kept: metrics, per-element atlas rectangle, text format and the six state
//! colour pairs. No original content is embedded.
use anyhow::{Result, bail, ensure};

pub const METRIC_COUNT: usize = 58;
pub const CURSOR_COUNT: usize = 7;
pub const ELEMENT_COUNT: usize = 163;
const STATES: usize = 6;
const MAX_STRINGS: usize = 4096;

/// Widget state order of the colour pairs in an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Disabled,
    Hidden,
    Focus,
    MouseOver,
    Pressed,
}

/// `bCByteAlphaColor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// Source rectangle in the atlas (left, top, right, bottom).
    pub rect: [i32; 4],
    pub tint: Rgba,
    pub hotspot: (i32, i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub rect: [i32; 4],
    /// Win32 `DT_*` text format flags.
    pub format: u32,
    /// Per state: (font colour, texture colour).
    pub colors: [(Rgba, Rgba); STATES],
    /// Font height in pixels (negative = em height) when the element carries a font.
    pub font_height: Option<i32>,
}

impl Element {
    pub fn font_color(&self, s: State) -> Rgba {
        self.colors[s as usize].0
    }
    pub fn texture_color(&self, s: State) -> Rgba {
        self.colors[s as usize].1
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub metrics: [i32; METRIC_COUNT],
    pub cursors: Vec<Cursor>,
    pub elements: Vec<Element>,
}

/// Index of `Static.Text` in the element table (body text colour).
pub const ELEMENT_STATIC_TEXT: usize = 32;

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len());
        let Some(end) = end else {
            bail!("thm: truncated at {:#x}", self.p)
        };
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn color(&mut self) -> Result<Rgba> {
        self.take(4)?; // vtable pointer, ignored
        let c = self.take(4)?;
        let (r, g, b) = (c[0], c[1], c[2]);
        let a = self.u32()?.min(255) as u8;
        Ok(Rgba { r, g, b, a })
    }
}

impl Theme {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() > 14 && &bytes[..8] == b"GENOMFLE",
            "thm: bad magic"
        );
        let footer = u32::from_le_bytes(bytes[10..14].try_into()?) as usize;
        ensure!(footer + 9 <= bytes.len(), "thm: footer outside file");
        let strings = read_strings(&bytes[footer..])?;
        let mut c = Cur {
            b: &bytes[..footer],
            p: 14,
        };
        ensure!(c.u32()? == 1, "thm: theme version");
        let name = string(&strings, c.u16()?)?;
        c.i32()?; // legacy group index
        ensure!(c.u32()? == 1, "thm: metrics version");
        ensure!(c.u32()? as usize == METRIC_COUNT, "thm: metric count");
        let mut metrics = [0; METRIC_COUNT];
        for m in &mut metrics {
            *m = c.i32()?;
        }
        ensure!(c.u32()? as usize == CURSOR_COUNT, "thm: cursor count");
        let mut cursors = Vec::with_capacity(CURSOR_COUNT);
        for _ in 0..CURSOR_COUNT {
            ensure!(c.u32()? == 1, "thm: cursor version");
            c.u16()?; // texture string
            let rect = [c.i32()?, c.i32()?, c.i32()?, c.i32()?];
            let tint = c.color()?;
            let hotspot = (c.i32()?, c.i32()?);
            cursors.push(Cursor {
                rect,
                tint,
                hotspot,
            });
        }
        ensure!(c.u32()? as usize == ELEMENT_COUNT, "thm: element count");
        let mut elements = Vec::with_capacity(ELEMENT_COUNT);
        for _ in 0..ELEMENT_COUNT {
            ensure!(c.u32()? == 2, "thm: element version");
            c.u16()?; // source bitmap
            c.u16()?; // atlas
            let rect = [c.i32()?, c.i32()?, c.i32()?, c.i32()?];
            let font_ref = c.i32()?;
            let format = c.u32()?;
            ensure!(c.u32()? as usize == STATES, "thm: state count");
            let mut colors = [(Rgba::default(), Rgba::default()); STATES];
            for s in &mut colors {
                *s = (c.color()?, c.color()?);
            }
            let font_height = if font_ref >= 0 {
                ensure!(c.u32()? == 1, "thm: font version");
                let len = c.u16()? as usize;
                c.take(len)?;
                let height = c.i32()?;
                c.take(7 * 4 + 1)?; // width..mipLevels, italic
                Some(height)
            } else {
                None
            };
            elements.push(Element {
                rect,
                format,
                colors,
                font_height,
            });
        }
        ensure!(c.p == footer, "thm: stream does not end at the footer");
        Ok(Self {
            name,
            metrics,
            cursors,
            elements,
        })
    }
}

fn read_strings(f: &[u8]) -> Result<Vec<String>> {
    ensure!(
        f[..4] == 0xDEAD_BEEF_u32.to_le_bytes(),
        "thm: footer marker"
    );
    ensure!(f[4] == 1, "thm: inline strings unsupported");
    let n = u32::from_le_bytes(f[5..9].try_into()?) as usize;
    ensure!(n <= MAX_STRINGS, "thm: string count");
    let mut c = Cur { b: f, p: 9 };
    (0..n)
        .map(|_| {
            let l = c.u16()? as usize;
            Ok(c.take(l)?.iter().map(|&b| b as char).collect())
        })
        .collect()
}

fn string(t: &[String], i: u16) -> Result<String> {
    t.get(i as usize)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("thm: string index {i}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(out: &mut Vec<u8>, r: u8, g: u8, b: u8, a: u32) {
        out.extend_from_slice(&0x101B_0088u32.to_le_bytes());
        out.extend_from_slice(&[r, g, b, 0]);
        out.extend_from_slice(&a.to_le_bytes());
    }

    /// Hand-built theme: 7 cursors, 163 elements, one of them with a font.
    fn synthetic() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&1u32.to_le_bytes());
        s.extend_from_slice(&0u16.to_le_bytes());
        s.extend_from_slice(&10i32.to_le_bytes());
        s.extend_from_slice(&1u32.to_le_bytes());
        s.extend_from_slice(&(METRIC_COUNT as u32).to_le_bytes());
        for i in 0..METRIC_COUNT as i32 {
            s.extend_from_slice(&i.to_le_bytes());
        }
        s.extend_from_slice(&(CURSOR_COUNT as u32).to_le_bytes());
        for _ in 0..CURSOR_COUNT {
            s.extend_from_slice(&1u32.to_le_bytes());
            s.extend_from_slice(&1u16.to_le_bytes());
            for v in [1i32, 2, 3, 4] {
                s.extend_from_slice(&v.to_le_bytes());
            }
            color(&mut s, 255, 255, 255, 255);
            s.extend_from_slice(&[0; 8]);
        }
        s.extend_from_slice(&(ELEMENT_COUNT as u32).to_le_bytes());
        for i in 0..ELEMENT_COUNT {
            s.extend_from_slice(&2u32.to_le_bytes());
            s.extend_from_slice(&1u16.to_le_bytes());
            s.extend_from_slice(&1u16.to_le_bytes());
            for v in [10i32, 20, 30, 40] {
                s.extend_from_slice(&v.to_le_bytes());
            }
            let font = i == ELEMENT_STATIC_TEXT;
            s.extend_from_slice(&(if font { 26i32 } else { -1 }).to_le_bytes());
            s.extend_from_slice(&5u32.to_le_bytes());
            s.extend_from_slice(&6u32.to_le_bytes());
            for _ in 0..6 {
                color(&mut s, 0xE0, 0xC4, 0x95, 255);
                color(&mut s, 1, 2, 3, 10);
            }
            if font {
                s.extend_from_slice(&1u32.to_le_bytes());
                s.extend_from_slice(&4u16.to_le_bytes());
                s.extend_from_slice(&[b'A', 0, b'B', 0]);
                s.extend_from_slice(&(-16i32).to_le_bytes());
                s.extend_from_slice(&[0; 29]);
            }
        }
        let footer = 14 + s.len();
        let mut out = b"GENOMFLE".to_vec();
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&(footer as u32).to_le_bytes());
        out.extend_from_slice(&s);
        out.extend_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        out.push(1);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(b"g3");
        out
    }

    #[test]
    fn parses_synthetic_theme() {
        let t = Theme::parse(&synthetic()).unwrap();
        assert_eq!(t.name, "g3");
        assert_eq!(t.metrics[57], 57);
        assert_eq!(t.cursors.len(), CURSOR_COUNT);
        assert_eq!(t.elements.len(), ELEMENT_COUNT);
        let e = &t.elements[ELEMENT_STATIC_TEXT];
        assert_eq!(e.font_height, Some(-16));
        assert_eq!(
            e.font_color(State::Normal),
            Rgba {
                r: 0xE0,
                g: 0xC4,
                b: 0x95,
                a: 255
            }
        );
        assert_eq!(e.texture_color(State::Pressed).a, 10);
    }

    #[test]
    fn rejects_truncated_and_wrong_counts() {
        let good = synthetic();
        assert!(Theme::parse(&good[..good.len() - 20]).is_err());
        assert!(Theme::parse(b"GENOMFLE").is_err());
        let mut bad = good.clone();
        // metric count follows theme version(4) + name(2) + group(4) + metrics version(4)
        bad[14 + 4 + 2 + 4 + 4] = 57;
        assert!(Theme::parse(&bad).is_err());
    }

    /// Real file (kept under the gitignored `.local/`); skipped when absent.
    #[test]
    fn parses_local_theme_when_present() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/gui/theme_g3.thm");
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let t = Theme::parse(&bytes).unwrap();
        let c = t.elements[ELEMENT_STATIC_TEXT].font_color(State::Normal);
        assert_eq!((c.r, c.g, c.b), (0xE0, 0xC4, 0x95));
        assert_eq!(t.metrics[0], 1280);
    }
}
