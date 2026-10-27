//! The hit path of the simulation: `DoLogicalDamage` (S:b4f50) and `AssessHit` (S:2b580) of
//! `gothic-script` run on the actors; the simulation does what the engine does with their result
//! (it subtracts `PSDamageReceiver.DamageAmount` from the hit points and enters the reaction the
//! script chose).
use super::*;
use crate::combat::{AttackKind, Deity, PerceptionKind};
use crate::script_host::SimHost;
use gothic_script::damage::{HitEvent, HitKind, do_logical_damage};
use gothic_script::enums as se;

/// The `gEAction` result of `AssessHit` as a [`HitResult`].
fn result_of(a: se::Action) -> HitResult {
    use se::Action as A;
    match a {
        A::QuickParadeStumble => HitResult::QuickParadeStumble,
        A::ParadeStumble => HitResult::ParadeStumble,
        A::HeavyParadeStumble => HitResult::HeavyParadeStumble,
        A::QuickStumble => HitResult::QuickStumble,
        A::Stumble => HitResult::Stumble,
        A::SitKnockDown => HitResult::SitKnockDown,
        A::LieKnockDown => HitResult::LieKnockDown,
        A::LieKnockOut => HitResult::LieKnockOut,
        A::PierceStumble => HitResult::PierceStumble,
        A::Die | A::LieDead => HitResult::LieDead,
        A::LiePiercedKO => HitResult::LiePiercedKO,
        A::LiePiercedDead => HitResult::LiePiercedDead,
        A::MagicParade => HitResult::MagicParade,
        _ => HitResult::NoReaction,
    }
}

impl<W: WorldQuery> Sim<W> {
    /// Assess one hit through the ported script functions and apply its outcome to the victim.
    pub fn resolve_ported(&mut self, att: ActorId, vic: ActorId, attack: &Attack) -> HitOutcome {
        let ev = {
            let kind = match attack.kind {
                AttackKind::Melee => HitKind::Melee,
                AttackKind::Projectile => HitKind::Projectile,
                AttackKind::Spell if attack.spell_projectile => HitKind::SpellProjectile,
                AttackKind::Spell => HitKind::SpellDirect,
            };
            let mut ev = HitEvent::melee(
                crate::script_host::entity_of(vic),
                crate::script_host::entity_of(att),
                se::DamageType::from_name(&format!("{:?}", attack.damage_type))
                    .unwrap_or(se::DamageType::None),
                attack.amount,
            );
            ev.kind = kind;
            ev.hit_mult = attack.hit_mult;
            ev.mana_used = attack.mana_used;
            ev.path_missile = attack.path_missile;
            ev.head_shot = attack.head_hit;
            ev.entity_name = attack.entity_name.clone();
            ev.spell_deity = attack.deity.map(|d| match d {
                Deity::Adanos => se::SpellDeity::Adanos,
                Deity::Beliar => se::SpellDeity::Beliar,
                Deity::Innos => se::SpellDeity::Innos,
            });
            ev.weapon_quality = attack.quality.max(0) as u32;
            ev.weapon_name = if attack.unarmed {
                "Fist".into()
            } else {
                attack.entity_name.clone()
            };
            ev
        };
        let (result, captured) = {
            self.script.capture_task = Some(None);
            let mut host = SimHost::new(self);
            let r = do_logical_damage(&mut host, &ev);
            let c = host.sim.script.capture_task.take().flatten();
            (r, c)
        };
        let Some((outcome, broadcast)) = result else {
            return HitOutcome::ignored_hit();
        };
        let _ = captured;
        {
            // The reaction the script chose starts on the victim (`_AI_Stumble` family,
            // `_AI_LieKnockDown`, `_AI_Die`).
            use gothic_script::combat::commands as c;
            let mut host = SimHost::new(self);
            let v = crate::script_host::entity_of(vic);
            match outcome.action {
                se::Action::Stumble
                | se::Action::QuickStumble
                | se::Action::ParadeStumble
                | se::Action::QuickParadeStumble
                | se::Action::HeavyParadeStumble
                | se::Action::PierceStumble => c::start_stumble(&mut host, v, outcome.action),
                se::Action::LieKnockDown => c::start_lie_knock_down(&mut host, v),
                se::Action::Die => c::start_die(&mut host, v),
                _ => {}
            }
        }
        let mut out = HitOutcome::ported(result_of(outcome.action), outcome.damage);
        out.parried = out.result.is_parry();
        out.perception = Some(match broadcast {
            gothic_script::damage::Broadcast::Defeat => PerceptionKind::Defeat,
            gothic_script::damage::Broadcast::Murder => PerceptionKind::Murder,
            gothic_script::damage::Broadcast::FightSound => PerceptionKind::FightSound,
        });
        out.returns_to_caller = !out.parried;
        // The engine applies `DamageAmount` to the hit points.
        let dmg = self.script.npc(vic).damage_amount.max(0);
        if dmg > 0 {
            self.actors[vic.0 as usize].hp -= dmg;
        }
        out.hp_lost = dmg;
        out
    }
}
