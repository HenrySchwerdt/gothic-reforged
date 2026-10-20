//! Shared enums. Numeric values are the engine's where the docs give them [CA]; the rest
//! carry no discriminant on purpose.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActorId(pub u32);

/// `gEDamageType` [CA 1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DamageType {
    #[default]
    None = 0,
    Impact = 1,
    Blade = 2,
    Missile = 3,
    Fire = 4,
    Ice = 5,
    Lightning = 6,
    Physics = 7,
}

impl DamageType {
    pub fn from_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "impact" => Self::Impact,
            "blade" => Self::Blade,
            "missile" => Self::Missile,
            "fire" => Self::Fire,
            "ice" => Self::Ice,
            "lightning" => Self::Lightning,
            "physics" => Self::Physics,
            _ => Self::None,
        }
    }
    /// Protection attribute tag; Physics and None use Impact [CA 2.7].
    pub fn protection_tag(self) -> &'static str {
        match self {
            Self::Blade => "PROT_BLADE",
            Self::Missile => "PROT_MISSILE",
            Self::Fire => "PROT_FIRE",
            Self::Ice => "PROT_ICE",
            Self::Lightning => "PROT_LIGHTNING",
            Self::Impact | Self::Physics | Self::None => "PROT_IMPACT",
        }
    }
}

macro_rules! species_enum {
    ($($v:ident => $s:literal),* $(,)?) => {
        /// Species used by the rules [CA 2.4, 2.6, 9.2, 10.4]; names are the template species strings.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum Species { $($v,)* Other }
        impl Species {
            pub fn from_name(s: &str) -> Self {
                match s.to_ascii_lowercase().as_str() {
                    $($s => Self::$v,)*
                    _ => Self::Other,
                }
            }
        }
    };
}

species_enum! {
    Human => "human", Skeleton => "skeleton", Zombie => "zombie", Golem => "golem",
    Demon => "demon", Orc => "orc", Goblin => "goblin", Troll => "troll",
    Minecrawler => "minecrawler", Wolf => "wolf", Boar => "boar", Sabertooth => "sabertooth",
    Shadowbeast => "shadowbeast", Ripper => "ripper", Lurker => "lurker", Varan => "varan",
    Snapper => "snapper", Alligator => "alligator", Trex => "trex", FireVaran => "firevaran",
    SwampLurker => "swamplurker", Ogre => "ogre", Stalker => "stalker", Dragon => "dragon",
    Scavenger => "scavenger", Bison => "bison", Rhino => "rhino", Bloodfly => "bloodfly",
    Deer => "deer", Pig => "pig", Cow => "cow", Gargoyle => "gargoyle",
    FireGolem => "firegolem", IceGolem => "icegolem", ScorpionKing => "scorpionking",
    Lizard => "lizard", Fish => "fish", Meatbug => "meatbug", Vulture => "vulture",
    Rabbit => "rabbit", Chicken => "chicken", Molerat => "molerat", Snake => "snake",
    Spider => "spider", Turtle => "turtle", Bird => "bird", Ray => "ray",
    Scorpion => "scorpion", Seagull => "seagull", Walrus => "walrus", Whale => "whale",
    Eagle => "eagle",
}

impl Species {
    /// `IsHumanoid` (S:10f20): Human or Orc [CA 2.6].
    pub fn is_humanoid(self) -> bool {
        matches!(self, Self::Human | Self::Orc)
    }

    /// `IsAmbientCreature` (S:10140) [CA 2.6].
    pub fn is_ambient(self) -> bool {
        use Species::*;
        matches!(
            self,
            Lizard
                | Fish
                | Meatbug
                | Vulture
                | Rabbit
                | Chicken
                | Molerat
                | Snake
                | Spider
                | Turtle
                | Bird
                | Ray
                | Scorpion
                | Seagull
                | Walrus
                | Whale
                | Eagle
        )
    }

    /// Undead (S:112f0) [CA 2.6, I: Skeleton and Zombie].
    pub fn is_undead(self) -> bool {
        matches!(self, Self::Skeleton | Self::Zombie)
    }

    /// `IsEvil` (S:10aa0) [CA 2.6].
    pub fn is_evil(self) -> bool {
        use Species::*;
        self.is_undead()
            || matches!(
                self,
                Golem | Demon | Gargoyle | FireGolem | IceGolem | ScorpionKing
            )
    }

