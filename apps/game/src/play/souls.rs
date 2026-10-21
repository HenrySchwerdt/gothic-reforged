//! `--play --combat souls`: the opt-in Souls-style combat flow (crate `gothic-combat-souls`) on
//! top of the real game. Classic combat stays the default; nothing here runs without the flag.
//!
//! What the flow takes over (only while the weapon is drawn, no dialog / window is open):
//! * input: the keyboard / mouse / pad actions of the crate (roll, light / heavy, guard, lock-on);
//!   the original bindings for these are swallowed so the old attack commands never fire;
//! * hero locomotion: rolls and lunges nudge the physics controller, free movement goes through
//!   the ported `step_command` pipeline with the lock-on target as `face`;
//! * the camera: while locked on, `LockCam` steers the orbit of the ported camera solver.
//!
//! What stays original: every damage number. Hero swings call `Sim::hit` (ported
//! `OnHit -> DoLogicalDamage -> AssessHit`) through `SimSink`; enemy swings are resolved by the
//! sim brain exactly as before. The flow only decides timing: i-frames set the sim's
//! `INVULNERABLE` status for their window, a raised guard puts the hero into the original
//! `Parade` action, and `--combat souls` also switches on the sim's NPC telegraph variant.
use super::anim::{CombatStance, find_motion, start_overlay};
use super::cam::PlayCam;
use super::{Mode, PlayState, Roster};
use crate::walk::{Collision, Player};
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton};
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator};
use gothic_combat_souls::actions::{ActionId, Hurt};
use gothic_combat_souls::animmap::first_playable;
use gothic_combat_souls::events::Event as SEvent;
use gothic_combat_souls::foe::FoeProfile;
use gothic_combat_souls::hero::{Gait as SGait, Hero, Motion};
use gothic_combat_souls::plugin::{
    InputLatch, SoulsEvent, SoulsMode, SoulsPlugin, SoulsRes, SoulsView, gather_input,
};
use gothic_combat_souls::sink::SimSink;
use gothic_combat_souls::world::Env;
use gothic_combat_souls::{SoulsWorld, Tuning};
use gothic_runtime::{CoreEvent, CoreRes, Event as CoreEv};
use gothic_sim::sim::EventKind;
use gothic_sim::types::{Action, ActorId, AiMode, status};
use gothic_ui::UiState;
use std::collections::HashMap;

/// Milliseconds an NPC swing is held (the sim's telegraph variant).
const TELEGRAPH_MS: u32 = 650;
/// Souls ids: the hero is 0, enemies are `actor id + 1`.
const HERO_ID: u32 = 0;

#[derive(SystemParam)]
struct Look<'w> {
    motion: ResMut<'w, AccumulatedMouseMotion>,
    scroll: ResMut<'w, AccumulatedMouseScroll>,
}

#[derive(SystemParam)]
struct Rig<'w, 's> {
    lib: ResMut<'w, ActorLibrary>,
    anims: Query<'w, 's, (&'static mut Animator, &'static CombatStance)>,
    roster: Res<'w, Roster>,
}

/// Whether the flow drives the hero this frame.
#[derive(Resource, Default)]
pub struct SoulsGate {
    pub active: bool,
}

/// Per-frame glue state.
#[derive(Resource, Default)]
struct Glue {
    /// Last locomotion the flow asked for; re-applied every frame.
    motion: Option<Motion>,
    /// Hero actor flags this module set (so they are only cleared by us).
    set_parade: bool,
    set_invulnerable: bool,
    /// Pending animation steps of the hero.
    anim: Vec<(String, String)>,
    /// Strikes of NPCs seen last frame: actor -> hit time.
    strikes: HashMap<ActorId, u64>,
}

pub fn register(app: &mut App) {
    // The telegraph variant of the simulation: NPC swings are held before they land.
    app.world_mut()
        .resource_mut::<CoreRes>()
        .0
        .sim
        .cfg
        .variants
        .npc_telegraph_ms = Some(TELEGRAPH_MS);
    app.add_plugins(SoulsPlugin { manual: true })
        .insert_resource(SoulsMode { telegraph: true })
        .insert_resource(SoulsRes::new(SoulsWorld::new(
            Vec3::ZERO,
            100.0,
            Tuning::default(),
        )))
        .init_resource::<SoulsGate>()
        .init_resource::<Glue>()
        .add_systems(
            PreUpdate,
            claim_input
                .after(gather_input)
                .after(bevy::input::InputSystems),
        )
        .add_systems(
            Update,
            (drive, animate)
                .chain()
                .after(super::hero::input)
                .after(crate::walk::input_mode)
                .before(crate::walk::player_motion),
        );
    eprintln!("play: --combat souls (opt-in Souls-style combat flow)");
    // A locally generated clip pack (if one exists) takes over the clips and action windows.
    super::er_pack::install(app);
}

