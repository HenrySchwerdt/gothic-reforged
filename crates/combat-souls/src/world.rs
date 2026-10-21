//! The Souls combat world: the hero, the foes, the lock-on and the hit resolution between them.
//! One [`SoulsWorld::step`] is one 60 Hz tick.
use crate::events::{Event, HitResult};
use crate::foe::{Foe, FoeProfile};
use crate::hero::{Hero, HeroCtx, Incoming, Input, Motion, State, kinematic_move};
use crate::lockon::{Candidate, LockChange, LockOn};
use crate::math::{dir_of, flat, yaw_of};
use crate::sink::HitSink;
use crate::tuning::Tuning;
use glam::Vec3;

/// What the core needs from the 3D world. The Bevy host implements it with the physics
/// controller and collision rays; tests and the demo use [`OpenField`].
pub trait Env {
    fn line_of_sight(&self, _from: Vec3, _to: Vec3) -> bool {
        true
    }
    /// Carries out the hero's locomotion for this tick (updates `pos`, `yaw`, `speed`).
    fn move_hero(&mut self, hero: &mut Hero, motion: &Motion, t: &Tuning) {
        kinematic_move(hero, motion, t);
    }
    /// Moves a foe by `delta`, kept out of walls.
    fn slide(&mut self, pos: Vec3, delta: Vec3) -> Vec3 {
        pos + delta
    }
}

/// An empty plane, optionally closed by a circular wall.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenField {
    pub arena_radius: Option<f32>,
}

impl OpenField {
    fn clamp(&self, p: Vec3) -> Vec3 {
        match self.arena_radius {
            Some(r) if flat(p).length() > r => {
                let f = flat(p).normalize() * r;
                Vec3::new(f.x, p.y, f.z)
            }
            _ => p,
        }
    }
}

impl Env for OpenField {
    fn move_hero(&mut self, hero: &mut Hero, motion: &Motion, t: &Tuning) {
        kinematic_move(hero, motion, t);
        hero.pos = self.clamp(hero.pos);
    }
    fn slide(&mut self, pos: Vec3, delta: Vec3) -> Vec3 {
        self.clamp(pos + delta)
    }
}

pub struct SoulsWorld {
    pub tuning: Tuning,
    pub hero: Hero,
    pub foes: Vec<Foe>,
    pub lock: LockOn,
    /// Set for one tick when lock-on was pressed with nothing to lock onto.
    pub recenter: bool,
    /// Events of the last tick, oldest first.
    pub events: Vec<Event>,
    pub tick: u64,
}

impl SoulsWorld {
    pub fn new(spawn: Vec3, hero_hp: f32, tuning: Tuning) -> Self {
        Self {
            hero: Hero::new(0, spawn, hero_hp, &tuning),
            tuning,
            foes: Vec::new(),
            lock: LockOn::default(),
            recenter: false,
            events: Vec::new(),
            tick: 0,
        }
    }

    pub fn add_foe(&mut self, id: u32, pos: Vec3, hp: f32, profile: FoeProfile) -> &mut Foe {
        let mut foe = Foe::new(id, pos, hp, profile);
        foe.yaw = yaw_of(flat(self.hero.pos - pos));
        self.foes.push(foe);
        self.foes.last_mut().expect("just pushed")
    }

    pub fn foe(&self, id: u32) -> Option<&Foe> {
        self.foes.iter().find(|f| f.id == id)
    }

    pub fn foe_mut(&mut self, id: u32) -> Option<&mut Foe> {
        self.foes.iter_mut().find(|f| f.id == id)
    }

    pub fn lock_pos(&self) -> Option<Vec3> {
        self.lock.target.and_then(|id| self.foe(id)).map(|f| f.pos)
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.foes
            .iter()
            .filter(|f| f.alive())
            .map(|f| Candidate {
                id: f.id,
                pos: f.pos,
            })
            .collect()
    }

    /// Advances the world by one tick and returns the events it produced.
    pub fn step(&mut self, inp: &Input, sink: &mut dyn HitSink, env: &mut dyn Env) -> &[Event] {
        self.events.clear();
        self.recenter = false;
        self.tick += 1;
        let t = self.tuning;

        // ---- lock-on --------------------------------------------------------------------------
        let cands = self.candidates();
        let los = |a: Vec3, b: Vec3| env.line_of_sight(a + Vec3::Y * 150.0, b + Vec3::Y * 150.0);
        let before = self.lock.target;
        if self.hero.is_dead() {
            self.lock.target = None;
        } else if inp.lock {
            if self
                .lock
                .press(self.hero.pos, inp.cam_yaw, &cands, &t, &los)
                == LockChange::Recenter
            {
                self.recenter = true;
            }
        } else if inp.switch != 0 {
            self.lock
                .switch(inp.switch, self.hero.pos, &cands, &t, &los);
        }
        self.lock.validate(self.hero.pos, &cands, &t, &los);
        if self.lock.target != before {
            self.events.push(Event::LockChanged {
                target: self.lock.target,
            });
        }

        // ---- hero -----------------------------------------------------------------------------
        let target = self.lock_pos();
        let riposte = self.riposte_candidate();
        let ctx = HeroCtx { target, riposte };
        let motion = self.hero.step(inp, &ctx, &t);
        env.move_hero(&mut self.hero, &motion, &t);

        // ---- foes -----------------------------------------------------------------------------
        let hero_pos = self.hero.pos;
        let hero_alive = !self.hero.is_dead();
        for foe in &mut self.foes {
            let delta = foe.step(hero_pos, hero_alive);
            if delta != Vec3::ZERO {
                foe.pos = env.slide(foe.pos, delta);
            }
        }
        self.separate(env);

        // ---- resolution -----------------------------------------------------------------------
        self.hero_attacks(sink);
        self.foe_attacks(sink);

        let mut out = std::mem::take(&mut self.hero.events);
        for foe in &mut self.foes {
            out.append(&mut foe.events);
        }
        self.events.append(&mut out);
        &self.events
    }

