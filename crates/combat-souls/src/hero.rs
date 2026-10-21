//! The player's state machine. Pure data in, pure data out; stepped once per 60 Hz tick.
//!
//! Copyright (c) 2026 Funny-Bones and contributors (MIT) for the adapted structure; the license
//! text and commit hash are in `THIRD_PARTY_NOTICES.md`.
//!
//! Structure adapted from the MIT-licensed `ELDEN-RING-Playermodel-Rust` reference
//! (commit 177a3b5): frame-based actions with per-input cancel windows, a one-slot
//! input buffer, dodge-on-release / hold-to-sprint, guard counter window, hit-stop. Timings and
//! rules here are our own and use no data from another game.
use crate::actions::{ActionId, Dir, Hurt, StrikeKind};
use crate::events::{Event, HitResult};
use crate::math::{angle_diff, approach, dir_of, flat, right_of, turn_toward, yaw_of};
use crate::sink::{HitReport, HitSink, Strike};
use crate::tuning::{DF, DT, Tuning};
use glam::{Vec2, Vec3};
use std::f32::consts::FRAC_PI_4;

pub const STICK_DEADZONE: f32 = 0.1;
/// Stick magnitude below which the hero walks.
pub const WALK_TILT: f32 = 0.55;
const RESPAWN_FRAMES: f32 = 120.0;

#[derive(Clone, Copy, Default, Debug)]
pub struct Button {
    pub held: bool,
    /// Went down since the last tick.
    pub pressed: bool,
    /// Went up since the last tick.
    pub released: bool,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// Movement stick: x right, y forward, relative to the camera.
    pub mv: Vec2,
    pub cam_yaw: f32,
    pub dodge: Button,
    pub light: Button,
    pub heavy: Button,
    pub guard: Button,
    pub walk: bool,
    /// Lock-on pressed this tick.
    pub lock: bool,
    /// Target switch: -1 left, +1 right, 0 none (one edge per flick or wheel notch).
    pub switch: i8,
}

impl Input {
    pub fn tilt(&self) -> f32 {
        self.mv.length().min(1.0)
    }

    /// World-space direction the stick asks for, if it is out of the deadzone.
    pub fn wish(&self) -> Option<Vec3> {
        if self.mv.length() < STICK_DEADZONE {
            return None;
        }
        let forward = dir_of(self.cam_yaw);
        let right = right_of(self.cam_yaw);
        Some((forward * self.mv.y + right * self.mv.x).normalize_or_zero())
            .filter(|v| *v != Vec3::ZERO)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gait {
    Walk,
    Run,
    Sprint,
}

/// What the hero wants the locomotion backend to do this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Free movement: wish direction (zero for none) and gait; `face` overrides the heading
    /// (lock-on strafing, guard).
    pub wish: Vec3,
    pub gait: Gait,
    pub face: Option<Vec3>,
    /// Scripted displacement this tick (rolls, lunges). When set, `wish` is ignored.
    pub root: Option<Vec3>,
    /// Heading the body should turn to while rooted.
    pub root_yaw: Option<f32>,
    /// Speed scale (guard up slows the walk).
    pub speed_scale: f32,
}

impl Motion {
    pub fn idle() -> Self {
        Self {
            wish: Vec3::ZERO,
            gait: Gait::Run,
            face: None,
            root: None,
            root_yaw: None,
            speed_scale: 1.0,
        }
    }
}

/// An input waiting for the current animation to allow it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    Light,
    Heavy,
    Dodge,
}

#[derive(Clone, Copy, Debug)]
pub struct Act {
    pub id: ActionId,
    /// Animation frame (30 fps).
    pub f: f32,
    pub landed: bool,
    /// World direction of the root motion.
    pub dir: Vec3,
    pub travelled: f32,
    /// Frames the heavy button was held at the charge point.
    pub charge: f32,
    pub charge_done: bool,
    pub charged: bool,
    /// Riposte target and where to stand.
    pub victim: Option<u32>,
    pub snap: Option<Vec3>,
}

#[derive(Clone, Copy, Debug)]
pub enum State {
    Free,
    Act(Act),
    Dead { t: f32 },
}

/// A swing arriving at the hero.
#[derive(Clone, Copy, Debug)]
pub struct Incoming {
    pub attacker: u32,
    pub from: Vec3,
    pub level: Hurt,
    pub poise: f32,
    pub parryable: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Receipt {
    pub result: HitResult,
    pub damage: i32,
}

/// Context the world gives the hero each tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeroCtx {
    /// Locked-on target position.
    pub target: Option<Vec3>,
    /// A staggered enemy within riposte reach: id and position.
    pub riposte: Option<(u32, Vec3)>,
}

