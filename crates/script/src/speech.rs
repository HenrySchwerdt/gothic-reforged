//! Voice-line choosers: which stock voice line (`SVM`) a character says, and `AssessTalk`.
//!
//! The `StartSay*` family of the script DLL picks a sample id from the situation (attack reason,
//! attitude, party membership, a rotating counter) and hands it to the speech command `FUN_baf0`;
//! this module returns the same ids and queues them as [`AiCommand::SayLine`], which the
//! simulation turns into a "play voice" event for the audio crate (`SVM_<voice>_<id>`).
//!
//! Ported: `StartSayAttackEnd`, `StartSayAttackReason`, `StartSayDefeatComment`,
//! `StartSayConfirm`, `StartSayFollow`, `StartSayHoldingPosition`, `StartSayRefuseTalk`,
//! `StartSayCrimeRefuse`, `StartSayZoneRefuse`, `StartSayAargh`, `StartSayDoesntWork`,
//! `StartSayNothingToGet`, `AssessTalk`, the dialog sequences `_AI_SayBossNews` and
//! `_AI_SayCrimeInvestigation` (as line plans) and `_AI_SayGuardNews` ([I] reconstruction).
//! The three `_AI_Say*` commands themselves are engine instructions (play the talk animation,
//! show the subtitle, wait for the sample); `SayLine` stands for all of them.
//!
//! Open points [U]: the DLL gates every line by a per-category cooldown (`S:bc20`, four
//! thresholds 0.0018 / 0.006 / 0.012 / 0.12 time units picked by a category id the callers pass
//! in a register); the port speaks unconditionally and leaves rate limiting to the audio layer.

use crate::attitude::get_attitude;
use crate::combat::select::npc_weapon_mode;
use crate::enums::{
    AiMode, Alignment, AttackReason, Attitude, NpcType, PartyMemberType, PlayerCrime, Species,
};
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::player::api::VoiceCounter;
use crate::predicates::{is_gate_guard, is_humanoid, is_upset, zone_attitude_to_player};
use crate::routine::state::State;

/// Queue a voice line (`FUN_baf0`, minus the cooldown) and report that it was said.
pub fn say(host: &mut dyn ScriptHost, speaker: EntityId, key: &'static str) -> bool {
    host.queue_ai(speaker, AiCommand::SayLine { key });
    true
}

