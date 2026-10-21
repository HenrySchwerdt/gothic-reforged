//! `--play --qa <scenario>`: input-level scripted playtests.
//!
//! The harness injects the same Bevy messages the OS backend would (`KeyboardInput`,
//! `MouseButtonInput`, `MouseMotion`, `CursorMoved`), so the game's own input systems, the UI
//! picking and the hotkeys all run unmodified. Direct state changes are limited to the explicit
//! `setup` of a scenario (teleport, grant gold / items) and are marked as such in the log.
//! Assertions read the core state (`CoreRes`), the UI state and the interaction state.
//!
//! Output per scenario: `.local/qa/<scenario>/NN_<name>.png`, `result.txt`, `result.json`.
//! The process exit code is 0 on PASS, 1 on FAIL, and the last stdout line is
//! `QA RESULT <scenario> PASS|FAIL`.
use super::interact::Interaction;
use super::{PlayState, Roster};
use crate::walk::{Player, Stream, VegStream};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButtonInput, MouseMotion};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::ui_widgets::Button;
use bevy::window::{CursorGrabMode, CursorMoved, CursorOptions, PrimaryWindow, WindowEvent};
use gothic_runtime::CoreRes;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

mod melee;
mod more;
mod more2;
mod more3;
mod moves;
mod moves2;
mod ranged;
mod scenarios;
mod world;

pub const CM: f32 = 0.01;

/// What a step reports each frame.
pub enum Poll {
    Done,
    Wait,
    /// The scenario cannot go on (recorded as a failure).
    Fail(String),
}

type StepFn = Box<dyn FnMut(&mut Cx) -> Poll + Send + Sync>;

struct Step {
    name: String,
    f: StepFn,
    started: bool,
    t0: f32,
}

/// One recorded check.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Mutable harness state shared by all steps.
pub struct QaState {
    pub scenario: String,
    pub dir: PathBuf,
    pub checks: Vec<Check>,
    pub store: BTreeMap<String, String>,
    pub notes: Vec<String>,
    held: HashSet<KeyCode>,
    release_next: Vec<KeyCode>,
    buttons_next: Vec<MouseButton>,
    shot_no: u32,
    window: Option<Entity>,
    cursor: Vec2,
    started_real: std::time::Instant,
    /// Hero position at the last progress mark of `walk_to` (stuck detection).
    walk_ref: Vec3,
    walk_ref_t: f32,
    walk_detour: f32,
    /// Free per-scenario counters.
    pub n: BTreeMap<String, f64>,
}

#[derive(Resource)]
pub struct Qa {
    steps: Vec<Step>,
    cur: usize,
    st: QaState,
    ready_since: Option<f32>,
    ready: bool,
    finished: bool,
    exit_at: Option<f32>,
    watchers: Vec<Box<dyn FnMut(&mut Cx) + Send + Sync>>,
    max_secs: f32,
}

/// What a step can touch.
pub struct Cx<'w> {
    pub world: &'w mut World,
    pub st: &'w mut QaState,
    pub now: f32,
    /// Seconds since the current step began.
    pub t: f32,
}

// ---- building scenarios ----------------------------------------------------------------------

/// Scenario builder.
#[derive(Default)]
pub struct Sc {
    steps: Vec<Step>,
    watchers: Vec<Box<dyn FnMut(&mut Cx) + Send + Sync>>,
    pub max_secs: f32,
}

impl Sc {
    pub fn new(max_secs: f32) -> Self {
        Self {
            max_secs,
            ..Default::default()
        }
    }

    pub fn step(
        &mut self,
        name: impl Into<String>,
        f: impl FnMut(&mut Cx) -> Poll + Send + Sync + 'static,
    ) -> &mut Self {
        self.steps.push(Step {
            name: name.into(),
            f: Box::new(f),
            started: false,
            t0: 0.0,
        });
        self
    }

