//! `--play --smoke`: a scripted showcase driving the real game systems (no shortcuts): fly over
//! Ardea, walk to Hamlar, talk (voice, option, quest toast), open the journal, draw the sword and
//! fight an orc raider. Writes `.local/play-{world,dialog,combat}.png`.
use super::events::{CombatLog, Voices};
use super::record::{IN_FLIGHT, Recorder};
use super::{Mode, PlayState, Roster, ScriptInput, angle_delta, yaw_of_dir};
use crate::walk::{Player, Stream, VegStream};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_runtime::CoreRes;
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
    mut open: MessageWriter<OpenScreen>,
    mut close: MessageWriter<CloseScreen>,
    mut recorder: Option<ResMut<Recorder>>,
    mut exit: MessageWriter<AppExit>,
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

    match s.phase {
        Phase::Loading => {
            let ready = stream.ready
                && stream.in_flight.is_empty()
                && stream.pending.is_empty()
                && veg.in_flight.is_empty()
                && veg.pending.is_empty()
                && player.placed
                && roster.pending.is_empty()
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
            let hero_m = hero_cm * CM;
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
            let _ = hero_m;
            if pt >= LEN {
                state.cinematic = None;
                go(&mut s, Phase::Walk, now);
                s.stuck_ref = hero_cm;
                s.stuck_since = now;
            }
        }
        Phase::Walk => {
            let Some(h) = hamlar else { return };
            let d = flat(h - hero_cm);
            let dist = d.length();
            // Camera follows the direction of travel.
            let want_yaw = yaw_of_dir(d.x, d.z);
            let dy = angle_delta(player.yaw, want_yaw);
            player.yaw += dy * (1.0 - (-2.5 * dt).exp());
            player.pitch += (-0.16 - player.pitch) * (1.0 - (-2.0 * dt).exp());
            // Detour when blocked.
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
            let rot = Quat::from_rotation_y(s.detour);
            if dist > 215.0 {
                input.wish = rot * d.normalize_or_zero();
            }
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
                state.focus = core.0.find_name("Hamlar");
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
                        go(&mut s, Phase::Journal, now);
                    }
                }
            } else if pt > 8.0 {
                // Dialog never opened: continue with the rest of the showcase.
                go(&mut s, Phase::Journal, now);
            }
        }
        Phase::Journal => {
            if state.mode == Mode::Dialog {
                input.end_dialog = true;
                return;
            }
            if !s.screen_open && pt > 0.8 {
                open.write(OpenScreen(Screen::Journal));
                s.screen_open = true;
            }
            if s.screen_open && pt > 3.6 {
                close.write(CloseScreen);
                s.screen_open = false;
                s.target = core
                    .0
                    .sim
                    .actors
                    .iter()
                    .filter(|a| a.name.starts_with("Orc_GameStartRaider") && a.is_alive())
                    .filter_map(|a| pos_of(a.id).map(|p| (flat(p - hero_cm).length(), a.id)))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|a| a.1);
                input.draw = Some(true);
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
                } else if now - s.last_attack > 0.85 {
                    input.attack = true;
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
                report(&s, &log, recorder.as_deref());
                if let Some(r) = recorder.as_mut() {
                    r.active = false;
                }
                go(&mut s, Phase::Flush, now);
            }
        }
        Phase::Flush => {
            s.flush_frames += 1;
            if s.flush_frames > 20 && IN_FLIGHT.load(Ordering::SeqCst) == 0 {
                exit.write(AppExit::Success);
            }
        }
    }
}

fn report(s: &Script, log: &CombatLog, rec: Option<&Recorder>) {
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
}
