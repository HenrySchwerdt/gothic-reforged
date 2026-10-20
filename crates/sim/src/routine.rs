//! Daily routines: time-of-day schedule, use-type -> task table, routine task priority
//! [CA 11.1-11.3]. How a routine *name* resolves to concrete points is not in the script DLL
//! [CA 11.1 U]; [`RoutineResolver`] is the hook the game (or a test) fills in.
use crate::types::Species;
use crate::world::Pos;
use gothic_script::routine::select::task_for_use_type as script_task_for_use_type;
use gothic_script::routine::state::State as ScriptState;
use gothic_script::routine::time as script_time;
use std::collections::BTreeMap;

/// Game clock: the normal clock runs 12 game seconds per real second [CA 11.2].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameClock {
    /// Game milliseconds since day 0, 00:00.
    pub game_ms: u64,
}

impl GameClock {
    pub fn at(day: u64, hour: u64) -> Self {
        Self {
            game_ms: (day * 24 + hour) * 3_600_000,
        }
    }
    /// Advance by `real_ms` of wall time at `speed` game seconds per real second.
    pub fn advance(&mut self, real_ms: u64, speed: u32) {
        self.game_ms += real_ms * u64::from(speed);
    }
    /// Hour of the day in `0.0..24.0`.
    pub fn hour_of_day(&self) -> f32 {
        (self.game_ms % 86_400_000) as f32 / 3_600_000.0
    }
    /// `hours = (year*365 + day)*24 + hour` as used by the quest state (year folded into days).
    pub fn total_hours(&self) -> u64 {
        self.game_ms / 3_600_000
    }
    /// `PlayerSleep` (S:76440): advance to 08:00, or to the next 08:00 when already past.
    pub fn sleep_until_morning(&mut self) {
        let day_start = self.game_ms / 86_400_000 * 86_400_000;
        let eight = day_start + 8 * 3_600_000;
        self.game_ms = if self.game_ms < eight {
            eight
        } else {
            eight + 86_400_000
        };
    }
}

/// `GetRoutineTime` (S:a99d0): 0 sleep, 1 work, 2 relax [CA 11.2]. The schedule itself is the
/// ported script rule (`gothic_script::routine::time`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoutineTime {
    Sleep,
    Work,
    Relax,
}

pub fn routine_time(species: Species, hour: f32) -> RoutineTime {
    match script_time::routine_time(species.to_script(), hour) {
        script_time::RoutineTime::Sleep => RoutineTime::Sleep,
        script_time::RoutineTime::Work => RoutineTime::Work,
        script_time::RoutineTime::Relax => RoutineTime::Relax,
    }
}

/// Anchor set a routine time selects [CA 11.2].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnchorKind {
    Sleeping,
    Working,
    Relaxing,
}

impl RoutineTime {
    pub fn anchor(self) -> AnchorKind {
        match self {
            Self::Sleep => AnchorKind::Sleeping,
            Self::Work => AnchorKind::Working,
            Self::Relax => AnchorKind::Relaxing,
        }
    }
}

/// `ZS_*` task names assigned by `SelectRoutineTask` (S:a9440) [CA 11.3].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RoutineTask {
    Arena,
    GuidePlayer,
    WaitForPlayer,
    FollowPlayer,
    FollowNpc,
    Smith,
    Grind,
    Cook,
    Barbecue,
    Alchemy,
    Study,
    CarryStones,
    Digger,
    Farmer,
    Woodwork,
    Lumberjack,
    Sleep,
    SlaveBabe,
    Boss,
    Bard,
    Preach,
    Guard,
    Trader,
    Pray,
    RunToAttackPoint,
    OrcParty,
    HangAround,
    AnimalSleep,
    Monster,
    Ambient,
}

impl RoutineTask {
    /// Map a script state name to the task the simulation tracks; `None` for states that are not
    /// daily-routine tasks.
    pub fn from_state(state: ScriptState) -> Option<Self> {
        use RoutineTask::*;
        Some(match state {
            ScriptState::Arena => Arena,
            ScriptState::GuidePlayer => GuidePlayer,
            ScriptState::WaitForPlayer => WaitForPlayer,
            ScriptState::FollowPlayer => FollowPlayer,
            ScriptState::FollowNpc => FollowNpc,
            ScriptState::Smith => Smith,
            ScriptState::Grind => Grind,
            ScriptState::Cook => Cook,
            ScriptState::Barbecue => Barbecue,
            ScriptState::Alchemy => Alchemy,
            ScriptState::Study => Study,
            ScriptState::CarryStones => CarryStones,
            ScriptState::Digger => Digger,
            ScriptState::Farmer => Farmer,
            ScriptState::Woodwork => Woodwork,
            ScriptState::Lumberjack => Lumberjack,
            ScriptState::Sleep => Sleep,
            ScriptState::SlaveBabe => SlaveBabe,
            ScriptState::Boss => Boss,
            ScriptState::Bard => Bard,
            ScriptState::Preach => Preach,
            ScriptState::Guard => Guard,
            ScriptState::Trader => Trader,
            ScriptState::Pray => Pray,
            ScriptState::RunToAttackPoint => RunToAttackPoint,
            ScriptState::OrcParty => OrcParty,
            ScriptState::HangAround => HangAround,
            ScriptState::AnimalSleep => AnimalSleep,
            ScriptState::Monster => Monster,
            ScriptState::Ambient => Ambient,
            _ => return None,
        })
    }
}

