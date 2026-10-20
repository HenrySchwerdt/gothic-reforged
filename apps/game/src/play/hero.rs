//! The animated hero: input (original bindings, see `controls.rs`), controller-to-animation
//! bridge and the interaction focus.
use super::anim::{CombatStance, Fallen, OverlayQueue, queue_steps};
use super::controls::{
    Act, DOUBLE_TAP_SECS, LONG_SECS, TAP_SECS, TURN_RATE, just_pressed, pressed, quick_slot_pressed,
};
use super::interact::Interaction;
use super::objects::WorldObjects;
use super::pose::Posed;
use super::{
    HeroActor, Mode, NpcActor, PlayState, ScriptAct, ScriptInput, Target, angle_delta,
    bevy_to_sim_yaw, yaw_of_dir,
};
use crate::walk::{Player, PlayerBody, PlayerCamera, player_motion, update_camera};
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_animation::locomotion::GaitHint;
use gothic_animation::{ActorSpawner, LocomotionInput, Stance, WeaponSpec};
use gothic_runtime::{Command, CoreCommand, CoreRes, HERO_REACH, SavePath};
use gothic_sim::item::{Equipment, Use, WeaponMode};
use gothic_sim::script::enums::Action as ScriptAction;
use gothic_sim::script::player::evade::Direction;
use gothic_sim::types::{ActorId, AniState};
use gothic_sim::world::Pos;
use gothic_ui::{OpenScreen, UiState};

const CM: f32 = 0.01;
/// Talk range of the interaction focus (cm).
pub const TALK_RANGE: f32 = 300.0;
/// Reach for objects, loose items and bodies (cm) [I: the focus ranges are template data].
pub const USE_RANGE: f32 = 250.0;
/// How far ahead the hero picks a target for bow and spells (cm) [I].
pub const RANGED_REACH: f32 = 3500.0;

pub fn register(app: &mut App) {
    app.add_systems(Startup, spawn_hero)
        .add_systems(
            Update,
            (place_hero, face_target, input, focus)
                .chain()
                .before(player_motion)
                .after(crate::walk::input_mode),
        )
        .add_systems(
            Update,
            (sync, super::anim::overlay_system, sword_visibility)
                .chain()
                .after(update_camera),
        );
}

fn spawn_hero(mut spawner: ActorSpawner, mut state: ResMut<PlayState>, kit: Res<super::HeroKit>) {
    // PC_Hero wears Head_Player and Body_Player only (resolved from its entity data); the
    // sword is the start-kit item.
    let resolved = kit
        .look
        .as_ref()
        .map(|l| gothic_animation::appearance::resolve(&spawner.library, l));
    let mut spec = match resolved {
        Some(Ok(spec)) => spec,
        other => {
            if let Some(Err(why)) = other {
                eprintln!("play: PC_Hero appearance unresolved: {why}");
            }
            return;
        }
    }
    .with_stance(Stance::Unarmed);
    if let Some(mesh) = &kit.sword_mesh {
        spec = spec.with_weapon(WeaponSpec::one_hand_sword(mesh));
    }
    match spawner.spawn(&spec, Transform::from_scale(Vec3::splat(CM))) {
        Ok(e) => {
            spawner.commands.entity(e).insert((
                HeroActor,
                OverlayQueue::default(),
                CombatStance(Stance::OneHand),
                Visibility::Hidden,
            ));
            state.hero_entity = Some(e);
        }
        Err(e) => eprintln!("hero spawn failed: {e:#}"),
    }
}