    /// `CanAttack` is false for ambient animals, Deer, Pig, Cow [CA 9.1].
    pub fn can_attack(self) -> bool {
        !(self.is_ambient() || matches!(self, Self::Deer | Self::Pig | Self::Cow))
    }

    /// Game animals for `Perk_Hunter_1` (S:57fc0) [CA 2.5, I on the set].
    pub fn is_game_animal(self) -> bool {
        matches!(
            self,
            Self::Deer | Self::Rabbit | Self::Chicken | Self::Pig | Self::Cow | Self::Seagull
        )
    }

    /// "Other animals" for `Perk_Hunter_2` (S:57bd0) [CA 2.5, I on the set].
    pub fn is_other_animal(self) -> bool {
        use Species::*;
        !self.is_humanoid()
            && !self.is_evil()
            && !self.is_game_animal()
            && !matches!(self, Goblin | Troll | Ogre | Stalker | Dragon | Minecrawler | Other)
    }

    /// Humanoid species list used by the tactic selector [CA 9.3].
    pub fn uses_humanoid_tactics(self) -> bool {
        use Species::*;
        matches!(
            self,
            Human | Skeleton | Zombie | Demon | Orc | Minecrawler | Ogre | ScorpionKing | Stalker
        )
    }
}

/// `gEAction` ids named in the docs [CA 2.3, 9.1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Action {
    #[default]
    None = 0,
    Attack = 1,
    PowerAttack = 2,
    QuickAttack = 3,
    QuickAttackR = 4,
    QuickAttackL = 5,
    SimpleWhirl = 6,
    TurnLeft = 7,
    TurnRight = 8,
    SprintAttack = 9,
    WhirlAttack = 10,
    PierceAttack = 11,
    JumpAttack = 12,
    RamAttack = 13,
    HackAttack = 14,
    FinishingAttack = 15,
    Parade = 16,
    GetUpAttack = 30,
    GetUpParade = 31,
    Shoot = 41,
    Cast = 44,
    Heal = 49,
    Wait = 50,
    JumpBack = 51,
    Fwd = 52,
    Back = 53,
    Left = 54,
    Right = 55,
    Move = 56,
    Jump = 57,
}

impl Action {
    pub fn is_quick(self) -> bool {
        matches!(
            self,
            Self::QuickAttack | Self::QuickAttackR | Self::QuickAttackL
        )
    }
    /// Sprint and power attacks share all later rules [CA 2.1].
    pub fn normalised(self) -> Self {
        if self == Self::SprintAttack {
            Self::PowerAttack
        } else {
            self
        }
    }
    pub fn is_melee_attack(self) -> bool {
        matches!(
            self,
            Self::Attack
                | Self::PowerAttack
                | Self::QuickAttack
                | Self::QuickAttackR
                | Self::QuickAttackL
                | Self::SimpleWhirl
                | Self::SprintAttack
                | Self::WhirlAttack
                | Self::PierceAttack
                | Self::HackAttack
                | Self::GetUpAttack
                | Self::FinishingAttack
        )
    }
}

/// Animation states that matter to the rules [CA 1.1, 5.2, 11.5].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AniState {
    #[default]
    Stand,
    Sneak,
    Parade,
    SitGround,
    SitStool,
    SitBench,
    SitThrone,
    SitKnockDown,
    LieKnockDown,
    Swim,
    Dive,
    Fall,
}

impl AniState {
    pub fn is_sitting(self) -> bool {
        matches!(
            self,
            Self::SitGround | Self::SitStool | Self::SitBench | Self::SitThrone
        )
    }
}

/// `AIMode` values named in the docs (Combat 3, Arena 5, Talk 6, Down 8, Dead 9) [CA 1.1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AiMode {
    #[default]
    Routine,
    Reaction,
    Combat,
    Arena,
    Talk,
    Flee,
    Down,
    Dead,
    Abandoned,
}

/// `gEAttitude` [CA 10.4]: Friendly 1, Neutral 2, Hostile 4, Panic 6 are read from the
/// binary; Angry 3 and Predator 5 are inferred from the enum order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Attitude {
    #[default]
    None = 0,
    Friendly = 1,
    Neutral = 2,
    Angry = 3,
    Hostile = 4,
    Predator = 5,
    Panic = 6,
}

/// `gEPlayerCrime` [CA 10.5]; ordered so `max` keeps the worst.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Crime {
    #[default]
    None = 0,
    Livestock = 1,
    Attack = 2,
    Theft = 3,
    Murder = 4,
}

