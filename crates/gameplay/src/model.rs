//! Enumerations decoded from the game binaries (see `docs/research/GAME_LOGIC_MAP.md`,
//! section 4.2/4.3/6). Raw integers are kept for values that have no known name so that
//! nothing is silently dropped.
use serde::{Deserialize, Serialize};

/// `gEQuestStatus` [verified names/values].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QuestStatus {
    Open = 0,
    Running = 1,
    Success = 2,
    Failed = 3,
    Obsolete = 4,
    Cancelled = 5,
    Lost = 6,
    Won = 7,
}

impl QuestStatus {
    /// `AreFinishedQuestsFinished` rule (GAME_RULES 1.4): Open, Running and Lost are
    /// not finished; Success, Failed, Obsolete, Cancelled and Won are.
    pub fn is_finished(self) -> bool {
        !matches!(self, Self::Open | Self::Running | Self::Lost)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Running => "Running",
            Self::Success => "Success",
            Self::Failed => "Failed",
            Self::Obsolete => "Obsolete",
            Self::Cancelled => "Cancelled",
            Self::Lost => "Lost",
            Self::Won => "Won",
        }
    }
}

/// `gEQuestType` [verified]. 9 is unused in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestType {
    HasItems,
    Report,
    Kill,
    Defeat,
    DriveAway,
    Arena,
    BringNpc,
    FollowNpc,
    EnterArea,
    FreeEnclave,
    Plunder,
    Sparring,
    SpellCast,
    Unknown(u32),
}

impl QuestType {
    pub fn from_raw(v: u32) -> Self {
        match v {
            0 => Self::HasItems,
            1 => Self::Report,
            2 => Self::Kill,
            3 => Self::Defeat,
            4 => Self::DriveAway,
            5 => Self::Arena,
            6 => Self::BringNpc,
            7 => Self::FollowNpc,
            8 => Self::EnterArea,
            10 => Self::FreeEnclave,
            11 => Self::Plunder,
            12 => Self::Sparring,
            13 => Self::SpellCast,
            other => Self::Unknown(other),
        }
    }
}

/// `gEInfoCondType` (INI `ConditionType`) [verified names, `General`=3 inferred].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfoCond {
    Crime,
    Fight,
    Hello,
    General,
    Overtime,
    Open,
    Activator,
    Running,
    Delivery,
    PartDelivery,
    Success,
    DoCancel,
    Failed,
    Cancelled,
    Join,
    Dismiss,
    Teach,
    Trade,
    PickPocket,
    Ready,
    Lost,
    Reactivator,
    Won,
    MasterThief,
    EnclaveFriend,
    PoliticalFriend,
    FirstWarn,
    SecondWarn,
    MobJoin,
    SlaveJoin,
    LongTimeNoSee,
    EnclaveCrime,
    PoliticalCrime,
    MobDismiss,
    Wait,
    Heal,
    Animal,
    Companion,
    NotFriend,
    NotFriendOrThief,
    NothingToSay,
    End,
    Back,
    StepBack,
    Unknown(u32),
}

impl InfoCond {
    pub fn from_raw(v: u32) -> Self {
        use InfoCond::*;
        match v {
            0 => Crime,
            1 => Fight,
            2 => Hello,
            3 => General,
            4 => Overtime,
            5 => Open,
            6 => Activator,
            7 => Running,
            8 => Delivery,
            9 => PartDelivery,
            10 => Success,
            11 => DoCancel,
            12 => Failed,
            13 => Cancelled,
            14 => Join,
            15 => Dismiss,
            16 => Teach,
            17 => Trade,
            18 => PickPocket,
            19 => Ready,
            20 => Lost,
            21 => Reactivator,
            22 => Won,
            23 => MasterThief,
            24 => EnclaveFriend,
            25 => PoliticalFriend,
            26 => FirstWarn,
            27 => SecondWarn,
            28 => MobJoin,
            29 => SlaveJoin,
            30 => LongTimeNoSee,
            31 => EnclaveCrime,
            32 => PoliticalCrime,
            33 => MobDismiss,
            34 => Wait,
            35 => Heal,
            36 => Animal,
            37 => Companion,
            38 => NotFriend,
            39 => NotFriendOrThief,
            50 => NothingToSay,
            51 => End,
            52 => Back,
            53 => StepBack,
            other => Unknown(other),
        }
    }