/// One of three lines chosen by `Random(3)`.
fn pick3(host: &mut dyn ScriptHost, lines: [&'static str; 3]) -> &'static str {
    lines[host.random(3).clamp(0, 2) as usize]
}

/// Port of `IsBig` (S:57de0): trolls, trexes and dragons.
pub fn is_big(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_big");
    let species = host.species(e);
    species == Species::Troll || species == Species::Trex || species.raw() == 52
}

// ---- StartSay* ----------------------------------------------------------------------------------

/// Port of `StartSayAttackEnd` (S:50cf0): the remark after a fight ends. Humanoid opponents that
/// died get "Victory", knocked-down ones a line that depends on why the fight started; monsters
/// get the "MonsterKilled" family.
pub fn start_say_attack_end(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_attack_end");
    if !is_humanoid(host, other) {
        let key = if is_big(host, other) {
            "BigMonsterKilled"
        } else {
            pick3(
                host,
                ["MonsterKilled_00", "MonsterKilled_01", "MonsterKilled_02"],
            )
        };
        return say(host, npc, key);
    }
    let key = match host.ai_mode(other) {
        AiMode::Dead => pick3(host, ["Victory_00", "Victory_01", "Victory_02"]),
        AiMode::Down => match host.attack_reason(npc) {
            AttackReason::Theft => "ThiefDown",
            AttackReason::UseInteractObject => "NeverTouchOurStuffAgain",
            AttackReason::Intruder | AttackReason::GateGuard | AttackReason::LeaveZone => {
                let zone = host.zone(npc);
                if zone.is_some() && zone_attitude_to_player(host, zone) == Attitude::Angry {
                    "AndNowGetOutOfHere"
                } else {
                    "NeverEnterAreaAgain"
                }
            }
            AttackReason::Livestock => "YouLearnedALesson",
            AttackReason::ReactToDamage => "NeverHitMeAgain",
            AttackReason::ReactToWeapon => "YouBetterShouldHaveListened",
            AttackReason::StopFight => "ThereIsNoFightingHere",
            _ => return false,
        },
        _ => return false,
    };
    say(host, npc, key)
}

/// Port of `StartSayAttackReason` (S:512d0): the shout at the start of a fight. An upset NPC
/// always says "IGetYouStill"; otherwise the line follows the attack reason, with separate
/// choices for monsters.
pub fn start_say_attack_reason(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_attack_reason");
    if is_upset(host, npc, other) {
        return say(host, npc, "IGetYouStill");
    }
    let reason = host.attack_reason(npc);
    let key = if !is_humanoid(host, other) {
        match reason {
            AttackReason::ReactToDamage => "YouAskedForIt",
            AttackReason::Enemy
            | AttackReason::Murder
            | AttackReason::GateGuard
            | AttackReason::Intruder
            | AttackReason::Livestock => {
                if is_big(host, other) {
                    "DieBigMonster"
                } else {
                    pick3(host, ["DieMonster_00", "DieMonster_01", "DieMonster_02"])
                }
            }
            _ => return false,
        }
    } else {
        match reason {
            AttackReason::Arena | AttackReason::Duel => "ArenaAttack",
            AttackReason::Enemy | AttackReason::Revolution | AttackReason::Kill => {
                pick3(host, ["DieEnemy_00", "DieEnemy_01", "DieEnemy_02"])
            }
            AttackReason::Murder => "Murder",
            AttackReason::Theft => "DirtyThief",
            AttackReason::UseInteractObject => "HandsOff",
            AttackReason::Intruder => "Intruder",
            AttackReason::GateGuard | AttackReason::ReactToWeapon => "YouAskedForIt",
            AttackReason::LeaveZone => "WhatDidYouDoInThere",
            AttackReason::Livestock => "StopThis",
            AttackReason::StopFight => "WillYouStopFighting",
            AttackReason::ReactToDamage => {
                if npc_weapon_mode(host, npc) == WeaponMode::Melee {
                    "YouDareHitMe"
                } else {
                    "DieEnemy_00"
                }
            }
            _ => return false,
        }
    };
    say(host, npc, key)
}

/// Port of `StartSayDefeatComment` (S:51040): a bystander comments on a knocked-out fighter.
/// Only fights the bystander is not part of get a remark, and then only 30% of the time.
pub fn start_say_defeat_comment(host: &mut dyn ScriptHost, npc: EntityId, loser: EntityId) -> bool {
    gothic_portcov::hit!("start_say_defeat_comment");
    if host.random(10) > 7 {
        return false;
    }
    let attacker = host.current_attacker(loser);
    let key = if attacker.is_none() || attacker == loser {
        // Nobody to blame: a friend's fall is mourned, a stranger's mocked.
        if get_attitude(host, npc, loser) == Attitude::Friendly {
            if get_attitude(host, npc, attacker) == Attitude::Friendly {
                "NotBad"
            } else {
                "FriendLost"
            }
        } else {
            "StupidMotherFucker"
        }
    } else if matches!(
        get_attitude(host, npc, attacker),
        Attitude::Friendly | Attitude::Neutral
    ) {
        "FriendWon"
    } else {
        "NotBad"
    };
    say(host, npc, key)
}

/// Port of `StartSayConfirm` (S:51680): party members have three variants, everybody else "Sure".
pub fn start_say_confirm(host: &mut dyn ScriptHost, npc: EntityId, _other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_confirm");
    let key = if host.has_svm_sample(npc, "NPCParty") {
        pick3(host, ["Confirm_00", "Confirm_01", "Confirm_02"])
    } else {
        "Sure"
    };
    say(host, npc, key)
}

/// Port of `StartSayFollow` (S:518f0): the companion's reaction to "follow me" (only when the
/// player gives the order).
pub fn start_say_follow(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_follow");
    if host.player() != other {
        return false;
    }
    let key = pick3(host, ["CmdFollow_00", "CmdFollow_01", "CmdFollow_02"]);
    say(host, npc, key)
}

/// Port of `StartSayHoldingPosition` (S:517c0): "Sure" for NPCs without party voices and for
/// party members of type 3 (a hired fighter), "PartyWait" for the other companions.
pub fn start_say_holding_position(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    _other: EntityId,
) -> bool {
    gothic_portcov::hit!("start_say_holding_position");
    let sure = !host.has_svm_sample(npc, "NPCParty")
        || host.party_member_type(npc) == PartyMemberType::Slave;
    say(host, npc, if sure { "Sure" } else { "PartyWait" })
}

/// Port of `StartSayAargh` (S:528a0): the pain grunt, rotating `AARGH_00..02`. The first call
/// says `_00`; afterwards the counter moves to 0 or 2 (or 1/2 right after the first) by a coin
/// flip so that the same grunt rarely repeats.
pub fn start_say_aargh(host: &mut dyn ScriptHost, npc: EntityId, _other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_aargh");
    let counter = host.voice_counter(VoiceCounter::Aargh);
    let roll = host.random(100);
    let (key, next) = match counter {
        0 => ("AARGH_00", if roll > 49 { 2 } else { 1 }),
        1 => ("AARGH_01", if roll < 50 { 0 } else { 2 }),
        _ => ("AARGH_02", if roll < 50 { 0 } else { 2 }),
    };
    host.set_voice_counter(VoiceCounter::Aargh, next);
    say(host, npc, key)
}

/// The line a busy NPC gives when it refuses to talk, by what it is doing: the use type of its
/// destination (`S:51a30`, first rotation step).
fn busy_line(use_type: crate::enums::UseType) -> &'static str {
    use crate::enums::UseType as U;
    match use_type {
        U::Anvil | U::Forge => "Smith",
        U::GrindStone => "Grind",
        U::Cauldron => "Cook",
        U::Barbecue => "Barbecue",
        U::Alchemy => "Alchemist",
        U::Bookshelf | U::Bookstand | U::Stoneplate => "Read",
        U::TakeStone | U::DropStone => "SlaveWorker",
        U::PickOre => "Mine",
        U::PickGround | U::DigGround | U::SawLog | U::Repair | U::Lumberjack => "WorkerRefuse",
        U::Field => "Farmer",
        U::Bed | U::SleepGround => "GotoBed",
        U::CleanFloor => "Sweep",
        U::Pace => "Think",
        U::Bard | U::OrcDrum | U::OrcBoulder => "Drummer",
        U::Stool | U::Bench | U::SitGround | U::Stand => "GotoBreak",
        U::Waterpipe => "GotoSmoke",
        U::WaterBarrel => "GotoDrink",
        U::SitCampfire => "Campfire",
        U::Smalltalk => "SmalltalkRefuse",
        U::Preach | U::Pee => "NotNow",
        U::Listener => "Listener",
        U::Guard => "Stone",
        _ => "WhatDoYouWant",
    }
}

/// Advance the shared refuse rotation 0 -> 1 -> 2 -> 0 and return the value used.
fn next_refuse(host: &mut dyn ScriptHost) -> u32 {
    let n = host.voice_counter(VoiceCounter::Refuse);
    host.set_voice_counter(VoiceCounter::Refuse, if n + 1 > 2 { 0 } else { n + 1 });
    n
}

/// Port of `StartSayRefuseTalk` (S:52280): a sleeping NPC says "SleepRefuse"; everybody else
/// rotates through three refusals (what they are busy with, a polite excuse, a brush-off).
/// The second and third steps depend on whether the NPC likes the player [I].
pub fn start_say_refuse_talk(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_refuse_talk");
    if host.ai_mode(npc) == AiMode::Sleep {
        return say(host, npc, "SleepRefuse");
    }
    let friendly = get_attitude(host, npc, other) == Attitude::Friendly;
    let key = match next_refuse(host) {
        0 => {
            let dest = host.destination_point(npc);
            busy_line(host.point_use_type(dest))
        }
        1 => {
            if friendly {
                "SeeYouLater"
            } else {
                "CertainlyBusy"
            }
        }
        _ => {
            if friendly {
                "TakeCare"
            } else {
                "DontDisturbMe"
            }
        }
    };
    say(host, npc, key)
}

/// Port of `StartSayCrimeRefuse` (S:524c0): an NPC that knows about a crime of the player
/// refuses to talk; after a theft the three steps are "DontStealAnything", "TheftRefuse",
/// "GetLost", otherwise "LeaveMeAlone", "AngryRefuse", "GetLost".
pub fn start_say_crime_refuse(host: &mut dyn ScriptHost, npc: EntityId, _other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_crime_refuse");
    let theft = host.last_player_crime(npc) == PlayerCrime::Theft;
    let key = match (next_refuse(host), theft) {
        (0, true) => "DontStealAnything",
        (1, true) => "TheftRefuse",
        (0, false) => "LeaveMeAlone",
        (1, false) => "AngryRefuse",
        _ => "GetLost",
    };
    say(host, npc, key)
}

/// Port of `StartSayZoneRefuse` (S:52640): an NPC in a zone hostile to the player refuses to
/// talk. In a locked-down (`Enclave`-level) zone the first step names the owning enclave
/// ("NoFriendOfORC", ..., "No" for none), in a plain zone it is "NoFriendOfEnclave"; then
/// "DontDisturbMe" and "GetLost".
pub fn start_say_zone_refuse(host: &mut dyn ScriptHost, npc: EntityId, _other: EntityId) -> bool {
    gothic_portcov::hit!("start_say_zone_refuse");
    use crate::enums::SecurityLevel;
    let zone = host.zone(npc);
    let strict = zone.is_some() && host.zone_security(zone) == SecurityLevel::Enclave;
    let key = match next_refuse(host) {
        0 if strict => {
            let owner = host.zone_owner(zone);
            match host.enclave_alignment(owner) {
                Alignment::Orc => "NoFriendOfORC",
                Alignment::Nrd => "NoFriendOfNRD",
                Alignment::Reb => "NoFriendOfREB",
                Alignment::Mid => "NoFriendOfMID",
                Alignment::Ass => "NoFriendOfASS",
                Alignment::Nom => "NoFriendOfNOM",
                _ => "No",
            }
        }
        0 => "NoFriendOfEnclave",
        1 => "DontDisturbMe",
        _ => "GetLost",
    };
    say(host, npc, key)
}

/// Port of `StartSayDoesntWork` (S:6f660): the player's grunt when an object cannot be used.
pub fn start_say_doesnt_work(host: &mut dyn ScriptHost, speaker: EntityId) -> bool {
    gothic_portcov::hit!("start_say_doesnt_work");
    let key = pick3(host, ["DoesntWork_00", "DoesntWork_01", "DoesntWork_02"]);
    say(host, speaker, key)
}

/// Port of `StartSayNothingToGet` (S:6f560): the player's comment on an emptied container.
pub fn start_say_nothing_to_get(host: &mut dyn ScriptHost, speaker: EntityId) -> bool {
    gothic_portcov::hit!("start_say_nothing_to_get");
    let key = pick3(
        host,
        ["NothingToGet_00", "NothingToGet_01", "NothingToGet_02"],
    );
    say(host, speaker, key)
}

// ---- AssessTalk ---------------------------------------------------------------------------------

/// Port of `HasCrimeRefuse` (S:bd1e0): an enclave member of the personal type that knows about a
/// crime of the player will not talk.
pub fn has_crime_refuse(host: &dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("has_crime_refuse");
    host.enclave(npc).is_some()
        && host.npc_type(npc) == NpcType::Personal
        && host.last_player_crime(npc) != PlayerCrime::None
}

/// Outcome of [`assess_talk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkOutcome {
    /// The NPC takes the talk: `ZS_Talk` was set for it (and its party).
    Talk,
    /// The NPC refused; the refusal line was queued when the player started the talk.
    Refused,
    /// Nothing happened (the NPC cannot talk right now or is not addressed).
    Ignored,
}