/// UseType -> task (port of the `SelectRoutineTask` switch, S:a9440; the table lives in
/// `gothic_script::routine::select::task_for_use_type`). `use_type` is the engine's `gEUseType`
/// name; unknown names behave like a point without a use type.
///
/// Differences from the earlier rule doc, taken from the ported code: Orcs do **not** run to
/// attack points (they hang around), and any non-Human, non-Orc species (Goblins included) uses
/// the animal rules.
pub fn task_for_use_type(species: Species, use_type: &str) -> RoutineTask {
    let use_type = gothic_script::enums::UseType::from_name(use_type)
        .unwrap_or(gothic_script::enums::UseType::None);
    let state = script_task_for_use_type(species.to_script(), use_type);
    RoutineTask::from_state(state).unwrap_or(RoutineTask::HangAround)
}

/// Inputs of the `SelectRoutineTask` priority list [CA 11.3].
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskContext<'a> {
    pub running_arena: bool,
    /// The player's party leader is this NPC.
    pub guiding_player: bool,
    /// The NPC is led by the player; `waiting` selects `WaitForPlayer`.
    pub led_by_player: bool,
    pub waiting: bool,
    /// Controlled party follower of another NPC.
    pub controlled_follower: bool,
    /// UseType of the destination point, if any.
    pub destination_use: Option<&'a str>,
}

pub fn select_routine_task(species: Species, c: &TaskContext<'_>) -> RoutineTask {
    if c.running_arena {
        RoutineTask::Arena
    } else if c.guiding_player {
        RoutineTask::GuidePlayer
    } else if c.led_by_player {
        if c.waiting {
            RoutineTask::WaitForPlayer
        } else {
            RoutineTask::FollowPlayer
        }
    } else if c.controlled_follower {
        RoutineTask::FollowNpc
    } else {
        c.destination_use
            .map_or(RoutineTask::HangAround, |u| task_for_use_type(species, u))
    }
}

/// A concrete place an NPC should be at a given time of day.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutinePoint {
    pub pos: Pos,
    pub use_type: String,
    pub anchor: AnchorKind,
}

/// Resolve `(npc, routine name, time)` to a point. The routine-name -> point-set mapping
/// lives in `Game.dll` and is not recovered [CA 11.1 U]; the game plugs its own data in.
pub trait RoutineResolver: Send + Sync {
    fn resolve(&self, npc: &str, routine: &str, time: RoutineTime) -> Option<RoutinePoint>;
}

/// Table-driven resolver for tests and data-driven setups.
#[derive(Debug, Clone, Default)]
pub struct StaticRoutines {
    points: BTreeMap<(String, AnchorKind), RoutinePoint>,
}

impl StaticRoutines {
    pub fn add(&mut self, npc: &str, anchor: AnchorKind, pos: Pos, use_type: &str) {
        self.points.insert(
            (npc.to_ascii_lowercase(), anchor),
            RoutinePoint {
                pos,
                use_type: use_type.into(),
                anchor,
            },
        );
    }
}

impl RoutineResolver for StaticRoutines {
    fn resolve(&self, npc: &str, _routine: &str, time: RoutineTime) -> Option<RoutinePoint> {
        self.points
            .get(&(npc.to_ascii_lowercase(), time.anchor()))
            .cloned()
    }
}

