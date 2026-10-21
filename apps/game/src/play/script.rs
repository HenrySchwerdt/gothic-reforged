//! `--play --smoke`: a scripted showcase driving the real game systems (no shortcuts): fly over
//! Ardea, walk to Hamlar, talk (voice, option, quest toast), pick a loose item up, open a chest,
//! use the anvil, cast a spell, shoot an arrow, open the journal, draw the sword and fight an orc
//! raider. Writes `.local/play-{world,dialog,pickup,chest,anvil,spell,arrow,combat}.png`.
//!
//! The only set-up the script does is what the quests would do: it hands the hero the smith's
//! hammer and a smithing blueprint with its ingredients (the anvil refuses without them) and
//! binds a spell to a quick slot (the player drags it there in the spell book).
use super::barks::BarkState;
use super::events::{CombatLog, Voices};
use super::fx::FxLog;
use super::interact::Interaction;
use super::objects::WorldObjects;
use super::record::{IN_FLIGHT, Recorder};
use super::routines::RoutineLog;
use super::{Mode, PlayState, Roster, ScriptAct, ScriptInput, Target, angle_delta, yaw_of_dir};
use crate::walk::{Player, Stream, VegStream};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_runtime::{Command, CoreCommand, CoreRes};
use gothic_sim::script::player::api::SkillCategory;
use gothic_ui::{CloseScreen, OpenScreen, Screen};
use std::sync::atomic::Ordering;
use std::time::Instant;

const CM: f32 = 0.01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Loading,
    Flyover,
    Walk,
    Talk,
    Pickup,
    Chest,
    Anvil,
    Spell,
    Arrow,
    Moves,
    Journal,
    Combat,
    Epilogue,
    Flush,
}

#[derive(Resource)]
struct Script {
    phase: Phase,
    started: f32,
    settled: u32,
    shots: u8,
    talked: bool,
    picks: usize,
    dialog_started: Option<f32>,
    voice_idle_since: Option<f32>,
    target: Option<gothic_sim::types::ActorId>,
    last_attack: f32,
    stuck_ref: Vec3,
    stuck_since: f32,
    detour: f32,
    frame_ms: Vec<f32>,
    last_frame: Option<Instant>,
    screen_open: bool,
    flush_frames: u32,
    fight_over_at: Option<f32>,
    /// Sub-step of the interaction phases and when it started.
    step: u32,
    step_at: f32,
    /// Object / item index chosen for the phase.
    pick: Option<usize>,
    /// Counter snapshot taken when a phase step began.
    mark: u32,
    results: Vec<String>,
    /// When the effect of the step (missile, arrow) was first seen.
    seen: Option<f32>,
}

pub fn register(app: &mut App) {
    app.insert_resource(Script {
        phase: Phase::Loading,
        started: 0.0,
        settled: 0,
        shots: 0,
        talked: false,
        picks: 0,
        dialog_started: None,
        voice_idle_since: None,
        target: None,
        last_attack: 0.0,
        stuck_ref: Vec3::ZERO,
        stuck_since: 0.0,
        detour: 0.0,
        frame_ms: Vec::new(),
        last_frame: None,
        screen_open: false,
        flush_frames: 0,
        fight_over_at: None,
        step: 0,
        step_at: 0.0,
        pick: None,
        mark: 0,
        results: Vec::new(),
        seen: None,
    })
    .add_systems(
        Update,
        direct
            .before(super::hero::input)
            .after(crate::walk::input_mode),
    );
}

fn shoot(commands: &mut Commands, rec: &mut Option<ResMut<Recorder>>, name: &str) {
    if let Some(r) = rec.as_mut()
        && r.active
    {
        r.names.push(name.to_string());
        return;
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(format!(".local/play-{name}.png")));
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn go(s: &mut Script, phase: Phase, now: f32) {
    eprintln!("play script: {:?} -> {:?} at {now:.1}s", s.phase, phase);
    s.phase = phase;
    s.started = now;
    s.step = 0;
    s.step_at = now;
    s.pick = None;
    s.seen = None;
}

fn next_step(s: &mut Script, now: f32) {
    s.step += 1;
    s.step_at = now;
    s.seen = None;
}

/// Resources the interaction phases read.
#[derive(SystemParam)]
struct Env<'w> {
    objects: Res<'w, WorldObjects>,
    inter: Res<'w, Interaction>,
    fx: Res<'w, FxLog>,
    barks: Res<'w, BarkState>,
    routines: Res<'w, RoutineLog>,
    cmd: MessageWriter<'w, CoreCommand>,
}

