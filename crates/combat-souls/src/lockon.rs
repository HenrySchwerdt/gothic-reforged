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
        let best = cands
            .iter()
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
                sa.total_cmp(&sb)
            });
        match best {
            Some((id, _, _)) => {
                self.target = Some(id);
                LockChange::Locked(id)
            }
            None => LockChange::Recenter,
        }
    }

    /// Move the lock to the neighbour on the given side (`+1` right of the screen, `-1` left).
    pub fn switch(
        &mut self,
        dir: i8,
        hero: Vec3,
        cands: &[Candidate],
        t: &Tuning,
        los: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> LockChange {
        let Some(cur) = self.target.and_then(|id| cands.iter().find(|c| c.id == id)) else {
            return LockChange::None;
        };
        if dir == 0 {
            return LockChange::None;
        }
        let base = yaw_of(flat(cur.pos - hero));
        // A positive yaw difference turns toward -X, which is the left of a -Z view.
        let side = if dir > 0 { -1.0 } else { 1.0 };
        let next = cands
            .iter()
            .filter(|c| c.id != cur.id)
            .filter(|c| flat(c.pos - hero).length() <= t.lock_range && los(hero, c.pos))
            .filter_map(|c| {
                let diff = angle_diff(base, yaw_of(flat(c.pos - hero)));
                (diff * side > 0.0).then_some((c.id, diff.abs()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match next {
            Some((id, _)) => {
                self.target = Some(id);
                LockChange::Locked(id)
            }
            None => LockChange::None,
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
        let mut lock = LockOn { target: Some(2) };
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
        let mut lock = LockOn { target: Some(1) };
        assert!(lock.validate(Vec3::ZERO, &[at(1, 0.0, 5000.0)], &t, &open));
        assert_eq!(lock.target, None);
        lock.target = Some(1);
        assert!(lock.validate(Vec3::ZERO, &[], &t, &open));
    }
}