/// The swing that is live this tick.
#[derive(Clone, Copy, Debug)]
pub struct ActiveHit {
    pub kind: StrikeKind,
    pub charged: bool,
    pub poise: f32,
    pub stop: f32,
    pub arc_deg: f32,
    pub victim: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Hero {
    pub id: u32,
    pub pos: Vec3,
    pub yaw: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub stamina: f32,
    pub poise: f32,
    pub state: State,
    pub speed: f32,
    pub sprinting: bool,
    pub guarding: bool,
    pub buffer: Option<(Req, f32)>,
    /// Seconds frozen after landing or taking a hit.
    pub hit_stop: f32,
    pub spawn: Vec3,
    pub events: Vec<Event>,
    guard_t: f32,
    guard_counter: f32,
    dodge_hold: f32,
    dodge_armed: bool,
    sprint_spent: bool,
    stamina_wait: f32,
    poise_wait: f32,
    exhausted_reported: bool,
}

impl Hero {
    pub fn new(id: u32, spawn: Vec3, hp: f32, t: &Tuning) -> Self {
        Self {
            id,
            pos: spawn,
            yaw: 0.0,
            hp,
            max_hp: hp,
            stamina: t.max_stamina,
            poise: t.max_poise,
            state: State::Free,
            speed: 0.0,
            sprinting: false,
            guarding: false,
            buffer: None,
            hit_stop: 0.0,
            spawn,
            events: Vec::new(),
            guard_t: 0.0,
            guard_counter: 0.0,
            dodge_hold: 0.0,
            dodge_armed: false,
            sprint_spent: false,
            stamina_wait: 0.0,
            poise_wait: 0.0,
            exhausted_reported: false,
        }
    }

    pub fn facing(&self) -> Vec3 {
        dir_of(self.yaw)
    }

    pub fn is_dead(&self) -> bool {
        matches!(self.state, State::Dead { .. })
    }

    pub fn action(&self) -> Option<ActionId> {
        match self.state {
            State::Act(a) => Some(a.id),
            _ => None,
        }
    }

    /// Frame of the running action.
    pub fn action_frame(&self) -> f32 {
        match self.state {
            State::Act(a) => a.f,
            _ => 0.0,
        }
    }

    /// Inside an i-frame window.
    pub fn invincible(&self) -> bool {
        match self.state {
            State::Act(a) => {
                let (from, to) = a.id.def().iframes;
                a.f >= from && a.f < to
            }
            State::Dead { .. } => true,
            State::Free => false,
        }
    }

    pub fn guard_up(&self, t: &Tuning) -> bool {
        self.guarding && self.guard_t >= t.guard_raise_frames
    }

    /// Frames since the guard went up; the parry window is measured from here.
    pub fn parry_active(&self, t: &Tuning) -> bool {
        self.guarding && self.guard_t >= t.parry_window.0 && self.guard_t < t.parry_window.1
    }

    pub fn guard_counter_ready(&self) -> bool {
        self.guard_counter > 0.0
    }

    /// The hit volume that is live this tick, if the swing has not connected yet.
    pub fn active_hit(&self) -> Option<ActiveHit> {
        let State::Act(a) = self.state else {
            return None;
        };
        let hit = a.id.def().hit?;
        if a.landed || a.f < hit.from || a.f >= hit.to {
            return None;
        }
        let kind = if hit.kind == StrikeKind::Heavy && a.charged {
            StrikeKind::Charged
        } else {
            hit.kind
        };
        Some(ActiveHit {
            kind,
            charged: a.charged,
            poise: if a.charged {
                hit.poise * 1.5
            } else {
                hit.poise
            },
            stop: hit.stop,
            arc_deg: hit.arc_deg,
            victim: a.victim,
        })
    }

    pub fn mark_hit(&mut self) {
        if let State::Act(a) = &mut self.state {
            a.landed = true;
        }
    }

