//! World effect entities: `eCParticle_PS` emitters placed directly in the world documents
//! (campfires, coal glow) and `gCEffect_PS` entities that name an effect of the effect map
//! (torches, braziers, crystals).
use crate::params::EmitterParams;
use crate::templates::engine_matrix;
use bevy::math::{Mat4, Vec3};
use gothic_formats::propstream::DecodeStats;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::sync::Arc;

/// A particle emitter entity of a world document.
#[derive(Clone, Debug)]
pub struct WorldEmitter {
    pub name: String,
    /// Engine-space entity matrix.
    pub matrix: Mat4,
    pub params: Arc<EmitterParams>,
}

/// A `gCEffect_PS` entity: plays `effect` (from the effect map) at its position.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldEffect {
    pub name: String,
    pub matrix: Mat4,
    pub effect: String,
    /// Offset in the entity frame (cm).
    pub offset: Vec3,
    pub probability: f32,
    /// `Static`: a permanent effect that never finishes (torches).
    pub is_static: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorldFx {
    pub emitters: Vec<WorldEmitter>,
    pub effects: Vec<WorldEffect>,
}

/// Which documents to scan and which entities to keep.
pub struct ScanArea<'a> {
    /// Engine-space centre (cm) and radius (cm) of the kept entities.
    pub center: Vec3,
    pub radius: f32,
    /// Document path filter (lower case path).
    pub document: &'a dyn Fn(&str) -> bool,
}

/// Scan the world documents for effect entities inside the area.
pub fn scan(vfs: &mut VirtualFileSystem, area: &ScanArea) -> WorldFx {
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat")) && (area.document)(&l)
        })
        .map(|(_, s)| s)
        .collect();
    let mut out = WorldFx::default();
    for slot in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let mut stats = DecodeStats::default();
        let Ok((doc, sets)) = WorldDocument::parse_with_properties(
            &bytes,
            &|c| c == "eCParticle_PS" || c == "gCEffect_PS",
            &mut stats,
        ) else {
            continue;
        };
        for (entity, sets) in doc.entities.iter().zip(sets) {
            if !entity.enabled {
                continue;
            }
            let matrix = engine_matrix(&entity.world_matrix);
            let at = matrix.w_axis.truncate();
            if (at - area.center).length() > area.radius {
                continue;
            }
            for set in &sets {
                match set.class.as_str() {
                    "eCParticle_PS" => {
                        if let Some(params) = EmitterParams::from_set(set) {
                            out.emitters.push(WorldEmitter {
                                name: entity.name.clone(),
                                matrix,
                                params: Arc::new(params),
                            });
                        }
                    }
                    "gCEffect_PS" => {
                        let effect = set.string("Effect").unwrap_or_default();
                        if effect.is_empty() {
                            continue;
                        }
                        let off = match set.get("Offset") {
                            Some(gothic_formats::propstream::PropValue::Vec3(v)) => *v,
                            _ => [0.0; 3],
                        };
                        out.effects.push(WorldEffect {
                            name: entity.name.clone(),
                            matrix,
                            effect: effect.to_owned(),
                            offset: Vec3::from_array(off),
                            probability: set.float("Probability").unwrap_or(1.0),
                            is_static: set.boolean("Static").unwrap_or(false),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    out
}
