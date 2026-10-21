//! Script mods driven by the headless core stub: the example mods, sandbox, budgets,
//! determinism and hot reload.
use gothic_mods_script::{
    Capability, ErrorKind, Issued, ScriptCommand, ScriptConfig, ScriptHost, ScriptManifest,
    StubCore, Vec3,
};
use std::path::{Path, PathBuf};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/examples")
}

fn host(seed: u64) -> ScriptHost {
    ScriptHost::new(ScriptConfig {
        seed,
        ..ScriptConfig::default()
    })
}

/// Run `ticks` core ticks, feeding each tick's events to the host and its commands back.
fn run(core: &mut StubCore, host: &mut ScriptHost, ticks: usize) {
    let mut commands: Vec<Issued> = Vec::new();
    for _ in 0..ticks {
        let events = core.tick(&commands);
        host.set_world(core.world.clone());
        commands = host.handle(&events);
    }
    core.tick(&commands);
}

fn chicken_world() -> StubCore {
    let mut core = StubCore::new();
    core.template("Chicken", "Chicken", "");
    core.world.hero = Vec3::new(1000.0, 200.0, -500.0);
    core
}

fn chicken_rain(seed: u64) -> (StubCore, ScriptHost) {
    let mut h = host(seed);
    h.load_dir(&examples().join("chicken_rain")).unwrap();
    let mut core = chicken_world();
    core.set_quest("Q_Letter", "Running");
    run(&mut core, &mut h, 2);
    assert_eq!(
        core.count_species("Chicken"),
        0,
        "only a success makes it rain"
    );
    core.set_quest("Q_Letter", "Success");
    run(&mut core, &mut h, 2);
    (core, h)
}

#[test]
fn chicken_rain_on_quest_success() {
    let (core, h) = chicken_rain(42);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert_eq!(core.count_species("Chicken"), 12);
    for a in core.world.actors.values() {
        assert!((a.pos.x - 1000.0).abs() <= 600.0);
        assert!((a.pos.z + 500.0).abs() <= 600.0);
        assert!(
            (1700.0..=2500.0).contains(&a.pos.y),
            "falls from above: {}",
            a.pos.y
        );
    }
    assert_eq!(core.notices, ["Q_Letter done - it's raining chickens!"]);
    let state = h.state("chicken_rain").unwrap().clone_cast::<rhai::Map>();
    assert_eq!(state["rains"].as_int().unwrap(), 1);
    assert!(core.applied.iter().all(|c| c.by == "chicken_rain"));
}

#[test]
fn same_seed_same_rain_other_seed_other_rain() {
    let (a, _) = chicken_rain(7);
    let (b, _) = chicken_rain(7);
    let (c, _) = chicken_rain(8);
    assert_eq!(a.applied, b.applied);
    assert_ne!(a.applied, c.applied);
}

#[test]
fn all_orcs_speak_in_rhymes() {
    let mut h = host(1);
    h.load_dir(&examples().join("orc_rhymes")).unwrap();
    let mut core = StubCore::new();
    let orc = core.add_actor("Orc_Warrior", "Orc", "Orcs", Vec3::default());
    let human = core.add_actor("Rebel", "Human", "Rebels", Vec3::default());
    h.set_world(core.world.clone());

    let line = core.say(Some(orc), "ORC_01", "You will die, human!");
    let text = h.rewrite_line(&line).expect("orc line rewritten");
    assert!(
        text == "You will die, human - that is the orcish plan!"
            || text == "You will die, human - run while you can, little man!",
        "{text}"
    );
    let line = core.say(Some(orc), "ORC_02", "Morra want gold.");
    assert!(h.rewrite_line(&line).unwrap().ends_with("old!"));
    let line = core.say(Some(orc), "ORC_03", "Grrr");
    assert!(h.rewrite_line(&line).unwrap().contains("rhyme"));
    // Humans and the hero speak normally.
    let line = core.say(Some(human), "REB_01", "Watch out!");
    assert_eq!(h.rewrite_line(&line), None);
    let line = core.say(None, "HERO_01", "Hello.");
    assert_eq!(h.rewrite_line(&line), None);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let state = h.state("orc_rhymes").unwrap().clone_cast::<rhai::Map>();
    assert_eq!(state["lines"].as_int().unwrap(), 3);
}