/// Where the hero turns to instead of the wish direction: the dialog partner, the fight
/// target, or the aim direction while a ranged weapon is drawn (`face` of `MoveCommand`).
fn face_target(
    mut player: ResMut<Player>,
    state: Res<PlayState>,
    core: Res<CoreRes>,
    time: Res<Time>,
) {
    let pos = player.ctrl.position;
    let toward = |id: ActorId| {
        core.0
            .sim
            .world
            .actors
            .get(&id)
            .map(|w| Vec3::new(w.0.x - pos.x, 0.0, w.0.z - pos.z))
    };
    let mut face = None;
    if state.mode == Mode::Dialog {
        face = state.dialog_npc.and_then(toward);
    } else if let Some((id, t)) = state.combat_focus
        && time.elapsed_secs() - t < 2.5
    {
        face = toward(id);
    } else if state.drawn
        && matches!(
            core.0.sim.actor(core.0.hero).equipment.weapon_mode(),
            WeaponMode::Ranged | WeaponMode::Magic
        )
    {
        face = Some(Vec3::new(-player.yaw.sin(), 0.0, -player.yaw.cos()));
    }
    player.face = face.filter(|f| f.length_squared() > 1.0);
}

fn camera_forward_xz(cam: &Transform) -> Vec2 {
    let f = cam.rotation * Vec3::NEG_Z;
    Vec2::new(f.x, f.z).normalize_or_zero()
}

/// The animation stance of the hands (`Equipment` -> motion name tokens).
pub fn stance_of(eq: &Equipment) -> Stance {
    match (eq.left_use(), eq.right_use()) {
        (Use::Bow, _) => Stance::Bow,
        (Use::CrossBow, _) => Stance::CrossBow,
        (Use::Shield, Use::OneH) => Stance::OneHandShield,
        (_, Use::OneH) => Stance::OneHand,
        (_, Use::TwoH | Use::Axe | Use::Halberd) => Stance::TwoHand,
        (_, Use::Staff) => Stance::Staff,
        (_, Use::Cast) => Stance::Magic,
        (_, Use::Fist | Use::PhysicalFist) => Stance::Fist,
        (Use::Torch, _) => Stance::Torch,
        _ => Stance::Unarmed,
    }
}

/// What the player wants this frame, gathered from keyboard, mouse or the smoke script.
#[derive(Default, Debug)]
struct Wanted {
    confirm: bool,
    quick: Option<usize>,
    draw: bool,
    sneak: bool,
    jump: bool,
    dodge: Option<Direction>,
    left_down: bool,
    /// The left button went up after being held this many seconds.
    left_up: Option<f32>,
    parry_down: bool,
    parry_up: bool,
    end_interaction: bool,
    window_key: Option<usize>,
    window_enter: bool,
    window_close: bool,
}

#[derive(Default)]
pub struct InputLocal {
    left_since: Option<f32>,
    scripted_release_at: Option<f32>,
    parrying: bool,
    taps: [(f32, f32); 4],
    aiming: bool,
    quick_left: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    mut script: ResMut<ScriptInput>,
    ui: Res<UiState>,
    core: Res<CoreRes>,
    save: Res<SavePath>,
    objects: Res<WorldObjects>,
    mut inter: ResMut<Interaction>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut commands: MessageWriter<CoreCommand>,
    mut screens: MessageWriter<OpenScreen>,
    mut hero: Query<&mut OverlayQueue, With<HeroActor>>,
    time: Res<Time>,
    mut local: Local<InputLocal>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    let window_open = inter.window.is_some();
    let blocked = ui.is_open() || state.mode == Mode::Dialog || state.hero_down;
    let grabbed = cursor
        .single()
        .is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    let rot = Quat::from_rotation_y(player.yaw);