    /// One tick. Returns the motion the locomotion backend must carry out.
    pub fn step(&mut self, inp: &Input, ctx: &HeroCtx, t: &Tuning) -> Motion {
        if let State::Dead { t: dead } = &mut self.state {
            *dead += DF;
            if *dead >= RESPAWN_FRAMES {
                let (id, spawn, hp) = (self.id, self.spawn, self.max_hp);
                *self = Hero::new(id, spawn, hp, t);
            }
            return Motion::idle();
        }
        self.read_buttons(inp, t);
        if self.hit_stop > 0.0 {
            self.hit_stop -= DT;
            return Motion::idle();
        }
        self.guard_counter = (self.guard_counter - DF).max(0.0);
        if let Some((_, age)) = &mut self.buffer {
            *age += DT;
        }
        if self.buffer.is_some_and(|(_, age)| age > t.buffer_secs) {
            self.buffer = None;
        }
        self.stamina_wait = (self.stamina_wait - DT).max(0.0);
        self.poise_wait = (self.poise_wait - DT).max(0.0);
        if self.poise_wait <= 0.0 {
            self.poise = t.max_poise;
        }

        let (motion, draining) = match self.state {
            State::Free => self.free(inp, ctx, t),
            State::Act(a) => (self.act(a, inp, ctx, t), false),
            State::Dead { .. } => (Motion::idle(), false),
        };

        if self.stamina <= 0.0 && !self.exhausted_reported {
            self.exhausted_reported = true;
            self.events.push(Event::Exhausted);
        }
        if self.stamina > 0.0 {
            self.exhausted_reported = false;
        }
        if !draining && self.stamina_wait <= 0.0 {
            let mult = if self.guarding {
                t.guard_regen_mult
            } else {
                1.0
            };
            self.stamina = (self.stamina + t.stamina_regen * mult * DT).min(t.max_stamina);
        }
        motion
    }

    fn spend(&mut self, amount: f32, t: &Tuning) {
        self.stamina = (self.stamina - amount).max(0.0);
        self.stamina_wait = t.stamina_delay;
    }

    fn read_buttons(&mut self, inp: &Input, t: &Tuning) {
        // Each animation says from which frame it listens for the next input.
        let (listening, listening_dodge) = match self.state {
            State::Act(a) => {
                let def = a.id.def();
                (
                    a.f >= def.input_from,
                    a.f >= def.input_from.min(def.cancel_dodge - 4.0),
                )
            }
            _ => (true, true),
        };
        if inp.dodge.pressed {
            self.dodge_hold = 0.0;
            self.dodge_armed = true;
        }
        if inp.dodge.held {
            self.dodge_hold += DF;
        }
        if inp.dodge.released {
            // Rolls come out on release, and only for a short press; longer is a sprint.
            if self.dodge_armed && self.dodge_hold < t.sprint_hold_frames && listening_dodge {
                self.buffer = Some((Req::Dodge, 0.0));
            }
            self.dodge_armed = false;
            self.dodge_hold = 0.0;
            self.sprint_spent = false;
        }
        if listening {
            if inp.heavy.pressed {
                self.buffer = Some((Req::Heavy, 0.0));
            }
            if inp.light.pressed {
                self.buffer = Some((Req::Light, 0.0));
            }
        }
    }

    // ------------------------------------------------------------------------------------------

    fn free(&mut self, inp: &Input, ctx: &HeroCtx, t: &Tuning) -> (Motion, bool) {
        let wish = inp.wish();
        if let Some((req, _)) = self.buffer {
            let started = self.start_request(req, inp, ctx, t, None);
            self.buffer = None;
            if started {
                return (Motion::idle(), false);
            }
        }

        self.sprinting = inp.dodge.held
            && self.dodge_hold >= t.sprint_hold_frames
            && wish.is_some()
            && !self.sprint_spent
            && self.stamina > 0.0;
        let mut draining = false;
        if self.sprinting {
            self.stamina -= t.sprint_drain * DT;
            self.stamina_wait = t.stamina_delay;
            draining = true;
            if self.stamina <= 0.0 {
                self.stamina = 0.0;
                self.sprinting = false;
                self.sprint_spent = true;
            }
        }

        if inp.guard.pressed {
            self.guard_t = 0.0;
        }
        if inp.guard.held && !self.sprinting {
            if !self.guarding {
                // A fresh press restarts the window; holding through an action does not.
                if !inp.guard.pressed {
                    self.guard_t = t.guard_raise_frames;
                }
                self.guarding = true;
            }
            self.guard_t += DF;
        } else {
            self.guarding = false;
            self.guard_t = 0.0;
        }

        let face = match ctx.target {
            Some(p) if !self.sprinting => {
                let d = flat(p - self.pos);
                (d.length_squared() > 1.0).then(|| d.normalize())
            }
            _ => None,
        };
        let gait = if self.sprinting {
            Gait::Sprint
        } else if inp.walk || inp.tilt() < WALK_TILT {
            Gait::Walk
        } else {
            Gait::Run
        };
        let motion = Motion {
            wish: wish.unwrap_or(Vec3::ZERO),
            gait,
            face,
            root: None,
            root_yaw: None,
            speed_scale: if self.guard_up(t) { 0.6 } else { 1.0 },
        };
        (motion, draining)
    }