/// Port of `AssessTalk` (S:30d50): somebody addresses `npc`. When the player talks to an NPC
/// that has something to say, both stop and enter `ZS_Talk` (an NPC with party members pulls
/// them in too). Refused are NPCs that are frozen, jumping, dead or unconscious, hostile ones,
/// those that know a crime of the player (`StartSayCrimeRefuse`), those in a zone hostile to the
/// player whose owner the player is at odds with (`StartSayZoneRefuse`) and those busy fighting,
/// fleeing or transformed (nothing is said then). An NPC without a dialog gives the generic
/// refusal (`StartSayRefuseTalk`). `has_info` tells whether the dialog data has an entry for
/// this pair. [I]: the dead/unconscious test uses the AI mode, not the task name.
pub fn assess_talk(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    talker: EntityId,
    has_info: bool,
) -> TalkOutcome {
    gothic_portcov::hit!("assess_talk");
    let player = host.player();
    let player_started = talker == player;
    if host.has_status(npc, crate::damage::STATUS_FROZEN) {
        return TalkOutcome::Ignored;
    }
    let mode = host.ai_mode(npc);
    if matches!(mode, AiMode::Dead | AiMode::Down | AiMode::Abandoned) {
        return TalkOutcome::Ignored;
    }
    if get_attitude(host, npc, talker) == Attitude::Hostile {
        return TalkOutcome::Ignored;
    }
    if has_crime_refuse(host, npc) {
        if player_started {
            start_say_crime_refuse(host, npc, talker);
        }
        return TalkOutcome::Refused;
    }
    let work = host.destination_point(npc);
    if work.is_some() {
        let zone = host.zone(work);
        if zone.is_some() && zone_attitude_to_player(host, zone) == Attitude::Angry {
            let owner = host.zone_owner(zone);
            if owner.is_some()
                && crate::predicates::political_attitude(host, npc, owner) == Attitude::Friendly
            {
                if player_started {
                    start_say_zone_refuse(host, npc, talker);
                }
                return TalkOutcome::Refused;
            }
        }
    }
    let busy = matches!(mode, AiMode::Combat | AiMode::Flee | AiMode::Arena)
        || host.is_transformed(npc)
        || (mode == AiMode::Talk && host.destination_point(npc).is_none())
        || host.is_swimming(npc);
    if busy {
        return TalkOutcome::Ignored;
    }
    if !has_info {
        if player_started {
            start_say_refuse_talk(host, npc, talker);
        }
        return TalkOutcome::Refused;
    }
    if !player_started && host.guard_point(npc).is_some() && !is_gate_guard(host, npc) {
        return TalkOutcome::Ignored;
    }
    // The NPC and its companions turn to the talker.
    let mut group = vec![npc];
    if host.party_leader(npc) == npc || host.party_leader(npc).is_none() {
        group.extend(host.party_members(npc));
    }
    for member in group {
        if member != npc && !host.has_free_los(member, talker) {
            continue;
        }
        host.full_stop(member);
        host.set_current_target(member, talker);
        host.set_task(member, State::Talk);
    }
    host.full_stop(talker);
    host.set_current_target(talker, npc);
    host.set_task(talker, State::Talk);
    TalkOutcome::Talk
}

