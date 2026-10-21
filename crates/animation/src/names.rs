//! Motion name grammar and catalog.
//!
//! Installed motion names follow
//! `<Actor>_<Pose>_<Left>_<Right>_<Pn>_<Action>_<Phase>_<Mode>_<Dir>_<Variant>_%_<Variant2>_<Pm>_<Speed>`
//! for example `Hero_Stand_None_1H_P0_Move_Walk_N_Fwd_00_%_00_P0_160`:
//!
//! * `Pose`: `Stand`, `Sneak`, `SitBench`, `LieDead`, `Swim`, ...
//! * `Left` / `Right`: what the left and right hand hold (`None`, `1H`, `2H`, `Fist`, `Bow`,
//!   `Arrow`, `CrossBow`, `Bolt`, `Staff`, `Shield`, `Torch`, `Cast`, `Potion`, ...).
//! * `Pn`: pose variant (`P0` base, `P1..` alternative sets).
//! * `Action` / `Phase`: `Move` + `Walk|Run|Sprint|Stand|Turn90`, `Ambient` + `Loop`,
//!   `Jump` + `Stand`, `Fall` + `Loop|End|Recover`, `Attack` + `Hit|Recover`, ...
//! * `Mode`: `N` normal (`I`/`O` in/out transitions), `Dir`: `Fwd|Back|Left|Right|Up|Down`.
//! * `Speed`: nominal ground speed in cm/s for movement clips (walk 150-160, run 300-400,
//!   sprint 400-500), 0 otherwise.
//!
//! This grammar is inferred from the file names only; it is not read from the engine.
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct MotionName {
    pub actor: String,
    pub pose: String,
    pub left: String,
    pub right: String,
    pub pn: String,
    pub action: String,
    pub phase: String,
    pub mode: String,
    pub dir: String,
    pub variant: u32,
    pub speed: u32,
}

impl MotionName {
    pub fn parse(name: &str) -> Option<MotionName> {
        let p: Vec<&str> = name.split('_').collect();
        if p.len() < 14 || p[10] != "%" {
            return None;
        }
        Some(MotionName {
            actor: p[0].to_string(),
            pose: p[1].to_string(),
            left: p[2].to_string(),
            right: p[3].to_string(),
            pn: p[4].to_string(),
            action: p[5].to_string(),
            phase: p[6].to_string(),
            mode: p[7].to_string(),
            dir: p[8].to_string(),
            variant: p[9].parse().ok()?,
            speed: p[13].parse().unwrap_or(0),
        })
    }

    /// Whether the clip should cycle: ambient/other loops and the continuous movement phases.
    pub fn is_cyclic(&self) -> bool {
        self.phase == "Loop"
            || (self.action == "Move" && matches!(self.phase.as_str(), "Walk" | "Run" | "Sprint"))
    }
}

/// Nominal speed from the trailing number of a motion name, 0 when absent.
pub fn trailing_speed(name: &str) -> f32 {
    name.rsplit('_')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .map_or(0.0, |v| v as f32)
}

/// Selection criteria; unset fields match anything.
#[derive(Debug, Clone, Default)]
pub struct MotionQuery<'a> {
    pub pose: Option<&'a str>,
    pub left: Option<&'a str>,
    pub right: Option<&'a str>,
    pub pn: Option<&'a str>,
    pub action: Option<&'a str>,
    pub phase: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub dir: Option<&'a str>,
}

fn eq(opt: Option<&str>, value: &str) -> bool {
    opt.is_none_or(|o| o.eq_ignore_ascii_case(value))
}

impl MotionQuery<'_> {
    fn matches(&self, m: &MotionName) -> bool {
        eq(self.pose, &m.pose)
            && eq(self.left, &m.left)
            && eq(self.right, &m.right)
            && eq(self.pn, &m.pn)
            && eq(self.action, &m.action)
            && eq(self.phase, &m.phase)
            && eq(self.mode, &m.mode)
            && eq(self.dir, &m.dir)
    }
}

/// Index of all motion names grouped by actor family.
#[derive(Default)]
pub struct MotionCatalog {
    by_actor: HashMap<String, Vec<(String, MotionName)>>,
}

impl MotionCatalog {
    pub fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut by_actor: HashMap<String, Vec<(String, MotionName)>> = HashMap::new();
        for name in names {
            if let Some(parsed) = MotionName::parse(name) {
                by_actor
                    .entry(parsed.actor.to_lowercase())
                    .or_default()
                    .push((name.to_string(), parsed));
            }
        }
        // Stable preference order: lowest variant, normal mode, then name.
        for list in by_actor.values_mut() {
            list.sort_by(|a, b| {
                (a.1.variant, a.1.mode != "N", &a.0).cmp(&(b.1.variant, b.1.mode != "N", &b.0))
            });
        }
        Self { by_actor }
    }

    pub fn len(&self) -> usize {
        self.by_actor.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_actor.is_empty()
    }

    /// Best matching motion for `actor` (case-insensitive family name).
    pub fn find(&self, actor: &str, query: &MotionQuery) -> Option<&str> {
        self.by_actor
            .get(&actor.to_lowercase())?
            .iter()
            .find(|(_, m)| query.matches(m))
            .map(|(n, _)| n.as_str())
    }

    /// Lower-case family names with at least one parsed motion, sorted.
    pub fn families(&self) -> Vec<&str> {
        let mut f: Vec<&str> = self.by_actor.keys().map(String::as_str).collect();
        f.sort_unstable();
        f
    }

    /// All parsed motions of `family` in preference order.
    pub fn motions(&self, family: &str) -> impl Iterator<Item = (&str, &MotionName)> {
        self.by_actor
            .get(&family.to_lowercase())
            .into_iter()
            .flatten()
            .map(|(n, m)| (n.as_str(), m))
    }

    pub fn parsed(&self, actor: &str, name: &str) -> Option<&MotionName> {
        self.by_actor
            .get(&actor.to_lowercase())?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, m)| m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALK: &str = "Hero_Stand_None_1H_P0_Move_Walk_N_Fwd_00_%_00_P0_160";

    #[test]
    fn parses_grammar() {
        let m = MotionName::parse(WALK).unwrap();
        assert_eq!(m.actor, "Hero");
        assert_eq!(m.left, "None");
        assert_eq!(m.right, "1H");
        assert_eq!(m.phase, "Walk");
        assert_eq!(m.dir, "Fwd");
        assert_eq!(m.speed, 160);
        assert!(m.is_cyclic());
        assert_eq!(trailing_speed(WALK), 160.0);
        assert!(MotionName::parse("Hero_Stand_short").is_none());
    }

    #[test]
    fn catalog_prefers_lowest_variant_and_filters() {
        let names = [
            "Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_01_%_00_P0_0",
            "Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_00_%_00_P0_0",
            "Hero_Stand_None_1H_P0_Move_Run_N_Fwd_00_%_00_P0_400",
            WALK,
            "Orc_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150",
        ];
        let catalog = MotionCatalog::new(names);
        assert_eq!(catalog.len(), 5);
        let idle = MotionQuery {
            pose: Some("Stand"),
            left: Some("None"),
            right: Some("None"),
            action: Some("Ambient"),
            ..Default::default()
        };
        assert_eq!(catalog.find("hero", &idle), Some(names[1]));
        let run = MotionQuery {
            phase: Some("Run"),
            right: Some("1H"),
            ..Default::default()
        };
        assert_eq!(catalog.find("Hero", &run), Some(names[2]));
        assert!(catalog.find("Orc", &run).is_none());
    }
}
