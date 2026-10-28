//! The companion commands of the player: `SendPerceptionCmdAttack` / `SendPerceptionCmdFollow`.
mod common;
use common::*;
use gothic_script::host::EntityId;
use gothic_sim::actor::Actor;
use gothic_sim::brain::Task;
use gothic_sim::config::SimConfig;
use gothic_sim::sim::Sim;
use gothic_sim::types::*;
use gothic_sim::world::Pos;

#[test]
fn a_companion_attacks_what_the_player_focuses_and_follows_again_on_command() {
    let mut sim = Sim::new(SimConfig::default(), open_world(), synthetic_items());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut a = Actor::npc(ActorId(0), "Diego", Species::Human, 10, 10);
    a.alignment = Alignment::Mid;
    let diego = sim.spawn(a, Pos::flat(100.0, 0.0));
    let mut o = Actor::npc(ActorId(0), "Orc", Species::Orc, 10, 10);
    o.alignment = Alignment::Orc;
    o.base_attitude_to_player = Attitude::Hostile;
    o.ai_controlled = false;
    let orc = sim.spawn(o, Pos::flat(500.0, 0.0));
    assert!(sim.join_party(diego));
    // He is told to wait, then to follow again.
    sim.wait_party(diego);
    assert_eq!(sim.actor(diego).brain.task, Task::WaitForPlayer);
    sim.player_companion_command(false);
    sim.run_ticks(5);
    assert_eq!(sim.actor(diego).brain.task, Task::FollowPlayer);
    // The hero focuses the orc and orders the attack.
    sim.script.player.focus = EntityId(orc.0 + 1);
    sim.player_companion_command(true);
    let st = sim.script.npc(diego).task;
    assert_eq!(sim.actor(diego).brain.task, Task::Attack, "script task {st:?} attitude {:?}", sim.attitude(diego, orc));
    assert_eq!(sim.actor(diego).target, Some(orc));
    let _ = hero;
}

#[test]
fn the_guard_names_the_crimes_of_his_enclave_when_the_dialog_says_guard_news() {
    use gothic_script::host::AiCommand;
    let mut sim = Sim::new(SimConfig::default(), open_world(), synthetic_items());
    sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut g = Actor::npc(ActorId(0), "Guard_01", Species::Human, 10, 10);
    g.npc_type = NpcType::Enclave;
    g.alignment = Alignment::Mid;
    g.enclave = Some("trelis".into());
    let guard = sim.spawn(g, Pos::flat(200.0, 0.0));
    sim.enclaves.entry("trelis".into()).or_default().murder_count = 1;
    sim.enclaves.entry("trelis".into()).or_default().theft_count = 2;
    sim.script.take_commands();
    sim.dialog_voice("SayGuardNews", guard);
    let keys: Vec<&str> = sim
        .script
        .take_commands()
        .into_iter()
        .filter_map(|(_, c)| match c {
            AiCommand::SayLine { key } => Some(key),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        ["Halt", "KnownMurder", "FirstMurder", "ThereIsMore", "SecondTheft", "WeObserveYou"]
    );
}
