//! Magic and ranged presentation: cast sounds, spell effects from the effect map and arrows.
//!
//! Spell visuals are the original effect sequences (`g3.efm`, played by `gothic-fx`): the cast
//! phases start `eff_magic_<spell>_{load,hold,power,cast}_01`, a spell missile carries the
//! `..._projectile_01` effect to its target and ends in `..._impact_01`; effects the script starts
//! itself (`MagicEvent::Effect`) play on the named character. Arrows stay a thin shaft that flies
//! in the time the simulation computed (`ScriptTables::magic.flights`).
use super::{HeroActor, NpcActor, Roster};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_audio::PlaySfx;
use gothic_fx::plugin::{SpawnEffect, StopEffects};
use gothic_fx::spell::{SpellStage, effect_name};
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::script::host::EntityId;
use gothic_sim::script::magic::api::MagicEvent;
use gothic_ui::{Notify, ToastKind};
use std::collections::HashMap;

const CM: f32 = 0.01;
/// Height of the effect anchor above a character's feet (chest/hand), metres.
const ANCHOR_UP: f32 = 1.3;

/// Counts for the smoke report.
#[derive(Resource, Default)]
pub struct FxLog {
    pub missiles: u32,
    pub arrows: u32,
    pub casts: u32,
    pub effects: u32,
}

/// Per caster: the spell of the last `Cast` event (the missile event carries only a template).
#[derive(Resource, Default)]
struct LastCast(HashMap<gothic_sim::types::ActorId, String>);

/// Effects held while a cast phase lasts (load, hold), by caster.
#[derive(Resource, Default)]
struct HeldFx(HashMap<gothic_sim::types::ActorId, u64>);

/// Debug aid for the effect screenshots: with `GR_FX_SHOTS=<prefix>` the first spell missile
/// of a run is followed by screenshots at fixed delays (`<prefix>-<ms>.png`).
#[derive(Resource, Default)]
struct ShotPlan {
    prefix: Option<String>,
    start: Option<f32>,
    done: usize,
}

const SHOT_DELAYS: [f32; 7] = [0.08, 0.2, 0.35, 0.5, 0.7, 1.0, 1.5];

pub fn register(app: &mut App) {
    app.init_resource::<FxLog>()
        .init_resource::<LastCast>()
        .init_resource::<HeldFx>()
        .insert_resource(ShotPlan {
            prefix: std::env::var("GR_FX_SHOTS").ok(),
            ..default()
        })
        .add_systems(
            Update,
            (spawn_missiles, hand_glow_cleanup, fx_shots).chain(),
        );
}

fn fx_shots(time: Res<Time>, log: Res<FxLog>, mut plan: ResMut<ShotPlan>, mut commands: Commands) {
    let Some(prefix) = plan.prefix.clone() else {
        return;
    };
    if plan.start.is_none() && log.missiles > 0 {
        plan.start = Some(time.elapsed_secs());
    }
    let Some(t0) = plan.start else { return };
    if let Some(d) = SHOT_DELAYS.get(plan.done)
        && time.elapsed_secs() - t0 >= *d
    {
        let path = format!("{prefix}-{}.png", (*d * 1000.0) as u32);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        plan.done += 1;
    }
}

fn actor_pos(core: &CoreRes, a: gothic_sim::types::ActorId) -> Option<Vec3> {
    core.0
        .sim
        .world
        .actors
        .get(&a)
        .map(|(p, _)| Vec3::new(p.x, p.y, p.z))
}

fn actor_of(id: EntityId) -> gothic_sim::types::ActorId {
    gothic_sim::types::ActorId(id.0.saturating_sub(1))
}

fn spell_sound(spell: &str) -> String {
    format!(
        "sfx_magic_{}_cast_01",
        spell
            .strip_prefix("Spl_")
            .unwrap_or(spell)
            .to_ascii_lowercase()
    )
}