#[test]
fn capabilities_are_enforced() {
    let mut h = host(0);
    h.add_source(
        ScriptManifest::new("sneaky", &[Capability::Ui]),
        r#"
        fn on_tick(e) { spawn_npc("Troll", #{x: 0, y: 0, z: 0}); }
        fn rewrite_line(line) { "hacked" }
        "#,
    )
    .unwrap();
    let mut core = StubCore::new();
    let commands = h.handle(&core.tick(&[]));
    assert!(commands.is_empty());
    let e = &h.errors()[0];
    assert_eq!(e.kind, ErrorKind::Runtime);
    assert!(e.message.contains("no capability 'spawn'"), "{}", e.message);
    // No `subtitles` capability: the hook is not called.
    let line = core.say(None, "K", "text");
    assert_eq!(h.rewrite_line(&line), None);
}

#[test]
fn sandbox_has_no_eval_modules_or_time() {
    let mut h = host(0);
    assert!(
        h.add_source(
            ScriptManifest::new("a", &[]),
            "fn on_tick(e) { eval(\"1\") }"
        )
        .is_err()
    );
    h.add_source(
        ScriptManifest::new("b", &[]),
        r#"fn on_tick(e) { import "std/fs" as fs; }"#,
    )
    .unwrap();
    h.add_source(
        ScriptManifest::new("c", &[]),
        "fn on_tick(e) { let t = timestamp(); }",
    )
    .unwrap();
    h.handle(&StubCore::new().tick(&[]));
    let errs = h.take_errors();
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(errs.iter().all(|e| e.kind == ErrorKind::Runtime));
    assert_eq!(errs[0].mod_id, "b");
    assert_eq!(errs[1].mod_id, "c");
}

#[test]
fn budgets_stop_runaway_mods_and_others_keep_running() {
    let mut h = host(0);
    h.add_source(
        ScriptManifest::new("busy", &[Capability::Ui]),
        "fn on_tick(e) { loop { } }",
    )
    .unwrap();
    h.add_source(
        ScriptManifest::new("spammer", &[Capability::Ui]),
        r#"fn on_tick(e) { for i in 0..100 { notice("hi"); } }"#,
    )
    .unwrap();
    h.add_source(
        ScriptManifest::new("good", &[Capability::Ui]),
        r#"fn on_tick(e) { notice("tick"); }"#,
    )
    .unwrap();
    let mut core = StubCore::new();
    let first = h.handle(&core.tick(&[]));
    let spam = first.iter().filter(|c| c.by == "spammer").count();
    assert_eq!(spam, h.config().commands_per_tick);
    assert!(first.iter().any(|c| c.by == "good"));
    for _ in 0..3 {
        let out = h.handle(&core.tick(&[]));
        assert!(
            out.contains(
                &ScriptCommand::Notice {
                    text: "tick".into()
                }
                .from_mod("good")
            )
        );
    }
    let errs = h.errors();
    assert!(
        errs.iter()
            .any(|e| e.mod_id == "busy" && e.kind == ErrorKind::Budget)
    );
    assert!(h.is_disabled("busy"));
    assert!(h.is_disabled("spammer"));
    assert!(!h.is_disabled("good"));
    assert!(
        errs.iter()
            .any(|e| e.mod_id == "busy" && e.kind == ErrorKind::Disabled)
    );
}

#[test]
fn hot_reload_keeps_state_and_survives_broken_edits() {
    let dir = std::env::temp_dir().join(format!("gr-mods-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mod.toml"),
        "[mod]\nid = \"counter\"\nversion = \"1.0.0\"\n[script]\ncapabilities = [\"ui\"]\n",
    )
    .unwrap();
    let v1 = "fn on_load() { this.n = 0; }\nfn on_quest_changed(e) { this.n += 1; notice(`v1 ${this.n}`); }\n";
    std::fs::write(dir.join("main.rhai"), v1).unwrap();
    let mut h = host(0);
    h.load_dir(&dir).unwrap();
    let mut core = StubCore::new();
    core.set_quest("A", "Running");
    run(&mut core, &mut h, 1);
    assert!(h.poll_reload().is_empty());

    let v2 = "fn on_reload() { this.reloads = 1; }\nfn on_quest_changed(e) { this.n += 1; notice(`v2 ${this.n}`); }\n";
    std::fs::write(dir.join("main.rhai"), v2).unwrap();
    assert_eq!(h.poll_reload(), ["counter"]);
    core.set_quest("B", "Running");
    run(&mut core, &mut h, 1);

    std::fs::write(dir.join("main.rhai"), "fn on_quest_changed(e) { ").unwrap();
    assert!(h.poll_reload().is_empty());
    assert_eq!(h.take_errors()[0].kind, ErrorKind::Compile);
    core.set_quest("C", "Running");
    run(&mut core, &mut h, 1);

    assert_eq!(core.notices, ["v1 1", "v2 2", "v2 3"]);
    let state = h.state("counter").unwrap().clone_cast::<rhai::Map>();
    assert_eq!(state["reloads"].as_int().unwrap(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reload_re_enables_a_disabled_mod() {
    let dir = std::env::temp_dir().join(format!("gr-mods-script-re-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("broken")).unwrap();
    std::fs::write(
        dir.join("broken/mod.toml"),
        "[mod]\nid = \"broken\"\nversion = \"1.0.0\"\n[script]\ncapabilities = [\"ui\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("broken/main.rhai"), "fn on_tick(e) { 1 / 0 }").unwrap();
    // A folder without [script] is a data-only mod and is skipped.
    std::fs::create_dir_all(dir.join("data_only")).unwrap();
    std::fs::write(
        dir.join("data_only/mod.toml"),
        "[mod]\nid = \"data_only\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let mut h = host(0);
    assert_eq!(h.discover(&dir).unwrap(), ["broken"]);
    let mut core = StubCore::new();
    run(&mut core, &mut h, 4);
    assert!(h.is_disabled("broken"));
    std::fs::write(
        dir.join("broken/main.rhai"),
        "fn on_tick(e) { notice(\"fixed\"); }",
    )
    .unwrap();
    assert_eq!(h.poll_reload(), ["broken"]);
    assert!(!h.is_disabled("broken"));
    run(&mut core, &mut h, 1);
    assert_eq!(core.notices, ["fixed"]);
    std::fs::remove_dir_all(&dir).unwrap();
}
