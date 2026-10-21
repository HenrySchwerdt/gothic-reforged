//! Synthetic-fixture tests: no original game data involved.
use gothic_formats::{info::Info, quest::Quest, stringtable::StringTable};
use gothic_gameplay::conditions::Unmet;
use gothic_gameplay::lint::lint;
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::script::DiagKind;
use gothic_gameplay::transcript::run_script;
use gothic_gameplay::{Assumptions, Engine, GameDatabase, GameState};

type Step<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a str);

#[allow(clippy::too_many_arguments)]
fn info(
    name: &str,
    owner: &str,
    parent: &str,
    quest: &str,
    ct: u32,
    ty: u32,
    knows: &str,
    extra: &str,
    script: &[Step],
) -> Info {
    let col = |f: &dyn Fn(&Step<'_>) -> String| {
        let mut s = script.iter().map(f).collect::<Vec<_>>().join(";");
        s.push(';');
        s
    };
    let text = format!(
        "[Info]\r\nName={name}\r\nSortID={}\r\nOwner={owner}\r\nInfoGiven=false\r\nParent={parent}\r\nQuest={quest}\r\nConditionType={ct}\r\nInfoType={ty}\r\nCondPlayerKnows={knows}\r\n{extra}InfoScript_Commands={}\r\nInfoScript_Entities1={}\r\nInfoScript_Entities2={}\r\nInfoScript_IDs1={}\r\nInfoScript_IDs2={}\r\nInfoScript_Texts={}\r\n",
        name.len() * 10 + script.len(),
        script.iter().map(|s| s.0).collect::<Vec<_>>().join(";"),
        col(&|s| s.1.to_string()),
        col(&|s| s.2.to_string()),
        col(&|s| s.3.to_string()),
        col(&|s| s.4.to_string()),
        col(&|s| s.5.to_string()),
    );
    Info::parse(text.as_bytes()).unwrap()
}

fn quest(
    name: &str,
    ty: u32,
    xp: u32,
    finished: &str,
    delivery: &str,
    amounts: &str,
    dest: &str,
) -> Quest {
    let text = format!(
        "[Quest]\r\nName={name}\r\nType={ty}\r\nFinishedQuests={finished}\r\nLogTopic=QUEST_{name}\r\nExperiencePoints={xp}\r\nDeliveryEntities={delivery}\r\nDeliveryAmounts={amounts}\r\nDeliveryCounter\r\nDestinationEntity={dest}\r\n"
    );
    Quest::parse(text.as_bytes()).unwrap()
}

fn strings() -> StringTable {
    let mut keys = String::new();
    for (k, en, de) in [
        ("QUEST_Q1", "Deliver the letter", "Bring den Brief"),
        ("QUEST_Q2", "Second", "Zweite"),
        ("INFO_HELLO", "Hello", "Hallo"),
        ("INFO_ASK", "Any work?", "Gibt es Arbeit?"),
        (
            "INFO_WORK",
            "Yes, take this letter to Bob.",
            "Ja, bring diesen Brief zu Bob.",
        ),
        ("INFO_DONE", "Here is the letter.", "Hier ist der Brief."),
        ("INFO_THANKS", "Thanks!", "Danke!"),
        ("INFO_BACK", "(Back)", "(Zurueck)"),
    ] {
        keys.push_str(&format!("{k}={en};;{de};\r\n"));
    }
    let text = format!(
        "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n{keys}[LocAdmin_Revisions]\r\n"
    );
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    StringTable::parse(&b).unwrap()
}

