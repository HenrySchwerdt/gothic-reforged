//! Magic and ranged presentation: cast sounds, spell missiles and arrows.
//!
//! PLACEHOLDER visuals: the original plays particle effects from the effect scripts
//! (`gCEffect_PS`, `EffectModule`); here a spell missile is a glowing sphere and an arrow a thin
//! shaft that fly from the shooter to the target in the flight time the simulation computed
//! (`ScriptTables::magic.flights`, so the visible impact and the hit event agree).
use super::{HeroActor, NpcActor, Roster, sim_to_bevy_yaw};
use bevy::prelude::*;
use gothic_audio::PlaySfx;
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::script::host::EntityId;
use gothic_sim::script::magic::api::MagicEvent;
use gothic_ui::{Notify, ToastKind};

const CM: f32 = 0.01;

#[derive(Component)]
pub struct Missile {
    from: Vec3,
    to: Vec3,
    t0: f32,
    dur: f32,
}

/// Counts for the smoke report.
#[derive(Resource, Default)]
pub struct FxLog {
    pub missiles: u32,
    pub arrows: u32,
    pub casts: u32,
}

pub fn register(app: &mut App) {
    app.init_resource::<FxLog>()
        .add_systems(Update, (spawn_missiles, move_missiles).chain());
}

fn actor_pos(core: &CoreRes, a: gothic_sim::types::ActorId) -> Option<Vec3> {
    core.0
        .sim
        .world
        .actors
        .get(&a)
        .map(|(p, _)| Vec3::new(p.x, p.y, p.z))
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

#[allow(clippy::too_many_arguments)]
fn spawn_missiles(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sound: MessageWriter<PlaySfx>,
    mut notify: MessageWriter<Notify>,
    mut log: ResMut<FxLog>,
    _roster: Res<Roster>,
    _q: Query<(), (With<HeroActor>, Without<NpcActor>)>,
) {
    for e in events.read() {
        match &e.0 {
            Event::Projectile {
                owner,
                target,
                template,
                spell,
            } => {
                let Some(from) = actor_pos(&core, *owner) else {
                    continue;
                };
                let yaw = core
                    .0
                    .sim
                    .world
                    .actors
                    .get(owner)
                    .map_or(0.0, |(_, y)| sim_to_bevy_yaw(*y));
                let to = target
                    .and_then(|t| actor_pos(&core, t))
                    .unwrap_or_else(|| from + Quat::from_rotation_y(yaw) * Vec3::NEG_Z * 2500.0);
                let now_ms = core.0.sim.now_ms();
                let dur = core
                    .0
                    .sim
                    .script
                    .magic
                    .flights
                    .iter()
                    .rev()
                    .find(|f| f.arrives_ms >= now_ms)
                    .map_or_else(
                        || ((to - from).length() / 3000.0).max(0.2),
                        |f| (f.arrives_ms - now_ms) as f32 / 1000.0,
                    )
                    .max(0.15);
                let up = Vec3::Y * 130.0;
                let (mesh, mat) = if *spell {
                    log.missiles += 1;
                    (
                        meshes.add(Sphere::new(0.22)),
                        materials.add(StandardMaterial {
                            base_color: Color::srgb(1.0, 0.55, 0.15),
                            emissive: LinearRgba::rgb(9.0, 3.0, 0.4),
                            unlit: true,
                            ..default()
                        }),
                    )
                } else {
                    log.arrows += 1;
                    sound.write(PlaySfx::new("sfx_ani_fight_bow_shoot_01", Some(from * CM)));
                    (
                        meshes.add(Cuboid::new(0.025, 0.025, 0.8)),
                        materials.add(StandardMaterial {
                            base_color: Color::srgb(0.55, 0.4, 0.22),
                            unlit: true,
                            ..default()
                        }),
                    )
                };
                let _ = template;
                let a = (from + up) * CM;
                let b = (to + up) * CM;
                commands.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat),
                    Transform::from_translation(a).looking_at(b, Vec3::Y),
                    Missile {
                        from: a,
                        to: b,
                        t0: time.elapsed_secs(),
                        dur,
                    },
                ));
            }
            Event::Magic(m) => match m {
                MagicEvent::Cast { spell, caster, .. } => {
                    log.casts += 1;
                    let at = actor_pos(
                        &core,
                        gothic_sim::types::ActorId(caster.0.saturating_sub(1)),
                    )
                    .map(|p| p * CM);
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

fn move_missiles(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &Missile, &mut Transform)>,
) {
    for (e, m, mut tf) in &mut q {
        let k = (time.elapsed_secs() - m.t0) / m.dur;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation = m.from.lerp(m.to, k);
    }
}