    /// Condition types that are selectable dialog options in the normal talk menu.
    pub fn is_menu_option(self) -> bool {
        !matches!(
            self,
            Self::Hello | Self::NothingToSay | Self::Wait | Self::Unknown(_)
        ) && !self.is_reaction()
    }

    /// Engine-triggered reactions (the NPC reacts to a situation); never part of the menu.
    pub fn is_reaction(self) -> bool {
        use InfoCond::*;
        matches!(
            self,
            Crime
                | Fight
                | Overtime
                | FirstWarn
                | SecondWarn
                | EnclaveCrime
                | PoliticalCrime
                | LongTimeNoSee
                | MasterThief
                | EnclaveFriend
                | PoliticalFriend
                | NotFriend
                | NotFriendOrThief
                | Animal
                | Companion
                | Join
                | Dismiss
                | MobJoin
                | SlaveJoin
                | MobDismiss
                | PickPocket
                | Heal
        )
    }
}

/// `gEInfoType` [verified names].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfoType {
    Refuse,
    Important,
    News,
    Info,
    Parent,
    Comment,
    QuestRunning,
    Unknown(u32),
}

impl InfoType {
    pub fn from_raw(v: u32) -> Self {
        match v {
            0 => Self::Refuse,
            1 => Self::Important,
            2 => Self::News,
            3 => Self::Info,
            4 => Self::Parent,
            5 => Self::Comment,
            7 => Self::QuestRunning,
            other => Self::Unknown(other),
        }
    }
}

/// `gEInfoNPCStatus` as used by `CondSecondaryNPCstates` (GAME_RULES 1.3 e, verified
/// numbering; the enumerator names are not recovered, these are descriptive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NpcStatus {
    Alive,
    AliveNotDefeated,
    Defeated,
    Dead,
    TalkedToPlayer,
    NotTalkedToPlayer,
}

impl NpcStatus {
    pub fn from_raw(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Alive,
            1 => Self::AliveNotDefeated,
            2 => Self::Defeated,
            3 => Self::Dead,
            4 => Self::TalkedToPlayer,
            5 => Self::NotTalkedToPlayer,
            _ => return None,
        })
    }
}

/// One info-script command, by the 68-entry table of the game (`Say`=0 ... `DecreaseRevolCount`=68).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Say,
    SaySvm,
    SaySound,
    SayKeepItUp,
    SayBossNews,
    SayGuardNews,
    SayConfirm,
    SayHoldingPosition,
    Excuse,
    Bribe,
    PickPocket,
    Attack,
    GiveXp,
    Give,
    Wear,
    Use,
    Heal,
    OpenDoor,
    SetClass,
    SetPal,
    SetEnclave,
    Sleep,
    SetTeachEnabled,
    SetTradeEnabled,
    SetPartyEnabled,
    SetMobEnabled,
    SetSlaveryEnabled,
    SetRevolutionEnabled,
    SetRaidEnabled,
    SetSleepingPoint,
    SetWorkingPoint,
    SetRelaxingPoint,
    FullStop,
    SetRoutine,
    SetGuardStatus,
    SetGameEvent,
    ClearGameEvent,
    MoveTo,
    SetSectorStatus,
    RunQuest,
    CloseQuest,
    SucceedQuest,
    StartEffect,
    DoTrade,
    DoLoot,
    ShowPicture,
    ShowSubtitle,
    PlayMusic,
    InfoWait,
    EndGame,
    Back,
    End,
    BoostAttribs,
    SetChapter,
    Erase,
    StartRevolution,
    Spawn,
    ShowGameMessage,
    AddQuestLog,
    ModReputPol,
    ModReputEnc,
    Grant,
    Humanize,
    JoinNpc,
    GiveAll,
    JoinPlayer,
    FailQuest,
    ResetCrime,
    DecreaseRevolCount,
    /// Pseudo-command: the label of a menu choice (no id in the table).
    Description,
}

