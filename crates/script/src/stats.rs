//! B01: NPC stat accessors (`GetAttribute.cpp`, 61 registered functions).
//!
//! Storage rule (read from the DLL): the **player** keeps attributes, protections and pools in
//! `PSPlayerMemory`; **every other NPC** (and a transformed player) derives attributes and
//! protections from its level with one shared formula, and the `Set*`/`Add*` forms are no-ops for
//! them. Hit points and stamina of NPCs live in `PSDamageReceiver`; mana is the constant 1000.
//!
//! The 61 functions collapse into table-driven functions over [`Attribute`], [`Protection`] and
//! [`Pool`]; the `Port of` doc lines give the registry name and address of each group member.

use crate::host::{Attribute, EntityId, Pool, Protection, ScriptHost};
use crate::predicates::{is_ambient_creature, is_humanoid};

/// Maximum protection value of any channel.
pub const PROTECTION_CAP: i32 = 80;
/// Mana of every NPC (`GetManaPoints`/`GetManaPointsMax` for non-players).
pub const NPC_MANA: i32 = 1000;

/// True when the entity reads its stats from player memory (the player, not transformed).
fn uses_player_memory(host: &dyn ScriptHost, e: EntityId) -> bool {
    e == host.player() && !host.is_transformed(e)
}

/// Port of `GetLevel` (S:36cb0): the NPC's `Level` property, +10 on hard and -10 on easy
/// (minimum 1). The player is never adjusted.
pub fn level(host: &dyn ScriptHost, e: EntityId) -> u32 {
    adjusted_level(host, e, host.level_raw(e))
}

/// Port of `GetLevelMax` (S:36db0): `LevelMax` with the same difficulty shift as [`level`].
pub fn level_max(host: &dyn ScriptHost, e: EntityId) -> u32 {
    adjusted_level(host, e, host.level_max_raw(e))
}

fn adjusted_level(host: &dyn ScriptHost, e: EntityId, raw: u32) -> u32 {
    if e == host.player() {
        return raw;
    }
    let shifted = i64::from(raw) + i64::from(host.difficulty().level_shift());
    shifted.max(1) as u32
}

/// Port of `GetCurrentLevel` (S:36b40).
///
/// Player: the `Level` property, or `(LevelMax + Level) / 2` (minimum 1) while transformed.
/// Everyone else: `min(GetLevel + Level, GetLevelMax)`. The decompiled code adds the raw `Level`
/// property on top of the difficulty-adjusted level; as `LevelMax` is normally equal to `Level`
/// in the data the cap makes this equal to `GetLevelMax` for stock NPCs [V code, I data].
pub fn current_level(host: &dyn ScriptHost, e: EntityId) -> u32 {
    if e == host.player() {
        if !host.is_transformed(e) {
            return host.level_raw(e);
        }
        let avg = (host.level_max_raw(e) + host.level_raw(e)) >> 1;
        return avg.max(1);
    }
    let raw = if host.variants().current_level_adds_raw {
        host.level_raw(e)
    } else {
        0
    };
    (level(host, e) + raw).min(level_max(host, e))
}

/// Port of `GetDiffToPlayerLevel` (S:36eb0): `CurrentLevel(npc) - player level`, 0 for the player.
pub fn diff_to_player_level(host: &dyn ScriptHost, e: EntityId) -> i32 {
    if e == host.player() {
        return 0;
    }
    current_level(host, e) as i32 - host.player_level() as i32
}

/// Per-attribute data: the factor (in halves of a level) the NPC formula uses.
const fn attribute_halves(attr: Attribute) -> i32 {
    match attr {
        // 10 + trunc(1.5 * level)
        Attribute::Strength | Attribute::Dexterity | Attribute::Intelligence => 3,
        // 10 + trunc(0.5 * level)
        Attribute::Alchemy | Attribute::Smithing | Attribute::Theft => 1,
    }
}

