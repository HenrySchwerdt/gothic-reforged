//! The bot against the synthetic quest database.
use gothic_gameplay::model::QuestStatus;
use gothic_questbot::report::{Action, Cheat};
use gothic_questbot::{BotOptions, StuckReason, fixture, play};

#[test]
fn completes_every_fixture_quest() {
    let db = fixture::database();
    let report = play(&db, BotOptions::default());
    assert!(report.all_completed(), "{}", report.to_text(true));
    assert_eq!(report.completed.len(), fixture::QUESTS.len());
    assert!(report.stuck.is_empty() && report.unsuccessful.is_empty());
    // Every quest went Open -> Running -> Success, in an order that respects prerequisites.
    let finished_at = |q: &str| {
        report
            .trace
            .iter()
            .find(|s| {
                s.quest_changes
                    .iter()
                    .any(|c| c.quest.eq_ignore_ascii_case(q) && c.to == QuestStatus::Success)
            })
            .map(|s| s.step)
            .unwrap_or_else(|| panic!("{q} never succeeded"))
    };
    for (later, earlier) in [
        ("Q_Herbs", "Q_Letter"),
        ("Q_Arena", "Q_Wolves"),
        ("Q_Escort", "Q_Scout"),
        ("Q_Ring", "Q_Herbs"),
        ("Q_Ring", "Q_Scout"),
        ("Q_Tribute", "Q_Arena"),
        ("Q_Final", "Q_Ring"),
        ("Q_Final", "Q_Mira"),
    ] {
        assert!(
            finished_at(earlier) < finished_at(later),
            "{earlier} before {later}"
        );
    }
}

#[test]
fn objectives_outside_dialogs_are_cheated_and_recorded() {
    let report = play(&fixture::database(), BotOptions::default());
    let cheats: Vec<&Cheat> = report
        .trace
        .iter()
        .filter_map(|s| match &s.action {
            Action::Cheat(c) => Some(c),
            _ => None,
        })
        .collect();
    assert!(
        cheats
            .iter()
            .any(|c| matches!(c, Cheat::GiveItems { item, amount: 3, .. } if item == "It_Herb"))
    );
    assert_eq!(
        cheats
            .iter()
            .filter(|c| matches!(c, Cheat::Kill { npc, .. } if npc == "Wolf"))
            .count(),
        2
    );
    assert!(
        cheats
            .iter()
            .any(|c| matches!(c, Cheat::Defeat { npc, .. } if npc == "Brutus"))
    );
    assert!(
        cheats
            .iter()
            .any(|c| matches!(c, Cheat::Enter { area, .. } if area == "Area_Pass"))
    );
}

#[test]
fn dialog_trees_are_explored() {
    let report = play(&fixture::database(), BotOptions::default());
    let talked = |info: &str| {
        report
            .trace
            .iter()
            .any(|s| matches!(&s.action, Action::Talk { ran, .. } if ran.iter().any(|r| r == info)))
    };
    assert!(
        talked("Mi_Story"),
        "the story child sets the event the quest needs"
    );
    assert!(talked("Mi_Help"));
}

#[test]
fn targets_limit_the_work() {
    let options = BotOptions {
        targets: Some(vec!["Q_Letter".into()]),
        ..BotOptions::default()
    };
    let report = play(&fixture::database(), options);
    assert!(report.all_completed());
    assert_eq!(report.targets, 1);
    // Wolves, arena etc. were not touched.
    assert!(
        !report
            .trace
            .iter()
            .any(|s| matches!(&s.action, Action::Cheat(_)))
    );
}

#[test]
fn stuck_quests_are_reported_with_reasons() {
    let report = play(&fixture::broken(), BotOptions::default());
    assert_eq!(report.completed.len(), fixture::QUESTS.len());
    assert_eq!(report.stuck.len(), 2, "{}", report.to_text(false));
    let lost = report.stuck.iter().find(|s| s.quest == "Q_Lost").unwrap();
    assert_eq!(lost.reason, StuckReason::NotActivated);
    assert_eq!(lost.status, QuestStatus::Open);
    let b = &lost.blockers[0];
    assert_eq!(b.info, "H_Lost");
    assert!(b.unmet.contains("Never_Set"), "{}", b.unmet);
    assert_eq!(b.hint.as_deref(), Some("no info sets game event Never_Set"));
    let locked = report.stuck.iter().find(|s| s.quest == "Q_Locked").unwrap();
    assert_eq!(locked.reason, StuckReason::PrerequisiteUnfinished);
    assert_eq!(locked.prerequisites, vec!["Q_Lost".to_string()]);
    // The text and JSON forms carry the same facts.
    let text = report.to_text(false);
    assert!(text.contains("STUCK Q_Lost [Open] NotActivated"), "{text}");
    let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
    assert_eq!(json["schema"], "gothic-questbot/1");
    assert_eq!(json["stuck"][0]["reason"], "not_activated");
}

#[test]
fn deterministic() {
    let a = play(&fixture::database(), BotOptions::default()).to_json();
    let b = play(&fixture::database(), BotOptions::default()).to_json();
    assert_eq!(a, b);
}

#[test]
fn a_permanent_refusal_does_not_stall_the_run() {
    // Old behaviour: the refusal outscored everything, the bot talked to Grump every round,
    // nothing changed twice and the run ended with almost every quest open.
    let report = play(&fixture::with_refusal(), BotOptions::default());
    assert!(report.all_completed(), "{}", report.to_text(true));
    let grump = report
        .trace
        .iter()
        .filter(
            |s| matches!(&s.action, Action::Talk { npc, .. } if npc.eq_ignore_ascii_case("Grump")),
        )
        .count();
    // Visited, found futile, and only revisited after real progress elsewhere.
    assert!(
        grump >= 1 && grump <= fixture::QUESTS.len() * 4,
        "{grump} talks to Grump"
    );
}

#[test]
fn stuck_runs_still_end_after_waiting_once() {
    let report = play(&fixture::broken(), BotOptions::default());
    assert!(!report.all_completed());
    assert!(report.rounds < 200, "{} rounds", report.rounds);
}

#[test]
fn stuck_report_explains_why_an_event_setter_did_not_run() {
    let report = play(&fixture::gated(), BotOptions::default());
    let gated = report.stuck.iter().find(|s| s.quest == "Q_Gated").unwrap();
    let hint = gated.blockers[0].hint.as_deref().unwrap_or_default();
    assert!(hint.contains("Gate_Open"), "{hint}");
    assert!(hint.contains("X_Gate @Xan"), "{hint}");
    assert!(hint.contains("Never_Set"), "{hint}");
}
