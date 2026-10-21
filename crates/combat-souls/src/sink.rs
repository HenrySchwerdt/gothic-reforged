//! The seam to the damage pipeline. The Souls flow decides *whether and when* a swing connects
//! (i-frames, guard, parry, poise); how much it hurts is always answered by a [`HitSink`].
//! In the game that is [`SimSink`], the ported `Sim::hit` -> `resolve_hit` -> `AssessHit` chain,
//! so stats, weapons and armour stay Gothic. [`SimpleSink`] is a toy for headless flow tests.
use crate::actions::StrikeKind;
use gothic_sim::actor::Actor;
use gothic_sim::combat::{Attack, resolve_hit};
use gothic_sim::sim::Sim;
use gothic_sim::types::{Action, ActorId};
use gothic_sim::world::WorldQuery;
use std::collections::BTreeMap;

/// One swing that has made contact (or would, for a preview).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub attacker: u32,
    pub victim: u32,
    pub kind: StrikeKind,
    /// A fully charged heavy attack.
    pub charged: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitReport {
    /// Final damage figure of the pipeline.
    pub damage: i32,
    /// Victim HP after the hit and its maximum.
    pub hp: f32,
    pub max_hp: f32,
    pub killed: bool,
    /// The pipeline refused the hit (immune, invulnerable, ...).
    pub ignored: bool,
}

pub trait HitSink {
    /// Resolve `strike`. With `commit == false` the victim must not change: the report says what
    /// would happen (used to size guard stamina damage).
    fn resolve(&mut self, strike: &Strike, commit: bool) -> HitReport;
    /// Current and maximum HP of a fighter.
    fn hp(&self, id: u32) -> (f32, f32);
}

/// Flat damage table for flow tests: `max(1, attack * kind multiplier - defence)`.
#[derive(Default, Debug, Clone)]
pub struct SimpleSink {
    fighters: BTreeMap<u32, (f32, f32, f32, f32)>,
    /// Every committed strike, in order.
    pub log: Vec<Strike>,
}

impl SimpleSink {
    pub fn add(&mut self, id: u32, hp: f32, attack: f32, defence: f32) {
        self.fighters.insert(id, (hp, hp, attack, defence));
    }

    pub fn multiplier(kind: StrikeKind, charged: bool) -> f32 {
        let m = match kind {
            StrikeKind::Light | StrikeKind::Foe => 1.0,
            StrikeKind::Heavy => 2.0,
            StrikeKind::Charged => 3.0,
            StrikeKind::Counter => 2.5,
            StrikeKind::Riposte => 4.0,
        };
        if charged { m * 1.5 } else { m }
    }
}

impl HitSink for SimpleSink {
    fn resolve(&mut self, s: &Strike, commit: bool) -> HitReport {
        let atk = self.fighters.get(&s.attacker).map_or(10.0, |f| f.2);
        let Some(v) = self.fighters.get_mut(&s.victim) else {
            return HitReport {
                ignored: true,
                ..HitReport::default()
            };
        };
        let dmg = (atk * Self::multiplier(s.kind, s.charged) - v.3)
            .max(1.0)
            .round();
        let hp = (v.0 - dmg).max(0.0);
        if commit {
            v.0 = hp;
            self.log.push(*s);
        }
        HitReport {
            damage: dmg as i32,
            hp,
            max_hp: v.1,
            killed: hp <= 0.0,
            ignored: false,
        }
    }

    fn hp(&self, id: u32) -> (f32, f32) {
        self.fighters.get(&id).map_or((0.0, 0.0), |f| (f.0, f.1))
    }
}

/// Maps souls fighter ids onto simulation actors and calls the ported pipeline.
///
/// The move class is chosen from the strike: light -> `QuickAttack`, heavy / counter ->
/// `PowerAttack`, riposte -> `HackAttack` (the original's double-damage blow). Charged swings add
/// a hit multiplier. The original rules (stats, protection, parry classes, stagger lock, kill and
/// defeat bookkeeping, perception) then run unchanged inside `Sim::hit`.
pub struct SimSink<'a, W: WorldQuery> {
    pub sim: &'a mut Sim<W>,
    pub actors: BTreeMap<u32, ActorId>,
}

impl<'a, W: WorldQuery> SimSink<'a, W> {
    pub fn new(sim: &'a mut Sim<W>) -> Self {
        Self {
            sim,
            actors: BTreeMap::new(),
        }
    }

    pub fn bind(&mut self, id: u32, actor: ActorId) {
        self.actors.insert(id, actor);
    }

    fn action_of(s: &Strike) -> (Action, f32) {
        match (s.kind, s.charged) {
            (StrikeKind::Light, _) => (Action::QuickAttack, 1.0),
            (StrikeKind::Heavy, false) => (Action::PowerAttack, 1.0),
            (StrikeKind::Heavy, true) | (StrikeKind::Charged, _) => (Action::PowerAttack, 1.5),
            (StrikeKind::Counter, _) => (Action::PowerAttack, 1.25),
            (StrikeKind::Riposte, _) => (Action::HackAttack, 1.0),
            (StrikeKind::Foe, _) => (Action::Attack, 1.0),
        }
    }

    fn build(att: &Actor, action: Action, mult: f32) -> Attack {
        let mut a = Attack::melee(att);
        a.hit_mult *= mult;
        let _ = action;
        a
    }
}

impl<W: WorldQuery> HitSink for SimSink<'_, W> {
    fn resolve(&mut self, s: &Strike, commit: bool) -> HitReport {
        let (Some(&att), Some(&vic)) = (self.actors.get(&s.attacker), self.actors.get(&s.victim))
        else {
            return HitReport {
                ignored: true,
                ..HitReport::default()
            };
        };
        let (action, mult) = Self::action_of(s);
        if !commit {
            let mut a = self.sim.actor(att).clone();
            a.action = action;
            let mut v = self.sim.actor(vic).clone();
            let attack = Self::build(&a, action, mult);
            let out = resolve_hit(
                &self.sim.cfg,
                &mut self.sim.rng.clone(),
                &a,
                &mut v,
                &attack,
            );
            return HitReport {
                damage: out.damage,
                hp: v.hp as f32,
                max_hp: v.hp_max(&self.sim.cfg) as f32,
                killed: out.killed(),
                ignored: out.ignored.is_some(),
            };
        }
        let previous = {
            let a = self.sim.actor_mut(att);
            let p = (a.action, a.target);
            a.action = action;
            a.target = Some(vic);
            p
        };
        let attack = Self::build(self.sim.actor(att), action, mult);
        let out = self.sim.hit(att, vic, attack);
        {
            let a = self.sim.actor_mut(att);
            a.action = previous.0;
            a.target = previous.1;
        }
        let v = self.sim.actor(vic);
        HitReport {
            damage: out.damage,
            hp: v.hp as f32,
            max_hp: v.hp_max(&self.sim.cfg) as f32,
            killed: out.killed() || v.is_down_or_dead(),
            ignored: out.ignored.is_some(),
        }
    }

    fn hp(&self, id: u32) -> (f32, f32) {
        self.actors.get(&id).map_or((0.0, 0.0), |&a| {
            let v = self.sim.actor(a);
            (v.hp as f32, v.hp_max(&self.sim.cfg) as f32)
        })
    }
}
