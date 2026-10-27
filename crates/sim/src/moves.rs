//! `OnMovementModeChanged` (S:adbf0) for the hero: what the script does when the engine's
//! movement mode machine (`gCCharacterMovement_PS::SetMovementMode`) changes the mode.
//!
//! The engine only reports `(last mode, mode, state time)`; the script decides the state the
//! character enters: falling starts `ZS_Fall`, sliding `ZS_Slide`, swimming `PS_Swim` (the hero;
//! other characters drown as ragdolls), and leaving the fall applies the fall damage and picks
//! `ZS_HeavyLand` / `ZS_Acrobat` / `ZS_DiveIn`. The host (the play layer) plays the animations
//! and owns the body; this module owns the rules and the data effects.

use crate::sim::Sim;
use crate::types::{ActorId, AniState};
use crate::world::WorldQuery;

/// `gECharMovementMode` values the script tests (field `+0x100`, `GetMovementMode`).
pub mod mode {
    pub const JUMPING: u8 = 6;
    pub const SWIMMING: u8 = 7;
    pub const DIVING: u8 = 8;
    pub const WADING: u8 = 10;
    pub const SLIDING: u8 = 0xc;
    pub const FALLING: u8 = 0xd;
}

/// A fall shorter than this is free (`DAT_101075a0`, double at S:0x101075a0 = 0.9 s) [V].
pub const FALL_FREE_SECS: f32 = 0.9;
/// Falling into water after more than this many seconds teaches `Perk_Acrobatic`
/// (`DAT_10107658`, double at S:0x10107658 = 7.45 s) [V].
pub const ACROBAT_LEARN_SECS: f32 = 7.45;
/// Damage factor of the fall rule (`DAT_10107660`, double = 160) [V].
pub const FALL_DAMAGE_FACTOR: f32 = 160.0;

/// The state the script put the character into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModeTask {
    /// Nothing to do (the routine / the player's state goes on).
    #[default]
    None,
    /// `ZS_Fall`: the fall pose.
    Fall,
    /// `ZS_Slide`: the slide pose.
    Slide,
    /// `PS_Swim`: items away, swim ani state.
    Swim,
    /// `ZS_HeavyLand`: hurt by the landing, plays the heavy landing clip.
    HeavyLand,
    /// `ZS_Acrobat`: the landing damage was halved by `Perk_Acrobatic`.
    Acrobat,
    /// `ZS_DiveIn`: fell into deep water.
    DiveIn,
    /// The hero died of the fall (`ZS_RagDollDead`).
    RagDollDead,
}

/// Result of [`Sim::movement_mode_changed`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeOutcome {
    pub task: ModeTask,
    /// Hit points the fall took.
    pub damage: i32,
    /// `Perk_Acrobatic` was learned by this fall into water (`GO_Learned`).
    pub learned_acrobat: bool,
    /// The character left the swim mode (the host restores the normal body).
    pub left_swim: bool,
}

impl<W: WorldQuery> Sim<W> {
    /// Port of OnMovementModeChanged (Script:adbf0) for `id` (the hero). `last` and `mode` are
    /// `GetLastMovementMode` / `GetMovementMode`, `state_secs` the time the character spent in
    /// the state it leaves (`PSRoutine::GetStateTime`, i.e. the fall time for the fall mode).
    pub fn movement_mode_changed(
        &mut self,
        id: ActorId,
        last: u8,
        mode: u8,
        state_secs: f32,
    ) -> ModeOutcome {
        gothic_portcov::hit!("movement_mode_changed");
        let mut out = ModeOutcome::default();
        let is_hero = self.actors[id.0 as usize].is_player;
        if last == mode {
            return out;
        }
        let acrobatic = self.actors[id.0 as usize].has_perk("perk_acrobatic");
        if last == mode::FALLING {
            if mode == mode::SWIMMING {
                // Into deep water: no damage; a very long fall teaches Perk_Acrobatic.
                if is_hero && state_secs > ACROBAT_LEARN_SECS && !acrobatic {
                    let a = &mut self.actors[id.0 as usize];
                    a.perks.insert("perk_acrobatic".into());
                    out.learned_acrobat = true;
                    self.script.messages.push((
                        "GO_Learned".into(),
                        "FO_It_Perk_Acrobatic".into(),
                        0,
                    ));
                }
                out.task = if is_hero {
                    ModeTask::DiveIn
                } else {
                    ModeTask::RagDollDead
                };
                self.apply_mode_task(id, out.task);
                return out;
            }
            if mode == mode::SLIDING {
                // Landed on ground too steep to stand: no damage, the slide state follows
                // (the original jumps over the damage rule here, S:adbf0 `LAB_100ae3d9`).
            } else {
                let dmg = self.land(id, state_secs);
                out.damage = dmg;
                if dmg > 0 {
                    let a = &self.actors[id.0 as usize];
                    out.task = if a.hp <= 0 || a.ai_mode == crate::types::AiMode::Dead {
                        ModeTask::RagDollDead
                    } else if acrobatic {
                        ModeTask::Acrobat
                    } else {
                        ModeTask::HeavyLand
                    };
                    self.apply_mode_task(id, out.task);
                    return out;
                }
            }
        }
        if last == mode::SWIMMING && mode != mode::SLIDING && mode != mode::FALLING {
            out.left_swim = true;
        }
        out.task = match mode {
            mode::FALLING => ModeTask::Fall,
            mode::SLIDING => ModeTask::Slide,
            mode::SWIMMING if is_hero => ModeTask::Swim,
            mode::SWIMMING => ModeTask::RagDollDead,
            _ => ModeTask::None,
        };
        self.apply_mode_task(id, out.task);
        if out.left_swim || (last == mode::FALLING && out.task == ModeTask::None) {
            let a = &mut self.actors[id.0 as usize];
            if matches!(
                a.ani_state,
                AniState::Swim | AniState::Dive | AniState::Fall
            ) {
                a.ani_state = AniState::Stand;
            }
        }
        out
    }

    /// The data side of the states the callback enters: `PS_Swim` sets the swim ani state and
    /// holsters everything (`HoldStacks(-1, -1)`), `ZS_Fall` the fall ani state.
    fn apply_mode_task(&mut self, id: ActorId, task: ModeTask) {
        let a = &mut self.actors[id.0 as usize];
        match task {
            ModeTask::Fall => a.ani_state = AniState::Fall,
            ModeTask::Swim | ModeTask::DiveIn => a.ani_state = AniState::Swim,
            ModeTask::Slide | ModeTask::HeavyLand | ModeTask::Acrobat => {
                a.ani_state = AniState::Stand
            }
            _ => {}
        }
    }
}