    /// Runs once.
    pub fn act(
        &mut self,
        name: impl Into<String>,
        mut f: impl FnMut(&mut Cx) + Send + Sync + 'static,
    ) -> &mut Self {
        self.step(name, move |cx| {
            f(cx);
            Poll::Done
        })
    }

    pub fn wait(&mut self, secs: f32) -> &mut Self {
        self.step(format!("wait {secs}s"), move |cx| {
            if cx.t >= secs { Poll::Done } else { Poll::Wait }
        })
    }

    /// Waits for `cond`; a timeout is a failure that ends the scenario.
    pub fn until(
        &mut self,
        name: impl Into<String>,
        timeout: f32,
        mut cond: impl FnMut(&mut Cx) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        let name = name.into();
        let n2 = name.clone();
        self.step(name, move |cx| {
            if cond(cx) {
                Poll::Done
            } else if cx.t > timeout {
                Poll::Fail(format!("timeout after {timeout}s waiting for: {n2}"))
            } else {
                Poll::Wait
            }
        })
    }

    /// A non-fatal assertion.
    pub fn check(
        &mut self,
        name: impl Into<String>,
        mut f: impl FnMut(&mut Cx) -> Result<String, String> + Send + Sync + 'static,
    ) -> &mut Self {
        let name = name.into();
        self.step(name.clone(), move |cx| {
            let (ok, detail) = match f(cx) {
                Ok(d) => (true, d),
                Err(d) => (false, d),
            };
            cx.st.checks.push(Check {
                name: name.clone(),
                ok,
                detail,
            });
            Poll::Done
        })
    }

    /// A screenshot (the file is written a few frames later).
    pub fn shot(&mut self, name: &str) -> &mut Self {
        let name = name.to_string();
        self.act(format!("shot {name}"), move |cx| cx.shot(&name));
        self.wait(0.6)
    }

    pub fn watch(&mut self, f: impl FnMut(&mut Cx) + Send + Sync + 'static) -> &mut Self {
        self.watchers.push(Box::new(f));
        self
    }
}

pub fn ok(s: impl Into<String>) -> Result<String, String> {
    Ok(s.into())
}

pub fn err(s: impl Into<String>) -> Result<String, String> {
    Err(s.into())
}

pub fn expect(
    cond: bool,
    good: impl Into<String>,
    bad: impl Into<String>,
) -> Result<String, String> {
    if cond {
        Ok(good.into())
    } else {
        Err(bad.into())
    }
}

// ---- registration -----------------------------------------------------------------------------

pub fn register(app: &mut App, scenario: &str) {
    let Some(sc) = scenarios::build(scenario) else {
        eprintln!(
            "qa: unknown scenario {scenario:?}; known: {}",
            scenarios::NAMES.join(", ")
        );
        std::process::exit(2);
    };
    // `GR_QA_OUT` moves the output directory (parallel agents must not wipe each other).
    let base = std::env::var("GR_QA_OUT").unwrap_or_else(|_| ".local/qa".into());
    let dir = PathBuf::from(format!("{base}/{scenario}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    app.insert_resource(Qa {
        steps: sc.steps,
        cur: 0,
        st: QaState {
            scenario: scenario.to_string(),
            dir,
            checks: Vec::new(),
            store: BTreeMap::new(),
            notes: Vec::new(),
            held: HashSet::new(),
            release_next: Vec::new(),
            buttons_next: Vec::new(),
            shot_no: 0,
            window: None,
            cursor: Vec2::new(640.0, 360.0),
            started_real: std::time::Instant::now(),
            walk_ref: Vec3::ZERO,
            walk_ref_t: 0.0,
            walk_detour: 0.0,
            n: BTreeMap::new(),
        },
        ready_since: None,
        ready: false,
        finished: false,
        exit_at: None,
        watchers: sc.watchers,
        max_secs: if sc.max_secs > 0.0 {
            sc.max_secs
        } else {
            240.0
        },
    });
    // Runs late in the frame; the injected messages are read by the next frame's input systems.
    app.add_systems(Last, run);
}

