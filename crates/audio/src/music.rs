//! Music table (`Library.pak/g3.mul`) and the pure selection logic of the music state machine.
//!
//! Data model recovered from the file (all property names are verbatim):
//!
//! * `gCMusicFragment` (62): one streamed segment. `Location` (`myrtana`, `varant`, `dungeon`,
//!   ...), `Situation` (`explore`, `fight`, plus Myrtana story cues `beliar`, `innos`,
//!   `revolution`, `title`, ...), `DayTime` (`day`/`night`), `Variation`/`NextVariation` (chain of
//!   alternates), `Sample` (stored as `.wav`, shipped as `.ogg` in `Music.pak/fragments/`),
//!   `Volume`, `LeadInEnd`, `LeadOutBegin`, `Duration` (milliseconds).
//! * `gCMusicTransition` (42): short stingers in `Music.pak/transitions/` with cross-fade timing
//!   (`FadeOutBegin/End`, `FadeInBegin/End`, `Offset`, `Timing`) and `ConnectIn/ConnectOut` ids.
//! * `gCMusicTrigger` (8): named switches (`Loc_Myrtana`, `Sit_Fight`, `Stop`, ...) that set a
//!   location and/or situation; the world's music-zone entities refer to these ids.
//!
//! Which transition stinger sits between two specific fragments is not decoded yet
//! (`ConnectIn/Out` pairing is [U]); the runtime uses plain cross-fades.
use crate::mul::MulDocument;
use anyhow::{Result, ensure};
use gothic_formats::propstream::PropertySet;

#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub location: String,
    pub situation: String,
    pub day_time: String,
    pub variation: String,
    pub next_variation: String,
    /// Sample name as stored (`x.wav`); see [`Fragment::vfs_path`].
    pub sample: String,
    pub volume: f32,
    pub lead_in_end_ms: i32,
    pub lead_out_begin_ms: i32,
    pub duration_ms: i32,
}

impl Fragment {
    /// Path inside the VFS (`fragments/<stem>.ogg`); the shipped files are Ogg, not WAV.
    pub fn vfs_path(&self) -> String {
        format!("fragments/{}.ogg", stem(&self.sample))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub sample: String,
    pub connect_in: i32,
    pub connect_out: i32,
    pub fade_out_begin_ms: i32,
    pub fade_out_end_ms: i32,
    pub fade_in_begin_ms: i32,
    pub fade_in_end_ms: i32,
    pub offset_ms: i32,
    pub duration_ms: i32,
}

impl Transition {
    pub fn vfs_path(&self) -> String {
        format!("transitions/{}.ogg", stem(&self.sample))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub id: String,
    pub location: String,
    pub situation: String,
    pub is_stop: bool,
}

fn stem(sample: &str) -> String {
    let s = sample.to_ascii_lowercase();
    s.strip_suffix(".wav").unwrap_or(&s).to_owned()
}

pub const EXPLORE: &str = "explore";
pub const FIGHT: &str = "fight";

#[derive(Debug, Clone, Default)]
pub struct MusicLibrary {
    pub fragments: Vec<Fragment>,
    pub transitions: Vec<Transition>,
    pub triggers: Vec<Trigger>,
    /// Table entries whose sample file is not shipped (for example
    /// `trelis_explore_night_01_new`); removed from `fragments` by [`MusicLibrary::retain_available`].
    pub missing: Vec<String>,
}

impl MusicLibrary {
    /// Drop fragments and transitions whose file `exists` rejects, remembering their names.
    pub fn retain_available(&mut self, exists: impl Fn(&str) -> bool) {
        let mut missing = Vec::new();
        self.fragments.retain(|f| {
            let ok = exists(&f.vfs_path());
            if !ok {
                missing.push(f.vfs_path());
            }
            ok
        });
        self.transitions.retain(|t| {
            let ok = exists(&t.vfs_path());
            if !ok {
                missing.push(t.vfs_path());
            }
            ok
        });
        self.missing.extend(missing);
    }

    pub fn from_document(doc: &MulDocument) -> Result<Self> {
        let mut lib = Self::default();
        for o in &doc.objects {
            match o.class.as_str() {
                "gCMusicFragment" => lib.fragments.push(fragment(o)),
                "gCMusicTransition" => lib.transitions.push(transition(o)),
                "gCMusicTrigger" => lib.triggers.push(Trigger {
                    id: s(o, "ID"),
                    location: s(o, "Location"),
                    situation: s(o, "Situation"),
                    is_stop: o.boolean("IsStopTrigger").unwrap_or(false),
                }),
                _ => {}
            }
        }
        ensure!(!lib.fragments.is_empty(), "music table has no fragments");
        Ok(lib)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_document(&crate::mul::parse(bytes)?)
    }

    pub fn trigger(&self, id: &str) -> Option<&Trigger> {
        self.triggers.iter().find(|t| t.id.eq_ignore_ascii_case(id))
    }

