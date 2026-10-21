//! Opt-in modern rendering and audio on top of the faithful look (#51). Nothing changes unless
//! asked for:
//!
//! * `--render-plus <faithful|low|medium|high|profile.json>` (or `GR_RENDER_PLUS`): the
//!   `gothic-render-plus` profile on the player camera and the sun, with its frame-time
//!   budget. SSR is switched off here: the game's surfaces and water use their own forward
//!   materials, which Bevy's SSR cannot see (see `docs/RENDER_PLUS.md`).
//! * `--audio-plus` (or `GR_AUDIO_PLUS`, play mode only): `gothic-audio`'s positional sounds
//!   play through `gothic-audio-plus` (HRTF, occlusion against the world collision, reverb).
//!   The reverb follows a ceiling probe above the listener (indoor / outdoor), since the level
//!   data has no reverb zones yet.
use super::{CM, Collision, PlayerCamera};
use bevy::audio::SpatialListener;
use bevy::prelude::*;
use gothic_audio_plus::bridge::GothicAudioBridgePlugin;
use gothic_audio_plus::plugin::{
    EmitterBlocking, EnhancedAudio, EnhancedAudioPlugin, EnhancedEmitter,
};
use gothic_audio_plus::{EnhancedSettings, probe};
use gothic_render_plus::plugin::{RenderPlusCamera, RenderPlusPlugin, RenderPlusSun};
use gothic_render_plus::{Effect, Preset, RenderProfile};

/// Occlusion is re-probed for each emitter every this many frames (staggered by entity).
const PROBE_EVERY: u32 = 6;
/// Listener ceiling below this (cm) counts as indoors for the reverb.
const INDOOR_CEILING_CM: f32 = 900.0;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .cloned()
}

