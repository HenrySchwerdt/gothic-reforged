//! Voice-over lookup: dialog / stock-voice string keys to `Speech_<Lang>.pak` files.
//!
//! Observed naming (see `docs/research/AUDIO_FORMATS.md`), all lowercase in the archive:
//!
//! * dialog lines: `<lang>_ogg/info_<speaker>_<key tail>_<lang>.ogg`, where the string-table key
//!   is `INFO_<KEY TAIL>` (for example key `INFO_BPANKRATZ320060306120442` is spoken by
//!   `info_hum_fat_bearish_pedar_bpankratz320060306120442_german.ogg`); `<speaker>` is
//!   `<voice class>_<npc>` (`hum_fat_bearish_pedar`) or `player_pc_hero` for the player. The same
//!   key can have several files when different voices speak the same line.
//! * stock voice lines: `<lang>_ogg/<svm key>_<lang>.ogg` with the string key `SVM_...` verbatim.
//! * every `.ogg` has a sibling `<lang>_lipsyncdata/<same stem>.xlip` (lip-sync track).
use std::collections::{HashMap, HashSet};

/// One voice recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceFile {
    /// Full VFS path (lowercase), e.g. `german_ogg/info_..._german.ogg`.
    pub path: String,
    /// Speaker part of an `info_` file (`hum_fat_bearish_pedar`); empty for `svm_` files.
    pub speaker: String,
    /// Lip-sync sibling path if present in the archive set.
    pub lipsync: Option<String>,
}

/// Audio language. `tag` is both the archive folder prefix and the filename suffix (`german`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechLanguage {
    pub name: String,
    pub tag: String,
}

impl SpeechLanguage {
    /// `German` / `English` (case-insensitive). Other languages ship no speech archive.
    pub fn new(name: &str) -> Option<Self> {
        let tag = name.to_ascii_lowercase();
        matches!(tag.as_str(), "german" | "english").then(|| Self {
            name: name.to_owned(),
            tag,
        })
    }
}

#[derive(Debug, Default)]
pub struct SpeechIndex {
    pub language: Option<SpeechLanguage>,
    files: Vec<VoiceFile>,
    /// Lowercase stem after `info_` (without language tag) per file, parallel to `files`.
    stems: Vec<String>,
    /// Every underscore-aligned suffix of that stem to the files carrying it.
    by_suffix: HashMap<String, Vec<u32>>,
    /// Lowercase full stem of `svm_` files.
    by_svm: HashMap<String, Vec<u32>>,
    /// Every underscore-aligned suffix of an `svm_` stem (event names contain underscores).
    svm_by_suffix: HashMap<String, Vec<u32>>,
}