    // ---- gather ------------------------------------------------------------------------
    let mut w = Wanted::default();
    for a in script.acts.drain(..) {
        match a {
            ScriptAct::Confirm => w.confirm = true,
            ScriptAct::Quick(n) => w.quick = Some(n),
            ScriptAct::DrawWeapon => w.draw = true,
            ScriptAct::Sneak => w.sneak = true,
            ScriptAct::Jump => w.jump = true,
            ScriptAct::LeftClick(secs) => {
                w.left_down = true;
                local.left_since = Some(now);
                local.scripted_release_at = Some(now + secs);
            }
            ScriptAct::Parry(d) => {
                w.parry_down = d;
                w.parry_up = !d;
            }
            ScriptAct::WindowKey(n) => w.window_key = Some(n),
            ScriptAct::WindowEnter => w.window_enter = true,
            ScriptAct::WindowClose => w.window_close = true,
            ScriptAct::EndInteraction => w.end_interaction = true,
        }
    }
    if let Some(t) = local.scripted_release_at
        && now >= t
    {
        local.scripted_release_at = None;
        w.left_up = Some(local.left_since.take().map_or(0.0, |s| now - s));
    }
    if !blocked && !window_open {
        w.confirm |= just_pressed(&keys, Act::Confirm);
        w.quick = w.quick.or_else(|| quick_slot_pressed(&keys));
        w.draw |= just_pressed(&keys, Act::DrawWeapon);
        w.sneak |= just_pressed(&keys, Act::Sneak);
        w.jump |= just_pressed(&keys, Act::Jump);
        if grabbed {
            if buttons.just_pressed(MouseButton::Left) {
                w.left_down = true;
                local.left_since = Some(now);
            }
            if buttons.just_released(MouseButton::Left) && local.scripted_release_at.is_none() {
                w.left_up = Some(local.left_since.take().map_or(0.0, |s| now - s));
            }
            w.parry_down |= buttons.just_pressed(MouseButton::Right);
            w.parry_up |= buttons.just_released(MouseButton::Right);
        }
    }
    if window_open {
        w.window_key = w
            .window_key
            .or_else(|| quick_slot_pressed(&keys).map(|n| n + 1));
        w.window_enter |= just_pressed(&keys, Act::Confirm);
        w.window_close |= keys.just_pressed(KeyCode::Escape);
        if let Some(n) = w.window_key {
            inter.window_key = Some(n);
        }
        inter.window_enter |= w.window_enter;
        inter.window_close |= w.window_close;
    }

    // ---- save / load / screens -------------------------------------------------------------
    if just_pressed(&keys, Act::QuickSave) || just_pressed(&keys, Act::Save) {
        commands.write(CoreCommand(Command::Save(save.0.clone())));
    }
    if just_pressed(&keys, Act::QuickLoad) || just_pressed(&keys, Act::Load) {
        commands.write(CoreCommand(Command::Load(save.0.clone())));
    }
    if !blocked && !window_open {
        // The UI plugin owns L / I / C; M (documents) and B (spell book) have no screen yet.
        let _ = (Act::Documents, Act::SpellBook, Act::Menu);
        if just_pressed(&keys, Act::Inventory) {
            let _ = &mut screens;
        }
    }

    if blocked || window_open {
        player.scripted_wish = Some(Vec3::ZERO);
        player.scripted_jump = Some(false);
        if let Some(i) = script.choose.take() {
            commands.write(CoreCommand(Command::Choose { index: i }));
        }
        if std::mem::take(&mut script.end_dialog) {
            commands.write(CoreCommand(Command::EndDialog));
        }
        return;
    }

    // ---- movement ----------------------------------------------------------------------------
    let hero_actor = core.0.sim.actor(core.0.hero);
    let sneaking = hero_actor.ani_state == AniState::Sneak;
    state.sneaking = sneaking;
    let mut wish_local = Vec3::ZERO;
    if script.active {
        let wv = script.wish;
        wish_local = rot.inverse() * Vec3::new(wv.x, 0.0, wv.z);
    } else {
        if pressed(&keys, Act::Forward) {
            wish_local += Vec3::NEG_Z;
        }
        if pressed(&keys, Act::Back) {
            wish_local += Vec3::Z;
        }
        if pressed(&keys, Act::StrafeRight) {
            wish_local += Vec3::X;
        }
        if pressed(&keys, Act::StrafeLeft) {
            wish_local -= Vec3::X;
        }
        if pressed(&keys, Act::TurnLeft) {
            player.yaw += TURN_RATE * dt;
        }
        if pressed(&keys, Act::TurnRight) {
            player.yaw -= TURN_RATE * dt;
        }
    }
    player.scripted_wish = Some(wish_local);
    let gait = if sneaking {
        gothic_physics::Gait::Sneak
    } else if pressed(&keys, Act::Sprint) {
        gothic_physics::Gait::Sprint
    } else {
        gothic_physics::Gait::Run
    };
    player.scripted_gait = Some(gait);

