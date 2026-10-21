//! A running effect: the command-sequence player of `gCEffect` (`Start`, `Process`,
//! `HasFinished`, Game:0x1005e870..0x1005ea20).
use crate::efm::{CommandSequence, EffectCommand};
use std::sync::Arc;

/// Plays one [`CommandSequence`]: every command fires once when the elapsed time reaches its
/// `TimeOffset`.
#[derive(Clone, Debug)]
pub struct EffectPlayer {
    sequence: Arc<CommandSequence>,
    elapsed: f32,
    next: usize,
    permanent: bool,
}

impl EffectPlayer {
    /// Port of gCEffect::Start (Game:0x1005e910): `permanent` effects (`gCEffect_PS.Static`)
    /// never finish. Time-zero commands fire on the first [`process`](Self::process) call.
    pub fn start(sequence: Arc<CommandSequence>, permanent: bool) -> Self {
        Self {
            sequence,
            elapsed: 0.0,
            next: 0,
            permanent,
        }
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Port of gCEffect::Process (Game:0x1005e980): advance by `dt` and return the
    /// commands that became due, in sequence order.
    pub fn process(&mut self, dt: f32) -> Vec<&EffectCommand> {
        self.elapsed += dt;
        let mut due = Vec::new();
        while let Some(c) = self.sequence.commands.get(self.next) {
            if self.elapsed < c.time {
                break;
            }
            due.push(c);
            self.next += 1;
        }
        due
    }

    /// Port of gCEffect::HasFinished (Game:0x1005e870): not permanent and past the last command's start time.
    pub fn has_finished(&self) -> bool {
        !self.permanent && self.sequence.end_time() < self.elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::efm::CommandKind;

    fn seq(times: &[f32]) -> Arc<CommandSequence> {
        Arc::new(CommandSequence {
            commands: times
                .iter()
                .map(|t| EffectCommand {
                    time: *t,
                    kind: CommandKind::PlaySound,
                })
                .collect(),
        })
    }

    #[test]
    fn commands_fire_once_at_their_time() {
        let mut p = EffectPlayer::start(seq(&[0.0, 0.5, 0.5, 2.0]), false);
        assert_eq!(p.process(0.0).len(), 1);
        assert_eq!(p.process(0.4).len(), 0);
        assert_eq!(p.process(0.2).len(), 2);
        assert_eq!(p.process(0.5).len(), 0);
        assert!(!p.has_finished());
        assert_eq!(p.process(1.0).len(), 1);
        assert!(p.has_finished());
        assert_eq!(p.process(5.0).len(), 0);
    }

    #[test]
    fn permanent_effects_never_finish() {
        let mut p = EffectPlayer::start(seq(&[0.0]), true);
        p.process(100.0);
        assert!(!p.has_finished());
    }

    #[test]
    fn empty_sequence_finishes_right_after_start() {
        let mut p = EffectPlayer::start(seq(&[]), false);
        assert!(!p.has_finished());
        p.process(0.01);
        assert!(p.has_finished());
    }
}
