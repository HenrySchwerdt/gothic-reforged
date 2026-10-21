//! Executes the `_AI_*` melee instructions of the NPCs the way the engine's script processing
//! unit does: the ported command (`begin_melee_command`, `begin_parade_command`,
//! `begin_jump_back_command`, S:381a0 and siblings) writes `Action` / `AniState` / hand and
//! returns its instruction list; each `sAICombatMoveInstr` (G:0x101696e0) plays one clip of the
//! move, the script's `OnHit` runs when the `Hit` clip reached 60 percent of its length
//! (`sAICombatMoveItlLoop`, G:0x1016dd00) and the instruction ends with the clip.
//!
//! The clips are the presentation's business: for an actor whose body the host drives
//! ([`Sim::set_host_drives_npc`]) every move instruction is an [`EventKind::NpcMove`]; the host
//! answers with [`Sim::npc_move_hit`] and [`Sim::npc_exec_done`]. Actors without a body
//! (headless runs, characters outside the show range) run the same instructions on timers
//! made from the move table of `move_duration_ms` and the hit windows of
//! `gothic_script::combat::commands::hit_window_secs` [I: no clip lengths without a body].
use super::*;
use crate::brain::{NpcExec, NpcStep, Strike};
use crate::item::Use;
use crate::script_host::SimHost;
use gothic_script::combat::commands::{
    MovePlan, PlanStep, begin_jump_back_command, begin_melee_command, begin_parade_command,
};
use gothic_script::enums as se;
use gothic_script::host::{EntityId, MovePhase};

/// A host that never answers must not freeze an NPC: an instruction ends by itself after this.
const EXEC_TIMEOUT_MS: u64 = 8_000;
/// `OnHit` of the script runs when the `Hit` clip played this fraction of its length
/// (`sAICombatMoveItlLoop`, G:0x10308308); the same constant the hero's mover uses.
pub const HIT_FRACTION: f32 = gothic_script::player::melee::HIT_FRACTION;
/// How long a raised guard stays up when nothing else changes the pose: the two seconds
/// `ZS_Attack_Parade` (S:17c00) waits while the opponent keeps swinging.
const GUARD_HOLD_MS: u64 = 2_000;
/// Headless: how long a cast keeps the caster busy [I: no clip lengths without a body].
const CAST_MS: u64 = 1_500;
/// Headless: the `Begin` clip that raises the guard [I].
const BEGIN_MS: u64 = 500;

fn entity(id: ActorId) -> EntityId {
    EntityId(id.0 + 1)
}

/// The sim action of a script action (`None` for ids the sim does not model).
pub(super) fn sim_action(a: se::Action) -> Option<Action> {
    Some(match a {
        se::Action::None => Action::None,
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
        se::Action::Parade => Action::Parade,
        se::Action::JumpBack => Action::JumpBack,
        _ => return None,
    })
}

/// The stumble `gEAction` a hit result asks for (`AssessHit` returns it; `ZS_Stumble` family
/// runs `_AI_*Stumble`, S:4aaf0 / S:46f80 / S:489c0 / S:47a90 / S:43490). The clips exist in a
/// left and a right form (`HitDirection`, the side the blow came from).
pub fn stumble_action(result: HitResult, from_left: bool) -> Option<se::Action> {
    use se::Action as A;
    Some(match result {
        HitResult::QuickStumble => A::QuickStumble,
        HitResult::Stumble => {
            if from_left {
                A::StumbleL
            } else {
                A::StumbleR
            }
        }
        HitResult::PierceStumble => A::PierceStumble,
        HitResult::QuickParadeStumble | HitResult::ParadeStumble | HitResult::MagicParade => {
            if from_left {
                A::ParadeStumbleL
            } else {
                A::ParadeStumbleR
            }
        }
        HitResult::HeavyParadeStumble => A::HeavyParadeStumble,
        _ => return None,
    })
}