    /// A staggered foe in front of the hero within riposte reach.
    fn riposte_candidate(&self) -> Option<(u32, Vec3)> {
        let h = &self.hero;
        self.foes
            .iter()
            .filter(|f| f.riposte_ready(h.pos, &self.tuning))
            .filter(|f| {
                self.lock.target.is_none_or(|id| id == f.id)
                    && flat(f.pos - h.pos).normalize_or_zero().dot(h.facing()) > 0.5
            })
            .min_by(|a, b| {
                flat(a.pos - h.pos)
                    .length()
                    .total_cmp(&flat(b.pos - h.pos).length())
            })
            .map(|f| (f.id, f.pos))
    }

    /// Bodies are solid: the hero is pushed out of living foes.
    fn separate(&mut self, env: &mut dyn Env) {
        for foe in self.foes.iter().filter(|f| f.alive()) {
            let away = flat(self.hero.pos - foe.pos);
            let min = self.tuning.hero_radius + foe.profile.radius;
            let d = away.length();
            if d < min {
                let dir = if d > 1e-3 {
                    away / d
                } else {
                    -dir_of(self.hero.yaw)
                };
                self.hero.pos = env.slide(self.hero.pos, dir * (min - d));
            }
        }
    }

    fn hero_attacks(&mut self, sink: &mut dyn HitSink) {
        let Some(hit) = self.hero.active_hit() else {
            return;
        };
        let t = self.tuning;
        let h = &self.hero;
        let facing = h.facing();
        let arc = hit.arc_deg.to_radians().cos();
        let pick = self
            .foes
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive() && hit.victim.is_none_or(|v| v == f.id))
            .filter(|(_, f)| {
                let d = flat(f.pos - h.pos);
                d.length() <= t.weapon_reach + f.profile.radius
                    && d.normalize_or_zero().dot(facing) >= arc
            })
            .min_by(|a, b| {
                flat(a.1.pos - h.pos)
                    .length()
                    .total_cmp(&flat(b.1.pos - h.pos).length())
            })
            .map(|(i, _)| i);
        let Some(i) = pick else {
            return;
        };
        let hero_id = self.hero.id;
        let (report, broken) = self.foes[i].receive(&hit, hero_id, sink, &t);
        if report.ignored {
            return;
        }
        self.hero.mark_hit();
        self.hero.hit_stop = hit.stop * t.hit_stop_scale;
        self.events.push(Event::HeroHit {
            foe: self.foes[i].id,
            kind: hit.kind,
            damage: report.damage,
            killed: report.killed,
            poise_broken: broken,
        });
    }

    fn foe_attacks(&mut self, sink: &mut dyn HitSink) {
        let t = self.tuning;
        for i in 0..self.foes.len() {
            let Some(swing) = self.foes[i].live_swing() else {
                continue;
            };
            let to = flat(self.hero.pos - swing.from);
            let reach = swing.attack.reach + t.hero_radius;
            if to.length() > reach
                || to.normalize_or_zero().dot(dir_of(swing.yaw))
                    < swing.attack.arc_deg.to_radians().cos()
            {
                continue;
            }
            let foe_id = self.foes[i].id;
            let rec = self.hero.receive(
                &Incoming {
                    attacker: foe_id,
                    from: swing.from,
                    level: swing.attack.level,
                    poise: swing.attack.poise,
                    parryable: swing.attack.parryable,
                },
                sink,
                &t,
            );
            if rec.result != HitResult::Ignored {
                self.events.push(Event::HeroHurt {
                    foe: foe_id,
                    result: rec.result,
                    damage: rec.damage,
                    level: swing.attack.level,
                });
            }
            if matches!(rec.result, HitResult::Blocked | HitResult::GuardBroken) {
                self.hero.hit_stop = 0.05;
            }
            self.foes[i].swing_result(rec.result);
        }
    }

    pub fn hero_state_name(&self) -> &'static str {
        match self.hero.state {
            State::Free if self.hero.sprinting => "sprint",
            State::Free if self.hero.guarding => "guard",
            State::Free => "free",
            State::Act(a) => a.id.def().name,
            State::Dead { .. } => "dead",
        }
    }
}