/// Interval after which a new routine destination is picked (2160 s) [CA 11.1].
pub const REPICK_DESTINATION_SECS: u64 = 2160;
/// Sleepers re-check the routine time every 60 s of state time [CA 11.2].
pub const SLEEP_POLL_SECS: u64 = 60;
/// Follow distance for party member of rank `r` (0-based among non-waiting members) is
/// `(r + 3) * 150`; 500 when unranked [CA 11.4].
pub fn follow_distance(rank: Option<u32>) -> f32 {
    rank.map_or(500.0, |r| (r + 3) as f32 * 150.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_schedule() {
        use RoutineTime::*;
        assert_eq!(routine_time(Species::Human, 3.0), Sleep);
        assert_eq!(routine_time(Species::Human, 6.0), Work);
        assert_eq!(routine_time(Species::Orc, 19.99), Work);
        assert_eq!(routine_time(Species::Human, 20.0), Relax);
        assert_eq!(routine_time(Species::Wolf, 5.0), Sleep);
        assert_eq!(routine_time(Species::Wolf, 12.0), Work);
        assert_eq!(routine_time(Species::Boar, 4.0), Sleep);
        assert_eq!(routine_time(Species::Boar, 23.5), Sleep);
        assert_eq!(routine_time(Species::Boar, 12.0), Work);
        assert_eq!(routine_time(Species::Shadowbeast, 12.0), Sleep);
        assert_eq!(routine_time(Species::Shadowbeast, 22.0), Work);
        assert_eq!(routine_time(Species::Goblin, 3.0), Work);
        assert_eq!(Sleep.anchor(), AnchorKind::Sleeping);
    }

    #[test]
    fn clock_runs_twelve_to_one() {
        let mut c = GameClock::at(0, 6);
        // 300 real seconds = 3600 game seconds = 1 hour.
        c.advance(300_000, 12);
        assert!((c.hour_of_day() - 7.0).abs() < 1e-4);
        assert_eq!(c.total_hours(), 7);
        c.sleep_until_morning();
        assert!((c.hour_of_day() - 8.0).abs() < 1e-4);
        assert_eq!(c.total_hours(), 8);
        // Already past 08:00 -> the next morning.
        let mut late = GameClock::at(0, 21);
        late.sleep_until_morning();
        assert_eq!(late.total_hours(), 32);
    }

    #[test]
    fn use_type_table() {
        use RoutineTask::*;
        assert_eq!(task_for_use_type(Species::Human, "Anvil"), Smith);
        assert_eq!(task_for_use_type(Species::Orc, "Anvil"), Smith);
        assert_eq!(task_for_use_type(Species::Human, "Field"), Farmer);
        assert_eq!(task_for_use_type(Species::Orc, "Field"), HangAround);
        assert_eq!(task_for_use_type(Species::Orc, "OrcDrum"), OrcParty);
        assert_eq!(task_for_use_type(Species::Human, "OrcDrum"), HangAround);
        assert_eq!(task_for_use_type(Species::Human, "Bed"), Sleep);
        assert_eq!(task_for_use_type(Species::Human, "Stool"), HangAround);
        assert_eq!(task_for_use_type(Species::Wolf, "SleepGround"), AnimalSleep);
        assert_eq!(task_for_use_type(Species::Wolf, "Roam"), Monster);
        assert_eq!(task_for_use_type(Species::Meatbug, "Anything"), Ambient);
        assert_eq!(task_for_use_type(Species::Human, "Guard"), Guard);
    }

    #[test]
    fn task_priority() {
        use RoutineTask::*;
        let base = TaskContext {
            destination_use: Some("Forge"),
            ..TaskContext::default()
        };
        assert_eq!(select_routine_task(Species::Human, &base), Smith);
        let led = TaskContext {
            led_by_player: true,
            ..base
        };
        assert_eq!(select_routine_task(Species::Human, &led), FollowPlayer);
        let waiting = TaskContext {
            waiting: true,
            ..led
        };
        assert_eq!(select_routine_task(Species::Human, &waiting), WaitForPlayer);
        let arena = TaskContext {
            running_arena: true,
            ..waiting
        };
        assert_eq!(select_routine_task(Species::Human, &arena), Arena);
        let guide = TaskContext {
            guiding_player: true,
            ..base
        };
        assert_eq!(select_routine_task(Species::Human, &guide), GuidePlayer);
        assert_eq!(
            select_routine_task(Species::Human, &TaskContext::default()),
            HangAround
        );
        assert_eq!(follow_distance(Some(0)), 450.0);
        assert_eq!(follow_distance(Some(2)), 750.0);
        assert_eq!(follow_distance(None), 500.0);
    }

    #[test]
    fn static_resolver() {
        let mut r = StaticRoutines::default();
        r.add("Smith", AnchorKind::Working, Pos::flat(10.0, 20.0), "Anvil");
        let p = r.resolve("smith", "Start", RoutineTime::Work).unwrap();
        assert_eq!(p.use_type, "Anvil");
        assert!(r.resolve("smith", "Start", RoutineTime::Sleep).is_none());
    }
}