/// Port of `GetStrength` (S:33540), `GetDexterity` (S:33630), `GetIntelligence` (S:33720),
/// `GetAlchemy` (S:33810), `GetSmithing` (S:33900), `GetTheft` (S:339f0).
///
/// Player: the player-memory base value. Otherwise `10 + trunc(f * CurrentLevel)` with
/// `f = 1.5` for STR/DEX/INT and `0.5` for the three skills.
pub fn attribute(host: &dyn ScriptHost, e: EntityId, attr: Attribute) -> i32 {
    if uses_player_memory(host, e) {
        return host.pm_attribute(attr);
    }
    10 + current_level(host, e) as i32 * attribute_halves(attr) / 2
}

/// Port of `SetStrength` (S:33ae0), `SetDexterity` (S:33ba0), `SetIntelligence` (S:33c60),
/// `SetAlchemy` (S:33d20), `SetSmithing` (S:33de0), `SetTheft` (S:33ea0): player only.
pub fn set_attribute(host: &mut dyn ScriptHost, e: EntityId, attr: Attribute, value: i32) -> bool {
    if e == host.player() {
        host.pm_set_attribute(attr, value);
    }
    true
}

/// Port of `AddStrength` (S:33f60), `AddDexterity` (S:34060), `AddIntelligence` (S:34160),
/// `AddAlchemy` (S:34260), `AddSmithing` (S:34360), `AddTheft` (S:34460): adds to the player's
/// base value and prints the gain (`STR`, `DEX`, `INT`, `ALC`, `SMT`, `THF`); no-op for NPCs.
pub fn add_attribute(host: &mut dyn ScriptHost, e: EntityId, attr: Attribute, delta: i32) -> bool {
    if e == host.player() {
        host.pm_add_attribute(attr, delta);
    }
    true
}

/// Port of `GetProtectionBlade` (S:34560), `GetProtectionImpact` (S:34690),
/// `GetProtectionMissile` (S:347c0), `GetProtectionFire` (S:348c0), `GetProtectionIce`
/// (S:34a10), `GetProtectionLightning` (S:34b60).
///
/// Player (untransformed): the player-memory value; Blades and Impact are divided by 5 with
/// `ExtendedContent` (shipped); capped at 80. Fire and Ice add 10 with `Perk_ResistHeat` /
/// `Perk_ResistCold`. NPC or transformed player: `min(80, trunc(1.5 * LevelMax))`.
pub fn protection(host: &dyn ScriptHost, e: EntityId, prot: Protection) -> i32 {
    if !uses_player_memory(host, e) {
        return (level_max(host, e) as i32 * 3 / 2).min(PROTECTION_CAP);
    }
    let raw = host.pm_protection(prot);
    let divided =
        matches!(prot, Protection::Blades | Protection::Impact) && host.setup().extended_content;
    let mut v = if divided { raw / 5 } else { raw }.min(PROTECTION_CAP);
    let perk = match prot {
        Protection::Fire => Some("Perk_ResistHeat"),
        Protection::Ice => Some("Perk_ResistCold"),
        _ => None,
    };
    if let Some(perk) = perk
        && host.is_skill_active(e, perk)
    {
        v += 10;
    }
    v
}

/// Port of `SetProtectionBlades` (S:34c60), `SetProtectionImpact` (S:34d20),
/// `SetProtectionMissile` (S:34de0), `SetProtectionFire` (S:34ea0), `SetProtectionIce` (S:34f60),
/// `SetProtectionLightning` (S:35020): player only.
pub fn set_protection(host: &mut dyn ScriptHost, e: EntityId, prot: Protection, v: i32) -> bool {
    if e == host.player() {
        host.pm_set_protection(prot, v);
    }
    true
}

/// Port of `AddProtectionBlades` (S:350e0), `...Impact` (S:351a0), `...Missile` (S:35260),
/// `...Fire` (S:35320), `...Ice` (S:353e0), `...Lightning` (S:354a0): player only.
pub fn add_protection(host: &mut dyn ScriptHost, e: EntityId, prot: Protection, d: i32) -> bool {
    if e == host.player() {
        host.pm_add_protection(prot, d);
    }
    true
}

/// Port of `GetHitPoints` (S:35560), `GetManaPoints` (S:35d40), `GetStaminaPoints` (S:36340).
///
/// Player memory first, then the damage receiver (hit points and stamina), else 0; mana of a
/// non-player is the constant 1000.
pub fn pool(host: &dyn ScriptHost, e: EntityId, which: Pool) -> i32 {
    if e == host.player() {
        return host.pm_pool(which);
    }
    match which {
        Pool::Mana => NPC_MANA,
        _ if host.has_damage_receiver(e) => host.dr_pool(e, which),
        _ => 0,
    }
}

