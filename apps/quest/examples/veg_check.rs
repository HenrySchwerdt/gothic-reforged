//! Host render of the Ardea ground cover with the Quest shaders (needs the local Ardea pack with `veg/`, see pack_veg).
//! Renders each view to an offscreen target, writes `.local/quest/veg_<name>.ppm` and prints chunk / instance / draw /
//! triangle counts per distance band. Usage: veg_check [out_dir=.local/quest] [width=1280] [height=1280]
//! Views: env GR_VEG_VIEWS="name:x_m,z_m,yaw_deg,pitch_deg,eye_above_ground_m;..." (Bevy-space metres).
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use gothic_quest::ardea::{self, TriCount};
use gothic_quest::vegetation::{Veg, VegPlugin};

#[derive(Resource)]
struct NextPath(PathBuf);

fn on_shot(cap: On<ScreenshotCaptured>, p: Res<NextPath>, o: Res<Out>) {
    let (w, h) = (o.1, o.2);
    if let Some(d) = &cap.image.data {
        let mut b = format!("P6
{w} {h}
255
").into_bytes();
        for px in d.chunks_exact(4) {
            b.extend_from_slice(&px[..3]);
        }
        let _ = std::fs::write(&p.0, b);
        println!("wrote {}", p.0.display());
    }
}

#[derive(Resource)]
struct Out(PathBuf, u32, u32);

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(a.get(1).map_or(".local/quest", String::as_str));
    let w: u32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(1280);
    let h: u32 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(1280);
    let dir = ardea::data_dir().expect("run from the repo root with .local/quest/ardea-pack present");
    let views: Vec<(String, [f32; 5])> = std::env::var("GR_VEG_VIEWS")
        .unwrap_or_else(|_| "plaza:-924,-110,200,-8,1.65".into())
        .split(';')
        .filter_map(|s| {
            let (n, v) = s.split_once(':')?;
            let f: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            Some((n.to_string(), [*f.first()?, *f.get(1)?, *f.get(2)?, *f.get(3)?, *f.get(4).unwrap_or(&1.65)]))
        })
        .collect();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin { primary_window: None, exit_condition: bevy::window::ExitCondition::DontExit, ..default() })
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    );
    app.add_plugins(gothic_quest::qmat::QuestMaterialPlugin);
    app.insert_resource(gothic_xr_quest::XrFrameData::default());
    ardea::build(&mut app, dir).expect("ardea build");
    app.add_plugins(VegPlugin);
    app.insert_resource(Out(out.clone(), w, h));
    app.add_observer(on_shot);
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    let mut timg = Image::new_target_texture(w, h, TextureFormat::Rgba8UnormSrgb, None);
    timg.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(timg);
    let fog = gothic_quest::qmat::fog_color();
    let cam = app
        .world_mut()
        .spawn((
            gothic_xr_quest::XrEye(0),
            Camera3d::default(),
            Camera { clear_color: ClearColorConfig::Custom(Color::srgb(fog.x, fog.y, fog.z)), ..default() },
            bevy::camera::RenderTarget::Image(target.clone().into()),
            Projection::Perspective(PerspectiveProjection { fov: 100f32.to_radians(), ..default() }),
            Msaa::Off,
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            bevy::core_pipeline::tonemapping::DebandDither::Disabled,
            bevy::render::view::NoIndirectDrawing,
            Transform::from_xyz(-924.0, 200.0, -110.0),
        ))
        .id();

    // Stream the cells around the start (5 ms per frame).
    for _ in 0..600 {
        app.update();
    }
    let a = app.world().resource::<ardea::Ardea>();
    println!("cells loaded {} spawned {} scene tris {}", a.cells_done, a.spawned, a.tris);

    for (name, v) in views {
        let ground = {
            let w = app.world();
            w.resource::<gothic_vr::VrWorld>().0.ground_below(Vec3::new(v[0] * 100.0, 50_000.0, v[1] * 100.0), 100_000.0)
        };
        let gy = ground.map_or(0.0, |y| y / 100.0);
        let pos = Vec3::new(v[0], gy + v[4], v[1]);
        let rot = Quat::from_euler(EulerRot::YXZ, v[2].to_radians(), v[3].to_radians(), 0.0);
        *app.world_mut().get_mut::<Transform>(cam).unwrap() = Transform { translation: pos, rotation: rot, scale: Vec3::ONE };
        // Let the chunk builder catch up and the fade settle.
        for _ in 0..240 {
            app.update();
        }
        report(&mut app, &name, pos);
        let (o, w, h) = {
            let o = app.world().resource::<Out>();
            (o.0.clone(), o.1, o.2)
        };
        let path = o.join(format!("veg_{name}.ppm"));
        app.world_mut().insert_resource(NextPath(path));
        app.world_mut().spawn(Screenshot::image(target.clone()));
        for _ in 0..120 {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    // Walk back and forth (hysteresis / despawn check): 120 m out and back at 6 m/s, 72 fps.
    let (mut max_res, mut max_tris) = (0usize, 0u64);
    for pass in 0..2 {
        for f in 0..1440 {
            let t = f as f32 / 72.0;
            let x = -924.0 + 6.0 * t;
            let x = if pass == 0 { x } else { -924.0 + 120.0 - 6.0 * t };
            let z = -110.0;
            let gy = app.world().resource::<gothic_vr::VrWorld>().0.ground_below(Vec3::new(x * 100.0, 50_000.0, z * 100.0), 100_000.0).map_or(0.0, |y| y / 100.0);
            app.world_mut().get_mut::<Transform>(cam).unwrap().translation = Vec3::new(x, gy + 1.65, z);
            app.update();
            let s = &app.world().resource::<Veg>().stats;
            max_res = max_res.max(s.chunks);
            max_tris = max_tris.max(s.tris);
        }
        let s = app.world().resource::<Veg>().stats.clone();
        println!("walk pass {pass}: builds {} despawns {} max resident chunks {max_res} max resident tris {max_tris} max spawn ms {:.2}", s.builds, s.despawns, s.max_spawn_ms);
    }
}

fn report(app: &mut App, name: &str, pos: Vec3) {
    let w = app.world_mut();
    let mut q = w.query::<(&ViewVisibility, &TriCount)>();
    let (mut n, mut tris) = (0u32, 0u64);
    for (v, t) in q.iter(w) {
        if v.get() {
            n += 1;
            tris += t.0 as u64;
        }
    }
    let veg = w.resource::<Veg>();
    let s = &veg.stats;
    println!(
        "[{name}] eye {:.0},{:.1},{:.0} m | veg: chunks resident {} (visible {}), instances resident {}, resident tris {}, visible draws {}, visible tris {} | whole scene visible draws {} tris {}",
        pos.x, pos.y, pos.z, s.chunks, s.chunks_visible, s.instances, s.tris, s.draws_visible, s.tris_visible, n, tris
    );
    for (lo, hi, c, i, t) in veg.bands(&[0.0, 500.0, 1200.0, 2000.0, 3000.0, 4000.0, 6000.0]) {
        println!("    band {:5.0}-{:5.0} cm: chunks {c:4} instances {i:7} tris {t:8}", lo, hi);
    }
}