    /// Starts the action a buffered request asks for, if stamina and the situation allow it.
    fn start_request(
        &mut self,
        req: Req,
        inp: &Input,
        ctx: &HeroCtx,
        t: &Tuning,
        from: Option<ActionId>,
    ) -> bool {
        if self.stamina <= 0.0 {
            return false;
        }
        let wish = inp.wish();
        let (id, dir, victim, snap) = match req {
            Req::Dodge => match (ctx.target, wish) {
                (Some(_), Some(w)) => {
                    let off = angle_diff(self.yaw, yaw_of(w));
                    let side = if off.abs() <= FRAC_PI_4 {
                        Dir::Front
                    } else if off.abs() >= 3.0 * FRAC_PI_4 {
                        Dir::Back
                    } else if off > 0.0 {
                        Dir::Left
                    } else {
                        Dir::Right
                    };
                    (ActionId::Roll(side), w, None, None)
                }
                (None, Some(w)) => {
                    self.yaw = yaw_of(w);
                    (ActionId::Roll(Dir::Front), w, None, None)
                }
                (_, None) => (ActionId::Backstep, -self.facing(), None, None),
            },
            Req::Light | Req::Heavy => {
                if let Some((id, pos)) = ctx.riposte {
                    let to = flat(pos - self.pos);
                    if to.length_squared() > 1.0 {
                        self.yaw = yaw_of(to);
                    }
                    let stand = pos - self.facing() * 130.0;
                    (ActionId::Riposte, self.facing(), Some(id), Some(stand))
                } else {
                    let id = match (req, from) {
                        (Req::Light, Some(ActionId::Light(n))) => ActionId::Light(n % 3 + 1),
                        (Req::Light, Some(a)) if a.is_roll() => ActionId::RollLight,
                        (Req::Light, _) if self.sprinting => ActionId::RunLight,
                        (Req::Light, _) => ActionId::Light(1),
                        (Req::Heavy, _) if self.guard_counter_ready() => ActionId::GuardCounter,
                        _ => ActionId::Heavy,
                    };
                    (id, self.facing(), None, None)
                }
            }
        };
        self.spend(id.cost(t), t);
        self.sprinting = false;
        self.guarding = false;
        self.guard_t = 0.0;
        self.dodge_armed = false;
        self.guard_counter = 0.0;
        self.state = State::Act(Act {
            id,
            f: 0.0,
            landed: false,
            dir: flat(dir),
            travelled: 0.0,
            charge: 0.0,
            charge_done: false,
            charged: false,
            victim,
            snap,
        });
        self.events.push(Event::HeroAction(id));
        true
    }

