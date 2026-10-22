//! The hero's movement states: what the script does with the engine's movement mode machine.
//!
//! The body is moved by the ported engine (`CharacterController::step_command`, see
//! `walk.rs::player_motion`). This layer is the host side of `OnMovementModeChanged`
//! (S:adbf0) and its states:
//!
//! * every mode change of the machine is forwarded to the simulation
//!   (`Command::HeroModeChanged` -> `Sim::movement_mode_changed`): fall damage by the fall time,
//!   `Perk_Acrobatic`, `PS_Swim` (items away, swim ani state), the hero's death by a fall;
//! * the animation side: `ZS_HeavyLand` (the `Fall_Hit` clip, the hero stands still while it
//!   plays), `ZS_Slide` (`Slide_Begin` / `Slide_End`), the wade and swim poses of the locomotion
//!   clips, `ZS_DiveIn` (the swim pose's `Fall_Hit`);
//! * `OnSlide` / `PS_Slide_Jump`: jump out of a slide (the controller does the physics);
//! * swimming up / down: the `Up` / `Down` session keys (jump / duck) while in the water;
//! * the stamina tick's view of the body (`IsSprinting`, jumping) and the timed interactions
//!   (`PS_Interact`: `StopMovement`, the hero stands still until the use is over).
//!
//! Sprinting: the engine and the stamina rules support it (mode 5, 600 cm/s, `Move_Sprint_*`,
//! -10 stamina per tick), but nothing in the player's script states or in
//! `gCCharacterControl_PS` selects it for the player; NPCs sprint through `ZS_Attack_Sprint`.
//! The play layer therefore never requests it (`OnPlayerWalk`, S:638b0, only sets walk / run).
use super::controls::InputFrame;
use super::pose::Posed;
use super::{HeroActor, PlayState};
use crate::walk::{Player, Stream, WaterSurfaces, player_motion, update_camera};
use bevy::prelude::*;
use gothic_animation::locomotion::{BodyPose, MoveKind};
use gothic_animation::{Locomotion, LocomotionInput};
use gothic_input::SessionKey;
use gothic_physics::MovementMode;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::moves::ModeTask;
use gothic_sim::vitals::Movement;

pub fn register(app: &mut App) {
    app.init_resource::<MoveStates>()
        .add_systems(
            Update,
            before_motion
                .after(super::hero::input)
                .before(player_motion),
        )
        .add_systems(
            Update,
            (after_motion, on_outcome)
                .chain()
                .after(player_motion)
                .before(update_camera),
        );
}

/// What the layer remembers between frames, and counters the QA scenarios read.
#[derive(Resource, Default)]
pub struct MoveStates {
    /// Elapsed time of the last mode change (`PSRoutine::GetStateTime` base).
    mode_since: f32,
    /// The hero chose to dive (down key held while swimming) and has not surfaced yet.
    diving: bool,
    /// Last `Movement` sent for the stamina tick.
    sent_move: Option<Movement>,
    /// Number of mode changes forwarded.
    pub forwarded: u32,
    /// The latest outcome of `OnMovementModeChanged`.
    pub last_task: Option<ModeTask>,
    /// Hit points the last fall cost.
    pub last_fall_damage: i32,
    /// Landings whose damage the simulation applied.
    pub landings: u32,
    /// Clips requested: heavy landing, slide, dive-in, slide jumps.
    pub heavy_lands: u32,
    pub slides: u32,
    pub dive_ins: u32,
    pub slide_jumps: u32,
    /// Seconds spent per movement mode (`gECharMovementMode` value), for the QA reports.
    pub mode_secs: std::collections::BTreeMap<u8, f32>,
}

