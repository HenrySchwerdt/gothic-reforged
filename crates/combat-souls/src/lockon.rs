//! Lock-on target selection: pick the foe nearest the view centre, switch left / right with the
//! mouse wheel or the right stick, break on distance, death or lost line of sight.
use crate::math::{angle_diff, flat, yaw_of};
use crate::tuning::Tuning;
use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub id: u32,
    pub pos: Vec3,
}

/// What the lock-on did on a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockChange {
    None,
    Locked(u32),
    Unlocked,
    /// Pressed with nothing to lock onto: the camera swings behind the hero.
    Recenter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockOn {
    pub target: Option<u32>,
    /// Ticks the target has been out of sight (the lock survives `lock_los_grace`).
    pub unseen_ticks: u16,
}

/// Half-angle of the cone around the view direction in which candidates are considered.
const VIEW_CONE: f32 = 1.75;

impl LockOn {
    /// Toggle the lock. `los` answers whether the hero can see a point from another.
    pub fn press(
        &mut self,
        hero: Vec3,
        cam_yaw: f32,
        cands: &[Candidate],
        t: &Tuning,
        los: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> LockChange {
        if self.target.take().is_some() {
            return LockChange::Unlocked;
        }
        self.unseen_ticks = 0;
        match best(hero, cam_yaw, cands, t, los, None) {
            Some(id) => {
                self.target = Some(id);
                LockChange::Locked(id)
            }
            None => LockChange::Recenter,
        }
    }

    /// Move the lock to the neighbour on the given side (`+1` right of the screen, `-1` left).
    /// Only foes within `lock_switch_max_deg` of the current target are reachable (no jumping
    /// to one behind the hero); among foes at nearly the same angle the nearer one wins.
    pub fn switch(
        &mut self,
        dir: i8,
        hero: Vec3,
        cands: &[Candidate],
        t: &Tuning,
        los: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> LockChange {
        if dir == 0 || self.target.is_none() {
            return LockChange::None;
        }
        let Some(cur) = self.target.and_then(|id| cands.iter().find(|c| c.id == id)) else {
            // The target vanished this tick (died): relock on the nearest in view.
            let toward = yaw_of(flat(cands.first().map_or(hero, |c| c.pos) - hero));
            return match best(hero, toward, cands, t, los, None) {
                Some(id) => {
                    self.target = Some(id);
                    self.unseen_ticks = 0;
                    LockChange::Locked(id)
                }
                None => {
                    self.target = None;
                    LockChange::Unlocked
                }
            };
        };
        let base = yaw_of(flat(cur.pos - hero));
        // A positive yaw difference turns toward -X, which is the left of a -Z view.
        let side = if dir > 0 { -1.0 } else { 1.0 };
        let max = t.lock_switch_max_deg.to_radians();
        let next = cands
            .iter()
            .filter(|c| c.id != cur.id)
            .filter(|c| flat(c.pos - hero).length() <= t.lock_range && los(hero, c.pos))
            .filter_map(|c| {
                let diff = angle_diff(base, yaw_of(flat(c.pos - hero)));
                (diff * side > 0.0 && diff.abs() <= max).then_some((
                    c.id,
                    diff.abs(),
                    flat(c.pos - hero).length(),
                ))
            })
            .min_by(|a, b| {
                // Within ~3 degrees the angle is a tie and the nearer foe wins.
                if (a.1 - b.1).abs() < 0.05 {
                    a.2.total_cmp(&b.2)
                } else {
                    a.1.total_cmp(&b.1)
                }
            });
        match next {
            Some((id, _, _)) => {
                self.target = Some(id);
                self.unseen_ticks = 0;
                LockChange::Locked(id)
            }
            None => LockChange::None,
        }
    }

    /// Per-tick upkeep: break on distance at once, on lost sight after `lock_los_grace`, and
    /// on the target's death retarget (if `lock_retarget_on_kill`) to the foe nearest the view
    /// direction `cam_yaw`.
    pub fn update(
        &mut self,
        hero: Vec3,
        cam_yaw: f32,
        cands: &[Candidate],
        t: &Tuning,
        los: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> LockChange {
        let Some(id) = self.target else {
            return LockChange::None;
        };
        match cands.iter().find(|c| c.id == id) {
            None => {
                self.unseen_ticks = 0;
                let next = if t.lock_retarget_on_kill {
                    best(hero, cam_yaw, cands, t, los, Some(id))
                } else {
                    None
                };
                self.target = next;
                next.map_or(LockChange::Unlocked, LockChange::Locked)
            }
            Some(c) if flat(c.pos - hero).length() > t.lock_break_range => {
                self.target = None;
                self.unseen_ticks = 0;
                LockChange::Unlocked
            }
            Some(c) if !los(hero, c.pos) => {
                self.unseen_ticks = self.unseen_ticks.saturating_add(1);
                if f32::from(self.unseen_ticks) * crate::tuning::DT > t.lock_los_grace {
                    self.target = None;
                    self.unseen_ticks = 0;
                    LockChange::Unlocked
                } else {
                    LockChange::None
                }
            }
            Some(_) => {
                self.unseen_ticks = 0;
                LockChange::None
            }
        }
    }

    /// Drops the lock when the target is gone, too far or out of sight.
    pub fn validate(
        &mut self,
        hero: Vec3,
        cands: &[Candidate],
        t: &Tuning,
        los: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> bool {
        let Some(id) = self.target else {
            return false;
        };
        let keep = cands
            .iter()
            .find(|c| c.id == id)
            .is_some_and(|c| flat(c.pos - hero).length() <= t.lock_break_range && los(hero, c.pos));
        if !keep {
            self.target = None;
        }
        !keep
    }
}

/// The foe a fresh lock picks: in range and sight, inside the view cone, scored by angle off
/// the view centre plus a little distance.
fn best(
    hero: Vec3,
    cam_yaw: f32,
    cands: &[Candidate],
    t: &Tuning,
    los: &dyn Fn(Vec3, Vec3) -> bool,
    exclude: Option<u32>,
) -> Option<u32> {
    cands
        .iter()
        .filter(|c| Some(c.id) != exclude)
        .filter(|c| {
            let d = flat(c.pos - hero);
            d.length() <= t.lock_range && los(hero, c.pos)
        })
        .map(|c| {
            let d = flat(c.pos - hero);
            let off = angle_diff(cam_yaw, yaw_of(d)).abs();
            (c.id, off, d.length())
        })
        .filter(|&(_, off, _)| off <= VIEW_CONE)
        .min_by(|a, b| {
            let sa = a.1 + a.2 / t.lock_range * 0.6;
            let sb = b.1 + b.2 / t.lock_range * 0.6;
            sa.total_cmp(&sb).then(a.0.cmp(&b.0))
        })
        .map(|(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::dir_of;

    fn open(_: Vec3, _: Vec3) -> bool {
        true
    }

    fn at(id: u32, yaw: f32, dist: f32) -> Candidate {
        Candidate {
            id,
            pos: dir_of(yaw) * dist,
        }
    }

    #[test]
    fn press_picks_the_foe_nearest_the_view_centre_and_toggles() {
        let t = Tuning::default();
        let cands = [at(1, 0.9, 600.0), at(2, 0.1, 900.0), at(3, 3.0, 300.0)];
        let mut lock = LockOn::default();
        assert_eq!(
            lock.press(Vec3::ZERO, 0.0, &cands, &t, &open),
            LockChange::Locked(2)
        );
        assert_eq!(
            lock.press(Vec3::ZERO, 0.0, &cands, &t, &open),
            LockChange::Unlocked
        );
        assert_eq!(
            lock.press(Vec3::ZERO, 0.0, &[], &t, &open),
            LockChange::Recenter
        );
    }

    #[test]
    fn switching_walks_across_the_screen() {
        let t = Tuning::default();
        // yaw > 0 is left of a -Z view, yaw < 0 is right.
        let cands = [at(1, 0.6, 700.0), at(2, 0.0, 700.0), at(3, -0.6, 700.0)];
        let mut lock = LockOn {
            target: Some(2),
            ..LockOn::default()
        };
        assert_eq!(
            lock.switch(1, Vec3::ZERO, &cands, &t, &open),
            LockChange::Locked(3)
        );
        assert_eq!(
            lock.switch(-1, Vec3::ZERO, &cands, &t, &open),
            LockChange::Locked(2)
        );
        assert_eq!(
            lock.switch(-1, Vec3::ZERO, &cands, &t, &open),
            LockChange::Locked(1)
        );
        assert_eq!(
            lock.switch(-1, Vec3::ZERO, &cands, &t, &open),
            LockChange::None
        );
    }

    #[test]
    fn lock_breaks_when_the_target_leaves_range_or_dies() {
        let t = Tuning::default();
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        assert!(lock.validate(Vec3::ZERO, &[at(1, 0.0, 5000.0)], &t, &open));
        assert_eq!(lock.target, None);
        lock.target = Some(1);
        assert!(lock.validate(Vec3::ZERO, &[], &t, &open));
    }

    #[test]
    fn sight_loss_has_a_grace_period() {
        let t = Tuning::default();
        let cands = [at(1, 0.0, 700.0)];
        let blocked = |_: Vec3, _: Vec3| false;
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        let grace_ticks = (t.lock_los_grace / crate::tuning::DT) as usize;
        for _ in 0..grace_ticks {
            assert_eq!(
                lock.update(Vec3::ZERO, 0.0, &cands, &t, &blocked),
                LockChange::None
            );
        }
        assert_eq!(lock.target, Some(1), "a short occlusion keeps the lock");
        // Seen again: the timer resets.
        lock.update(Vec3::ZERO, 0.0, &cands, &t, &open);
        assert_eq!(lock.unseen_ticks, 0);
        let mut broke = false;
        for _ in 0..=grace_ticks + 1 {
            broke |= lock.update(Vec3::ZERO, 0.0, &cands, &t, &blocked) == LockChange::Unlocked;
        }
        assert!(broke && lock.target.is_none());
        // Distance breaks at once.
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        assert_eq!(
            lock.update(Vec3::ZERO, 0.0, &[at(1, 0.0, 5000.0)], &t, &open),
            LockChange::Unlocked
        );
    }

    #[test]
    fn killing_the_target_moves_the_lock_to_the_next_in_view() {
        let t = Tuning::default();
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        // Foe 1 died (gone from the candidates); 3 is behind the hero, 2 in view.
        let cands = [at(2, 0.3, 800.0), at(3, 3.0, 200.0)];
        assert_eq!(
            lock.update(Vec3::ZERO, 0.0, &cands, &t, &open),
            LockChange::Locked(2)
        );
        let off = Tuning {
            lock_retarget_on_kill: false,
            ..t
        };
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        assert_eq!(
            lock.update(Vec3::ZERO, 0.0, &cands, &off, &open),
            LockChange::Unlocked
        );
        // Nobody left in view: unlock.
        let mut lock = LockOn {
            target: Some(1),
            ..LockOn::default()
        };
        assert_eq!(
            lock.update(Vec3::ZERO, 0.0, &[at(3, 3.0, 200.0)], &t, &open),
            LockChange::Unlocked
        );
    }

    #[test]
    fn switching_skips_foes_behind_and_prefers_the_nearer_on_ties() {
        let t = Tuning::default();
        // Current target ahead; 4 is far to the right-behind (beyond the max angle).
        let cands = [at(2, 0.0, 700.0), at(4, -2.4, 400.0)];
        let mut lock = LockOn {
            target: Some(2),
            ..LockOn::default()
        };
        assert_eq!(
            lock.switch(1, Vec3::ZERO, &cands, &t, &open),
            LockChange::None
        );
        // Two foes at the same angle to the right: the nearer one.
        let cands = [at(2, 0.0, 700.0), at(5, -0.5, 1200.0), at(6, -0.51, 500.0)];
        assert_eq!(
            lock.switch(1, Vec3::ZERO, &cands, &t, &open),
            LockChange::Locked(6)
        );
        // Out of sight candidates are skipped.
        let mut lock = LockOn {
            target: Some(2),
            ..LockOn::default()
        };
        let hidden = |_: Vec3, p: Vec3| p.x.abs() < 1.0;
        assert_eq!(
            lock.switch(
                1,
                Vec3::ZERO,
                &[at(2, 0.0, 700.0), at(7, -0.4, 600.0)],
                &t,
                &hidden
            ),
            LockChange::None
        );
    }

    #[test]
    fn switching_after_the_target_died_relocks() {
        let t = Tuning::default();
        let mut lock = LockOn {
            target: Some(9),
            ..LockOn::default()
        };
        assert_eq!(
            lock.switch(1, Vec3::ZERO, &[at(2, 0.1, 700.0)], &t, &open),
            LockChange::Locked(2)
        );
    }
}
