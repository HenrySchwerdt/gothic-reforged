//! Sound effects: the raw sample index of `Sound.pak` and the `sfx/**.tple` emitter templates.
//!
//! Sound definitions are ordinary templates whose reference entity carries an `eCAudioEmitter_PS`
//! property set (`Sound` sample name, `Volume`, `MinDistance`/`MaxDistance` in world units,
//! `SpawningMode`, `SecondsBetweenRepeats`, ...). Sample names are stored with the authoring
//! extension (`.wav`) even where the shipped file is Ogg, so lookups go by lowercase stem.
use gothic_formats::propstream::DecodeStats;
use gothic_formats::template::TemplateDocument;
use std::collections::HashMap;

/// Index of playable samples by lowercase stem.
#[derive(Debug, Default)]
pub struct SoundIndex {
    by_stem: HashMap<String, Vec<String>>,
}

impl SoundIndex {
    /// `paths` are VFS paths of `Sound.pak` entries (`.ogg` / `.wav`, any case).
    pub fn build<'a>(paths: impl Iterator<Item = &'a str>) -> Self {
        let mut idx = Self::default();
        let mut all: Vec<String> = paths
            .map(|p| p.replace('\\', "/").to_ascii_lowercase())
            .filter(|p| p.ends_with(".ogg") || p.ends_with(".wav"))
            .collect();
        all.sort();
        for p in all {
            idx.by_stem.entry(stem_of(&p)).or_default().push(p);
        }
        idx
    }

    pub fn len(&self) -> usize {
        self.by_stem.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_stem.is_empty()
    }

    /// Exact sample by stem or by path (extension ignored; Ogg preferred over WAV).
    pub fn find(&self, name: &str) -> Option<&str> {
        let s = stem_of(&name.replace('\\', "/").to_ascii_lowercase());
        let v = self.by_stem.get(&s)?;
        v.iter()
            .find(|p| p.ends_with(".ogg"))
            .or_else(|| v.first())
            .map(String::as_str)
    }

    /// Numbered variants of a base name (`foo` -> `foo_01`, `foo_02`, `foo_03_a00`, ...).
    pub fn variants(&self, base: &str) -> Vec<&str> {
        let prefix = format!(
            "{}_",
            stem_of(&base.replace('\\', "/").to_ascii_lowercase())
        );
        let mut out: Vec<&str> = self
            .by_stem
            .iter()
            .filter(|(k, _)| {
                k.strip_prefix(&prefix)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            })
            .flat_map(|(_, v)| v.iter().map(String::as_str))
            .collect();
        out.sort_unstable();
        out
    }

    /// Resolve a sample name; when no exact match exists pick among numbered variants using
    /// `pick` (any integer, e.g. from an RNG).
    pub fn resolve(&self, name: &str, pick: usize) -> Option<&str> {
        if let Some(p) = self.find(name) {
            return Some(p);
        }
        let v = self.variants(name);
        (!v.is_empty()).then(|| v[pick % v.len()])
    }
}

fn stem_of(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(s, _)| s).to_owned()
}

/// One `eCAudioEmitter_PS` definition from a sound template.
#[derive(Debug, Clone, PartialEq)]
pub struct EmitterDef {
    /// Template entity name (`SFX_Env_Waterfall_03`).
    pub name: String,
    /// Template path inside `Templates.pak`.
    pub template: String,
    pub sample: String,
    pub volume: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub use_stream: bool,
    /// `eEAudioEmitterMode` value (0 = ? , 1 = repeating; meaning of the other values is [U]).
    pub spawning_mode: i32,
    pub seconds_between_repeats: [f32; 2],
}

#[derive(Debug, Default)]
pub struct SfxLibrary {
    emitters: HashMap<String, EmitterDef>,
}

impl SfxLibrary {
    pub fn insert(&mut self, def: EmitterDef) {
        self.emitters.insert(def.name.to_ascii_lowercase(), def);
    }

    pub fn len(&self) -> usize {
        self.emitters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.emitters.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&EmitterDef> {
        self.emitters.get(&name.to_ascii_lowercase())
    }

    pub fn iter(&self) -> impl Iterator<Item = &EmitterDef> {
        self.emitters.values()
    }

    /// Parse a template; adds one definition per entity that has an audio emitter.
    pub fn add_template(&mut self, path: &str, bytes: &[u8]) -> bool {
        let mut stats = DecodeStats::default();
        let Ok(doc) = TemplateDocument::parse(bytes, &mut stats) else {
            return false;
        };
        let mut found = false;
        for e in &doc.entities {
            let Some(ps) = e.set("eCAudioEmitter_PS") else {
                continue;
            };
            let Some(sample) = ps.string("Sound").filter(|s| !s.is_empty()) else {
                continue;
            };
            let range = |n: &str| match ps.get(n) {
                Some(gothic_formats::propstream::PropValue::Range1(r)) => *r,
                _ => [0.0, 0.0],
            };
            self.insert(EmitterDef {
                name: e.name.clone(),
                template: path.to_owned(),
                sample: sample.to_owned(),
                volume: ps.float("Volume").unwrap_or(1.0),
                min_distance: ps.float("MinDistance").unwrap_or(0.0),
                max_distance: ps.float("MaxDistance").unwrap_or(0.0),
                use_stream: ps.boolean("UseStream").unwrap_or(false),
                spawning_mode: ps.int("SpawningMode").unwrap_or(0),
                seconds_between_repeats: range("SecondsBetweenRepeats"),
            });
            found = true;
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SoundIndex {
        SoundIndex::build(
            [
                "creature/wolf/sfx_creature_wolf_sniff_01.ogg",
                "creature/wolf/sfx_creature_wolf_sniff_02.ogg",
                "Foley/Footsteps/human/grass/walk/eff_step_human_walk_left_grass_01.wav",
                "environment/water/water_riverrapid_02.ogg",
                "environment/water/water_riverrapid_02.wav",
                "notes.txt",
            ]
            .into_iter(),
        )
    }

    #[test]
    fn stem_lookup_ignores_extension_and_prefers_ogg() {
        let i = index();
        assert_eq!(i.len(), 5);
        assert_eq!(
            i.find("water_riverrapid_02.wav"),
            Some("environment/water/water_riverrapid_02.ogg")
        );
        assert_eq!(
            i.find("EFF_STEP_HUMAN_WALK_LEFT_GRASS_01"),
            Some("foley/footsteps/human/grass/walk/eff_step_human_walk_left_grass_01.wav")
        );
        assert!(i.find("missing").is_none());
    }

    #[test]
    fn variants_resolve_by_pick() {
        let i = index();
        assert_eq!(i.variants("sfx_creature_wolf_sniff").len(), 2);
        let a = i.resolve("sfx_creature_wolf_sniff", 0).unwrap();
        let b = i.resolve("sfx_creature_wolf_sniff", 1).unwrap();
        assert_ne!(a, b);
        assert!(i.resolve("sfx_creature_wolf_howl", 0).is_none());
    }
}