fn world_ready(world: &World) -> bool {
    let stream = world.resource::<Stream>();
    let veg = world.resource::<VegStream>();
    let player = world.resource::<Player>();
    let roster = world.resource::<Roster>();
    let core = world.resource::<CoreRes>();
    stream.ready
        && stream.in_flight.is_empty()
        && stream.pending.is_empty()
        && veg.in_flight.is_empty()
        && veg.pending.is_empty()
        && player.placed
        && super::npcs::pending_in_ring(roster, stream) == 0
        && roster.binds.is_empty()
        && core.0.find_name("Hamlar").is_some()
}

fn run(world: &mut World) {
    {
        let n = world.resource::<Time>().elapsed_secs() as u32;
        let mut q = world.resource_mut::<Qa>();
        let last = q.st.n.get("beat").copied().unwrap_or(-1.0);
        if f64::from(n / 5) != last {
            q.st.n.insert("beat".into(), f64::from(n / 5));
            eprintln!("qa: heartbeat t={n}s ready={}", q.ready);
        }
    }
    world.resource_scope(|world, mut qa: Mut<Qa>| {
        let now = world.resource::<Time>().elapsed_secs();
        let qa = &mut *qa;
        if qa.st.window.is_none() {
            qa.st.window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .iter(world)
                .next();
        }
        // Releases scheduled by `tap` / `click` go out one frame after the press.
        for k in std::mem::take(&mut qa.st.release_next) {
            send_key(world, &mut qa.st, k, false);
        }
        for b in std::mem::take(&mut qa.st.buttons_next) {
            send_button(world, &qa.st, b, false);
        }
        if qa.finished {
            if qa.exit_at.is_some_and(|t| now >= t) {
                let fail = qa.st.checks.iter().any(|c| !c.ok);
                gothic_portcov::dump();
                world.write_message(AppExit::from_code(u8::from(fail)));
                qa.exit_at = Some(f32::MAX);
            }
            return;
        }
        if !qa.ready {
            if world_ready(world) && world.resource::<Time>().elapsed_secs() > 5.0 {
                let since = *qa.ready_since.get_or_insert(now);
                if now - since > 3.0 {
                    qa.ready = true;
                    eprintln!("qa: world ready at {now:.1}s, scenario {}", qa.st.scenario);
                }
            } else {
                qa.ready_since = None;
            }
            if (now as u32).is_multiple_of(5) && qa.st.n.insert("last_dbg".into(), f64::from(now as u32)) != Some(f64::from(now as u32)) {
                let stream = world.resource::<Stream>();
                let roster = world.resource::<Roster>();
                eprintln!(
                    "qa: waiting at {now:.0}s: stream.ready={} in_flight={} pending={} placed={} roster queue={} (in ring {}) binds={} spawned={}/{} hamlar={}",
                    stream.ready,
                    stream.in_flight.len(),
                    stream.pending.len(),
                    world.resource::<Player>().placed,
                    roster.queue.len(),
                    super::npcs::pending_in_ring(roster, stream),
                    roster.binds.len(),
                    roster.spawned,
                    roster.total,
                    world.resource::<CoreRes>().0.find_name("Hamlar").is_some()
                );
            }
            if now > 20.0 && qa.st.n.insert("pend_dbg".into(), 1.0).is_none() {
                let stream = world.resource::<Stream>();
                let roster = world.resource::<Roster>();
                let coll = world.resource::<crate::walk::Collision>();
                let hero = world.resource::<Player>().ctrl.position;
                for p in roster.queue.iter().take(6).map(|i| &roster.cands[*i as usize]) {
                    let c = stream.settings.cell_at(p.pos_cm);
                    eprintln!(
                        "qa: pending {} cell=({},{}) wanted={} resident={} chunk={} dist={:.0}m ground={:?}",
                        p.record.name,
                        c.x,
                        c.z,
                        stream.streamer.wanted().contains(&c),
                        stream.streamer.resident().contains(&c),
                        coll.0.has_chunk(crate::walk::tile_chunk_id((c.x, c.z))),
                        (p.pos_cm - hero).length() * 0.01,
                        coll.0.ground_below(Vec3::new(p.pos_cm.x, p.pos_cm.y + 50.0, p.pos_cm.z), 800.0)
                    );
                }
            }
            if now > 400.0 {
                finish(world, qa, now, Some("world never became ready".into()));
            }
            return;
        }
        // Real-time guard.
        if qa.st.started_real.elapsed().as_secs_f32() > qa.max_secs + 400.0 {
            finish(world, qa, now, Some("scenario time budget exceeded".into()));
            return;
        }
        let mut cx = Cx {
            world,
            st: &mut qa.st,
            now,
            t: 0.0,
        };
        for w in &mut qa.watchers {
            w(&mut cx);
        }
        let Some(step) = qa.steps.get_mut(qa.cur) else {
            finish(world, qa, now, None);
            return;
        };
        if !step.started {
            step.started = true;
            step.t0 = now;
            eprintln!("qa[{:>6.1}s] step {}: {}", now, qa.cur, step.name);
        }
        let mut cx = Cx {
            world,
            st: &mut qa.st,
            now,
            t: now - step.t0,
        };
        match (step.f)(&mut cx) {
            Poll::Wait => {}
            Poll::Done => qa.cur += 1,
            Poll::Fail(why) => {
                let name = step.name.clone();
                qa.st.checks.push(Check {
                    name: format!("step {name}"),
                    ok: false,
                    detail: why.clone(),
                });
                eprintln!("qa: FATAL at step {name}: {why}");
                finish(world, qa, now, None);
            }
        }
    });
}

