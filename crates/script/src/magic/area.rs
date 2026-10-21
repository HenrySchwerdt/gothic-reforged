//! B21: the entities and states that spells leave behind.
//!
//! * Summoned area effects (`Smn_*` templates run `ZS_Hailstorm`, `ZS_Firerain`, `ZS_Heatwave`,
//!   `ZS_Frostwave`, `ZS_Meteor`, `ZS_Lightning`, `ZS_IceBomb`, `ZS_Light`, `ZS_Timebubble`):
//!   [`AreaEffect`] holds the state of one and [`AreaEffect::tick`] advances it. The damage ticks
//!   are `MagicAreaDamageTick` ([`area_damage_tick`]); their parameters are the data tables of
//!   the DLL ([`AreaKind::config`]).
//! * Character states: `ZS_Stunned`, `ZS_Freeze`, `ZS_MagicSleep`, `ZS_MagicHypnosis` and the
//!   mist pair `ZS_Mist`/`ZS_Misted` as [`Def`]s run by the common interpreter.

use crate::enums::{Action, AiMode, AniState, DamageType};
use crate::host::{AiCommand, EntityId, Pos, ScriptHost};
use crate::magic::api::{MagicEvent, SpellHit};
use crate::magic::effects::{STATUS_CONTROLLED, STATUS_FROZEN, STUN_RADIUS, spell_duration_bonus};
use crate::player::step;
use crate::routine::select::continue_routine;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};

/// The kinds of summoned area effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AreaKind {
    /// `ZS_Hailstorm`: lightning rain that follows the caster.
    Hailstorm,
    /// `ZS_Firerain`: fire rain.
    Firerain,
    /// `ZS_Heatwave`: an expanding fire wave.
    Heatwave,
    /// `ZS_Frostwave`: an expanding ice wave.
    Frostwave,
    /// `ZS_Meteor`: one strong fire blast after a delay.
    Meteor,
    /// `ZS_Lightning`: one strong lightning blast after a delay.
    Lightning,
    /// `ZS_IceBomb`: a missile-type blast.
    IceBomb,
    /// `ZS_Light`: a floating light.
    Light,
    /// `ZS_Timebubble`: the time bubble.
    Timebubble,
    /// `ZS_Mist`: a mist that makes the friends of the caster follow and the rest attack him.
    Mist,
    /// `ZS_Mist_NoAggro`: the same mist, but strangers are left alone.
    MistNoAggro,
}

/// How an area effect deals damage (the data table behind `MagicAreaDamageTick`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaConfig {
    /// Damage type.
    pub damage_type: DamageType,
    /// A wave that hits every target once as its front passes (otherwise random rain).
    pub sweep: bool,
    /// Rain: fixed part of the damage.
    pub base: i32,
    /// Rain: random part (`rand(n)`).
    pub rand: i32,
    /// Rain: base hit chance in percent (plus five per candidate, at most 70).
    pub chance: i32,
    /// Sweep: radius in cm.
    pub radius: f32,
    /// Sweep: damage at the centre.
    pub centre: f32,
    /// Sweep: damage at the edge.
    pub edge: f32,
    /// Time before the first tick in ms.
    pub delay_ms: u32,
    /// Time between ticks in ms.
    pub tick_ms: u32,
}

