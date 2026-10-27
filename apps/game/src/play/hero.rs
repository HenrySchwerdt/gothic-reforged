//! The animated hero: input (original bindings, see `controls.rs`), controller-to-animation
//! bridge and the interaction focus.
use super::anim::{CombatStance, Fallen, OverlayQueue};
use super::controls::{Act, InputFrame, just_pressed, quick_slot_pressed};
use super::interact::Interaction;
use super::objects::WorldObjects;
use super::pose::Posed;
use super::{
    HeroActor, Mode, NpcActor, PlayState, ScriptAct, ScriptInput, Target, angle_delta,
    bevy_to_sim_yaw, yaw_of_dir,
};
use crate::walk::{Player, PlayerBody, PlayerCamera, player_motion, update_camera};
use bevy::prelude::*;
use gothic_animation::locomotion::GaitHint;
use gothic_animation::{ActorSpawner, LocomotionInput, Stance, WeaponSpec};
use gothic_gameplay::focus::{
    Candidate as FocusCandidate, Class as FocusClass, CombatMode as FocusMode, FocusConfig,
    Viewer as FocusViewer, find_focus,
};
use gothic_input::player::{Hand, MeleeFacts, Task as PlayerTask, msg};
use gothic_input::{Context, Order, SessionKey};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event, SavePath};
use gothic_sim::item::{Equipment, Use, WeaponMode};
use gothic_sim::script::routine::state::State as ScriptState;
use gothic_sim::types::{ActorId, AniState};
use gothic_sim::world::Pos;

const CM: f32 = 0.01;
/// Weapon reach plus the hero's half size, the scale of the focus range rating (cm) [I].
const FOCUS_REACH: f32 = 190.0;
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
    app.add_systems(Update, key_events.after(input));
    super::hero_moves::register(app);
    super::move_states::register(app);
}