    // Leaving a seat, bed or work station: any movement key (or the confirm key again).
    let moving_key = [Act::Forward, Act::Back, Act::StrafeLeft, Act::StrafeRight]
        .iter()
        .any(|a| just_pressed(&keys, *a));
    if (w.end_interaction || moving_key || w.confirm) && inter.holding_pose {
        commands.write(CoreCommand(Command::EndInteraction));
        inter.holding_pose = false;
        inter.release_pose = true;
        w.confirm = false;
    }

    // ---- sneak, jump, dodge ----------------------------------------------------------------
    if w.sneak {
        commands.write(CoreCommand(Command::ToggleSneak));
    }
    let jumped = w.jump && player.ctrl.grounded;
    player.scripted_jump = Some(jumped);
    if jumped {
        commands.write(CoreCommand(Command::Jump));
    }
    for (i, (act, dir)) in [
        (Act::Forward, Direction::Forward),
        (Act::Back, Direction::Backward),
        (Act::StrafeLeft, Direction::Left),
        (Act::StrafeRight, Direction::Right),
    ]
    .into_iter()
    .enumerate()
    {
        if just_pressed(&keys, act) {
            // [U] no dodge key is documented: a double tap of a direction key dodges.
            if now - local.taps[i].0 < DOUBLE_TAP_SECS && local.taps[i].1 == 1.0 {
                w.dodge = Some(dir);
                local.taps[i].1 = 0.0;
            } else {
                local.taps[i] = (now, 1.0);
            }
        }
    }
    if let Some(direction) = w.dodge {
        commands.write(CoreCommand(Command::Dodge { direction }));
    }

    // ---- weapons ---------------------------------------------------------------------------
    let mode = hero_actor.equipment.weapon_mode();
    if w.draw {
        let want = !state.drawn;
        match mode {
            WeaponMode::Ranged if want => {
                commands.write(CoreCommand(Command::ReadyRanged));
            }
            _ => {}
        }
        state.drawn = want && mode != WeaponMode::None;
        commands.write(CoreCommand(Command::SetDrawn(state.drawn)));
    }
    if let Some(d) = script.draw.take() {
        state.drawn = d;
        commands.write(CoreCommand(Command::SetDrawn(d)));
    }
    if let Some(n) = w.quick
        && let Some(Some(template)) = state.quick.get(n).cloned()
    {
        quick_use(&template, &core, &mut state, &mut commands);
    }

    // ---- targets -----------------------------------------------------------------------------
    let hero_cm = player.ctrl.position;
    // Bow and spells aim where the camera looks; melee strikes go where the hero faces.
    let aim_yaw = if mode == WeaponMode::Melee {
        state.hero_yaw
    } else {
        player.yaw
    };
    let fwd = Vec2::new(
        (-aim_yaw.sin()).clamp(-1.0, 1.0),
        (-aim_yaw.cos()).clamp(-1.0, 1.0),
    );
    // Melee strikes reach `HERO_REACH`; bow and spell target what is in front of the hero.
    let reach = if mode == WeaponMode::Melee {
        HERO_REACH
    } else {
        RANGED_REACH
    };
    let enemy = nearest_in_front(&core, hero_cm, fwd, reach);

    // ---- talk (legacy smoke flag) and confirm ------------------------------------------------
    let talk = std::mem::take(&mut script.talk);
    if (talk || w.confirm)
        && let Some(target) = state.target
    {
        match target {
            Target::Npc(a) | Target::Body(a) => {
                commands.write(CoreCommand(Command::Confirm { focus: a.0 + 1 }));
            }
            Target::Object(i) | Target::Item(i) => {
                if let Some(e) = objects.list.get(i).and_then(|o| o.entity) {
                    if matches!(target, Target::Item(_)) {
                        inter.pending_item = Some(i);
                    }
                    commands.write(CoreCommand(Command::Confirm { focus: e }));
                }
            }
        }
        return;
    }