impl Cmd {
    /// Case-insensitive lookup. Returns `None` for commands outside the game's table.
    pub fn parse(name: &str) -> Option<Self> {
        use Cmd::*;
        Some(match name.to_ascii_lowercase().as_str() {
            "say" => Say,
            "saysvm" => SaySvm,
            "saysound" => SaySound,
            "saykeepitup" => SayKeepItUp,
            "saybossnews" => SayBossNews,
            "sayguardnews" => SayGuardNews,
            "sayconfirm" => SayConfirm,
            "sayholdingposition" => SayHoldingPosition,
            "excuse" => Excuse,
            "bribe" => Bribe,
            "pickpocket" => PickPocket,
            "attack" => Attack,
            "givexp" => GiveXp,
            "give" => Give,
            "wear" => Wear,
            "use" => Use,
            "heal" => Heal,
            "opendoor" => OpenDoor,
            "setclass" => SetClass,
            "setpal" => SetPal,
            "setenclave" => SetEnclave,
            "sleep" => Sleep,
            "setteachenabled" => SetTeachEnabled,
            "settradeenabled" => SetTradeEnabled,
            "setpartyenabled" => SetPartyEnabled,
            "setmobenabled" => SetMobEnabled,
            "setslaveryenabled" => SetSlaveryEnabled,
            "setrevolutionenabled" => SetRevolutionEnabled,
            "setraidenabled" => SetRaidEnabled,
            "setsleepingpoint" => SetSleepingPoint,
            "setworkingpoint" => SetWorkingPoint,
            "setrelaxingpoint" => SetRelaxingPoint,
            "fullstop" => FullStop,
            "setroutine" => SetRoutine,
            "setguardstatus" => SetGuardStatus,
            "setgameevent" => SetGameEvent,
            "cleargameevent" => ClearGameEvent,
            "moveto" => MoveTo,
            "setsectorstatus" => SetSectorStatus,
            "runquest" => RunQuest,
            "closequest" => CloseQuest,
            "succeedquest" => SucceedQuest,
            "starteffect" => StartEffect,
            "dotrade" => DoTrade,
            "doloot" => DoLoot,
            "showpicture" => ShowPicture,
            "showsubtitle" => ShowSubtitle,
            "playmusic" => PlayMusic,
            "infowait" => InfoWait,
            "endgame" => EndGame,
            "back" => Back,
            "end" => End,
            "boostattribs" => BoostAttribs,
            "setchapter" => SetChapter,
            "erase" => Erase,
            "startrevolution" => StartRevolution,
            "spawn" => Spawn,
            "showgamemessage" => ShowGameMessage,
            "addquestlog" => AddQuestLog,
            "modreputpol" => ModReputPol,
            "modreputenc" => ModReputEnc,
            "grant" => Grant,
            "humanize" => Humanize,
            "joinnpc" => JoinNpc,
            "giveall" => GiveAll,
            "joinplayer" => JoinPlayer,
            "failquest" => FailQuest,
            "resetcrime" => ResetCrime,
            "decreaserevolcount" => DecreaseRevolCount,
            "description" => Description,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_known_values() {
        assert_eq!(InfoCond::from_raw(6), InfoCond::Activator);
        assert_eq!(InfoCond::from_raw(52), InfoCond::Back);
        assert_eq!(InfoCond::from_raw(44), InfoCond::Unknown(44));
        assert_eq!(QuestType::from_raw(9), QuestType::Unknown(9));
        assert_eq!(Cmd::parse("Pickpocket"), Some(Cmd::PickPocket));
        assert_eq!(Cmd::parse("Nope"), None);
        assert!(QuestStatus::Success.is_finished() && !QuestStatus::Running.is_finished());
    }
}