/// `gEPoliticalAlignment` [CA 10.4].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub enum Alignment {
    #[default]
    None,
    Orc,
    Nrd,
    Reb,
    Mid,
    Ass,
    Nom,
    Out,
    Slave,
    Pirate,
}

impl Alignment {
    pub fn from_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "orc" => Self::Orc,
            "nrd" => Self::Nrd,
            "reb" => Self::Reb,
            "mid" => Self::Mid,
            "ass" => Self::Ass,
            "nom" => Self::Nom,
            "out" => Self::Out,
            "slave" => Self::Slave,
            "pirate" | "pir" => Self::Pirate,
            _ => Self::None,
        }
    }
    /// Tag used by the `pol:<tag>` fame keys of the gameplay state.
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Orc => "orc",
            Self::Nrd => "nrd",
            Self::Reb => "reb",
            Self::Mid => "mid",
            Self::Ass => "ass",
            Self::Nom => "nom",
            Self::Out => "out",
            Self::Slave => "slave",
            Self::Pirate => "pir",
        }
    }
}

/// NPC class (`Mage`, `Paladin` matter for the potion rule and tactics) [CA 9.1, 9.3].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NpcClass {
    #[default]
    None,
    Warrior,
    Ranger,
    Mage,
    Paladin,
}

impl NpcClass {
    pub fn from_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "warrior" => Self::Warrior,
            "ranger" => Self::Ranger,
            "mage" => Self::Mage,
            "paladin" => Self::Paladin,
            _ => Self::None,
        }
    }
}

/// `gENPCType` (`Personal`, `Enclave`, `Political`, `Friend`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NpcType {
    #[default]
    Personal,
    Enclave,
    Political,
    Friend,
}

impl NpcType {
    pub fn from_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "enclave" => Self::Enclave,
            "political" => Self::Political,
            "friend" => Self::Friend,
            _ => Self::Personal,
        }
    }
}

/// `AttackReason` [CA 10.2, 10.4]. Discriminants 2-6, 8-14, 18 are read from the binary;
/// the others are inferred placeholders ordered so a lower value never replaces a higher
/// one while in combat [CA 10.2].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum AttackReason {
    #[default]
    None = 0,
    Livestock = 2,
    StopThief = 3,
    Theft = 4,
    UseInteractObject = 5,
    ReactToWeapon = 6,
    StopFight = 8,
    ReactToDamage = 9,
    GateGuard = 10,
    Intruder = 11,
    Enemy = 12,
    Murder = 13,
    Prey = 14,
    /// I
    Kill = 15,
    /// I
    Duel = 16,
    /// I
    Revolution = 17,
    Arena = 18,
}

impl AttackReason {
    /// `HasAttackReasonToKill` [CA 10.4].
    pub fn to_kill(self, zone_friendly: bool) -> bool {
        match self {
            Self::Enemy | Self::Murder | Self::Revolution | Self::Kill => true,
            Self::GateGuard | Self::Intruder => !zone_friendly,
            _ => false,
        }
    }
    /// `HasAttackReasonToWatch` [CA 10.4].
    pub fn to_watch(self) -> bool {
        matches!(self, Self::Duel | Self::Arena | Self::Kill)
    }
}

/// Status-effect bits [CA 9.7].
pub mod status {
    pub const BURNING: u32 = 0x1;
    pub const FROZEN: u32 = 0x2;
    pub const POISONED: u32 = 0x4;
    pub const DISEASED: u32 = 0x8;
    pub const MISTED: u32 = 0x40;
    pub const INVULNERABLE: u32 = 0x80;
    pub const SOUL_TRAVEL: u32 = 0x200;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn species_sets() {
        assert!(Species::Orc.is_humanoid() && Species::Human.is_humanoid());
        assert!(!Species::Wolf.is_humanoid());
        assert!(Species::Seagull.is_ambient());
        assert!(Species::Skeleton.is_evil() && Species::Golem.is_evil());
        assert!(!Species::Orc.is_evil());
        assert!(!Species::Pig.can_attack() && Species::Wolf.can_attack());
        assert_eq!(Species::from_name("FireGolem"), Species::FireGolem);
        assert_eq!(Species::from_name("nonsense"), Species::Other);
    }

    #[test]
    fn crime_order_keeps_worst() {
        assert_eq!(Crime::Theft.max(Crime::Murder), Crime::Murder);
        assert!(AttackReason::Enemy > AttackReason::Theft);
    }
}