    // ---- mouse -------------------------------------------------------------------------------
    let legacy_attack = std::mem::take(&mut script.attack);
    if legacy_attack {
        w.left_down = true;
        w.left_up = Some(0.1);
    }
    if !state.drawn {
        // Benutzen 1: the left button confirms while no weapon is drawn.
        if w.left_down && grabbed && !script.active {
            if let Some(target) = state.target {
                let focus = match target {
                    Target::Npc(a) | Target::Body(a) => Some(a.0 + 1),
                    Target::Object(i) | Target::Item(i) => {
                        objects.list.get(i).and_then(|o| o.entity)
                    }
                };
                if let Some(focus) = focus {
                    if let Target::Item(i) = target {
                        inter.pending_item = Some(i);
                    }
                    commands.write(CoreCommand(Command::Confirm { focus }));
                }
            }
        }
        return;
    }
    let target = state.target.and_then(|t| match t {
        Target::Npc(a) => Some(a),
        _ => None,
    });
    let target = enemy.or(target);
    if let Some(t) = target {
        state.combat_focus = Some((t, now));
    }
    match mode {
        WeaponMode::Melee => {
            if w.parry_down
                && let Some(t) = target
            {
                local.parrying = true;
                commands.write(CoreCommand(Command::Parade { target: t }));
            }
            if w.parry_up {
                local.parrying = false;
            }
            if let Some(held) = w.left_up {
                let airborne = !player.ctrl.grounded;
                let action = if airborne {
                    // The ported player states have no jump attack; the standard attack runs.
                    ScriptAction::Attack
                } else if local.parrying {
                    if held < TAP_SECS {
                        ScriptAction::QuickAttack
                    } else {
                        ScriptAction::PierceAttack
                    }
                } else if held < TAP_SECS {
                    ScriptAction::QuickAttack
                } else if held < LONG_SECS {
                    ScriptAction::Attack
                } else {
                    ScriptAction::PowerAttack
                };
                if let Ok(mut q) = hero.single_mut() {
                    match action {
                        ScriptAction::QuickAttack => queue_steps(
                            &mut q,
                            &[(
                                if time.elapsed_secs() as u32 % 2 == 0 {
                                    "QuickAttackR"
                                } else {
                                    "QuickAttackL"
                                },
                                "Hit",
                            )],
                        ),
                        ScriptAction::PowerAttack => {
                            queue_steps(
                                &mut q,
                                &[("PowerAttack", "Raise"), ("PowerAttack", "Hit")],
                            );
                        }
                        _ => queue_steps(&mut q, &[("Attack", "Raise"), ("Attack", "Hit")]),
                    }
                }
                if let Some(t) = target {
                    commands.write(CoreCommand(Command::MeleeMove { action, target: t }));
                }
            }
        }
        WeaponMode::Ranged => {
            if w.left_down && !local.aiming {
                local.aiming = true;
                commands.write(CoreCommand(Command::AimRanged { aim: true }));
                if let Ok(mut q) = hero.single_mut() {
                    queue_steps(&mut q, &[("Aim", "Raise"), ("Aim", "Aim")]);
                }
            }
            if w.left_up.is_some() && local.aiming {
                local.aiming = false;
                // `PS_Ranged_Shoot` returns to the ready state by itself.
                commands.write(CoreCommand(Command::ReleaseRanged { target }));
                if let Ok(mut q) = hero.single_mut() {
                    queue_steps(&mut q, &[("Aim", "Recover")]);
                }
            }
        }
        WeaponMode::Magic => {
            if let Some(held) = w.left_up {
                commands.write(CoreCommand(Command::CastSpell {
                    target,
                    power: held >= LONG_SECS,
                }));
                if let Ok(mut q) = hero.single_mut() {
                    queue_steps(
                        &mut q,
                        &[("Cast", "Raise"), ("Cast", "Hit"), ("Cast", "Recover")],
                    );
                }
            }
        }
        WeaponMode::None => {}
    }
    let _ = &mut local.quick_left;
}

