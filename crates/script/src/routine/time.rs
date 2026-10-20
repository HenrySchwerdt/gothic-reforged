//! Time-of-day slots of the daily routine.

use crate::enums::Species;

/// `GetRoutineTime` result: which routine point is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoutineTime {
    /// 0: sleeping point.
    Sleep,
    /// 1: working point.
    Work,
    /// 2: relaxing point.
    Relax,
}

impl RoutineTime {
    /// The raw value the script returns.
    pub const fn raw(self) -> i32 {
        match self {
            Self::Sleep => 0,
            Self::Work => 1,
            Self::Relax => 2,
        }
    }
}

/// Sleep window `[from, to)` in hours, wrapping past midnight when `from > to`.
#[derive(Debug, Clone, Copy)]
struct Window {
    from: f32,
    to: f32,
}

impl Window {
    fn contains(self, h: f32) -> bool {
        if self.from <= self.to {
            (self.from..self.to).contains(&h)
        } else {
            h >= self.from || h < self.to
        }
    }
}

/// Port of `GetRoutineTime` (S:a99d0): the routine slot for a species at `hour` (0..24).
///
/// Human and Orc: sleep before 06:00, work until 20:00, relax afterwards. Herbivores and boars
/// (Scavenger, Boar, Bison, Rhino, Pig, Cow) sleep from 23:00 to 05:00; Wolf, Sabertooth and
/// Ripper sleep 04:00-06:00; Shadowbeast sleeps 06:00-20:00; every other species works all day.
pub fn routine_time(species: Species, hour: f32) -> RoutineTime {
    use Species as S;
    let sleep = |w: Window| {
        if w.contains(hour) {
            RoutineTime::Sleep
        } else {
            RoutineTime::Work
        }
    };
    match species {
        S::Human | S::Orc => {
            if hour < 6.0 {
                RoutineTime::Sleep
            } else if hour >= 20.0 {
                RoutineTime::Relax
            } else {
                RoutineTime::Work
            }
        }
        S::Scavenger | S::Boar | S::Bison | S::Rhino | S::Pig | S::Cow => sleep(Window {
            from: 23.0,
            to: 5.0,
        }),
        S::Wolf | S::Sabertooth | S::Ripper => sleep(Window { from: 4.0, to: 6.0 }),
        S::Shadowbeast => sleep(Window {
            from: 6.0,
            to: 20.0,
        }),
        _ => RoutineTime::Work,
    }
}

/// Port of `GetTravellingWalkMode` (S:a9c00): `2` (fast) for Minecrawler, Scavenger, Wolf,
/// Bison, Rhino and Snapper, `1` otherwise.
pub fn travelling_walk_mode(species: Species) -> u32 {
    use Species as S;
    match species {
        S::Minecrawler | S::Scavenger | S::Wolf | S::Bison | S::Rhino | S::Snapper => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_per_species() {
        use RoutineTime::*;
        assert_eq!(routine_time(Species::Human, 3.0), Sleep);
        assert_eq!(routine_time(Species::Human, 6.0), Work);
        assert_eq!(routine_time(Species::Orc, 19.99), Work);
        assert_eq!(routine_time(Species::Human, 20.0), Relax);
        assert_eq!(routine_time(Species::Wolf, 5.0), Sleep);
        assert_eq!(routine_time(Species::Wolf, 12.0), Work);
        assert_eq!(routine_time(Species::Boar, 4.0), Sleep);
        assert_eq!(routine_time(Species::Boar, 23.5), Sleep);
        assert_eq!(routine_time(Species::Boar, 5.0), Work);
        assert_eq!(routine_time(Species::Boar, 12.0), Work);
        assert_eq!(routine_time(Species::Shadowbeast, 12.0), Sleep);
        assert_eq!(routine_time(Species::Shadowbeast, 22.0), Work);
        assert_eq!(routine_time(Species::Goblin, 3.0), Work);
        assert_eq!(RoutineTime::Relax.raw(), 2);
    }

    #[test]
    fn walk_modes() {
        assert_eq!(travelling_walk_mode(Species::Wolf), 2);
        assert_eq!(travelling_walk_mode(Species::Snapper), 2);
        assert_eq!(travelling_walk_mode(Species::Human), 1);
    }
}
