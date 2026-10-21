//! Enemy side of the flow: a poise gauge, stagger / parry vulnerability and the *telegraph*
//! attack cycle (wind-up, tracking cut-off, strike window, recovery).
//!
//! The same [`Foe`] serves two roles. In the arena demo and in tests it is autonomous (it chases
//! and picks attacks itself). In the game the NPC brain keeps deciding *that* it attacks and the
//! host calls [`Foe::command_attack`]; the foe then holds the raised weapon for the wind-up
//! before the swing becomes live. [`FoeProfile::classic`] is the original near-instant swing,
//! [`FoeProfile::with_telegraph`] the readable variant.
use crate::actions::{Hurt, StrikeKind};
use crate::events::{Event, HitResult};
use crate::hero::ActiveHit;
use crate::math::{dir_of, flat, turn_toward, yaw_of};
use crate::sink::{HitReport, HitSink, Strike};
use crate::tuning::{DT, Tuning};
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoeAttack {
    pub name: &'static str,
    /// Seconds the weapon is raised before the swing is live.
    pub windup: f32,
    /// The foe stops turning toward the target this many seconds into the wind-up.
    pub track_until: f32,
    /// Seconds the swing stays live.
    pub strike: f32,
    pub recover: f32,
    pub reach: f32,
    pub arc_deg: f32,
    pub level: Hurt,
    /// Poise damage dealt to the hero.
    pub poise: f32,
    pub parryable: bool,
    /// Forward lunge during the strike (cm).
    pub lunge: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoeProfile {
    pub name: &'static str,
    pub radius: f32,
    pub aggro: f32,
    pub speed: f32,
    pub turn_deg: f32,
    pub max_poise: f32,
    pub stagger_secs: f32,
    pub parried_secs: f32,
    pub cooldown: f32,
    pub attacks: Vec<FoeAttack>,
}

impl FoeProfile {
    /// Original-style foe: the swing comes almost at once and the foe tracks you until it lands.
    pub fn classic() -> Self {
        let atk = |name, level, poise, reach| FoeAttack {
            name,
            windup: 0.2,
            track_until: 0.2,
            strike: 0.18,
            recover: 0.45,
            reach,
            arc_deg: 60.0,
            level,
            poise,
            parryable: true,
            lunge: 40.0,
        };
        Self {
            name: "orc",
            radius: 55.0,
            aggro: 1400.0,
            speed: 330.0,
            turn_deg: 540.0,
            max_poise: 60.0,
            stagger_secs: 1.8,
            parried_secs: 1.6,
            cooldown: 0.5,
            attacks: vec![
                atk("slash", Hurt::Middle, 25.0, 230.0),
                atk("overhead", Hurt::Large, 40.0, 240.0),
                atk("sweep", Hurt::Small, 15.0, 250.0),
            ],
        }
    }

    /// The telegraphed variant of this profile: long raise, tracking cut off before the swing,
    /// a longer recovery that opens the foe up, a lunge that punishes standing still.
    pub fn with_telegraph(mut self) -> Self {
        for (i, a) in self.attacks.iter_mut().enumerate() {
            let slow = 1.0 + 0.25 * i as f32;
            a.windup = 0.75 * slow;
            a.track_until = a.windup * 0.55;
            a.strike = 0.22;
            a.recover = 0.9;
            a.lunge = 120.0;
        }
        self.cooldown = 1.1;
        self
    }

    pub fn telegraphed() -> Self {
        Self::classic().with_telegraph()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoeState {
    Idle,
    Chase,
    Windup(u8),
    Strike(u8),
    Recover,
    Flinch,
    Stagger { parried: bool },
    Dead,
}

#[derive(Clone, Debug)]
pub struct Foe {
    pub id: u32,
    pub pos: Vec3,
    pub yaw: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub poise: f32,
    pub state: FoeState,
    /// Seconds in the current state.
    pub t: f32,
    pub profile: FoeProfile,
    /// Picks its own attacks and chases; off when a brain drives it.
    pub autonomous: bool,
    /// The host simulation owns this enemy's swings (NPC brain of the real game); only poise,
    /// stagger and the riposte window are tracked here.
    pub observer: bool,
    pub hit_stop: f32,
    pub events: Vec<Event>,
    cooldown: f32,
    next: usize,
    last: usize,
    struck: bool,
    poise_wait: f32,
}

/// A live enemy swing.
#[derive(Clone, Copy, Debug)]
pub struct Swing {
    pub attack: FoeAttack,
    pub index: u8,
    pub from: Vec3,
    pub yaw: f32,
}

impl Foe {
    pub fn new(id: u32, pos: Vec3, hp: f32, profile: FoeProfile) -> Self {
        Self {
            id,
            pos,
            yaw: 0.0,
            hp,
            max_hp: hp,
            poise: profile.max_poise,
            state: FoeState::Idle,
            t: 0.0,
            profile,
            autonomous: true,
            observer: false,
            hit_stop: 0.0,
            events: Vec::new(),
            cooldown: 0.0,
            next: 0,
            last: 0,
            struck: false,
            poise_wait: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.state != FoeState::Dead
    }

    fn enter(&mut self, s: FoeState) {
        self.state = s;
        self.t = 0.0;
    }

    /// Windup progress 0..1 (for the telegraph glow), `None` outside a wind-up.
    pub fn windup_progress(&self) -> Option<f32> {
        match self.state {
            FoeState::Windup(i) => {
                Some((self.t / self.profile.attacks[i as usize].windup).min(1.0))
            }
            _ => None,
        }
    }

    /// A brain asks for attack `index`. Ignored unless the foe is free to act.
    pub fn command_attack(&mut self, index: usize) -> bool {
        if !matches!(self.state, FoeState::Idle | FoeState::Chase)
            || index >= self.profile.attacks.len()
        {
            return false;
        }
        self.struck = false;
        self.last = index;
        self.enter(FoeState::Windup(index as u8));
        self.events.push(Event::FoeWindup {
            foe: self.id,
            attack: index as u8,
        });
        let a = self.profile.attacks[index];
        self.events.push(Event::FoeTelegraph {
            foe: self.id,
            attack: index as u8,
            lead: a.windup,
            track: a.track_until.min(a.windup),
            parryable: a.parryable,
        });
        true
    }

    /// The enemy is open to a riposte.
    pub fn riposte_ready(&self, hero_pos: Vec3, t: &Tuning) -> bool {
        let FoeState::Stagger { parried } = self.state else {
            return false;
        };
        let total = if parried {
            self.profile.parried_secs
        } else {
            self.profile.stagger_secs
        };
        self.t < total - 0.3
            && flat(hero_pos - self.pos).length() <= t.riposte_reach + self.profile.radius
    }

    /// One tick. Returns the displacement the foe wants this tick (the world applies it).
    pub fn step(&mut self, hero_pos: Vec3, hero_alive: bool) -> Vec3 {
        if self.state == FoeState::Dead {
            return Vec3::ZERO;
        }
        if self.hit_stop > 0.0 {
            self.hit_stop -= DT;
            return Vec3::ZERO;
        }
        self.t += DT;
        self.cooldown = (self.cooldown - DT).max(0.0);
        self.poise_wait = (self.poise_wait - DT).max(0.0);
        if self.poise_wait <= 0.0 && !matches!(self.state, FoeState::Stagger { .. }) {
            self.poise = self.profile.max_poise;
        }
        let to = flat(hero_pos - self.pos);
        let dist = to.length();
        let goal_yaw = if dist > 1.0 { yaw_of(to) } else { self.yaw };
        let turn = self.profile.turn_deg.to_radians() * DT;
        let mut delta = Vec3::ZERO;

        match self.state {
            FoeState::Idle => {
                if self.autonomous && hero_alive && dist <= self.profile.aggro {
                    self.enter(FoeState::Chase);
                }
            }
            FoeState::Chase => {
                self.yaw = turn_toward(self.yaw, goal_yaw, turn);
                let a = self.profile.attacks[self.next % self.profile.attacks.len()];
                if !hero_alive || dist > self.profile.aggro * 1.6 {
                    self.enter(FoeState::Idle);
                } else if self.autonomous && self.cooldown <= 0.0 && dist <= a.reach * 0.85 {
                    let i = self.next % self.profile.attacks.len();
                    self.next += 1;
                    self.command_attack(i);
                } else if self.autonomous && dist > a.reach * 0.7 {
                    delta = dir_of(self.yaw) * self.profile.speed * DT;
                }
            }
            FoeState::Windup(i) => {
                let a = self.profile.attacks[i as usize];
                if self.t < a.track_until {
                    self.yaw = turn_toward(self.yaw, goal_yaw, turn);
                } else if self.t - DT < a.track_until && a.track_until < a.windup {
                    // The swing direction is fixed from here: the dodge cue.
                    self.events.push(Event::FoeCommit {
                        foe: self.id,
                        attack: i,
                    });
                }
                if self.t >= a.windup {
                    self.struck = false;
                    self.enter(FoeState::Strike(i));
                    self.events.push(Event::FoeStrike {
                        foe: self.id,
                        attack: i,
                    });
                }
            }
            FoeState::Strike(i) => {
                let a = self.profile.attacks[i as usize];
                if dist > a.reach * 0.45 {
                    delta = dir_of(self.yaw) * (a.lunge / a.strike) * DT;
                }
                if self.t >= a.strike {
                    self.enter(FoeState::Recover);
                }
            }
            FoeState::Recover => {
                let i = self.last;
                if self.t >= self.profile.attacks[i].recover {
                    self.cooldown = self.profile.cooldown;
                    self.enter(FoeState::Chase);
                }
            }
            FoeState::Flinch => {
                if self.t >= 0.4 {
                    self.enter(FoeState::Chase);
                }
            }
            FoeState::Stagger { parried } => {
                let total = if parried {
                    self.profile.parried_secs
                } else {
                    self.profile.stagger_secs
                };
                if self.t >= total {
                    self.poise = self.profile.max_poise;
                    self.cooldown = 0.6;
                    self.events.push(Event::FoeRecovered { foe: self.id });
                    self.enter(FoeState::Chase);
                }
            }
            FoeState::Dead => {}
        }
        delta
    }

    /// The swing that is live this tick and has not connected yet.
    pub fn live_swing(&self) -> Option<Swing> {
        match self.state {
            FoeState::Strike(i) if !self.struck && !self.observer => Some(Swing {
                attack: self.profile.attacks[i as usize],
                index: i,
                from: self.pos,
                yaw: self.yaw,
            }),
            _ => None,
        }
    }

    /// Records what happened to the live swing. Dodged swings stay live for their window.
    pub fn swing_result(&mut self, result: HitResult) {
        match result {
            HitResult::Dodged | HitResult::Ignored => {}
            HitResult::Parried => {
                self.struck = true;
                self.poise = self.profile.max_poise;
                self.enter(FoeState::Stagger { parried: true });
                self.events.push(Event::FoeStaggered {
                    foe: self.id,
                    by_parry: true,
                });
            }
            _ => self.struck = true,
        }
    }

    /// A hero swing connects. Damage comes from the sink; the foe adds poise and reactions.
    pub fn receive(
        &mut self,
        hit: &ActiveHit,
        hero_id: u32,
        sink: &mut dyn HitSink,
        t: &Tuning,
    ) -> (HitReport, bool) {
        let report = sink.resolve(
            &Strike {
                attacker: hero_id,
                victim: self.id,
                kind: hit.kind,
                charged: hit.charged,
            },
            true,
        );
        if report.ignored {
            return (report, false);
        }
        self.hp = report.hp;
        self.max_hp = report.max_hp.max(self.max_hp);
        self.hit_stop = hit.stop * t.hit_stop_scale;
        self.poise_wait = 8.0;
        if report.killed || self.hp <= 0.0 {
            self.hp = 0.0;
            self.enter(FoeState::Dead);
            self.events.push(Event::FoeDied { foe: self.id });
            return (report, false);
        }
        let already_open = matches!(self.state, FoeState::Stagger { .. });
        self.poise -= hit.poise;
        let mut broken = false;
        if hit.kind == StrikeKind::Riposte {
            // A riposte ends the opening with a long recovery.
            self.cooldown = 1.2;
            self.enter(FoeState::Recover);
            self.t = 0.0;
        } else if already_open {
            // Hits on an open enemy keep it down but do not extend the opening.
        } else if self.poise <= 0.0 {
            broken = true;
            self.struck = true;
            self.enter(FoeState::Stagger { parried: false });
            self.events.push(Event::FoeStaggered {
                foe: self.id,
                by_parry: false,
            });
        } else if matches!(
            self.state,
            FoeState::Idle | FoeState::Chase | FoeState::Recover
        ) && hit.poise >= 25.0
        {
            self.enter(FoeState::Flinch);
        }
        (report, broken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegraph_lengthens_windups_and_cuts_tracking_early() {
        let c = FoeProfile::classic();
        let t = FoeProfile::telegraphed();
        for (a, b) in c.attacks.iter().zip(&t.attacks) {
            assert!(b.windup > a.windup * 3.0, "{} {}", a.windup, b.windup);
            assert!(b.track_until < b.windup);
            assert!(b.recover > a.recover);
        }
    }

    #[test]
    fn commanded_attack_goes_windup_strike_recover() {
        let mut f = Foe::new(1, Vec3::ZERO, 100.0, FoeProfile::telegraphed());
        f.autonomous = false;
        assert!(f.command_attack(0));
        let hero = Vec3::new(0.0, 0.0, -150.0);
        let mut saw_strike = false;
        for _ in 0..600 {
            f.step(hero, true);
            saw_strike |= matches!(f.state, FoeState::Strike(_));
        }
        assert!(saw_strike);
        assert_eq!(f.state, FoeState::Chase);
    }
}
