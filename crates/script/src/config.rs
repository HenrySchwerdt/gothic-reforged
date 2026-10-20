//! Engine setup flags and behaviour variants.
//!
//! The shipped `Ini/ge3.INI` (Community Patch Team preset) enables `ExtendedContent` and
//! `AIMode`; the defaults here are those shipped values, and every rule that branches on a flag
//! is a single code path that reads this struct (no duplicated functions).

/// `eSSetupEngine` values the script reads (COMBAT_AI_RULES 0.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSetup {
    /// `ExtendedContent`: switches protection, NPC damage, XP and trade formulas.
    pub extended_content: bool,
    /// `AIMode`: stamina-only parrying and parry restrictions.
    pub ai_mode: bool,
    /// `MinHitDuration` (0..=15).
    pub min_hit_duration: u32,
    /// `XPModifier` percent (1..=200).
    pub xp_modifier: u32,
    /// `QuickLoot`.
    pub quick_loot: bool,
}

impl Default for EngineSetup {
    /// The shipped `ge3.INI` values.
    fn default() -> Self {
        Self {
            extended_content: true,
            ai_mode: true,
            min_hit_duration: 6,
            xp_modifier: 100,
            quick_loot: true,
        }
    }
}

impl EngineSetup {
    /// The vanilla (patch-less) engine defaults when the INI keys are absent.
    pub fn vanilla() -> Self {
        Self {
            extended_content: false,
            ai_mode: false,
            ..Self::default()
        }
    }
}

/// Game difficulty (`EDifficulty`): shifts NPC levels by +-10 (COMBAT_AI_RULES 2.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    /// Level -10 (minimum 1).
    Easy,
    /// No shift.
    #[default]
    Normal,
    /// Level +10.
    Hard,
}

impl Difficulty {
    /// Level offset applied to NPC levels.
    pub const fn level_shift(self) -> i32 {
        match self {
            Self::Easy => -10,
            Self::Normal => 0,
            Self::Hard => 10,
        }
    }
}

/// Named behaviour switches that are *not* shipped-INI values. All default to the shipped game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variants {
    /// The script's debug byte (`DAT_10118aaa` in the DLL): when set every routine action lasts
    /// 10 s instead of its randomised duration. Off in the shipped game.
    pub fast_routines: bool,
    /// `CurrentLevel` adds the raw `Level` property to the difficulty-adjusted `GetLevel` before
    /// capping at `LevelMax` (the reading of the decompiled S:36b40; PORT_NOTES 8.2). Turn off
    /// for `min(GetLevel, LevelMax)`.
    pub current_level_adds_raw: bool,
}

impl Default for Variants {
    fn default() -> Self {
        Self {
            fast_routines: false,
            current_level_adds_raw: true,
        }
    }
}