impl AreaKind {
    /// The damage table; `None` for Light and Timebubble, which hurt nobody.
    pub const fn config(self) -> Option<AreaConfig> {
        const fn sweep(ty: DamageType, radius: f32, centre: f32, delay_ms: u32) -> AreaConfig {
            AreaConfig {
                damage_type: ty,
                sweep: true,
                base: 0,
                rand: 0,
                chance: 0,
                radius,
                centre,
                edge: 1.0,
                delay_ms,
                tick_ms: 1,
            }
        }
        const fn rain(ty: DamageType) -> AreaConfig {
            AreaConfig {
                damage_type: ty,
                sweep: false,
                base: 50,
                rand: 50,
                chance: 33,
                radius: 1000.0,
                centre: 0.0,
                edge: 0.0,
                delay_ms: 0,
                tick_ms: 400,
            }
        }
        Some(match self {
            Self::Hailstorm => rain(DamageType::Lightning),
            Self::Firerain => rain(DamageType::Fire),
            Self::Heatwave => sweep(DamageType::Fire, 1000.0, 400.0, 0),
            Self::Frostwave => sweep(DamageType::Ice, 1000.0, 200.0, 0),
            Self::Meteor => sweep(DamageType::Fire, 500.0, 1000.0, 500),
            Self::Lightning => sweep(DamageType::Lightning, 500.0, 1000.0, 500),
            Self::IceBomb => sweep(DamageType::Missile, 1000.0, 600.0, 0),
            Self::Light | Self::Timebubble | Self::Mist | Self::MistNoAggro => return None,
        })
    }

    /// The state name of the entity (`ZS_Hailstorm`, ...).
    pub const fn state_name(self) -> &'static str {
        match self {
            Self::Hailstorm => "ZS_Hailstorm",
            Self::Firerain => "ZS_Firerain",
            Self::Heatwave => "ZS_Heatwave",
            Self::Frostwave => "ZS_Frostwave",
            Self::Meteor => "ZS_Meteor",
            Self::Lightning => "ZS_Lightning",
            Self::IceBomb => "ZS_IceBomb",
            Self::Light => "ZS_Light",
            Self::Timebubble => "ZS_Timebubble",
            Self::Mist => "ZS_Mist",
            Self::MistNoAggro => "ZS_Mist_NoAggro",
        }
    }

    /// The effect a summon template produces, from its name (`Smn_Firerain`).
    pub fn from_template(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "smn_hailstorm" => Self::Hailstorm,
            "smn_firerain" => Self::Firerain,
            "smn_heatwave" => Self::Heatwave,
            "smn_frostwave" => Self::Frostwave,
            "smn_meteor" => Self::Meteor,
            "smn_lightning" => Self::Lightning,
            "smn_icebomb" => Self::IceBomb,
            "smn_light" => Self::Light,
            "smn_timebubble" => Self::Timebubble,
            "smn_mist" => Self::Mist,
            "smn_mist_noaggro" => Self::MistNoAggro,
            _ => return None,
        })
    }
}

/// One running area effect (the summoned entity and its state).
#[derive(Debug, Clone, PartialEq)]
pub struct AreaEffect {
    /// Which effect.
    pub kind: AreaKind,
    /// The caster (`PSInteraction.Owner`).
    pub owner: EntityId,
    /// The spell (`PSInteraction.Spell`).
    pub spell: String,
    /// Centre of the effect.
    pub at: Pos,
    /// `PSDamage.ManaUsed`, copied from the caster when the spell was cast.
    pub mana_used: i32,
    /// Total life in ms (zero for the one-shot waves, which end when nothing is left to hit).
    pub duration_ms: u32,
    /// Milliseconds since the start.
    pub age_ms: u32,
    /// Milliseconds until the next tick.
    next_tick_ms: u32,
    /// Sweep front: the distance of the last target hit.
    front: f32,
}

impl AreaEffect {
    /// Start an effect (`ZS_*` entry state). Rain lasts `8 + rand(4)` seconds; Light and
    /// Timebubble `80 + 10 * rand(4)`; the hero's duration grows by the school bonus.
    pub fn new(
        host: &mut dyn ScriptHost,
        kind: AreaKind,
        owner: EntityId,
        spell: &str,
        at: Pos,
        mana_used: i32,
    ) -> Self {
        let roll = host.random(4);
        let secs = match kind {
            AreaKind::Hailstorm | AreaKind::Firerain => 8 + roll,
            AreaKind::Light | AreaKind::Timebubble => 80 + 10 * roll,
            AreaKind::Mist | AreaKind::MistNoAggro => MIST_SECS,
            _ => 0,
        };
        let bonus = spell_duration_bonus(host, owner, spell);
        let duration_ms = (secs as f32 * (1.0 + bonus) * 1000.0) as u32;
        let cfg = kind.config();
        host.emit_magic(MagicEvent::Area {
            state: kind.state_name(),
            owner,
            at,
        });
        Self {
            kind,
            owner,
            spell: spell.into(),
            at,
            mana_used,
            duration_ms,
            age_ms: 0,
            next_tick_ms: cfg.map_or(1000, |c| if c.sweep { c.delay_ms } else { c.tick_ms }),
            front: 0.0,
        }
    }