/// Quick slot use: weapons are worn and drawn, bows readied, spells selected, everything else
/// used (`OnPlayerUseQuickSlot` -> `OnPlayerUseStack`).
fn quick_use(
    template: &str,
    core: &CoreRes,
    state: &mut PlayState,
    commands: &mut MessageWriter<CoreCommand>,
) {
    let sim = &core.0.sim;
    let hero = sim.actor(core.0.hero);
    let kind = sim
        .items
        .get(template)
        .map_or(Use::None, gothic_sim::item::ItemDef::use_kind);
    let worn = hero
        .equipment
        .all()
        .any(|i| i.name.eq_ignore_ascii_case(template));
    let say = |commands: &mut MessageWriter<CoreCommand>, c: Command| {
        commands.write(CoreCommand(c));
    };
    // A bow in the left hand makes every other hand combination unusable: put it away first.
    let left_bow = hero
        .equipment
        .left
        .as_ref()
        .filter(|i| matches!(i.use_kind(), Use::Bow | Use::CrossBow))
        .map(|i| i.name.clone());
    match kind {
        Use::OneH | Use::TwoH | Use::Axe | Use::Staff | Use::Halberd => {
            if !worn {
                if let Some(b) = left_bow {
                    say(commands, Command::UseItem { template: b });
                }
                say(
                    commands,
                    Command::UseItem {
                        template: template.to_string(),
                    },
                );
                state.drawn = true;
            } else {
                state.drawn = !state.drawn;
            }
            say(commands, Command::SetDrawn(state.drawn));
        }
        Use::Bow | Use::CrossBow => {
            if !worn {
                say(
                    commands,
                    Command::UseItem {
                        template: template.to_string(),
                    },
                );
            }
            // Ammunition goes into the right hand with the bow in the left.
            let ammo = if kind == Use::Bow { "Arrow" } else { "It_Bolt" };
            let ammo_worn = hero
                .equipment
                .right
                .as_ref()
                .is_some_and(|i| i.name.eq_ignore_ascii_case(ammo));
            if !ammo_worn && hero.inventory.keys().any(|k| k.eq_ignore_ascii_case(ammo)) {
                say(
                    commands,
                    Command::UseItem {
                        template: ammo.to_string(),
                    },
                );
            }
            state.drawn = !(state.drawn && worn);
            if state.drawn {
                say(commands, Command::ReadyRanged);
            }
            say(commands, Command::SetDrawn(state.drawn));
        }
        Use::Cast => {
            if let Some(b) = left_bow {
                say(commands, Command::UseItem { template: b });
            }
            say(
                commands,
                Command::SelectSpell {
                    template: template.to_string(),
                },
            );
            state.drawn = true;
            say(commands, Command::SetDrawn(true));
        }
        _ => say(
            commands,
            Command::UseItem {
                template: template.to_string(),
            },
        ),
    }
}

/// Nearest living character in front of the hero within `reach`.
fn nearest_in_front(core: &CoreRes, hero_cm: Vec3, fwd: Vec2, reach: f32) -> Option<ActorId> {
    let mut best: Option<(f32, ActorId)> = None;
    for a in &core.0.sim.actors {
        if a.id == core.0.hero || !a.is_alive() {
            continue;
        }
        let Some(p) = core.0.sim.world.actors.get(&a.id).map(|w| w.0) else {
            continue;
        };
        let d = Vec2::new(p.x - hero_cm.x, p.z - hero_cm.z);
        let dist = d.length();
        if dist > reach || dist < 1.0 {
            continue;
        }
        if d.normalize().dot(fwd) < 0.2 {
            continue;
        }
        if best.is_none_or(|b| dist < b.0) {
            best = Some((dist, a.id));
        }
    }
    best.map(|b| b.1)
}

