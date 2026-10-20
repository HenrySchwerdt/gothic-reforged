//! B11: down states: killing and defeating characters, the dead and unconscious states, and fall
//! damage.
//!
//! `Kill` and `Defeat` are the two ways a character leaves the fight: `Kill` sets AI mode
//! `Dead`, `Defeat` sets AI mode `Down` (the victim wakes up later). Both clean up (effects, hand
//! items, party), report to the quest manager, pay experience once and clear the destination.
//! The pure animation states of B11 (stumbles, landings, sliding, swimming) have no game logic
//! beyond what [`land_from_fall`] covers and are left to the animation layer.

use crate::crime::{memorize_player_crime, notify_enclave};
use crate::damage::give_xp;
use crate::enums::{
    Action, AiMode, AniState, AttackReason, LastFight, NpcType, PartyMemberType, PlayerCrime, Slot,
    Species,
};
use crate::host::{AiCommand, EntityId, Pool, ScriptHost};
use crate::predicates::{is_boss, is_droppable_weapon, is_guard};
use crate::routine::select::{continue_routine, set_current_destination_point};
use crate::routine::state::State;
use crate::routine::work::{End, Out, Step};
use crate::stats;
use Step::*;

/// Seconds of unconscious state time between regeneration ticks.
pub const UNCONSCIOUS_TICK_SECS: f32 = 1.0;
/// Hit and stamina points regained per tick while unconscious.
pub const UNCONSCIOUS_REGEN: i32 = 10;
/// Hit points regained per tick after an arena defeat.
pub const UNCONSCIOUS_REGEN_ARENA_HP: i32 = 15;
/// Message kind of the game-over banner.
pub const MSG_GAME_OVER: u32 = 13;
/// Status bit: poisoned (cleared on duels and arena fights).
const STATUS_POISON: u32 = 4;

/// Port of `FUN_10007790` (S:7790): seconds a defeated NPC stays down.
///
/// 5 s for the player and the player's party, 20 s after an arena defeat, 30 s otherwise.
pub fn unconscious_secs(host: &dyn ScriptHost, npc: EntityId) -> f32 {
    let player = host.player();
    if npc == player || host.party_leader(npc) == player {
        5.0
    } else if host.last_player_attack_reason(npc) == AttackReason::Arena {
        20.0
    } else {
        30.0
    }
}

/// Port of `ResetAll` + `ResetInteraction` (S:247f0, S:24c20): drop every effect and interaction
/// of the character. The engine part (animation, collision group, look-at) is the host's
/// `reset_character`; the script-visible part clears the frozen status.
pub fn reset_all(host: &mut dyn ScriptHost, npc: EntityId) {
    host.reset_character(npc);
    host.set_status_bits(npc, crate::damage::STATUS_FROZEN, false);
}

/// Port of `DropHandItems` (S:24d10): droppable weapons in the hands fall to the ground, the
/// hands are emptied.
pub fn drop_hand_items(host: &mut dyn ScriptHost, npc: EntityId) {
    for slot in [Slot::RightHand, Slot::LeftHand] {
        if is_droppable_weapon(host, npc, slot) {
            host.drop_slot(npc, slot);
        }
    }
    host.hold_stacks(npc, -1, -1);
}

/// Port of `RemoveNonCombatItems` (S:24ef0): put away whatever is in the hands unless it is a
/// weapon pose (tool-idle, melee, ranged or magic ready).
pub fn remove_non_combat_items(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    use crate::helpers::{HandClass, hand_class};
    let class = hand_class(
        host.slot_use_type(npc, Slot::LeftHand),
        host.slot_use_type(npc, Slot::RightHand),
    );
    if matches!(
        class,
        HandClass::Idle | HandClass::MeleeReady | HandClass::RangedReady | HandClass::MagicReady
    ) {
        return false;
    }
    host.hold_stacks(npc, -1, -1);
    true
}