fn db() -> GameDatabase {
    let quests = vec![
        quest("Q1", 1, 100, "", "Bob", "1", ""),
        quest("Q2", 2, 0, "Q1", "Orc", "2", ""),
    ];
    let infos = vec![
        info(
            "A1",
            "Alice",
            "",
            "",
            3,
            3,
            "",
            "",
            &[
                ("Say", "player", "", "", "", "INFO_ASK"),
                ("Say", "npc", "", "", "", "INFO_WORK"),
                ("SetGameEvent", "", "", "Alice_Offer", "", ""),
            ],
        ),
        info(
            "A2",
            "Alice",
            "",
            "Q1",
            6,
            3,
            "Alice_Offer",
            "",
            &[
                ("Say", "player", "", "", "", "INFO_THANKS"),
                ("Give", "npc", "player", "It_Letter", "1", ""),
            ],
        ),
        info(
            "B1",
            "Bob",
            "",
            "Q1",
            8,
            3,
            "",
            "",
            &[
                ("Say", "player", "", "", "", "INFO_DONE"),
                ("GiveXP", "", "", "10", "", ""),
            ],
        ),
        info(
            "A3",
            "Alice",
            "",
            "Q1",
            10,
            3,
            "",
            "",
            &[("Say", "npc", "", "", "", "INFO_THANKS")],
        ),
        info(
            "A4",
            "Alice",
            "",
            "Q2",
            6,
            3,
            "",
            "",
            &[("Frobnicate", "", "", "", "", "")],
        ),
        // Dead: event never set by anyone.
        info(
            "A5",
            "Alice",
            "",
            "",
            3,
            3,
            "Never_Set",
            "",
            &[("Say", "npc", "", "", "", "INFO_HELLO")],
        ),
        // Tree: parent with two children, one clearing siblings.
        info(
            "T0",
            "Tom",
            "",
            "",
            3,
            4,
            "",
            "",
            &[("Say", "player", "", "", "", "INFO_ASK")],
        ),
        info(
            "T1",
            "Tom",
            "T0",
            "",
            3,
            3,
            "",
            "ClearChildren=true\r\n",
            &[("Say", "player", "", "", "", "INFO_HELLO")],
        ),
        info(
            "T2",
            "Tom",
            "T0",
            "",
            3,
            3,
            "",
            "",
            &[("Say", "player", "", "", "", "INFO_THANKS")],
        ),
    ];
    GameDatabase::from_parts(quests, infos, strings(), "German").unwrap()
}

fn id(db: &GameDatabase, n: &str) -> gothic_gameplay::InfoId {
    db.info_id(n).unwrap()
}

#[test]
fn availability_follows_events_and_given_state() {
    let db = db();
    let eng = Engine::with(&db, Assumptions::STRICT);
    let mut st = GameState::new();
    assert_eq!(
        eng.check(&st, id(&db, "A2"), "Alice"),
        Err(Unmet::Event("Alice_Offer".into()))
    );
    let r = eng.execute(&mut st, id(&db, "A1"), "Alice");
    assert!(r.diagnostics.is_empty());
    assert!(st.knows("alice_offer"));
    // A1 is not permanent: given once.
    assert_eq!(
        eng.check(&st, id(&db, "A1"), "Alice"),
        Err(Unmet::AlreadyGiven)
    );
    assert!(eng.check(&st, id(&db, "A2"), "Alice").is_ok());
}

#[test]
fn quest_lifecycle_pays_rewards_and_unlocks_followups() {
    let db = db();
    let eng = Engine::with(&db, Assumptions::STRICT);
    let mut st = GameState::new();
    eng.execute(&mut st, id(&db, "A1"), "Alice");
    // Q1 is Open; Delivery/Success are not yet available.
    assert!(eng.check(&st, id(&db, "B1"), "Bob").is_err());
    eng.execute(&mut st, id(&db, "A2"), "Alice");
    assert_eq!(st.quest_status("Q1"), QuestStatus::Running);
    assert_eq!(st.item_count("It_Letter"), 1);
    assert!(
        eng.check(&st, id(&db, "A3"), "Alice").is_err(),
        "Success needs status Success"
    );
    // Report quest: delivering to Bob counts his counter and succeeds.
    assert!(eng.check(&st, id(&db, "B1"), "Bob").is_ok());
    let r = eng.execute(&mut st, id(&db, "B1"), "Bob");
    assert_eq!(st.quest_status("Q1"), QuestStatus::Success);
    // Quest XP 100 x5 plus GiveXP 10 x5.
    assert_eq!(st.xp, 550);
    assert_eq!(st.level, 1);
    assert!(r.outputs.iter().any(|o| matches!(
        o,
        gothic_gameplay::script::Output::QuestChanged {
            to: QuestStatus::Success,
            ..
        }
    )));
    assert!(eng.check(&st, id(&db, "A3"), "Alice").is_ok());
}

#[test]
fn prerequisite_quest_gates_activation() {
    let db = db();
    let eng = Engine::with(&db, Assumptions::STRICT);
    let mut st = GameState::new();
    assert_eq!(
        eng.check(&st, id(&db, "A4"), "Alice"),
        Err(Unmet::PrerequisiteQuest("Q1".into()))
    );
    st.set_quest_status("Q1", QuestStatus::Success);
    assert!(eng.check(&st, id(&db, "A4"), "Alice").is_ok());
    // Lost is not "finished".
    st.set_quest_status("Q1", QuestStatus::Lost);
    assert!(eng.check(&st, id(&db, "A4"), "Alice").is_err());
}

#[test]
fn unknown_commands_are_reported_not_ignored() {
    let db = db();
    let eng = Engine::with(&db, Assumptions::STRICT);
    let mut st = GameState::new();
    st.set_quest_status("Q1", QuestStatus::Success);
    let r = eng.execute(&mut st, id(&db, "A4"), "Alice");
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.kind == DiagKind::UnknownCommand && d.command == "Frobnicate")
    );
    assert_eq!(st.quest_status("Q2"), QuestStatus::Running);
}

