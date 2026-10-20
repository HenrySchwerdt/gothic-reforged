//! The simulation: fixed-tick stepping of all actors, hit application, kill / defeat
//! bookkeeping, perception reactions, crimes, party, trade, and the bridge to the quest /
//! dialog engine of `gothic-gameplay` (forwarded world effects in, kill / defeat / XP out).
//!
//! Determinism: one seeded [`Rng`], fixed tick length, actors processed in id order, `BTree*`
//! containers only. Positions and geometry come from the injected [`WorldQuery`].
use crate::actor::{Actor, FightVsPlayer};
use crate::attitude::{AttitudeCtx, get_attitude, political_attitude};
use crate::brain::{Strike, Task};
use crate::combat::{
    Attack, Deity, HitContext, HitOutcome, HitResult, IgnoreReason, PerceptionKind,
    bow_hit_multiplier, kill_xp, resolve_hit,
};
use crate::config::SimConfig;
use crate::crime::{
    Enclave, EnclaveStatus, Memorized, Reaction, crime_for_reason, excuse_succeeds,
    is_bothered_by_theft_of, memorize_player_crime, react_to_crime,
};
use crate::data::NpcRecord;
use crate::economy::{self, PickpocketResult, PriceCtx, TradeSide};
use crate::item::{ItemDb, ItemDef, Unmet, check_requirements};
use crate::perception::{can_perceive, can_receive_perception, fight_hearing_range, sees_within};
use crate::rng::Rng;
use crate::routine::{
    GameClock, RoutineResolver, TaskContext, follow_distance, routine_time, select_routine_task,
};
use crate::tactics::{
    RangedDecision, TacticsCtx, attack_range, follow_time_secs, ranged_move, select_combat_move,
    wants_potion, wants_to_flee,
};
use crate::types::*;
use crate::vitals::{self, TickEnv, stamina_cost};
use crate::world::{Pos, WorldQuery, in_fov};
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::script::{ExecResult, Output, npc_defeated, npc_killed};
use gothic_gameplay::{GameDatabase, GameState};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Run speed and walk speed in game units per second [I].
pub const RUN_SPEED: f32 = 550.0;
pub const WALK_SPEED: f32 = 180.0;
/// `MinHitDuration` unit in milliseconds [CA 0.2, I].
const HIT_DURATION_UNIT_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Hit {
        attacker: ActorId,
        victim: ActorId,
        action: Action,
        damage: i32,
        result: HitResult,
        parried: bool,
        ignored: Option<IgnoreReason>,
    },
    /// A strike whose target had moved out of range.
    Whiff {
        attacker: ActorId,
        target: ActorId,
        action: Action,
    },
    Killed {
        victim: ActorId,
        killer: Option<ActorId>,
    },
    Defeated {
        victim: ActorId,
        by: Option<ActorId>,
    },
    WokeUp {
        actor: ActorId,
    },
    Xp {
        amount: u64,
        level_before: u32,
        level_after: u32,
    },
    AttackStarted {
        actor: ActorId,
        target: ActorId,
        reason: AttackReason,
    },
    Fled {
        actor: ActorId,
        from: ActorId,
    },
    Warned {
        actor: ActorId,
        stage: u8,
    },
    CrimeWitnessed {
        witness: ActorId,
        culprit: ActorId,
        crime: Crime,
    },
    CrimeMemorized {
        witness: ActorId,
        crime: Crime,
        outcome: Memorized,
    },
    HelpCalled {
        by: ActorId,
        helper: ActorId,
    },
    Excuse {
        npc: ActorId,
        success: bool,
    },
    Pickpocket {
        target: ActorId,
        result: PickpocketResult,
    },
    Potion {
        actor: ActorId,
        healed: i32,
    },
    Fall {
        actor: ActorId,
        damage: i32,
    },
    Quest {
        quest: String,
        from: QuestStatus,
        to: QuestStatus,
    },
    Spawned {
        actor: ActorId,
    },
    Teleported {
        actor: ActorId,
    },
    PartyJoined {
        actor: ActorId,
    },
    PartyLeft {
        actor: ActorId,
    },
    TradeOpened {
        npc: ActorId,
    },
    /// A forwarded effect the simulation does not act on (yet).
    Unhandled {
        command: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub t_ms: u64,
    pub kind: EventKind,
}

/// Spell data (properties of the spell template, not derivable from script) [CA 9.7].
#[derive(Debug, Clone)]
pub struct SpellSpec {
    pub name: String,
    pub damage_type: DamageType,
    pub damage: i32,
    pub min_mana: i32,
    pub max_mana: i32,
    pub deity: Deity,
}

pub struct Sim<W: WorldQuery> {
    pub cfg: SimConfig,
    pub rng: Rng,
    pub tick: u64,
    pub clock: GameClock,
    pub world: W,
    pub items: ItemDb,
    pub actors: Vec<Actor>,
    pub state: GameState,
    pub db: Option<Arc<GameDatabase>>,
    pub enclaves: BTreeMap<String, Enclave>,
    pub chapter: u32,
    pub player_alignment: Alignment,
    pub hero: Option<ActorId>,
    pub events: Vec<Event>,
    pub resolver: Option<Box<dyn RoutineResolver>>,
    /// The NPC of the running dialog (`npc` / `self` in forwarded effects).
    pub dialog_partner: Option<ActorId>,
    pub gui_open: bool,
    pub npc_templates: BTreeMap<String, NpcRecord>,
    /// Script-port state (routine tasks, interact points, queued AI commands), see
    /// [`crate::script_host`].
    pub script: crate::script_host::ScriptTables,
}

fn two_mut(v: &mut [Actor], i: usize, j: usize) -> (&mut Actor, &mut Actor) {
    assert_ne!(i, j);
    if i < j {
        let (a, b) = v.split_at_mut(j);
        (&mut a[i], &mut b[0])
    } else {
        let (a, b) = v.split_at_mut(i);
        (&mut b[0], &mut a[j])
    }
}

/// Total duration of a melee move including recovery [I]; the hit window opens earlier
/// ([`vitals::hit_window_secs`]).
pub fn move_duration_ms(a: Action) -> u64 {
    match a {
        Action::Attack => 900,
        Action::PowerAttack | Action::SprintAttack => 1100,
        Action::QuickAttack | Action::QuickAttackL | Action::QuickAttackR => 500,
        Action::SimpleWhirl => 1100,
        Action::PierceAttack => 1500,
        Action::HackAttack => 1400,
        Action::WhirlAttack => 1400,
        _ => 600,
    }
}

/// Animation lock after a hit [I].
pub fn stagger_ms(r: HitResult) -> u64 {
    match r {
        HitResult::QuickParadeStumble | HitResult::QuickStumble => 400,
        HitResult::ParadeStumble | HitResult::MagicParade => 600,
        HitResult::HeavyParadeStumble => 1000,
        HitResult::Stumble => 800,
        HitResult::PierceStumble => 900,
        HitResult::SitKnockDown => 2500,
        _ => 0,
    }
}

impl<W: WorldQuery> Sim<W> {
    pub fn new(cfg: SimConfig, world: W, items: ItemDb) -> Self {
        let rng = Rng::new(cfg.seed);
        Self {
            cfg,
            rng,
            tick: 0,
            clock: GameClock::at(0, 8),
            world,
            items,
            actors: Vec::new(),
            state: GameState::new(),
            db: None,
            enclaves: BTreeMap::new(),
            chapter: 1,
            player_alignment: Alignment::None,
            hero: None,
            events: Vec::new(),
            resolver: None,
            dialog_partner: None,
            gui_open: false,
            npc_templates: BTreeMap::new(),
            script: crate::script_host::ScriptTables::default(),
        }
    }

    pub fn with_database(mut self, db: Arc<GameDatabase>) -> Self {
        self.db = Some(db);
        self
    }

    pub fn with_state(mut self, state: GameState) -> Self {
        self.clock = GameClock {
            game_ms: state.hours * 3_600_000,
        };
        self.state = state;
        self
    }

    pub fn now_ms(&self) -> u64 {
        self.tick * u64::from(self.cfg.tick_ms)
    }