fn finish(world: &mut World, qa: &mut Qa, now: f32, fatal: Option<String>) {
    if let Some(f) = fatal {
        qa.st.checks.push(Check {
            name: "harness".into(),
            ok: false,
            detail: f,
        });
    }
    // Release everything still held.
    let held: Vec<KeyCode> = qa.st.held.iter().copied().collect();
    for k in held {
        send_key(world, &mut qa.st, k, false);
    }
    let pass = qa.st.checks.iter().all(|c| c.ok);
    let mut txt = String::new();
    for c in &qa.st.checks {
        txt.push_str(&format!(
            "{} {}: {}\n",
            if c.ok { "PASS" } else { "FAIL" },
            c.name,
            c.detail
        ));
    }
    for n in &qa.st.notes {
        txt.push_str(&format!("note: {n}\n"));
    }
    txt.push_str(&format!("RESULT {}\n", if pass { "PASS" } else { "FAIL" }));
    let _ = std::fs::write(qa.st.dir.join("result.txt"), &txt);
    println!(
        "{txt}QA RESULT {} {}",
        qa.st.scenario,
        if pass { "PASS" } else { "FAIL" }
    );
    qa.finished = true;
    qa.exit_at = Some(now + 1.5);
}

// ---- input injection ----------------------------------------------------------------------------

fn send_key(world: &mut World, st: &mut QaState, key: KeyCode, down: bool) {
    let Some(window) = st.window else { return };
    if down {
        st.held.insert(key);
    } else {
        st.held.remove(&key);
    }
    world.write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: None,
        repeat: false,
        window,
    });
}

fn send_button(world: &mut World, st: &QaState, button: MouseButton, down: bool) {
    let Some(window) = st.window else { return };
    let ev = MouseButtonInput {
        button,
        state: if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        window,
    };
    // The winit runner emits the specific message and the `WindowEvent` wrapper that picking reads.
    world.write_message(WindowEvent::MouseButtonInput(ev.clone()));
    world.write_message(ev);
}

