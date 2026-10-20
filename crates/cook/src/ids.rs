//! Stable asset identifiers and cooker profiles.
use serde::{Deserialize, Serialize};

/// Bump when any cooked layout or cooking rule changes; invalidates every artifact.
pub const COOKER_VERSION: u32 = 1;

/// 64-bit id of an asset name: XXH3 of the lower-cased basename (extension included).
/// `0` is reserved for "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetId(pub u64);

impl AssetId {
    pub const NONE: AssetId = AssetId(0);

    pub fn of(name: &str) -> Self {
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let lower = base.to_ascii_lowercase();
        AssetId(hash64(lower.as_bytes()).max(1))
    }
    pub fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

pub fn hash64(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureMode {
    /// Keep the source BC1/2/3 blocks; optionally drop top mips so max dimension <= cap.
    Bc { max_dim: Option<u32> },
    /// Transcode to ASTC 6x6 (needs the `astc` feature), capped at `max_dim`.
    Astc6x6 { max_dim: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub texture: TextureMode,
    /// Triangle ratio of LOD1 and LOD2 relative to LOD0 (0 = skip that LOD).
    pub lod_ratios: [f32; 2],
    /// Submeshes below this triangle count get no LODs.
    pub lod_min_triangles: usize,
    /// Quantize motion rotation keys to 16-bit snorm quaternions.
    pub quantize_motion: bool,
    /// Placeholder for later texture upscaling / PBR derivation (see `remaster.rs`).
    pub remaster_hook: bool,
}

impl Profile {
    pub fn by_name(name: &str) -> Option<Self> {
        let p = |texture, lod_ratios, quantize_motion, remaster_hook| Profile {
            name: name.into(),
            texture,
            lod_ratios,
            lod_min_triangles: 300,
            quantize_motion,
            remaster_hook,
        };
        Some(match name {
            "desktop-high" => p(TextureMode::Bc { max_dim: None }, [0.5, 0.2], false, false),
            "desktop-low" => p(
                TextureMode::Bc {
                    max_dim: Some(1024),
                },
                [0.4, 0.12],
                true,
                false,
            ),
            "quest" => p(
                TextureMode::Astc6x6 { max_dim: 1024 },
                [0.3, 0.08],
                true,
                false,
            ),
            "remaster" => p(TextureMode::Bc { max_dim: None }, [0.5, 0.2], false, true),
            _ => return None,
        })
    }
    pub fn all_names() -> &'static [&'static str] {
        &["desktop-high", "desktop-low", "quest", "remaster"]
    }
    /// Hash of every setting that changes cooked bytes (part of each artifact key).
    pub fn settings_hash(&self) -> u64 {
        hash64(
            format!(
                "{}|{:?}|{:?}|{}|{}|{}",
                COOKER_VERSION,
                self.texture,
                self.lod_ratios,
                self.quantize_motion,
                self.lod_min_triangles,
                self.remaster_hook
            )
            .as_bytes(),
        )
    }
}
