//! The speaking cooldown of the script runtime's speak helper (`Script_Game.dll` `S:baf0` with
//! its gate `S:bc20`).
//!
//! Every stock-voice line goes through one helper that is called with a *category* id (a
//! register argument chosen by the calling script function). The gate only lets humanoids speak;
//! categories 1, 3..=11 (except 2), 12, 13 and 14 keep the play time of their last line in a
//! table shared by **all** characters, and a new line of such a category waits until a
//! per-category span has passed. Categories 0 and 2 (and everything else) are never limited.
//!
//! Spans (doubles read from the DLL's data section): 0.006 for categories 1, 4, 11; 0.0018 for
//! 3, 5, 6, 8, 9, 10, 13; 0.12 for 7 and 14; 0.012 for 12. The unit of `GetPlayingTime` is not
//! recovered; the spans are applied as **game hours** [I] (0.0018 h = 6.5 game seconds, 0.12 h =
//! 7.2 game minutes), the only reading that gives sensible spoken-line gaps.
//!
//! The category of a line comes from the script state the speaker is in when it speaks (the
//! caller functions of the helper, found by scanning the DLL for its call sites) [I].
use gothic_script::routine::state::State;

/// Number of categories of the shared table.
pub const CATEGORIES: usize = 15;

/// Category of a line spoken by a character in `state`.
pub fn category_of(state: Option<State>) -> u8 {
    use State::*;
    match state {
        Some(Arena | ArenaLoop) => 7,
        Some(WatchFight | WatchFightGoto | WatchFightGoBack | WatchFightLoop) => 6,
        Some(WatchFightSpectator | WatchFightSpectatorLoop) => 6,
        Some(Flee | FleeGoBack | FleeGotoFleePoint | FleeObserveFollower) => 13,
        Some(AttackWaitForFlee | AttackWaitForFleeLoop) => 14,
        Some(ReactToSneak) => 11,
        Some(ObserveZonePlayer | ReactToZone | ReactToZoneLoop) => 8,
        Some(ReactToFootsteps) => 4,
        Some(UnconsciousEnd | MagicSleep | MagicSleepLoop | MagicHypnosis | MagicHypnosisLoop) => 4,
        Some(ReactToDamage | ReactToDamageEnd | ReactToDamageGoBack | ReactToDamageLoop) => 10,
        Some(
            ReactToWeapon
            | ReactToWeaponEnd
            | ReactToWeaponWaitForAttack
            | ReactToWeaponWaitForSecondWarn,
        ) => 9,
        Some(FollowPlayer | FollowPlayerLoop) => 1,
        Some(Attack) => 3,
        _ => 0,
    }
}

/// The span (game hours) a category waits after its last line, `None` for unlimited ones.
pub fn span_hours(category: u8) -> Option<f64> {
    match category {
        1 | 4 | 11 => Some(0.006),
        3 | 5 | 6 | 8 | 9 | 10 | 13 => Some(0.0018),
        7 | 14 => Some(0.12),
        12 => Some(0.012),
        _ => None,
    }
}

/// The shared table of last-line times.
#[derive(Debug, Clone, Default)]
pub struct BarkGate {
    last: [f64; CATEGORIES],
}

impl BarkGate {
    /// May a humanoid speak a line of `category` at game time `now_hours`? Records the line
    /// when it may.
    pub fn allow(&mut self, category: u8, now_hours: f64) -> bool {
        let i = usize::from(category).min(CATEGORIES - 1);
        if let Some(span) = span_hours(category)
            && self.last[i] != 0.0
            && now_hours - self.last[i] < span
        {
            return false;
        }
        self.last[i] = now_hours;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_categories_wait_and_unlimited_ones_do_not() {
        let mut g = BarkGate::default();
        assert!(g.allow(3, 10.0));
        assert!(!g.allow(3, 10.001)); // 0.0018 h = 6.5 s
        assert!(g.allow(3, 10.002));
        assert!(g.allow(0, 10.0) && g.allow(0, 10.0));
        assert!(g.allow(7, 11.0));
        assert!(!g.allow(7, 11.1));
        assert!(g.allow(7, 11.13));
    }

    #[test]
    fn states_map_to_the_caller_categories() {
        assert_eq!(category_of(Some(State::Flee)), 13);
        assert_eq!(category_of(Some(State::ReactToWeapon)), 9);
        assert_eq!(category_of(None), 0);
    }
}