    fn act(&mut self, mut a: Act, inp: &Input, ctx: &HeroCtx, t: &Tuning) -> Motion {
        let def = a.id.def();
        let mut motion = Motion::idle();

        // Heavy attacks wait at the charge point while the button stays down.
        let mut advance = true;
        if let Some(hold) = def.hold
            && a.f >= hold
            && !a.charge_done
        {
            if inp.heavy.held && a.charge < t.charge_max_frames {
                a.charge += DF;
                advance = false;
            } else {
                a.charge_done = true;
                a.charged = a.charge >= t.charge_frames;
            }
        }
        if advance {
            a.f += DF;
        }

        // Turning before the swing: toward the lock-on target or the stick.
        if a.f < def.turn_until && advance {
            let goal = match (ctx.target, inp.wish()) {
                (Some(p), _) => Some(yaw_of(flat(p - self.pos))),
                (None, Some(w)) => Some(yaw_of(w)),
                _ => None,
            };
            if let Some(goal) = goal {
                self.yaw = turn_toward(self.yaw, goal, t.turn_action_deg.to_radians() * DT);
                if a.id.is_attack() {
                    a.dir = flat(self.facing());
                }
            }
        }

        // Root motion.
        if let Some(snap) = a.snap.take() {
            let d = flat(snap - self.pos);
            motion.root = Some(if d.length() < 400.0 { d } else { Vec3::ZERO });
        } else if def.travel.is_some() {
            let now = def.travelled(a.f);
            let step = now - a.travelled;
            a.travelled = now;
            motion.root = Some(a.dir * step);
        } else {
            motion.root = Some(Vec3::ZERO);
        }

        // Leaving the action.
        if a.f >= def.total {
            self.state = State::Free;
            return Motion::idle();
        }
        let wish = inp.wish();
        // Buffered inputs interrupt once their cancel frame is reached.
        if let Some((req, _)) = self.buffer {
            let at = match req {
                Req::Light => def.cancel_light,
                Req::Heavy => def.cancel_heavy,
                Req::Dodge => def.cancel_dodge,
            };
            if a.f >= at {
                self.state = State::Act(a);
                let from = Some(a.id);
                // Chains only continue from attacks; anything else restarts at the first hit.
                let from = if matches!(a.id, ActionId::Light(_)) || a.id.is_roll() {
                    from
                } else {
                    None
                };
                let before = self.state;
                if self.start_request(req, inp, ctx, t, from) {
                    self.buffer = None;
                    return Motion::idle();
                }
                self.state = before;
                self.buffer = None;
            }
        }
        if inp.guard.held && a.f >= def.cancel_guard {
            self.state = State::Free;
            return Motion::idle();
        }
        if wish.is_some() && a.f >= def.cancel_move {
            self.state = State::Free;
            return Motion::idle();
        }
        self.state = State::Act(a);
        if let Some(r) = motion.root
            && r.length_squared() > 1e-6
        {
            motion.root_yaw = Some(self.yaw);
        }
        motion
    }

    // ------------------------------------------------------------------------------------------

    /// A foe swing reaches the hero. Decides evade / parry / block / hit and, when it goes
    /// through, asks the pipeline for the damage.
    pub fn receive(&mut self, inc: &Incoming, sink: &mut dyn HitSink, t: &Tuning) -> Receipt {
        let none = Receipt {
            result: HitResult::Ignored,
            damage: 0,
        };
        if self.is_dead() {
            return none;
        }
        if self.invincible() {
            return Receipt {
                result: HitResult::Dodged,
                damage: 0,
            };
        }
        let strike = Strike {
            attacker: inc.attacker,
            victim: self.id,
            kind: StrikeKind::Foe,
            charged: false,
        };
        let toward = flat(inc.from - self.pos);
        let in_arc =
            toward.normalize_or_zero().dot(self.facing()) >= t.guard_arc_deg.to_radians().cos();

        if in_arc && inc.parryable && self.parry_active(t) {
            self.guard_counter = t.guard_counter_window;
            self.enter_reaction(ActionId::GuardHit);
            return Receipt {
                result: HitResult::Parried,
                damage: 0,
            };
        }
        if in_arc && self.guard_up(t) {
            let would = sink.resolve(&strike, false);
            let cost = (would.damage as f32 * t.guard_stamina_ratio).max(6.0);
            self.stamina -= cost;
            self.stamina_wait = t.stamina_delay;
            if self.stamina <= 0.0 {
                self.stamina = 0.0;
                self.enter_reaction(ActionId::GuardBreak);
                return Receipt {
                    result: HitResult::GuardBroken,
                    damage: 0,
                };
            }
            self.guard_counter = t.guard_counter_window;
            self.enter_reaction(ActionId::GuardHit);
            return Receipt {
                result: HitResult::Blocked,
                damage: 0,
            };
        }

        let report: HitReport = sink.resolve(&strike, true);
        if report.ignored {
            return none;
        }
        self.hp = report.hp;
        self.max_hp = report.max_hp.max(self.max_hp);
        if report.killed || self.hp <= 0.0 {
            self.hp = 0.0;
            self.sprinting = false;
            self.guarding = false;
            self.buffer = None;
            self.state = State::Dead { t: 0.0 };
            self.events.push(Event::HeroDied);
            return Receipt {
                result: HitResult::Killed,
                damage: report.damage,
            };
        }

        self.flinch(inc.level, inc.poise, t);
        Receipt {
            result: HitResult::Hit,
            damage: report.damage,
        }
    }