    /// Distinct locations that have at least one fragment, sorted.
    pub fn locations(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.fragments.iter().map(|f| f.location.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Fragment group (all variations) for exactly `(location, situation, daytime)`.
    pub fn group(&self, location: &str, situation: &str, day_time: &str) -> Vec<&Fragment> {
        self.fragments
            .iter()
            .filter(|f| {
                f.location.eq_ignore_ascii_case(location)
                    && f.situation.eq_ignore_ascii_case(situation)
                    && f.day_time.eq_ignore_ascii_case(day_time)
            })
            .collect()
    }

    /// Resolve a music request to the first fragment of the best matching group.
    ///
    /// Fallback order: requested daytime, then `day`; requested situation, then `explore` of the
    /// same location, then the same situation at the default region, then its `explore`.
    /// `None` only when neither the location nor the default region has any music.
    pub fn resolve(
        &self,
        location: &str,
        situation: &str,
        night: bool,
        default_region: &str,
    ) -> Option<&Fragment> {
        let times: &[&str] = if night { &["night", "day"] } else { &["day"] };
        let tries = [
            (location, situation),
            (location, "explore"),
            (default_region, situation),
            (default_region, "explore"),
        ];
        for (loc, sit) in tries {
            for t in times {
                if let Some(f) = self.group(loc, sit, t).first() {
                    return Some(f);
                }
            }
        }
        None
    }

    /// The fragment to play after `current` finishes. A single-variation group repeats itself
    /// (`None` here means "loop the same fragment"); alternates chain through `NextVariation`.
    /// `Some(None)` means one-shot cue: stop after it (`NextVariation == -1`).
    pub fn next_after(&self, current: &Fragment) -> NextFragment<'_> {
        let nv = current.next_variation.trim();
        if nv == "-1" {
            return NextFragment::EndOfCue;
        }
        if nv.is_empty() {
            return NextFragment::Repeat;
        }
        match self
            .group(&current.location, &current.situation, &current.day_time)
            .into_iter()
            .find(|f| f.variation == nv)
        {
            Some(f) => NextFragment::Play(f),
            None => NextFragment::Repeat,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NextFragment<'a> {
    Repeat,
    Play(&'a Fragment),
    EndOfCue,
}

fn s(o: &PropertySet, name: &str) -> String {
    o.string(name).unwrap_or_default().to_owned()
}

fn fragment(o: &PropertySet) -> Fragment {
    Fragment {
        location: s(o, "Location"),
        situation: s(o, "Situation"),
        day_time: s(o, "DayTime"),
        variation: s(o, "Variation"),
        next_variation: s(o, "NextVariation"),
        sample: s(o, "Sample"),
        volume: o.float("Volume").unwrap_or(1.0),
        lead_in_end_ms: o.int("LeadInEnd").unwrap_or(0),
        lead_out_begin_ms: o.int("LeadOutBegin").unwrap_or(0),
        duration_ms: o.int("Duration").unwrap_or(0),
    }
}

fn transition(o: &PropertySet) -> Transition {
    let i = |n: &str| o.int(n).unwrap_or(0);
    Transition {
        sample: s(o, "Sample"),
        connect_in: i("ConnectIn"),
        connect_out: i("ConnectOut"),
        fade_out_begin_ms: i("FadeOutBegin"),
        fade_out_end_ms: i("FadeOutEnd"),
        fade_in_begin_ms: i("FadeInBegin"),
        fade_in_end_ms: i("FadeInEnd"),
        offset_ms: i("Offset"),
        duration_ms: i("Duration"),
    }
}

#[cfg(test)]
pub(crate) fn test_library() -> MusicLibrary {
    let f = |loc: &str, sit: &str, day: &str, var: &str, next: &str| Fragment {
        location: loc.into(),
        situation: sit.into(),
        day_time: day.into(),
        variation: var.into(),
        next_variation: next.into(),
        sample: format!("{loc}_{sit}_{day}_{var}.wav"),
        volume: 1.0,
        lead_in_end_ms: 0,
        lead_out_begin_ms: 1000,
        duration_ms: 1000,
    };
    MusicLibrary {
        fragments: vec![
            f("myrtana", "explore", "day", "00", ""),
            f("myrtana", "explore", "night", "00", ""),
            f("myrtana", "fight", "day", "00", ""),
            f("myrtana", "innos", "day", "00", "-1"),
            f("varant", "explore", "day", "0", "1"),
            f("varant", "explore", "day", "1", "0"),
            f("braga", "explore", "day", "00", ""),
        ],
        transitions: vec![],
        missing: vec![],
        triggers: vec![Trigger {
            id: "Loc_Myrtana".into(),
            location: "Myrtana".into(),
            situation: String::new(),
            is_stop: false,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_falls_back_sensibly() {
        let l = test_library();
        let r = |loc, sit, night| {
            l.resolve(loc, sit, night, "myrtana")
                .map(|f| f.sample.as_str())
        };
        assert_eq!(
            r("myrtana", "explore", true),
            Some("myrtana_explore_night_00.wav")
        );
        assert_eq!(
            r("braga", "explore", true),
            Some("braga_explore_day_00.wav")
        );
        // braga has no fight music: fall back to braga explore, not the default region
        assert_eq!(r("braga", "fight", false), Some("braga_explore_day_00.wav"));
        // unknown location uses the default region
        assert_eq!(
            r("nowhere", "fight", false),
            Some("myrtana_fight_day_00.wav")
        );
        assert_eq!(
            r("Myrtana", "innos", false),
            Some("myrtana_innos_day_00.wav")
        );
        assert!(
            l.resolve("nowhere", "explore", false, "also_nowhere")
                .is_none()
        );
    }

    #[test]
    fn variation_chain_and_cues() {
        let l = test_library();
        let v0 = l.group("varant", "explore", "day")[0];
        match l.next_after(v0) {
            NextFragment::Play(f) => assert_eq!(f.variation, "1"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            l.next_after(l.group("braga", "explore", "day")[0]),
            NextFragment::Repeat
        );
        assert_eq!(
            l.next_after(l.group("myrtana", "innos", "day")[0]),
            NextFragment::EndOfCue
        );
    }

    #[test]
    fn vfs_path_uses_ogg() {
        let l = test_library();
        assert_eq!(
            l.fragments[0].vfs_path(),
            "fragments/myrtana_explore_day_00.ogg"
        );
        assert!(l.trigger("loc_myrtana").is_some());
        assert_eq!(l.locations(), vec!["braga", "myrtana", "varant"]);
    }
}