#[test]
fn save_round_trip_preserves_everything() {
    let db = db();
    let eng = Engine::with(&db, Assumptions::STRICT);
    let mut st = GameState::new();
    eng.execute(&mut st, id(&db, "A1"), "Alice");
    eng.execute(&mut st, id(&db, "A2"), "Alice");
    let json = st.to_json().unwrap();
    let back = GameState::from_json(&json).unwrap();
    assert_eq!(back, st);
    assert_eq!(back.to_json().unwrap(), json);
}

#[test]
fn dialog_tree_navigation_and_german_text() {
    let db = db();
    let eng = Engine::new(&db);
    let mut st = GameState::new();
    let out = run_script(&eng, &mut st, "Tom", &["Hallo"]);
    assert!(out.contains("Du: Gibt es Arbeit?"), "{out}");
    assert!(out.contains("Du: Hallo"), "{out}");
    // A lone root entry (the Parent info T0) runs automatically [R 1.5].
    // T1 cleared its siblings, so T2 never shows and the conversation returns to root.
    assert!(st.told.contains("t2"));
    let out_en = {
        let db_en = GameDatabase::from_parts(
            vec![],
            vec![info(
                "E1",
                "Eve",
                "",
                "",
                3,
                3,
                "",
                "",
                &[("Say", "npc", "", "", "", "INFO_HELLO")],
            )],
            strings(),
            "English",
        )
        .unwrap();
        let eng = Engine::new(&db_en);
        let mut st = GameState::new();
        run_script(&eng, &mut st, "Eve", &[])
    };
    assert!(out_en.contains("Eve: Hello"), "{out_en}");
}

#[test]
fn lint_finds_dead_events_and_completable_quests() {
    let db = db();
    let r = lint(&db);
    // Q1: Alice offers, activator, Bob delivers. Q2: prerequisite Q1 finishes.
    let q1 = r.quests.iter().find(|q| q.name == "Q1").unwrap();
    assert_eq!(q1.verdict, "completable");
    assert!(
        r.dead_events
            .iter()
            .any(|e| e.event == "never_set" && e.kind == "never_set")
    );
    assert!(r.unreachable_infos.iter().any(|u| u.info == "A5"));
    assert_eq!(r.coverage.distinct_commands_unknown, 1, "Frobnicate");
    assert_eq!(r.coverage.unknown_command_uses, 1);
}

#[test]
fn engine_hooks_complete_kill_quests() {
    let db = db();
    let mut st = GameState::new();
    st.set_quest_status("Q2", QuestStatus::Running);
    let mut sink = Default::default();
    gothic_gameplay::questmgr::on_npc_killed(&db, &mut st, "Orc", &mut sink);
    assert_eq!(st.quest_status("Q2"), QuestStatus::Running, "needs 2 kills");
    gothic_gameplay::questmgr::on_npc_killed(&db, &mut st, "Orc", &mut sink);
    assert_eq!(st.quest_status("Q2"), QuestStatus::Success);
}

#[test]
fn friend_lines_need_fame_of_75() {
    // `EnclaveFriend` (24) / `PoliticalFriend` (25): `gCInfo_PS::Execute` refuses below 75
    // (GAME_RULES 1.6); the NPC's enclave is unknown, so any enclave counts.
    let infos = vec![
        info("F1", "Fay", "", "", 24, 3, "", "", &[]),
        info("F2", "Fay", "", "", 25, 3, "", "", &[]),
    ];
    let db = GameDatabase::from_parts(vec![], infos, strings(), "German").unwrap();
    let eng = Engine::with(&db, Assumptions::PERMISSIVE);
    let mut st = GameState::new();
    assert!(matches!(
        eng.check(&st, id(&db, "F1"), "Fay"),
        Err(Unmet::Flag(_))
    ));
    st.add_fame("enc:ishtar", 74);
    assert!(eng.check(&st, id(&db, "F1"), "Fay").is_err());
    st.add_fame("enc:ishtar", 1);
    assert!(eng.check(&st, id(&db, "F1"), "Fay").is_ok());
    // The political line looks at the political fame only.
    assert!(eng.check(&st, id(&db, "F2"), "Fay").is_err());
    st.add_fame("pol:rebels", 80);
    assert!(eng.check(&st, id(&db, "F2"), "Fay").is_ok());
    // The optimistic profile (linter) assumes the reputation.
    let opt = Engine::with(&db, Assumptions::OPTIMISTIC);
    assert!(opt.check(&GameState::new(), id(&db, "F1"), "Fay").is_ok());
}