/// Port of `GetHitPointsMax` (S:35660), `GetManaPointsMax` (S:35e10), `GetStaminaPointsMax`
/// (S:36440).
///
/// NPC hit points: 1 for ambient creatures, else `max(100, 20 * LevelMax)`, 0 without a damage
/// receiver. NPC stamina: `max(100, 10 * LevelMax)` for humanoids, `max(100, 20 * LevelMax)` for
/// other species, 100 without a damage receiver. Mana 1000.
pub fn pool_max(host: &dyn ScriptHost, e: EntityId, which: Pool) -> i32 {
    if which == Pool::Hit && is_ambient_creature(host, e) {
        return 1;
    }
    if e == host.player() {
        return host.pm_pool_max(which);
    }
    match which {
        Pool::Mana => NPC_MANA,
        _ if !host.has_damage_receiver(e) => {
            if which == Pool::Hit {
                0
            } else {
                100
            }
        }
        Pool::Hit => (level_max(host, e) as i32 * 20).max(100),
        Pool::Stamina => {
            let per_level = if is_humanoid(host, e) { 10 } else { 20 };
            (level_max(host, e) as i32 * per_level).max(100)
        }
    }
}

/// Port of `GetHitPointsPercent` (S:357a0), `GetManaPointsPercent` (S:35ee0),
/// `GetStaminaPointsPercent` (S:36560): `100 * value / max`, never negative; 100 for an entity
/// without a damage receiver.
pub fn pool_percent(host: &dyn ScriptHost, e: EntityId, which: Pool) -> i32 {
    if e != host.player() && which != Pool::Mana && !host.has_damage_receiver(e) {
        return 100;
    }
    let (value, max) = if e == host.player() || which == Pool::Mana {
        (pool(host, e, which), pool_max(host, e, which))
    } else {
        (host.dr_pool(e, which), host.dr_pool_max(e, which))
    };
    if max <= 0 {
        return 0;
    }
    (i64::from(value) * 100 / i64::from(max)).max(0) as i32
}

/// Port of `SetHitPoints` (S:358c0), `SetManaPoints` (S:35fc0), `SetStaminaPoints` (S:36680).
///
/// The value is clamped to `0..=max` where max is the damage receiver's (NPC) or player-memory
/// maximum, then written to the NPC receiver and mirrored into player memory for the player.
/// Returns `false` for an entity with neither store (hit points / stamina only).
pub fn set_pool(host: &mut dyn ScriptHost, e: EntityId, which: Pool, value: i32) -> bool {
    let is_player = e == host.player();
    if which == Pool::Mana {
        if is_player {
            let max = host.pm_pool_max(Pool::Mana);
            host.pm_set_pool(Pool::Mana, value.clamp(0, max.max(0)));
        }
        return true;
    }
    let has_dr = host.has_damage_receiver(e);
    if !is_player && !has_dr {
        return false;
    }
    let max = if is_player {
        host.pm_pool_max(which)
    } else {
        host.dr_pool_max(e, which)
    };
    let v = value.clamp(0, max.max(0));
    if has_dr {
        host.dr_set_pool(e, which, v);
    }
    if is_player {
        host.pm_set_pool(which, v);
    }
    true
}

/// Port of `SetHitPointsMax` (S:359f0), `SetManaPointsMax` (S:36080), `SetStaminaPointsMax`
/// (S:367a0): writes the maximum and re-clamps the current value (`HitPointsMax` also floors the
/// current value at 0).
pub fn set_pool_max(host: &mut dyn ScriptHost, e: EntityId, which: Pool, max: i32) -> bool {
    let is_player = e == host.player();
    if which == Pool::Mana {
        if is_player {
            host.pm_set_pool_max(Pool::Mana, max);
        }
        return true;
    }
    let has_dr = host.has_damage_receiver(e);
    if !is_player && !has_dr {
        return false;
    }
    let current = if is_player {
        host.pm_pool(which)
    } else {
        host.dr_pool(e, which)
    };
    let current = current.max(0);
    if has_dr {
        host.dr_set_pool(e, which, current);
        host.dr_set_pool_max(e, which, max);
    }
    if is_player {
        host.pm_set_pool(which, current);
        host.pm_set_pool_max(which, max);
    }
    true
}

