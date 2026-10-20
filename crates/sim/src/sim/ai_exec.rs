//! Runs the ported combat and reaction states (`gothic-script`, B08-B10) on the simulation and
//! executes the AI commands they queue.
//!
//! The script side is a state machine that asks the host to do one thing at a time (swing,
//! parade, walk, wait). [`Sim::run_script_states`] steps the machine whenever the actor is idle
//! and turns each queued command into the simulation's motion: a swing opens its hit window
//! after the move's delay, a movement instruction runs until its local callback ends it, task
//! callbacks run every tick.

use super::*;
use crate::brain::GotoState;
use crate::script_host::SimHost;
use gothic_script::combat::{is_combat_task, run_callback};
use gothic_script::enums as se;
use gothic_script::host::{AiCommand, EntityId, GotoKind, NpcApi};
use gothic_script::routine::state::{Callback, State};
use gothic_script::routine::work::{Flow, run_state};

/// Speed of the sidestep and backing-off movements in game units per second [I].
const SIDESTEP_SPEED: f32 = 300.0;
/// Contact distance at which an approach to an actor stops by itself.
const CONTACT_DISTANCE: f32 = 110.0;
/// Distance at which a walk to a point or object counts as arrived.
const ARRIVED_DISTANCE: f32 = 80.0;

fn entity(id: ActorId) -> EntityId {
    EntityId(id.0 + 1)
}

fn actor_id(e: EntityId) -> ActorId {
    ActorId(e.0.saturating_sub(1))
}

/// The simulation task that mirrors a script task, `None` for routine tasks (those are the
/// routine machinery's business).
fn task_for(state: Option<State>) -> Option<Task> {
    match state {
        Some(State::Attack) | None => Some(Task::Attack),
        Some(State::Flee) => Some(Task::Flee),
        Some(State::ReactToWeapon) => Some(Task::ReactToWeapon),
        Some(State::Threaten) => Some(Task::Threaten),
        Some(s) if is_combat_task(s) => Some(Task::Reaction),
        _ => None,
    }
}

impl<W: WorldQuery> Sim<W> {
    /// Put the script state machine of `id` into `state` (the entry of a task).
    pub(super) fn script_enter(&mut self, id: ActorId, state: State) {
        let combat_state = u32::from(self.actors[id.0 as usize].combat_state_active);
        let s = self.script.npc_mut(id);
        s.task = Some(state);
        s.state = Some(state);
        s.progress = 0;
        s.task_position = 0;
        s.state_time = 0.0;
        s.task_time = 0.0;
        s.task_cb = Callback::None;
        s.local_cb = Callback::None;
        if state == State::Attack {
            s.combat_state = combat_state;
        }
        self.actors[id.0 as usize].brain.goto = None;
    }

    /// Put the script state machine of `id` into `ZS_Attack` (called when a fight starts).
    pub(super) fn script_begin_attack(&mut self, id: ActorId) {
        self.script_enter(id, State::Attack);
        self.actors[id.0 as usize].state_position = 0;
    }

    /// One tick of an actor whose task runs on the script (everything but the attack's
    /// strike handling): clocks, task callback, running movement, idle decision.
    pub(super) fn script_task_think(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let dt = self.dt();
        {
            let s = self.script.npc_mut(id);
            s.state_time += dt;
            s.task_time += dt;
        }
        // Task callbacks run every tick.
        let cb = self.script.npc(id).task_cb;
        if cb != Callback::None {
            let npc = entity(id);
            let mut host = SimHost::new(self);
            host.self_entity = npc;
            run_callback(&mut host, npc, cb);
            self.reflect_script_task(i, now);
            if !matches!(
                self.actors[i].brain.task,
                Task::Attack | Task::Flee | Task::ReactToWeapon | Task::Threaten | Task::Reaction
            ) {
                return;
            }
        }
        if self.actors[i].brain.goto.is_some() {
            self.goto_tick(i, now);
            if self.actors[i].brain.goto.is_some() {
                return;
            }
        }
        if now < self.actors[i].brain.busy_until_ms {
            if let Some(t) = self.actors[i].target {
                self.face(i, t);
            }
            return;
        }
        self.run_script_states(i, now);
    }

    /// Step the actor's script state machine until it queues something, then execute that.
    pub(super) fn run_script_states(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let npc = entity(id);
        let mut host = SimHost::new(self);
        host.self_entity = npc;
        host.other_entity = host.current_target(npc);
        for _ in 0..24 {
            let before = (host.current_task(npc), host.current_state(npc));
            if run_state(&mut host, npc) == Flow::Yield {
                break;
            }
            if before == (host.current_task(npc), host.current_state(npc)) {
                break;
            }
        }
        let cmds = host.sim.script.take_commands_for(id);
        for cmd in cmds {
            self.exec_command(i, cmd, now);
        }
        self.reflect_script_task(i, now);
    }

    /// One decision step of the ported `ZS_Attack` family for the idle actor `i`.
    pub(super) fn run_combat_script(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        if self.script.npc(id).task != Some(State::Attack)
            && task_for(self.script.npc(id).task).is_none()
        {
            self.script_begin_attack(id);
        }
        self.run_script_states(i, now);
    }