    /// Advance by `dt_ms`. Returns false once the entity is gone (`ZS_Decay`).
    pub fn tick(&mut self, host: &mut dyn ScriptHost, dt_ms: u32) -> bool {
        self.age_ms += dt_ms;
        if self.kind == AreaKind::Hailstorm && host.exists(self.owner) {
            // The storm follows the caster (`ZS_Hailstorm_FadeIn_Loop` moves it every frame).
            self.at = host.position(self.owner);
        }
        if matches!(self.kind, AreaKind::Mist | AreaKind::MistNoAggro) {
            return self.tick_mist(host, dt_ms);
        }
        let Some(cfg) = self.kind.config() else {
            return self.duration_ms == 0 || self.age_ms < self.duration_ms;
        };
        if !cfg.sweep && self.age_ms >= self.duration_ms {
            return false;
        }
        let mut budget = dt_ms;
        loop {
            if self.next_tick_ms > budget {
                self.next_tick_ms -= budget;
                return true;
            }
            budget -= self.next_tick_ms;
            self.next_tick_ms = cfg.tick_ms;
            let more = area_damage_tick(host, self, &cfg);
            if cfg.sweep {
                // A sweep hits one target per tick; with nobody left beyond the front it is over.
                return more;
            }
        }
    }
}

impl AreaEffect {
    /// `ZS_Mist_Loop` / `ZS_Mist_End`: once a second the mist is tallied (`MagicMistTick`); when it
    /// is over everybody is freed.
    fn tick_mist(&mut self, host: &mut dyn ScriptHost, dt_ms: u32) -> bool {
        if self.age_ms >= self.duration_ms {
            for npc in host.all_npcs() {
                host.set_status_bits(npc, STATUS_CONTROLLED, false);
            }
            return false;
        }
        if self.next_tick_ms > dt_ms {
            self.next_tick_ms -= dt_ms;
            return true;
        }
        self.next_tick_ms = 1000;
        magic_mist_tick(host, self, self.kind == AreaKind::Mist);
        true
    }
}

/// Seconds a mist lasts (20, plus the school bonus).
pub const MIST_SECS: i32 = 20;
/// Radius of the mist in cm (the DLL compares the squared distance with 1,000,000).
pub const MIST_RADIUS: f32 = 1000.0;

/// Port of `AssessMist` (S:5d040) and `AssessMistNoAggro`: a character that can notice the
/// caster and is neither fleeing nor asleep reacts to the mist. One already fighting only stops.
/// Friends of the caster become `ZS_Misted` (they follow him); everybody else attacks him
/// (`ReactToDamage`) unless the mist is the no-aggro kind, which leaves them alone.
pub fn assess_mist(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    caster: EntityId,
    aggressive: bool,
) -> bool {
    gothic_portcov::hit!("assess_mist");
    if !crate::perception::can_receive_perception_from(host, npc, caster, false) {
        return false;
    }
    if matches!(host.ai_mode(npc), AiMode::Flee | AiMode::Sleep) {
        return false;
    }
    if host.ai_mode(npc) == AiMode::Combat {
        host.full_stop(npc);
        return true;
    }
    if crate::attitude::get_attitude(host, npc, caster) == crate::enums::Attitude::Friendly {
        host.full_stop(npc);
        host.set_current_target(npc, caster);
        host.set_task(npc, State::Misted);
    } else if aggressive {
        host.full_stop(npc);
        crate::perception::assess_target(
            host,
            npc,
            caster,
            crate::enums::AttackReason::ReactToDamage,
        );
    }
    true
}