    fn push(&mut self, kind: EventKind) {
        let t_ms = self.now_ms();
        self.events.push(Event { t_ms, kind });
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    // ---- actors -----------------------------------------------------------------------

    pub fn spawn(&mut self, mut actor: Actor, at: Pos) -> ActorId {
        let id = ActorId(self.actors.len() as u32);
        actor.id = id;
        if actor.hp == 0 && actor.ai_mode != AiMode::Dead {
            // Fresh actors start at full resources (`RefreshHitPoints`) [CA 2.13].
            actor.refresh(&self.cfg);
        }
        self.world.teleport(id, at);
        self.actors.push(actor);
        self.push(EventKind::Spawned { actor: id });
        id
    }

    /// Create the player character from the saved progression (level, attributes, skills).
    pub fn spawn_hero(&mut self, at: Pos) -> ActorId {
        let mut h = Actor::hero(ActorId(0), &self.cfg);
        economy::sync_hero(&self.cfg, &mut h, &self.state);
        h.hp = h.hp_max(&self.cfg);
        h.mana = h.mana_max();
        let id = self.spawn(h, at);
        self.hero = Some(id);
        id
    }

    pub fn spawn_record(&mut self, rec: &NpcRecord, at: Pos) -> ActorId {
        let a = rec.to_actor(ActorId(0), &self.cfg, &self.items);
        self.spawn(a, at)
    }

    pub fn actor(&self, id: ActorId) -> &Actor {
        &self.actors[id.0 as usize]
    }
    pub fn actor_mut(&mut self, id: ActorId) -> &mut Actor {
        &mut self.actors[id.0 as usize]
    }
    pub fn find(&self, name: &str) -> Option<ActorId> {
        self.actors
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .map(|a| a.id)
    }

    pub fn world_pos(&self, id: ActorId) -> Pos {
        self.pos(id)
    }

    fn pos(&self, id: ActorId) -> Pos {
        self.world.position(id).unwrap_or_default()
    }
    pub fn dist(&self, a: ActorId, b: ActorId) -> f32 {
        self.pos(a).dist_xz(self.pos(b))
    }

    /// Wear an item from the database, checking attribute and skill requirements [GAME_RULES 4.2].
    pub fn equip(&mut self, id: ActorId, item: &str) -> Result<(), String> {
        let def = self
            .items
            .get(item)
            .cloned()
            .ok_or_else(|| format!("unknown item {item}"))?;
        self.equip_def(id, def).map_err(|u| format!("{u:?}"))
    }

    pub fn equip_def(&mut self, id: ActorId, def: ItemDef) -> Result<(), Unmet> {
        let a = &self.actors[id.0 as usize];
        // NPC equipment ignores requirements (loot is generated for them).
        if a.is_player {
            check_requirements(&def, |t| a.attribute(&self.cfg, t), &a.perks)?;
        }
        let a = &mut self.actors[id.0 as usize];
        a.equipment.equip(def);
        Ok(())
    }

    // ---- attitude -----------------------------------------------------------------------

    pub fn attitude(&self, from: ActorId, to: ActorId) -> Attitude {
        let ctx = AttitudeCtx {
            cfg: &self.cfg,
            chapter: self.chapter,
            fame: &self.state.fame,
            enclaves: &self.enclaves,
            player_alignment: self.player_alignment,
        };
        let player = self.hero.map_or(self.actor(from), |h| self.actor(h));
        get_attitude(&ctx, self.actor(from), self.actor(to), player)
    }

    fn reporter_friendly(&self, npc: &Actor) -> bool {
        political_attitude(
            &self.cfg,
            self.chapter,
            npc.alignment,
            self.player_alignment,
        ) == Attitude::Friendly
    }

    fn is_player_side(&self, id: ActorId) -> bool {
        let a = self.actor(id);
        a.is_player || (a.party_leader.is_some() && a.party_leader == self.hero && !a.summoned)
    }

    // ---- stepping -----------------------------------------------------------------------

    pub fn step(&mut self) {
        self.tick += 1;
        let now = self.now_ms();
        self.clock
            .advance(u64::from(self.cfg.tick_ms), self.cfg.game_speed);
        self.state.hours = self.clock.total_hours();
        for i in 0..self.actors.len() {
            self.think(i, now);
        }
    }

    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Step until `done` holds or `max_ticks` passed; returns whether it held.
    pub fn run_until(&mut self, max_ticks: u64, mut done: impl FnMut(&Self) -> bool) -> bool {
        for _ in 0..max_ticks {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    fn think(&mut self, i: usize, now: u64) {
        if self.actors[i].ai_mode == AiMode::Dead {
            return;
        }
        // Once-per-second resource tick [CA 5.2, I on the interval].
        if self.actors[i].brain.task != Task::Unconscious
            && now >= self.actors[i].brain.next_second_ms
        {
            self.actors[i].brain.next_second_ms = now + 1000;
            self.second_tick(i, now);
            if self.actors[i].ai_mode == AiMode::Dead {
                return;
            }
        }
        if self.actors[i].ani_state == AniState::SitKnockDown
            && now >= self.actors[i].brain.busy_until_ms
        {
            self.actors[i].ani_state = AniState::Stand;
        }
        if !self.actors[i].ai_controlled {
            return;
        }
        let task = self.actors[i].brain.task;
        match task {
            Task::Dead => {}
            Task::Unconscious => self.unconscious_think(i, now),
            _ if now < self.actors[i].brain.busy_until_ms && task != Task::Attack => {}
            Task::Attack | Task::AttackWaitForFlee => self.attack_think(i, now),
            Task::Flee => self.flee_think(i, now),
            Task::Threaten | Task::ReactToWeapon => self.reaction_think(i, now),
            Task::FollowPlayer | Task::WaitForPlayer => {
                self.follow_think(i, now);
                self.perception_poll(i, now);
            }
            Task::Routine => {
                self.routine_think(i, now);
                self.perception_poll(i, now);
            }
        }
    }

    fn second_tick(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let env = TickEnv {
            gui_open: self.gui_open,
            ..TickEnv::default()
        };
        vitals::apply_tick(&self.cfg, &mut self.actors[i], &env);
        if self.actors[i].hp <= 0 {
            if self.actors[i].can_be_killed() {
                self.kill(id, None);
            } else {
                self.actors[i].hp = 1;
            }
        }
        let _ = now;
    }

    // ---- hits -----------------------------------------------------------------------------

    /// Situation inputs for a hit between two actors, read from the world.
    pub fn hit_context(&self, att: ActorId, vic: ActorId) -> HitContext {
        let (ap, vp) = (self.pos(att), self.pos(vic));
        let fov = in_fov(&self.world, vic, ap, self.cfg.fov_degrees);
        let los = self.world.line_of_sight(vp, ap);
        let a = self.actor(att);
        let v = self.actor(vic);
        HitContext {
            distance: ap.dist(vp),
            attacker_in_victim_fov: fov,
            victim_sees_attacker: fov && los,
            victim_is_attacker_target: a.is_player || a.target == Some(vic),
            victim_to_attacker: self.attitude(vic, att),
            attacker_to_victim: self.attitude(att, vic),
            gui_open: self.gui_open,
            victim_in_arena: v.attack_reason == AttackReason::Arena,
            victim_friendly_to_player: self
                .hero
                .is_some_and(|h| self.attitude(vic, h) == Attitude::Friendly),
        }
    }

    /// Resolve and apply `attack` (its context is filled from the world) and run the full
    /// aftermath: stagger lock, kill / defeat bookkeeping, perception broadcast, reactions.
    pub fn hit(&mut self, att: ActorId, vic: ActorId, mut attack: Attack) -> HitOutcome {
        attack.ctx = self.hit_context(att, vic);
        self.apply_attack(att, vic, &attack)
    }

    /// Melee hit with the attacker's current weapon.
    pub fn melee_hit(&mut self, att: ActorId, vic: ActorId) -> HitOutcome {
        let attack = Attack::melee(self.actor(att));
        self.hit(att, vic, attack)
    }

    fn apply_attack(&mut self, att: ActorId, vic: ActorId, attack: &Attack) -> HitOutcome {
        let (ai, vi) = (att.0 as usize, vic.0 as usize);
        let now = self.now_ms();
        let action = self.actors[ai].action;
        let outcome = {
            let (a, v) = two_mut(&mut self.actors, ai, vi);
            resolve_hit(&self.cfg, &mut self.rng, a, v, attack)
        };
        self.push(EventKind::Hit {
            attacker: att,
            victim: vic,
            action,
            damage: outcome.damage,
            result: outcome.result,
            parried: outcome.parried,
            ignored: outcome.ignored,
        });
        if outcome.ignored.is_some() {
            return outcome;
        }
        // Aftermath on the victim.
        let lock = stagger_ms(outcome.result);
        {
            let v = &mut self.actors[vi];
            v.brain.last_hit_taken_ms = Some(now);
            if outcome.result == HitResult::SitKnockDown {
                // A knocked-down character cannot be hit again [CA 1.1].
                v.ani_state = AniState::SitKnockDown;
            }
            if lock > 0 && v.action != Action::SprintAttack {
                v.brain.strike = None;
                v.action = Action::None;
                v.brain.parade_until_ms = 0;
                v.brain.busy_until_ms = now + lock;
            }
        }
        if outcome.killed() {
            self.kill(vic, Some(att));
        } else if outcome.defeated() {
            self.defeat(vic, Some(att));
        }
        if let Some(kind) = outcome.perception {
            self.broadcast(kind, att, vic);
        }
        // The victim reacts (NPCs and bots).
        if !outcome.killed() && !outcome.defeated() && self.actors[vi].ai_controlled {
            let v = &self.actors[vi];
            if v.brain.task != Task::Attack || v.target != Some(att) {
                if v.brain.task == Task::Attack && v.attack_reason >= AttackReason::ReactToDamage {
                    // already fighting someone else for a serious reason
                } else {
                    self.assess_target(vic, att, AttackReason::ReactToDamage);
                }
            }
        }
        outcome
    }

    /// Ranged shot: the draw time sets the damage multiplier [CA 9.6]; hits land with
    /// `ranged_hit_pct` (the original is aim / collision driven, no roll) [CA 0.1].
    pub fn shoot(
        &mut self,
        att: ActorId,
        vic: ActorId,
        held_secs: f32,
        ammo: &str,
    ) -> Option<HitOutcome> {
        let ammo_def = self.items.get(ammo).cloned()?;
        // Crossbows and NPC archers shoot at full power; the player's bow draw sets the
        // multiplier [CA 9.6].
        let full_power = self.actor(att).equipment.left_use() == crate::item::Use::CrossBow
            || !self.actor(att).is_player;
        let mult = bow_hit_multiplier(if full_power { 2.0 } else { held_secs });
        if self.actor(att).is_player {
            // NPC archers re-supply ammunition automatically [CA 9.6].
            self.state.take_item(ammo, 1);
        }
        let now = self.now_ms();
        self.actors[att.0 as usize].action = Action::Shoot;
        self.actors[att.0 as usize].brain.busy_until_ms = now + 1500;
        if !self.rng.chance_pct(self.cfg.ranged_hit_pct) {
            self.push(EventKind::Whiff {
                attacker: att,
                target: vic,
                action: Action::Shoot,
            });
            return None;
        }
        let attack = Attack::projectile(&ammo_def, mult);
        Some(self.hit(att, vic, attack))
    }

    /// Cast a projectile spell: mana (player only; NPC mana is a constant 1000 [CA 2.13]) is
    /// spent up to `mana_used`, which feeds the damage formula [CA 2.5, 9.7].
    pub fn cast(
        &mut self,
        caster: ActorId,
        vic: ActorId,
        spell: &SpellSpec,
        power: bool,
    ) -> Option<HitOutcome> {
        let cost = if power {
            spell.max_mana
        } else {
            spell.min_mana
        };
        let c = &mut self.actors[caster.0 as usize];
        if c.is_player {
            if c.mana < cost {
                return None;
            }
            c.mana -= cost;
        }
        let used = if c.is_player { cost } else { 0 };
        let attack = Attack::spell(
            &spell.name,
            spell.damage_type,
            spell.damage,
            used,
            spell.deity,
        );
        Some(self.hit(caster, vic, attack))
    }

    // ---- kill / defeat -------------------------------------------------------------------

    fn absorb(&mut self, r: ExecResult) {
        self.apply_outputs(&r.outputs);
    }

    /// `Kill` (S:1bc30) [CA 4.1]: AIMode Dead, quest hooks, XP once for the player side.
    pub fn kill(&mut self, victim: ActorId, killer: Option<ActorId>) {
        let vi = victim.0 as usize;
        let name = self.actors[vi].name.clone();
        {
            let v = &mut self.actors[vi];
            v.ai_mode = AiMode::Dead;
            v.brain.task = Task::Dead;
            v.brain.strike = None;
            v.action = Action::None;
            v.target = None;
            v.hp = 0;
        }
        self.push(EventKind::Killed { victim, killer });
        if self.actors[vi].is_player {
            self.state.game_over = true;
        } else {
            self.quest_killed(&name);
        }
        if let Some(k) = killer {
            self.award_kill_xp(victim, k);
        }
    }

    fn quest_killed(&mut self, name: &str) {
        match self.db.clone() {
            Some(db) => {
                let r = npc_killed(&db, &mut self.state, name);
                self.absorb(r);
            }
            None => self.state.npc_mut(name).dead = true,
        }
    }

    fn award_kill_xp(&mut self, victim: ActorId, killer: ActorId) {
        if !self.is_player_side(killer) || self.actors[victim.0 as usize].defeated_by_player {
            return;
        }
        if self.actors[victim.0 as usize].is_player {
            return;
        }
        self.actors[victim.0 as usize].defeated_by_player = true;
        let xp = kill_xp(&self.cfg, &self.actors[victim.0 as usize]);
        self.give_xp(xp);
    }

    /// `GiveXP` to the player through the gameplay state, then refresh the hero's stats.
    pub fn give_xp(&mut self, xp: u64) {
        let (before, after) = economy::grant_xp(&mut self.state, xp);
        self.push(EventKind::Xp {
            amount: xp,
            level_before: before,
            level_after: after,
        });
        self.refresh_hero();
    }

    /// Copy level, taught attributes and skills from the gameplay state into the hero actor
    /// (call after teaching or learning).
    pub fn refresh_hero(&mut self) {
        if let Some(h) = self.hero {
            let mut tmp = self.actors[h.0 as usize].clone();
            economy::sync_hero(&self.cfg, &mut tmp, &self.state);
            let a = &mut self.actors[h.0 as usize];
            a.level = tmp.level;
            a.level_max = tmp.level_max;
            a.base = tmp.base;
            a.perks = tmp.perks;
        }
    }

    /// `Defeat` (S:27510) [CA 4.2]: AIMode Down, knocked-out timer, Attack crime, quest hooks.
    pub fn defeat(&mut self, victim: ActorId, by: Option<ActorId>) {
        let vi = victim.0 as usize;
        let now = self.now_ms();
        let by_player_side = by.is_some_and(|b| self.is_player_side(b));
        let by_player = by.is_some_and(|b| self.actors[b.0 as usize].is_player);
        let name = self.actors[vi].name.clone();
        {
            let arena = self.actors[vi].attack_reason == AttackReason::Arena;
            let v = &mut self.actors[vi];
            v.ai_mode = AiMode::Down;
            v.brain.task = Task::Unconscious;
            v.brain.since_ms = now;
            v.brain.next_second_ms = now + 1000;
            v.brain.unconscious_secs = vitals::unconscious_secs(by_player_side, arena);
            v.brain.unconscious_arena = arena;
            v.brain.strike = None;
            v.action = Action::None;
            v.target = None;
            if v.hp < 1 {
                v.hp = 1;
            }
            // The Attack crime is memorised for a non-guard, non-boss victim outside the arena.
            if by_player && !v.is_guard() && !v.boss && !arena {
                v.last_player_crime = v.last_player_crime.max(Crime::Attack);
            }
        }
        self.push(EventKind::Defeated { victim, by });
        match self.db.clone() {
            Some(db) => {
                let r = npc_defeated(&db, &mut self.state, &name);
                self.absorb(r);
            }
            None => self.state.npc_mut(&name).defeated = true,
        }
        if let Some(k) = by {
            self.award_kill_xp(victim, k);
        }
    }

    fn unconscious_think(&mut self, i: usize, now: u64) {
        if now < self.actors[i].brain.next_second_ms {
            return;
        }
        self.actors[i].brain.next_second_ms = now + 1000;
        let arena = self.actors[i].brain.unconscious_arena;
        vitals::unconscious_tick(&self.cfg, &mut self.actors[i], arena);
        let since = self.actors[i].brain.since_ms;
        let limit = u64::from(self.actors[i].brain.unconscious_secs) * 1000;
        if now.saturating_sub(since) >= limit {
            self.wake_up(i, now);
        }
    }

    /// `ZS_Unconscious_End`: back on its feet, picks a fight with its attacker [CA 9.4].
    fn wake_up(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let attacker = self.actors[i].current_attacker;
        {
            let a = &mut self.actors[i];
            a.ai_mode = AiMode::Reaction;
            a.brain.task = Task::Routine;
            a.brain.since_ms = now;
            a.action = Action::None;
            a.ani_state = AniState::Stand;
        }
        self.push(EventKind::WokeUp { actor: id });
        match attacker {
            Some(t) if self.actors[t.0 as usize].ai_mode != AiMode::Dead => {
                self.assess_target(id, t, AttackReason::ReactToDamage);
            }
            _ => self.actors[i].ai_mode = AiMode::Routine,
        }
    }

    // ---- perception broadcasts and reactions -------------------------------------------

    /// `SendPerceptionFightSound / Defeat / Murder` [CA 1.1, 10.2].
    fn broadcast(&mut self, kind: PerceptionKind, attacker: ActorId, victim: ActorId) {
        let n = self.actors.len();
        let (ap, vp) = (self.pos(attacker), self.pos(victim));
        for r in 0..n {
            let rid = ActorId(r as u32);
            if rid == attacker || rid == victim {
                continue;
            }
            if !can_receive_perception(&self.actors[r], &self.actors[attacker.0 as usize]) {
                continue;
            }
            if !self.actors[r].ai_controlled || self.actors[r].brain.task == Task::Attack {
                continue;
            }
            let rp = self.pos(rid);
            match kind {
                PerceptionKind::FightSound => {
                    let enclave_attack = self.actors[r]
                        .enclave
                        .as_ref()
                        .and_then(|e| self.enclaves.get(e))
                        .is_some_and(|e| e.status == EnclaveStatus::Attack);
                    let range = fight_hearing_range(enclave_attack);
                    if rp.dist(ap) > range && rp.dist(vp) > range {
                        continue;
                    }
                    let to_att = self.attitude(rid, attacker);
                    let to_vic = self.attitude(rid, victim);
                    if matches!(to_att, Attitude::Panic) {
                        self.start_flee(rid, attacker);
                    } else if matches!(to_att, Attitude::Hostile | Attitude::Predator) {
                        self.assess_target(rid, attacker, AttackReason::Enemy);
                    } else if to_vic == Attitude::Friendly && to_att != Attitude::Friendly {
                        let same_enclave = self.actors[r].enclave.is_some()
                            && self.actors[r].enclave == self.actors[victim.0 as usize].enclave;
                        let reason = if same_enclave {
                            AttackReason::StopFight
                        } else {
                            AttackReason::Enemy
                        };
                        self.assess_target(rid, attacker, reason);
                    }
                }
                PerceptionKind::Murder => {
                    let sees = sees_within(
                        &self.cfg,
                        &self.world,
                        &self.actors[r],
                        &self.actors[victim.0 as usize],
                        2000.0,
                    ) || sees_within(
                        &self.cfg,
                        &self.world,
                        &self.actors[r],
                        &self.actors[attacker.0 as usize],
                        2000.0,
                    );
                    if !sees {
                        continue;
                    }
                    let vs = self.actors[victim.0 as usize].species;
                    let reason = if matches!(vs, Species::Pig | Species::Cow) {
                        AttackReason::Livestock
                    } else if vs.is_humanoid() && self.attitude(rid, victim) == Attitude::Friendly {
                        if self.actors[r].enclave.is_some()
                            && self.actors[r].enclave == self.actors[attacker.0 as usize].enclave
                        {
                            AttackReason::StopFight
                        } else {
                            AttackReason::Murder
                        }
                    } else {
                        continue;
                    };
                    if self.actors[r].species.is_humanoid() {
                        self.assess_target(rid, attacker, reason);
                    }
                }
                PerceptionKind::Defeat => {} // comment only [CA 10.2]
            }
        }
    }

    /// `AssessTarget` (S:31470) [CA 10.2]: Panic or "cannot attack" -> flee, else remember
    /// the reason (a lower one never replaces a higher one in combat), memorise the crime,
    /// attack.
    pub fn assess_target(&mut self, npc: ActorId, target: ActorId, reason: AttackReason) {
        if npc == target || !self.actors[npc.0 as usize].is_alive() {
            return;
        }
        let att = self.attitude(npc, target);
        if att == Attitude::Panic || !self.actors[npc.0 as usize].species.can_attack() {
            self.start_flee(npc, target);
            return;
        }
        let crime = crime_for_reason(reason);
        if self.actors[target.0 as usize].is_player && crime != Crime::None {
            let friendly = self.reporter_friendly(&self.actors[npc.0 as usize]);
            let (mut w, enclaves) = (self.actors[npc.0 as usize].clone(), &mut self.enclaves);
            let outcome = memorize_player_crime(&mut w, enclaves, crime, friendly);
            self.actors[npc.0 as usize].last_player_crime = w.last_player_crime;
            self.push(EventKind::CrimeMemorized {
                witness: npc,
                crime,
                outcome,
            });
        }
        self.start_attack(npc, target, reason);
    }

    pub fn start_attack(&mut self, npc: ActorId, target: ActorId, reason: AttackReason) {
        let now = self.now_ms();
        let tp = self.actors[target.0 as usize].is_player;
        let a = &mut self.actors[npc.0 as usize];
        if a.ai_mode == AiMode::Dead {
            return;
        }
        let same = a.brain.task == Task::Attack && a.target == Some(target);
        if a.brain.task == Task::Attack && a.target.is_some() && !same && reason <= a.attack_reason
        {
            return; // keep the current fight; a lower reason never replaces a higher one
        }
        a.attack_reason = a.attack_reason.max(reason);
        if same {
            return;
        }
        if a.ai_mode != AiMode::Arena {
            a.ai_mode = AiMode::Combat;
        }
        a.brain.task = Task::Attack;
        a.brain.since_ms = now;
        a.brain.chase_since_ms = None;
        a.target = Some(target);
        a.drawn = true;
        if tp {
            a.last_fight_vs_player = FightVsPlayer::Running;
        }
        self.push(EventKind::AttackStarted {
            actor: npc,
            target,
            reason,
        });
    }

    pub fn start_flee(&mut self, npc: ActorId, from: ActorId) {
        let now = self.now_ms();
        let a = &mut self.actors[npc.0 as usize];
        if a.ai_mode == AiMode::Dead || a.brain.task == Task::Flee {
            return;
        }
        a.ai_mode = AiMode::Flee;
        a.brain.task = Task::Flee;
        a.brain.since_ms = now;
        a.target = Some(from);
        a.brain.strike = None;
        self.push(EventKind::Fled { actor: npc, from });
    }

    fn back_to_routine(&mut self, i: usize, now: u64) {
        let leader = self.actors[i].party_leader;
        let hero = self.hero;
        let a = &mut self.actors[i];
        a.ai_mode = AiMode::Routine;
        a.brain.task = if leader.is_some() && leader == hero {
            Task::FollowPlayer
        } else {
            Task::Routine
        };
        a.brain.since_ms = now;
        a.brain.strike = None;
        a.brain.chase_since_ms = None;
        a.target = None;
        a.action = Action::None;
        a.attack_reason = AttackReason::None;
    }

    // ---- combat AI ----------------------------------------------------------------------

    fn face(&mut self, i: usize, target: ActorId) {
        let p = self.pos(target);
        self.world.face_towards(self.actors[i].id, p);
    }

    fn attack_think(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        // Animations finish.
        {
            let a = &mut self.actors[i];
            if a.action == Action::Parade && now >= a.brain.parade_until_ms {
                a.action = Action::None;
            }
            if a.brain.strike.is_none()
                && now >= a.brain.busy_until_ms
                && a.action != Action::None
                && a.action != Action::Parade
            {
                a.action = Action::None;
            }
        }
        // The hit window of a pending strike.
        if let Some(st) = self.actors[i].brain.strike
            && now >= st.at_ms
        {
            self.actors[i].brain.strike = None;
            self.resolve_strike(i, st);
            if self.actors[i].ai_mode == AiMode::Dead || self.actors[i].brain.task != Task::Attack {
                return;
            }
        }
        if now < self.actors[i].brain.busy_until_ms {
            if let Some(t) = self.actors[i].target {
                self.face(i, t);
            }
            return;
        }
        // Target checks.
        let Some(tid) = self.actors[i].target else {
            self.end_attack(i, now);
            return;
        };
        let tmode = self.actors[tid.0 as usize].ai_mode;
        if matches!(tmode, AiMode::Dead | AiMode::Down) {
            self.end_attack(i, now);
            return;
        }
        let dist = self.dist(id, tid);
        let (sp, fist) = {
            let a = &self.actors[i];
            (a.species, a.equipment.weapon_damage().is_none())
        };
        let range = attack_range(sp, fist, Action::Attack);

        // Give-up rule [CA 9.1].
        if self.actors[i].brain.task == Task::AttackWaitForFlee {
            if now - self.actors[i].brain.since_ms >= 500 {
                if dist > 2000.0 {
                    if self.actors[tid.0 as usize].is_player {
                        self.actors[i].last_fight_vs_player = FightVsPlayer::Cancel;
                    }
                    self.back_to_routine(i, now);
                } else {
                    self.actors[i].brain.task = Task::Attack;
                    self.actors[i].brain.chase_since_ms = None;
                }
            }
            return;
        }
        if dist > range {
            match self.actors[i].brain.chase_since_ms {
                None => self.actors[i].brain.chase_since_ms = Some(now),
                Some(t0) => {
                    if now - t0 > (follow_time_secs(&self.cfg, sp) * 1000.0) as u64 {
                        self.actors[i].brain.task = Task::AttackWaitForFlee;
                        self.actors[i].brain.since_ms = now;
                        return;
                    }
                }
            }
        } else {
            self.actors[i].brain.chase_since_ms = None;
        }

        // Optional variant: flee on low HP.
        if wants_to_flee(&self.cfg, &self.actors[i]) {
            self.start_flee(id, tid);
            return;
        }
        // The potion rule [CA 9.1].
        if wants_potion(&self.cfg, &self.actors[i], dist) {
            self.drink_potion(i, now);
            return;
        }

        match self.actors[i].equipment.weapon_mode() {
            crate::item::WeaponMode::Ranged => {
                let los = self.world.line_of_sight(self.pos(id), self.pos(tid));
                match ranged_move(dist, los) {
                    RangedDecision::Approach => {
                        self.world
                            .move_towards(id, self.pos(tid), RUN_SPEED * self.dt());
                    }
                    RangedDecision::NewTarget => {
                        self.world
                            .move_towards(id, self.pos(tid), WALK_SPEED * self.dt());
                    }
                    RangedDecision::Shoot => {
                        self.face(i, tid);
                        let ammo =
                            if self.actors[i].equipment.left_use() == crate::item::Use::CrossBow {
                                "It_Bolt"
                            } else {
                                "It_Arrow"
                            };
                        if self.shoot(id, tid, 2.0, ammo).is_none() {
                            self.actors[i].brain.busy_until_ms = now + 1500;
                        }
                    }
                }
            }
            crate::item::WeaponMode::Magic => {
                // NPC spell casting needs per-spell data; hosts drive it through `cast`. [stub]
                self.face(i, tid);
            }
            _ => self.melee_decision(i, tid, now, dist),
        }
    }

    fn dt(&self) -> f32 {
        self.cfg.tick_ms as f32 / 1000.0
    }

    fn melee_decision(&mut self, i: usize, tid: ActorId, now: u64, dist: f32) {
        let id = self.actors[i].id;
        let (sp, fist) = {
            let a = &self.actors[i];
            (a.species, a.equipment.weapon_damage().is_none())
        };
        let ctx = {
            let t = &self.actors[tid.0 as usize];
            let can_hit = self.actors[i].brain.last_hit_taken_ms.is_none_or(|t0| {
                now.saturating_sub(t0)
                    >= u64::from(self.cfg.min_hit_duration) * HIT_DURATION_UNIT_MS
            });
            TacticsCtx {
                dist,
                attack_range: attack_range(sp, fist, Action::Attack),
                target_attacking: t
                    .brain
                    .strike
                    .is_some_and(|s| s.target == id && s.at_ms >= now),
                target_parading: t.is_parading(),
                los: self.world.line_of_sight(self.pos(id), self.pos(tid)),
                target_is_player: t.is_player,
                target_is_mage: t.class == NpcClass::Mage,
                arena: self.actors[i].ai_mode == AiMode::Arena,
                party_condition: false,
                can_hit,
                current_move: self.actors[i].brain.last_move,
                defensive: self.actors[i].stamina * 100
                    < self.actors[i].stamina_max(&self.cfg) * 15,
            }
        };
        let mv = if self.actors[i].combat_state_active {
            select_combat_move(&self.cfg, &mut self.rng, &self.actors[i], &ctx)
        } else {
            crate::tactics::passive_move(dist)
        };
        self.execute_move(i, tid, mv, now, dist);
    }

    fn execute_move(&mut self, i: usize, tid: ActorId, mv: Action, now: u64, dist: f32) {
        let id = self.actors[i].id;
        let tp = self.pos(tid);
        let (sp, fist) = {
            let a = &self.actors[i];
            (a.species, a.equipment.weapon_damage().is_none())
        };
        let dt = self.dt();
        match mv {
            Action::Fwd => {
                let stop = attack_range(sp, fist, Action::Attack) * 0.8;
                let step = (RUN_SPEED * dt).min((dist - stop).max(0.0));
                if step > 0.0 {
                    self.world.move_towards(id, tp, step);
                }
                self.face(i, tid);
                self.actors[i].brain.last_move = Action::Fwd;
            }
            Action::Left | Action::Right | Action::Back | Action::JumpBack => {
                let me = self.pos(id);
                let dir = me.yaw_to(tp);
                let (dx, dz) = match mv {
                    Action::Back | Action::JumpBack => (-dir.cos(), -dir.sin()),
                    Action::Left => (dir.sin(), -dir.cos()),
                    _ => (-dir.sin(), dir.cos()),
                };
                let len = if mv == Action::JumpBack { 250.0 } else { 120.0 };
                let to = Pos::new(me.x + dx * len, me.y, me.z + dz * len);
                self.world.move_towards(id, to, len);
                self.face(i, tid);
                let a = &mut self.actors[i];
                a.brain.busy_until_ms = now + if mv == Action::JumpBack { 600 } else { 400 };
                a.brain.last_move = mv;
                a.stamina = (a.stamina - stamina_cost(&self.cfg, mv)).max(0);
            }
            Action::TurnLeft | Action::TurnRight | Action::Wait => {
                self.face(i, tid);
                self.actors[i].brain.busy_until_ms = now + 200;
                self.actors[i].brain.last_move = mv;
            }
            Action::Parade => {
                self.face(i, tid);
                let a = &mut self.actors[i];
                a.action = Action::Parade;
                a.brain.parade_until_ms = now + 500;
                a.brain.busy_until_ms = now + 600;
                a.brain.last_move = Action::Parade;
            }
            m if m.is_melee_attack() => {
                let reach = attack_range(sp, fist, m);
                if dist > reach {
                    // Not in reach: close the distance instead.
                    let step = (RUN_SPEED * dt).min(dist - reach * 0.8);
                    self.world.move_towards(id, tp, step.max(0.0));
                    self.face(i, tid);
                    return;
                }
                let cost = stamina_cost(&self.cfg, m);
                self.face(i, tid);
                let a = &mut self.actors[i];
                if a.stamina <= 0 && cost > 0 {
                    // Exhausted: catch breath.
                    a.brain.busy_until_ms = now + 500;
                    return;
                }
                a.stamina = (a.stamina - cost).max(0);
                a.action = m;
                a.brain.last_move = m;
                a.brain.strike = Some(Strike {
                    at_ms: now + (vitals::hit_window_secs(m) * 1000.0) as u64,
                    action: m,
                    target: tid,
                });
                a.brain.busy_until_ms = now + move_duration_ms(m);
            }
            _ => {
                self.actors[i].brain.busy_until_ms = now + 200;
            }
        }
    }

    fn resolve_strike(&mut self, i: usize, st: Strike) {
        let id = self.actors[i].id;
        if matches!(self.actors[i].ai_mode, AiMode::Dead | AiMode::Down) {
            return;
        }
        let t = &self.actors[st.target.0 as usize];
        if matches!(t.ai_mode, AiMode::Dead | AiMode::Down) {
            return;
        }
        let (sp, fist) = {
            let a = &self.actors[i];
            (a.species, a.equipment.weapon_damage().is_none())
        };
        let reach = attack_range(sp, fist, st.action) + 50.0;
        if self.dist(id, st.target) > reach {
            self.push(EventKind::Whiff {
                attacker: id,
                target: st.target,
                action: st.action,
            });
            return;
        }
        self.actors[i].action = st.action;
        let attack = Attack::melee(&self.actors[i]);
        self.hit(id, st.target, attack);
    }

    fn drink_potion(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let heal = self
            .items
            .get("It_Potion_Health")
            .map_or(50, ItemDef::heal_amount)
            .max(1);
        let max = self.actors[i].hp_max(&self.cfg);
        let a = &mut self.actors[i];
        if let Some(n) = a.inventory.get_mut("it_potion_health") {
            *n = n.saturating_sub(1);
        }
        let before = a.hp;
        a.hp = (a.hp + heal).min(max);
        a.action = Action::Heal;
        a.brain.busy_until_ms = now + 1500;
        a.brain.potions_used += 1;
        let healed = a.hp - before;
        self.push(EventKind::Potion { actor: id, healed });
    }

    /// `ZS_Attack_End` (S:1a610): finish a defeated target that deserves it, return to routine.
    fn end_attack(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        if let Some(t) = self.actors[i].target {
            let (down, can_kill) = {
                let tv = &self.actors[t.0 as usize];
                (tv.ai_mode == AiMode::Down, tv.can_be_killed())
            };
            let zone_friendly = false;
            if down && can_kill && self.actors[i].attack_reason.to_kill(zone_friendly) {
                // FinishingAttack
                self.actors[i].action = Action::FinishingAttack;
                self.kill(t, Some(id));
            }
            if self.actors[t.0 as usize].is_player
                && self.actors[i].last_fight_vs_player == FightVsPlayer::Running
            {
                self.actors[i].last_fight_vs_player =
                    if self.actors[t.0 as usize].ai_mode == AiMode::Dead {
                        FightVsPlayer::Won
                    } else {
                        FightVsPlayer::Lost
                    };
            }
        }
        self.back_to_routine(i, now);
    }

    fn flee_think(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let Some(from) = self.actors[i].target else {
            self.back_to_routine(i, now);
            return;
        };
        let dist = self.dist(id, from);
        let elapsed = now - self.actors[i].brain.since_ms;
        if dist > 3000.0 || elapsed > 20_000 || self.actors[from.0 as usize].ai_mode == AiMode::Dead
        {
            self.back_to_routine(i, now);
            return;
        }
        let me = self.pos(id);
        let dir = me.yaw_to(self.pos(from));
        let to = Pos::new(me.x - dir.cos() * 1000.0, me.y, me.z - dir.sin() * 1000.0);
        self.world.move_towards(id, to, RUN_SPEED * self.dt());
    }

    fn reaction_think(&mut self, i: usize, now: u64) {
        let id = self.actors[i].id;
        let Some(t) = self.actors[i].target else {
            self.back_to_routine(i, now);
            return;
        };
        let dist = self.dist(id, t);
        let elapsed = now - self.actors[i].brain.since_ms;
        self.face(i, t);
        match self.actors[i].brain.task {
            Task::ReactToWeapon => {
                // [CA 10.3] second warning at 4 s, attack at 8 s; abort if far or lowered.
                let drawn = self.actors[t.0 as usize].drawn
                    && self.actors[t.0 as usize].equipment.weapon_mode()
                        != crate::item::WeaponMode::None;
                if dist > 1000.0 || !drawn {
                    self.back_to_routine(i, now);
                } else if elapsed >= 8000 {
                    self.assess_target(id, t, AttackReason::ReactToWeapon);
                } else if elapsed >= 4000 && self.actors[i].brain.warned < 1 {
                    self.actors[i].brain.warned = 1;
                    self.push(EventKind::Warned {
                        actor: id,
                        stage: 1,
                    });
                }
            }
            Task::Threaten => {
                // [CA 10.3] attack below 700 or after 5 s; give up beyond 1000.
                if dist < 700.0 || elapsed >= 5000 {
                    self.assess_target(id, t, AttackReason::Enemy);
                } else if dist > 1000.0 {
                    self.back_to_routine(i, now);
                }
            }
            _ => {}
        }
    }

    fn follow_think(&mut self, i: usize, now: u64) {
        let Some(h) = self.hero else { return };
        let id = self.actors[i].id;
        if self.actors[i].party_leader != Some(h) {
            self.actors[i].brain.task = Task::Routine;
            return;
        }
        if self.actors[i].brain.task == Task::WaitForPlayer {
            return;
        }
        if matches!(
            self.actors[h.0 as usize].ani_state,
            AniState::Swim | AniState::Dive
        ) {
            self.actors[i].brain.task = Task::WaitForPlayer;
            return;
        }
        let rank = self
            .actors
            .iter()
            .filter(|a| a.party_leader == Some(h) && a.brain.task == Task::FollowPlayer)
            .position(|a| a.id == id)
            .map(|r| r as u32);
        let fd = follow_distance(rank);
        let d = self.dist(id, h);
        if d > fd {
            self.world
                .move_towards(id, self.pos(h), (RUN_SPEED * self.dt()).min(d - fd));
        }
        let _ = now;
    }

    fn routine_think(&mut self, i: usize, now: u64) {
        if self.resolver.is_none() {
            return;
        }
        let id = self.actors[i].id;
        let hour = self.clock.hour_of_day();
        let rt = routine_time(self.actors[i].species, hour);
        let needs = self.actors[i].brain.routine_time != Some(rt)
            || self.actors[i].brain.routine_point.is_none()
                && now.saturating_sub(self.actors[i].brain.last_destination_ms) > 1000;
        if needs {
            let (name, routine) = (
                self.actors[i].name.clone(),
                self.actors[i].routine_name.clone(),
            );
            let pt = self
                .resolver
                .as_ref()
                .and_then(|r| r.resolve(&name, &routine, rt));
            let a = &mut self.actors[i];
            a.brain.routine_time = Some(rt);
            a.brain.last_destination_ms = now;
            if let Some(p) = &pt {
                let ctx = TaskContext {
                    destination_use: Some(&p.use_type),
                    ..TaskContext::default()
                };
                a.brain.routine_task = Some(select_routine_task(a.species, &ctx));
            }
            a.brain.routine_point = pt;
        }
        if let Some(p) = self.actors[i].brain.routine_point.clone() {
            let d = self.pos(id).dist_xz(p.pos);
            if d > 50.0 {
                self.world.move_towards(id, p.pos, WALK_SPEED * self.dt());
            }
        }
    }

    /// `AssessActivePerception` poll every 100 ms [CA 10.1]. Reactions in the doc's order:
    /// Panic -> flee, Predator -> prey, Hostile -> enemy, angry non-humanoid -> threaten, player
    /// weapon -> `ZS_ReactToWeapon`.
    fn perception_poll(&mut self, i: usize, now: u64) {
        if now < self.actors[i].brain.next_perception_ms {
            return;
        }
        self.actors[i].brain.next_perception_ms = now + 100;
        let id = self.actors[i].id;
        let n = self.actors.len();
        for j in 0..n {
            if j == i {
                continue;
            }
            let tid = ActorId(j as u32);
            if matches!(self.actors[j].ai_mode, AiMode::Dead) {
                continue;
            }
            let att = self.attitude(id, tid);
            if !can_perceive(
                &self.cfg,
                &self.world,
                &self.actors[i],
                &self.actors[j],
                att,
            ) {
                continue;
            }
            let down = self.actors[j].is_down_or_dead();
            match att {
                Attitude::Panic if !down => {
                    self.start_flee(id, tid);
                    return;
                }
                Attitude::Predator if !down => {
                    self.assess_target(id, tid, AttackReason::Prey);
                    return;
                }
                Attitude::Hostile if !down => {
                    self.assess_target(id, tid, AttackReason::Enemy);
                    return;
                }
                Attitude::Angry
                    if !down
                        && !self.actors[i].species.is_humanoid()
                        && self.actors[j].species.is_humanoid() =>
                {
                    let a = &mut self.actors[i];
                    a.brain.task = Task::Threaten;
                    a.brain.since_ms = now;
                    a.target = Some(tid);
                    return;
                }
                _ => {}
            }
            // AssessPlayerWeapon [CA 10.2]: humanoid witness within 700.
            let t = &self.actors[j];
            let me = &self.actors[i];
            if t.is_player
                && t.drawn
                && t.equipment.weapon_mode() != crate::item::WeaponMode::None
                && me.species.is_humanoid()
                && me.npc_type != NpcType::Friend
                && att != Attitude::Friendly
                && sees_within(&self.cfg, &self.world, me, t, 700.0)
                && me.brain.task == Task::Routine
            {
                let a = &mut self.actors[i];
                a.ai_mode = AiMode::Reaction;
                a.brain.task = Task::ReactToWeapon;
                a.brain.since_ms = now;
                a.brain.warned = 0;
                a.target = Some(tid);
                return;
            }
        }
    }

    // ---- crimes ------------------------------------------------------------------------

    /// The player takes `item` (`AssessTakeItem`, S:30620) or uses an interact object
    /// (`AssessUseInteractObject`, S:316f0): witnesses within 1000 with FOV / LOS who are
    /// bothered react via [`react_to_crime`]; reacting guards call help [CA 10.2].
    pub fn witness_theft(
        &mut self,
        item: &str,
        marked_dropped: bool,
        interact_object: bool,
    ) -> Vec<ActorId> {
        let Some(hero) = self.hero else { return vec![] };
        if !is_bothered_by_theft_of(item, marked_dropped) {
            return vec![];
        }
        let reason = if interact_object {
            AttackReason::UseInteractObject
        } else {
            AttackReason::Theft
        };
        let mut reacted = Vec::new();
        for j in 0..self.actors.len() {
            let wid = ActorId(j as u32);
            let w = &self.actors[j];
            if wid == hero
                || !w.ai_controlled
                || !can_receive_perception(w, &self.actors[hero.0 as usize])
                || w.npc_type == NpcType::Friend
                || !sees_within(
                    &self.cfg,
                    &self.world,
                    w,
                    &self.actors[hero.0 as usize],
                    1000.0,
                )
            {
                continue;
            }
            let att = self.attitude(wid, hero);
            match react_to_crime(&self.actors[j], reason, att) {
                Reaction::Attack(r) => {
                    self.push(EventKind::CrimeWitnessed {
                        witness: wid,
                        culprit: hero,
                        crime: crime_for_reason(r),
                    });
                    self.assess_target(wid, hero, r);
                    reacted.push(wid);
                }
                Reaction::Flee => {
                    self.start_flee(wid, hero);
                    reacted.push(wid);
                }
                Reaction::Watch | Reaction::Ignore => {}
            }
        }
        for w in reacted.clone() {
            self.call_help(w, hero);
        }
        reacted
    }

    /// `CallHelp` (S:332e0): friendly guards within 1000 of the thief join with `StopThief`.
    fn call_help(&mut self, caller: ActorId, thief: ActorId) {
        let tp = self.pos(thief);
        for j in 0..self.actors.len() {
            let gid = ActorId(j as u32);
            if gid == caller || gid == thief {
                continue;
            }
            let g = &self.actors[j];
            if !g.is_guard() || !g.ai_controlled || g.is_down_or_dead() {
                continue;
            }
            if self.pos(gid).dist(tp) > 1000.0 {
                continue;
            }
            if self.attitude(gid, caller) != Attitude::Friendly {
                continue;
            }
            self.push(EventKind::HelpCalled {
                by: caller,
                helper: gid,
            });
            self.assess_target(gid, thief, AttackReason::StopThief);
        }
    }

    /// `_AI_Excuse` (S:3d130) on `npc` for the crime it remembers [CA 10.5]. Success calms it
    /// down and forgets the personal memory [I]; failure makes it attack.
    pub fn excuse(&mut self, npc: ActorId) -> bool {
        let Some(hero) = self.hero else { return false };
        let ni = npc.0 as usize;
        let crime = {
            let a = &self.actors[ni];
            let enc = a
                .enclave
                .as_ref()
                .and_then(|e| self.enclaves.get(e))
                .map_or(Crime::None, |e| e.known_crime);
            a.last_player_crime.max(enc)
        };
        let h = &self.actors[hero.0 as usize];
        let ok = excuse_succeeds(
            &mut self.rng,
            crime,
            h.theft(&self.cfg),
            h.has_perk("perk_excuse"),
        );
        self.push(EventKind::Excuse { npc, success: ok });
        let now = self.now_ms();
        if ok {
            self.actors[ni].last_player_crime = Crime::None;
            if self.actors[ni].brain.task == Task::Attack {
                self.back_to_routine(ni, now);
            }
        } else {
            self.assess_target(npc, hero, AttackReason::Theft);
        }
        ok
    }

    /// Pickpocketing [CA 8.2]. A caught thief gets `AssessTarget(Theft)` from the victim.
    pub fn pickpocket(&mut self, target: ActorId) -> PickpocketResult {
        let Some(hero) = self.hero else {
            return PickpocketResult::Impossible;
        };
        let res = economy::pickpocket(
            &self.cfg,
            &mut self.rng,
            &self.actors[hero.0 as usize],
            &self.actors[target.0 as usize],
        );
        self.push(EventKind::Pickpocket {
            target,
            result: res,
        });
        match res {
            PickpocketResult::Caught => self.assess_target(target, hero, AttackReason::Theft),
            PickpocketResult::Success { xp } if xp > 0 => self.give_xp(xp),
            _ => {}
        }
        res
    }

    /// `ResetCrime` / pardon: forget all remembered crimes [CA 10.5].
    pub fn reset_crime(&mut self) {
        for e in self.enclaves.values_mut() {
            e.pardon();
        }
        for a in &mut self.actors {
            a.last_player_crime = Crime::None;
            a.last_fight_vs_player = FightVsPlayer::None;
        }
    }

    // ---- fall damage, trade, party ---------------------------------------------------

    /// Landing after `secs` in the fall state [CA 5.1].
    pub fn land(&mut self, id: ActorId, secs: f32) -> i32 {
        let a = &self.actors[id.0 as usize];
        if a.status & status::INVULNERABLE != 0 || a.species == Species::Bloodfly {
            return 0;
        }
        let dmg = vitals::fall_damage(secs, a.has_perk("perk_acrobatic"));
        if dmg > 0 {
            let a = &mut self.actors[id.0 as usize];
            a.hp -= dmg;
            let dead = a.hp <= 0;
            if dead && !a.can_be_killed() {
                a.hp = 1;
            }
            self.push(EventKind::Fall {
                actor: id,
                damage: dmg,
            });
            if dead && self.actors[id.0 as usize].can_be_killed() {
                self.kill(id, None);
            }
        }
        dmg
    }

    /// Price the player pays / gets for `item` with `npc` [CA 8.1].
    pub fn trade_price(&self, npc: ActorId, item: &str, side: TradeSide) -> Option<i32> {
        let def = self.items.get(item)?;
        let hero = self.hero?;
        let ctx = PriceCtx {
            has_barter_perk: self.actor(hero).has_perk("perk_barter"),
            npc_friendly: self.attitude(npc, hero) == Attitude::Friendly,
        };
        Some(economy::trade_price(&self.cfg, def, side, ctx))
    }

    /// Execute a purchase / sale between the player (gameplay inventory, gold = `it_gold`)
    /// and an NPC inventory. Returns the price.
    pub fn trade(&mut self, npc: ActorId, item: &str, side: TradeSide) -> Result<i32, String> {
        let price = self
            .trade_price(npc, item, side)
            .ok_or("unknown item or no hero")?;
        let key = item.to_ascii_lowercase();
        match side {
            TradeSide::PlayerBuys => {
                let stock = self.actors[npc.0 as usize]
                    .inventory
                    .get(&key)
                    .copied()
                    .unwrap_or(0);
                if stock == 0 {
                    return Err("npc does not have the item".into());
                }
                if self.state.gold() < price as u32 {
                    return Err("not enough gold".into());
                }
                self.state
                    .take_item(gothic_gameplay::state::GOLD_ITEM, price as u32);
                self.state.add_item(item, 1);
                *self.actors[npc.0 as usize].inventory.get_mut(&key).unwrap() -= 1;
            }
            TradeSide::PlayerSells => {
                if !self.state.take_item(item, 1) {
                    return Err("player does not have the item".into());
                }
                self.state
                    .add_item(gothic_gameplay::state::GOLD_ITEM, price as u32);
                *self.actors[npc.0 as usize]
                    .inventory
                    .entry(key)
                    .or_insert(0) += 1;
            }
        }
        Ok(price)
    }

    /// `CanJoinPlayerParty` (S:a80a0): at most one active companion [CA 11.4].
    pub fn join_party(&mut self, npc: ActorId) -> bool {
        let Some(h) = self.hero else { return false };
        let summoned = self.actors[npc.0 as usize].summoned;
        if !summoned {
            let blocked = self.actors.iter().any(|a| {
                a.id != npc
                    && a.party_leader == Some(h)
                    && !a.summoned
                    && a.brain.task == Task::FollowPlayer
            });
            if blocked {
                return false;
            }
            // Waiting companions are dismissed.
            let waiting: Vec<ActorId> = self
                .actors
                .iter()
                .filter(|a| {
                    a.party_leader == Some(h) && !a.summoned && a.brain.task == Task::WaitForPlayer
                })
                .map(|a| a.id)
                .collect();
            for w in waiting {
                self.dismiss_party(w);
            }
        }
        let a = &mut self.actors[npc.0 as usize];
        a.party_leader = Some(h);
        a.brain.task = Task::FollowPlayer;
        let name = a.name.to_ascii_lowercase();
        self.state.party.insert(name);
        self.push(EventKind::PartyJoined { actor: npc });
        true
    }

    pub fn dismiss_party(&mut self, npc: ActorId) {
        let a = &mut self.actors[npc.0 as usize];
        a.party_leader = None;
        a.brain.task = Task::Routine;
        let name = a.name.to_ascii_lowercase();
        self.state.party.remove(&name);
        self.push(EventKind::PartyLeft { actor: npc });
    }

    pub fn wait_party(&mut self, npc: ActorId) {
        self.actors[npc.0 as usize].brain.task = Task::WaitForPlayer;
    }

    // ---- gameplay bridge ---------------------------------------------------------------

    fn resolve_name(&self, name: &str) -> Option<ActorId> {
        match name.trim().to_ascii_lowercase().as_str() {
            "player" | "pc_hero" | "hero" | "other" => self.hero,
            "" | "npc" | "self" | "owner" => self.dialog_partner,
            n => self.find(n),
        }
    }

    /// Feed the outputs of a dialog / quest execution into the simulation: forwarded world
    /// effects are carried out, XP and quest changes are mirrored as events.
    pub fn apply_outputs(&mut self, outs: &[Output]) {
        for o in outs {
            match o {
                Output::Effect {
                    command,
                    entity1,
                    entity2,
                    arg1,
                    arg2,
                } => self.apply_effect(command, entity1, entity2, arg1, arg2),
                Output::Xp {
                    amount,
                    level_before,
                    level_after,
                } => {
                    self.push(EventKind::Xp {
                        amount: u64::from(*amount),
                        level_before: *level_before,
                        level_after: *level_after,
                    });
                    self.refresh_hero();
                }
                Output::QuestChanged { quest, from, to } => self.push(EventKind::Quest {
                    quest: quest.clone(),
                    from: *from,
                    to: *to,
                }),
                _ => {}
            }
        }
    }

    fn apply_effect(&mut self, command: &str, e1: &str, e2: &str, a1: &str, a2: &str) {
        let (r1, r2) = (self.resolve_name(e1), self.resolve_name(e2));
        let now = self.now_ms();
        match command.to_ascii_lowercase().as_str() {
            "attack" => {
                if let (Some(a), Some(b)) = (r1, r2) {
                    let reason = match a1.trim().to_ascii_lowercase().as_str() {
                        "duel" => AttackReason::Duel,
                        "guard" | "gateguard" => AttackReason::GateGuard,
                        "reacttoweapon" => AttackReason::ReactToWeapon,
                        "revolution" => AttackReason::Revolution,
                        "murder" => AttackReason::Murder,
                        _ => AttackReason::Enemy,
                    };
                    self.assess_target(a, b, reason);
                }
            }
            "spawn" => {
                let tmpl = self.npc_templates.get(&a1.to_ascii_lowercase()).cloned();
                if let Some(rec) = tmpl {
                    let at = parse_pos(a2).unwrap_or_default();
                    self.spawn_record(&rec, at);
                } else {
                    self.push(EventKind::Unhandled {
                        command: format!("Spawn {a1} (unknown template)"),
                    });
                }
            }
            "moveto" => {
                if let (Some(a), Some(b)) = (r1, r2)
                    && self.actors[a.0 as usize].ai_mode != AiMode::Dead
                {
                    let p = self.pos(b);
                    self.world.teleport(a, p);
                    self.push(EventKind::Teleported { actor: a });
                }
            }
            "dotrade" | "doloot" => {
                if let Some(n) = r2.or(self.dialog_partner) {
                    self.push(EventKind::TradeOpened { npc: n });
                }
            }
            "excuse" => {
                if let Some(n) = r2.or(self.dialog_partner) {
                    self.excuse(n);
                }
            }
            "pickpocket" => {
                if let Some(n) = r2.or(self.dialog_partner) {
                    self.pickpocket(n);
                }
            }
            "heal" => {
                if let Some(a) = r1 {
                    let max = self.actors[a.0 as usize].hp_max(&self.cfg);
                    self.actors[a.0 as usize].hp = max;
                }
            }
            "wear" => {
                if let Some(a) = r1 {
                    let _ = self.equip(a, a1);
                }
            }
            "fullstop" => {
                if let Some(a) = r1 {
                    self.back_to_routine(a.0 as usize, now);
                }
            }
            "resetcrime" => self.reset_crime(),
            "startrevolution" => {
                if let Some(e) = r1
                    .or(r2)
                    .and_then(|a| self.actors[a.0 as usize].enclave.clone())
                {
                    self.enclaves.entry(e).or_default().status = EnclaveStatus::Attack;
                }
            }
            "joinplayer" => {
                if let Some(a) = r1 {
                    let join = a1.trim().eq_ignore_ascii_case("true")
                        || a1.trim().parse::<i64>().is_ok_and(|v| v != 0);
                    if join {
                        self.join_party(a);
                    } else {
                        self.dismiss_party(a);
                    }
                }
            }
            "joinnpc" => {
                if let Some(a) = r2 {
                    self.join_party(a);
                }
            }
            "humanize" => {
                if let Some(a) = r1 {
                    self.actors[a.0 as usize].immortal = false;
                }
            }
            "sleep" => {
                self.clock.game_ms = self.clock.game_ms.max(self.state.hours * 3_600_000);
            }
            other => self.push(EventKind::Unhandled {
                command: other.to_string(),
            }),
        }
    }

    /// Notify the quest manager that the player entered `area` [GAME_RULES 2.4].
    pub fn enter_area(&mut self, area: &str) {
        if let Some(db) = self.db.clone() {
            let r = gothic_gameplay::script::area_entered(&db, &mut self.state, "PC_Hero", area);
            self.absorb(r);
        }
    }
}

/// Parse `wpos x y z` style position words (`lpos`, `wposg`, `lposg` too) [GAME_RULES 3 Spawn].
fn parse_pos(s: &str) -> Option<Pos> {
    let nums: Vec<f32> = s
        .split_whitespace()
        .filter_map(|w| w.parse::<f32>().ok())
        .collect();
    (nums.len() >= 3).then(|| Pos::new(nums[0], nums[1], nums[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_words() {
        assert_eq!(parse_pos("wpos 1 2 3"), Some(Pos::new(1.0, 2.0, 3.0)));
        assert_eq!(
            parse_pos("lposg 10.5 0 -4"),
            Some(Pos::new(10.5, 0.0, -4.0))
        );
        assert_eq!(parse_pos("wpos 1 2"), None);
    }

    #[test]
    fn durations_are_ordered() {
        assert!(move_duration_ms(Action::QuickAttack) < move_duration_ms(Action::Attack));
        assert!(stagger_ms(HitResult::SitKnockDown) > stagger_ms(HitResult::Stumble));
        assert_eq!(stagger_ms(HitResult::LieDead), 0);
    }
}