    /// Mirror the script task into the simulation task: tasks the script ended return the actor
    /// to its routine.
    pub(super) fn reflect_script_task(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let task = self.script.npc(id).task;
        match task_for(task) {
            Some(t) => {
                if self.actors[i].brain.task != t {
                    self.actors[i].brain.task = t;
                    self.actors[i].brain.since_ms = now;
                }
            }
            None => {
                if !matches!(self.actors[i].ai_mode, AiMode::Dead | AiMode::Down) {
                    self.back_to_routine(i, now);
                }
            }
        }
    }

    /// Execute one command the script queued for actor `i`.
    pub(super) fn exec_command(&mut self, i: usize, cmd: AiCommand, now: u64) {
        let id = self.actors[i].id;
        if matches!(
            cmd,
            AiCommand::Attack { .. }
                | AiCommand::Parade { .. }
                | AiCommand::JumpBack { .. }
                | AiCommand::CombatGoto { .. }
                | AiCommand::WaitMs { .. }
        ) {
            // Every instruction starts its own stage counting.
            self.actors[i].state_position = 0;
        }
        match cmd {
            AiCommand::Attack { action, target } => {
                let Some(m) = from_script_action(action) else {
                    return;
                };
                self.begin_swing(i, actor_id(target), m, now);
            }
            AiCommand::Parade { target } => {
                self.face(i, actor_id(target));
                let a = &mut self.actors[i];
                a.action = Action::Parade;
                a.brain.parade_until_ms = now + 600;
                a.brain.busy_until_ms = now + 600;
                a.brain.last_move = Action::Parade;
            }
            AiCommand::JumpBack { target } => {
                let tid = actor_id(target);
                let tp = self.pos(tid);
                let me = self.pos(id);
                let dir = me.yaw_to(tp);
                let to = Pos::new(me.x - dir.cos() * 250.0, me.y, me.z - dir.sin() * 250.0);
                self.world.move_towards(id, to, 250.0);
                self.face(i, tid);
                let cost = stamina_cost(&self.cfg, Action::JumpBack);
                let a = &mut self.actors[i];
                a.action = Action::JumpBack;
                a.brain.last_move = Action::JumpBack;
                a.brain.busy_until_ms = now + 600;
                a.stamina = (a.stamina - cost).max(0);
            }
            AiCommand::CombatGoto {
                target,
                walk_mode,
                kind,
            } => {
                self.actors[i].brain.goto = Some(GotoState {
                    target: Some(actor_id(target)),
                    pos: None,
                    kind,
                    walk_mode,
                    stop: CONTACT_DISTANCE,
                    since_ms: now,
                });
            }
            AiCommand::GotoExt { target } => {
                let goto = if target.0 != 0 && target.0 >> 24 == 0 {
                    GotoState {
                        target: Some(actor_id(target)),
                        pos: None,
                        kind: GotoKind::Approach,
                        walk_mode: 2,
                        stop: 250.0,
                        since_ms: now,
                    }
                } else {
                    GotoState {
                        target: None,
                        pos: SimHost::<W>::entity_position_of(&self.script, target),
                        kind: GotoKind::Approach,
                        walk_mode: 2,
                        stop: ARRIVED_DISTANCE,
                        since_ms: now,
                    }
                };
                self.actors[i].brain.goto = Some(goto);
            }
            AiCommand::GotoPos { pos } => {
                self.actors[i].brain.goto = Some(GotoState {
                    target: None,
                    pos: Some(SimHost::<W>::to_world(pos)),
                    kind: GotoKind::Approach,
                    walk_mode: 2,
                    stop: ARRIVED_DISTANCE,
                    since_ms: now,
                });
            }
            AiCommand::Shoot { target } => {
                let tid = actor_id(target);
                self.face(i, tid);
                let ammo = if self.actors[i].equipment.left_use() == crate::item::Use::CrossBow {
                    "It_Bolt"
                } else {
                    "It_Arrow"
                };
                if self.shoot(id, tid, 2.0, ammo).is_none() {
                    self.actors[i].brain.busy_until_ms = now + 1500;
                }
            }
            AiCommand::WaitMs { ms } => {
                let a = &mut self.actors[i];
                a.brain.busy_until_ms = now + u64::from(ms);
                if a.action == Action::Parade {
                    a.brain.parade_until_ms = a.brain.busy_until_ms;
                }
            }
            AiCommand::Wait { secs } | AiCommand::Action { secs, .. } => {
                self.actors[i].brain.busy_until_ms = now + (secs.max(0) as u64) * 1000;
            }
            AiCommand::UseItem(name) if name.eq_ignore_ascii_case("It_Potion_Health") => {
                self.drink_potion(i, now);
            }
            AiCommand::Cast { .. } => {
                // NPC spell casting needs per-spell data; hosts drive it through `cast`. [stub]
                self.actors[i].brain.busy_until_ms = now + 1500;
            }
            AiCommand::HoldInventoryStacks {
                right: -1,
                left: -1,
            } => {
                // Putting the weapons away.
                if self.actors[i].brain.task != Task::Attack {
                    self.actors[i].drawn = false;
                }
            }
            AiCommand::SayLine { key } => {
                self.push(EventKind::Voice {
                    actor: id,
                    key: key.to_string(),
                });
            }
            AiCommand::SaySvm { family, .. } => {
                let stage = match family {
                    "WeaponDown" | "StopMagic" | "LookingForTroubleAgain" => Some(0),
                    "ISaidWeaponDown" | "ISaidStopMagic" => Some(1),
                    _ => None,
                };
                if let Some(stage) = stage {
                    self.actors[i].brain.warned = stage;
                    self.push(EventKind::Warned { actor: id, stage });
                }
            }
            _ => {}
        }
    }

