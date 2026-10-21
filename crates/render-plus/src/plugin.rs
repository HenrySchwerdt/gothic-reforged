//! Bevy side: applies the current [`Plan`] to every camera marked [`RenderPlusCamera`] and the
//! directional light marked [`RenderPlusSun`], runs the budget [`Governor`] on the real frame
//! time, and restores what the camera had before when an effect goes off again.
use crate::budget::{Change, Governor};
use crate::plan::Plan;
use crate::profile::{RenderProfile, SsaoQuality};
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::prepass::{
    DeferredPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass,
};
use bevy::light::{FogVolume, ShadowFilteringMethod, VolumetricFog, VolumetricLight};
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::{
    ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
    ScreenSpaceReflections,
};
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};

/// Put this on a 3D camera to let the profile drive it. What the camera had before (MSAA,
/// prepasses, shadow filtering) is recorded on first sight and restored when effects go off.
#[derive(Component, Default, Debug, Clone)]
pub struct RenderPlusCamera {
    original: Option<CameraOriginal>,
}

#[derive(Debug, Clone)]
struct CameraOriginal {
    msaa: Msaa,
    depth: bool,
    normal: bool,
    motion: bool,
    deferred: bool,
    shadow_filter: Option<ShadowFilteringMethod>,
}

/// Put this on the sun (a `DirectionalLight` with shadows) for light shafts, soft and contact
/// shadows.
#[derive(Component, Default, Debug, Clone)]
pub struct RenderPlusSun {
    original_contact: Option<bool>,
}

/// Put this on water and other mirror-like meshes (with a `StandardMaterial`): while SSR is
/// on, their material renders deferred so the reflections can see them; otherwise it stays
/// forward, exactly as before. (Bevy 0.20's SSR only works on deferred surfaces, and a
/// deferred material is invisible to a camera without a deferred prepass.)
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct RenderPlusReflective;

/// The fog volume the plugin keeps centred on the first render-plus camera.
#[derive(Component)]
pub struct RenderPlusFog;

/// What is applied right now (for overlays and the demo).
#[derive(Resource, Default, Debug, Clone)]
pub struct RenderPlusState {
    pub governor: Governor,
    pub plan: Plan,
    /// Last budget change and the frame it happened on.
    pub last_change: Option<(Change, u64)>,
    frame: u64,
    dirty: bool,
}

pub struct RenderPlusPlugin;