/// Swallows the original bindings the flow replaces, while it is active.
fn claim_input(
    gate: Res<SoulsGate>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    if !gate.active {
        return;
    }
    for k in [KeyCode::Space, KeyCode::KeyQ, KeyCode::Tab] {
        keys.reset(k);
    }
    // Direction keys keep moving the hero, but their fresh presses must not look like the
    // double-tap that triggers the original dodge.
    for k in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
        keys.clear_just_pressed(k);
    }
    for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        mouse.reset(b);
    }
}

/// The locomotion backend of the flow, over the physics controller.
struct PlayEnv<'a> {
    player: &'a mut Player,
    collision: &'a gothic_physics::CollisionWorld,
    last: &'a mut Option<Motion>,
}

impl Env for PlayEnv<'_> {
    fn line_of_sight(&self, from: Vec3, to: Vec3) -> bool {
        let d = to - from;
        let len = d.length();
        len < 1.0 || self.collision.raycast(from, d / len, len - 20.0).is_none()
    }

    fn move_hero(&mut self, hero: &mut Hero, motion: &Motion, _t: &Tuning) {
        if let Some(root) = motion.root {
            self.player.ctrl.nudge(self.collision, root);
            if let Some(y) = motion.root_yaw {
                self.player.ctrl.heading = y;
            }
        }
        hero.pos = self.player.ctrl.position;
        hero.yaw = self.player.ctrl.heading;
        hero.speed = self.player.ctrl.velocity.length();
        *self.last = Some(*motion);
    }
}

fn sim_pos(core: &CoreRes, id: ActorId) -> Option<Vec3> {
    core.0
        .sim
        .world
        .actors
        .get(&id)
        .map(|w| Vec3::new(w.0.x, w.0.y, w.0.z))
}

