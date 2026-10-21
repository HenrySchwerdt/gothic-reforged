//! Everything the core tells the outside world: presentation (animation, audio, HUD) and the
//! script layer subscribe to these; nothing in here changes game state by itself.
use crate::actions::{ActionId, Hurt, StrikeKind};

/// What happened to a swing that reached its victim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitResult {
    /// Out of reach, dead victim or refused by the pipeline.
    Ignored,
    /// Evaded by i-frames.
    Dodged,
    /// Deflected inside the parry window.
    Parried,
    /// Absorbed by the raised guard.
    Blocked,
    /// Guard blocked but stamina ran out.
    GuardBroken,
    /// Went through the pipeline and damaged the victim.
    Hit,
    Killed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The player started an action.
    HeroAction(ActionId),
    /// A player swing connected with a foe.
    HeroHit {
        foe: u32,
        kind: StrikeKind,
        damage: i32,
        killed: bool,
        poise_broken: bool,
    },
    /// A foe swing reached the player.
    HeroHurt {
        foe: u32,
        result: HitResult,
        damage: i32,
        level: Hurt,
    },
    HeroDied,
    FoeWindup {
        foe: u32,
        attack: u8,
    },
    FoeStrike {
        foe: u32,
        attack: u8,
    },
    FoeStaggered {
        foe: u32,
        by_parry: bool,
    },
    FoeRecovered {
        foe: u32,
    },
    FoeDied {
        foe: u32,
    },
    LockChanged {
        target: Option<u32>,
    },
    /// Stamina hit zero while acting.
    Exhausted,
}
