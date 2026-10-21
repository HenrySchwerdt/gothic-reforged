//! Movers and doors (`gCMover_PS`, `gCDoor_PS`): an entity that plays a motion to open or
//! close. The state machine and its transitions are ported; the motion itself (EMotionFX
//! playback, physics controller) is the host's, which reports when the motion stopped.
//!
//! `gEMoverState`: 0 open, 1 opening, 2 closed, 3 closing (a door's `Status` 0 maps to open and
//! anything else to closed, and collision is on only while the state is closed).
//!
//! Reconstructed: the decompiler lost the sign of the play direction; opening plays the
//! motion forward and closing backward, as `Toggle` implies.

/// `gEMoverState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoverState {
    Open = 0,
    Opening = 1,
    #[default]
    Closed = 2,
    Closing = 3,
}

/// What the host has to do after a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoverAction {
    /// Start the motion at `from` (seconds into the clip) with the given speed (+1 forward,
    /// -1 backward; `reversed` movers play the other way round).
    Play { from: PlayFrom, speed: i32 },
    /// The state settled: the open flag changed (`vfn 0x174`) and the targets are triggered
    /// (`vfn 0x178`).
    Settled { open: bool, trigger_targets: bool },
}

/// Where a motion starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayFrom {
    /// The clip's beginning.
    Start,
    /// The current play time (reversing a running motion).
    Current,
    /// The clip's end (instant open / close: the motion jumps to its final pose).
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mover {
    pub state: MoverState,
    /// `+0x10c`: the clip is stored reversed.
    pub reversed: bool,
    /// `+0xf8`: decides in which state the finished motions end (see [`Mover::motion_stopped`]).
    pub rest: i32,
}

impl Mover {
    fn speed(&self, opening: bool) -> i32 {
        let forward = if self.reversed { -1 } else { 1 };
        if opening { forward } else { -forward }
    }

    /// Port of gCMover_PS::Open (Game:0x1a7780): ignored while already opening. A closing
    /// mover reverses from where it is; `instant` jumps to the open pose.
    pub fn open(&mut self, instant: bool) -> Option<MoverAction> {
        if self.state == MoverState::Opening {
            return None;
        }
        if self.state == MoverState::Closing {
            self.state = MoverState::Opening;
            return Some(MoverAction::Play {
                from: PlayFrom::Current,
                speed: self.speed(true),
            });
        }
        if instant {
            self.state = MoverState::Open;
            return Some(MoverAction::Settled {
                open: true,
                trigger_targets: false,
            });
        }
        self.state = MoverState::Opening;
        Some(MoverAction::Play {
            from: PlayFrom::Start,
            speed: self.speed(true),
        })
    }

    /// Port of gCMover_PS::Close (Game:0x1a7a70).
    pub fn close(&mut self, instant: bool) -> Option<MoverAction> {
        if self.state == MoverState::Closing {
            return None;
        }
        if self.state == MoverState::Opening {
            self.state = MoverState::Closing;
            return Some(MoverAction::Play {
                from: PlayFrom::Current,
                speed: self.speed(false),
            });
        }
        if instant {
            self.state = MoverState::Closed;
            return Some(MoverAction::Settled {
                open: false,
                trigger_targets: false,
            });
        }
        self.state = MoverState::Closing;
        Some(MoverAction::Play {
            from: PlayFrom::End,
            speed: self.speed(false),
        })
    }

    /// Port of gCMover_PS::Toggle (Game:0x1a7130): open what is closed or closing, close
    /// what is open or opening.
    pub fn toggle(&mut self) -> Option<MoverAction> {
        match self.state {
            MoverState::Open | MoverState::Opening => self.close(false),
            MoverState::Closed | MoverState::Closing => self.open(false),
        }
    }

    /// Port of gCMover_PS::OnProcess (Game:0x1a7240): the host calls it when the motion's play
    /// speed has dropped to zero (it ran to its end). With `rest == 0` the finished motions end
    /// in the opposite state (as decompiled; see the module notes).
    pub fn motion_stopped(&mut self) -> Option<MoverAction> {
        let (state, open) = match (self.state, self.rest == 0) {
            (MoverState::Opening, true) => (MoverState::Closed, false),
            (MoverState::Opening, false) => (MoverState::Open, true),
            (MoverState::Closing, true) => (MoverState::Open, true),
            (MoverState::Closing, false) => (MoverState::Closed, false),
            _ => return None,
        };
        self.state = state;
        Some(MoverAction::Settled {
            open,
            trigger_targets: self.rest == 0,
        })
    }