/// The thing the camera looks at within reach: a character (talk), a body (loot), an interact
/// object or a loose item (`gCFocus_PS`; ranges and the view cone are [I]).
#[allow(clippy::too_many_arguments)]
fn focus(
    player: Res<Player>,
    core: Res<CoreRes>,
    objects: Res<WorldObjects>,
    mut state: ResMut<PlayState>,
    cam: Query<&Transform, With<PlayerCamera>>,
    pose: Query<&Posed, With<HeroActor>>,
) {
    if state.mode == Mode::Dialog {
        return;
    }
    if pose.single().is_ok() {
        // While the hero uses something, the used thing stays the target.
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let fwd = camera_forward_xz(cam);
    let hero = player.ctrl.position;
    let mut best: Option<(f32, Target)> = None;
    let mut consider = |score: f32, t: Target| {
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, t));
        }
    };
    for a in &core.0.sim.actors {
        if a.id == core.0.hero {
            continue;
        }
        let Some(p) = core.0.sim.world.actors.get(&a.id).map(|w| w.0) else {
            continue;
        };
        let d = Vec2::new(p.x - hero.x, p.z - hero.z);
        let dist = d.length();
        let range = if a.is_alive() { TALK_RANGE } else { USE_RANGE };
        if dist > range || dist < 1.0 {
            continue;
        }
        let cos = d.normalize().dot(fwd);
        if cos < 0.55 {
            continue;
        }
        let t = if a.is_alive() {
            Target::Npc(a.id)
        } else {
            Target::Body(a.id)
        };
        consider(dist / range + (1.0 - cos), t);
    }
    for (i, o) in objects.list.iter().enumerate() {
        let usable = if o.def.is_item {
            !o.taken && o.entity.is_some() && o.visual.is_some()
        } else {
            o.entity.is_some() && o.def.player_usable() && !o.taken
        };
        if !usable {
            continue;
        }
        let d = Vec2::new(o.pos_cm.x - hero.x, o.pos_cm.z - hero.z);
        let dist = d.length();
        if dist > USE_RANGE || dist < 1.0 {
            continue;
        }
        let cos = d.normalize().dot(fwd);
        if cos < 0.5 {
            continue;
        }
        let t = if o.def.is_item {
            Target::Item(i)
        } else {
            Target::Object(i)
        };
        consider(dist / USE_RANGE + (1.0 - cos), t);
    }
    state.target = best.map(|b| b.1);
    state.focus = match state.target {
        Some(Target::Npc(a)) => Some(a),
        _ => None,
    };
}

