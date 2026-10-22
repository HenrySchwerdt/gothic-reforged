//! Projectiles in flight: arrows, bolts and spell missiles as the simulation flies them
//! (`Sim::shot_views`, port of `gCProjectile_PS`, engine port E51).
//!
//! Nothing is faked here: every frame the shots of the simulation are drawn (new id: spawn the
//! model of the ammunition or the effect of the spell; id gone: despawn and stop the effect),
//! positions are interpolated between the 50 ms simulation steps. Impacts
//! (`EventKind::ProjectileImpact`) start the impact and target effects of the spell and the hit
//! sounds.
use super::objects::WorldObjects;
use bevy::prelude::*;
use gothic_animation::ActorSpawner;
use gothic_audio::PlaySfx;
use gothic_fx::plugin::{SpawnEffect, StopEffects};
use gothic_fx::spell::{SpellStage, effect_name};
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::sim::EventKind;
use std::collections::HashMap;

const CM: f32 = 0.01;
/// Duration of a simulation step in seconds (interpolation of the drawn position): the core
/// steps with `tick_ms = 1000 / TICK_HZ` truncated (`core.rs`), not the 50 ms of `SimConfig`.
const TICK: f32 = ((1000.0 / gothic_runtime::TICK_HZ as f32) as u32) as f32 / 1000.0;
/// Chest height above the feet for a target effect (cm).
const CHEST: f32 = 130.0;

/// Presentation of one shot.
struct Drawn {
    entity: Entity,
    last: Vec3,
    changed_at: f32,
    effect_key: Option<u64>,
}

/// Shots on screen by simulation id.
#[derive(Resource, Default)]
pub struct ShotsOnScreen {
    drawn: HashMap<u64, Drawn>,
    /// Spell name per shot id, remembered for the impact.
    spell_of: HashMap<u64, String>,
    /// Counters for the QA report.
    pub spawned: u32,
    pub spell_spawned: u32,
    pub arrows_spawned: u32,
    pub impacts: u32,
    pub impacts_on_actor: u32,
    pub effects_started: u32,
}