    /// Poise and reaction after damage went through: super armour, poise break, hurt animation.
    fn flinch(&mut self, level: Hurt, poise_damage: f32, t: &Tuning) {
        self.poise -= poise_damage;
        self.poise_wait = t.poise_reset;
        let armoured = matches!(self.state, State::Act(a)
            if a.id.def().armour.is_some_and(|(x, y)| a.f >= x && a.f < y));
        let attacking = matches!(self.state, State::Act(a) if a.id.is_attack());
        if level == Hurt::Knockdown {
            self.enter_reaction(ActionId::Hurt(Hurt::Knockdown));
        } else if self.poise <= 0.0 {
            self.poise = t.max_poise;
            self.enter_reaction(ActionId::Stagger);
        } else if !armoured && !(attacking && level == Hurt::Small) {
            self.enter_reaction(ActionId::Hurt(level));
        }
    }

    /// Damage the host's simulation already applied to the hero (NPC swings resolved by the
    /// ported pipeline): mirrors HP and plays the poise / hurt / death reaction.
    pub fn apply_external_damage(
        &mut self,
        hp_after: f32,
        level: Hurt,
        poise_damage: f32,
        t: &Tuning,
    ) {
        if self.is_dead() {
            return;
        }
        self.hp = hp_after.max(0.0);
        if self.hp <= 0.0 {
            self.sprinting = false;
            self.guarding = false;
            self.buffer = None;
            self.state = State::Dead { t: 0.0 };
            self.events.push(Event::HeroDied);
        } else {
            self.flinch(level, poise_damage, t);
        }
    }

    /// The host absorbed a swing with the original parry rules while our guard was up; the block
    /// costs `stamina_cost` and breaks the guard when the bar runs out.
    pub fn apply_external_block(&mut self, stamina_cost: f32, t: &Tuning) {
        self.stamina = (self.stamina - stamina_cost).max(0.0);
        self.stamina_wait = t.stamina_delay;
        if self.stamina <= 0.0 {
            self.enter_reaction(ActionId::GuardBreak);
        } else {
            self.guard_counter = t.guard_counter_window;
            self.enter_reaction(ActionId::GuardHit);
        }
    }

    fn enter_reaction(&mut self, id: ActionId) {
        self.sprinting = false;
        self.guarding = false;
        self.guard_t = 0.0;
        self.buffer = None;
        self.state = State::Act(Act {
            id,
            f: 0.0,
            landed: false,
            dir: flat(-self.facing()),
            travelled: 0.0,
            charge: 0.0,
            charge_done: true,
            charged: false,
            victim: None,
            snap: None,
        });
        self.events.push(Event::HeroAction(id));
    }
}

/// Applies the guard slowdown / speeds for a simple kinematic backend.
pub fn target_speed(m: &Motion, locked: bool, hero_yaw: f32, t: &Tuning) -> f32 {
    if m.wish == Vec3::ZERO {
        return 0.0;
    }
    let base = match m.gait {
        Gait::Sprint => t.sprint_speed,
        Gait::Walk => t.walk_speed,
        Gait::Run => {
            if locked {
                let along = m.wish.dot(dir_of(hero_yaw));
                if along > FRAC_PI_4.cos() {
                    t.run_speed
                } else if along < -FRAC_PI_4.cos() {
                    t.back_speed
                } else {
                    t.strafe_speed
                }
            } else {
                t.run_speed
            }
        }
    };
    base * m.speed_scale
}

/// Kinematic locomotion used by the headless world and the demo; the game replaces it with the
/// physics controller.
pub fn kinematic_move(hero: &mut Hero, m: &Motion, t: &Tuning) {
    if let Some(root) = m.root {
        hero.pos += root;
        hero.speed = root.length() / DT;
        if let Some(y) = m.root_yaw {
            hero.yaw = y;
        }
        return;
    }
    let locked = m.face.is_some();
    let goal = m.face.or((m.wish != Vec3::ZERO).then_some(m.wish));
    if let Some(g) = goal {
        let rate = if locked {
            t.turn_locked_deg
        } else {
            t.turn_free_deg
        };
        hero.yaw = turn_toward(hero.yaw, yaw_of(g), rate.to_radians() * DT);
    }
    let target = target_speed(m, locked, hero.yaw, t);
    let accel = if target > hero.speed { 1800.0 } else { 2400.0 };
    hero.speed = approach(hero.speed, target, accel * DT);
    if m.wish != Vec3::ZERO {
        hero.pos += m.wish * hero.speed * DT;
    }
}