#[derive(SystemParam)]
struct UiOut<'w> {
    open: MessageWriter<'w, OpenScreen>,
    close: MessageWriter<'w, CloseScreen>,
    exit: MessageWriter<'w, AppExit>,
}

/// Walks the hero toward `goal` (cm) with the Hamlar walk's detour when blocked; the camera
/// follows. Returns the horizontal distance.
#[allow(clippy::too_many_arguments)]
fn walk_to(
    s: &mut Script,
    input: &mut ScriptInput,
    player: &mut Player,
    goal: Vec3,
    stop: f32,
    now: f32,
    dt: f32,
) -> f32 {
    let hero_cm = player.ctrl.position;
    let d = Vec3::new(goal.x - hero_cm.x, 0.0, goal.z - hero_cm.z);
    let dist = d.length();
    let want_yaw = yaw_of_dir(d.x, d.z);
    player.yaw += angle_delta(player.yaw, want_yaw) * (1.0 - (-3.0 * dt).exp());
    player.pitch += (-0.16 - player.pitch) * (1.0 - (-2.0 * dt).exp());
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    if flat(hero_cm).distance(flat(s.stuck_ref)) > 60.0 {
        s.stuck_ref = hero_cm;
        s.stuck_since = now;
        s.detour *= 0.5;
    } else if now - s.stuck_since > 1.2 {
        s.detour = if s.detour.abs() < 0.1 {
            1.1
        } else {
            -s.detour * 1.2
        };
        s.stuck_since = now;
    }
    if dist > stop {
        input.wish = Quat::from_rotation_y(s.detour) * d.normalize_or_zero();
    }
    dist
}

/// Aim the camera at `goal` (cm).
fn face(player: &mut Player, goal: Vec3, dt: f32) {
    let hero_cm = player.ctrl.position;
    let d = Vec3::new(goal.x - hero_cm.x, 0.0, goal.z - hero_cm.z);
    let want = yaw_of_dir(d.x, d.z);
    player.yaw += angle_delta(player.yaw, want) * (1.0 - (-6.0 * dt).exp());
    player.pitch += (-0.2 - player.pitch) * (1.0 - (-3.0 * dt).exp());
}

