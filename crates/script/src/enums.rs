//! Engine enumerations the script layer reads (`gE*` in `Game.dll`). Names and numeric values come
//! from the engine's public reflection data; they are identifiers only. Generated once from
//! the local class-schema dump, then maintained by hand.

/// Declares a `#[repr(u32)]`-style engine enum with raw-value conversion.
macro_rules! engine_enum {
    ($(#[$m:meta])* $name:ident { $($v:ident = $n:expr,)* }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($v,)* }
        impl $name {
            /// Every variant in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$v,)*];
            /// Raw engine value.
            pub const fn raw(self) -> u32 { match self { $($name::$v => $n,)* } }
            /// Convert a raw engine value.
            pub fn from_raw(raw: u32) -> Option<Self> {
                match raw { $($n => Some($name::$v),)* _ => None }
            }
        }
    };
}

engine_enum! {
    /// `gESpecies`: creature kind of an NPC (`PSNpc.Species`).
    Species {
    Human = 0,
    Skeleton = 1,
    Zombie = 2,
    Golem = 3,
    Demon = 4,
    Orc = 5,
    Goblin = 6,
    Troll = 7,
    Minecrawler = 8,
    Scavenger = 9,
    Wolf = 10,
    Boar = 11,
    Sabertooth = 12,
    Shadowbeast = 13,
    Bison = 14,
    Rhino = 15,
    Ripper = 16,
    Lurker = 17,
    Varan = 18,
    Snapper = 19,
    Alligator = 20,
    Trex = 21,
    FireVaran = 22,
    Bloodfly = 23,
    Lizard = 24,
    Fish = 25,
    Meatbug = 26,
    Vulture = 27,
    Rabbit = 28,
    Deer = 29,
    Chicken = 30,
    Molerat = 31,
    Snake = 32,
    Pig = 33,
    Cow = 34,
    Spider = 35,
    Turtle = 36,
    Bird = 37,
    Gargoyle = 38,
    SwampLurker = 39,
    EmptyB = 40,
    Ogre = 41,
    Ray = 42,
    Scorpion = 43,
    Seagull = 44,
    Walrus = 45,
    Whale = 46,
    Eagle = 47,
    FireGolem = 48,
    IceGolem = 49,
    ScorpionKing = 50,
    Stalker = 51,
    Dragon = 52,
    }
}

engine_enum! {
    /// `gEUseType`: what an interact point is used for (item use types and routine work points).
    UseType {
    None = 0,
    Action = 1,
    OneH = 2,
    TwoH = 3,
    Arrow = 4,
    Bow = 5,
    CrossBow = 6,
    Bolt = 7,
    Fist = 8,
    Shield = 9,
    Armor = 10,
    Helmet = 11,
    Staff = 12,
    Amulet = 13,
    Ring = 14,
    Cast = 15,
    Potion = 16,
    Plant = 17,
    Meat = 18,
    Fruit = 19,
    Bread = 20,
    Bottle = 21,
    Cup = 22,
    Bowl = 23,
    Torch = 24,
    Alarmhorn = 25,
    Broom = 26,
    Brush = 27,
    Lute = 28,
    Rake = 29,
    TrophyTeeth = 30,
    Valuable = 31,
    Smoke = 32,
    OrcPipe = 33,
    Scoop = 34,
    Stick = 35,
    Shovel = 36,
    Hammer = 37,
    Fan = 38,
    Pan = 39,
    Saw = 40,
    TrophySkin = 41,
    Map = 42,
    Book = 43,
    Letter = 44,
    Key = 45,
    Lockpick = 46,
    CarryFront = 47,
    CarryShoulder = 48,
    Pickaxe = 49,
    TrophyFur = 50,
    Halberd = 51,
    Axe = 52,
    ItemE = 53,
    Modify = 54,
    PhysicalFist = 55,
    ItemH = 56,
    Anvil = 57,
    Forge = 58,
    GrindStone = 59,
    Cauldron = 60,
    Barbecue = 61,
    Alchemy = 62,
    Bookshelf = 63,
    Bookstand = 64,
    TakeStone = 65,
    DropStone = 66,
    PickOre = 67,
    PickGround = 68,
    DigGround = 69,
    Field = 70,
    Repair = 71,
    SawLog = 72,
    Lumberjack = 73,
    Bed = 74,
    SleepGround = 75,
    CleanFloor = 76,
    Dance = 77,
    FanBoss = 78,
    Boss = 79,
    Throne = 80,
    Pace = 81,
    Bard = 82,
    Stool = 83,
    Bench = 84,
    Waterpipe = 85,
    WaterBarrel = 86,
    PirateTreasure = 87,
    Campfire = 88,
    SitCampfire = 89,
    SitGround = 90,
    Smalltalk = 91,
    Preach = 92,
    Spectator = 93,
    Stand = 94,
    Guard = 95,
    Trader = 96,
    Listener = 97,
    OrcDance = 98,
    Stoneplate = 99,
    OrcDrum = 100,
    Door = 101,
    OrcBoulder = 102,
    EatGround = 103,
    DrinkWater = 104,
    Pee = 105,
    Chest = 106,
    Shrine = 107,
    AttackPoint = 108,
    Roam = 109,
    BodyA = 110,
    Beard = 111,
    Hair = 112,
    Head = 113,
    Body = 114,
    Flee = 115,
    Talk = 116,
    }
}

engine_enum! {
    /// `gEAction`: animation-level action ids passed to `_AI_Action` and stored in `PSRoutine.Action`.
    Action {
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
    ParadeR = 17,
    ParadeL = 18,
    ExitParade = 19,
    QuickParadeStumble = 20,
    ParadeStumble = 21,
    ParadeStumbleR = 22,
    ParadeStumbleL = 23,
    HeavyParadeStumble = 24,
    QuickStumble = 25,
    Stumble = 26,
    StumbleR = 27,
    StumbleL = 28,
    SitKnockDown = 29,
    GetUpAttack = 30,
    GetUpParade = 31,
    LieKnockDown = 32,
    LieKnockOut = 33,
    PierceStumble = 34,
    Die = 35,
    LieDead = 36,
    LiePiercedKO = 37,
    LiePiercedDead = 38,
    AbortAttack = 39,
    Aim = 40,
    Shoot = 41,
    Reload = 42,
    Cock = 43,
    Cast = 44,
    PowerCast = 45,
    MagicParade = 46,
    QuickCast = 47,
    Summon = 48,
    Heal = 49,
    Wait = 50,
    JumpBack = 51,
    Fwd = 52,
    Back = 53,
    Left = 54,
    Right = 55,
    Move = 56,
    Jump = 57,
    Evade = 58,
    Slide = 59,
    Fall = 60,
    Dive = 61,
    CombatActionsEnd = 62,
    Use = 63,
    Equip = 64,
    Reach = 65,
    Take = 66,
    Drop = 67,
    Hold = 68,
    HoldLeft = 69,
    HoldRight = 70,
    Transfer = 71,
    FlameSword = 72,
    Fry = 73,
    Eat = 74,
    Drink = 75,
    Stand = 76,
    Sneak = 77,
    Kneel = 78,
    SitGround = 79,
    SitStool = 80,
    SitBench = 81,
    SitThrone = 82,
    SleepBed = 83,
    SleepGround = 84,
    OrcDrum = 85,
    Waterpipe = 86,
    Anvil = 87,
    OrcAnvil = 88,
    Forge = 89,
    CoolWeapon = 90,
    DrinkWaterBarrel = 91,
    GrindStone = 92,
    Cauldron = 93,
    Barbecue = 94,
    Alchemy = 95,
    Bookstand = 96,
    Stoneplate = 97,
    PickOre = 98,
    OpenChest = 99,
    Lockpick = 100,
    OpenDoor = 101,
    ExitInteract = 102,
    PickGround = 103,
    DigGround = 104,
    Repair = 105,
    SawLog = 106,
    Lumberjack = 107,
    RakeField = 108,
    CleanFloor = 109,
    TiltOrcBoulder = 110,
    HoldOrcBoulder = 111,
    LiftOrcBoulder = 112,
    Fan = 113,
    Dance = 114,
    OrcDance = 115,
    FieldWork = 116,
    EatGround = 117,
    DrinkWater = 118,
    ArmsCrossed = 119,
    HandsOnHips = 120,
    Warn = 121,
    Ambient = 122,
    Think = 123,
    Pee = 124,
    Say = 125,
    Listen = 126,
    WatchFight = 127,
    Cheer = 128,
    SuperCheer = 129,
    Applaud = 130,
    QuickUse0 = 131,
    QuickUse1 = 132,
    QuickUse2 = 133,
    QuickUse3 = 134,
    QuickUse4 = 135,
    QuickUse5 = 136,
    QuickUse6 = 137,
    QuickUse7 = 138,
    QuickUse8 = 139,
    QuickUse9 = 140,
    QuickUseH = 141,
    QuickUseM = 142,
    QuickUseS = 143,
    Count = 145,
    }
}

engine_enum! {
    /// `gEAIMode`: top-level AI mode of an NPC (`PSRoutine.AIMode`).
    AiMode {
    Routine = 0,
    Reaction = 1,
    Combat = 3,
    Flee = 4,
    Arena = 5,
    Talk = 6,
    Down = 8,
    Dead = 9,
    Abandoned = 10,
    PlayerParty = 11,
    }
}

engine_enum! {
    /// `gEAniState`: persistent body pose (`PSRoutine.AniState`).
    AniState {
    Dummy0 = 0,
    Dummy1 = 1,
    Stand = 2,
    Sneak = 3,
    Parade = 4,
    Kneel = 5,
    SitGround = 6,
    SitStool = 7,
    SitBench = 8,
    SitThrone = 9,
    SleepBed = 10,
    SleepGround = 11,
    TiltOrcBoulder = 12,
    HoldOrcBoulder = 13,
    LiftOrcBoulder = 14,
    SitKnockDown = 15,
    LieKnockDown = 16,
    LieKnockOut = 17,
    LieStraightDead = 18,
    LieDead = 19,
    LiePiercedKO = 20,
    LiePiercedDead = 21,
    TalkStand = 22,
    TalkSitGround = 23,
    TalkSitStool = 24,
    TalkSitBench = 25,
    TalkSitThrone = 26,
    Wade = 27,
    Swim = 28,
    Dive = 29,
    Count = 30,
    }
}

engine_enum! {
    /// `gEAttackReason`: why an NPC is attacking.
    AttackReason {
    None = 0,
    LeaveZone = 1,
    Livestock = 2,
    StopThief = 3,
    Theft = 4,
    UseInteractObject = 5,
    ReactToWeapon = 6,
    Angry = 7,
    StopFight = 8,
    ReactToDamage = 9,
    GateGuard = 10,
    Intruder = 11,
    Enemy = 12,
    Murder = 13,
    Prey = 14,
    DriveAway = 15,
    PlayerCommand = 16,
    Duel = 17,
    Arena = 18,
    Revolution = 19,
    Kill = 20,
    }
}

engine_enum! {
    /// `gENPCType`.
    NpcType {
    Personal = 0,
    Enclave = 1,
    Political = 2,
    Friend = 3,
    }
}

engine_enum! {
    /// `gEPoliticalAlignment`.
    Alignment {
    None = 0,
    Orc = 1,
    Nrd = 2,
    Reb = 3,
    Mid = 4,
    Ass = 5,
    Nom = 6,
    Out = 7,
    Slave = 8,
    Pirate = 9,
    }
}

engine_enum! {
    /// `gEGuardStatus`.
    GuardStatus {
    Active = 0,
    FirstWarnGiven = 1,
    SecondWarnGiven = 2,
    Inactive = 3,
    Behind = 4,
    }
}

engine_enum! {
    /// `gEEnclaveStatus`.
    EnclaveStatus {
    Routine = 0,
    Attack = 1,
    Flee = 2,
    }
}

engine_enum! {
    /// `gELastFightAgainstPlayer`.
    LastFight {
    None = 0,
    Lost = 1,
    Won = 2,
    Cancel = 3,
    Running = 4,
    Friendly = 5,
    }
}

engine_enum! {
    /// `gEPartyMemberType`.
    PartyMemberType {
    None = 0,
    Party = 1,
    Mob = 2,
    Slave = 3,
    Controlled = 4,
    Summoned = 5,
    }
}

engine_enum! {
    /// `gEAnchorType`.
    AnchorType {
    Local = 0,
    Roam = 1,
    Patrol = 2,
    Event = 3,
    }
}

engine_enum! {
    /// `gEClass`: guild class of an NPC.
    NpcClass {
    None = 0,
    Mage = 1,
    Paladin = 2,
    Warrior = 3,
    Ranger = 4,
    EmptyA = 5,
    EmptyB = 6,
    EmptyC = 7,
    EmptyD = 8,
    EmptyE = 9,
    EmptyF = 10,
    EmptyG = 11,
    }
}

engine_enum! {
    /// `gESlot`: equipment slot.
    Slot {
    None = 0,
    RightHand = 1,
    LeftHand = 2,
    EquipLeftHand = 3,
    Beard = 4,
    BackLeft = 5,
    BackRight = 6,
    Bow = 7,
    Crossbow = 8,
    Armor = 9,
    Belt = 10,
    Amulet = 11,
    Ring1 = 12,
    Ring2 = 13,
    Ammo = 14,
    Hair = 15,
    Head = 16,
    Body = 17,
    Helmet = 18,
    }
}

engine_enum! {
    /// `gESpellDeity`.
    SpellDeity {
    Adanos = 0,
    Beliar = 1,
    Innos = 2,
    None = 3,
    }
}

