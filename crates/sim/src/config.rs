//! Simulation configuration. Defaults reproduce the shipped `ge3.INI` (Community Patch
//! preset) [CA 0.2]; every "better than the original" deviation is an explicit toggle in
//! [`Variants`] that is off by default.
use serde::{Deserialize, Serialize};

/// `EDifficulty`: 0 easy, 1 normal, 2 hard [CA 2.13].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    /// Level adjustment applied to NPCs by `GetLevel` / `GetLevelMax` [CA 2.13].
    pub fn level_delta(self) -> i32 {
        match self {
            Self::Easy => -10,
            Self::Normal => 0,
            Self::Hard => 10,
        }
    }
}

/// Optional deviations from the original rules. All `false`/`None` = original behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Variants {
    /// The original's `WantToFlee` is a stub returning 0 [CA 9.1]. `Some(p)`: a non-boss NPC
    /// flees when its HP drops below `p` percent and it has no potion.
    pub flee_below_hp_pct: Option<i32>,
    /// The original has no passive HP regeneration without `Perk_MasterGladiator` [CA 5.2].
    /// `Some(n)`: every actor regains `n` HP per tick-second outside combat.
    pub passive_hp_regen: Option<i32>,
    /// NPC melee ignores the weapon and uses STR only [CA 2.5]. `true`: the weapon's damage
    /// is added on top (as the player's is).
    pub npc_weapon_damage_counts: bool,
    /// Parrying needs stamina > 0 (inferred in the base rules, see `combat::can_parade`);
    /// set to `false` to allow parrying with an empty stamina bar.
    pub parry_needs_stamina: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimConfig {
    /// `ExtendedContent` [CA 0.2], shipped `true`.
    pub extended_content: bool,
    /// `AIMode` [CA 0.2], shipped `true`.
    pub ai_mode: bool,
    /// `XPModifier` percent (1..=200) [CA 0.2, CA 3], shipped 100.
    pub xp_modifier: u32,
    /// `MinHitDuration` [CA 0.2], shipped 6.
    pub min_hit_duration: u32,
    pub difficulty: Difficulty,
    pub seed: u64,
    /// Fixed tick length in milliseconds (the sim is stepped, never wall-clock driven).
    pub tick_ms: u32,
    /// Game seconds per real second [CA 11.2].
    pub game_speed: u32,
    /// Game chapter from which Orc and Assassin are friendly [CA 10.4, I].
    pub orc_ass_friendly_from_chapter: u32,
    /// Chance (percent) that an unobstructed ranged shot hits. The original has no roll
    /// (collision/aim driven) [CA 0.1]; this stands in for the missing aiming model [I].
    pub ranged_hit_pct: i32,
    /// Horizontal field of view of NPCs in degrees [I].
    pub fov_degrees: f32,
    pub variants: Variants,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            extended_content: true,
            ai_mode: true,
            xp_modifier: 100,
            min_hit_duration: 6,
            difficulty: Difficulty::Normal,
            seed: 0x474F_5448_4943,
            tick_ms: 50,
            game_speed: 12,
            orc_ass_friendly_from_chapter: 4,
            ranged_hit_pct: 75,
            fov_degrees: 120.0,
            variants: Variants::default(),
        }
    }
}

impl SimConfig {
    /// The pre-patch ruleset (`ExtendedContent=false`, `AIMode=false`).
    pub fn vanilla() -> Self {
        Self {
            extended_content: false,
            ai_mode: false,
            ..Self::default()
        }
    }

    /// `XPModifier` as the engine sanitises it: above 200 resets to 100 [CA 0.2].
    pub fn xp_percent(&self) -> u32 {
        if self.xp_modifier > 200 || self.xp_modifier == 0 {
            100
        } else {
            self.xp_modifier
        }
    }
}
