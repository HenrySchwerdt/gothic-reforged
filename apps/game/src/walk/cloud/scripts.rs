//! Script mods (`gothic-mods-script`) hosted in the play mode. Every frame, after the fixed
//! core ticks and before the presentation reads them, the core's events go to the scripts:
//!
//! * `rewrite_line` (capability `subtitles`) replaces the text of `LineSpoken` events in
//!   place, so the dialog box shows the rewritten line (the voice key is unchanged);
//! * the events `gothic_mods_script::runtime::event_from_runtime` maps are dispatched, after one
//!   `on_tick` with the frame's time;
//! * the commands come back as core commands (`GiveHero`, `GiveNpc`) or toasts (`Notice`).
//!
//! `SpawnNpc` is **not** forwarded: the play mode binds every `ActorSpawned` event to the next
//! body waiting in its roster (FIFO), so a core actor spawned by a script would take another
//! character's body. Such commands are dropped with a warning.
use bevy::app::RunFixedMainLoopSystems;
use bevy::ecs::message::MessageMutator;
use bevy::prelude::*;
use gothic_mods_script::runtime::{command_to_runtime, event_from_runtime};
use gothic_mods_script::{Issued, Line, ScriptCommand, ScriptConfig, ScriptEvent, ScriptHost};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::world::WorldQuery;
use gothic_ui::{Notify, ToastKind};
use std::path::Path;

/// The script host of the play mode.
#[derive(Resource)]
pub(crate) struct Scripts {
    pub host: ScriptHost,
    /// Mods already warned about a dropped `SpawnNpc`.
    warned_spawn: Vec<String>,
}

/// Seed of the core in `play::install`; the scripts' RNG streams derive from it.
const WORLD_SEED: u64 = 0x6074_1C20;

/// Load the script mods among the enabled mods of `dir` (load order).
pub(crate) fn load(dir: &Path) -> anyhow::Result<ScriptHost> {
    let mut host = ScriptHost::new(ScriptConfig {
        seed: WORLD_SEED,
        ..Default::default()
    });
    for m in super::mods::load_order(dir)?.order {
        let text = std::fs::read_to_string(m.dir.join("mod.toml")).unwrap_or_default();
        let has_script =
            gothic_mods_script::ScriptManifest::parse(&text).is_ok_and(|s| s.script.is_some());
        if has_script && let Err(e) = host.load_dir(&m.dir) {
            eprintln!("mods: script mod {}: {e:#}", m.id());
        }
    }
    Ok(host)
}

pub(crate) fn install(app: &mut App, dir: &Path) {
    let host = match load(dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("mods: {e:#}; no script mods");
            return;
        }
    };
    if host.mods().is_empty() {
        eprintln!("mods: no enabled script mods in {}", dir.display());
        return;
    }
    eprintln!("mods: script mods {:?}", host.mods());
    app.insert_resource(Scripts {
        host,
        warned_spawn: Vec::new(),
    })
    .add_systems(
        RunFixedMainLoop,
        run_scripts.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
    );
}

/// The world snapshot scripts read (`world` capability): hero, actors, quests, game hours.
/// Positions are the core's (mirrored, origin-relative centimetres).
fn world_view(core: &gothic_runtime::Core) -> gothic_mods_script::WorldView {
    let sim = &core.sim;
    let pos = |id| {
        sim.world
            .position(id)
            .map(|p| gothic_mods_script::Vec3::new(p.x, p.y, p.z))
            .unwrap_or_default()
    };
    gothic_mods_script::WorldView {
        hero: pos(core.hero),
        actors: sim
            .actors
            .iter()
            .map(|a| {
                (
                    a.id.0,
                    gothic_mods_script::ActorInfo {
                        id: a.id.0,
                        name: a.name.clone(),
                        species: format!("{:?}", a.species),
                        guild: a.enclave.clone().unwrap_or_default(),
                        pos: pos(a.id),
                        alive: a.is_alive(),
                    },
                )
            })
            .collect(),
        quests: sim
            .state
            .quests
            .iter()
            .filter_map(|(name, q)| Some((name.clone(), format!("{:?}", q.status?))))
            .collect(),
        hours: sim.clock.game_ms as f64 / 3_600_000.0,
    }
}