impl SpeechIndex {
    /// Build from every visible VFS path (any order, any case).
    pub fn build<'a>(language: SpeechLanguage, paths: impl Iterator<Item = &'a str>) -> Self {
        let ogg_prefix = format!("{}_ogg/", language.tag);
        let lip_prefix = format!("{}_lipsyncdata/", language.tag);
        let tail = format!("_{}", language.tag);
        let mut lips = HashSet::new();
        let mut oggs = Vec::new();
        for p in paths {
            let p = p.replace('\\', "/").to_ascii_lowercase();
            if p.starts_with(&lip_prefix) && p.ends_with(".xlip") {
                lips.insert(p);
            } else if p.starts_with(&ogg_prefix) && p.ends_with(".ogg") {
                oggs.push(p);
            }
        }
        oggs.sort();
        let mut idx = Self {
            language: Some(language),
            ..Self::default()
        };
        for p in oggs {
            let full_stem = &p[ogg_prefix.len()..p.len() - 4];
            let Some(stem) = full_stem.strip_suffix(&tail) else {
                continue;
            };
            let lip = format!("{lip_prefix}{full_stem}.xlip");
            let lipsync = lips.contains(&lip).then_some(lip);
            let n = idx.files.len() as u32;
            if let Some(rest) = stem.strip_prefix("svm_") {
                idx.by_svm.entry(stem.to_owned()).or_default().push(n);
                let mut starts = vec![0usize];
                starts.extend(rest.match_indices('_').map(|(i, _)| i + 1));
                for s in starts {
                    idx.svm_by_suffix
                        .entry(rest[s..].to_owned())
                        .or_default()
                        .push(n);
                }
            } else if let Some(rest) = stem.strip_prefix("info_") {
                // Register every underscore-aligned suffix; lookup asks for the exact key tail,
                // the remaining prefix is the speaker.
                let mut starts = vec![0usize];
                starts.extend(rest.match_indices('_').map(|(i, _)| i + 1));
                for s in starts {
                    idx.by_suffix
                        .entry(rest[s..].to_owned())
                        .or_default()
                        .push(n);
                }
            } else {
                continue;
            }
            idx.stems.push(stem.to_owned());
            idx.files.push(VoiceFile {
                path: p,
                speaker: String::new(),
                lipsync,
            });
        }
        idx
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// All recordings of a string-table key (`INFO_...` or `SVM_...`, any case).
    pub fn candidates(&self, key: &str) -> Vec<VoiceFile> {
        let lower = key.to_ascii_lowercase();
        if lower.starts_with("svm_") {
            return self
                .by_svm
                .get(&lower)
                .map(|v| v.iter().map(|&i| self.files[i as usize].clone()).collect())
                .unwrap_or_default();
        }
        let Some(tail) = lower.strip_prefix("info_") else {
            return Vec::new();
        };
        let Some(ids) = self.by_suffix.get(tail) else {
            return Vec::new();
        };
        ids.iter()
            .map(|&i| {
                let mut f = self.files[i as usize].clone();
                let stem = self.stems[i as usize]
                    .strip_prefix("info_")
                    .unwrap_or_default();
                f.speaker = stem
                    .strip_suffix(tail)
                    .map(|s| s.trim_end_matches('_').to_owned())
                    .unwrap_or_default();
                f
            })
            .collect()
    }

    /// Whether any recording exists for the key.
    pub fn has(&self, key: &str) -> bool {
        let lower = key.to_ascii_lowercase();
        match lower.strip_prefix("info_") {
            Some(tail) => self.by_suffix.contains_key(tail),
            None => self.by_svm.contains_key(&lower),
        }
    }

    /// Stock voice line for an NPC voice (`Hum_Warrior_Dumb`, or a named voice such as `Diego`)
    /// and an event (`ThoughtSo`, `Teach_Axe_3_01`): this is what the `SaySVM` script command
    /// names (`ID1` = event). Tries the exact `svm_<voice>_<event>` file, then the same event
    /// from another voice of the same race prefix (`hum`, `orc`, `ass`), then any voice.
    pub fn svm(&self, voice: &str, event: &str) -> Option<VoiceFile> {
        let (voice, event) = (voice.to_ascii_lowercase(), event.to_ascii_lowercase());
        let exact = format!("svm_{voice}_{event}");
        if let Some(&i) = self.by_svm.get(&exact).and_then(|v| v.first()) {
            return Some(self.files[i as usize].clone());
        }
        let ids = self.svm_by_suffix.get(&event)?;
        let race = voice.split('_').next().unwrap_or("");
        let pick = |same_race: bool| {
            ids.iter().copied().find(|&i| {
                let stem = &self.stems[i as usize];
                // the suffix must be the whole event: stem = svm_<speaker>_<event>
                stem.ends_with(&format!("_{event}"))
                    && (!same_race || stem[4..].starts_with(&format!("{race}_")))
            })
        };
        pick(true)
            .or_else(|| pick(false))
            .map(|i| self.files[i as usize].clone())
    }

    /// Choose one recording. `speaker_hint` (voice class or NPC name, any case) prefers files whose
    /// speaker string contains it (`player` / `player_pc_hero` for the hero). Falls back to the
    /// first file in path order. An `SVM_<voice>_<event>` key without an exact file falls back to
    /// the same event from another voice (see [`SpeechIndex::svm`]).
    pub fn lookup(&self, key: &str, speaker_hint: Option<&str>) -> Option<VoiceFile> {
        let mut c = self.candidates(key);
        if c.is_empty() {
            let lower = key.to_ascii_lowercase();
            let rest = lower.strip_prefix("svm_")?;
            // Longest suffix first: drop leading voice segments one by one (race, build, mood).
            let segs: Vec<&str> = rest.split('_').collect();
            for skip in 1..segs.len() {
                let (voice, event) = (segs[..skip].join("_"), segs[skip..].join("_"));
                if let Some(f) = self.svm(&voice, &event) {
                    return Some(f);
                }
            }
            return None;
        }
        if c.len() > 1
            && let Some(h) = speaker_hint
                .map(str::to_ascii_lowercase)
                .filter(|h| !h.is_empty())
            && let Some(i) = c.iter().position(|f| f.speaker.contains(&h))
        {
            return Some(c.swap_remove(i));
        }
        c.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SpeechIndex {
        let paths = [
            "german_ogg/info_hum_fat_bearish_pedar_bpankratz320060406182859_0_german.ogg",
            "German_Ogg/info_player_pc_hero_bpankratz320060406182859_0_german.ogg",
            "german_ogg/info_hum_warrior_clear_tom_filler325_german.ogg",
            "german_ogg/svm_hum_warrior_hard_thisway_german.ogg",
            "german_lipsyncdata/svm_hum_warrior_hard_thisway_german.xlip",
            "english_ogg/info_hum_warrior_clear_tom_filler325_english.ogg",
            "german_ogg/readme.txt",
        ];
        SpeechIndex::build(SpeechLanguage::new("German").unwrap(), paths.into_iter())
    }

    #[test]
    fn dialog_key_maps_to_file_with_speaker() {
        let i = index();
        assert_eq!(i.len(), 4);
        let c = i.candidates("INFO_BPANKRATZ320060406182859_0");
        assert_eq!(c.len(), 2);
        assert!(c.iter().any(|f| f.speaker == "hum_fat_bearish_pedar"));
        assert!(c.iter().any(|f| f.speaker == "player_pc_hero"));
        // the `_0` variant is part of the key; the bare key must not match
        assert!(!i.has("INFO_BPANKRATZ320060406182859"));
    }

    #[test]
    fn speaker_hint_selects_voice() {
        let i = index();
        let f = i
            .lookup("INFO_BPANKRATZ320060406182859_0", Some("player_pc"))
            .unwrap();
        assert!(f.path.contains("player_pc_hero"));
        let f = i
            .lookup("info_bpankratz320060406182859_0", Some("PEDAR"))
            .unwrap();
        assert!(f.path.contains("pedar"));
    }

    #[test]
    fn svm_event_falls_back_to_other_voices() {
        let i = SpeechIndex::build(
            SpeechLanguage::new("German").unwrap(),
            [
                "german_ogg/svm_hum_warrior_hard_thisway_german.ogg",
                "german_ogg/svm_hum_old_wise_teach_axe_3_01_german.ogg",
                "german_ogg/svm_orc_boss_clear_teach_axe_3_01_german.ogg",
            ]
            .into_iter(),
        );
        let p = |v, e| i.svm(v, e).map(|f| f.path);
        assert_eq!(
            p("Hum_Warrior_Hard", "ThisWay").as_deref(),
            Some("german_ogg/svm_hum_warrior_hard_thisway_german.ogg")
        );
        // other human voice for the same event; orc voice picks the orc file
        assert!(
            p("Hum_Fat_Dumb", "Teach_Axe_3_01")
                .unwrap()
                .contains("hum_old_wise")
        );
        assert!(
            p("Orc_Warrior_Calm", "Teach_Axe_3_01")
                .unwrap()
                .contains("orc_boss")
        );
        assert!(p("Hum_Fat_Dumb", "NoSuchEvent").is_none());
        // `lookup` splits an unknown key into voice and event by itself
        let f = i
            .lookup("SVM_Orc_Warrior_Calm_Teach_Axe_3_01", None)
            .unwrap();
        assert!(f.path.contains("orc_boss"));
        assert!(i.lookup("SVM_Hum_Fat_Dumb_Nothing", None).is_none());
    }

    #[test]
    fn svm_lipsync_and_language_isolation() {
        let i = index();
        let f = i.lookup("SVM_Hum_Warrior_Hard_ThisWay", None).unwrap();
        assert_eq!(
            f.lipsync.as_deref(),
            Some("german_lipsyncdata/svm_hum_warrior_hard_thisway_german.xlip")
        );
        assert_eq!(i.candidates("INFO_FILLER325").len(), 1);
        assert!(i.candidates("INFO_NOPE").is_empty());
        assert!(SpeechLanguage::new("Polish").is_none());
    }
}