/// Port of `AddHitPoints` (S:35b50): `SetHitPoints(GetHitPoints + delta)`. No god-mode rule here.
pub fn add_hit_points(host: &mut dyn ScriptHost, e: EntityId, delta: i32) -> bool {
    let now = pool(host, e, Pool::Hit);
    set_pool(host, e, Pool::Hit, now.saturating_add(delta));
    true
}

/// Port of `AddManaPoints` (S:36140): the player in god mode never loses mana; otherwise the
/// delta goes to player memory (no effect for NPCs, whose mana is constant). Returns whether the
/// change was applied.
pub fn add_mana_points(host: &mut dyn ScriptHost, e: EntityId, delta: i32) -> bool {
    let is_player = e == host.player();
    if is_player && host.cheat_god() && delta < 0 {
        return false;
    }
    if is_player {
        host.pm_add_pool(Pool::Mana, delta);
    }
    true
}

/// Port of `AddStaminaPoints` (S:36900): for the player the call is a no-op in god mode or while
/// the free-action timer runs (`GuardStatus == Active`, COMBAT_AI_RULES 9.5); otherwise
/// `SetStaminaPoints(GetStaminaPoints + delta)`. Returns whether it was applied.
pub fn add_stamina_points(host: &mut dyn ScriptHost, e: EntityId, delta: i32) -> bool {
    use crate::enums::GuardStatus;
    if e == host.player() && (host.cheat_god() || host.guard_status(e) == GuardStatus::Active) {
        return false;
    }
    let now = pool(host, e, Pool::Stamina);
    set_pool(host, e, Pool::Stamina, now.saturating_add(delta));
    true
}