// ---- dialog sequences -----------------------------------------------------------------------------

/// One line of a scripted dialog sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsLine {
    /// The player speaks (`false`: the NPC).
    pub player: bool,
    /// Sample id.
    pub key: &'static str,
}

const fn npc_line(key: &'static str) -> NewsLine {
    NewsLine { player: false, key }
}

/// The escalation level of the boss news (`S:3c490`): the political crime count of the NPC's
/// alignment, 1 below two crimes, 2 at two and 3 above.
pub fn boss_news_level(crimes: i32) -> u8 {
    match crimes {
        i32::MIN..=1 => 1,
        2 => 2,
        _ => 3,
    }
}

/// Port of `_AI_SayBossNews` (S:3c490): the line plan of a boss telling the player about the
/// crimes against his faction. After the opener "BadNews" the lines depend on the level; at
/// level 3 the boss declares revolution: the plan's last element tells the caller to set the
/// political player-crime flag, notify the enclave (mode 1) and attack with reason Revolution.
pub fn boss_news_plan(crimes: i32) -> (Vec<NewsLine>, bool) {
    gothic_portcov::hit!("boss_news_plan");
    let level = boss_news_level(crimes);
    let mut lines = vec![npc_line("BadNews")];
    match level {
        1 => lines.extend([npc_line("FirstEnclaveCrime"), npc_line("WillKillSuspect")]),
        2 => lines.extend([
            npc_line("SecondEnclaveCrime"),
            npc_line("WillKillAllSuspects"),
            npc_line("WhatDoYouMean"),
            npc_line("YouKnowTheAnswer"),
        ]),
        _ => lines.extend([
            npc_line("WillTakeNoMoreRisks"),
            npc_line("KillAllSuspectsNow"),
        ]),
    }
    (lines, level == 3)
}

