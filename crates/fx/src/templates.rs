//! Particle templates: `*.tple` documents under `VFX/` whose entities carry `eCParticle_PS`.
//! Effect commands (`gCEffectCommandSpawnEntity`) refer to them by template GUID.
use crate::params::EmitterParams;
use bevy::math::Mat4;
use gothic_formats::propstream::DecodeStats;
use gothic_formats::template::{TemplateDocument, TemplateEntity};
use gothic_formats::vfs::VirtualFileSystem;
use std::collections::HashMap;
use std::sync::Arc;

/// One emitter of a template with its placement relative to the template root.
#[derive(Clone, Debug)]
pub struct TemplateEmitter {
    pub name: String,
    /// Parameters in the engine's (unmirrored) space.
    pub params: Arc<EmitterParams>,
    /// Transform relative to the template's reference entity, engine space.
    pub local: Mat4,
}

/// All emitters of one template.
#[derive(Clone, Debug, Default)]
pub struct EmitterSet {
    pub name: String,
    pub emitters: Vec<TemplateEmitter>,
}

/// GUID/name index of particle templates.
#[derive(Debug, Default)]
pub struct FxTemplates {
    by_guid: HashMap<String, Arc<EmitterSet>>,
    by_name: HashMap<String, Arc<EmitterSet>>,
}

/// Engine matrix (row vectors, translation in the last row) as a glam matrix whose columns are
/// the axes and the translation, so `parent * local` composes as usual.
pub fn engine_matrix(m: &[[f32; 4]; 4]) -> Mat4 {
    Mat4::from_cols_array_2d(m)
}

impl FxTemplates {
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Every distinct template (each is registered under several GUIDs).
    pub fn iter_sets(&self) -> impl Iterator<Item = &Arc<EmitterSet>> {
        self.by_name.values()
    }

    pub fn by_guid(&self, guid: &str) -> Option<&Arc<EmitterSet>> {
        self.by_guid.get(guid)
    }

    pub fn by_name(&self, name: &str) -> Option<&Arc<EmitterSet>> {
        self.by_name.get(&name.to_ascii_lowercase())
    }

    /// Register a parsed template; templates without particle emitters are skipped.
    pub fn add_document(&mut self, doc: &TemplateDocument) -> bool {
        let Some(root) = doc.reference_header() else {
            return false;
        };
        let root_world = engine_matrix(&root.world_matrix);
        let root_inv = root_world.inverse();
        let emitters: Vec<TemplateEmitter> = doc
            .entities
            .iter()
            .filter(|e| !e.deleted)
            .filter_map(|e| emitter_of(e, &root_inv))
            .collect();
        if emitters.is_empty() {
            return false;
        }
        let set = Arc::new(EmitterSet {
            name: root.name.clone(),
            emitters,
        });
        for e in &doc.entities {
            self.by_guid.entry(e.guid.clone()).or_insert(set.clone());
        }
        self.by_name
            .entry(root.name.to_ascii_lowercase())
            .or_insert(set);
        true
    }

    /// Load every template below `VFX/` from the archives.
    pub fn load(vfs: &mut VirtualFileSystem) -> anyhow::Result<Self> {
        let slots: Vec<_> = vfs
            .iter()
            .filter(|(p, _)| {
                let l = p.to_ascii_lowercase();
                l.ends_with(".tple") && l.starts_with("vfx/")
            })
            .map(|(_, s)| s)
            .collect();
        let mut out = Self::default();
        let mut stats = DecodeStats::default();
        for slot in slots {
            let bytes = vfs.read(slot)?;
            if let Ok(doc) = TemplateDocument::parse(&bytes, &mut stats) {
                out.add_document(&doc);
            }
        }
        Ok(out)
    }
}

fn emitter_of(e: &TemplateEntity, root_inv: &Mat4) -> Option<TemplateEmitter> {
    let params = EmitterParams::from_set(e.set("eCParticle_PS")?)?;
    let world = engine_matrix(&e.world_matrix);
    let local = *root_inv * world;
    Some(TemplateEmitter {
        name: e.name.clone(),
        params: Arc::new(params),
        local,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_matrix_puts_the_translation_in_the_last_column() {
        let m = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
        ];
        let g = engine_matrix(&m);
        assert_eq!(g.w_axis.truncate(), bevy::math::Vec3::new(10.0, 20.0, 30.0));
    }
}