/// Hand a dying leader's party to its first remaining member.
fn dissolve_party(host: &mut dyn ScriptHost, leader: EntityId) {
    let members = host.party_members(leader);
    let mut it = members.into_iter();
    if let Some(first) = it.next() {
        host.set_party_leader(first, EntityId::NONE);
        for m in it {
            host.set_party_leader(m, first);
        }
    }
}

/// The scripted retaliation when "Talchef" dies: these mine NPCs take the routine "Mine_Attack".
const TALCHEF_AVENGERS: &[&str] = &[
    "Mine_Warrior_01",
    "Mine_Warrior_02",
    "Mine_Warrior_03",
    "Mine_Elite_01",
    "Mine_Elite_02",
    "Mine_Elite_03",
    "Mine_Scout_01",
    "Mine_Scout_02",
];

/// Does `who` count as "the player's side" for experience: the player or a party member?
fn player_side(host: &dyn ScriptHost, who: EntityId) -> bool {
    let player = host.player();
    who == player || host.party_leader(who) == player
}

/// Port of `Kill` (S:1bc30): `victim` dies, `killer` is the character that did it.
///
/// Stamps the last fight, resets effects and interactions, drops hand items, sets AI mode Dead,
/// dissolves the victim's party, shows the game-over banner for the player (any other victim
/// leaves the combat group), tells the quest manager, applies the Talchef retaliation, pays XP
/// once if the player's side killed it, notifies the enclave and cleans up the loot.
pub fn kill(host: &mut dyn ScriptHost, victim: EntityId, killer: EntityId) -> bool {
    let player = host.player();
    let now = host.playing_time();
    host.set_last_fight_timestamp(victim, now);
    reset_all(host, victim);
    remove_non_combat_items(host, victim);
    drop_hand_items(host, victim);
    host.set_ai_mode(victim, AiMode::Dead);
    host.set_can_be_pushed(victim, false);
    if host.party_leader(victim).is_some() {
        host.set_party_leader(victim, EntityId::NONE);
    }
    dissolve_party(host, victim);
    if victim == player {
        host.print_game_message("GO_GameOver", "", MSG_GAME_OVER);
    } else {
        host.set_combat_state(victim, 0);
    }
    host.on_npc_felled(victim, killer, true);
    if host.name(victim) == "Talchef" {
        for name in TALCHEF_AVENGERS {
            let npc = host.find_npc(name);
            if npc.is_some() {
                host.set_routine_name(npc, "Mine_Attack");
            }
        }
    }
    if player_side(host, killer) && !host.defeated_by_player(victim) {
        give_xp(host, victim, player, -1);
        host.set_defeated_by_player(victim, true);
    }
    notify_enclave(host, victim, 0);
    set_current_destination_point(host, victim, EntityId::NONE);
    clean_up_plunder(host, victim);
    true
}

/// Port of `CleanUpPlunderInv` (S:1c380): a dead Demon keeps nothing; everything but a summoned
/// creature gets its plunder regenerated from the treasure sets.
pub fn clean_up_plunder(host: &mut dyn ScriptHost, npc: EntityId) {
    if host.species(npc) == Species::Demon {
        host.delete_unequipped_stacks(npc);
    }
    if host.party_member_type(npc) != PartyMemberType::Summoned {
        host.generate_plunder(npc);
    }
}