/// An effect following a character (its chest), or standing where he is.
#[allow(clippy::too_many_arguments)]
fn effect_on(
    out: &mut MessageWriter<SpawnEffect>,
    roster: &Roster,
    core: &CoreRes,
    hand: Option<Entity>,
    actor: gothic_sim::types::ActorId,
    name: String,
    permanent: bool,
    key: Option<u64>,
    log: &mut FxLog,
) {
    log.effects += 1;
    // The hero's cast effects sit at his casting hand (the effects link to the owner entity;
    // here the owner is the rune in the right hand).
    if actor == core.0.hero
        && let Some(h) = hand
    {
        out.write(SpawnEffect {
            name,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            follow: Some(h),
            permanent,
            key,
        });
        return;
    }
    if let Some(e) = roster.by_actor.get(&actor) {
        out.write(SpawnEffect {
            name,
            position: Vec3::Y * ANCHOR_UP,
            rotation: Quat::IDENTITY,
            follow: Some(*e),
            permanent,
            key,
        });
    } else if let Some(p) = actor_pos(core, actor) {
        out.write(SpawnEffect {
            name,
            position: p * CM + Vec3::Y * ANCHOR_UP,
            rotation: Quat::IDENTITY,
            follow: None,
            permanent,
            key,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_missiles(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    mut sound: MessageWriter<PlaySfx>,
    mut notify: MessageWriter<Notify>,
    mut fx: MessageWriter<SpawnEffect>,
    mut stop: MessageWriter<StopEffects>,
    mut log: ResMut<FxLog>,
    mut last: ResMut<LastCast>,
    mut held: ResMut<HeldFx>,
    roster: Res<Roster>,
    hand: Res<super::ranged::HandAnchor>,
    _q: Query<(), (With<HeroActor>, Without<NpcActor>)>,
) {
    for e in events.read() {
        match &e.0 {
            Event::Projectile {
                owner,
                template,
                spell,
                ..
            } => {
                // The flight itself is drawn from the simulation's shots (`shots.rs`); here
                // only the launch: counters, the bow's sounds and the shoot effect.
                let from = actor_pos(&core, *owner);
                if *spell {
                    log.missiles += 1;
                    if std::env::var_os("GR_FX_DEBUG").is_some() {
                        eprintln!("fx: spell missile template {template}");
                    }
                } else {
                    log.arrows += 1;
                    sound.write(PlaySfx::new(
                        "sfx_ani_fight_bow_shoot_01",
                        from.map(|p| p * CM),
                    ));
                    effect_on(
                        &mut fx,
                        &roster,
                        &core,
                        hand.0,
                        *owner,
                        gothic_projectile::aim::SHOOT_EFFECT.to_owned(),
                        false,
                        None,
                        &mut log,
                    );
                    // The string-raise glow ends with the shot.
                    stop.write(StopEffects {
                        key: 0x3_0000_0000 | owner.0 as u64,
                    });
                }
            }
            // A parried blow is steel on steel: sparks at the defender (`eff_col_weaponhitsweapon`).
            Event::Sim(gothic_sim::sim::EventKind::Hit {
                victim,
                parried: true,
                ..
            }) => {
                effect_on(
                    &mut fx,
                    &roster,
                    &core,
                    hand.0,
                    *victim,
                    "eff_col_weaponhitsweapon_metal_metal_01".to_owned(),
                    false,
                    None,
                    &mut log,
                );
            }
            Event::Magic(m) => match m {
                MagicEvent::Phase {
                    caster,
                    spell,
                    phase,
                } => {
                    let actor = actor_of(*caster);
                    // A new phase ends the held effect of the previous one.
                    if let Some(k) = held.0.remove(&actor) {
                        stop.write(StopEffects { key: k });
                    }
                    let Some(stage) = SpellStage::of_phase(*phase) else {
                        continue;
                    };
                    let key = 0x2_0000_0000 | actor.0 as u64;
                    let sustained = matches!(stage, SpellStage::Load | SpellStage::Hold);
                    let mut stages = vec![stage];
                    if stage == SpellStage::Load {
                        stages.push(SpellStage::Hand);
                    }
                    for s in stages {
                        effect_on(
                            &mut fx,
                            &roster,
                            &core,
                            hand.0,
                            actor,
                            effect_name(spell, s),
                            sustained,
                            sustained.then_some(key),
                            &mut log,
                        );
                    }
                    if sustained {
                        held.0.insert(actor, key);
                    }
                }
                MagicEvent::Effect { name, on } => {
                    effect_on(
                        &mut fx,
                        &roster,
                        &core,
                        hand.0,
                        actor_of(*on),
                        name.clone(),
                        false,
                        None,
                        &mut log,
                    );
                }
                MagicEvent::Cast { spell, caster, .. } => {
                    log.casts += 1;
                    let actor = actor_of(*caster);
                    last.0.insert(actor, spell.clone());
                    let at = actor_pos(&core, actor).map(|p| p * CM);
                    sound.write(PlaySfx::new(spell_sound(spell), at));
                }
                MagicEvent::Message { key, subject } => {
                    let _ = EntityId(subject.0);
                    notify.write(Notify {
                        text: (*key).to_string(),
                        kind: ToastKind::Info,
                    });
                }
                MagicEvent::Summoned { template, .. } => {
                    notify.write(Notify {
                        text: format!("Beschworen: {template}"),
                        kind: ToastKind::Info,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// The glow of the spell in the hand (load / hold effects, keyed by the caster) ends when the
/// hero no longer holds a spell: another weapon was drawn or the weapons were put away.
fn hand_glow_cleanup(
    core: Res<CoreRes>,
    play: Res<super::PlayState>,
    mut held: ResMut<HeldFx>,
    mut stop: MessageWriter<StopEffects>,
) {
    let hero = core.0.hero;
    let magic = play.drawn
        && core.0.sim.actor(hero).equipment.weapon_mode() == gothic_sim::item::WeaponMode::Magic;
    if !magic && let Some(k) = held.0.remove(&hero) {
        stop.write(StopEffects { key: k });
    }
}
