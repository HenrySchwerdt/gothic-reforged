//! Integration test over the real Gothic 3 install. Ignored by default:
//! `cargo test -p gothic-gameplay --test real_install -- --ignored`.
//! Set `GOTHIC3_DATA` to override the default Data directory.
use gothic_gameplay::lint::{lint, simulate};
use gothic_gameplay::transcript::run_script;
use gothic_gameplay::{Engine, GameDatabase, GameState};
use std::path::PathBuf;

fn data() -> PathBuf {
    PathBuf::from(
        std::env::var("GOTHIC3_DATA")
            .unwrap_or_else(|_| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into()),
    )
}

#[test]
#[ignore = "needs the original game install"]
fn whole_game_lints_and_plays() {
    let db = GameDatabase::load_install(&data(), "German").unwrap();
    assert_eq!(db.quests.len(), 570);
    assert_eq!(db.infos.len(), 4591);
    let r = lint(&db);
    // Every condition type and command in the data is understood.
    assert_eq!(r.coverage.distinct_conditions_unknown, 0);
    assert_eq!(r.coverage.distinct_commands_unknown, 0);
    assert!(r.quests_completable >= 540, "{}", r.quests_completable);
    let (sim, st) = simulate(&db);
    assert!(
        sim.quests_success.len() >= 400,
        "{}",
        sim.quests_success.len()
    );
    assert_eq!(GameState::from_json(&st.to_json().unwrap()).unwrap(), st);
}

#[test]
#[ignore = "needs the original game install"]
fn kerth_talks_german() {
    let db = GameDatabase::load_install(&data(), "German").unwrap();
    let eng = Engine::new(&db);
    let mut st = GameState::new();
    let out = run_script(&eng, &mut st, "Kerth", &["Was steht euch bevor"]);
    assert!(out.contains("Innos sei mit dir!"), "{out}");
    assert!(
        out.contains("Der Kampf um das Überleben der Clans"),
        "{out}"
    );
    assert!(st.knows("Kerth_FightOrks"));
}