impl MoveStates {
    /// `mode=seconds` list of the time spent in each movement mode.
    pub fn mode_summary(&self) -> String {
        self.mode_secs
            .iter()
            .map(|(m, s)| format!("{m}={s:.1}s"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Before the controller step: the hero stands still while `ZS_HeavyLand` plays and while a
/// timed interaction runs; the dive input.
fn before_motion(
    frame: Res<InputFrame>,
    mut ms: ResMut<MoveStates>,
    mut player: ResMut<Player>,
    hero: Query<(&Locomotion, Option<&Posed>), With<HeroActor>>,
    inter: Res<super::interact::Interaction>,
    stream: Res<Stream>,
    water: Res<WaterSurfaces>,
) {
    // The water level of the lake / river meshes under the hero (`gCWaterZone_PS`); the sea
    // level covers the open sea.
    let feet = player.ctrl.position;
    let cell = stream.settings.cell_at(feet);
    player.scripted_water = water
        .level_at((cell.x, cell.z), feet, 400.0, 1500.0)
        .map(Some);
    let mode = player.ctrl.machine.mode();
    // Swimming up and down: `SessionKey::Up` is the jump key, `Down` the duck key.
    if matches!(mode, MovementMode::Swimming | MovementMode::Diving) {
        let up = frame.down(SessionKey::Jump);
        let down = frame.down(SessionKey::Duck);
        if down {
            ms.diving = true;
        }
        if ms.diving && !down && player.ctrl.dive_depth() <= 0.5 {
            ms.diving = false;
        }
        let v = f32::from(u8::from(up)) - f32::from(u8::from(down));
        player.scripted_dive = Some(if ms.diving { v } else { 0.0 });
        // The jump key means "up" in the water, not a jump.
        player.scripted_jump = Some(false);
    } else {
        ms.diving = false;
        player.scripted_dive = None;
    }
    if let Ok((loco, posed)) = hero.single() {
        let heavy = loco.key.kind == MoveKind::HeavyLand;
        // `PS_Interact`: `StopMovement`, then the use; held poses end on a movement key.
        let timed_use = inter.active.is_some()
            && posed.is_some_and(|p| !p.is_releasing())
            && !inter.holding_pose;
        if heavy || timed_use {
            player.scripted_wish = Some(Vec3::ZERO);
            player.scripted_jump = Some(false);
        }
    }
}

/// After the controller step: forward the mode changes, set the animation inputs.
fn after_motion(
    time: Res<Time>,
    core: Res<CoreRes>,
    mut ms: ResMut<MoveStates>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    mut commands: MessageWriter<CoreCommand>,
    mut hero: Query<&mut LocomotionInput, With<HeroActor>>,
) {
    let now = time.elapsed_secs();
    let report = std::mem::take(&mut player.report);
    let changes = player.ctrl.drain_mode_changes();
    let acrobatic = core.0.sim.actor(core.0.hero).has_perk("perk_acrobatic");
    let mut heavy = false;
    let mut dive_in = false;
    for ch in changes {
        // The time spent in the state that ended: the controller's fall clock for the fall
        // mode (it restarts at the jump apex), our own otherwise.
        let secs = if ch.previous == MovementMode::Falling {
            report.landed.map_or(now - ms.mode_since, |l| l.fall_time)
        } else {
            now - ms.mode_since
        };
        ms.mode_since = now;
        ms.forwarded += 1;
        if ch.previous == MovementMode::Falling {
            if ch.mode == MovementMode::Swimming {
                dive_in = true;
                ms.dive_ins += 1;
            } else if let Some(l) = report.landed
                && l.damage > 0
                && !acrobatic
                && ch.mode != MovementMode::Sliding
            {
                heavy = true;
                ms.heavy_lands += 1;
            }
        }
        if ch.mode == MovementMode::Sliding {
            ms.slides += 1;
        }
        if ch.previous == MovementMode::Sliding && ch.mode == MovementMode::Jumping {
            ms.slide_jumps += 1;
        }
        commands.write(CoreCommand(Command::HeroModeChanged {
            last: ch.previous.raw(),
            mode: ch.mode.raw(),
            state_secs: secs,
        }));
    }
    let mode = player.ctrl.machine.mode();
    *ms.mode_secs.entry(mode.raw()).or_insert(0.0) += time.delta_secs();
    // `PS_Swim_Loop` (S:af8e0) has no weapon handling: nothing is drawn while swimming.
    if matches!(mode, MovementMode::Swimming | MovementMode::Diving) && state.drawn {
        state.drawn = false;
        commands.write(CoreCommand(Command::SetDrawn(false)));
    }
    // The stamina tick (S:b0520): `IsSprinting`; jumping costs and gives nothing.
    let movement = match mode {
        MovementMode::Sprinting => Movement::Sprinting,
        MovementMode::Jumping => Movement::Jumping,
        _ => Movement::Idle,
    };
    if ms.sent_move != Some(movement) {
        ms.sent_move = Some(movement);
        commands.write(CoreCommand(Command::HeroMovement(movement)));
    }
    if let Ok(mut input) = hero.single_mut() {
        input.body = match mode {
            MovementMode::Wading => BodyPose::Wade,
            MovementMode::Swimming | MovementMode::Diving => BodyPose::Swim,
            _ => BodyPose::Stand,
        };
        input.slide = mode == MovementMode::Sliding;
        if heavy {
            input.heavy_land = true;
        } else if report.landed.is_some() || !player.ctrl.grounded {
            input.heavy_land = false;
        }
        input.dive_in = dive_in;
    }
}

/// The answer of `OnMovementModeChanged`: the data effects the body has to follow.
fn on_outcome(
    mut events: MessageReader<CoreEvent>,
    mut ms: ResMut<MoveStates>,
    mut state: ResMut<PlayState>,
    mut commands: MessageWriter<CoreCommand>,
) {
    for e in events.read() {
        let Event::ModeChanged(o) = &e.0 else {
            continue;
        };
        ms.last_task = Some(o.task);
        if o.damage > 0 {
            ms.last_fall_damage = o.damage;
            ms.landings += 1;
        }
        // `PS_Swim`: `HoldStacks(-1, -1)` puts every held item away.
        if matches!(o.task, ModeTask::Swim | ModeTask::DiveIn) && state.drawn {
            state.drawn = false;
            commands.write(CoreCommand(Command::SetDrawn(false)));
        }
    }
}