/// Port of `Defeat` (S:27510): `victim` is knocked out by `attacker`.
///
/// Duel, arena, drive-away or reasonless fights end the victim's poisoning. Same cleanup as
/// [`kill`] but AI mode Down. When the player defeats a character outside a duel or arena that is
/// neither guard nor boss, the victim remembers an **Attack** crime. The quest manager is told,
/// the player's side collects the victim's XP once, the victim's fight result against the player
/// becomes `Lost`, its combat state and attack reason clear.
pub fn defeat(host: &mut dyn ScriptHost, victim: EntityId, attacker: EntityId) -> bool {
    let player = host.player();
    let reason = host.attack_reason(victim);
    if matches!(
        reason,
        AttackReason::Duel | AttackReason::Arena | AttackReason::DriveAway | AttackReason::None
    ) {
        host.set_status_bits(victim, STATUS_POISON, false);
    }
    reset_all(host, victim);
    remove_non_combat_items(host, victim);
    drop_hand_items(host, victim);
    host.set_ai_mode(victim, AiMode::Down);
    host.set_can_be_pushed(victim, false);
    if attacker == player
        && !is_guard(host, victim)
        && !is_boss(host, victim)
        && !matches!(reason, AttackReason::Arena | AttackReason::Duel)
    {
        memorize_player_crime(host, victim, attacker, PlayerCrime::Attack);
    }
    host.on_npc_felled(victim, attacker, false);
    if player_side(host, attacker) {
        if !host.defeated_by_player(victim) {
            give_xp(host, victim, player, -1);
            host.set_defeated_by_player(victim, true);
        }
        if !host.is_transformed(victim) {
            host.set_last_fight_vs_player(victim, LastFight::Lost);
        }
    }
    host.set_combat_state(victim, 0);
    set_current_destination_point(host, victim, EntityId::NONE);
    clean_up_plunder(host, victim);
    host.set_attack_reason(victim, AttackReason::None);
    true
}

fn dead_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.ai_mode(npc) == AiMode::Dead {
        host.set_ani_state(npc, AniState::LieDead);
        host.set_state(npc, State::DeadLoop);
        return Out::Leave;
    }
    let attacker = host.current_attacker(npc);
    kill(host, npc, attacker);
    host.queue_ai(npc, AiCommand::AniStateAction(Action::Die));
    Out::Yield
}

fn dead_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    // The corpse settles: once it stops moving it leaves the collision groups of the living.
    if host.state_progress(npc) == 0 {
        host.set_state_progress(npc, 1);
        host.set_collision_group(npc, 0xd);
    }
    Out::Abort
}

fn unconscious_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let attacker = host.current_attacker(npc);
    defeat(host, npc, attacker);
    host.queue_ai(
        npc,
        AiCommand::SaySvm {
            family: "DOWN",
            index: 0,
        },
    );
    host.set_current_action(npc, Action::None);
    host.set_ani_state(npc, AniState::Stand);
    Out::Next
}

fn unconscious_settle(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ani_state(npc, AniState::LieKnockOut);
    host.set_task_time(npc, 0.0);
    host.set_state(npc, State::UnconsciousLoop);
    Out::Leave
}

fn unconscious_tick(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) <= UNCONSCIOUS_TICK_SECS {
        return Out::Abort;
    }
    let hp = if host.last_player_attack_reason(npc) == AttackReason::Arena {
        UNCONSCIOUS_REGEN_ARENA_HP
    } else {
        UNCONSCIOUS_REGEN
    };
    stats::add_hit_points(host, npc, hp);
    stats::add_stamina_points(host, npc, UNCONSCIOUS_REGEN);
    if host.task_time(npc) > unconscious_secs(host, npc) {
        if npc == host.player() {
            continue_routine(host, npc);
            return Out::Leave;
        }
        if !host.in_dialog_with_player(npc) {
            host.set_state(npc, State::UnconsciousEnd);
            return Out::Leave;
        }
    }
    host.set_state_time(npc, 0.0);
    Out::Abort
}

fn unconscious_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    host.set_collision_group(npc, 4);
    let attacker = host.current_attacker(npc);
    let family = if host.random(100) < 50 {
        "OhMyHead"
    } else {
        "NextTimeYoureInForIt"
    };
    host.queue_ai(npc, AiCommand::SaySvm { family, index: 0 });
    host.set_current_target(npc, EntityId::NONE);
    host.set_alternative_target(npc, attacker);
    host.set_task(npc, State::FetchWeapon);
    Out::Leave
}