#[allow(clippy::too_many_arguments)]
fn direct(
    mut commands: Commands,
    time: Res<Time>,
    mut s: ResMut<Script>,
    mut input: ResMut<ScriptInput>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    stream: Res<Stream>,
    veg: Res<VegStream>,
    roster: Res<Roster>,
    core: Res<CoreRes>,
    voices: Res<Voices>,
    log: Res<CombatLog>,
    mut w: Env,
    mut ui: UiOut,
    mut recorder: Option<ResMut<Recorder>>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let pt = now - s.started;
    let hero_cm = player.ctrl.position;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let pos_of = |id| {
        core.0
            .sim
            .world
            .actors
            .get(&id)
            .map(|w| Vec3::new(w.0.x, w.0.y, w.0.z))
    };
    input.active = s.phase != Phase::Loading;
    input.wish = Vec3::ZERO;
    let hamlar = core.0.find_name("Hamlar").and_then(pos_of);

    if matches!(s.phase, Phase::Walk | Phase::Combat) && recorder.is_none() {
        if let Some(last) = s.last_frame {
            s.frame_ms.push(last.elapsed().as_secs_f32() * 1000.0);
        }
        s.last_frame = Some(Instant::now());
    } else {
        s.last_frame = None;
    }

    // Nearest living raider of the game start (target of the spell, the arrow and the fight).
    let raider = |skip: Option<gothic_sim::types::ActorId>| {
        core.0
            .sim
            .actors
            .iter()
            .filter(|a| a.name.starts_with("Orc_GameStartRaider") && a.is_alive())
            .filter(|a| Some(a.id) != skip)
            .filter_map(|a| pos_of(a.id).map(|p| (flat(p - hero_cm).length(), a.id)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|a| a.1)
    };

    match s.phase {
        Phase::Loading => {
            let ready = stream.ready
                && stream.in_flight.is_empty()
                && stream.pending.is_empty()
                && veg.in_flight.is_empty()
                && veg.pending.is_empty()
                && player.placed
                && super::npcs::pending_in_ring(&roster, &stream) == 0
                && roster.binds.is_empty()
                && hamlar.is_some();
            if ready && stream.last_activity > 5 {
                s.settled += 1;
            } else {
                s.settled = 0;
            }
            if s.settled > 90 || now > 400.0 {
                if let Some(h) = hamlar {
                    let d = flat(h - hero_cm);
                    player.yaw = yaw_of_dir(d.x, d.z);
                    player.pitch = -0.14;
                }
                eprintln!(
                    "play script: world ready ({} NPCs spawned), starting showcase",
                    roster.spawned
                );
                if let Some(r) = recorder.as_mut() {
                    r.active = true;
                }
                if let Some(h) = hamlar {
                    let h = h * CM;
                    state.cinematic = Some((h + Vec3::new(-26.0, 24.0, -30.0), h + Vec3::Y * 1.5));
                }
                go(&mut s, Phase::Flyover, now);
            }
        }
        Phase::Flyover => {
            const LEN: f32 = 6.0;
            let h = hamlar.unwrap_or(hero_cm) * CM;
            let look = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
            let end_pos =
                (hero_cm + Vec3::Y * 150.0 + look * Vec3::Z * 280.0 + Vec3::Y * 20.0) * CM;
            let k = smooth(pt / LEN);
            let start = h + Vec3::new(-26.0, 24.0, -30.0);
            let mid = Vec3::new(h.x + 24.0, h.y + 16.0, h.z - 22.0);
            let p = if k < 0.5 {
                start.lerp(mid, smooth(k * 2.0))
            } else {
                mid.lerp(end_pos, smooth(k * 2.0 - 1.0))
            };
            let target = (h + Vec3::Y * 1.5).lerp(end_pos + look * Vec3::NEG_Z * 10.0, k);
            state.cinematic = Some((p, target));
            if pt >= LEN - 0.5 && s.seen.is_none() && !w.routines.poses.is_empty() {
                // Characters at work: the routine tasks of the ported states are visible.
                shoot(&mut commands, &mut recorder, "routine");
                s.results.push(format!(
                    "routine: {} poses started before the walk, e.g. {:?}",
                    w.routines.poses.len(),
                    w.routines.poses.iter().take(3).collect::<Vec<_>>()
                ));
                s.shots = 0;
                s.seen = Some(0.0);
            }
            if pt >= LEN && s.shots == 0 {
                s.results
                    .push(format!("routines at the start: {}", npc_states(&core)));
            }
            if pt >= LEN {
                state.cinematic = None;
                go(&mut s, Phase::Walk, now);
                s.stuck_ref = hero_cm;
                s.stuck_since = now;
            }
        }
        Phase::Walk => {
            let Some(h) = hamlar else { return };
            let dist = walk_to(&mut s, &mut input, &mut player, h, 215.0, now, dt);
            if s.shots == 0 && pt > 1.2 {
                shoot(&mut commands, &mut recorder, "world");
                s.shots = 1;
            }
            if dist <= 215.0 || pt > 45.0 {
                go(&mut s, Phase::Talk, now);
            }
        }
        Phase::Talk => {
            if !s.talked && pt > 0.6 {
                s.talked = true;
                input.talk = true;
                // The interaction focus needs the camera to look at the NPC.
                state.target = core.0.find_name("Hamlar").map(Target::Npc);
            }
            if state.mode == Mode::Dialog && s.dialog_started.is_none() {
                s.dialog_started = Some(now);
            }
            if let Some(t0) = s.dialog_started {
                let dpt = now - t0;
                if s.shots == 1 && dpt > 2.5 && !voices.menu.is_empty() {
                    shoot(&mut commands, &mut recorder, "dialog");
                    s.shots = 2;
                }
                if voices.idle() && !voices.menu.is_empty() {
                    s.voice_idle_since.get_or_insert(now);
                } else if !voices.idle() {
                    s.voice_idle_since = None;
                }
                let waited = s.voice_idle_since.is_some_and(|t| now - t > 0.5) || dpt > 9.0;
                if waited && !voices.menu.is_empty() {
                    const WANTED: [&str; 2] = ["Reddock", "Rebellen"];
                    if s.picks < WANTED.len() {
                        let pick = voices
                            .menu
                            .iter()
                            .position(|c| c.label.contains(WANTED[s.picks]))
                            .or_else(|| voices.menu.iter().position(|c| c.info.is_some()))
                            .unwrap_or(0);
                        eprintln!("play script: choosing \"{}\"", voices.menu[pick].label);
                        input.choose = Some(pick);
                        s.picks += 1;
                        s.voice_idle_since = None;
                        s.dialog_started = Some(now);
                    } else {
                        input.end_dialog = true;
                        go(&mut s, Phase::Pickup, now);
                    }
                }
            } else if pt > 8.0 {
                // Dialog never opened: continue with the rest of the showcase.
                go(&mut s, Phase::Pickup, now);
            }
        }
        Phase::Pickup => {
            if state.mode == Mode::Dialog {
                input.end_dialog = true;
                return;
            }
            if s.step == 0 {
                // A loose item that is shown: the nearest one (potions first).
                let best = w
                    .objects
                    .list
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.def.is_item && o.visual.is_some() && o.entity.is_some())
                    .min_by(|a, b| {
                        let pa = f32::from(a.1.def.name != "It_Potion_Health");
                        let pb = f32::from(b.1.def.name != "It_Potion_Health");
                        let da = flat(a.1.pos_cm - hero_cm).length();
                        let db = flat(b.1.pos_cm - hero_cm).length();
                        (pa * 1.0e5 + da).total_cmp(&(pb * 1.0e5 + db))
                    })
                    .map(|(i, _)| i);
                match best {
                    Some(i) => {
                        eprintln!("play script: item {}", w.objects.list[i].def.name);
                        s.pick = Some(i);
                        s.mark = core.0.sim.actor(core.0.hero).inventory.values().sum();
                        s.stuck_ref = hero_cm;
                        s.stuck_since = now;
                        next_step(&mut s, now);
                    }
                    None if pt > 20.0 => {
                        s.results.push("pickup: no loose item streamed in".into());
                        go(&mut s, Phase::Chest, now);
                    }
                    None => {}
                }
                return;
            }
            let Some(i) = s.pick else { return };
            let goal = w.objects.list[i].pos_cm;
            let st = now - s.step_at;
            match s.step {
                1 => {
                    let dist = walk_to(&mut s, &mut input, &mut player, goal, 120.0, now, dt);
                    if dist <= 120.0 || st > 25.0 {
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    face(&mut player, goal, dt);
                    if state.target == Some(Target::Item(i)) || st > 4.0 {
                        input.acts.push_back(ScriptAct::Confirm);
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    let got = w.inter.log.iter().any(|l| l.contains("PickedUp"));
                    if (got && st > 0.5) || st > 6.0 {
                        shoot(&mut commands, &mut recorder, "pickup");
                        s.results.push(format!(
                            "pickup: {} {}",
                            w.objects.list[i].def.name,
                            if got { "taken" } else { "NOT taken" }
                        ));
                        next_step(&mut s, now);
                    }
                }
                _ => {
                    if st > 0.8 {
                        go(&mut s, Phase::Chest, now);
                    }
                }
            }
        }
        Phase::Chest => {
            if s.step == 0 {
                let best = w
                    .objects
                    .list
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        o.entity.is_some()
                            && o.def.script_use() == gothic_sim::script::enums::UseType::Chest
                            && o.def.lock.as_ref().is_none_or(|l| !l.locked)
                    })
                    .min_by(|a, b| {
                        flat(a.1.pos_cm - hero_cm)
                            .length()
                            .total_cmp(&flat(b.1.pos_cm - hero_cm).length())
                    })
                    .map(|(i, _)| i);
                s.pick = best;
                s.stuck_ref = hero_cm;
                s.stuck_since = now;
                if best.is_none() {
                    s.results.push("chest: none registered".into());
                    go(&mut s, Phase::Anvil, now);
                } else {
                    next_step(&mut s, now);
                }
                return;
            }
            let Some(i) = s.pick else { return };
            let goal = w.objects.list[i].pos_cm;
            let st = now - s.step_at;
            match s.step {
                1 => {
                    let dist = walk_to(&mut s, &mut input, &mut player, goal, 150.0, now, dt);
                    if dist <= 150.0 || st > 30.0 {
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    face(&mut player, goal, dt);
                    if state.target == Some(Target::Object(i)) || st > 4.0 {
                        input.acts.push_back(ScriptAct::Confirm);
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    if w.inter.window.is_some() && st > 0.8 {
                        shoot(&mut commands, &mut recorder, "chest");
                        let lines = w.inter.window.as_ref().map_or(0, |x| x.lines.len());
                        s.results
                            .push(format!("chest: loot window with {lines} stacks"));
                        next_step(&mut s, now);
                    } else if st > 10.0 {
                        s.results.push("chest: no loot window".into());
                        go(&mut s, Phase::Anvil, now);
                    }
                }
                4 => {
                    if st > 0.8 {
                        input.acts.push_back(ScriptAct::WindowEnter);
                        next_step(&mut s, now);
                    }
                }
                5 => {
                    if st > 1.2 {
                        input.acts.push_back(ScriptAct::WindowClose);
                        next_step(&mut s, now);
                    }
                }
                _ => {
                    if (w.inter.active.is_none() && st > 1.0) || st > 8.0 {
                        go(&mut s, Phase::Anvil, now);
                    }
                }
            }
        }
        Phase::Anvil => {
            if s.step == 0 {
                let best = w
                    .objects
                    .list
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        o.entity.is_some()
                            && o.def.script_use() == gothic_sim::script::enums::UseType::Anvil
                    })
                    .map(|(i, _)| i)
                    .next();
                s.pick = best;
                s.stuck_ref = hero_cm;
                s.stuck_since = now;
                // Quest set-up: the hammer and a smithing blueprint with its ingredients.
                let blueprint = w
                    .objects
                    .recipe_cat
                    .iter()
                    .filter(|(_, c)| **c == SkillCategory::Smith)
                    .map(|(k, _)| k.clone())
                    .min();
                if let Some(bp) = &blueprint {
                    w.cmd.write(CoreCommand(Command::GiveHero {
                        template: "It_Hammer_Smith".into(),
                        amount: 1,
                    }));
                    w.cmd.write(CoreCommand(Command::GiveHero {
                        template: bp.clone(),
                        amount: 1,
                    }));
                    if let Some(id) = w.objects.recipe_ids.get(bp)
                        && let Some(r) = core.0.sim.script.recipe_of(*id)
                    {
                        if !r.required_perk.is_empty() {
                            w.cmd
                                .write(CoreCommand(Command::GrantPerk(r.required_perk.clone())));
                        }
                        for (item, n) in &r.ingredients {
                            w.cmd.write(CoreCommand(Command::GiveHero {
                                template: item.clone(),
                                amount: *n,
                            }));
                        }
                    }
                }
                if best.is_none() {
                    s.results.push("anvil: none registered".into());
                    go(&mut s, Phase::Spell, now);
                } else {
                    next_step(&mut s, now);
                }
                return;
            }
            let Some(i) = s.pick else { return };
            let goal = w.objects.list[i].pos_cm;
            let st = now - s.step_at;
            match s.step {
                1 => {
                    let dist = walk_to(&mut s, &mut input, &mut player, goal, 150.0, now, dt);
                    if dist <= 150.0 || st > 40.0 {
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    face(&mut player, goal, dt);
                    if state.target == Some(Target::Object(i)) || st > 4.0 {
                        input.acts.push_back(ScriptAct::Confirm);
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    if w.inter.window.is_some() && st > 0.8 {
                        shoot(&mut commands, &mut recorder, "anvil");
                        let lines = w.inter.window.as_ref().map_or(0, |x| x.lines.len());
                        s.results
                            .push(format!("anvil: recipe window with {lines} recipes"));
                        s.mark = u32::from(w.inter.log.iter().any(|l| l.contains("Crafted")));
                        next_step(&mut s, now);
                    } else if st > 6.0 {
                        let why = w
                            .inter
                            .log
                            .iter()
                            .rev()
                            .find(|l| l.contains("Refused"))
                            .cloned()
                            .unwrap_or_else(|| "no event".into());
                        s.results.push(format!("anvil: no window ({why})"));
                        shoot(&mut commands, &mut recorder, "anvil");
                        go(&mut s, Phase::Spell, now);
                    }
                }
                4 => {
                    if st > 0.6 {
                        input.acts.push_back(ScriptAct::WindowKey(1));
                        next_step(&mut s, now);
                    }
                }
                5 => {
                    if st > 1.2 {
                        let made = w.inter.log.iter().any(|l| l.contains("Crafted"));
                        s.results.push(format!(
                            "anvil: craft {}",
                            if made { "made an item" } else { "refused" }
                        ));
                        input.acts.push_back(ScriptAct::WindowClose);
                        next_step(&mut s, now);
                    }
                }
                _ => {
                    if (w.inter.active.is_none() && st > 1.0) || st > 8.0 {
                        go(&mut s, Phase::Spell, now);
                    }
                }
            }
        }
        Phase::Spell => {
            if state.hero_down {
                go(&mut s, Phase::Journal, now);
                return;
            }
            if s.step == 0 {
                s.target = raider(None);
                if s.target.is_none() {
                    s.results.push("spell: no target".into());
                    go(&mut s, Phase::Arrow, now);
                    return;
                }
                // Bind the fireball rune to quick slot 6 (what the spell book's drag does).
                if let Some(slot) = state.quick.get_mut(5) {
                    *slot = Some("It_Spell_Fireball".into());
                }
                s.mark = w.fx.missiles;
                s.stuck_ref = hero_cm;
                s.stuck_since = now;
                next_step(&mut s, now);
                return;
            }
            let Some(t) = s.target else { return };
            let Some(tp) = pos_of(t) else { return };
            let st = now - s.step_at;
            match s.step {
                1 => {
                    let dist = walk_to(&mut s, &mut input, &mut player, tp, 650.0, now, dt);
                    if dist <= 650.0 || st > 40.0 {
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    face(&mut player, tp, dt);
                    if st > 0.5 {
                        input.acts.push_back(ScriptAct::Quick(5));
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    face(&mut player, tp, dt);
                    if st > 1.5 {
                        input.acts.push_back(ScriptAct::LeftClick(0.15));
                        next_step(&mut s, now);
                    }
                }
                4 => {
                    face(&mut player, tp, dt);
                    if w.fx.missiles > s.mark {
                        s.seen.get_or_insert(now);
                    }
                    if s.seen.is_some_and(|t| now - t > 0.04) {
                        shoot(&mut commands, &mut recorder, "spell");
                        s.results.push("spell: fireball missile in flight".into());
                        next_step(&mut s, now);
                    } else if st > 4.0 {
                        let why = w
                            .inter
                            .log
                            .iter()
                            .rev()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; ");
                        s.results.push(format!("spell: no missile ({why})"));
                        shoot(&mut commands, &mut recorder, "spell");
                        go(&mut s, Phase::Arrow, now);
                    }
                }
                _ => {
                    if st > 1.8 {
                        go(&mut s, Phase::Arrow, now);
                    }
                }
            }
        }
        Phase::Arrow => {
            if state.hero_down {
                go(&mut s, Phase::Moves, now);
                return;
            }
            if s.step == 0 {
                s.target = raider(None);
                s.mark = w.fx.arrows;
                next_step(&mut s, now);
                return;
            }
            let Some(t) = s.target else {
                go(&mut s, Phase::Moves, now);
                return;
            };
            let Some(tp) = pos_of(t) else { return };
            let st = now - s.step_at;
            match s.step {
                1 => {
                    face(&mut player, tp, dt);
                    if st > 0.3 {
                        input.acts.push_back(ScriptAct::Quick(2));
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    face(&mut player, tp, dt);
                    if st > 1.5 {
                        input.acts.push_back(ScriptAct::LeftClick(1.0));
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    face(&mut player, tp, dt);
                    if w.fx.arrows > s.mark {
                        s.seen.get_or_insert(now);
                    }
                    if s.seen.is_some_and(|t| now - t > 0.04) {
                        shoot(&mut commands, &mut recorder, "arrow");
                        s.results.push("arrow: shot".into());
                        next_step(&mut s, now);
                    } else if st > 5.0 {
                        let hero = core.0.sim.actor(core.0.hero);
                        s.results.push(format!(
                            "arrow: no projectile (mode {:?}, left {:?}, right {:?}, state {:?}, drawn {})",
                            hero.equipment.weapon_mode(),
                            hero.equipment.left.as_ref().map(|i| i.name.clone()),
                            hero.equipment.right.as_ref().map(|i| i.name.clone()),
                            core.0.sim.script.npc(core.0.hero).state,
                            state.drawn,
                        ));
                        shoot(&mut commands, &mut recorder, "arrow");
                        go(&mut s, Phase::Moves, now);
                    }
                }
                _ => {
                    if st > 1.5 {
                        go(&mut s, Phase::Moves, now);
                    }
                }
            }
        }
        Phase::Moves => {
            // Sneak on, walk, sneak off, jump: the movement commands of the ported states.
            let st = now - s.step_at;
            let fwd = Quat::from_rotation_y(player.yaw) * Vec3::NEG_Z;
            match s.step {
                0 => {
                    input.acts.push_back(ScriptAct::Sneak);
                    next_step(&mut s, now);
                }
                1 => {
                    if st > 0.6 {
                        s.results
                            .push(format!("moves: sneaking {}", state.sneaking));
                        next_step(&mut s, now);
                    }
                }
                2 => {
                    input.wish = fwd;
                    if st > 1.0 {
                        input.acts.push_back(ScriptAct::Sneak);
                        next_step(&mut s, now);
                    }
                }
                3 => {
                    if st > 0.6 {
                        s.results
                            .push(format!("moves: sneak off {}", !state.sneaking));
                        input.acts.push_back(ScriptAct::Jump);
                        s.mark = 0;
                        next_step(&mut s, now);
                    }
                }
                _ => {
                    if !player.ctrl.grounded {
                        s.mark = 1;
                    }
                    if st > 1.0 {
                        let airborne = s.mark == 1;
                        s.results.push(format!("moves: jump airborne {airborne}"));
                        go(&mut s, Phase::Journal, now);
                    }
                }
            }
        }
        Phase::Journal => {
            if state.mode == Mode::Dialog {
                input.end_dialog = true;
                return;
            }
            if !s.screen_open && pt > 0.8 {
                ui.open.write(OpenScreen(Screen::Journal));
                s.screen_open = true;
            }
            if s.screen_open && pt > 3.6 {
                ui.close.write(CloseScreen);
                s.screen_open = false;
                s.target = raider(None);
                // Back to the sword: quick slot 1 wears and draws it.
                input.acts.push_back(ScriptAct::Quick(0));
                go(&mut s, Phase::Combat, now);
                s.last_attack = now;
            }
        }
        Phase::Combat => {
            let Some(t) = s.target else {
                go(&mut s, Phase::Epilogue, now);
                return;
            };
            let Some(tp) = pos_of(t) else { return };
            let d = flat(tp - hero_cm);
            let dist = d.length();
            let want_yaw = yaw_of_dir(d.x, d.z);
            player.yaw += angle_delta(player.yaw, want_yaw) * (1.0 - (-3.0 * dt).exp());
            player.pitch += (-0.2 - player.pitch) * (1.0 - (-2.0 * dt).exp());
            let alive = core.0.sim.actor(t).is_alive();
            if alive && !state.hero_down {
                if dist > 190.0 {
                    input.wish = d.normalize_or_zero();
                } else if now - s.last_attack > 0.85 && pt > 1.0 {
                    input.acts.push_back(ScriptAct::LeftClick(0.1));
                    s.last_attack = now;
                    state.combat_focus = Some((t, now));
                }
            }
            if s.shots == 2
                && log.first_hit_at.is_some_and(|t0| now - t0 > 1.0)
                && now - s.started > 1.0
            {
                shoot(&mut commands, &mut recorder, "combat");
                s.shots = 3;
            }
            let over = !alive || state.hero_down;
            if over {
                s.fight_over_at.get_or_insert(now);
            }
            if s.fight_over_at.is_some_and(|t0| now - t0 > 2.0) || pt > 15.0 {
                go(&mut s, Phase::Epilogue, now);
            }
        }
        Phase::Epilogue => {
            if pt > 2.0 {
                s.results.push(format!(
                    "routines at the end: {} NPCs got points; {}",
                    w.objects.with_points,
                    npc_states(&core)
                ));
                let summary = Summary {
                    barks: &w.barks,
                    routines: &w.routines,
                    fx: &w.fx,
                };
                report(&s, &log, recorder.as_deref(), &summary);
                if let Some(r) = recorder.as_mut() {
                    r.active = false;
                }
                go(&mut s, Phase::Flush, now);
            }
        }
        Phase::Flush => {
            s.flush_frames += 1;
            if s.flush_frames > 20 && IN_FLIGHT.load(Ordering::SeqCst) == 0 {
                ui.exit.write(AppExit::Success);
            }
        }
    }
}

/// Histogram of what the characters' script states are (smoke report).
fn npc_states(core: &CoreRes) -> String {
    let mut tasks: std::collections::BTreeMap<String, u32> = Default::default();
    let mut with_dest = 0;
    for a in core.0.sim.actors.iter().filter(|a| a.id != core.0.hero) {
        let n = core.0.sim.script.npc(a.id);
        let k = match n.state {
            Some(st) => format!("{st:?}"),
            None => "none".into(),
        };
        *tasks.entry(k).or_default() += 1;
        if n.dest.0 != 0 {
            with_dest += 1;
        }
    }
    format!("{with_dest} with a destination, states {tasks:?}")
}

struct Summary<'a> {
    barks: &'a BarkState,
    routines: &'a RoutineLog,
    fx: &'a FxLog,
}

fn report(s: &Script, log: &CombatLog, rec: Option<&Recorder>, sum: &Summary) {
    if let Some(r) = rec {
        eprintln!(
            "play script: recorded {} frames = {:.1} s at 30 fps",
            r.frame,
            r.frame as f32 / 30.0
        );
    }
    if s.frame_ms.len() > 30 {
        let v = &s.frame_ms[10..];
        let avg = v.iter().sum::<f32>() / v.len() as f32;
        let mut sorted = v.to_vec();
        sorted.sort_by(f32::total_cmp);
        eprintln!(
            "PLAY FRAME TIME walking and fighting among NPCs: {:.2} ms avg ({:.0} fps), p95 {:.2} ms, {} frames",
            avg,
            1000.0 / avg,
            sorted[sorted.len() * 95 / 100],
            v.len()
        );
    }
    eprintln!(
        "play script: hits on NPCs {}, hits on hero {}, kills {}",
        log.hits_on_npcs, log.hits_on_hero, log.kills
    );
    for r in &s.results {
        eprintln!("play script result: {r}");
    }
    let mut voices: Vec<&str> = sum.barks.played.iter().map(|(v, _)| v.as_str()).collect();
    voices.sort_unstable();
    voices.dedup();
    eprintln!(
        "play script result: {} barks played ({} voices), {} without own recording; first: {:?}",
        sum.barks.played.len(),
        voices.len(),
        sum.barks.missing,
        sum.barks.played.iter().take(4).collect::<Vec<_>>()
    );
    let mut acts: Vec<String> = sum
        .routines
        .poses
        .iter()
        .map(|(w, a)| format!("{w}:{a}"))
        .collect();
    acts.sort();
    acts.dedup();
    eprintln!(
        "play script result: routine poses {} ({} distinct), unavailable {:?}; sample {:?}",
        sum.routines.poses.len(),
        acts.len(),
        sum.routines.unavailable.iter().take(5).collect::<Vec<_>>(),
        acts.iter().take(8).collect::<Vec<_>>()
    );
    eprintln!(
        "play script result: routine commands {:?}",
        sum.routines.kinds
    );
    eprintln!(
        "play script result: fx casts {}, spell missiles {}, arrows {}",
        sum.fx.casts, sum.fx.missiles, sum.fx.arrows
    );
}