#[allow(clippy::too_many_arguments)]
fn sync(
    time: Res<Time>,
    player: Res<Player>,
    core: Res<CoreRes>,
    mut state: ResMut<PlayState>,
    mut commands: MessageWriter<CoreCommand>,
    mut q: Query<
        (
            &mut Transform,
            Option<&mut LocomotionInput>,
            &mut Visibility,
            &mut CombatStance,
        ),
        (With<HeroActor>, Without<Fallen>),
    >,
    mut body: Query<&mut Visibility, (With<PlayerBody>, Without<HeroActor>)>,
    mut vel: Local<Vec3>,
    npcs: Query<(&NpcActor, &Transform), Without<HeroActor>>,
) {
    if let Ok(mut v) = body.single_mut() {
        *v = Visibility::Hidden;
    }
    let dt = time.delta_secs().clamp(1.0e-3, 0.1);
    let pos = player.ctrl.position;
    let Ok((mut tf, input, mut vis, mut cstance)) = q.single_mut() else {
        return;
    };
    *vis = if player.placed && !state.hide_hero {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    state.last_hero_cm = Some(pos);
    // E01: the animation velocity is the controller's filtered local velocity (forward, right).
    let (vf, vr) = player.ctrl.local_ani_velocity();
    let h = player.ctrl.heading;
    let fwd = Vec3::new(-h.sin(), 0.0, -h.cos());
    let right = Vec3::new(h.cos(), 0.0, -h.sin());
    let mut v = fwd * vf + right * vr;
    v.y = if player.ctrl.grounded {
        0.0
    } else {
        player.ctrl.vertical_speed
    };
    *vel = v;
    let horizontal = Vec2::new(vel.x, vel.z);
    // Facing is the controller's heading (turn model); the target only feeds the turn-in-place.
    let mut target = None;
    if state.mode == Mode::Dialog
        && let Some(id) = state.dialog_npc
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        target = Some(yaw_of_dir(p.x - pos.x, p.z - pos.z));
    } else if let Some((id, t)) = state.combat_focus
        && time.elapsed_secs() - t < 2.5
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        target = Some(yaw_of_dir(p.x - pos.x, p.z - pos.z));
    }
    let _ = &npcs;
    state.hero_yaw = player.ctrl.heading;
    let turn_error = target.map_or(0.0, |t| angle_delta(state.hero_yaw, t));
    tf.translation = pos * CM;
    tf.rotation = Quat::from_rotation_y(state.hero_yaw);
    tf.scale = Vec3::splat(CM);
    let equipped = stance_of(&core.0.sim.actor(core.0.hero).equipment);
    cstance.0 = if equipped == Stance::Unarmed || equipped == Stance::Torch {
        Stance::OneHand
    } else {
        equipped
    };
    if let Some(mut input) = input {
        input.velocity = Vec3::new(vel.x, vel.y, vel.z);
        input.grounded = player.ctrl.grounded;
        input.stance = if state.drawn {
            equipped
        } else if equipped == Stance::Torch {
            Stance::Torch
        } else {
            Stance::Unarmed
        };
        input.sneak = state.sneaking;
        input.gait = match player.ctrl.machine.mode() {
            gothic_physics::MovementMode::Sneaking => Some(GaitHint::Sneak),
            gothic_physics::MovementMode::Walking => Some(GaitHint::Walk),
            gothic_physics::MovementMode::Running => Some(GaitHint::Run),
            gothic_physics::MovementMode::Sprinting => Some(GaitHint::Sprint),
            _ => None,
        };
        input.turn_error = if horizontal.length() < 40.0 {
            turn_error
        } else {
            0.0
        };
    }
    commands.write(CoreCommand(Command::HeroTransform {
        pos: Pos::new(pos.x, pos.y, pos.z),
        yaw: bevy_to_sim_yaw(state.hero_yaw),
    }));
}

/// The start sword's mesh is only shown while the sword is the weapon in the right hand (a
/// drawn bow or spell replaces it; the bow itself has no mesh on the hero yet).
fn sword_visibility(
    core: Res<CoreRes>,
    mut weapons: Query<(&ChildOf, &mut Visibility), With<gothic_animation::runtime::WeaponMount>>,
    hero_entity: Query<Entity, With<HeroActor>>,
) {
    let Ok(h) = hero_entity.single() else { return };
    let sim = &core.0.sim;
    let right = &sim.actor(core.0.hero).equipment.right;
    let sword_in_hand = right
        .as_ref()
        .is_some_and(|i| i.use_kind().is_melee_weapon());
    for (parent, mut vis) in &mut weapons {
        if parent.parent() != h {
            continue;
        }
        let want = if sword_in_hand {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Puts the hero on the ground at the original start once its tile collision is resident
/// (nearest surface below the recorded height, not the roof above it).
fn place_hero(
    mut player: ResMut<Player>,
    collision: Res<crate::walk::Collision>,
    stream: Res<crate::walk::Stream>,
) {
    if player.placed || player.fly {
        return;
    }
    let p = player.ctrl.position;
    let cell = stream.settings.cell_at(p);
    if !collision
        .0
        .has_chunk(crate::walk::tile_chunk_id((cell.x, cell.z)))
    {
        return;
    }
    if let Some(y) = collision
        .0
        .ground_below(Vec3::new(p.x, p.y + 150.0, p.z), 900.0)
    {
        player.ctrl.position.y = y + 5.0;
        player.placed = true;
    }
}