/// Execute the closing step of the boss news: record the suspect comment count for the boss's
/// alignment and, at level 3, start the revolution.
pub fn boss_news_finish(host: &mut dyn ScriptHost, boss: EntityId) {
    let alignment = host.alignment(boss);
    let crimes = host.political_crime_count(alignment);
    let level = boss_news_level(crimes);
    for line in boss_news_plan(crimes).0 {
        host.queue_ai(boss, AiCommand::SayLine { key: line.key });
    }
    if level == 3 {
        host.set_political_crime_count(alignment, crimes);
        let enclave = host.enclave(boss);
        if enclave.is_some() {
            crate::crime::notify_enclave(host, boss, 1);
        }
        let player = host.player();
        crate::perception::assess_target(host, boss, player, AttackReason::Revolution);
    }
}

/// Port of `_AI_SayCrimeInvestigation` (S:3be50): the question an investigator asks and the
/// answer. `crime` 1 is a murder, 2 a theft; `step` 1 asks, 2 accuses. The follow-up (reached
/// when the player had something to say) is "No" after the first question and "Interrupted"
/// after the accusation [I].
pub fn crime_investigation_plan(crime: u32, step: u32) -> Vec<NewsLine> {
    gothic_portcov::hit!("crime_investigation_plan");
    let first = match (crime, step) {
        (1, 1) => "DoYouKnowSomething",
        (1, 2) => "BetYouAreResponsible",
        (2, 1) => "DoYouKnowSomethingOfThisThen",
        (2, 2) => "BetYouAreResponsibleForThatToo",
        _ => return Vec::new(),
    };
    let reply = if step == 1 { "No" } else { "Interrupted" };
    vec![
        npc_line(first),
        NewsLine {
            player: true,
            key: reply,
        },
    ]
}

