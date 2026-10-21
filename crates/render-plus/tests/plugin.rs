//! The plugin on a headless app (no GPU): the components it puts on cameras and the sun, and
//! that switching back restores exactly what was there.
#![cfg(feature = "render")]
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::prepass::{
    DeferredPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass,
};
use bevy::light::{FogVolume, ShadowFilteringMethod, VolumetricFog, VolumetricLight};
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceReflections};
use bevy::prelude::*;
use bevy::render::camera::TemporalJitter;
use gothic_render_plus::plugin::{
    RenderPlusCamera, RenderPlusFog, RenderPlusPlugin, RenderPlusReflective, RenderPlusSun,
};
use gothic_render_plus::{Effect, Preset, RenderProfile};

fn app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, RenderPlusPlugin));
    let cam = app
        .world_mut()
        .spawn((
            RenderPlusCamera::default(),
            Msaa::Sample4,
            Transform::default(),
        ))
        .id();
    let sun = app
        .world_mut()
        .spawn((DirectionalLight::default(), RenderPlusSun::default()))
        .id();
    app.update();
    (app, cam, sun)
}

fn set(app: &mut App, profile: RenderProfile) {
    *app.world_mut().resource_mut::<RenderProfile>() = profile;
    app.update();
    app.update();
}

#[test]
fn faithful_default_touches_nothing() {
    let (app, cam, sun) = app();
    let e = app.world().entity(cam);
    assert_eq!(*e.get::<Msaa>().unwrap(), Msaa::Sample4);
    assert!(!e.contains::<DepthPrepass>() && !e.contains::<ScreenSpaceAmbientOcclusion>());
    assert!(!e.contains::<TemporalAntiAliasing>() && !e.contains::<ShadowFilteringMethod>());
    let s = app.world().entity(sun);
    assert!(!s.contains::<VolumetricLight>());
    assert!(!s.get::<DirectionalLight>().unwrap().contact_shadows_enabled);
}

#[test]
fn high_applies_everything_and_faithful_restores() {
    let (mut app, cam, sun) = app();
    let mut high = RenderProfile::preset(Preset::High);
    high.budget.enabled = false;
    set(&mut app, high);
    let e = app.world().entity(cam);
    assert!(e.contains::<ScreenSpaceAmbientOcclusion>());
    assert!(e.contains::<TemporalAntiAliasing>() && e.contains::<ContrastAdaptiveSharpening>());
    assert!(e.contains::<VolumetricFog>() && e.contains::<ContactShadows>());
    assert!(e.contains::<ScreenSpaceReflections>() && e.contains::<DeferredPrepass>());
    assert!(e.contains::<DepthPrepass>() && e.contains::<NormalPrepass>());
    assert!(e.contains::<MotionVectorPrepass>());
    assert_eq!(*e.get::<Msaa>().unwrap(), Msaa::Off);
    assert_eq!(
        *e.get::<ShadowFilteringMethod>().unwrap(),
        ShadowFilteringMethod::Temporal
    );
    let s = app.world().entity(sun);
    assert!(s.contains::<VolumetricLight>());
    assert!(s.get::<DirectionalLight>().unwrap().contact_shadows_enabled);
    let mut fog = app
        .world_mut()
        .query_filtered::<&FogVolume, With<RenderPlusFog>>();
    assert_eq!(fog.iter(app.world()).count(), 1);

    set(&mut app, RenderProfile::default());
    let e = app.world().entity(cam);
    for (name, has) in [
        ("ssao", e.contains::<ScreenSpaceAmbientOcclusion>()),
        ("taa", e.contains::<TemporalAntiAliasing>()),
        ("jitter", e.contains::<TemporalJitter>()),
        ("cas", e.contains::<ContrastAdaptiveSharpening>()),
        ("fog", e.contains::<VolumetricFog>()),
        ("contact", e.contains::<ContactShadows>()),
        ("ssr", e.contains::<ScreenSpaceReflections>()),
        ("deferred", e.contains::<DeferredPrepass>()),
        ("depth", e.contains::<DepthPrepass>()),
        ("normal", e.contains::<NormalPrepass>()),
        ("motion", e.contains::<MotionVectorPrepass>()),
        ("filter", e.contains::<ShadowFilteringMethod>()),
    ] {
        assert!(!has, "{name} left on the camera");
    }
    assert_eq!(*e.get::<Msaa>().unwrap(), Msaa::Sample4);
    let s = app.world().entity(sun);
    assert!(!s.contains::<VolumetricLight>());
    assert!(!s.get::<DirectionalLight>().unwrap().contact_shadows_enabled);
    assert_eq!(fog.iter(app.world()).count(), 0);
}

#[test]
fn prepasses_the_camera_had_are_kept() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, RenderPlusPlugin));
    let cam = app
        .world_mut()
        .spawn((
            RenderPlusCamera::default(),
            DepthPrepass,
            Transform::default(),
        ))
        .id();
    app.update();
    let high = RenderProfile::preset(Preset::High);
    set(&mut app, high.only(Effect::Ssao));
    assert!(app.world().entity(cam).contains::<NormalPrepass>());
    set(&mut app, RenderProfile::default());
    let e = app.world().entity(cam);
    assert!(e.contains::<DepthPrepass>() && !e.contains::<NormalPrepass>());
}

#[test]
fn single_effect_only_adds_its_components() {
    let (mut app, cam, sun) = app();
    let high = RenderProfile::preset(Preset::High);
    set(&mut app, high.only(Effect::Volumetric));
    let e = app.world().entity(cam);
    assert!(e.contains::<VolumetricFog>());
    assert!(!e.contains::<DepthPrepass>() && !e.contains::<TemporalAntiAliasing>());
    assert_eq!(*e.get::<Msaa>().unwrap(), Msaa::Sample4);
    assert!(app.world().entity(sun).contains::<VolumetricLight>());
}

#[test]
fn reflective_surfaces_render_deferred_only_while_ssr_is_on() {
    let (mut app, _, _) = app();
    app.init_resource::<Assets<StandardMaterial>>();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    app.world_mut()
        .spawn((MeshMaterial3d(handle.clone()), RenderPlusReflective));
    let method = |app: &App| {
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .opaque_render_method
    };
    app.update();
    assert_eq!(method(&app), OpaqueRendererMethod::Forward);
    let high = RenderProfile::preset(Preset::High);
    set(&mut app, high.only(Effect::Ssr));
    assert_eq!(method(&app), OpaqueRendererMethod::Deferred);
    set(&mut app, high.only(Effect::Ssao));
    assert_eq!(method(&app), OpaqueRendererMethod::Forward);
}