/// The render profile asked for, if any (`Err` names a bad value).
pub(crate) fn render_profile(spec: &str) -> Result<RenderProfile, String> {
    let preset = match spec.to_ascii_lowercase().as_str() {
        "faithful" | "off" => Some(Preset::Faithful),
        "low" => Some(Preset::Low),
        "medium" | "" | "on" | "1" => Some(Preset::Medium),
        "high" => Some(Preset::High),
        _ => None,
    };
    let mut profile = match preset {
        Some(p) => RenderProfile::preset(p),
        None => {
            let text = std::fs::read_to_string(spec)
                .map_err(|e| format!("render-plus profile {spec:?}: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("render-plus profile {spec:?}: {e}"))?
        }
    };
    if profile.is_on(Effect::Ssr) {
        profile.set(Effect::Ssr, false);
        warn!("render-plus: SSR is off in the game (forward materials; see docs/RENDER_PLUS.md)");
    }
    Ok(profile)
}

pub(crate) fn install(app: &mut App, args: &[String], play: bool) -> anyhow::Result<()> {
    let render = flag(args, "--render-plus")
        .or_else(|| args.iter().any(|a| a == "--render-plus").then(String::new))
        .or_else(|| std::env::var("GR_RENDER_PLUS").ok());
    if let Some(spec) = render {
        let profile = render_profile(&spec).map_err(anyhow::Error::msg)?;
        let on: Vec<_> = Effect::ALL
            .iter()
            .filter(|&&e| profile.is_on(e))
            .map(|e| e.name())
            .collect();
        eprintln!(
            "render-plus: {} (budget {})",
            if on.is_empty() {
                "faithful".to_string()
            } else {
                on.join(", ")
            },
            if profile.budget.enabled {
                format!("{:.1} ms", profile.budget.target_ms)
            } else {
                "off".into()
            }
        );
        app.add_plugins(RenderPlusPlugin)
            .insert_resource(profile)
            .add_systems(Update, mark_render_targets);
    }
    let audio =
        args.iter().any(|a| a == "--audio-plus") || std::env::var_os("GR_AUDIO_PLUS").is_some();
    if audio {
        if !play {
            warn!("audio-plus: only with --play (the walk mode has no game audio)");
        } else {
            eprintln!("audio-plus: HRTF, occlusion against the world collision, reverb");
            app.add_plugins((EnhancedAudioPlugin, GothicAudioBridgePlugin))
                .insert_resource(EnhancedAudio {
                    settings: EnhancedSettings::all_on(),
                    ..default()
                })
                .add_systems(PostUpdate, (occlusion, room_reverb));
        }
    }
    Ok(())
}

/// The player camera and the shadowed sun take the profile.
fn mark_render_targets(
    mut commands: Commands,
    cams: Query<Entity, (With<PlayerCamera>, Without<RenderPlusCamera>)>,
    suns: Query<(Entity, &DirectionalLight), Without<RenderPlusSun>>,
    marked_sun: Query<(), With<RenderPlusSun>>,
) {
    for e in &cams {
        commands.entity(e).insert(RenderPlusCamera::default());
    }
    if marked_sun.is_empty()
        && let Some((e, _)) = suns.iter().find(|(_, l)| l.shadow_maps_enabled)
    {
        commands.entity(e).insert(RenderPlusSun::default());
    }
}

/// Is the segment between two world points (metres) blocked by the level collision?
fn blocked(collision: &Collision, a: Vec3, b: Vec3) -> bool {
    let (a, b) = (a / CM, b / CM);
    let d = b - a;
    let len = d.length();
    // The emitter's own surface (a wall it is mounted on) must not count.
    len > 1.0 && collision.0.raycast(a, d / len, len - 25.0).is_some()
}

fn occlusion(
    mut frame: Local<u32>,
    audio: Res<EnhancedAudio>,
    collision: Res<Collision>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut emitters: Query<
        (Entity, &GlobalTransform, Option<&mut EmitterBlocking>),
        With<EnhancedEmitter>,
    >,
    mut commands: Commands,
) {
    *frame = frame.wrapping_add(1);
    let Some(ear) = listener.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    for (e, tf, current) in &mut emitters {
        let due = (e.index_u32().wrapping_add(*frame)).is_multiple_of(PROBE_EVERY);
        if current.is_some() && !due {
            continue;
        }
        let b = probe(
            ear.to_array(),
            tf.translation().to_array(),
            audio.probe_spread,
            &|a, b| blocked(&collision, Vec3::from_array(a), Vec3::from_array(b)),
        );
        match current {
            Some(mut c) => c.0 = b,
            None => {
                commands.entity(e).insert(EmitterBlocking(b));
            }
        }
    }
}

/// Indoor / outdoor reverb from a ray straight up from the listener (four times a second).
fn room_reverb(
    time: Res<Time>,
    mut next: Local<f32>,
    collision: Res<Collision>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut audio: ResMut<EnhancedAudio>,
) {
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 0.25;
    let Some(ear) = listener.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let preset = room_preset(collision.0.raycast(ear / CM, Vec3::Y, INDOOR_CEILING_CM));
    let params = preset.params();
    if audio.outdoor != params {
        audio.outdoor = params;
    }
}

/// Reverb preset for the distance (cm) to the ceiling above the listener, if any.
pub(crate) fn room_preset(ceiling_cm: Option<f32>) -> gothic_audio_plus::Preset {
    match ceiling_cm {
        Some(d) if d < 450.0 => gothic_audio_plus::Preset::Indoor,
        Some(_) => gothic_audio_plus::Preset::Hall,
        None => gothic_audio_plus::Preset::Outdoor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_from_presets_and_ssr_stays_off() {
        assert!(render_profile("faithful").unwrap().is_faithful());
        let medium = render_profile("").unwrap();
        assert!(medium.is_on(Effect::Ssao) && medium.budget.enabled);
        let high = render_profile("HIGH").unwrap();
        assert!(high.is_on(Effect::Volumetric) && !high.is_on(Effect::Ssr));
        assert!(render_profile("/nonexistent/profile.json").is_err());
    }

    #[test]
    fn profile_from_json() {
        let dir = std::env::temp_dir().join(format!("gr-plus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.json");
        std::fs::write(&path, r#"{"taa":{"enabled":true},"ssr":{"enabled":true}}"#).unwrap();
        let p = render_profile(path.to_str().unwrap()).unwrap();
        assert!(p.is_on(Effect::Taa) && !p.is_on(Effect::Ssr) && !p.is_on(Effect::Ssao));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ceiling_picks_the_room() {
        use gothic_audio_plus::Preset as R;
        assert_eq!(room_preset(None), R::Outdoor);
        assert_eq!(room_preset(Some(280.0)), R::Indoor);
        assert_eq!(room_preset(Some(700.0)), R::Hall);
    }

    #[test]
    fn blocking_uses_the_level_collision_in_centimetres() {
        let mut world = gothic_physics::CollisionWorld::new();
        // A wall at x = 500 cm (5 m), 10 m wide and high.
        let v = |y: f32, z: f32| Vec3::new(500.0, y, z);
        world.insert_chunk(
            1,
            vec![
                [v(-500.0, -500.0), v(500.0, -500.0), v(500.0, 500.0)],
                [v(-500.0, -500.0), v(500.0, 500.0), v(-500.0, 500.0)],
            ],
        );
        let c = Collision(world);
        assert!(blocked(&c, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)));
        assert!(!blocked(&c, Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)));
        // A source on the wall itself is not behind it.
        assert!(!blocked(&c, Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0)));
        let b = probe([0.0; 3], [10.0, 0.0, 0.0], 0.6, &|a, b| {
            blocked(&c, Vec3::from_array(a), Vec3::from_array(b))
        });
        assert!(b.obstruction > 0.99 && b.occlusion > 0.5, "{b:?}");
    }
}