/// Port of `MagicMistTick` (S:5c820) and `MagicMistTickNoAggro`: characters inside the mist that
/// are not yet under it get the status bit and assess the mist; those that left it lose the bit.
pub fn magic_mist_tick(host: &mut dyn ScriptHost, fx: &AreaEffect, aggressive: bool) {
    gothic_portcov::hit!("magic_mist_tick");
    for npc in host.all_npcs() {
        let inside = host.position(npc).distance(fx.at) <= MIST_RADIUS;
        let misted = host.has_status(npc, STATUS_CONTROLLED);
        if inside && !misted {
            assess_mist(host, npc, fx.owner, aggressive);
            host.set_status_bits(npc, STATUS_CONTROLLED, true);
        } else if !inside && misted {
            host.set_status_bits(npc, STATUS_CONTROLLED, false);
        }
    }
}

/// Port of `MagicAreaDamageTick` (S:5a0a0). Candidates are the characters within the radius
/// (nearest first) other than the owner and his party; entities without a character property
/// are skipped. A *sweep* hits the nearest candidate beyond the front and moves the front to it
/// (+5 cm); damage falls linearly from the centre value to the edge value. *Rain* needs a
/// d100 roll of at most `chance + min(5 n, 70)` (n candidates) and then hits one random
/// candidate for `base + rand(rand)`. Returns false when no candidate was left.
pub fn area_damage_tick(host: &mut dyn ScriptHost, fx: &mut AreaEffect, cfg: &AreaConfig) -> bool {
    gothic_portcov::hit!("area_damage_tick");
    let owner = fx.owner;
    let owner_leader = host.party_leader(owner);
    let radius = cfg.radius;
    let mut candidates = Vec::new();
    for e in host.characters_near(fx.at, radius) {
        if e == owner {
            continue;
        }
        let leader = host.party_leader(e);
        if leader.is_some() && (leader == owner || leader == owner_leader) {
            continue;
        }
        let d = host.position(e).distance(fx.at);
        if cfg.sweep && fx.front != 0.0 && d <= fx.front {
            continue;
        }
        candidates.push((e, d));
    }
    if candidates.is_empty() {
        return false;
    }
    let (target, dist, amount) = if cfg.sweep {
        let (e, d) = candidates[0];
        fx.front = d + 5.0;
        let amount = (cfg.centre + (d / radius) * (cfg.edge - cfg.centre)).round() as i32;
        (e, d, amount)
    } else {
        let n = candidates.len() as i32;
        let chance = cfg.chance + (n * 5).min(70);
        if host.random(100) > chance {
            return true;
        }
        let pick = host.random(n).clamp(0, n - 1) as usize;
        let (e, d) = candidates[pick];
        (e, d, cfg.base + host.random(cfg.rand))
    };
    let _ = dist;
    let deity = host
        .spell(&fx.spell)
        .map_or(crate::enums::SpellDeity::None, |s| s.deity);
    host.spell_damage(
        owner,
        target,
        SpellHit {
            spell: fx.spell.clone(),
            damage_type: cfg.damage_type,
            amount,
            mana_used: fx.mana_used,
            deity,
        },
    );
    true
}

// ---- character states -----------------------------------------------------------------------------

/// Seconds the stun lasts (`ZS_Stunned_Loop`).
pub const STUN_SECS: f32 = 8.0;
/// Seconds a magic sleep lasts for an NPC (`ZS_MagicSleep_Loop`).
pub const SLEEP_SECS_NPC: f32 = 30.0;
/// Seconds a magic sleep lasts for the hero.
pub const SLEEP_SECS_PLAYER: f32 = 5.0;
/// Seconds the hypnosis lasts.
pub const HYPNOSIS_SECS: f32 = 60.0;
/// A freeze value of the hit rule is in tenths of a second [I].
pub const FREEZE_UNIT_SECS: f32 = 0.1;