/// `ZS_Dead` (S:1b770): kill the character (once) and lie down.
pub const DEAD: &[Step] = &[Custom(dead_start), Then(End::State(State::DeadLoop))];
/// `ZS_Dead_Loop` (S:1ba30): the corpse stays.
pub const DEAD_LOOP: &[Step] = &[Custom(dead_loop)];
/// `ZS_Unconscious` (S:26cd0): defeat the character and let it fall.
pub const UNCONSCIOUS: &[Step] = &[Custom(unconscious_start), Custom(unconscious_settle)];
/// `ZS_Unconscious_Loop` (S:26f70): regenerate once per second, wake after
/// [`unconscious_secs`].
pub const UNCONSCIOUS_LOOP: &[Step] = &[Custom(unconscious_tick)];
/// `ZS_Unconscious_End` (S:27360): the woken character grumbles and fetches its weapon.
pub const UNCONSCIOUS_END: &[Step] = &[Custom(unconscious_end)];

/// Port of the fall-damage rule of `OnMovementModeChanged` (S:adbf0): `floor(160 * (t - 0.9)^2)`
/// for a fall of `t` seconds, 0 up to 0.9 s.
pub fn fall_damage(t: f32) -> i32 {
    if t <= 0.9 {
        0
    } else {
        (160.0 * (t - 0.9) * (t - 0.9)).floor() as i32
    }
}

/// Port of the landing rule of `OnMovementModeChanged` (S:adbf0): apply fall damage after `t`
/// seconds in the `ZS_Fall` state.
///
/// Nothing happens for ghosts (status 0x80), the god-mode player and Bloodflies. `Perk_Acrobatic`
/// halves the damage (and the host plays `ZS_Acrobat`). A character that cannot be killed stays
/// at 1 hit point; others at 0 hit points die (`ZS_RagDollDead`). Returns the damage dealt.
pub fn land_from_fall(host: &mut dyn ScriptHost, npc: EntityId, t: f32) -> i32 {
    if host.has_status(npc, 0x80)
        || (npc == host.player() && host.cheat_god())
        || host.species(npc) == Species::Bloodfly
    {
        return 0;
    }
    let mut dmg = fall_damage(t);
    if dmg == 0 {
        return 0;
    }
    if host.is_skill_active(npc, "Perk_Acrobatic") {
        dmg /= 2;
    }
    let hp = stats::pool(host, npc, Pool::Hit);
    if hp - dmg <= 0 {
        if crate::damage::can_be_killed(host, npc) {
            stats::set_pool(host, npc, Pool::Hit, 0);
            host.full_stop(npc);
            host.set_task(npc, State::RagDollDead);
        } else {
            stats::set_pool(host, npc, Pool::Hit, 1);
        }
    } else {
        stats::add_hit_points(host, npc, -dmg);
    }
    dmg
}

