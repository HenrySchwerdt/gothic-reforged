//! Item use scripts: the `gCItem_PS` script lines (`ScriptLine` command, two entity names and
//! two string arguments) and their interpreter, `gCItem_PS::ExecuteScript`.
use gothic_formats::props::tails::ScriptLine;

/// One decoded script command. Entities are scene entity names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemScriptAction {
    PlaySound {
        entity: String,
    },
    /// `ScriptAdmin::AssessTarget(entity1, entity2)`.
    Attack {
        attacker: String,
        target: String,
    },
    /// Make sure `receiver` owns `amount` of `item`, then run the info's `Give` bookkeeping
    /// with `giver`.
    Give {
        receiver: String,
        giver: String,
        item: String,
        amount: i32,
    },
    GiveXp {
        entity: String,
        amount: i32,
    },
    SetTeachEnabled {
        entity: String,
        enabled: bool,
    },
    SetTradeEnabled {
        entity: String,
        enabled: bool,
    },
    SetPartyEnabled {
        entity: String,
        enabled: bool,
    },
    SetSleepingPoint {
        npc: String,
        point: String,
    },
    SetWorkingPoint {
        npc: String,
        point: String,
    },
    SetRelaxingPoint {
        npc: String,
        point: String,
    },
    SetRoutine {
        npc: String,
        routine: String,
    },
    /// `Active` maps to guard status 0, `Inactive` to 3.
    SetGuardStatus {
        npc: String,
        active: bool,
    },
    SetGameEvent {
        event: String,
    },
    ClearGameEvent {
        event: String,
    },
    SetSectorStatus {
        sector: String,
        enabled: bool,
    },
    RunQuest {
        quest: String,
    },
    CloseQuest {
        quest: String,
    },
    StartEffect {
        effect: String,
        entity: String,
    },
    /// HUD message; types of 0x13 and above print as type 0x10.
    ShowGameMessage {
        text: String,
        kind: i32,
    },
    AddQuestLog {
        quest: String,
        first: String,
        second: String,
    },
}

fn truthy(s: &str) -> bool {
    s == "true" || s == "1"
}

fn int(s: &str) -> i32 {
    s.trim().parse().unwrap_or(0)
}

impl ItemScriptAction {
    /// Decode one script line; `None` for an unknown command (the original ignores those).
    ///
    /// Field use per command: `entity1`/`entity2` name the first and second entity, `id1`
    /// is the string argument and `id2` the second argument or boolean.
    pub fn from_line(l: &ScriptLine) -> Option<Self> {
        let (e1, e2) = (l.entity1.clone(), l.entity2.clone());
        let b1 = truthy(&l.id1);
        let b2 = truthy(&l.id2);
        Some(match l.command.as_str() {
            "PlaySound" => Self::PlaySound { entity: e1 },
            "Attack" => Self::Attack {
                attacker: e1,
                target: e2,
            },
            "Give" => Self::Give {
                receiver: e1,
                giver: e2,
                item: l.id1.clone(),
                amount: int(&l.id2),
            },
            "GiveXP" => Self::GiveXp {
                entity: e1,
                amount: int(&l.id1),
            },
            "SetTeachEnabled" => Self::SetTeachEnabled {
                entity: e1,
                enabled: b1,
            },
            "SetTradeEnabled" => Self::SetTradeEnabled {
                entity: e1,
                enabled: b1,
            },
            "SetPartyEnabled" => Self::SetPartyEnabled {
                entity: e1,
                enabled: b1,
            },
            "SetSleepingPoint" => Self::SetSleepingPoint { npc: e1, point: e2 },
            "SetWorkingPoint" => Self::SetWorkingPoint { npc: e1, point: e2 },
            "SetRelaxingPoint" => Self::SetRelaxingPoint { npc: e1, point: e2 },
            "SetRoutine" => Self::SetRoutine {
                npc: e1,
                routine: l.id1.clone(),
            },
            "SetGuardStatus" => {
                let active = l.id1.eq_ignore_ascii_case("Active");
                if !active && !l.id1.eq_ignore_ascii_case("Inactive") {
                    return None;
                }
                Self::SetGuardStatus { npc: e1, active }
            }
            "SetGameEvent" => Self::SetGameEvent {
                event: l.id1.clone(),
            },
            "ClearGameEvent" => Self::ClearGameEvent {
                event: l.id1.clone(),
            },
            "SetSectorStatus" => Self::SetSectorStatus {
                sector: l.id1.clone(),
                enabled: b2,
            },
            "RunQuest" => Self::RunQuest {
                quest: l.id1.clone(),
            },
            "CloseQuest" => Self::CloseQuest {
                quest: l.id1.clone(),
            },
            "StartEffect" => Self::StartEffect {
                effect: l.id1.clone(),
                entity: e1,
            },
            "ShowGameMessage" => {
                if l.id1.is_empty() {
                    return None;
                }
                let k = int(&l.id2);
                Self::ShowGameMessage {
                    text: l.id1.clone(),
                    kind: if k < 0x13 { k } else { 0x10 },
                }
            }
            "AddQuestLog" => Self::AddQuestLog {
                quest: l.id1.clone(),
                first: e2,
                second: e1,
            },
            _ => return None,
        })
    }
}

