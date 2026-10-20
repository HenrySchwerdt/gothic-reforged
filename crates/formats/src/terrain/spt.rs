//! Minimal reader for SpeedTree RT `.spt` parameter files (`__IdvSpt_02_`).
//!
//! The files are token streams (`u32 id`, payload) of tree-generator parameters; the
//! procedural generator itself is not reimplemented. Only the fields needed to build a
//! stand-in tree are read, all verified on the 98 installed files:
//!
//! * token `2000`: `u32 length` + bark texture file name,
//! * token `2001`: `f32` tree size in cm (e.g. 1100 for a large Douglas fir, 75 for the
//!   smallest holly),
//! * every other length-prefixed `*.tga` / `*.dds` string: leaf/frond/composite textures.
//!
//! Anything else is ignored; unparsable input yields an error, never a panic.
use anyhow::{Result, ensure};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Spt {
    /// Tree height in cm.
    pub size: f32,
    pub bark: String,
    /// Other texture file names (leaf cards, frond cards, composites), in file order.
    pub textures: Vec<String>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Spt {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() >= 24 && bytes.len() <= 1 << 20, "spt size");
        ensure!(
            bytes.get(8..20) == Some(b"__IdvSpt_02_"),
            "not a SpeedTree spt"
        );
        let mut out = Spt::default();
        let mut at = 20;
        let mut found_size = false;
        while at + 8 <= bytes.len() {
            let token = u32_at(bytes, at).unwrap_or(0);
            if token == 2001 && !found_size {
                let size = f32::from_bits(u32_at(bytes, at + 4).unwrap_or(0));
                if size.is_finite() && size > 0.0 {
                    out.size = size;
                    found_size = true;
                    at += 8;
                    continue;
                }
            }
            // length-prefixed texture names
            if let Some(len) = u32_at(bytes, at)
                && (5..=96).contains(&len)
                && let Some(s) = bytes.get(at + 4..at + 4 + len as usize)
                && s.iter().all(|c| c.is_ascii_graphic())
            {
                let name = String::from_utf8_lossy(s).into_owned();
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".tga") || lower.ends_with(".dds") || lower.ends_with(".png") {
                    if token == 2000 || (out.bark.is_empty() && lower.contains("bark")) {
                        out.bark = name;
                    } else if out.bark != name && !out.textures.contains(&name) {
                        out.textures.push(name);
                    }
                    at += 4 + len as usize;
                    continue;
                }
            }
            at += 1;
        }
        // Token 2000 is itself the length of the string that follows when it directly precedes
        // the bark name; the scan above handles both cases.
        ensure!(out.size > 0.0, "spt has no size token");
        Ok(out)
    }
}