/// Port of `_AI_SayGuardNews` (S:3a3c0) as a line plan [I]. The guard stops the player
/// ("Halt"), names the crime he knows (`murders`/`thefts` counts of the enclave: "KnownMurder"
/// or "KnownTheft"), describes the first or second case ("FirstMurder", "SecondMurder",
/// "FirstTheft", "SecondTheft", with "ThereIsMore" between two crime kinds) and ends with the
/// verdict "WeObserveYou". The answer options are the dialog keys `DIA_BRIBE` (a light crime,
/// first offence), `DIA_EXCUSE` and `DIA_PROVOKE`. Returns the lines and the option keys.
pub fn guard_news_plan(murders: i32, thefts: i32) -> (Vec<NewsLine>, Vec<&'static str>) {
    gothic_portcov::hit!("guard_news_plan");
    let mut lines = vec![npc_line("Halt")];
    if murders > 0 {
        lines.push(npc_line("KnownMurder"));
        lines.push(npc_line(if murders > 1 {
            "SecondMurder"
        } else {
            "FirstMurder"
        }));
        if thefts > 0 {
            lines.push(npc_line("ThereIsMore"));
        }
    }
    if thefts > 0 {
        if murders == 0 {
            lines.push(npc_line("KnownTheft"));
        }
        lines.push(npc_line(if thefts > 1 {
            "SecondTheft"
        } else {
            "FirstTheft"
        }));
    }
    if murders == 0 && thefts == 0 {
        return (lines, Vec::new());
    }
    lines.push(npc_line("WeObserveYou"));
    let options = if murders > 1 || thefts > 1 || murders > 0 {
        vec!["DIA_EXCUSE", "DIA_PROVOKE"]
    } else {
        vec!["DIA_BRIBE", "DIA_EXCUSE", "DIA_PROVOKE"]
    };
    (lines, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::AiMode;
    use crate::host::{EntityApi, NpcApi};
    use crate::player::api::PlayerApi;
    use crate::testhost::TestWorld;

    fn keys(w: &mut TestWorld, npc: EntityId) -> Vec<&'static str> {
        w.finish_commands(npc)
            .into_iter()
            .filter_map(|c| match c {
                AiCommand::SayLine { key } => Some(key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn attack_end_after_a_kill_and_a_knockdown() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guard", Species::Human, 10, 10);
        let foe = w.spawn_npc("Thief", Species::Human, 10, 10);
        w.set_ai_mode(foe, AiMode::Down);
        w.set_attack_reason(npc, AttackReason::Theft);
        assert!(start_say_attack_end(&mut w, npc, foe));
        assert_eq!(keys(&mut w, npc), vec!["ThiefDown"]);
        w.set_ai_mode(foe, AiMode::Dead);
        assert!(start_say_attack_end(&mut w, npc, foe));
        assert!(keys(&mut w, npc)[0].starts_with("Victory_"));
        // Wanting nothing for an unknown reason.
        w.set_ai_mode(foe, AiMode::Down);
        w.set_attack_reason(npc, AttackReason::Enemy);
        assert!(!start_say_attack_end(&mut w, npc, foe));
    }

    #[test]
    fn monsters_get_monster_lines() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Hunter", Species::Human, 10, 10);
        let troll = w.spawn_npc("Troll", Species::Troll, 10, 10);
        assert!(start_say_attack_end(&mut w, npc, troll));
        assert_eq!(keys(&mut w, npc), vec!["BigMonsterKilled"]);
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 10, 10);
        assert!(start_say_attack_end(&mut w, npc, wolf));
        assert!(keys(&mut w, npc)[0].starts_with("MonsterKilled_"));
    }

    #[test]
    fn attack_reason_picks_the_line() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guard", Species::Human, 10, 10);
        let foe = w.spawn_npc("Thief", Species::Human, 10, 10);
        for (reason, expect) in [
            (AttackReason::Theft, "DirtyThief"),
            (AttackReason::Murder, "Murder"),
            (AttackReason::UseInteractObject, "HandsOff"),
            (AttackReason::Intruder, "Intruder"),
            (AttackReason::LeaveZone, "WhatDidYouDoInThere"),
            (AttackReason::Arena, "ArenaAttack"),
            (AttackReason::StopFight, "WillYouStopFighting"),
        ] {
            w.set_attack_reason(npc, reason);
            assert!(start_say_attack_reason(&mut w, npc, foe), "{reason:?}");
            assert_eq!(keys(&mut w, npc), vec![expect]);
        }
        w.set_attack_reason(npc, AttackReason::None);
        assert!(!start_say_attack_reason(&mut w, npc, foe));
    }

    #[test]
    fn aargh_never_repeats_the_first_grunt() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Bandit", Species::Human, 10, 10);
        w.rolls = vec![80];
        start_say_aargh(&mut w, npc, EntityId::NONE);
        assert_eq!(keys(&mut w, npc), vec!["AARGH_00"]);
        assert_eq!(w.voice_counter(VoiceCounter::Aargh), 2);
        w.rolls = vec![10];
        start_say_aargh(&mut w, npc, EntityId::NONE);
        assert_eq!(keys(&mut w, npc), vec!["AARGH_02"]);
        assert_eq!(w.voice_counter(VoiceCounter::Aargh), 0);
    }

    #[test]
    fn refusals_rotate() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Citizen", Species::Human, 10, 10);
        let player = w.player();
        w.set_last_player_crime(npc, PlayerCrime::Theft);
        let mut said = Vec::new();
        for _ in 0..4 {
            start_say_crime_refuse(&mut w, npc, player);
            said.extend(keys(&mut w, npc));
        }
        assert_eq!(
            said,
            vec![
                "DontStealAnything",
                "TheftRefuse",
                "GetLost",
                "DontStealAnything"
            ]
        );
        w.set_last_player_crime(npc, PlayerCrime::Attack);
        w.set_voice_counter(VoiceCounter::Refuse, 0);
        start_say_crime_refuse(&mut w, npc, player);
        assert_eq!(keys(&mut w, npc), vec!["LeaveMeAlone"]);
    }

    #[test]
    fn sleeping_npcs_refuse_with_a_yawn() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Sleeper", Species::Human, 10, 10);
        w.set_ai_mode(npc, AiMode::Sleep);
        let p = w.player();
        start_say_refuse_talk(&mut w, npc, p);
        assert_eq!(keys(&mut w, npc), vec!["SleepRefuse"]);
    }

    #[test]
    fn follow_only_answers_the_player() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Companion", Species::Human, 10, 10);
        let other = w.spawn_npc("Other", Species::Human, 10, 10);
        assert!(!start_say_follow(&mut w, npc, other));
        let p = w.player();
        assert!(start_say_follow(&mut w, npc, p));
        assert!(keys(&mut w, npc)[0].starts_with("CmdFollow_"));
    }

    #[test]
    fn talk_is_taken_refused_or_ignored() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Citizen", Species::Human, 10, 10);
        let p = w.player();
        // Normal case: both enter ZS_Talk.
        assert_eq!(assess_talk(&mut w, npc, p, true), TalkOutcome::Talk);
        assert_eq!(w.current_task(npc), Some(State::Talk));
        assert_eq!(w.current_task(p), Some(State::Talk));
        // No dialog: a refusal line.
        let npc2 = w.spawn_npc("Mute", Species::Human, 10, 10);
        assert_eq!(assess_talk(&mut w, npc2, p, false), TalkOutcome::Refused);
        assert_eq!(keys(&mut w, npc2).len(), 1);
        // Dead people do not talk.
        let corpse = w.spawn_npc("Corpse", Species::Human, 10, 10);
        w.set_ai_mode(corpse, AiMode::Dead);
        assert_eq!(assess_talk(&mut w, corpse, p, true), TalkOutcome::Ignored);
    }

    #[test]
    fn crime_knowledge_blocks_talking() {
        let mut w = TestWorld::new();
        let enclave = w.add_enclave(Alignment::Orc);
        let npc = w.spawn_npc("Citizen", Species::Human, 10, 10);
        w.set_enclave(npc, enclave);
        w.set_last_player_crime(npc, PlayerCrime::Attack);
        let p = w.player();
        assert_eq!(assess_talk(&mut w, npc, p, true), TalkOutcome::Refused);
        assert_eq!(keys(&mut w, npc), vec!["LeaveMeAlone"]);
    }

    #[test]
    fn boss_news_escalates_with_the_crime_count() {
        let (l1, rev1) = boss_news_plan(1);
        assert_eq!(
            l1.iter().map(|l| l.key).collect::<Vec<_>>(),
            ["BadNews", "FirstEnclaveCrime", "WillKillSuspect"]
        );
        assert!(!rev1);
        let (l2, _) = boss_news_plan(2);
        assert_eq!(l2.len(), 5);
        let (l3, rev3) = boss_news_plan(3);
        assert_eq!(l3.last().map(|l| l.key), Some("KillAllSuspectsNow"));
        assert!(rev3);
    }

    #[test]
    fn investigation_and_guard_news_plans() {
        let plan = crime_investigation_plan(1, 1);
        assert_eq!(plan[0].key, "DoYouKnowSomething");
        assert!(plan[1].player);
        assert!(crime_investigation_plan(0, 0).is_empty());
        let (lines, options) = guard_news_plan(1, 1);
        let keys: Vec<_> = lines.iter().map(|l| l.key).collect();
        assert_eq!(
            keys,
            [
                "Halt",
                "KnownMurder",
                "FirstMurder",
                "ThereIsMore",
                "FirstTheft",
                "WeObserveYou"
            ]
        );
        assert!(options.contains(&"DIA_PROVOKE"));
        assert!(guard_news_plan(0, 0).1.is_empty());
    }
}