/// Services the interpreter needs from the host.
pub trait ItemScriptHost {
    /// Is the game event already set? (`SetGameEvent` lines guard the whole script.)
    fn game_event_set(&self, event: &str) -> bool;
    /// Does the session have the player-memory property set that stores game events? Without
    /// it the guard is skipped.
    fn has_event_store(&self) -> bool {
        true
    }
    /// Perform one action.
    fn run(&mut self, action: &ItemScriptAction);
}

/// Outcome of [`execute_script`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRun {
    /// A `SetGameEvent` line named an event that is already set: nothing was executed.
    AlreadyDone,
    /// All known lines ran (count).
    Executed(usize),
}

/// Port of gCItem_PS::ExecuteScript (Game:0x100b9df0): scripts guarded by a `SetGameEvent`
/// line run only once per event; otherwise every line runs in order. (The original returns
/// `false` in both cases and sets an "executed" flag, which the caller keeps.)
pub fn execute_script(lines: &[ScriptLine], host: &mut dyn ItemScriptHost) -> ScriptRun {
    gothic_portcov::hit!("execute_script");
    if host.has_event_store() {
        for l in lines {
            if l.command == "SetGameEvent" && host.game_event_set(&l.id1) {
                return ScriptRun::AlreadyDone;
            }
        }
    }
    let mut n = 0;
    for l in lines {
        if let Some(a) = ItemScriptAction::from_line(l) {
            host.run(&a);
            n += 1;
        }
    }
    ScriptRun::Executed(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(cmd: &str, e1: &str, e2: &str, id1: &str, id2: &str) -> ScriptLine {
        ScriptLine {
            command: cmd.into(),
            entity1: e1.into(),
            entity2: e2.into(),
            id1: id1.into(),
            id2: id2.into(),
        }
    }

    #[derive(Default)]
    struct Host {
        events: Vec<String>,
        ran: Vec<ItemScriptAction>,
    }
    impl ItemScriptHost for Host {
        fn game_event_set(&self, e: &str) -> bool {
            self.events.iter().any(|x| x == e)
        }
        fn run(&mut self, a: &ItemScriptAction) {
            self.ran.push(a.clone());
        }
    }

    #[test]
    fn decodes_commands() {
        let a = ItemScriptAction::from_line(&line("Give", "Hero", "Npc", "It_Gold", "50")).unwrap();
        assert_eq!(
            a,
            ItemScriptAction::Give {
                receiver: "Hero".into(),
                giver: "Npc".into(),
                item: "It_Gold".into(),
                amount: 50
            }
        );
        let a =
            ItemScriptAction::from_line(&line("SetTradeEnabled", "Smith", "", "1", "")).unwrap();
        assert_eq!(
            a,
            ItemScriptAction::SetTradeEnabled {
                entity: "Smith".into(),
                enabled: true
            }
        );
        assert!(matches!(
            ItemScriptAction::from_line(&line("ShowGameMessage", "", "", "Hi", "99")),
            Some(ItemScriptAction::ShowGameMessage { kind: 0x10, .. })
        ));
        assert!(
            ItemScriptAction::from_line(&line("SetGuardStatus", "N", "", "Maybe", "")).is_none()
        );
        assert!(ItemScriptAction::from_line(&line("Nope", "", "", "", "")).is_none());
    }

    #[test]
    fn game_event_guard_runs_a_script_once() {
        let lines = vec![
            line("SetGameEvent", "", "", "EV_Letter", ""),
            line("RunQuest", "", "", "Q_Letter", ""),
        ];
        let mut h = Host::default();
        assert_eq!(execute_script(&lines, &mut h), ScriptRun::Executed(2));
        h.events.push("EV_Letter".into());
        h.ran.clear();
        assert_eq!(execute_script(&lines, &mut h), ScriptRun::AlreadyDone);
        assert!(h.ran.is_empty());
    }
}