/// A button of the UI with its centre in logical window pixels.
#[derive(Clone, Debug)]
pub struct UiBtn {
    pub entity: Entity,
    pub center: Vec2,
    pub size: Vec2,
    pub text: String,
}

impl Cx<'_> {
    // ---- input ----
    pub fn key_down(&mut self, k: KeyCode) {
        if !self.st.held.contains(&k) {
            send_key(self.world, self.st, k, true);
        }
    }
    pub fn key_up(&mut self, k: KeyCode) {
        if self.st.held.contains(&k) {
            send_key(self.world, self.st, k, false);
        }
    }
    pub fn is_held(&self, k: KeyCode) -> bool {
        self.st.held.contains(&k)
    }
    /// Press now, release next frame.
    pub fn tap(&mut self, k: KeyCode) {
        send_key(self.world, self.st, k, true);
        self.st.release_next.push(k);
    }
    pub fn release_all(&mut self) {
        let held: Vec<KeyCode> = self.st.held.iter().copied().collect();
        for k in held {
            send_key(self.world, self.st, k, false);
        }
    }
    pub fn mouse_down(&mut self, b: MouseButton) {
        send_button(self.world, self.st, b, true);
    }
    pub fn mouse_up(&mut self, b: MouseButton) {
        send_button(self.world, self.st, b, false);
    }
    /// Press now, release next frame.
    /// The original use key: the left button (talk, take, open) while no weapon is drawn.
    pub fn use_key(&mut self) {
        self.mouse_click(MouseButton::Left);
    }

    pub fn mouse_click(&mut self, b: MouseButton) {
        send_button(self.world, self.st, b, true);
        self.st.buttons_next.push(b);
    }
    pub fn mouse_motion(&mut self, delta: Vec2) {
        self.world.write_message(MouseMotion { delta });
    }
    /// Move the pointer to logical window pixels.
    pub fn cursor_to(&mut self, p: Vec2) {
        let Some(window) = self.st.window else { return };
        let delta = Some(p - self.st.cursor);
        self.st.cursor = p;
        if let Some(mut w) = self.world.get_mut::<Window>(window) {
            w.set_cursor_position(Some(p));
        }
        let ev = CursorMoved {
            window,
            position: p,
            delta,
        };
        self.world
            .write_message(WindowEvent::CursorMoved(ev.clone()));
        self.world.write_message(ev);
    }
    pub fn cursor_grabbed(&mut self) -> bool {
        self.world
            .query_filtered::<&CursorOptions, With<PrimaryWindow>>()
            .iter(self.world)
            .next()
            .is_some_and(|c| c.grab_mode != CursorGrabMode::None)
    }

    // ---- UI ----
    pub fn buttons(&mut self) -> Vec<UiBtn> {
        let scale = self
            .world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .iter(self.world)
            .next()
            .map_or(1.0, |w| w.scale_factor());
        let mut q = self
            .world
            .query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform, &Hovered), With<Button>>(
            );
        let raw: Vec<(Entity, Vec2, Vec2)> = q
            .iter(self.world)
            .map(|(e, n, t, _)| (e, t.translation / scale, n.size() / scale))
            .collect();
        raw.into_iter()
            .map(|(entity, center, size)| UiBtn {
                entity,
                center,
                size,
                text: self.text_under(entity),
            })
            .collect()
    }

    /// All text below `root`, joined by `|`.
    pub fn text_under(&self, root: Entity) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if let Some(t) = self.world.get::<Text>(e) {
                out.push(t.0.clone());
            }
            if let Some(c) = self.world.get::<Children>(e) {
                for ch in c.iter().rev() {
                    stack.push(ch);
                }
            }
        }
        out.join("|")
    }

    /// Visible UI text containing `needle` (any node).
    pub fn find_text(&mut self, needle: &str) -> Option<Vec2> {
        let scale = self
            .world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .iter(self.world)
            .next()
            .map_or(1.0, |w| w.scale_factor());
        let mut q = self
            .world
            .query::<(&Text, &UiGlobalTransform, &ComputedNode)>();
        q.iter(self.world)
            .find(|(t, _, n)| t.0.contains(needle) && n.size().x > 0.0)
            .map(|(_, g, _)| g.translation / scale)
    }

    pub fn all_text(&mut self) -> Vec<String> {
        let mut q = self.world.query::<&Text>();
        q.iter(self.world).map(|t| t.0.clone()).collect()
    }

    // ---- state ----
    pub fn core(&self) -> &CoreRes {
        self.world.resource::<CoreRes>()
    }
    pub fn core_mut(&mut self) -> Mut<'_, CoreRes> {
        self.world.resource_mut::<CoreRes>()
    }
    pub fn player(&self) -> &Player {
        self.world.resource::<Player>()
    }
    pub fn play(&self) -> &PlayState {
        self.world.resource::<PlayState>()
    }
    pub fn inter(&self) -> &Interaction {
        self.world.resource::<Interaction>()
    }
    pub fn ui(&self) -> &gothic_ui::UiState {
        self.world.resource::<gothic_ui::UiState>()
    }
    /// Hero position in the core's space (centimetres).
    pub fn hero_pos(&self) -> Vec3 {
        let c = self.core();
        c.0.sim
            .world
            .actors
            .get(&c.0.hero)
            .map_or(Vec3::ZERO, |w| Vec3::new(w.0.x, w.0.y, w.0.z))
    }
    pub fn actor_pos(&self, id: gothic_sim::types::ActorId) -> Option<Vec3> {
        self.core()
            .0
            .sim
            .world
            .actors
            .get(&id)
            .map(|w| Vec3::new(w.0.x, w.0.y, w.0.z))
    }
    pub fn gold(&self) -> u32 {
        let c = self.core();
        c.0.sim.actor(c.0.hero).inventory.count("It_Gold")
    }
    pub fn hero_hp(&self) -> i32 {
        let c = self.core();
        c.0.sim.actor(c.0.hero).hp
    }
    pub fn count_item(&self, template: &str) -> u32 {
        let c = self.core();
        c.0.sim.actor(c.0.hero).inventory.count(template)
    }
    pub fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        eprintln!("qa: {s}");
        self.st.notes.push(s);
    }
    pub fn check(&mut self, name: &str, ok: bool, detail: impl Into<String>) {
        let detail = detail.into();
        eprintln!(
            "qa: check {name}: {} ({detail})",
            if ok { "ok" } else { "FAIL" }
        );
        self.st.checks.push(Check {
            name: name.into(),
            ok,
            detail,
        });
    }

    pub fn shot(&mut self, name: &str) {
        self.st.shot_no += 1;
        let path = self
            .st
            .dir
            .join(format!("{:02}_{name}.png", self.st.shot_no));
        self.world
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }

    /// Hash of the persistent core state.
    pub fn state_hash(&mut self) -> u64 {
        self.core_mut().0.state_hash()
    }

    // ---- movement helpers ----

    /// Turn the camera toward `goal` (cm) through mouse motion. Returns the remaining yaw error.
    pub fn aim_at(&mut self, goal: Vec3, pitch: f32) -> f32 {
        let hero = self.player().ctrl.position;
        let d = goal - hero;
        let want = super::yaw_of_dir(d.x, d.z);
        let (yaw, pit) = {
            let p = self.player();
            (p.yaw, p.pitch)
        };
        let dy = super::angle_delta(yaw, want);
        let dp = pitch - pit;
        if self.cursor_grabbed() {
            // player.yaw -= dx * 0.0025; pitch -= dy * 0.0025.
            self.mouse_motion(Vec2::new(-dy * 0.2 / 0.0024, -dp * 0.2 / 0.0024));
        } else {
            let mut p = self.world.resource_mut::<Player>();
            p.yaw += dy * 0.4;
            p.pitch += dp * 0.4;
        }
        dy.abs()
    }

    /// Walk toward `goal` (cm) with W (and Shift when `run`), steering by mouse motion.
    /// Done when within `stop` cm horizontally.
    pub fn walk_to(&mut self, goal: Vec3, stop: f32, run: bool) -> Poll {
        let hero = self.player().ctrl.position;
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        let dist = flat(goal - hero).length();
        if dist <= stop {
            self.key_up(KeyCode::KeyW);
            return Poll::Done;
        }
        if flat(hero).distance(flat(self.st.walk_ref)) > 60.0 || self.t < 0.05 {
            self.st.walk_ref = hero;
            self.st.walk_ref_t = self.now;
            self.st.walk_detour *= 0.5;
        } else if self.now - self.st.walk_ref_t > 1.5 {
            self.st.walk_detour = if self.st.walk_detour.abs() < 0.1 {
                1.1
            } else {
                -self.st.walk_detour * 1.2
            };
            self.st.walk_ref_t = self.now;
        }
        let dir =
            Quat::from_rotation_y(self.st.walk_detour) * flat(goal - hero).normalize_or_zero();
        let aim = hero + dir * 1000.0;
        self.aim_at(aim, -0.15);
        self.key_down(KeyCode::KeyW);
        let _ = run;
        Poll::Wait
    }

    /// Click the centre of a button whose text contains `needle` (move the pointer first, click
    /// on a later frame so hover is established).
    pub fn click_button_text(&mut self, needle: &str) -> bool {
        let btns = self.buttons();
        let Some(b) = btns.iter().find(|b| b.text.contains(needle)) else {
            return false;
        };
        self.cursor_to(b.center);
        self.st.n.insert("pending_click".into(), 1.0);
        true
    }
}