/// Is `npc` a regular victim for the player's XP (not a friend-type)?
pub fn pays_xp(host: &dyn ScriptHost, npc: EntityId) -> bool {
    host.npc_type(npc) != NpcType::Friend
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Alignment, UseType};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::routine::work::run_state;
    use crate::testhost::TestWorld;

    fn world() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let p = w.player();
        let victim = w.spawn_npc("Bandit", Species::Human, 12, 12);
        w.set_alignment(victim, Alignment::Orc);
        (w, p, victim)
    }

    #[test]
    fn kill_cleans_up_and_pays_xp_once() {
        let (mut w, p, v) = world();
        w.set_slot(v, Slot::RightHand, UseType::OneH, 1);
        w.set_leader(v, EntityId::NONE, PartyMemberType::None, false);
        let a = w.spawn_npc("Follower", Species::Human, 5, 5);
        let b = w.spawn_npc("Follower2", Species::Human, 5, 5);
        w.set_leader(a, v, PartyMemberType::Party, false);
        w.set_leader(b, v, PartyMemberType::Party, false);
        w.playing_time = 42.0;
        assert!(kill(&mut w, v, p));
        assert_eq!(w.ai_mode(v), AiMode::Dead);
        assert_eq!(w.last_fight_timestamp(v), 42.0);
        assert_eq!(w.dropped, vec![Slot::RightHand]);
        assert_eq!(w.xp, 120, "10 * level 12");
        assert!(w.defeated_by_player(v));
        assert_eq!(
            w.party_leader(a),
            EntityId::NONE,
            "the first member leads on"
        );
        assert_eq!(w.party_leader(b), a);
        assert_eq!(w.felled, vec![(v, true)]);
        // A second kill pays nothing.
        kill(&mut w, v, p);
        assert_eq!(w.xp, 120);
        // Deaths by other NPCs pay nothing.
        let other = w.spawn_npc("Other", Species::Human, 12, 12);
        kill(&mut w, other, v);
        assert_eq!(w.xp, 120);
        // Party members collect for the player.
        let buddy = w.spawn_npc("Buddy", Species::Human, 5, 5);
        w.set_leader(buddy, p, PartyMemberType::Party, false);
        let third = w.spawn_npc("Third", Species::Human, 12, 12);
        kill(&mut w, third, buddy);
        assert_eq!(w.xp, 240);
    }

    #[test]
    fn the_players_death_shows_game_over() {
        let (mut w, p, v) = world();
        kill(&mut w, p, v);
        assert_eq!(w.messages.last().unwrap().0, "GO_GameOver");
        assert_eq!(w.messages.last().unwrap().2, MSG_GAME_OVER);
    }

    #[test]
    fn talchefs_death_sends_the_mine_to_war() {
        let mut w = TestWorld::new();
        let p = w.player();
        let boss = w.spawn_npc("Talchef", Species::Human, 20, 20);
        let guard = w.spawn_npc("Mine_Warrior_01", Species::Human, 5, 5);
        kill(&mut w, boss, p);
        assert_eq!(w.routine_of(guard), "Mine_Attack");
    }

    #[test]
    fn demons_lose_their_inventory_and_others_get_plunder() {
        let (mut w, p, _) = world();
        let demon = w.spawn_npc("Demon", Species::Demon, 30, 30);
        kill(&mut w, demon, p);
        assert!(w.stacks_deleted.contains(&demon));
        assert!(w.plundered.contains(&demon));
        let summon = w.spawn_npc("Sum", Species::Skeleton, 5, 5);
        w.set_leader(summon, EntityId::NONE, PartyMemberType::Summoned, false);
        kill(&mut w, summon, p);
        assert!(!w.plundered.contains(&summon));
    }

    #[test]
    fn defeat_by_the_player_is_an_attack_crime() {
        let (mut w, p, v) = world();
        w.set_attack_reason(v, AttackReason::Enemy);
        w.set_status(v, STATUS_POISON);
        assert!(defeat(&mut w, v, p));
        assert_eq!(w.ai_mode(v), AiMode::Down);
        assert_eq!(w.last_player_crime(v), PlayerCrime::Attack);
        assert_eq!(w.last_fight_vs_player(v), LastFight::Lost);
        assert_eq!(w.attack_reason(v), AttackReason::None);
        assert!(
            w.has_status(v, STATUS_POISON),
            "enemy fights keep the poison"
        );
        assert_eq!(w.xp, 120);
        assert_eq!(w.felled, vec![(v, false)]);
        // Duels and arena fights are no crime and end poisoning.
        let duelist = w.spawn_npc("Duelist", Species::Human, 5, 5);
        w.set_attack_reason(duelist, AttackReason::Duel);
        w.set_status(duelist, STATUS_POISON);
        defeat(&mut w, duelist, p);
        assert_eq!(w.last_player_crime(duelist), PlayerCrime::None);
        assert!(!w.has_status(duelist, STATUS_POISON));
        // Guards and bosses report to the enclave instead of remembering.
        let enc = w.add_enclave(Alignment::Orc);
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        w.set_alignment(guard, Alignment::Orc);
        w.set_npc_type(guard, NpcType::Enclave);
        w.set_enclave(guard, enc);
        w.set_attack_reason(guard, AttackReason::Enemy);
        defeat(&mut w, guard, p);
        assert_eq!(w.last_player_crime(guard), PlayerCrime::None);
        // NPC against NPC: no crime, no XP.
        let xp = w.xp;
        let other = w.spawn_npc("Other", Species::Human, 5, 5);
        defeat(&mut w, other, v);
        assert_eq!(w.xp, xp);
        assert_eq!(w.last_player_crime(other), PlayerCrime::None);
    }

    #[test]
    fn wake_up_times() {
        let (mut w, p, v) = world();
        assert_eq!(unconscious_secs(&w, p), 5.0);
        assert_eq!(unconscious_secs(&w, v), 30.0);
        w.set_last_player_ar(v, AttackReason::Arena);
        assert_eq!(unconscious_secs(&w, v), 20.0);
        w.set_leader(v, p, PartyMemberType::Party, false);
        assert_eq!(unconscious_secs(&w, v), 5.0);
    }

    #[test]
    fn unconscious_cycle() {
        let (mut w, p, v) = world();
        w.set_current_attacker(v, p);
        w.set_attack_reason(v, AttackReason::Enemy);
        w.set_task_directly(v, State::Unconscious);
        for _ in 0..3 {
            run_state(&mut w, v);
            w.finish_commands(v);
        }
        assert_eq!(w.ai_mode(v), AiMode::Down);
        assert_eq!(w.current_state(v), Some(State::UnconsciousLoop));
        assert_eq!(w.ani_state(v), AniState::LieKnockOut);
        assert!(
            w.log
                .iter()
                .any(|c| matches!(c, AiCommand::SaySvm { family: "DOWN", .. }))
        );
        // Regeneration once per second.
        w.dr_set_pool(v, Pool::Hit, 50);
        w.dr_set_pool(v, Pool::Stamina, 10);
        w.set_state_time(v, 0.5);
        run_state(&mut w, v);
        assert_eq!(w.dr_pool(v, Pool::Hit), 50);
        w.set_state_time(v, 1.5);
        w.set_task_time_value(v, 3.0);
        run_state(&mut w, v);
        assert_eq!(w.dr_pool(v, Pool::Hit), 60);
        assert_eq!(w.dr_pool(v, Pool::Stamina), 20);
        assert_eq!(
            w.current_state(v),
            Some(State::UnconsciousLoop),
            "30 s have not passed"
        );
        // After 30 s the character wakes up and grumbles.
        w.set_state_time(v, 1.5);
        w.set_task_time_value(v, 31.0);
        w.rolls = vec![10];
        run_state(&mut w, v);
        assert_eq!(w.current_state(v), Some(State::UnconsciousEnd));
        run_state(&mut w, v);
        assert_eq!(w.ai_mode(v), AiMode::Reaction);
        assert_eq!(w.current_task(v), Some(State::FetchWeapon));
        assert_eq!(w.alternative_target(v), p);
        assert!(w.log.iter().any(|c| matches!(
            c,
            AiCommand::SaySvm {
                family: "OhMyHead",
                ..
            }
        )));
    }

    #[test]
    fn arena_defeats_regenerate_faster_and_dialog_blocks_waking() {
        let (mut w, p, v) = world();
        w.set_last_player_ar(v, AttackReason::Arena);
        w.set_task_directly(v, State::UnconsciousLoop);
        w.dr_set_pool(v, Pool::Hit, 10);
        w.set_state_time(v, 2.0);
        w.set_task_time_value(v, 25.0);
        w.in_dialog = true;
        run_state(&mut w, v);
        assert_eq!(w.dr_pool(v, Pool::Hit), 25);
        assert_eq!(
            w.current_state(v),
            Some(State::UnconsciousLoop),
            "a conversation keeps it down"
        );
        w.in_dialog = false;
        w.set_state_time(v, 2.0);
        run_state(&mut w, v);
        assert_eq!(w.current_state(v), Some(State::UnconsciousEnd));
        // The player just continues his routine.
        w.set_task_directly(p, State::UnconsciousLoop);
        w.set_state_time(p, 2.0);
        w.set_task_time_value(p, 6.0);
        run_state(&mut w, p);
        assert_ne!(w.current_state(p), Some(State::UnconsciousLoop));
    }

    #[test]
    fn dead_state_kills_once_and_lies_still() {
        let (mut w, p, v) = world();
        w.set_current_attacker(v, p);
        w.set_task_directly(v, State::Dead);
        run_state(&mut w, v);
        assert_eq!(w.ai_mode(v), AiMode::Dead);
        assert!(w.log.contains(&AiCommand::AniStateAction(Action::Die)));
        assert_eq!(w.current_state(v), Some(State::Dead));
        run_state(&mut w, v);
        assert_eq!(w.current_state(v), Some(State::DeadLoop));
        let xp = w.xp;
        // A body that is already dead goes straight to the loop without a second kill.
        w.set_task_directly(v, State::Dead);
        run_state(&mut w, v);
        assert_eq!(w.current_state(v), Some(State::DeadLoop));
        assert_eq!(w.xp, xp);
        assert_eq!(w.ani_state(v), AniState::LieDead);
    }

    #[test]
    fn fall_damage_numbers() {
        assert_eq!(fall_damage(0.5), 0);
        assert_eq!(fall_damage(0.9), 0);
        assert_eq!(fall_damage(1.5), 57);
        assert_eq!(fall_damage(2.0), 193);
    }

    #[test]
    fn landing_hurts_or_kills() {
        let (mut w, _p, v) = world();
        w.dr_set_pool(v, Pool::Hit, 240);
        assert_eq!(land_from_fall(&mut w, v, 1.5), 57);
        assert_eq!(w.dr_pool(v, Pool::Hit), 183);
        w.grant_skill(v, "Perk_Acrobatic");
        assert_eq!(land_from_fall(&mut w, v, 1.5), 28);
        // Lethal falls kill killable characters and spare essential ones.
        let weak = w.spawn_npc("Weak", Species::Human, 1, 1);
        w.dr_set_pool(weak, Pool::Hit, 10);
        land_from_fall(&mut w, weak, 2.0);
        assert_eq!(w.current_task(weak), Some(State::RagDollDead));
        let essential = w.spawn_npc("Essential", Species::Human, 1, 1);
        w.set_immortal(essential, true);
        w.dr_set_pool(essential, Pool::Hit, 10);
        land_from_fall(&mut w, essential, 2.0);
        assert_eq!(w.dr_pool(essential, Pool::Hit), 1);
        // Ghosts and Bloodflies are immune.
        let fly = w.spawn_npc("Fly", Species::Bloodfly, 1, 1);
        assert_eq!(land_from_fall(&mut w, fly, 3.0), 0);
        w.set_status(essential, 0x80);
        assert_eq!(land_from_fall(&mut w, essential, 3.0), 0);
        // God mode.
        let hero = w.player();
        w.god = true;
        assert_eq!(land_from_fall(&mut w, hero, 3.0), 0);
    }

    #[test]
    fn hand_items() {
        let (mut w, _, v) = world();
        w.set_slot(v, Slot::RightHand, UseType::Potion, 1);
        assert!(
            remove_non_combat_items(&mut w, v),
            "a potion pose is put away"
        );
        w.set_slot(v, Slot::RightHand, UseType::OneH, 1);
        assert!(!remove_non_combat_items(&mut w, v), "a drawn sword stays");
        drop_hand_items(&mut w, v);
        assert_eq!(w.dropped, vec![Slot::RightHand]);
        assert!(pays_xp(&w, v));
    }
}