/// Run the subtitle hook on a core event (in place) and map it to its script event.
pub(crate) fn rewrite_and_map(host: &mut ScriptHost, ev: &mut Event) -> Option<ScriptEvent> {
    if let Event::LineSpoken {
        speaker,
        key,
        text,
        player,
        actor,
    } = ev
    {
        let line = Line {
            actor: actor.map(|a| a.0),
            speaker: speaker.clone(),
            key: key.clone(),
            text: text.clone().unwrap_or_default(),
            player: *player,
        };
        if let Some(new) = host.rewrite_line(&line) {
            *text = Some(new);
        }
    }
    event_from_runtime(ev)
}

/// What becomes of a script command in the play mode.
#[derive(Debug)]
pub(crate) enum Outcome {
    Core(Command),
    Toast(String),
    /// Not supported here (see the module docs) or unknown template.
    Dropped(&'static str),
}

pub(crate) fn route(issued: &Issued) -> Outcome {
    match &issued.command {
        ScriptCommand::Notice { text } => Outcome::Toast(text.clone()),
        ScriptCommand::SpawnNpc { .. } => {
            Outcome::Dropped("SpawnNpc is not supported in the play mode (roster binding)")
        }
        c => match command_to_runtime(c, |_| None) {
            Some(cmd) => Outcome::Core(cmd),
            None => Outcome::Dropped("no core command"),
        },
    }
}

fn run_scripts(
    mut scripts: ResMut<Scripts>,
    core: Res<CoreRes>,
    mut events: MessageMutator<CoreEvent>,
    mut commands: MessageWriter<CoreCommand>,
    mut notify: MessageWriter<Notify>,
    time: Res<Time>,
) {
    let scripts = &mut *scripts;
    scripts.host.set_world(world_view(&core.0));
    let mut batch = vec![ScriptEvent::Tick {
        dt: time.delta_secs(),
    }];
    for e in events.read() {
        if let Some(se) = rewrite_and_map(&mut scripts.host, &mut e.0) {
            batch.push(se);
        }
    }
    for issued in scripts.host.handle(&batch) {
        match route(&issued) {
            Outcome::Core(c) => {
                commands.write(CoreCommand(c));
            }
            Outcome::Toast(text) => {
                notify.write(Notify {
                    text,
                    kind: ToastKind::Info,
                });
            }
            Outcome::Dropped(why) => {
                if !scripts.warned_spawn.contains(&issued.by) {
                    eprintln!("mods: {}: {why}; command dropped", issued.by);
                    scripts.warned_spawn.push(issued.by.clone());
                }
            }
        }
    }
    for e in scripts.host.take_errors() {
        eprintln!("mods: script error {e:?}");
    }
    for l in scripts.host.take_log() {
        eprintln!("mods: {l:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_mods_script::{Capability, ScriptManifest};
    use gothic_sim::types::ActorId;

    fn examples() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/examples")
    }

    #[test]
    fn cloud_scripts_load_enabled_examples() {
        let host = load(&examples()).unwrap();
        let mods = host.mods();
        assert!(mods.contains(&"chicken_rain"), "{mods:?}");
        assert!(mods.contains(&"orc_rhymes"), "{mods:?}");
        assert!(
            !mods.contains(&"forest_hermit"),
            "data-only mod has no script"
        );
    }

    #[test]
    fn cloud_scripts_rewrite_line_in_place() {
        let mut host = ScriptHost::new(ScriptConfig::default());
        host.add_source(
            ScriptManifest::new("shout", &[Capability::Subtitles]),
            "fn rewrite_line(line) { if line.player { return; } line.text.to_upper() }",
        )
        .unwrap();
        let mut ev = Event::LineSpoken {
            speaker: "Bob".into(),
            key: "B_HELLO".into(),
            text: Some("hello".into()),
            player: false,
            actor: Some(ActorId(7)),
        };
        let mapped = rewrite_and_map(&mut host, &mut ev).unwrap();
        assert!(
            matches!(&ev, Event::LineSpoken { text: Some(t), key, .. } if t == "HELLO" && key == "B_HELLO")
        );
        assert!(matches!(mapped, ScriptEvent::LineSpoken(l) if l.text == "HELLO"));
        let mut hero = Event::LineSpoken {
            speaker: "Du".into(),
            key: "P_HELLO".into(),
            text: Some("hi".into()),
            player: true,
            actor: None,
        };
        rewrite_and_map(&mut host, &mut hero);
        assert!(matches!(&hero, Event::LineSpoken { text: Some(t), .. } if t == "hi"));
    }

    /// The Bevy wiring: a core event written before the frame is rewritten in place before
    /// `Update` reads it, and a notice comes out as a toast.
    #[test]
    fn cloud_scripts_system_in_app() {
        let mut host = ScriptHost::new(ScriptConfig::default());
        host.add_source(
            ScriptManifest::new("loud", &[Capability::Subtitles, Capability::Ui]),
            r#"fn rewrite_line(line) { line.text + "!" }
               fn on_line_spoken(e) { notice(e.text); }"#,
        )
        .unwrap();
        let db = gothic_gameplay::GameDatabase::from_parts(
            Vec::new(),
            Vec::new(),
            super::super::fixtures::strings(),
            "German",
        )
        .unwrap();
        let core = gothic_runtime::Core::new(
            std::sync::Arc::new(db),
            gothic_sim::item::ItemDb::default(),
            gothic_gameplay::GameState::new(),
            1,
        );
        #[derive(Resource, Default)]
        struct Seen(Vec<String>, Vec<String>);
        fn observe(
            mut seen: ResMut<Seen>,
            mut ev: MessageReader<CoreEvent>,
            mut toasts: MessageReader<Notify>,
        ) {
            for e in ev.read() {
                if let Event::LineSpoken { text: Some(t), .. } = &e.0 {
                    seen.0.push(t.clone());
                }
            }
            seen.1.extend(toasts.read().map(|n| n.text.clone()));
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<CoreEvent>()
            .add_message::<CoreCommand>()
            .add_message::<Notify>()
            .init_resource::<Seen>()
            .insert_resource(CoreRes(core))
            .insert_resource(Scripts {
                host,
                warned_spawn: Vec::new(),
            })
            .add_systems(
                RunFixedMainLoop,
                run_scripts.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            )
            .add_systems(Update, observe);
        app.update();
        app.world_mut().write_message(CoreEvent(Event::LineSpoken {
            speaker: "Bob".into(),
            key: "B_HELLO".into(),
            text: Some("Hello".into()),
            player: false,
            actor: None,
        }));
        app.update();
        app.update();
        let seen = app.world().resource::<Seen>();
        assert_eq!(seen.0, ["Hello!"]);
        assert_eq!(seen.1, ["Hello!"]);
    }

    #[test]
    fn cloud_scripts_route_commands() {
        let mut host = ScriptHost::new(ScriptConfig::default());
        host.add_source(
            ScriptManifest::new(
                "gifts",
                &[
                    Capability::Ui,
                    Capability::Items,
                    Capability::Spawn,
                    Capability::World,
                ],
            ),
            r#"fn on_quest_changed(e) {
                notice(`${e.quest} ${e.status}`);
                give_hero("It_Apple", 2);
                spawn_npc("Chicken", hero_pos());
            }"#,
        )
        .unwrap();
        let mut ev = Event::QuestChanged {
            quest: "Q_Letter".into(),
            to: gothic_gameplay::model::QuestStatus::Success,
        };
        let se = rewrite_and_map(&mut host, &mut ev).unwrap();
        let out: Vec<Outcome> = host.handle(&[se]).iter().map(route).collect();
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(matches!(&out[0], Outcome::Toast(t) if t == "Q_Letter Success"));
        assert!(matches!(
            &out[1],
            Outcome::Core(Command::GiveHero { template, amount: 2 }) if template == "It_Apple"
        ));
        assert!(matches!(out[2], Outcome::Dropped(_)));
    }
}
