//! Bevy plugin: collects keyboard / mouse / gamepad input, steps the Souls core at a fixed 60 Hz
//! and publishes its events as [`SoulsEvent`] messages.
//!
//! Hosts drive the core in one of two ways:
//!
//! * standalone (arena demo, tests): leave [`SoulsPlugin::manual`] off. The plugin steps the world
//!   with the boxed [`SoulsBackend`] (any [`HitSink`] / [`Env`]).
//! * in the game: set `manual` and add a system that calls [`SoulsRes::advance`] with a
//!   [`crate::sink::SimSink`] over the real simulation and an [`Env`] backed by the physics
//!   controller. Nothing else in this module needs to know about the game.
use crate::animmap::AnimMap;
use crate::events::Event;
use crate::hero::{Button, Input};
use crate::lockcam::LockCam;
use crate::sink::{HitSink, SimpleSink};
use crate::tuning::{DT, Tuning};
use crate::world::{Env, OpenField, SoulsWorld};
use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseButton};
use bevy::prelude::*;
use glam::Vec3 as GVec3;

/// Mode switch (`--combat souls`) as a resource so other systems can ask.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoulsMode {
    /// Enemies telegraph their attacks.
    pub telegraph: bool,
}

impl Default for SoulsMode {
    fn default() -> Self {
        Self { telegraph: true }
    }
}

#[derive(Resource)]
pub struct SoulsRes {
    pub world: SoulsWorld,
    pub anim: AnimMap,
    pub lock_cam: LockCam,
    /// Seconds not yet consumed by 60 Hz ticks.
    pub accum: f32,
}

impl SoulsRes {
    pub fn new(world: SoulsWorld) -> Self {
        Self {
            world,
            anim: AnimMap::gothic(),
            lock_cam: LockCam::default(),
            accum: 0.0,
        }
    }

    /// Feeds `dt` seconds of wall time; runs as many 60 Hz ticks as fit. `take_input` is asked for
    /// each tick's input (edges must be consumed by exactly one tick). Returns the tick events.
    pub fn advance(
        &mut self,
        dt: f32,
        mut take_input: impl FnMut() -> Input,
        sink: &mut dyn HitSink,
        env: &mut dyn Env,
    ) -> Vec<Event> {
        self.accum += dt.clamp(0.0, 0.1);
        let mut out = Vec::new();
        while self.accum >= DT {
            self.accum -= DT;
            let inp = take_input();
            out.extend_from_slice(self.world.step(&inp, sink, env));
        }
        out
    }
}

/// Core events as Bevy messages (animation, audio, HUD and scripts subscribe).
#[derive(Message, Clone, Copy, Debug)]
pub struct SoulsEvent(pub Event);

/// Camera yaw the stick is relative to; the host writes it every frame.
#[derive(Resource, Default, Clone, Copy)]
pub struct SoulsView {
    pub cam_yaw: f32,
}

/// Input collected between ticks. Edges stay latched until a tick takes them, so a press
/// shorter than a tick (or a frame without ticks) is never lost.
#[derive(Resource, Default)]
pub struct InputLatch {
    pub current: Input,
    /// When set, the script replaces the live devices (screenshot runs, tests).
    pub scripted: bool,
    switch_armed: bool,
}

impl InputLatch {
    pub fn take(&mut self) -> Input {
        let out = self.current;
        let c = &mut self.current;
        for b in [&mut c.dodge, &mut c.light, &mut c.heavy, &mut c.guard] {
            b.pressed = false;
            b.released = false;
        }
        c.lock = false;
        c.switch = 0;
        out
    }
}

fn merge(b: &mut Button, held: bool, was: bool) {
    b.pressed |= held && !was;
    b.released |= !held && was;
    b.held = held;
}

/// Default backend of the standalone loop.
#[derive(Resource)]
pub struct SoulsBackend {
    pub sink: Box<dyn HitSink + Send + Sync>,
    pub env: Box<dyn Env + Send + Sync>,
}

impl Default for SoulsBackend {
    fn default() -> Self {
        Self {
            sink: Box::new(SimpleSink::default()),
            env: Box::new(OpenField::default()),
        }
    }
}

#[derive(Default)]
pub struct SoulsPlugin {
    /// The host steps the world itself (see the module docs).
    pub manual: bool,
}