    /// Port of gCDoor_PS::OnNotifyPropertyValueChangedExitEx (Game:0xcc020), `Status`: a door
    /// whose status is 0 is open, otherwise closed.
    pub fn set_door_status(&mut self, status: i32) {
        self.state = if status == 0 {
            MoverState::Open
        } else {
            MoverState::Closed
        };
    }

    /// Skeleton collision is on while the door is closed (same function, `MoverState`).
    pub fn collides(&self) -> bool {
        self.state == MoverState::Closed
    }

    /// Port of gCDoor_PS::SetIsOpen (Game:0xcbf80): the door status for an open flag
    /// (0 = open, 1 = closed).
    pub fn door_status(open: bool) -> i32 {
        if open { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_runs_the_motion_forward_and_close_backward() {
        let mut m = Mover::default();
        assert_eq!(
            m.open(false),
            Some(MoverAction::Play {
                from: PlayFrom::Start,
                speed: 1
            })
        );
        assert_eq!(m.state, MoverState::Opening);
        // asking again while opening does nothing
        assert_eq!(m.open(false), None);
        // closing a running opening reverses it from the current time
        assert_eq!(
            m.close(false),
            Some(MoverAction::Play {
                from: PlayFrom::Current,
                speed: -1
            })
        );
        assert_eq!(m.state, MoverState::Closing);
        assert_eq!(m.close(false), None);
    }

    #[test]
    fn reversed_clips_play_the_other_way() {
        let mut m = Mover {
            reversed: true,
            ..Mover::default()
        };
        assert_eq!(
            m.open(false),
            Some(MoverAction::Play {
                from: PlayFrom::Start,
                speed: -1
            })
        );
    }

    #[test]
    fn instant_open_and_close_settle_at_once() {
        let mut m = Mover::default();
        assert_eq!(
            m.open(true),
            Some(MoverAction::Settled {
                open: true,
                trigger_targets: false
            })
        );
        assert_eq!(m.state, MoverState::Open);
        assert!(!m.collides());
        m.close(true);
        assert_eq!(m.state, MoverState::Closed);
        assert!(m.collides());
    }

    #[test]
    fn toggle_flips_between_the_halves() {
        let mut m = Mover::default();
        m.toggle();
        assert_eq!(m.state, MoverState::Opening);
        m.toggle();
        assert_eq!(m.state, MoverState::Closing);
        m.toggle();
        assert_eq!(m.state, MoverState::Opening);
    }

    #[test]
    fn finished_motions_settle_by_the_rest_setting() {
        let mut m = Mover {
            rest: 1,
            ..Mover::default()
        };
        m.open(false);
        assert_eq!(
            m.motion_stopped(),
            Some(MoverAction::Settled {
                open: true,
                trigger_targets: false
            })
        );
        assert_eq!(m.state, MoverState::Open);
        m.close(false);
        assert_eq!(
            m.motion_stopped(),
            Some(MoverAction::Settled {
                open: false,
                trigger_targets: false
            })
        );
        assert_eq!(m.state, MoverState::Closed);
        // rest 0 triggers the targets at both ends
        let mut z = Mover::default();
        z.open(false);
        assert_eq!(
            z.motion_stopped(),
            Some(MoverAction::Settled {
                open: false,
                trigger_targets: true
            })
        );
        // nothing to settle at rest
        assert_eq!(m.motion_stopped(), None);
    }

    #[test]
    fn door_status_zero_is_open() {
        let mut m = Mover::default();
        m.set_door_status(0);
        assert_eq!(m.state, MoverState::Open);
        m.set_door_status(1);
        assert_eq!(m.state, MoverState::Closed);
        assert_eq!(Mover::door_status(true), 0);
        assert_eq!(Mover::door_status(false), 1);
    }
}