    /// Start a melee swing: pay the stamina, open the hit window later, stay busy for the
    /// recovery. Stamina-starved fighters catch their breath instead.
    fn begin_swing(&mut self, i: usize, tid: ActorId, m: Action, now: u64) {
        let cost = stamina_cost(&self.cfg, m);
        self.face(i, tid);
        if m == Action::FinishingAttack {
            let id = self.actors[i].id;
            self.actors[i].action = m;
            self.actors[i].brain.last_move = m;
            self.actors[i].brain.busy_until_ms = now + 1200;
            self.kill(tid, Some(id));
            return;
        }
        let a = &mut self.actors[i];
        if a.stamina <= 0 && cost > 0 {
            a.brain.busy_until_ms = now + 500;
            return;
        }
        a.stamina = (a.stamina - cost).max(0);
        a.action = m;
        a.brain.last_move = m;
        a.brain.last_strike_ms = Some(now);
        a.brain.strike = Some(Strike {
            at_ms: now + (vitals::hit_window_secs(m) * 1000.0) as u64,
            action: m,
            target: tid,
        });
        a.brain.busy_until_ms = now + move_duration_ms(m);
    }

    /// One tick of a running movement instruction: the callback may end it by switching the
    /// state; otherwise the actor moves.
    pub(super) fn goto_tick(&mut self, i: usize, now: u64) {
        let Some(g) = self.actors[i].brain.goto else {
            return;
        };
        let id = self.actors[i].id;
        let npc = entity(id);
        let cb = self.script.npc(id).local_cb;
        let ended = {
            let mut host = SimHost::new(self);
            host.self_entity = npc;
            run_callback(&mut host, npc, cb)
        };
        if ended {
            self.actors[i].brain.goto = None;
            return;
        }
        let dt = self.dt();
        let me = self.pos(id);
        let dest = match (g.target, g.pos) {
            (Some(t), _) => self.pos(t),
            (None, Some(p)) => p,
            (None, None) => {
                self.actors[i].brain.goto = None;
                return;
            }
        };
        let dist = ((dest.x - me.x).powi(2) + (dest.z - me.z).powi(2)).sqrt();
        match g.kind {
            GotoKind::Approach => {
                let speed = if g.walk_mode == 1 {
                    WALK_SPEED
                } else {
                    RUN_SPEED
                };
                if dist <= g.stop {
                    self.actors[i].brain.goto = None;
                } else {
                    let step = (speed * dt).min(dist - g.stop);
                    self.world.move_towards(id, dest, step);
                }
                self.world.face_towards(id, dest);
            }
            GotoKind::Back | GotoKind::StrafeLeft | GotoKind::StrafeRight => {
                let dir = me.yaw_to(dest);
                let (dx, dz) = match g.kind {
                    GotoKind::Back => (-dir.cos(), -dir.sin()),
                    GotoKind::StrafeLeft => (dir.sin(), -dir.cos()),
                    _ => (-dir.sin(), dir.cos()),
                };
                let len = SIDESTEP_SPEED * dt;
                let to = Pos::new(me.x + dx * len, me.y, me.z + dz * len);
                self.world.move_towards(id, to, len);
                self.world.face_towards(id, dest);
            }
            GotoKind::TurnLeft | GotoKind::TurnRight => {
                self.world.face_towards(id, dest);
            }
        }
        let _ = now;
    }
}

/// Map the script's action to the simulation's.
fn from_script_action(a: se::Action) -> Option<Action> {
    Some(match a {
        se::Action::Attack => Action::Attack,
        se::Action::PowerAttack => Action::PowerAttack,
        se::Action::QuickAttack => Action::QuickAttack,
        se::Action::QuickAttackR => Action::QuickAttackR,
        se::Action::QuickAttackL => Action::QuickAttackL,
        se::Action::SimpleWhirl => Action::SimpleWhirl,
        se::Action::SprintAttack => Action::SprintAttack,
        se::Action::WhirlAttack => Action::WhirlAttack,
        se::Action::PierceAttack => Action::PierceAttack,
        se::Action::HackAttack => Action::HackAttack,
        se::Action::FinishingAttack => Action::FinishingAttack,
        _ => return None,
    })
}