impl Plugin for SoulsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoulsMode>()
            .init_resource::<SoulsView>()
            .init_resource::<InputLatch>()
            .add_message::<SoulsEvent>()
            .add_systems(PreUpdate, gather_input.after(bevy::input::InputSystems));
        if !self.manual {
            app.init_resource::<SoulsBackend>()
                .add_systems(Update, standalone_tick);
        }
    }
}

/// Keyboard + mouse + gamepad -> [`Input`].
///
/// Keyboard: WASD move, Left Shift walk, Space dodge (tap) / sprint (hold), LMB light, RMB heavy
/// (hold to charge), Q guard (tap just before a hit to parry), Tab or MMB lock-on, mouse wheel
/// switches target. Gamepad: left stick, South/East dodge, R1 light, R2 heavy, L1 guard, R3 lock,
/// right stick flick switches target.
#[allow(clippy::too_many_arguments)]
pub fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    pads: Query<&Gamepad>,
    view: Res<SoulsView>,
    mut latch: ResMut<InputLatch>,
) {
    if latch.scripted {
        return;
    }
    let was = latch.current;
    let mut mv = Vec2::ZERO;
    let key = |k| keys.pressed(k);
    mv.x += f32::from(u8::from(key(KeyCode::KeyD))) - f32::from(u8::from(key(KeyCode::KeyA)));
    mv.y += f32::from(u8::from(key(KeyCode::KeyW))) - f32::from(u8::from(key(KeyCode::KeyS)));
    let mut dodge = key(KeyCode::Space);
    let mut light = mouse.pressed(MouseButton::Left);
    let mut heavy = mouse.pressed(MouseButton::Right);
    let mut guard = key(KeyCode::KeyQ);
    let mut lock = keys.just_pressed(KeyCode::Tab) || mouse.just_pressed(MouseButton::Middle);
    let mut walk = key(KeyCode::ShiftLeft);
    let mut switch = match scroll.delta.y {
        y if y > 0.5 => -1,
        y if y < -0.5 => 1,
        _ => 0,
    };
    if let Some(pad) = pads.iter().next() {
        let s = pad.left_stick();
        if s.length() > 0.15 {
            mv = s;
        }
        dodge |= pad.pressed(GamepadButton::South) || pad.pressed(GamepadButton::East);
        light |= pad.pressed(GamepadButton::RightTrigger);
        heavy |= pad.pressed(GamepadButton::RightTrigger2);
        guard |= pad.pressed(GamepadButton::LeftTrigger);
        lock |= pad.just_pressed(GamepadButton::RightThumb);
        walk |= mv.length() > 0.01 && mv.length() < 0.5;
        let rx = pad.get(GamepadAxis::RightStickX).unwrap_or(0.0);
        if rx.abs() > 0.7 {
            if !latch.switch_armed {
                switch = if rx > 0.0 { 1 } else { -1 };
                latch.switch_armed = true;
            }
        } else if rx.abs() < 0.3 {
            latch.switch_armed = false;
        }
    }
    let c = &mut latch.current;
    c.mv = mv.clamp_length_max(1.0).to_array().into();
    c.cam_yaw = view.cam_yaw;
    c.walk = walk;
    merge(&mut c.dodge, dodge, was.dodge.held);
    merge(&mut c.light, light, was.light.held);
    merge(&mut c.heavy, heavy, was.heavy.held);
    merge(&mut c.guard, guard, was.guard.held);
    c.lock |= lock;
    if switch != 0 {
        c.switch = switch;
    }
}

fn standalone_tick(
    time: Res<Time>,
    mut res: ResMut<SoulsRes>,
    mut backend: ResMut<SoulsBackend>,
    mut latch: ResMut<InputLatch>,
    mut out: MessageWriter<SoulsEvent>,
) {
    let SoulsBackend { sink, env } = &mut *backend;
    let events = res.advance(time.delta_secs(), || latch.take(), &mut **sink, &mut **env);
    for e in events {
        out.write(SoulsEvent(e));
    }
}

/// Converts between glam versions of Bevy and this crate (both are glam 0.33 today; kept as a
/// seam in case they diverge).
pub fn to_bevy(v: GVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn from_bevy(v: Vec3) -> GVec3 {
    GVec3::new(v.x, v.y, v.z)
}

/// Starting world for a standalone arena run.
pub fn arena_world(tuning: Tuning) -> SoulsRes {
    SoulsRes::new(SoulsWorld::new(GVec3::ZERO, 400.0, tuning))
}