/// The quick slot and potion keys the melee loop took out of the queue (`PS_Melee_Loop`'s jump
/// table calls `OnPlayerUseQuickSlot` / `OnPlayerPotionQuickUse`, which the host performs).
fn key_events(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    mut state: ResMut<PlayState>,
    mut commands: MessageWriter<CoreCommand>,
) {
    for e in events.read() {
        let Event::Player(p) = &e.0 else { continue };
        match p {
            gothic_sim::script::player::api::PlayerEvent::QuickSlot(n) => {
                // `QuickUse n` is named after its key: n = 1..9 then 0, so key 0 is slot 10.
                let slot = ((*n + 9) % 10) as usize;
                if let Some(Some(template)) = state.quick.get(slot).cloned() {
                    quick_use(
                        &template,
                        &core,
                        &mut state,
                        PlayerTask::Melee,
                        None,
                        &mut commands,
                    );
                }
            }
            gothic_sim::script::player::api::PlayerEvent::PotionKey(n) => {
                if let Some(template) = potion_of(&core, *n) {
                    commands.write(CoreCommand(Command::UseItem { template }));
                }
            }
            _ => {}
        }
    }
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
    // The bow sits on the back (`Slot_Bow`) until it is drawn into the left hand; the arrow
    // shows in the right hand while the bow is up (holster slot = drawn slot marks it).
    if let Some(mesh) = &kit.arrow_mesh {
        spec = spec.with_weapon(WeaponSpec {
            mesh: mesh.clone(),
            drawn_slot: "Slot_RightHand_Weapon".into(),
            holster_slot: "Slot_RightHand_Weapon".into(),
            offset: Transform::IDENTITY,
        });
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

/// Where the hero turns to instead of the wish direction (`face` of `MoveCommand`): the dialog
/// partner, the alignment target the hero's script set (`Entity::SetAlignmentTarget` with a
/// movement constraint of 360: `PS_Melee_*`, `gCCharacterMovement_PS::
/// ProcessControledRotationToTarget`, G:0x100e3fb0), or the aim direction while a ranged weapon
/// or a spell is drawn.
/// The camera direction is a unit vector; `face` ignores vectors shorter than a centimetre (a target
/// standing on the hero), so it is given a length in the same unit.
const CAMERA_ALIGN_LENGTH: f32 = 100.0;

fn face_target(mut player: ResMut<Player>, state: Res<PlayState>, core: Res<CoreRes>) {
    let pos = player.ctrl.position;
    let toward = |id: ActorId| {
        core.0
            .sim
            .world
            .actors
            .get(&id)
            .map(|w| Vec3::new(w.0.x - pos.x, 0.0, w.0.z - pos.z))
    };
    let (align, constraint) = core.0.sim.hero_alignment();
    let mut face = None;
    if state.mode == Mode::Dialog {
        face = state.dialog_npc.and_then(toward);
    } else if let Some(t) = align
        && constraint >= gothic_sim::script::player::melee::CONSTRAINT_DEGREES
    {
        face = toward(t);
    } else if constraint == gothic_sim::script::player::ranged::FOLLOW_CAMERA_CONSTRAINT
        && matches!(
            core.0.sim.hero_script_state().0,
            Some(ScriptState::PsRanged | ScriptState::PsMagic)
        )
    {
        // `ProcessMovements` (G:0xddd40): with the constraint at 0 the body is aligned to the
        // camera direction every frame (`SetAlignmentLookAt`), whatever the wish direction.
        face = Some(Vec3::new(-player.yaw.sin(), 0.0, -player.yaw.cos()) * CAMERA_ALIGN_LENGTH);
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

/// What the smoke script and the window keys ask for this frame (the session keys go through
/// `InputFrame` and the router).
#[derive(Default, Debug)]
struct Wanted {
    confirm: bool,
    quick: Option<usize>,
    draw: bool,
    sneak: bool,
    jump: bool,
    end_interaction: bool,
    window_key: Option<usize>,
    window_enter: bool,
    window_close: bool,
}

/// `PS_*` task of the hero as the key router distinguishes them: the hero's real script task.
fn router_task(
    state: &PlayState,
    inter: &Interaction,
    script_task: Option<ScriptState>,
) -> PlayerTask {
    if state.mode == Mode::Dialog {
        PlayerTask::Talk
    } else if inter.holding_pose {
        PlayerTask::Interact
    } else {
        match script_task {
            Some(ScriptState::PsMelee) => PlayerTask::Melee,
            Some(ScriptState::PsRanged) => PlayerTask::Ranged,
            Some(ScriptState::PsMagic) => PlayerTask::Magic,
            _ => PlayerTask::Normal,
        }
    }
}

/// The potion of pool `n` (0 health, 1 mana, 2 stamina) the hero carries, by template name [I].
fn potion_of(core: &CoreRes, n: u32) -> Option<String> {
    let tokens: &[&str] = match n {
        0 => &["potion_health", "health"],
        1 => &["potion_mana", "mana"],
        _ => &["potion_stamina", "stamina", "energy"],
    };
    let hero = core.0.sim.actor(core.0.hero);
    for t in tokens {
        if let Some(k) = hero
            .inventory
            .keys()
            .find(|k| k.to_ascii_lowercase().contains(t) && hero.inventory.count(k) > 0)
        {
            return Some(k.to_string());
        }
    }
    None
}

/// The weapon key: out of `PS_Melee_Loop` it is an action in the player's queue (`Hold`, 68), out
/// of `PS_Normal_Loop` the script's `OnPlayerSelectWeapon` runs at once.
fn weapon_key(task: PlayerTask, focus: Option<ActorId>, commands: &mut MessageWriter<CoreCommand>) {
    if matches!(
        task,
        PlayerTask::Melee | PlayerTask::Ranged | PlayerTask::Magic
    ) {
        commands.write(CoreCommand(Command::PlayerAction {
            id: msg::HOLD,
            focus,
        }));
    } else {
        commands.write(CoreCommand(Command::SelectWeapon));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    mut script: ResMut<ScriptInput>,
    mut core: ResMut<CoreRes>,
    save: Res<SavePath>,
    objects: Res<WorldObjects>,
    mut inter: ResMut<Interaction>,
    mut frame: ResMut<InputFrame>,
    mut commands: MessageWriter<CoreCommand>,
    mover: Res<super::hero_moves::HeroMover>,
    time: Res<Time>,
) {
    let _ = time;
    let window_open = inter.window.is_some();
    let blocked = frame.blocked;
    let grabbed = frame.grabbed;
    let rot = Quat::from_rotation_y(player.yaw);

    // ---- gather (smoke script, window keys) ---------------------------------------------------
    let mut w = Wanted::default();
    for a in script.acts.drain(..) {
        match a {
            ScriptAct::Confirm => w.confirm = true,
            ScriptAct::Quick(n) => w.quick = Some(n),
            ScriptAct::DrawWeapon => w.draw = true,
            ScriptAct::Sneak => w.sneak = true,
            ScriptAct::Jump => w.jump = true,
            // The mouse buttons of the script are held by `controls::gather_input`.
            ScriptAct::LeftClick(_) | ScriptAct::Parry(_) => {}
            ScriptAct::WindowKey(n) => w.window_key = Some(n),
            ScriptAct::WindowEnter => w.window_enter = true,
            ScriptAct::WindowClose => w.window_close = true,
            ScriptAct::EndInteraction => w.end_interaction = true,
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

    // ---- save / load ---------------------------------------------------------------------------
    if just_pressed(&keys, Act::QuickSave) {
        commands.write(CoreCommand(Command::Save(save.0.clone())));
    }
    if just_pressed(&keys, Act::QuickLoad) {
        commands.write(CoreCommand(Command::Load(save.0.clone())));
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

    // ---- movement: the camera-relative wish of the four movement keys ---------------------------
    let hero_actor = core.0.sim.actor(core.0.hero);
    let sneaking = hero_actor.ani_state == AniState::Sneak;
    state.sneaking = sneaking;
    let wish_local = if script.active {
        let wv = script.wish;
        rot.inverse() * Vec3::new(wv.x, 0.0, wv.z)
    } else {
        let (right, forward) = frame.move_axes();
        Vec3::new(right, 0.0, -forward)
    };
    // A combat move in flight owns the body (`EnableCombatMovementFromSPU`): the keys do not
    // move the hero until the instruction is done.
    player.scripted_wish = Some(if mover.moving { Vec3::ZERO } else { wish_local });
    // `OnPlayerWalk`: the walk key (held, or the toggle) slows to the walk gait; run otherwise.
    let gait = if sneaking {
        gothic_physics::Gait::Sneak
    } else if frame.walk {
        gothic_physics::Gait::Walk
    } else {
        gothic_physics::Gait::Run
    };
    player.scripted_gait = Some(gait);

    // Leaving a seat, bed or work station: any movement key (or the use key again).
    let moving_key = [
        SessionKey::Forward,
        SessionKey::Backward,
        SessionKey::StrafeLeft,
        SessionKey::StrafeRight,
    ]
    .iter()
    .any(|k| frame.pressed_edge(*k));
    let movement_held = [
        SessionKey::Forward,
        SessionKey::Backward,
        SessionKey::StrafeLeft,
        SessionKey::StrafeRight,
    ]
    .iter()
    .any(|k| frame.down(*k));

    // ---- route the session keys -----------------------------------------------------------------
    let mode = hero_actor.equipment.weapon_mode();
    let (script_task, script_state, state_time) = core.0.sim.hero_script_state();
    let task = router_task(&state, &inter, script_task);
    let left = match hero_actor.equipment.left.as_ref().map(|i| i.use_kind()) {
        Some(Use::Bow) => Hand::Bow,
        Some(Use::CrossBow) => Hand::CrossBow,
        Some(Use::Staff) => Hand::Staff,
        Some(_) => Hand::Other,
        None => Hand::Empty,
    };
    let right_use = hero_actor.equipment.right.as_ref().map(|i| i.use_kind());
    let hero_ani = hero_actor.ani_state;
    let hero_down = matches!(
        hero_actor.ai_mode,
        gothic_sim::types::AiMode::Dead | gothic_sim::types::AiMode::Down
    );
    let cast_ready = right_use == Some(Use::Cast);
    // The melee layer's answers (`CanDo`, weapon classes) come from the ported script functions.
    let facts = core.bypass_change_detection().0.sim.player_control_facts();
    let ctx = Context {
        task,
        state: script_state.map_or("PS_Normal_Loop", ScriptState::name),
        state_time,
        ani_state: match hero_ani {
            AniState::Sneak => 3,
            AniState::Parade => 4,
            _ => 2,
        },
        left,
        ammo_in_hand: matches!(right_use, Some(Use::Arrow | Use::Bolt)),
        human: true,
        can_jump: player.ctrl.grounded,
        standing_or_braking: player.ctrl.velocity.length() < 20.0,
        keys_ignored: hero_down,
        hud_consumed: false,
        melee: MeleeFacts {
            can_do: facts.can_do,
            right_one_hander: facts.right_one_hander,
            two_hander_or_tool: facts.two_hander_or_tool,
            left_one_hander_or_torch: facts.left_one_hander_or_torch,
            finishing_target: facts.finishing_target,
        },
        cast_action: (cast_ready && facts.cast_action != 0).then_some(facts.cast_action),
        extended_hold: core.0.sim.cfg.ai_mode,
    };
    let msgs = frame.msgs.clone();
    let mut orders: Vec<Order> = Vec::new();
    for m in &msgs {
        orders.extend(frame.router.route(m, &ctx));
    }

    // Targets: bow and spells take the focus entity of the ranged / magic focus mode
    // (`PS_Ranged_Aim`: `PSFocus::SetFocusEntity(GetFocusEntity(ranged))`, `PS_Magic_Loop`: the
    // focus only for spells with `SpellTarget` = focus; the others are cast free along the
    // forward axis, `MagicProjectile`). The melee layer picks its own target (`PS_Melee_Loop`).
    let queue_focus = state.focus;

    if std::env::var_os("GR_INPUT_DEBUG").is_some() && !orders.is_empty() {
        eprintln!(
            "input: task={task:?} state={:?} orders={orders:?} drawn={}",
            script_state.map(ScriptState::name),
            state.drawn
        );
    }
    // ---- act on the orders ------------------------------------------------------------------------
    let mut use_pressed = false;
    let mut draw = w.draw;
    let mut quick = w.quick;
    let mut jump = w.jump;
    let mut sneak_toggle = w.sneak;
    for o in orders {
        match o {
            // In `PS_Melee` every queued action belongs to the loop state of the melee task:
            // attacks, the guard, the hop back, the jump, the sneak, the weapon key, the quick
            // slots and the potion keys (`PS_Melee_Loop`'s jump table).
            Order::Message(id) if task == PlayerTask::Melee => {
                commands.write(CoreCommand(Command::PlayerAction {
                    id,
                    focus: queue_focus,
                }));
            }
            // In `PS_Ranged` the aim, shoot, reload and abort keys, the weapon key, the quick
            // slots and the potions are read from the queue by the loop states
            // (`PS_Ranged_Walk_Loop`, `PS_Ranged_Aim_Loop`, `OnPowerAim_Loop`). Jump and sneak
            // also move the body, which the host does at once.
            Order::Message(id)
                if matches!(task, PlayerTask::Ranged | PlayerTask::Magic)
                    && !matches!(id, msg::JUMP | msg::SNEAK) =>
            {
                commands.write(CoreCommand(Command::PlayerAction {
                    id,
                    focus: queue_focus,
                }));
            }
            Order::Message(id) => match id {
                msg::JUMP => jump = true,
                msg::SNEAK => sneak_toggle = true,
                msg::HOLD => draw = true,
                msg::USE => use_pressed = true,
                m if (msg::QUICK_SLOT_BASE..msg::QUICK_SLOT_BASE + 10).contains(&m) => {
                    // `QuickUse n` is named after its key: n = 1..9 then 0, so key 0 is slot 10.
                    quick = Some(((m - msg::QUICK_SLOT_BASE + 9) % 10) as usize);
                }
                m if (msg::POTION_BASE..msg::POTION_BASE + 3).contains(&m) => {
                    if let Some(template) = potion_of(&core, m - msg::POTION_BASE) {
                        commands.write(CoreCommand(Command::UseItem { template }));
                    }
                }
                _ => {}
            },
            Order::EnterState(name) => match name {
                // `FullStop` + `SetState`: the riposte out of the guard, the chains of the
                // attack key, the guard released after 2.5 s.
                n if n.starts_with("PS_Melee") => {
                    commands.write(CoreCommand(Command::PlayerEnterState { name: n.into() }));
                }
                // The cast key let go too early: the spell is not cast.
                "PS_Magic_Loop" => {
                    commands.write(CoreCommand(Command::AbortCast));
                }
                _ => {}
            },
            Order::Walk(_)
            | Order::SneakHeld(_)
            | Order::ToggleHud
            | Order::SkipTalkLine
            | Order::ConfirmTransformed => {}
        }
    }

    if (w.end_interaction || moving_key || use_pressed || w.confirm) && inter.holding_pose {
        commands.write(CoreCommand(Command::EndInteraction));
        inter.holding_pose = false;
        inter.release_pose = true;
        use_pressed = false;
        w.confirm = false;
    }

    // ---- the control layer's view for the script (`PSFocus`, `PSCamera`, the key latch) --------
    commands.write(CoreCommand(Command::HeroControl {
        melee_focus: state.melee_focus,
        focus: state.focus,
        first_person: state.hide_hero && !state.keep_items,
        movement_key: movement_held,
    }));

    // ---- sneak, jump -------------------------------------------------------------------------------
    if sneak_toggle {
        commands.write(CoreCommand(Command::ToggleSneak));
    }
    let jumped = jump && player.ctrl.grounded;
    player.scripted_jump = Some(jumped);
    if jumped {
        commands.write(CoreCommand(Command::Jump));
    }

    // ---- weapons -------------------------------------------------------------------------------------
    if draw {
        if mode == WeaponMode::Melee
            || matches!(
                task,
                PlayerTask::Melee | PlayerTask::Ranged | PlayerTask::Magic
            )
        {
            // The weapon key: `OnPlayerSelectWeapon`; the draw is the task's `_AI_Hold...`.
            weapon_key(task, queue_focus, &mut commands);
        } else {
            // Bow and spells keep the host-side draw until their states are ported.
            let want = !state.drawn;
            if mode == WeaponMode::Ranged && want {
                commands.write(CoreCommand(Command::ReadyRanged));
            }
            state.drawn = want && mode != WeaponMode::None;
            commands.write(CoreCommand(Command::SetDrawn(state.drawn)));
        }
    }
    if let Some(d) = script.draw.take() {
        if mode == WeaponMode::Melee
            || matches!(
                task,
                PlayerTask::Melee | PlayerTask::Ranged | PlayerTask::Magic
            )
        {
            if d != (core.0.sim.actor(core.0.hero).drawn) {
                weapon_key(task, queue_focus, &mut commands);
            }
        } else {
            state.drawn = d;
            commands.write(CoreCommand(Command::SetDrawn(d)));
        }
    }
    if let Some(n) = quick
        && let Some(Some(template)) = state.quick.get(n).cloned()
    {
        quick_use(
            &template,
            &core,
            &mut state,
            task,
            queue_focus,
            &mut commands,
        );
    }

    // ---- talk (legacy smoke flag), the Enter key and the use key -------------------------------------
    let talk = std::mem::take(&mut script.talk);
    // `USE` (message 63) belongs to `PS_Normal` / `PS_Interact`: with a weapon drawn the left
    // button is the attack, aim or cast key and never talks. A click that only captured the
    // cursor is not a use.
    let use_click = use_pressed && !state.drawn && (grabbed || script.active);
    if (talk || w.confirm || use_click)
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
    }
}

/// Quick slot use: weapons are worn and drawn, bows readied, spells selected, everything else
/// used (`OnPlayerUseQuickSlot` -> `OnPlayerUseStack`).
fn quick_use(
    template: &str,
    core: &CoreRes,
    state: &mut PlayState,
    task: PlayerTask,
    queue_focus: Option<ActorId>,
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
                // The draw follows once the item sits in its slot.
                if !hero.drawn {
                    weapon_key(task, queue_focus, commands);
                }
            } else {
                // The same weapon again: draw or put away.
                weapon_key(task, queue_focus, commands);
            }
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

/// The thing the hero looks at: a character (talk), a body (loot), an interact object or a loose
/// item. Selection is the port of `gCFocus_PS::FindFocusEntity` (ranges, view windows and
/// priorities of the hero template, `gothic_gameplay::focus`); the view direction is the camera's.
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
    // Candidates for the ported focus rules (`gothic_gameplay::focus`); `ids` maps a
    // candidate's index back to what it stands for. Sizes and the line of sight are the host's
    // estimates [I]: half-extents of 40 cm for characters and objects, 15 cm for items.
    // The focus mode follows the hero's combat task (`PSFocus.CurrentMode`).
    let mode = match core.0.sim.hero_script_state().0 {
        Some(ScriptState::PsMelee) => FocusMode::Melee,
        Some(ScriptState::PsRanged) => FocusMode::Ranged,
        Some(ScriptState::PsMagic) => FocusMode::Magic,
        _ if state.drawn => match core.0.sim.actor(core.0.hero).equipment.weapon_mode() {
            WeaponMode::Ranged => FocusMode::Ranged,
            WeaponMode::Magic => FocusMode::Magic,
            _ => FocusMode::Normal,
        },
        _ => FocusMode::Normal,
    };
    // CollectInteractionEntities: only what lies within the largest scan range of the mode
    // (`gCFocus_PS::GetCurrentMaxRange`, Game:0x1006c1d0). The world holds tens of thousands of
    // objects and every character the hero ever met, so reject by distance before collecting.
    let max_range = FocusConfig::hero().max_range(mode);
    let near = |p: [f32; 3]| (p[0] - hero.x).hypot(p[2] - hero.z) <= max_range;
    let mut ids: Vec<Target> = Vec::new();
    let mut cands: Vec<FocusCandidate> = Vec::new();
    let mut push = |t: Target, class: FocusClass, p: [f32; 3], radius: f32| {
        cands.push(FocusCandidate {
            id: ids.len(),
            class,
            pos: p,
            radius,
            high_priority: false,
            script_priority: 0.0,
            los: true,
        });
        ids.push(t);
    };
    for a in &core.0.sim.actors {
        if a.id == core.0.hero {
            continue;
        }
        let Some(p) = core.0.sim.world.actors.get(&a.id).map(|w| w.0) else {
            continue;
        };
        if !near([p.x, p.y, p.z]) {
            continue;
        }
        let t = if a.is_alive() {
            Target::Npc(a.id)
        } else {
            Target::Body(a.id)
        };
        push(t, FocusClass::Character, [p.x, p.y, p.z], 40.0);
    }
    for (i, o) in objects.list.iter().enumerate() {
        if !near([o.pos_cm.x, o.pos_cm.y, o.pos_cm.z]) || !objects.docs[o.doc as usize].on {
            continue;
        }
        let usable = if o.def.is_item {
            !o.taken && o.entity.is_some() && o.visual.is_some()
        } else {
            o.entity.is_some() && o.def.player_usable() && !o.taken
        };
        if !usable {
            continue;
        }
        let p = [o.pos_cm.x, o.pos_cm.y, o.pos_cm.z];
        if o.def.is_item {
            push(Target::Item(i), FocusClass::Item, p, 15.0);
        } else {
            push(Target::Object(i), FocusClass::Interact, p, 40.0);
        }
    }
    let viewer = FocusViewer {
        pos: [hero.x, hero.y, hero.z],
        yaw: fwd.x.atan2(fwd.y),
        reach: FOCUS_REACH,
    };
    let cfg = FocusConfig::hero();
    let found = find_focus(&cfg, mode, &viewer, &cands);
    state.target = found.map(|i| ids[i]);
    state.focus = match state.target {
        Some(Target::Npc(a)) => Some(a),
        _ => None,
    };
    // `PSFocus::GetFocusEntity(melee)` for `PS_Melee_Loop`: the same search in the melee mode.
    state.melee_focus = find_focus(&FocusConfig::hero(), FocusMode::Melee, &viewer, &cands)
        .and_then(|i| match ids[i] {
            Target::Npc(a) | Target::Body(a) => Some(a),
            _ => None,
        });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
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
    parts: Query<&gothic_animation::runtime::PartMeshes, With<HeroActor>>,
    mut part_vis: Query<&mut Visibility, (Without<HeroActor>, Without<PlayerBody>)>,
    mut body_hidden: Local<bool>,
) {
    if let Ok(mut v) = body.single_mut() {
        *v = Visibility::Hidden;
    }
    // Special bow mode / first person: the body parts go, the held items stay.
    let hide_body = state.hide_hero && state.keep_items;
    if hide_body != *body_hidden {
        *body_hidden = hide_body;
        if let Ok(p) = parts.single() {
            for (_, es) in &p.0 {
                for e in es {
                    if let Ok(mut v) = part_vis.get_mut(*e) {
                        *v = if hide_body {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        };
                    }
                }
            }
        }
    }
    let pos = player.ctrl.position;
    // The weapon is drawn when the script's hold put it in the hand (`PSInventory::HoldStacks`).
    state.drawn = core.0.sim.actor(core.0.hero).drawn;
    let Ok((mut tf, input, mut vis, mut cstance)) = q.single_mut() else {
        return;
    };
    *vis = if player.placed && (!state.hide_hero || state.keep_items) {
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
    } else if let (Some(id), c) = core.0.sim.hero_alignment()
        && c >= gothic_sim::script::player::melee::CONSTRAINT_DEGREES
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        // `SetAlignmentTarget` + the 360 degree constraint: the body turns to the target.
        target = Some(yaw_of_dir(p.x - pos.x, p.z - pos.z));
    }
    // The target of the fight as the script wrote it (`PSNpc::SetCurrentTarget`) feeds the
    // health bar and the fight music.
    if matches!(core.0.sim.hero_script_state().0, Some(ScriptState::PsMelee))
        && let Some(t) = core.0.sim.hero_current_target()
    {
        state.combat_focus = Some((t, time.elapsed_secs()));
    }
    let _ = &npcs;
    state.hero_yaw = player.ctrl.heading;
    let turn_error = target.map_or(0.0, |t| angle_delta(state.hero_yaw, t));
    tf.translation = pos * CM;
    // The body keeps the heading: with a ranged weapon up `ProcessMovements` aligns it to the
    // camera (`face_target`), the aim clips are played as they are.
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
        input.parade = core.0.sim.actor(core.0.hero).ani_state == AniState::Parade;
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

/// Which held items show: the sword while a melee weapon is in the right hand, the bow while
/// one is equipped (on the back until drawn), the arrow while a bow is up with ammunition.
fn sword_visibility(
    core: Res<CoreRes>,
    state: Res<PlayState>,
    mut weapons: Query<(
        &ChildOf,
        &gothic_animation::runtime::WeaponMount,
        &mut Visibility,
    )>,
    hero_entity: Query<(Entity, &gothic_animation::runtime::CharacterComposition), With<HeroActor>>,
) {
    let Ok((h, comp)) = hero_entity.single() else {
        return;
    };
    let left_joint = comp.0.joint_by_name.get("slot_lefthand_weapon").copied();
    let sim = &core.0.sim;
    let eq = &sim.actor(core.0.hero).equipment;
    let sword_in_hand = eq
        .right
        .as_ref()
        .is_some_and(|i| i.use_kind().is_melee_weapon());
    let bow_equipped = matches!(eq.left_use(), Use::Bow | Use::CrossBow);
    let arrow_up = state.drawn
        && bow_equipped
        && eq
            .right
            .as_ref()
            .is_some_and(|i| matches!(i.use_kind(), Use::Arrow | Use::Bolt));
    for (parent, mount, mut vis) in &mut weapons {
        if parent.parent() != h {
            continue;
        }
        let show = if Some(mount.drawn_joint) == left_joint {
            bow_equipped
        } else if mount.drawn_joint == mount.holster_joint {
            arrow_up
        } else {
            sword_in_hand
        };
        let want = if show {
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