/// Port of `ZS_Stunned` (S:5dd70): the victim stumbles and stands dazed; `ZS_Stunned_Loop` waits
/// out [`STUN_SECS`] and `ZS_Stunned_End` stands up and picks the routine (or the fight) up again.
fn zs_stunned(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_stunned");
    match step(host, npc) {
        0 => {
            host.full_stop(npc);
            host.set_task_time(npc, 0.0);
            host.set_current_action(npc, Action::Stumble);
            host.queue_ai(
                npc,
                AiCommand::Stumble {
                    action: Action::Stumble,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_state(npc, State::StunnedLoop);
            Flow::Done
        }
    }
}

/// Port of `ZS_Stunned_Loop` (S:5e300): wait until the stun is over, then `ZS_Stunned_End`.
fn zs_stunned_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_stunned_loop");
    if host.task_time(npc) > STUN_SECS {
        host.set_state(npc, State::StunnedEnd);
        return Flow::Done;
    }
    host.queue_ai(npc, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `ZS_Stunned_End` (S:5e4d0): stand up; a victim who knows its attacker assesses him
/// (`AssessAgressor`), otherwise the routine continues.
fn zs_stunned_end(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_stunned_end");
    match step(host, npc) {
        0 => {
            host.queue_ai(npc, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            let attacker = host.current_attacker(npc);
            if attacker.is_some() && !matches!(host.ai_mode(attacker), AiMode::Dead) {
                crate::perception::assess_target(
                    host,
                    npc,
                    attacker,
                    crate::enums::AttackReason::ReactToDamage,
                );
            } else {
                continue_routine(host, npc);
            }
            Flow::Done
        }
    }
}

/// Port of `ZS_Freeze` (S:58fd0): the victim stops where it is, its time stands still and the
/// status bit is set (the length was stored in the task position by the hit).
fn zs_freeze(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_freeze");
    host.full_stop(npc);
    host.set_ai_mode(npc, AiMode::Sleep);
    host.set_status_bits(npc, STATUS_FROZEN, true);
    host.emit_magic(MagicEvent::Status {
        target: npc,
        mask: STATUS_FROZEN,
        on: true,
    });
    host.set_task_time(npc, 0.0);
    host.set_state(npc, State::FreezeLoop);
    Flow::Done
}

/// Port of `ZS_Freeze_Loop` (S:59160): when the ice melts the victim thaws and turns on whoever
/// froze it (or resumes its routine).
fn zs_freeze_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_freeze_loop");
    let length = host.task_position(npc) as f32 * FREEZE_UNIT_SECS;
    if host.task_time(npc) < length {
        host.queue_ai(npc, AiCommand::WaitMs { ms: 100 });
        return Flow::Yield;
    }
    host.set_status_bits(npc, STATUS_FROZEN, false);
    host.emit_magic(MagicEvent::Status {
        target: npc,
        mask: STATUS_FROZEN,
        on: false,
    });
    let attacker = host.current_attacker(npc);
    if attacker.is_some() && host.ai_mode(attacker) != AiMode::Dead {
        crate::perception::assess_target(
            host,
            npc,
            attacker,
            crate::enums::AttackReason::ReactToDamage,
        );
    } else {
        host.set_ai_mode(npc, AiMode::Routine);
        continue_routine(host, npc);
    }
    Flow::Done
}

/// Port of `ZS_MagicSleep` (S:59350) and `ZS_MagicHypnosis` (S:599d0): the victim drops
/// everything it fights with, stands, then lies down (`AniStateAction` 0x54) in sleep mode.
fn zs_magic_sleep(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_magic_sleep");
    match step(host, npc) {
        0 => {
            host.set_ai_mode(npc, AiMode::Sleep);
            host.full_stop(npc);
            host.set_current_target(npc, EntityId::NONE);
            host.queue_ai(npc, AiCommand::StandUp);
            Flow::Yield
        }
        1 => {
            host.queue_ai(npc, AiCommand::AniStateAction(Action::SitGround));
            host.set_ani_state(npc, AniState::SleepGround);
            Flow::Yield
        }
        _ => {
            host.set_task_time(npc, 0.0);
            host.set_state_time(npc, 0.0);
            let next = if host.current_task(npc) == Some(State::MagicHypnosis) {
                State::MagicHypnosisLoop
            } else {
                State::MagicSleepLoop
            };
            host.set_state(npc, next);
            Flow::Done
        }
    }
}

/// Port of `ZS_MagicSleep_Loop` (S:59720) and `ZS_MagicHypnosis_Loop` (S:59da0): lie there; after a
/// second the sleeper may wake. A sleeper wakes after 30 s (the hero after 5 s) by the sleep
/// spell, after 60 s from hypnosis; the caster is then its alternative target and it attacks.
fn zs_magic_sleep_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_magic_sleep_loop");
    let hypnosis = host.current_task(npc) == Some(State::MagicHypnosis);
    let limit = if hypnosis {
        HYPNOSIS_SECS
    } else if npc == host.player() {
        SLEEP_SECS_PLAYER
    } else {
        SLEEP_SECS_NPC
    };
    if host.state_time(npc) > 1.0 && host.task_time(npc) > limit {
        host.set_ani_state(npc, AniState::Stand);
        host.set_current_target(npc, EntityId::NONE);
        host.set_ai_mode(npc, AiMode::Routine);
        continue_routine(host, npc);
        return Flow::Done;
    }
    host.queue_ai(npc, AiCommand::WaitMs { ms: 200 });
    Flow::Yield
}

/// Port of `ZS_Misted` (S:5d1e0): a character under the mist goes into reaction mode, stands up
/// and loses its destination; the loop lets it go as soon as the mist bit is gone.
fn zs_misted(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_misted");
    match step(host, npc) {
        0 => {
            host.set_ai_mode(npc, AiMode::Reaction);
            host.queue_ai(npc, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.set_on_destination(npc, false);
            host.set_state(npc, State::MistedLoop);
            Flow::Done
        }
    }
}

/// Port of `ZS_Misted_Loop` (S:5d410): while the 0x40 bit holds the character stands; afterwards the
/// routine continues.
fn zs_misted_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Flow {
    gothic_portcov::hit!("zs_misted_loop");
    if host.has_status(npc, STATUS_CONTROLLED) {
        host.queue_ai(npc, AiCommand::WaitMs { ms: 200 });
        return Flow::Yield;
    }
    continue_routine(host, npc);
    Flow::Done
}

/// Port of `ZS_Decay` (S:5a4d0): the entity plays its end effect and is removed.
fn zs_decay(host: &mut dyn ScriptHost, e: EntityId) -> Flow {
    gothic_portcov::hit!("zs_decay");
    host.remove_entity(e);
    Flow::Done
}

/// Port of `ZS_WaitForMeteor` (S:5a000) and `ZS_WaitForDawn`: the DLL's bodies are empty.
fn zs_nothing(_host: &mut dyn ScriptHost, _e: EntityId) -> Flow {
    gothic_portcov::hit!("zs_nothing");
    Flow::Done
}

/// Definitions of the B21 character states.
pub fn definition(state: State) -> Option<Def> {
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        State::Stunned => custom(zs_stunned),
        State::StunnedLoop => custom(zs_stunned_loop),
        State::StunnedEnd => custom(zs_stunned_end),
        State::Freeze => custom(zs_freeze),
        State::FreezeLoop => custom(zs_freeze_loop),
        State::MagicSleep | State::MagicHypnosis => custom(zs_magic_sleep),
        State::MagicSleepLoop | State::MagicHypnosisLoop => custom(zs_magic_sleep_loop),
        State::Misted => custom(zs_misted),
        State::MistedLoop => custom(zs_misted_loop),
        State::Decay => custom(zs_decay),
        State::WaitForMeteor | State::WaitForDawn => custom(zs_nothing),
        _ => None,
    }
}

/// Radius of the stun for the tests and the HUD.
pub const fn stun_radius() -> f32 {
    STUN_RADIUS
}
