//! Names of the effect-map entries a spell plays. The effect map stores them as
//! `eff_magic_<spell>_<stage>_01`; the stages follow the cast phases of the script
//! (`Phase` 2 load, 1 hold, 4 power, 5 cast) plus the missile's flight and impact.
//! [V] from the 148 `eff_magic_*` entries of `g3.efm`.

/// A stage of a spell's presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpellStage {
    /// Charging (cast phase 2).
    Load,
    /// Effect on the casting hand while charging.
    Hand,
    /// Held ready (cast phase 1).
    Hold,
    /// Fully powered (cast phase 4).
    Power,
    /// The release (cast phase 5).
    Cast,
    /// The missile in flight.
    Projectile,
    /// Where the missile arrives.
    Impact,
    /// On the victim.
    Target,
}

impl SpellStage {
    fn word(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Hand => "hand",
            Self::Hold => "hold",
            Self::Power => "power",
            Self::Cast => "cast",
            Self::Projectile => "projectile",
            Self::Impact => "impact",
            Self::Target => "target",
        }
    }

    /// The stage a script cast phase starts (`0` reset, `3` abort start nothing).
    pub fn of_phase(phase: u8) -> Option<Self> {
        match phase {
            1 => Some(Self::Hold),
            2 => Some(Self::Load),
            4 => Some(Self::Power),
            5 => Some(Self::Cast),
            _ => None,
        }
    }
}

/// `Spl_Fireball` -> `fireball`.
pub fn spell_base(spell: &str) -> String {
    let s = spell.strip_prefix("Spl_").unwrap_or(spell);
    s.to_ascii_lowercase()
}

/// Effect-map name of a spell stage: `eff_magic_fireball_impact_01`.
pub fn effect_name(spell: &str, stage: SpellStage) -> String {
    format!("eff_magic_{}_{}_01", spell_base(spell), stage.word())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_effect_map() {
        assert_eq!(
            effect_name("Spl_Fireball", SpellStage::Impact),
            "eff_magic_fireball_impact_01"
        );
        assert_eq!(
            effect_name("Spl_LightningBolt", SpellStage::Projectile),
            "eff_magic_lightningbolt_projectile_01"
        );
    }

    #[test]
    fn phases_map_to_stages() {
        assert_eq!(SpellStage::of_phase(2), Some(SpellStage::Load));
        assert_eq!(SpellStage::of_phase(5), Some(SpellStage::Cast));
        assert_eq!(SpellStage::of_phase(0), None);
        assert_eq!(SpellStage::of_phase(3), None);
    }
}