impl<W: WorldQuery> Sim<W> {
    /// The host plays the clips of `actor` (its body is on screen) and reports back.
    pub fn set_host_drives_npc(&mut self, actor: ActorId, on: bool) {
        if let Some(a) = self.actors.get_mut(actor.0 as usize) {
            a.brain.plan.host_drives = on;
            if !on {
                a.brain.plan.exec = NpcExec::Idle;
                a.brain.plan.steps.clear();
            }
        }
    }

    /// Run `f` against the ported commands with `self`/`other` set for actor `i`.
    fn with_npc_host<R>(
        &mut self,
        i: usize,
        f: impl FnOnce(&mut SimHost<'_, W>, EntityId) -> R,
    ) -> R {
        let npc = entity(self.actors[i].id);
        let mut host = SimHost::new(self);
        host.self_entity = npc;
        f(&mut host, npc)
    }

    /// `_AI_Attack` and siblings for NPC `i`: the ported command picks the hand and the pose
    /// and lists the instructions.
    pub(super) fn npc_begin_melee(&mut self, i: usize, action: se::Action, target: ActorId) {
        self.face(i, target);
        let plan = self.with_npc_host(i, |h, npc| begin_melee_command(h, npc, action));
        // Opt-in variant: NPC swings are telegraphed (never changes the player's moves).
        if let Some(ms) = self.cfg.variants.npc_telegraph_ms
            && !self.actors[i].is_player
        {
            self.actors[i].brain.plan.steps.push_back(NpcStep::Wait(ms));
        }
        self.npc_queue_plan(i, &plan, target, false);
    }

    /// `_AI_Parade` for NPC `i`.
    pub(super) fn npc_begin_parade(&mut self, i: usize, target: ActorId) {
        self.face(i, target);
        let plan = self.with_npc_host(i, |h, npc| begin_parade_command(h, npc));
        self.npc_queue_plan(i, &plan, target, true);
    }

    /// `_AI_JumpBack` for NPC `i`.
    pub(super) fn npc_begin_jump_back(&mut self, i: usize, target: ActorId) {
        self.face(i, target);
        let plan = self.with_npc_host(i, |h, npc| begin_jump_back_command(h, npc));
        self.npc_queue_plan(i, &plan, target, false);
    }

    fn npc_queue_plan(&mut self, i: usize, plan: &MovePlan, target: ActorId, guard: bool) {
        let a = &mut self.actors[i];
        a.brain.parade_until_ms = 0;
        if let Some(m) = sim_action(plan.action) {
            a.brain.last_move = m;
        }
        for step in &plan.steps {
            a.brain.plan.steps.push_back(match *step {
                PlanStep::Stamina(d) => NpcStep::Stamina(d),
                PlanStep::Move { phase, speed } => NpcStep::Move {
                    action: plan.action,
                    phase,
                    speed,
                    pose: plan.pose,
                    target: Some(target),
                },
                PlanStep::WaitMs(ms) => NpcStep::Wait(ms),
            });
        }
        a.brain.plan.steps.push_back(NpcStep::End { guard });
    }

    /// The actor's plan is running or has steps left.
    pub(super) fn npc_plan_active(&self, i: usize) -> bool {
        !self.actors[i].brain.plan.is_idle()
    }

    /// Drop the plan (the actor was hit out of its move, or died).
    pub(super) fn npc_cancel_plan(&mut self, i: usize) {
        let p = &mut self.actors[i].brain.plan;
        let was_busy = !p.is_idle();
        p.steps.clear();
        p.exec = NpcExec::Idle;
        if was_busy && p.host_drives {
            let id = self.actors[i].id;
            self.push(EventKind::NpcStop { actor: id });
        }
    }

    /// Advance the plan of actor `i`; true while it still occupies the actor.
    pub(super) fn npc_plan_tick(&mut self, i: usize, now: u64) -> bool {
        for _ in 0..16 {
            // 1. the instruction in flight
            match self.actors[i].brain.plan.exec.clone() {
                NpcExec::Idle => {}
                NpcExec::Wait { until_ms } => {
                    if now < until_ms {
                        return true;
                    }
                    self.actors[i].brain.plan.exec = NpcExec::Idle;
                }
                NpcExec::Move {
                    seq,
                    hit_phase,
                    target,
                    action,
                    hit_at_ms,
                    end_ms,
                    hit_sent,
                } => {
                    let host = self.actors[i].brain.plan.host_drives;
                    let mut sent = hit_sent;
                    if !host
                        && !hit_sent
                        && hit_phase
                        && hit_at_ms.is_some_and(|t| now >= t)
                        && let Some(t) = target
                    {
                        self.npc_strike(i, action, t);
                        sent = true;
                    }
                    if sent != hit_sent
                        && let NpcExec::Move { hit_sent, .. } = &mut self.actors[i].brain.plan.exec
                    {
                        *hit_sent = true;
                    }
                    let timeout = now >= self.actors[i].brain.plan.deadline_ms;
                    let over = if host { timeout } else { now >= end_ms };
                    if !over {
                        return true;
                    }
                    if hit_phase && !sent && !host {
                        // A clip the timers skipped past still lands its blow.
                        if let Some(t) = target {
                            self.npc_strike(i, action, t);
                        }
                    }
                    let _ = seq;
                    self.actors[i].brain.plan.exec = NpcExec::Idle;
                }
            }
            // 2. the next step
            let Some(step) = self.actors[i].brain.plan.steps.pop_front() else {
                return false;
            };
            match step {
                NpcStep::Stamina(d) => {
                    let id = self.actors[i].id;
                    let mut host = SimHost::new(self);
                    host.self_entity = entity(id);
                    gothic_script::stats::add_stamina_points(&mut host, entity(id), d);
                }
                NpcStep::Wait(ms) => {
                    self.actors[i].brain.plan.exec = NpcExec::Wait {
                        until_ms: now + u64::from(ms),
                    };
                }
                NpcStep::Move {
                    action,
                    phase,
                    speed,
                    pose,
                    target,
                } => self.npc_start_move(i, now, action, phase, speed, pose, target),
                NpcStep::End { guard } => {
                    self.npc_end_command(i, now, guard);
                    return false;
                }
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn npc_start_move(
        &mut self,
        i: usize,
        now: u64,
        action: se::Action,
        phase: MovePhase,
        speed: f32,
        pose: se::AniState,
        target: Option<ActorId>,
    ) {
        let id = self.actors[i].id;
        let hit_phase = phase == MovePhase::Hit;
        let host = self.actors[i].brain.plan.host_drives;
        self.actors[i].brain.plan.seq += 1;
        let seq = self.actors[i].brain.plan.seq;
        self.actors[i].brain.plan.deadline_ms = now + EXEC_TIMEOUT_MS;
        if host {
            self.push(EventKind::NpcMove {
                actor: id,
                seq,
                action,
                phase,
                speed,
                pose,
                target,
            });
            self.actors[i].brain.plan.exec = NpcExec::Move {
                seq,
                action,
                hit_phase,
                target,
                hit_at_ms: None,
                end_ms: now + EXEC_TIMEOUT_MS,
                hit_sent: false,
            };
            return;
        }
        // Headless: the legacy timeline of the move. The blow lands when the hit window of
        // the move opened (the `Hit` instruction directly, or at the start of the `Hit` clip
        // after a `Raise`), the instruction ends with the recovery of the move.
        let cast = matches!(action, se::Action::Cast | se::Action::PowerCast);
        let m = sim_action(action).unwrap_or(Action::None);
        let window = if cast {
            0
        } else {
            (vitals::hit_window_secs(m) * 1000.0) as u64
        };
        let total = if cast { CAST_MS } else { move_duration_ms(m) };
        let (hit_at, end) = match phase {
            MovePhase::Raise => {
                self.actors[i].brain.last_raise_ms = Some(now);
                (None, now + window)
            }
            MovePhase::Hit => {
                let start = self.actors[i].brain.last_raise_ms.take().unwrap_or(now);
                (Some(start + window), start + total)
            }
            MovePhase::Begin => (None, now + BEGIN_MS),
            MovePhase::Recover => (None, now),
        };
        self.actors[i].brain.plan.exec = NpcExec::Move {
            seq,
            action,
            hit_phase,
            target,
            hit_at_ms: hit_at.map(|t| t.max(now)),
            end_ms: end.max(now),
            hit_sent: false,
        };
    }

    /// The command is complete: the pose it leaves.
    fn npc_end_command(&mut self, i: usize, now: u64, guard: bool) {
        let a = &mut self.actors[i];
        a.brain.last_raise_ms = None;
        if guard {
            a.action = Action::Parade;
            a.ani_state = AniState::Parade;
            a.brain.parade_until_ms = now + GUARD_HOLD_MS;
            a.brain.busy_until_ms = now;
        } else if a.action == a.brain.last_move || a.action == Action::None {
            a.action = Action::None;
            if a.ani_state == AniState::Parade {
                a.ani_state = AniState::Stand;
            }
        }
    }

    /// The damage moment of the move in flight (`OnHit`): the reach test and the damage of
    /// `Sim::resolve_strike`; for a cast the moment the spell leaves (`_AI_Cast_Execute`,
    /// S:39180).
    fn npc_strike(&mut self, i: usize, action: se::Action, target: ActorId) {
        if matches!(action, se::Action::Cast | se::Action::PowerCast) {
            let id = self.actors[i].id;
            let power = action == se::Action::PowerCast;
            let mut host = SimHost::new(self);
            host.self_entity = entity(id);
            gothic_script::magic::cast::run_cast(&mut host, entity(id), power);
            return;
        }
        let Some(m) = sim_action(action) else { return };
        // The finishing blow kills through its own timer (`FINISHING_SECS`).
        if !m.is_melee_attack() || m == Action::FinishingAttack {
            return;
        }
        self.resolve_strike(
            i,
            Strike {
                at_ms: 0,
                action: m,
                target,
            },
        );
    }

    /// `_AI_Cast` for NPC `i` (S:38710): the cast animation, the spell leaves when its `Hit`
    /// clip reaches the damage moment.
    pub(super) fn npc_begin_cast(&mut self, i: usize, power: bool, target: ActorId) {
        self.face(i, target);
        let action = if power {
            se::Action::PowerCast
        } else {
            se::Action::Cast
        };
        let a = &mut self.actors[i];
        a.action = Action::Cast;
        a.ani_state = AniState::Stand;
        a.brain.last_move = Action::Cast;
        a.brain.parade_until_ms = 0;
        for phase in [MovePhase::Raise, MovePhase::Hit] {
            a.brain.plan.steps.push_back(NpcStep::Move {
                action,
                phase,
                speed: 1.0,
                pose: se::AniState::Stand,
                target: Some(target),
            });
        }
        a.brain.plan.steps.push_back(NpcStep::End { guard: false });
    }

    /// The clip of the actor's instruction moved its body (root motion of a lunge, a hop back).
    pub fn nudge_npc(&mut self, actor: ActorId, dx: f32, dz: f32) {
        let Some(a) = self.actors.get(actor.0 as usize) else {
            return;
        };
        if a.brain.plan.is_idle() || a.is_player {
            return;
        }
        let old = self.pos(actor);
        let mut p = old;
        p.x += dx;
        p.z += dz;
        // Bodies do not overlap: a lunge stops at the other character's capsule (2 x 35.3 cm,
        // the NPC templates' `CollisionRadius`).
        let blocked = self.actors.iter().any(|o| {
            o.id != actor
                && o.is_alive()
                && self
                    .world
                    .position(o.id)
                    .is_some_and(|q| q.dist_xz(p) < 70.6 && q.dist_xz(p) < q.dist_xz(old))
        });
        if !blocked {
            self.world.teleport(actor, p);
        }
    }

    /// The host's clip of the instruction `seq` of `actor` reached its damage moment.
    pub fn npc_move_hit(&mut self, actor: ActorId, seq: u32) {
        let i = actor.0 as usize;
        let Some(a) = self.actors.get(i) else { return };
        let NpcExec::Move {
            seq: s,
            action,
            hit_phase: true,
            target: Some(t),
            hit_sent: false,
            ..
        } = a.brain.plan.exec.clone()
        else {
            return;
        };
        if s != seq {
            return;
        }
        if let NpcExec::Move { hit_sent, .. } = &mut self.actors[i].brain.plan.exec {
            *hit_sent = true;
        }
        self.npc_strike(i, action, t);
    }

    /// The host finished the instruction `seq` of `actor` (its clip ended, or none was found).
    pub fn npc_exec_done(&mut self, actor: ActorId, seq: u32) {
        let i = actor.0 as usize;
        let Some(a) = self.actors.get(i) else { return };
        if let NpcExec::Move { seq: s, .. } = a.brain.plan.exec
            && s == seq
        {
            self.actors[i].brain.plan.exec = NpcExec::Idle;
        }
    }

    /// A blow knocked NPC `i` out of its move: the reaction clip of the hit result plays and the
    /// actor waits for it (`ZS_Stumble` family). Headless actors use the legacy lock.
    pub(super) fn npc_react_to_hit(
        &mut self,
        i: usize,
        now: u64,
        result: HitResult,
        attacker: ActorId,
    ) {
        self.npc_cancel_plan(i);
        let id = self.actors[i].id;
        if !self.actors[i].brain.plan.host_drives {
            return;
        }
        let from_left = {
            let (me, other) = (self.pos(id), self.pos(attacker));
            let mut azi = me.yaw_to(other) - self.world.facing(id);
            while azi > std::f32::consts::PI {
                azi -= std::f32::consts::TAU;
            }
            while azi < -std::f32::consts::PI {
                azi += std::f32::consts::TAU;
            }
            azi > 0.0
        };
        let Some(action) = stumble_action(result, from_left) else {
            return;
        };
        let speed = self.with_npc_host(i, |h, npc| {
            gothic_script::combat::commands::start_stumble(h, npc, action);
            gothic_script::combat::commands::animation_speed(h, npc, action, false)
        });
        self.actors[i].brain.plan.steps.push_back(NpcStep::Move {
            action,
            phase: MovePhase::Hit,
            speed,
            pose: se::AniState::Stand,
            target: Some(attacker),
        });
        self.actors[i]
            .brain
            .plan
            .steps
            .push_back(NpcStep::End { guard: false });
        let _ = now;
    }
}

// ---- `_AI_SelectWeapon` (S:49600) ------------------------------------------------------------

impl<W: WorldQuery> Sim<W> {
    /// `_AI_SelectWeapon` for NPC `i`: the ported mode choice (`combat::weapon`, S:4a030) and, when
    /// the hands hold another configuration, the swap of the hold instruction
    /// (`_AI_HoldInventoryItems`): melee weapons, bow with arrows, or the spell of
    /// `combat::weapon::choose_spell` (S:49890).
    pub(super) fn npc_select_weapon(&mut self, i: usize) {
        use gothic_script::combat::weapon::{choose_spell, select_weapon_mode};
        use gothic_script::helpers::WeaponMode as M;
        let id = self.actors[i].id;
        let Some(tid) = self.actors[i].target else {
            return;
        };
        let npc = entity(id);
        let has_ranged = {
            let a = &self.actors[i];
            matches!(a.equipment.left_use(), Use::Bow | Use::CrossBow)
                || a.brain.melee_kit.as_ref().is_some_and(|(l, _)| {
                    l.as_ref()
                        .is_some_and(|d| matches!(d.use_kind(), Use::Bow | Use::CrossBow))
                })
                || a.inventory.iter().any(|(t, _)| {
                    self.items
                        .get(t)
                        .is_some_and(|d| matches!(d.use_kind(), Use::Bow | Use::CrossBow))
                })
        };
        let want = {
            let mut host = SimHost::new(self);
            host.self_entity = npc;
            select_weapon_mode(&host, npc, entity(tid), has_ranged)
        };
        let Some(want) = want else { return };
        self.npc_stow_melee(i);
        match want {
            M::Melee | M::None => self.npc_ready_melee(i),
            M::Ranged => self.npc_ready_ranged(i),
            M::Magic => {
                let held = self.actors[i]
                    .equipment
                    .right
                    .as_ref()
                    .filter(|d| d.use_kind() == Use::Cast)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                let spell = {
                    let mut host = SimHost::new(self);
                    host.self_entity = npc;
                    host.other_entity = entity(tid);
                    choose_spell(&mut host, npc, &held)
                };
                self.npc_ready_spell(i, spell);
            }
        }
        self.actors[i].drawn = true;
    }

    /// Remember the melee weapons before the hands take something else.
    fn npc_stow_melee(&mut self, i: usize) {
        let a = &mut self.actors[i];
        if a.equipment.weapon_mode() == crate::item::WeaponMode::Melee {
            a.brain.melee_kit = Some((a.equipment.left.take(), a.equipment.right.take()));
        } else if matches!(a.equipment.right_use(), Use::Cast | Use::Arrow | Use::Bolt) {
            a.equipment.right = None;
            if matches!(a.equipment.left_use(), Use::Bow | Use::CrossBow) {
                a.equipment.left = None;
            }
        }
    }

    fn npc_ready_melee(&mut self, i: usize) {
        let kit = self.actors[i].brain.melee_kit.take();
        let a = &mut self.actors[i];
        if let Some((l, r)) = kit {
            a.equipment.left = l;
            a.equipment.right = r;
            return;
        }
        if a.equipment.weapon_mode() == crate::item::WeaponMode::Melee {
            return;
        }
        a.equipment.left = None;
        let found = a
            .inventory
            .iter()
            .filter_map(|(t, _)| self.items.get(t))
            .find(|d| d.use_kind().is_melee_weapon())
            .cloned();
        let sp = a.species;
        let def = found.or_else(|| crate::data::default_weapon(&self.items, sp));
        if let Some(d) = def {
            self.actors[i].equipment.equip(d);
        }
    }

    fn npc_ready_ranged(&mut self, i: usize) {
        let a = &self.actors[i];
        let bow = a
            .brain
            .melee_kit
            .iter()
            .filter_map(|(l, _)| l.clone())
            .find(|d| matches!(d.use_kind(), Use::Bow | Use::CrossBow))
            .or_else(|| {
                a.inventory
                    .iter()
                    .filter_map(|(t, _)| self.items.get(t))
                    .find(|d| matches!(d.use_kind(), Use::Bow | Use::CrossBow))
                    .cloned()
            });
        let Some(bow) = bow else {
            return self.npc_ready_melee(i);
        };
        let ammo = if bow.use_kind() == Use::CrossBow {
            "Bolt"
        } else {
            "Arrow"
        };
        let ammo_def = self.items.get(ammo).cloned();
        let a = &mut self.actors[i];
        a.equipment.left = Some(bow);
        a.equipment.right = ammo_def;
    }

    fn npc_ready_spell(&mut self, i: usize, spell: &str) {
        let Some(def) = self.items.get(spell).cloned() else {
            return self.npc_ready_melee(i);
        };
        let a = &mut self.actors[i];
        a.inventory.insert(spell, 1);
        // A staff stays in the other hand (the `(Staff, Cast)` row of the hand table, CA 2.12).
        let staff = a
            .brain
            .melee_kit
            .as_ref()
            .and_then(|(l, r)| {
                [l, r]
                    .into_iter()
                    .flatten()
                    .find(|d| d.use_kind() == Use::Staff)
            })
            .cloned();
        a.equipment.left = staff;
        a.equipment.right = Some(def);
    }
}