impl Plugin for RenderPlusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderProfile>()
            .init_resource::<RenderPlusState>()
            .add_systems(
                PostUpdate,
                (
                    govern,
                    apply_cameras,
                    apply_sun,
                    apply_reflective,
                    follow_fog,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

fn govern(time: Res<Time<Real>>, profile: Res<RenderProfile>, mut state: ResMut<RenderPlusState>) {
    state.frame += 1;
    let ms = time.delta_secs() * 1000.0;
    let s = &mut *state;
    if let Some(change) = s.governor.update(&profile, ms) {
        info!(
            "render-plus budget: {change:?} at {:.1} ms",
            s.governor.smoothed
        );
        s.last_change = Some((change, s.frame));
    }
    let plan = Plan::new(&profile, &s.governor.suppressed());
    if plan != s.plan {
        s.plan = plan;
        s.dirty = true;
    }
}

fn quality(q: SsaoQuality) -> ScreenSpaceAmbientOcclusionQualityLevel {
    match q {
        SsaoQuality::Low => ScreenSpaceAmbientOcclusionQualityLevel::Low,
        SsaoQuality::Medium => ScreenSpaceAmbientOcclusionQualityLevel::Medium,
        SsaoQuality::High => ScreenSpaceAmbientOcclusionQualityLevel::High,
        SsaoQuality::Ultra => ScreenSpaceAmbientOcclusionQualityLevel::Ultra,
    }
}

/// Insert `c` when `on`, else remove `T`.
fn toggle<T: Component>(e: &mut EntityCommands, on: bool, c: impl FnOnce() -> T) {
    if on {
        e.insert(c());
    } else {
        e.remove::<T>();
    }
}

#[allow(clippy::type_complexity)]
fn apply_cameras(
    mut commands: Commands,
    mut state: ResMut<RenderPlusState>,
    mut cams: Query<(
        Entity,
        &mut RenderPlusCamera,
        Option<&Msaa>,
        Has<DepthPrepass>,
        Has<NormalPrepass>,
        Has<MotionVectorPrepass>,
        Has<DeferredPrepass>,
        Option<&ShadowFilteringMethod>,
    )>,
) {
    if !state.dirty && !cams.iter_mut().any(|(_, cam, ..)| cam.is_added()) {
        return;
    }
    let plan = state.plan.clone();
    for (entity, mut cam, msaa, depth, normal, motion, deferred, filter) in &mut cams {
        let o = cam
            .original
            .get_or_insert_with(|| CameraOriginal {
                msaa: msaa.copied().unwrap_or_default(),
                depth,
                normal,
                motion,
                deferred,
                shadow_filter: filter.copied(),
            })
            .clone();
        let mut e = commands.entity(entity);
        toggle(&mut e, plan.ssao.is_some(), || {
            let s = plan.ssao.unwrap_or_default();
            ScreenSpaceAmbientOcclusion {
                quality_level: quality(s.quality),
                radius: s.radius,
                ..default()
            }
        });
        if plan.taa {
            e.insert(TemporalAntiAliasing::default());
        } else {
            e.remove::<(TemporalAntiAliasing, TemporalJitter, MipBias)>();
        }
        toggle(&mut e, plan.sharpening.is_some(), || {
            ContrastAdaptiveSharpening {
                enabled: true,
                sharpening_strength: plan.sharpening.unwrap_or(0.0),
                denoise: false,
            }
        });
        toggle(&mut e, plan.volumetric.is_some(), || {
            let v = plan.volumetric.unwrap_or_default();
            VolumetricFog {
                ambient_intensity: v.ambient,
                jitter: v.jitter,
                step_count: v.steps,
                ..default()
            }
        });
        toggle(&mut e, plan.contact_shadows.is_some(), || ContactShadows {
            length: plan.contact_shadows.unwrap_or(0.3),
            ..default()
        });
        toggle(&mut e, plan.ssr.is_some(), || {
            let s = plan.ssr.unwrap_or_default();
            ScreenSpaceReflections {
                linear_steps: s.steps,
                min_perceptual_roughness: 0.0..0.05,
                max_perceptual_roughness: (s.max_roughness - 0.05).max(0.0)..s.max_roughness,
                ..default()
            }
        });
        if plan.temporal_shadow_filter {
            e.insert(ShadowFilteringMethod::Temporal);
        } else {
            match o.shadow_filter {
                Some(f) => e.insert(f),
                None => e.remove::<ShadowFilteringMethod>(),
            };
        }
        toggle(&mut e, plan.depth_prepass || o.depth, || DepthPrepass);
        toggle(&mut e, plan.normal_prepass || o.normal, || NormalPrepass);
        toggle(&mut e, plan.motion_prepass || o.motion, || {
            MotionVectorPrepass
        });
        toggle(&mut e, plan.deferred || o.deferred, || DeferredPrepass);
        e.insert(if plan.msaa_off { Msaa::Off } else { o.msaa });
    }
    state.dirty = false;
}

fn apply_sun(
    mut commands: Commands,
    state: Res<RenderPlusState>,
    mut suns: Query<(
        Entity,
        &mut DirectionalLight,
        &mut RenderPlusSun,
        Has<VolumetricLight>,
    )>,
) {
    let plan = &state.plan;
    for (entity, mut light, mut sun, shafts) in &mut suns {
        let original = *sun
            .original_contact
            .get_or_insert(light.contact_shadows_enabled);
        let contact = original || plan.contact_shadows.is_some();
        if light.contact_shadows_enabled != contact {
            light.contact_shadows_enabled = contact;
        }
        #[cfg(feature = "pcss")]
        if light.soft_shadow_size != plan.sun_soft_size {
            light.soft_shadow_size = plan.sun_soft_size;
        }
        if shafts != plan.volumetric.is_some() {
            toggle(&mut commands.entity(entity), !shafts, || VolumetricLight);
        }
    }
}

fn apply_reflective(
    state: Res<RenderPlusState>,
    surfaces: Query<&MeshMaterial3d<StandardMaterial>, With<RenderPlusReflective>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    let want = if state.plan.ssr.is_some() {
        OpaqueRendererMethod::Deferred
    } else {
        OpaqueRendererMethod::Forward
    };
    for handle in &surfaces {
        // Read first: `get_mut` would mark the material changed (and re-specialize it) every
        // frame.
        if materials
            .get(&handle.0)
            .is_some_and(|m| m.opaque_render_method != want)
            && let Some(mut m) = materials.get_mut(&handle.0)
        {
            m.opaque_render_method = want;
        }
    }
}

fn follow_fog(
    mut commands: Commands,
    state: Res<RenderPlusState>,
    cams: Query<&GlobalTransform, With<RenderPlusCamera>>,
    mut fog: Query<(Entity, &mut Transform, &mut FogVolume), With<RenderPlusFog>>,
) {
    let want = state.plan.volumetric.filter(|v| v.density > 0.0);
    let center = cams.iter().next().map(GlobalTransform::translation);
    match (want, center, fog.single_mut()) {
        (Some(v), Some(c), Ok((_, mut tf, mut volume))) => {
            let [w, h] = v.extent;
            tf.translation = Vec3::new(c.x, c.y + h * 0.25, c.z);
            tf.scale = Vec3::new(w, h, w);
            if volume.density_factor != v.density {
                volume.density_factor = v.density;
            }
        }
        (Some(v), Some(c), Err(_)) => {
            let [w, h] = v.extent;
            commands.spawn((
                RenderPlusFog,
                FogVolume {
                    density_factor: v.density,
                    ..default()
                },
                Transform::from_translation(Vec3::new(c.x, c.y + h * 0.25, c.z))
                    .with_scale(Vec3::new(w, h, w)),
            ));
        }
        (None, _, Ok((entity, ..))) => {
            commands.entity(entity).despawn();
        }
        _ => {}
    }
}