impl ShotsOnScreen {
    /// World positions (cm) of the shots drawn now, with their entity.
    pub fn live(&self) -> Vec<(u64, Entity, Vec3)> {
        self.drawn
            .iter()
            .map(|(id, d)| (*id, d.entity, d.last))
            .collect()
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<ShotsOnScreen>()
        .add_systems(Update, (draw_shots, impacts).chain());
}

/// Turn the model's forward axis (the arrow points along +Z in the item meshes [I]) toward the
/// flight direction.
fn orient(dir: Vec3) -> Quat {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(Vec3::NEG_Z, d)
}

#[allow(clippy::too_many_arguments)]
fn draw_shots(
    time: Res<Time>,
    core: Res<CoreRes>,
    objects: Res<WorldObjects>,
    mut spawner: ActorSpawner,
    mut screen: ResMut<ShotsOnScreen>,
    mut fx: MessageWriter<SpawnEffect>,
    mut stop: MessageWriter<StopEffects>,
    mut sound: MessageWriter<PlaySfx>,
    mut tf: Query<&mut Transform>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let views = core.0.sim.shot_views();
    let mut seen: Vec<u64> = Vec::with_capacity(views.len());
    for v in &views {
        seen.push(v.id);
        let pos_m = v.position * CM;
        if !screen.drawn.contains_key(&v.id) {
            // Spell name of the shot (the view carries only the template).
            let spell = core
                .0
                .sim
                .script
                .magic
                .flights
                .iter()
                .find(|f| f.shot.id == v.id)
                .map(|f| f.shot.spec.spell.clone())
                .unwrap_or_default();
            let rot = orient(v.direction);
            let mut key = None;
            let entity = if v.spell {
                screen.spell_spawned += 1;
                let e = commands
                    .spawn((
                        Transform::from_translation(pos_m).with_rotation(rot),
                        Visibility::default(),
                    ))
                    .id();
                let k = 0x4_0000_0000 | v.id;
                fx.write(SpawnEffect {
                    name: effect_name(&spell, SpellStage::Projectile),
                    position: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    follow: Some(e),
                    permanent: true,
                    key: Some(k),
                });
                screen.effects_started += 1;
                key = Some(k);
                screen.spell_of.insert(v.id, spell);
                e
            } else {
                screen.arrows_spawned += 1;
                let mesh = objects.item_mesh.get(&v.template.to_ascii_lowercase());
                let root = mesh.and_then(|m| {
                    spawner
                        .spawn_static(
                            m,
                            Transform::from_translation(pos_m)
                                .with_rotation(rot)
                                .with_scale(Vec3::splat(CM)),
                        )
                        .ok()
                });
                match root {
                    Some(e) => e,
                    None => {
                        // Without a model the arrow is a thin shaft.
                        commands
                            .spawn((
                                Transform::from_translation(pos_m).with_rotation(rot),
                                Visibility::default(),
                            ))
                            .id()
                    }
                }
            };
            screen.spawned += 1;
            if std::env::var_os("GR_RANGED_DEBUG").is_some() {
                eprintln!(
                    "shots: t={now:.2} spawn id {} {} spell={:?} at {:?} dir {:?}",
                    v.id,
                    v.template,
                    screen.spell_of.get(&v.id),
                    v.position,
                    v.direction
                );
            }
            sound.write(PlaySfx::new(
                if v.spell {
                    "sfx_fight_arrowwhoosh_01"
                } else {
                    "sfx_fight_arrowwhoosh_02"
                },
                Some(pos_m),
            ));
            screen.drawn.insert(
                v.id,
                Drawn {
                    entity,
                    last: v.position,
                    changed_at: now,
                    effect_key: key,
                },
            );
        }
        let Some(d) = screen.drawn.get_mut(&v.id) else {
            continue;
        };
        if d.last != v.position {
            if std::env::var_os("GR_RANGED_DEBUG").is_some()
                && (now * 5.0) as u32 != (d.changed_at * 5.0) as u32
            {
                let kmh = (v.position - d.last).length() / TICK * 0.01 * 3.6;
                eprintln!(
                    "shots: t={now:.2} id {} pos {:?} step {:.0} km/h flying {}",
                    v.id, v.position, kmh, v.flying
                );
            }
            d.last = v.position;
            d.changed_at = now;
        }
        let k = ((now - d.changed_at) / TICK).clamp(0.0, 1.0);
        let p = if v.flying {
            v.previous.lerp(v.position, k)
        } else {
            v.position
        };
        if let Ok(mut t) = tf.get_mut(d.entity) {
            t.translation = p * CM;
            t.rotation = orient(v.direction);
        }
    }
    // Shots that left the simulation.
    let gone: Vec<u64> = screen
        .drawn
        .keys()
        .filter(|id| !seen.contains(id))
        .copied()
        .collect();
    for id in gone {
        if std::env::var_os("GR_RANGED_DEBUG").is_some() {
            eprintln!("shots: t={now:.2} shot {id} gone");
        }
        if let Some(d) = screen.drawn.remove(&id) {
            if let Some(k) = d.effect_key {
                stop.write(StopEffects { key: k });
            }
            commands.entity(d.entity).despawn();
        }
        screen.spell_of.remove(&id);
    }
}

/// Impact effects and sounds.
#[allow(clippy::too_many_arguments)]
fn impacts(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    mut screen: ResMut<ShotsOnScreen>,
    mut fx: MessageWriter<SpawnEffect>,
    mut sound: MessageWriter<PlaySfx>,
) {
    for e in events.read() {
        let Event::Sim(EventKind::ProjectileImpact {
            shot,
            at,
            on_actor,
            spell,
        }) = &e.0
        else {
            continue;
        };
        screen.impacts += 1;
        if std::env::var_os("GR_RANGED_DEBUG").is_some() {
            eprintln!("shots: impact shot {shot} at {at:?} on {on_actor:?} spell {spell}");
        }
        let at_m = Vec3::from_array(*at) * CM;
        if on_actor.is_some() {
            screen.impacts_on_actor += 1;
        }
        if *spell {
            let name = screen.spell_of.get(shot).cloned().unwrap_or_default();
            fx.write(SpawnEffect {
                name: effect_name(&name, SpellStage::Impact),
                position: at_m,
                rotation: Quat::IDENTITY,
                follow: None,
                permanent: false,
                key: None,
            });
            screen.effects_started += 1;
            if let Some(a) = on_actor
                && let Some((p, _)) = core.0.sim.world.actors.get(a)
            {
                fx.write(SpawnEffect {
                    name: effect_name(&name, SpellStage::Target),
                    position: Vec3::new(p.x, p.y + CHEST, p.z) * CM,
                    rotation: Quat::IDENTITY,
                    follow: None,
                    permanent: false,
                    key: None,
                });
                screen.effects_started += 1;
            }
            let stem = name
                .strip_prefix("Spl_")
                .unwrap_or(&name)
                .to_ascii_lowercase();
            sound.write(PlaySfx::new(
                format!("sfx_magic_{stem}_impact_01"),
                Some(at_m),
            ));
        } else if on_actor.is_none() {
            // An arrow in the level: the material sample (`eff_col_arrow_<material>`); the
            // material of the ray is not known yet, earth is the commonest [I].
            sound.write(PlaySfx::new("eff_col_arrow_earth_01", Some(at_m)));
        }
    }
}