/// A step that moves the pointer onto `find` (retrying until it exists) and clicks it.
pub fn click_step(
    sc: &mut Sc,
    name: &str,
    mut find: impl FnMut(&mut Cx) -> Option<Vec2> + Send + Sync + 'static,
) {
    let name = name.to_string();
    let mut phase = 0u32;
    let mut at = 0.0f32;
    let n2 = name.clone();
    sc.step(format!("click {name}"), move |cx| match phase {
        0 => match find(cx) {
            Some(p) => {
                cx.cursor_to(p);
                phase = 1;
                at = cx.now;
                Poll::Wait
            }
            None if cx.t > 10.0 => Poll::Fail(format!("nothing to click: {n2}")),
            None => Poll::Wait,
        },
        1 => {
            if cx.now - at > 0.25 {
                let hovered: Vec<String> = {
                    let mut q = cx.world.query::<(Entity, &Hovered)>();
                    let es: Vec<Entity> = q
                        .iter(cx.world)
                        .filter(|(_, h)| h.get())
                        .map(|(e, _)| e)
                        .collect();
                    es.into_iter().map(|e| cx.text_under(e)).collect()
                };
                eprintln!("qa: click {n2} at {:?}, hovered: {hovered:?}", cx.st.cursor);
                cx.mouse_down(MouseButton::Left);
                phase = 2;
                at = cx.now;
            }
            Poll::Wait
        }
        2 => {
            if cx.now - at > 0.12 {
                let pressed: Vec<String> = {
                    let mut q = cx.world.query_filtered::<Entity, With<bevy::ui::Pressed>>();
                    let es: Vec<Entity> = q.iter(cx.world).collect();
                    es.into_iter().map(|e| cx.text_under(e)).collect()
                };
                eprintln!("qa: pressed while held: {pressed:?}");
                cx.mouse_up(MouseButton::Left);
                phase = 3;
                at = cx.now;
            }
            Poll::Wait
        }
        _ => {
            if cx.now - at > 0.25 {
                phase = 0;
                Poll::Done
            } else {
                Poll::Wait
            }
        }
    });
}