fn hurt_level(result: gothic_sim::combat::HitResult) -> Hurt {
    use gothic_sim::combat::HitResult as R;
    match result {
        R::QuickStumble => Hurt::Small,
        R::Stumble | R::PierceStumble => Hurt::Large,
        R::SitKnockDown | R::LieKnockDown | R::LieKnockOut => Hurt::Knockdown,
        _ => Hurt::Middle,
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time>,
    mut core: ResMut<CoreRes>,
    mut player: ResMut<Player>,
    collision: Res<Collision>,
    state: Res<PlayState>,
    ui: Res<UiState>,
    mut res: ResMut<SoulsRes>,
    mut latch: ResMut<InputLatch>,
    mut view: ResMut<SoulsView>,
    mut gate: ResMut<SoulsGate>,
    mut glue: ResMut<Glue>,
    mut cam: ResMut<PlayCam>,
    mut look: Look,
    mut core_events: MessageReader<CoreEvent>,
    mut out: MessageWriter<SoulsEvent>,
    mut rig: Rig,
) {
    let hero_id = core.0.hero;
    let active = state.drawn
        && state.mode == Mode::Explore
        && !state.hero_down
        && state.cinematic.is_none()
        && !ui.is_open()
        && player.placed
        && !player.fly;
    gate.active = active;
    if !active {
        clear_flags(&mut core, hero_id, &mut glue);
        res.world.lock.target = None;
        // Leave the original controls in charge.
        glue.motion = None;
        for _ in core_events.read() {}
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let tuning = res.world.tuning;

    // ---- mirror the game into the flow ----------------------------------------------------
    {
        let h = &mut res.world.hero;
        h.pos = player.ctrl.position;
        h.yaw = player.ctrl.heading;
        let actor = core.0.sim.actor(hero_id);
        h.hp = actor.hp as f32;
        h.max_hp = actor.hp_max(&core.0.sim.cfg) as f32;
    }
    view.cam_yaw = cam.state.view_yaw();
    sync_foes(&mut res, &core);

    // ---- events the simulation produced since the last frame -------------------------------
    for e in core_events.read() {
        let CoreEv::Sim(EventKind::Hit {
            attacker,
            victim,
            damage,
            result,
            parried,
            ignored,
            ..
        }) = &e.0
        else {
            continue;
        };
        if *victim != hero_id || ignored.is_some() {
            continue;
        }
        let foe_id = attacker.0 + 1;
        if *parried || result.is_parry() {
            if res.world.hero.parry_active(&tuning) {
                if let Some(f) = res.world.foe_mut(foe_id) {
                    f.swing_result(gothic_combat_souls::HitResult::Parried);
                }
                res.world.hero.apply_external_block(0.0, &tuning);
            } else {
                let cost = (*damage as f32 * tuning.guard_stamina_ratio).max(6.0);
                res.world.hero.apply_external_block(cost, &tuning);
            }
        } else if *damage > 0 {
            let hp = core.0.sim.actor(hero_id).hp as f32;
            let poise = (*damage as f32 * 0.4).max(8.0);
            res.world
                .hero
                .apply_external_damage(hp, hurt_level(*result), poise, &tuning);
        }
    }

    // ---- step the flow ------------------------------------------------------------------------
    let mut sink = SimSink::new(&mut core.0.sim);
    sink.bind(HERO_ID, hero_id);
    for f in &res.world.foes {
        sink.bind(f.id, ActorId(f.id - 1));
    }
    let mut last = glue.motion;
    let mut env = PlayEnv {
        player: &mut player,
        collision: &collision.0,
        last: &mut last,
    };
    let SoulsRes { world, accum, .. } = &mut *res;
    *accum += dt;
    let mut events = Vec::new();
    while *accum >= gothic_combat_souls::tuning::DT {
        *accum -= gothic_combat_souls::tuning::DT;
        let inp = latch.take();
        events.extend_from_slice(world.step(&inp, &mut sink, &mut env));
    }
    glue.motion = last;

    // ---- feed the controller ---------------------------------------------------------------
    let target = res.world.lock_pos();
    if let Some(m) = glue.motion {
        let rot = Quat::from_rotation_y(player.yaw);
        let local = rot.inverse() * m.wish;
        player.scripted_wish = Some(if m.root.is_some() { Vec3::ZERO } else { local });
        player.scripted_gait = Some(match m.gait {
            SGait::Walk => gothic_physics::Gait::Walk,
            SGait::Run => gothic_physics::Gait::Run,
            SGait::Sprint => gothic_physics::Gait::Sprint,
        });
        player.face = if m.root.is_some() {
            None
        } else {
            m.face.map(|f| Vec3::new(f.x, 0.0, f.z))
        };
    }
    if let Some(t) = target {
        let d = t - player.ctrl.position;
        if d.length_squared() > 1.0 && glue.motion.is_some_and(|m| m.root.is_none()) {
            player.face = Some(Vec3::new(d.x, 0.0, d.z));
        }
    }
    player.scripted_jump = Some(false);

    // ---- camera ------------------------------------------------------------------------------
    let hero_pos = player.ctrl.position;
    if res.world.recenter {
        res.lock_cam.recenter();
    }
    let hero_yaw = player.ctrl.heading;
    let SoulsRes { lock_cam, .. } = &mut *res;
    lock_cam.drive(&mut cam.state, hero_pos, target, hero_yaw, dt);
    if target.is_some() {
        look.motion.delta = Vec2::ZERO;
    }
    look.scroll.delta = Vec2::ZERO;

    // ---- the simulation's view of the hero ---------------------------------------------------
    let invincible = res.world.hero.invincible();
    let guard = res.world.hero.guard_up(&tuning);
    let stamina_frac = res.world.hero.stamina / tuning.max_stamina;
    {
        let cfg_stamina = {
            let a = core.0.sim.actor(hero_id);
            a.stamina_max(&core.0.sim.cfg)
        };
        let a = core.0.sim.actor_mut(hero_id);
        if invincible {
            a.status |= status::INVULNERABLE;
            glue.set_invulnerable = true;
        } else if glue.set_invulnerable {
            a.status &= !status::INVULNERABLE;
            glue.set_invulnerable = false;
        }
        if guard && a.action == Action::None {
            a.action = Action::Parade;
            glue.set_parade = true;
        } else if !guard && glue.set_parade {
            if a.action == Action::Parade {
                a.action = Action::None;
            }
            glue.set_parade = false;
        }
        a.stamina = (stamina_frac * cfg_stamina as f32).round() as i32;
    }

    // ---- consequences in the simulation, messages, animation ----------------------------------
    let now = core.0.sim.now_ms();
    for e in &events {
        out.write(SoulsEvent(*e));
        match e {
            SEvent::FoeStaggered { foe, .. } => {
                // A broken foe loses its swing and stands open for a moment.
                let id = ActorId(foe - 1);
                if (id.0 as usize) < core.0.sim.actors.len() {
                    let a = core.0.sim.actor_mut(id);
                    a.brain.strike = None;
                    a.action = Action::None;
                    a.brain.busy_until_ms = now + 1600;
                }
            }
            SEvent::HeroAction(id) => {
                let steps = pick_steps(&res, *id, &mut rig.lib, &mut rig.anims, &state);
                if let Some(steps) = steps {
                    glue.anim = steps;
                }
            }
            _ => {}
        }
    }
    telegraph_visuals(
        &mut glue,
        &core,
        &rig.roster,
        &mut rig.anims,
        &mut rig.lib,
        now,
    );
}

fn clear_flags(core: &mut CoreRes, hero: ActorId, glue: &mut Glue) {
    if glue.set_invulnerable || glue.set_parade {
        let a = core.0.sim.actor_mut(hero);
        if glue.set_invulnerable {
            a.status &= !status::INVULNERABLE;
        }
        if glue.set_parade && a.action == Action::Parade {
            a.action = Action::None;
        }
        glue.set_invulnerable = false;
        glue.set_parade = false;
    }
}

/// Keeps one shadow foe per hostile NPC near the hero (poise, stagger, lock-on candidates). The
/// NPC brain keeps deciding its swings; the shadow is an observer.
fn sync_foes(res: &mut SoulsRes, core: &CoreRes) {
    let hero_id = core.0.hero;
    let Some(hero_pos) = sim_pos(core, hero_id) else {
        return;
    };
    let range = res.world.tuning.lock_break_range;
    let mut seen = Vec::new();
    for a in &core.0.sim.actors {
        if a.id == hero_id || a.is_down_or_dead() {
            continue;
        }
        let hostile = a.ai_mode == AiMode::Combat && a.target == Some(hero_id);
        let Some(p) = sim_pos(core, a.id) else {
            continue;
        };
        if !hostile || (p - hero_pos).length() > range {
            continue;
        }
        let id = a.id.0 + 1;
        seen.push(id);
        let hp = a.hp as f32;
        if res.world.foe(id).is_none() {
            let foe = res.world.add_foe(id, p, hp, FoeProfile::telegraphed());
            foe.observer = true;
            foe.autonomous = false;
        }
        if let Some(f) = res.world.foe_mut(id) {
            f.pos = p;
            f.hp = hp;
        }
    }
    res.world.foes.retain(|f| seen.contains(&f.id));
}

/// First animation sequence of the action that the hero's set has; the first step starts now.
fn pick_steps(
    res: &SoulsRes,
    id: ActionId,
    lib: &mut ActorLibrary,
    anims: &mut Query<(&mut Animator, &CombatStance)>,
    state: &PlayState,
) -> Option<Vec<(String, String)>> {
    let hero = state.hero_entity?;
    let (mut animator, stance) = anims.get_mut(hero).ok()?;
    let family = animator.rig.family.clone();
    let cands = res.anim.get(id);
    let seq = first_playable(cands, &|a, p| {
        find_motion(lib, &family, stance.0, "Stand", a, p).is_some()
    })?
    .clone();
    let mut steps = seq.into_iter();
    let (a, p) = steps.next()?;
    if start_overlay(lib, &mut animator, stance.0, &a, &p) {
        let want = id.def().total / gothic_combat_souls::tuning::ANIM_FPS;
        if let Some(o) = animator.overlay.as_mut()
            && !super::er_pack::is_pack(o)
        {
            let secs = o.motion.clip.duration;
            let _ = want;
            o.rate = gothic_combat_souls::animmap::AnimMap::rate(id, secs);
        }
    }
    Some(steps.collect())
}

/// Plays the remaining steps of the hero's sequence.
fn animate(
    mut glue: ResMut<Glue>,
    state: Res<PlayState>,
    mut lib: ResMut<ActorLibrary>,
    mut anims: Query<(&mut Animator, &CombatStance)>,
) {
    if glue.anim.is_empty() {
        return;
    }
    let Some(hero) = state.hero_entity else {
        return;
    };
    let Ok((mut animator, stance)) = anims.get_mut(hero) else {
        return;
    };
    let done = animator
        .overlay
        .as_ref()
        .is_none_or(|o| o.time >= o.motion.clip.duration);
    if done {
        let (a, p) = glue.anim.remove(0);
        start_overlay(&mut lib, &mut animator, stance.0, &a, &p);
    }
}

/// Starts the raised-weapon animation of an NPC swing the moment its brain commits to it, slowed
/// over the wind-up, so the telegraph is visible (the original plays the swing only at the hit).
fn telegraph_visuals(
    glue: &mut Glue,
    core: &CoreRes,
    roster: &Roster,
    anims: &mut Query<(&mut Animator, &CombatStance)>,
    lib: &mut ActorLibrary,
    now: u64,
) {
    let mut current = HashMap::new();
    for a in &core.0.sim.actors {
        if let Some(st) = a.brain.strike {
            current.insert(a.id, st.at_ms);
            if glue.strikes.contains_key(&a.id) {
                continue;
            }
            let Some(&entity) = roster.by_actor.get(&a.id) else {
                continue;
            };
            let Ok((mut animator, stance)) = anims.get_mut(entity) else {
                continue;
            };
            if start_overlay(lib, &mut animator, stance.0, "Attack", "Raise")
                && let Some(o) = animator.overlay.as_mut()
            {
                let wind = st.at_ms.saturating_sub(now).max(200) as f32 / 1000.0;
                o.rate = (o.motion.clip.duration / wind).clamp(0.25, 1.5);
            }
        }
    }
    glue.strikes = current;
}