/// Port of `AddHitPointsMax` (S:35c20), `AddManaPointsMax` (S:36260), `AddStaminaPointsMax`
/// (S:36a20): the player's maximum grows (and the gain is printed as `HP`/`MP`/`SP`); no-op for
/// NPCs.
pub fn add_pool_max(host: &mut dyn ScriptHost, e: EntityId, which: Pool, delta: i32) -> bool {
    if e == host.player() {
        host.pm_add_pool_max(which, delta);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{GuardStatus, Species};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi};
    use crate::testhost::TestWorld;

    fn world() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let p = w.player();
        let npc = w.spawn_npc("Bandit", Species::Human, 12, 20);
        (w, p, npc)
    }

    #[test]
    fn npc_attributes_from_level() {
        let (w, _, npc) = world();
        // CurrentLevel = min(12 + 12, 20) = 20 (LevelMax caps the sum).
        assert_eq!(current_level(&w, npc), 20);
        assert_eq!(attribute(&w, npc, Attribute::Strength), 10 + 30);
        assert_eq!(attribute(&w, npc, Attribute::Theft), 10 + 10);
        assert_eq!(protection(&w, npc, Protection::Fire), 30);
        assert_eq!(pool_max(&w, npc, Pool::Hit), 400);
        assert_eq!(pool_max(&w, npc, Pool::Stamina), 200);
        assert_eq!(pool(&w, npc, Pool::Mana), 1000);
    }

    #[test]
    fn difficulty_shifts_level_not_player() {
        let (mut w, p, npc) = world();
        w.difficulty = crate::config::Difficulty::Hard;
        assert_eq!(level(&w, npc), 22);
        assert_eq!(level_max(&w, npc), 30);
        w.difficulty = crate::config::Difficulty::Easy;
        assert_eq!(level(&w, npc), 2);
        let weak = w.spawn_npc("Rat", Species::Molerat, 3, 3);
        assert_eq!(level(&w, weak), 1, "never below 1");
        w.set_player_level(7);
        assert_eq!(level(&w, p), 7);
    }

    #[test]
    fn caps_and_species_rules() {
        let (mut w, _, _) = world();
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 60, 60);
        assert_eq!(protection(&w, wolf, Protection::Blades), 80, "capped at 80");
        assert_eq!(
            pool_max(&w, wolf, Pool::Stamina),
            1200,
            "non-humanoid doubles"
        );
        let bug = w.spawn_npc("Meatbug", Species::Meatbug, 1, 1);
        assert_eq!(
            pool_max(&w, bug, Pool::Hit),
            1,
            "ambient creatures have 1 HP"
        );
    }

    #[test]
    fn player_reads_player_memory_with_extended_content() {
        let (mut w, p, _) = world();
        w.pm.protection.insert(Protection::Blades, 100);
        w.pm.protection.insert(Protection::Missile, 100);
        assert_eq!(protection(&w, p, Protection::Blades), 20, "divided by 5");
        assert_eq!(
            protection(&w, p, Protection::Missile),
            80,
            "not divided, capped"
        );
        w.setup.extended_content = false;
        assert_eq!(protection(&w, p, Protection::Blades), 80);
        w.pm.protection.insert(Protection::Fire, 10);
        assert_eq!(protection(&w, p, Protection::Fire), 10);
        w.grant_skill(p, "Perk_ResistHeat");
        assert_eq!(protection(&w, p, Protection::Fire), 20);
        set_attribute(&mut w, p, Attribute::Strength, 55);
        add_attribute(&mut w, p, Attribute::Strength, 5);
        assert_eq!(attribute(&w, p, Attribute::Strength), 60);
    }

    #[test]
    fn npc_setters_are_noops() {
        let (mut w, _, npc) = world();
        let before = attribute(&w, npc, Attribute::Strength);
        set_attribute(&mut w, npc, Attribute::Strength, 99);
        add_protection(&mut w, npc, Protection::Fire, 50);
        assert_eq!(attribute(&w, npc, Attribute::Strength), before);
        assert_eq!(protection(&w, npc, Protection::Fire), 30);
    }

    #[test]
    fn hit_points_clamp_and_percent() {
        let (mut w, _, npc) = world();
        let max = w.dr_pool_max(npc, Pool::Hit);
        set_pool(&mut w, npc, Pool::Hit, max);
        add_hit_points(&mut w, npc, -100);
        assert_eq!(pool(&w, npc, Pool::Hit), max - 100);
        assert_eq!(pool_percent(&w, npc, Pool::Hit), (max - 100) * 100 / max);
        add_hit_points(&mut w, npc, -10_000);
        assert_eq!(pool(&w, npc, Pool::Hit), 0);
        assert_eq!(pool_percent(&w, npc, Pool::Hit), 0);
        add_hit_points(&mut w, npc, 10_000);
        assert_eq!(pool(&w, npc, Pool::Hit), max);
    }

    #[test]
    fn player_stamina_rules() {
        let (mut w, p, _) = world();
        w.pm.pools.insert(Pool::Stamina, (100, 150));
        w.set_guard_status(p, GuardStatus::Inactive);
        assert!(add_stamina_points(&mut w, p, -30));
        assert_eq!(pool(&w, p, Pool::Stamina), 70);
        w.set_guard_status(p, GuardStatus::Active);
        assert!(!add_stamina_points(&mut w, p, -30), "free-action timer");
        assert_eq!(pool(&w, p, Pool::Stamina), 70);
        w.set_guard_status(p, GuardStatus::Inactive);
        w.god = true;
        assert!(!add_stamina_points(&mut w, p, -30), "god mode");
        assert!(!add_mana_points(&mut w, p, -5), "god mode blocks mana loss");
        assert!(add_mana_points(&mut w, p, 5));
    }

    #[test]
    fn diff_to_player() {
        let (mut w, p, npc) = world();
        w.set_player_level(8);
        assert_eq!(diff_to_player_level(&w, npc), 20 - 8);
        assert_eq!(diff_to_player_level(&w, p), 0);
    }

    #[test]
    fn transformed_player_uses_npc_formulas() {
        let (mut w, p, _) = world();
        w.set_player_level(10);
        w.set_player_level_max(20);
        w.set_transformed(p, true);
        assert_eq!(current_level(&w, p), 15);
        assert_eq!(attribute(&w, p, Attribute::Strength), 10 + 22);
    }
}
