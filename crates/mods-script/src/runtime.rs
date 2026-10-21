//! Adapters between the game core (`gothic_runtime::{Event, Command}`) and the script
//! vocabulary. The host feeds `event_from_runtime` results to [`crate::ScriptHost::handle`]
//! and turns the returned commands back into core commands with `command_to_runtime`.
use crate::vocab::{Line, ScriptCommand, ScriptEvent};
use gothic_runtime::{Command, Event};
use gothic_sim::data::NpcRecord;
use gothic_sim::sim::EventKind;
use gothic_sim::types::ActorId;
use gothic_sim::world::Pos;

/// The script event for a core event (`None` for events scripts do not see).
pub fn event_from_runtime(ev: &Event) -> Option<ScriptEvent> {
    Some(match ev {
        Event::QuestChanged { quest, to } => ScriptEvent::QuestChanged {
            quest: quest.clone(),
            status: format!("{to:?}"),
        },
        Event::ActorSpawned { actor, name } => ScriptEvent::ActorSpawned {
            actor: actor.0,
            name: name.clone(),
        },
        Event::Sim(EventKind::Killed { victim, killer }) => ScriptEvent::ActorKilled {
            victim: victim.0,
            killer: killer.map(|k| k.0),
        },
        Event::LineSpoken {
            speaker,
            key,
            text,
            player,
            actor,
        } => ScriptEvent::LineSpoken(Line {
            actor: actor.map(|a| a.0),
            speaker: speaker.clone(),
            key: key.clone(),
            text: text.clone().unwrap_or_default(),
            player: *player,
        }),
        Event::DialogStarted { npc, .. } => ScriptEvent::DialogStarted { npc: npc.0 },
        Event::DialogEnded { npc } => ScriptEvent::DialogEnded {
            npc: npc.map(|n| n.0),
        },
        Event::ItemMoved {
            item,
            amount,
            gained,
        } => ScriptEvent::ItemMoved {
            item: item.clone(),
            amount: *amount,
            gained: *gained,
        },
        Event::XpGained {
            amount,
            level_after,
            ..
        } => ScriptEvent::XpGained {
            amount: *amount,
            level_after: *level_after,
        },
        _ => return None,
    })
}

/// The core command for a script command. `npc` resolves a template name to its record
/// (e.g. from the loaded `npcs.json` templates). `Notice` is presentation-only and returns
/// `None`; the host shows it itself.
pub fn command_to_runtime(
    cmd: &ScriptCommand,
    npc: impl Fn(&str) -> Option<NpcRecord>,
) -> Option<Command> {
    Some(match cmd {
        ScriptCommand::SpawnNpc { template, at, yaw } => Command::SpawnNpc {
            record: Box::new(npc(template)?),
            at: Pos {
                x: at.x,
                y: at.y,
                z: at.z,
            },
            yaw: *yaw,
        },
        ScriptCommand::GiveHero { template, amount } => Command::GiveHero {
            template: template.clone(),
            amount: *amount,
        },
        ScriptCommand::GiveNpc {
            actor,
            template,
            amount,
        } => Command::GiveNpc {
            actor: ActorId(*actor),
            template: template.clone(),
            amount: *amount,
        },
        ScriptCommand::Notice { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_gameplay::model::QuestStatus;

    #[test]
    fn maps_events_and_commands() {
        let e = event_from_runtime(&Event::QuestChanged {
            quest: "Q".into(),
            to: QuestStatus::Success,
        });
        assert_eq!(
            e,
            Some(ScriptEvent::QuestChanged {
                quest: "Q".into(),
                status: "Success".into()
            })
        );
        assert!(event_from_runtime(&Event::Notice("x".into())).is_none());
        let c = command_to_runtime(
            &ScriptCommand::SpawnNpc {
                template: "Chicken".into(),
                at: crate::Vec3::new(1.0, 2.0, 3.0),
                yaw: 0.5,
            },
            |t| {
                Some(NpcRecord {
                    name: t.into(),
                    ..NpcRecord::default()
                })
            },
        );
        assert!(matches!(c, Some(Command::SpawnNpc { record, .. }) if record.name == "Chicken"));
        assert!(
            command_to_runtime(&ScriptCommand::Notice { text: "x".into() }, |_| None).is_none()
        );
    }
}
