//! Walkable, streamed, textured Ardea region: terrain + static scenery + capsule controller.
//!
//! `gothic-reforged --walk [--data-dir DIR] [--radius TILES] [--smoke] [--screenshot PREFIX]`
//! F toggles fly/walk, V toggles first/third person, click grabs the mouse, Esc releases.
mod cloud;
mod env;
mod gmat;
#[path = "play/mod.rs"]
mod play;
mod post;
mod precip;
mod windsys;

use crate::world_loader::{
    LoaderHandle, MatData, MatKind, Origin, Part, PartKind, Request, Response, TILE_CM, TexData,
    TileKey, TilePayload, VegPayload, spawn_loader,
};
use anyhow::Result;
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::AccumulatedMouseMotion,
    light::{CascadeShadowConfigBuilder, NotShadowCaster},
    mesh::{Indices, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow, WindowResolution},
};
use gmat::{
    F_A1, F_B0, F_B1, F_FADE, F_MASK, F_NA, F_NB, F_VBLEND, GothicMaterial, GothicMaterialsPlugin,
    GothicParams, SkyMaterial, SkyParams, WaterMaterial, WaterParams,
};
use gothic_physics::{CharacterController, CollisionWorld, ControllerConfig};
use gothic_world::{StreamingSettings, TileStreamer};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Mutex,
    time::Instant,
};

/// Gothic-space origin: the Ardea city centre (cm).
const ORIGIN: Vec3 = Vec3::new(92_400.0, 0.0, -11_000.0);
const CM: f32 = 0.01;
const EYE_HEIGHT_CM: f32 = 165.0;

#[derive(Resource)]
struct LoaderRes {
    requests: std::sync::mpsc::Sender<Request>,
    responses: Mutex<std::sync::mpsc::Receiver<Response>>,
}

/// World-units (metres) distances for ground-cover fade and streaming.
/// Original vegetation view range (VeryHigh, 7500 cm) with the fade start at the middle of the
/// randomised 65..75 % band (`eCVegetation_PS::OnRender`).
const FOLIAGE_FADE: (f32, f32) = (52.5, 75.0);
const VEG_RADIUS: i32 = 3;
/// SpeedTree near/far cross-fade range in metres.
const TREE_LOD: (f32, f32) = (40.0, 55.0);
/// Sign applied to the normal-map green channel (D3D convention vs. the mirrored world).
const NORMAL_Y_SIGN: f32 = 1.0;

#[derive(Resource, Default)]
struct Caches {
    meshes: HashMap<u64, Handle<Mesh>>,
    surfaces: HashMap<(u64, u8), Handle<GothicMaterial>>,
    water: Option<Handle<WaterMaterial>>,
    /// Sea level (metres) taken from the first water surface spawned.
    sea_y: Option<f32>,
    fallback: Option<Handle<GothicMaterial>>,
    images: HashMap<(u64, bool), Option<Handle<Image>>>,
    /// Bounding radius (metres, unscaled mesh) per geometry id, for distance culling.
    radius: HashMap<u64, f32>,
}

#[derive(Resource, Default)]
struct VegStream {
    wanted: HashSet<TileKey>,
    in_flight: HashSet<TileKey>,
    pending: VecDeque<(VegPayload, usize, Vec<Entity>)>,
    spawned: HashMap<TileKey, Vec<Entity>>,
    instances: usize,
}

#[derive(Component)]
struct SkyDome;

#[derive(Component)]
struct Ocean;

#[derive(Resource, Clone, Copy)]
struct SunDir(Vec3);

const SUN_ROT: (f32, f32) = (0.9, -0.85);

struct Pending {
    payload: TilePayload,
    next: usize,
    collision_done: bool,
    entities: Vec<Entity>,
}

#[derive(Resource)]
struct Stream {
    settings: StreamingSettings,
    /// Tile residency under the ported PVS prefetch rules.
    streamer: TileStreamer,
    in_flight: HashSet<TileKey>,
    pending: VecDeque<Pending>,
    spawned: HashMap<TileKey, Vec<Entity>>,
    ready: bool,
    loader_ms: u128,
    records: usize,
    triangles_nav: usize,
    statics: usize,
    statics_with_collision: usize,
    trees: usize,
    tiles_loaded: usize,
    parts_spawned: usize,
    double_sided: bool,
    last_activity: u32,
}

#[derive(Resource)]
struct Collision(CollisionWorld);

#[derive(Resource)]
struct Player {
    ctrl: CharacterController,
    yaw: f32,
    pitch: f32,
    fly: bool,
    third: bool,
    placed: bool,
    /// Fly camera position in cm (origin-relative, mirrored space).
    fly_pos: Vec3,
    scripted_wish: Option<Vec3>,
    /// Gait chosen by the play mode's input layer (`None`: keyboard Shift = sprint).
    scripted_gait: Option<gothic_physics::Gait>,
    /// Jump request of the play mode's input layer (`None`: keyboard Space).
    scripted_jump: Option<bool>,
    /// Play mode: the camera owns yaw / pitch and the ported movement pipeline
    /// (`step_command`) drives the controller.
    command: bool,
    /// Direction the hero turns to instead of the wish direction (dialog, lock-on, aiming).
    face: Option<Vec3>,
}

#[derive(Component)]
struct PlayerCamera;
#[derive(Component)]
struct PlayerBody;

#[derive(Resource)]
struct Smoke {
    enabled: bool,
    prefix: String,
    stage: u32,
    frames: u32,
    frame_times: Vec<f32>,
    walk_log: Vec<(Vec3, bool)>,
    stable: u32,
    shots: Vec<Shot>,
    shot_index: usize,
}

/// Extra screenshot viewpoint: name, position in metres relative to the origin (mirrored
/// space, Y up), yaw and pitch in radians.
struct Shot {
    name: String,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
}

fn parse_shots(spec: &str) -> Vec<Shot> {
    spec.split(';')
        .filter_map(|item| {
            let (name, nums) = item.split_once(':')?;
            let v: Vec<f32> = nums
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            (v.len() == 5).then(|| Shot {
                name: name.trim().to_string(),
                pos: Vec3::new(v[0], v[1], v[2]),
                yaw: v[3],
                pitch: v[4],
            })
        })
        .collect()
}

fn tile_chunk_id(key: TileKey) -> u64 {
    (u64::from(key.0 as u32) << 32) | u64::from(key.1 as u32)
}

pub fn run(args: &[String]) -> Result<()> {
    let arg = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .filter(|s| !s.starts_with("--"))
            .cloned()
    };
    let data_dir = arg("--data-dir")
        .unwrap_or_else(|| r"D:\SteamLibrary\steamapps\common\Gothic 3\Data".to_string());
    let radius: i32 = arg("--radius").and_then(|s| s.parse().ok()).unwrap_or(10);
    let spawn: (f32, f32) = arg("--spawn")
        .and_then(|s| {
            let (x, z) = s.split_once(',')?;
            Some((x.parse().ok()?, z.parse().ok()?))
        })
        .unwrap_or((4000.0, 4000.0));
    let smoke = args.iter().any(|a| a == "--smoke");
    let prefix = arg("--screenshot").unwrap_or_else(|| ".local/ardea".into());
    let shots = arg("--shots").map(|s| parse_shots(&s)).unwrap_or_default();
    let handle: LoaderHandle = spawn_loader(data_dir.clone().into(), Origin(ORIGIN));
    // The grid keeps at least `radius` tiles resident in every direction (the prefetch
    // rectangle is 0.4 x far plus one 10 000-unit cell).
    let pvs_cell_size = gothic_world::pvs::MAIN_CELL_SIZE;
    let settings = StreamingSettings {
        cell_size: TILE_CM,
        pvs_cell_size,
        far: StreamingSettings::far_for_extent(radius as f32 * TILE_CM, pvs_cell_size),
        operations_per_frame: 4,
    };
    let fog_end = radius as f32 * TILE_CM * CM * 0.92;
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / Ardea walk".into(),
                resolution: std::env::var("GR_RES")
                    .ok()
                    .and_then(|s| {
                        let (w, h) = s.split_once('x')?;
                        Some(WindowResolution::new(w.parse().ok()?, h.parse().ok()?))
                    })
                    .unwrap_or_else(|| WindowResolution::new(1280, 720)),
                present_mode: if smoke || std::env::var_os("GR_NOVSYNC").is_some() {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }),
    )
    .add_plugins((GothicMaterialsPlugin, post::PostPlugin))
    .add_plugins(gothic_fx::plugin::FxPlugin {
        data_dir: std::path::PathBuf::from(&data_dir),
        origin: ORIGIN,
        radius: 20_000.0,
    });
    if std::env::var_os("GR_PROF").is_some() {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    app.insert_resource(LoaderRes {
        requests: handle.requests,
        responses: Mutex::new(handle.responses),
    })
    .insert_resource(Stream {
        streamer: TileStreamer::new(settings.clone()),
        settings,
        in_flight: HashSet::new(),
        pending: VecDeque::new(),
        spawned: HashMap::new(),
        ready: false,
        loader_ms: 0,
        records: 0,
        triangles_nav: 0,
        statics: 0,
        statics_with_collision: 0,
        trees: 0,
        tiles_loaded: 0,
        parts_spawned: 0,
        double_sided: args.iter().any(|a| a == "--double-sided"),
        last_activity: 0,
    })
    .insert_resource(Collision(CollisionWorld::new()))
    .insert_resource(Caches::default())
    .insert_resource(Player {
        ctrl: CharacterController::new(
            ControllerConfig::default(),
            Vec3::new(spawn.0, 6000.0, spawn.1),
        ),
        yaw: arg("--yaw").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        pitch: -0.05,
        fly: args.iter().any(|a| a == "--fly"),
        third: false,
        placed: false,
        fly_pos: Vec3::new(0.0, 25_000.0, 0.0),
        scripted_wish: None,
        scripted_gait: None,
        scripted_jump: None,
        command: false,
        face: None,
    })
    .insert_resource(Smoke {
        enabled: smoke,
        prefix,
        stage: 0,
        frames: 0,
        frame_times: Vec::new(),
        walk_log: Vec::new(),
        stable: 0,
        shots,
        shot_index: 0,
    })
    .insert_resource(bevy::light::DirectionalLightShadowMap {
        size: env_f32("GR_SHMAP", 2048.0) as usize,
    })
    .insert_resource(VegStream::default())
    .insert_resource(SunDir(
        Quat::from_euler(EulerRot::YXZ, SUN_ROT.0, SUN_ROT.1, 0.0) * Vec3::Z,
    ))
    .insert_resource(ClearColor(Color::srgb(0.52, 0.68, 0.88)))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 1.0),
        brightness: 650.0,
        ..default()
    })
    .insert_resource(FogEnd(fog_end))
    .insert_resource(env::OrigEnv::new(ORIGIN, args))
    .add_plugins((env::EnvPlugin, precip::PrecipPlugin, windsys::WindPlugin))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            receive_tiles,
            stream_tiles,
            stream_vegetation,
            spawn_pending,
            spawn_vegetation,
            sky_follow,
            ocean_follow,
            input_mode,
            player_motion,
            update_camera,
            smoke_script,
            hud,
        )
            .chain(),
    );
    if play::requested(args) {
        play::install(&mut app, args)?;
    }
    cloud::install(
        &mut app,
        args,
        std::path::Path::new(&data_dir),
        play::requested(args),
    );
    app.add_plugins(crate::perf::PerfPlugin {
        args: args.to_vec(),
    });
    app.run();
    Ok(())
}

#[derive(Resource)]
struct FogEnd(f32);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut skies: ResMut<Assets<SkyMaterial>>,
    fog: Res<FogEnd>,
    sun: Res<SunDir>,
) {
    let fog_color = Color::srgb(0.60, 0.74, 0.92);
    let sky_handle = skies.add(SkyMaterial {
        clouds: None,
        params: SkyParams {
            sun_dir: sun.0.extend(0.0),
            zenith: Vec4::new(0.04, 0.20, 0.80, 1.0),
            horizon: Vec4::new(0.52, 0.70, 0.95, 1.0),
            ground: Vec4::new(0.60, 0.74, 0.92, 1.0),
            sun_color: Vec4::new(1.0, 0.86, 0.62, 1.0),
            cloud: Vec4::new(1.0, 1.0, 1.0, 1.0),
            moon: Vec4::new(0.0, -1.0, 0.0, 0.07),
            moon_color: Vec4::new(0.8, 0.737, 0.733, 0.0),
            sun_misc: Vec4::new(0.1, 1.0, 0.0, 0.0),
        },
    });
    commands.insert_resource(SkyHandle(sky_handle.clone()));
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
        MeshMaterial3d(sky_handle),
        Transform::from_scale(Vec3::splat(3500.0)),
        NotShadowCaster,
        SkyDome,
    ));
    let cam = commands
        .spawn((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 70f32.to_radians(),
                near: 0.05,
                far: 4000.0,
                ..default()
            }),
            Exposure::default(),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Transform::from_xyz(0.0, 250.0, 0.0),
            DistanceFog {
                color: fog_color,
                directional_light_color: Color::srgb(1.0, 0.9, 0.7),
                directional_light_exponent: 30.0,
                falloff: FogFalloff::Linear {
                    start: fog.0 * 0.55,
                    end: fog.0,
                },
            },
            PlayerCamera,
        ))
        .id();
    if std::env::var_os("GR_NOBLOOM").is_none() {
        commands.entity(cam).insert(post::BloomPost::original());
        // The HDR exposure curve of the original needs the unclamped scene.
        if std::env::var_os("GR_HDR").is_some() {
            commands.entity(cam).insert(bevy::camera::Hdr);
        }
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 24_000.0,
            color: Color::srgb(1.0, 0.96, 0.88),
            shadow_maps_enabled: std::env::var_os("GR_NOSHADOW").is_none(),
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: env_f32("GR_CASC", 3.0) as usize,
            minimum_distance: 0.1,
            maximum_distance: env_f32("GR_SHDIST", 90.0),
            // The LsPSM detail map of the original covers `ms_fDetailShadowRadius` (22 m).
            first_cascade_far_bound: env_f32(
                "GR_SHFIRST",
                gothic_render_rules::shadows::DETAIL_RADIUS * CM,
            ),
            overlap_proportion: 0.2,
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, SUN_ROT.0, SUN_ROT.1, 0.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.35, 1.05))),
        MeshMaterial3d::<StandardMaterial>(Handle::default()),
        Transform::default(),
        Visibility::Hidden,
        PlayerBody,
    ));
}

#[derive(Resource)]
struct SkyHandle(Handle<SkyMaterial>);

fn receive_tiles(
    loader: Res<LoaderRes>,
    mut stream: ResMut<Stream>,
    mut veg: ResMut<VegStream>,
    sky: Res<SkyHandle>,
    mut skies: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut orig_env: ResMut<env::OrigEnv>,
    mut precip_tex: ResMut<precip::PrecipTexture>,
) {
    let rx = loader.responses.lock().expect("loader channel");
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Response::Ready {
                records,
                millis,
                cell_weights,
            } => {
                stream.streamer.set_cell_weights(cell_weights);
                stream.ready = true;
                stream.records = records;
                stream.loader_ms = millis;
                eprintln!("loader ready: {records} placed meshes indexed in {millis} ms");
                let _ = loader
                    .requests
                    .send(Request::Texture("wolkentest_01".into()));
                let _ = loader.requests.send(Request::Texture("rainandsnow".into()));
            }
            Response::Texture(name, tex) if name.contains("rainandsnow") => {
                precip_tex.0 = tex
                    .and_then(|t| texture_image(&t, true))
                    .map(|img| images.add(img));
            }
            Response::Texture(_, tex) => {
                if let Some(img) = tex.and_then(|t| texture_image(&t, false))
                    && let Some(mut mat) = skies.get_mut(&sky.0)
                {
                    mat.clouds = Some(images.add(img));
                    mat.params.ground.w = 1.0;
                }
            }
            Response::Weather(p) => orig_env.install(*p),
            Response::Failed(e) => eprintln!("loader: {e}"),
            Response::Vegetation(payload) => {
                let key = payload.key;
                veg.in_flight.remove(&key);
                stream.last_activity = 0;
                if veg.wanted.contains(&key) {
                    veg.instances += payload.instances;
                    veg.pending.push_back((*payload, 0, Vec::new()));
                }
            }
            Response::Tile(payload) => {
                let key = payload.key;
                stream.in_flight.remove(&key);
                stream.statics += payload.stats.statics;
                stream.statics_with_collision += payload.stats.statics_with_collision_mesh;
                stream.trees += payload.stats.trees;
                stream.tiles_loaded += 1;
                stream.last_activity = 0;
                if stream
                    .streamer
                    .is_resident(gothic_world::CellId { x: key.0, z: key.1 })
                {
                    stream.pending.push_back(Pending {
                        payload: *payload,
                        next: 0,
                        collision_done: false,
                        entities: Vec::new(),
                    });
                }
            }
        }
    }
}

fn observer_cm(player: &Player) -> Vec3 {
    if player.fly {
        player.fly_pos
    } else {
        player.ctrl.position
    }
}

fn stream_tiles(
    loader: Res<LoaderRes>,
    mut stream: ResMut<Stream>,
    player: Res<Player>,
    mut collision: ResMut<Collision>,
    mut commands: Commands,
) {
    stream.last_activity += 1;
    if !stream.ready {
        return;
    }
    let at = observer_cm(&player);
    if stream.streamer.observe(at).is_err() {
        return;
    }
    let ops = stream.settings.operations_per_frame;
    for c in stream.streamer.take_unloads(at, ops) {
        let key = (c.x, c.z);
        stream.in_flight.remove(&key);
        stream.pending.retain(|p| p.payload.key != key);
        collision.0.remove_chunk(tile_chunk_id(key));
        if let Some(entities) = stream.spawned.remove(&key) {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
    }
    let room = 24usize.saturating_sub(stream.in_flight.len());
    for c in stream.streamer.next_loads(at, ops.min(room)) {
        stream.in_flight.insert((c.x, c.z));
        let _ = loader.requests.send(Request::Tile((c.x, c.z)));
    }
}

fn texture_image(texture: &TexData, srgb: bool) -> Option<Image> {
    let format = match (&texture.fourcc, srgb) {
        (b"DXT1", true) => TextureFormat::Bc1RgbaUnormSrgb,
        (b"DXT1", false) => TextureFormat::Bc1RgbaUnorm,
        (b"DXT3", true) => TextureFormat::Bc2RgbaUnormSrgb,
        (b"DXT3", false) => TextureFormat::Bc2RgbaUnorm,
        (b"DXT5", true) => TextureFormat::Bc3RgbaUnormSrgb,
        (b"DXT5", false) => TextureFormat::Bc3RgbaUnorm,
        _ => return None,
    };
    if !texture.width.is_multiple_of(4) || !texture.height.is_multiple_of(4) {
        return None;
    }
    let data: Vec<u8> = texture.mips.iter().flatten().copied().collect();
    let mut image = Image::new(
        Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = texture.mips.len() as u32;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    Some(image)
}

fn image_handle(
    tex: &Option<std::sync::Arc<TexData>>,
    srgb: bool,
    caches: &mut Caches,
    images: &mut Assets<Image>,
) -> Option<Handle<Image>> {
    let t = tex.as_ref()?;
    caches
        .images
        .entry((t.id, srgb))
        .or_insert_with(|| texture_image(t, srgb).map(|i| images.add(i)))
        .clone()
}

fn surface_material(
    data: &Option<std::sync::Arc<MatData>>,
    kind: PartKind,
    caches: &mut Caches,
    materials: &mut Assets<GothicMaterial>,
    images: &mut Assets<Image>,
    force_two_sided: bool,
) -> Handle<GothicMaterial> {
    let Some(data) = data else {
        return caches
            .fallback
            .get_or_insert_with(|| {
                materials.add(GothicMaterial {
                    params: GothicParams {
                        tint_a: Vec4::new(0.55, 0.55, 0.5, 1.0),
                        ..default()
                    },
                    a0: None,
                    a1: None,
                    b0: None,
                    b1: None,
                    na: None,
                    nb: None,
                    alpha_mode: AlphaMode::Opaque,
                    two_sided: false,
                })
            })
            .clone();
    };
    let cache_key = (data.id, kind as u8);
    if let Some(h) = caches.surfaces.get(&cache_key) {
        return h.clone();
    }
    let foliage = matches!(kind, PartKind::Foliage | PartKind::Tree | PartKind::TreeFar);
    let wind_mode = match kind {
        PartKind::Foliage => gmat::WindMode::Vegetation,
        PartKind::TreeBark | PartKind::TreeFarBark => gmat::WindMode::Tree,
        PartKind::Tree => gmat::WindMode::TreeLeaves,
        _ => gmat::WindMode::None,
    };
    let mut params = GothicParams::default();
    if wind_mode != gmat::WindMode::None {
        // Leaf cards are about 3 % of the tree height (metres).
        params.wind_mat = Vec4::new(
            wind_mode as i32 as f32,
            data.wind_height,
            data.wind_height * CM * 0.03,
            0.0,
        );
    }
    let mut flags = 0;
    let (l0, l1) = (data.layers.first(), data.layers.get(1));
    let a0 = l0.and_then(|l| image_handle(&l.tex[0], true, caches, images));
    let mut a1 = None;
    let (mut b0, mut b1) = (None, None);
    if let Some(l) = l0 {
        params.tint_a = Vec4::new(l.tint[0], l.tint[1], l.tint[2], 1.0);
        params.scale_a = Vec4::new(l.scale[0][0], l.scale[0][1], l.scale[1][0], l.scale[1][1]);
        if l.tex[1].is_some() && std::env::var_os("GR_NOA1").is_none() {
            a1 = image_handle(&l.tex[1], true, caches, images);
            flags |= F_A1;
        }
    }
    if data.vertex_blend
        && let Some(l) = l1
    {
        flags |= F_VBLEND | F_B0;
        params.tint_b = Vec4::new(l.tint[0], l.tint[1], l.tint[2], 1.0);
        params.scale_b = Vec4::new(l.scale[0][0], l.scale[0][1], l.scale[1][0], l.scale[1][1]);
        b0 = image_handle(&l.tex[0], true, caches, images);
        if l.tex[1].is_some() {
            b1 = image_handle(&l.tex[1], true, caches, images);
            flags |= F_B1;
        }
    }
    let na = data.normals[0]
        .as_ref()
        .and_then(|(t, _)| image_handle(&Some(t.clone()), false, caches, images));
    let nb = data.normals[1]
        .as_ref()
        .and_then(|(t, _)| image_handle(&Some(t.clone()), false, caches, images));
    let mut scale_n = Vec4::ONE;
    if na.is_some() && !foliage {
        flags |= F_NA;
        let s = data.normals[0].as_ref().map_or([1.0; 2], |n| n.1);
        scale_n.x = s[0];
        scale_n.y = s[1];
    }
    if nb.is_some() && !foliage {
        flags |= F_NB;
        let s = data.normals[1].as_ref().map_or([1.0; 2], |n| n.1);
        scale_n.z = s[0];
        scale_n.w = s[1];
    }
    params.scale_n = scale_n;
    gmat::set_uv_anim(&mut params, &data.anim);
    let (alpha_mode, mut two_sided) = match data.blend_mode {
        Some(1) => {
            flags |= F_MASK;
            let r = f32::from(data.mask_reference.unwrap_or(128)) / 255.0;
            params.misc.z = if r < 0.05 { 0.4 } else { r };
            (AlphaMode::Mask(params.misc.z), true)
        }
        Some(2 | 8) => (AlphaMode::Blend, true),
        Some(7) => (AlphaMode::Add, true),
        _ => (AlphaMode::Opaque, false),
    };
    if foliage {
        if flags & F_MASK == 0 {
            params.misc.z = 0.4;
        }
        flags |= F_MASK | 1024;
        two_sided = true;
    }
    if kind == PartKind::Foliage {
        // The vertex alpha of ground cover is the wind weight, not a layer blend.
        flags &= !F_VBLEND;
        flags |= F_FADE;
        params.misc.x = FOLIAGE_FADE.0;
        params.misc.y = FOLIAGE_FADE.1;
    }
    params.misc.w = NORMAL_Y_SIGN;
    params.flags = flags;
    params.ao_strength = if std::env::var_os("GR_NOAO").is_some() {
        0.0
    } else if kind == PartKind::Terrain {
        0.55
    } else {
        0.7
    };
    params.roughness = if kind == PartKind::Terrain {
        0.95
    } else {
        0.85
    };
    let alpha_mode = if flags & F_MASK != 0 {
        AlphaMode::Mask(params.misc.z)
    } else {
        alpha_mode
    };
    let handle = materials.add(GothicMaterial {
        params,
        a0,
        a1,
        b0,
        b1,
        na,
        nb,
        alpha_mode,
        two_sided: two_sided
            || force_two_sided
            || matches!(kind, PartKind::TreeBark | PartKind::TreeFarBark),
    });
    caches.surfaces.insert(cache_key, handle.clone());
    if wind_mode != gmat::WindMode::None {
        gmat::register_wind_material(handle.id());
    }
    handle
}

fn water_material(
    data: &Option<std::sync::Arc<MatData>>,
    caches: &mut Caches,
    materials: &mut Assets<WaterMaterial>,
    images: &mut Assets<Image>,
    sun: Vec3,
) -> Handle<WaterMaterial> {
    if let Some(h) = &caches.water {
        return h.clone();
    }
    let (mut n0, mut n1) = (None, None);
    if let Some(d) = data {
        n0 = d.normals[0]
            .as_ref()
            .and_then(|(t, _)| image_handle(&Some(t.clone()), false, caches, images));
        n1 = d.normals[1]
            .as_ref()
            .and_then(|(t, _)| image_handle(&Some(t.clone()), false, caches, images));
    }
    let n1 = n1.or_else(|| n0.clone());
    let handle = materials.add(WaterMaterial {
        params: WaterParams {
            sun_dir: sun.extend(0.0),
            deep: Vec4::new(0.03, 0.13, 0.18, 1.0),
            shallow: Vec4::new(0.12, 0.34, 0.38, 1.0),
            sky: Vec4::new(0.45, 0.62, 0.86, 1.0),
            sun_color: Vec4::new(1.0, 0.86, 0.62, 1.0),
            misc: Vec4::new(0.5, NORMAL_Y_SIGN, 0.72, 0.0),
        },
        n0,
        n1,
    });
    caches.water = Some(handle.clone());
    handle
}

fn mesh_handle(part: &Part, caches: &mut Caches, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    if let Some(h) = caches.meshes.get(&part.geom.id) {
        return h.clone();
    }
    let g = &part.geom;
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, g.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, g.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, g.colors.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, g.tangents.clone())
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_1,
        if g.light.len() == g.positions.len() {
            g.light.clone()
        } else {
            vec![[0.0f32; 2]; g.positions.len()]
        },
    )
    .with_inserted_indices(Indices::U32(g.indices.clone()));
    let handle = meshes.add(mesh);
    // Terrain tiles and merged ground cover are unique; only shared geometry is worth caching.
    if !matches!(
        part.kind,
        PartKind::Terrain
            | PartKind::Foliage
            | PartKind::MergedSmall
            | PartKind::MergedMid
            | PartKind::MergedBig
    ) {
        caches.meshes.insert(g.id, handle.clone());
    }
    handle
}

#[derive(bevy::ecs::system::SystemParam)]
struct Gpu<'w> {
    caches: ResMut<'w, Caches>,
    meshes: ResMut<'w, Assets<Mesh>>,
    surfaces: ResMut<'w, Assets<GothicMaterial>>,
    waters: ResMut<'w, Assets<WaterMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    sun: Res<'w, SunDir>,
}

fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Profiling ablation: `GR_HIDE=terrain,static,tree,foliage,water` hides those part kinds.
fn profile_hidden(kind: PartKind) -> bool {
    static HIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let h = HIDE.get_or_init(|| std::env::var("GR_HIDE").unwrap_or_default());
    let name = match kind {
        PartKind::Terrain => "terrain",
        PartKind::Static | PartKind::MergedSmall | PartKind::MergedMid | PartKind::MergedBig => {
            "static"
        }
        PartKind::Tree | PartKind::TreeBark | PartKind::TreeFar | PartKind::TreeFarBark => "tree",
        PartKind::Foliage => "foliage",
        PartKind::Water => "water",
    };
    !h.is_empty() && h.split(',').any(|n| n == name)
}

fn spawn_part(
    commands: &mut Commands,
    part: &Part,
    gpu: &mut Gpu,
    force_two_sided: bool,
) -> Entity {
    let mesh = mesh_handle(part, &mut gpu.caches, &mut gpu.meshes);
    let mut transform = Transform::from_matrix(part.transform);
    transform.translation *= CM;
    transform.scale *= CM;
    let is_water = part.kind == PartKind::Water
        || part
            .material
            .as_ref()
            .is_some_and(|m| m.kind == MatKind::Water);
    if is_water {
        let material = water_material(
            &part.material,
            &mut gpu.caches,
            &mut gpu.waters,
            &mut gpu.images,
            gpu.sun.0,
        );
        let sea = *gpu.caches.sea_y.get_or_insert(transform.translation.y);
        // Sea-level tile water is replaced by the single camera-following ocean plane.
        let visibility = if (transform.translation.y - sea).abs() < 0.3 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        return commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                NotShadowCaster,
                visibility,
            ))
            .id();
    }
    let material = surface_material(
        &part.material,
        part.kind,
        &mut gpu.caches,
        &mut gpu.surfaces,
        &mut gpu.images,
        force_two_sided,
    );
    let mut e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform));
    let merged_cull = match part.kind {
        PartKind::MergedSmall => Some(90.0),
        PartKind::MergedMid => Some(200.0),
        _ => None,
    };
    if let Some(end) = merged_cull
        && std::env::var_os("GR_NOCULL").is_none()
    {
        e.insert(bevy::camera::visibility::VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: end..end,
            use_aabb: true,
        });
    }
    if part.kind == PartKind::Static && std::env::var_os("GR_NOCULL").is_none() {
        // Small props vanish well before the fog end; saves per-entity render work.
        let r = *gpu.caches.radius.entry(part.geom.id).or_insert_with(|| {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for p in &part.geom.positions {
                lo = lo.min(Vec3::from(*p));
                hi = hi.max(Vec3::from(*p));
            }
            ((hi - lo).length() * 0.5).max(0.0)
        });
        let scale = transform.scale.max_element().abs();
        // Placed visuals with LoD meshes use the bounding sphere of the whole visual so every
        // stage switches at the same distances.
        let radius_m = part.lod.map_or(r * scale, |l| l.radius_cm * CM);
        if std::env::var_os("GR_DUMP").is_some() && radius_m > 3.0 {
            let t = transform.translation;
            if (t.x - 40.0).abs() < 30.0 && (t.z - 40.0).abs() < 30.0 {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for u in &part.geom.uvs {
                    for k in 0..2 {
                        lo[k] = lo[k].min(u[k]);
                        hi[k] = hi[k].max(u[k]);
                    }
                }
                let m = part.material.as_ref().map(|m| {
                    (
                        m.layer_names.clone(),
                        m.layers
                            .iter()
                            .map(|l| (l.scale, l.tex[0].as_ref().map(|t| (t.width, t.height))))
                            .collect::<Vec<_>>(),
                        m.vertex_blend,
                    )
                });
                eprintln!(
                    "DUMP t={t:?} r={radius_m:.1} verts={} uv {lo:?}..{hi:?} mat={m:?}",
                    part.geom.positions.len()
                );
            }
        }
        if radius_m < env_f32("GR_SHMIN", 3.0) {
            e.insert(NotShadowCaster);
        }
        // Original small-object culling (`CullSmallObjects`): fade between the distances where the
        // projected area factor crosses S and 0.6 S, S = fScreenObjectDistanceCulling (VeryHigh).
        let (fade_start, fade_end) = gothic_render_rules::cull::distance_band(
            radius_m,
            gothic_render_rules::cull::screen_cull_factor(0),
        );
        let mut range = bevy::camera::visibility::VisibilityRange {
            start_margin: 0.0..0.0,
            // Floor of 15 m so pick-up items stay visible (deviation: the original has no floor).
            end_margin: fade_start.clamp(15.0, 600.0)..fade_end.clamp(20.0, 800.0),
            use_aabb: true,
        };
        if let Some(l) = part.lod {
            // Mesh LoD stages (`CalculateLoDStage`): hard switches at the distances where the
            // area factor crosses the stage table.
            let global = env_f32("GR_LODG", gothic_render_rules::lod::global_factor(0));
            let d = gothic_render_rules::lod::stage_distances(
                radius_m,
                usize::from(l.stages),
                global,
                1.0,
            );
            let stage = usize::from(l.stage);
            let start = if stage == 0 { 0.0 } else { d[stage - 1] };
            range.start_margin = start..start;
            range.use_aabb = false;
            if let Some(&end) = d.get(stage) {
                range.end_margin = end..end;
            } else {
                let a = range.end_margin.start.max(start + 1.0);
                range.end_margin = a..range.end_margin.end.max(a + 1.0);
            }
        }
        e.insert(range);
    }
    if profile_hidden(part.kind) {
        e.insert(Visibility::Hidden);
    }
    if matches!(
        part.kind,
        PartKind::Foliage | PartKind::Tree | PartKind::TreeFar
    ) {
        e.insert(NotShadowCaster);
    }
    // SpeedTree LOD: full geometry near, whole-tree billboard far, dither cross-fade between.
    match part.kind {
        PartKind::Tree | PartKind::TreeBark => {
            e.insert(bevy::camera::visibility::VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: TREE_LOD.0..TREE_LOD.1,
                use_aabb: false,
            });
        }
        PartKind::TreeFar | PartKind::TreeFarBark => {
            e.insert(bevy::camera::visibility::VisibilityRange {
                start_margin: TREE_LOD.0..TREE_LOD.1,
                end_margin: 1.0e6..1.0e6,
                use_aabb: false,
            });
        }
        _ => {}
    }
    e.id()
}

fn spawn_pending(
    mut commands: Commands,
    mut stream: ResMut<Stream>,
    mut collision: ResMut<Collision>,
    mut gpu: Gpu,
) {
    let start = Instant::now();
    let double_sided = stream.double_sided;
    while start.elapsed().as_micros() < 4000 {
        let Some(front) = stream.pending.front_mut() else {
            break;
        };
        if !front.collision_done {
            let tris: Vec<[Vec3; 3]> = std::mem::take(&mut front.payload.collision);
            let n = tris.len();
            collision
                .0
                .insert_chunk(tile_chunk_id(front.payload.key), tris);
            front.collision_done = true;
            if front.payload.terrain_collision_from_nav {
                stream.triangles_nav += n;
            }
            continue;
        }
        let front = stream.pending.front_mut().expect("front exists");
        if front.next >= front.payload.parts.len() {
            let done = stream.pending.pop_front().expect("front exists");
            stream.spawned.insert(done.payload.key, done.entities);
            continue;
        }
        let part = &front.payload.parts[front.next];
        front.next += 1;
        let entity = spawn_part(&mut commands, part, &mut gpu, double_sided);
        if cloud::statics::routes(part.kind) && !profile_hidden(part.kind) {
            commands.entity(entity).insert(cloud::statics::StaticProp);
        }
        front.entities.push(entity);
        stream.parts_spawned += 1;
    }
}

/// Request ground cover for tiles near the player, drop it again further out.
fn stream_vegetation(
    loader: Res<LoaderRes>,
    stream: Res<Stream>,
    player: Res<Player>,
    mut veg: ResMut<VegStream>,
    mut commands: Commands,
) {
    if !stream.ready || std::env::var_os("GR_NOVEG").is_some() {
        return;
    }
    let center = stream.settings.cell_at(observer_cm(&player));
    let dist = |k: &TileKey| (k.0 - center.x).abs().max((k.1 - center.z).abs());
    let drop: Vec<TileKey> = veg
        .wanted
        .iter()
        .filter(|k| dist(k) > VEG_RADIUS + 1)
        .copied()
        .collect();
    for k in drop {
        veg.wanted.remove(&k);
        veg.in_flight.remove(&k);
        veg.pending.retain(|(p, _, _)| p.key != k);
        if let Some(entities) = veg.spawned.remove(&k) {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
    }
    let mut wanted_now = Vec::new();
    for dx in -VEG_RADIUS..=VEG_RADIUS {
        for dz in -VEG_RADIUS..=VEG_RADIUS {
            let k = (center.x + dx, center.z + dz);
            if !veg.wanted.contains(&k) && stream.spawned.contains_key(&k) {
                wanted_now.push(k);
            }
        }
    }
    wanted_now.sort_by_key(|k| dist(k));
    for k in wanted_now.into_iter().take(2) {
        if veg.in_flight.len() >= 4 {
            break;
        }
        veg.wanted.insert(k);
        veg.in_flight.insert(k);
        let _ = loader.requests.send(Request::Vegetation(k));
    }
}

fn spawn_vegetation(mut commands: Commands, mut veg: ResMut<VegStream>, mut gpu: Gpu) {
    let start = Instant::now();
    while start.elapsed().as_micros() < 2500 {
        let Some(front) = veg.pending.front_mut() else {
            break;
        };
        if front.1 >= front.0.parts.len() {
            let (payload, _, entities) = veg.pending.pop_front().expect("front exists");
            veg.spawned.insert(payload.key, entities);
            continue;
        }
        let part = &front.0.parts[front.1];
        front.1 += 1;
        let entity = spawn_part(&mut commands, part, &mut gpu, true);
        front.2.push(entity);
    }
}

/// One huge water plane at sea level that follows the camera, so the sea reaches the horizon
/// instead of ending at tile borders.
fn ocean_follow(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    caches: Res<Caches>,
    camera: Query<&Transform, (With<PlayerCamera>, Without<Ocean>)>,
    mut ocean: Query<&mut Transform, With<Ocean>>,
) {
    let Ok(cam) = camera.single() else { return };
    if let Ok(mut t) = ocean.single_mut() {
        t.translation.x = cam.translation.x;
        t.translation.z = cam.translation.z;
        return;
    }
    if let (Some(handle), Some(y)) = (&caches.water, caches.sea_y) {
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(8000.0, 8000.0))),
            MeshMaterial3d(handle.clone()),
            Transform::from_xyz(cam.translation.x, y - 0.05, cam.translation.z),
            NotShadowCaster,
            Ocean,
        ));
    }
}

fn sky_follow(
    camera: Query<&Transform, (With<PlayerCamera>, Without<SkyDome>)>,
    mut sky: Query<&mut Transform, With<SkyDome>>,
) {
    if let (Ok(c), Ok(mut s)) = (camera.single(), sky.single_mut()) {
        s.translation = c.translation;
        if std::env::var_os("GR_NOSKY").is_some() {
            s.scale = Vec3::ZERO;
        }
    }
}

fn input_mode(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut player: ResMut<Player>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    camera: Query<&Transform, With<PlayerCamera>>,
) {
    if keys.just_pressed(KeyCode::KeyF) {
        player.fly = !player.fly;
        if player.fly {
            if let Ok(t) = camera.single() {
                player.fly_pos = t.translation / CM;
            }
        } else {
            // Drop back into walk mode where the fly camera is.
            let p = player.fly_pos;
            player.ctrl.position = p - Vec3::Y * EYE_HEIGHT_CM;
            player.ctrl.vertical_speed = 0.0;
            player.ctrl.grounded = false;
        }
    }
    if keys.just_pressed(KeyCode::KeyV) {
        player.third = !player.third;
    }
    if let Ok(mut c) = cursor.single_mut() {
        if buttons.just_pressed(MouseButton::Left) {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
        }
        if keys.just_pressed(KeyCode::Escape) {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
        }
    }
}

fn player_motion(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    time: Res<Time>,
    stream: Res<Stream>,
    collision: Res<Collision>,
    caches: Res<Caches>,
    mut player: ResMut<Player>,
) {
    let dt = time.delta_secs().min(0.1);
    if !player.command
        && cursor
            .single()
            .is_ok_and(|c| c.grab_mode != CursorGrabMode::None)
    {
        player.yaw -= mouse.delta.x * 0.0025;
        player.pitch = (player.pitch - mouse.delta.y * 0.0025).clamp(-1.5, 1.5);
    }
    let rot = Quat::from_rotation_y(player.yaw);
    let forward = rot * Vec3::NEG_Z;
    let right = rot * Vec3::X;
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish -= right;
    }
    if let Some(w) = player.scripted_wish {
        wish = rot * w;
    }
    let sprint = keys.pressed(KeyCode::ShiftLeft);
    if player.fly {
        let look = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
        let mut dir = Vec3::ZERO;
        {
            if keys.pressed(KeyCode::KeyW) {
                dir += look * Vec3::NEG_Z;
            }
            if keys.pressed(KeyCode::KeyS) {
                dir -= look * Vec3::NEG_Z;
            }
            if keys.pressed(KeyCode::KeyD) {
                dir += look * Vec3::X;
            }
            if keys.pressed(KeyCode::KeyA) {
                dir -= look * Vec3::X;
            }
        }
        if keys.pressed(KeyCode::Space) {
            dir.y += 1.0;
        }
        if keys.pressed(KeyCode::ControlLeft) {
            dir.y -= 1.0;
        }
        let speed = if sprint { 6000.0 } else { 1500.0 };
        player.fly_pos += dir.normalize_or_zero() * speed * dt;
        return;
    }
    let tile = stream.settings.cell_at(player.ctrl.position);
    let tile_ready = collision.0.has_chunk(tile_chunk_id((tile.x, tile.z)));
    if !player.placed {
        if !tile_ready {
            return;
        }
        let p = player.ctrl.position;
        if let Some(y) = collision
            .0
            .ground_below(Vec3::new(p.x, 15_000.0, p.z), 40_000.0)
        {
            player.ctrl.position.y = y + 5.0;
            player.placed = true;
        } else {
            return;
        }
    }
    if !tile_ready {
        return; // never integrate gravity into a void
    }
    // Speeds come from the PC_Hero movement template (Run 400 cm/s, Sprint 600 cm/s), with the
    // directional limits of the original; see PHYSICS_MOVEMENT.md.
    let gait = player.scripted_gait.unwrap_or(if sprint {
        gothic_physics::Gait::Sprint
    } else {
        gothic_physics::Gait::Run
    });
    let jump = player
        .scripted_jump
        .take()
        .unwrap_or_else(|| keys.just_pressed(KeyCode::Space));
    if player.command {
        // E01/E02: wish direction from the camera frame, the turn model, mode machine and swim
        // run inside `step_command`. The water surface is the sea level [I: lakes and rivers
        // have no recorded level yet].
        let local = player.scripted_wish.unwrap_or(Vec3::ZERO);
        let keys_axes = gothic_physics::movement::MoveKeys {
            forward: (-local.z).max(0.0),
            backward: local.z.max(0.0),
            left: (-local.x).max(0.0),
            right: local.x.max(0.0),
        };
        let wish = gothic_physics::movement::wish_direction(forward, right, &keys_axes);
        let water_level = caches
            .sea_y
            .map(|y| y * 100.0)
            .filter(|y| player.ctrl.position.y < y + 400.0);
        player.ctrl.player_controlled = true;
        let cmd = gothic_physics::MoveCommand {
            wish,
            gait,
            jump,
            face: player.face,
            water_level,
            ..Default::default()
        };
        player.ctrl.step_command(&collision.0, &cmd, dt);
        return;
    }
    player.ctrl.facing = Some(Vec2::new(forward.x, forward.z));
    player.ctrl.step_gait(&collision.0, wish, gait, jump, dt);
}

fn update_camera(
    fog_end: Res<FogEnd>,
    orig_env: Res<env::OrigEnv>,
    mut fog: Query<&mut DistanceFog, With<PlayerCamera>>,
    mut player: ResMut<Player>,
    collision: Res<Collision>,
    mut camera: Query<&mut Transform, (With<PlayerCamera>, Without<PlayerBody>)>,
    mut body: Query<(&mut Transform, &mut Visibility), With<PlayerBody>>,
) {
    let Ok(mut cam) = camera.single_mut() else {
        return;
    };
    // Fog must not swallow the world when the camera is high above it.
    if orig_env.packet.is_none()
        && let Ok(mut f) = fog.single_mut()
    {
        let height = cam.translation.y.max(0.0);
        let end = fog_end.0 + 3.0 * (height - 15.0).max(0.0);
        f.falloff = FogFalloff::Linear {
            start: end * 0.35,
            end,
        };
    }
    let look = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    if player.fly {
        cam.translation = player.fly_pos * CM;
        cam.rotation = look;
        if let Ok((_, mut v)) = body.single_mut() {
            *v = Visibility::Hidden;
        }
        return;
    }
    let eye = player.ctrl.position + Vec3::Y * EYE_HEIGHT_CM;
    let mut pos = eye;
    if player.third {
        let back = look * Vec3::Z;
        let want = 320.0;
        let dist = collision
            .0
            .raycast(eye, back, want)
            .map_or(want, |d| (d - 25.0).max(30.0));
        pos = eye + back * dist + Vec3::Y * 30.0;
    }
    cam.translation = pos * CM;
    cam.rotation = look;
    if let Ok((mut t, mut v)) = body.single_mut() {
        t.translation = (player.ctrl.position + Vec3::Y * 87.5) * CM;
        *v = if player.third {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let _ = &mut player;
}

fn smoke_script(
    mut commands: Commands,
    mut smoke: ResMut<Smoke>,
    mut player: ResMut<Player>,
    stream: Res<Stream>,
    veg: Res<VegStream>,
    time: Res<Time>,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
    diags: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    if !smoke.enabled {
        return;
    }
    smoke.frames += 1;
    let loaded_all = stream.ready
        && stream.in_flight.is_empty()
        && stream.pending.is_empty()
        && veg.in_flight.is_empty()
        && veg.pending.is_empty()
        && player.placed_or_fly();
    if loaded_all && stream.last_activity > 5 {
        smoke.stable += 1;
    } else {
        smoke.stable = 0;
    }
    let timeout = smoke.frames > 60 * 240;
    let shoot = |commands: &mut Commands, path: String| {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    };
    match smoke.stage {
        0 if smoke.stable > 90 || timeout => {
            eprintln!(
                "streaming settled after {} frames: tiles {} parts {} collision-tris {} loader-startup {} ms",
                smoke.frames,
                stream.tiles_loaded,
                stream.parts_spawned,
                stream.triangles_nav,
                stream.loader_ms
            );
            smoke.stage = 1;
            smoke.frames = 0;
            smoke.frame_times.clear();
            player.fly = false;
        }
        1 => {
            smoke.frame_times.push(time.delta_secs());
            if smoke.frames == 120 {
                let name = format!("{}-walk.png", smoke.prefix);
                shoot(&mut commands, name);
                let avg = smoke.frame_times[30..].iter().sum::<f32>()
                    / (smoke.frame_times.len() - 30) as f32;
                let mut lines: Vec<String> = diags
                    .iter()
                    .filter(|d| d.path().as_str().starts_with("render/"))
                    .filter_map(|d| {
                        d.average().map(|a| {
                            format!("{}={:.3}", d.path().as_str().replace("render/", ""), a)
                        })
                    })
                    .collect();
                lines.sort();
                if !lines.is_empty() {
                    eprintln!("DIAG {}", lines.join(" "));
                }
                let mut sorted = smoke.frame_times[30..].to_vec();
                sorted.sort_by(f32::total_cmp);
                eprintln!(
                    "FRAME median {:.2} ms p95 {:.2} ms; mesh entities {} visible {}",
                    sorted[sorted.len() / 2] * 1000.0,
                    sorted[sorted.len() * 95 / 100] * 1000.0,
                    meshes.iter().count(),
                    meshes.iter().filter(|v| v.get()).count()
                );
                eprintln!(
                    "FRAME TIME standing in Ardea: {:.2} ms avg ({:.0} fps)",
                    avg * 1000.0,
                    1.0 / avg
                );
            }
            if smoke.frames == 150 {
                smoke.stage = 2;
                smoke.frames = 0;
                player.scripted_wish = Some(Vec3::NEG_Z);
                smoke.walk_log.clear();
            }
        }
        2 => {
            // Walk forward for 6 s through the real collision to prove the controller.
            let p = player.ctrl.position;
            let g = player.ctrl.grounded;
            smoke.walk_log.push((p, g));
            if smoke.frames == 360 {
                player.scripted_wish = None;
                let start = smoke.walk_log[0].0;
                let end = smoke.walk_log.last().expect("log").0;
                let grounded = smoke.walk_log.iter().filter(|(_, g)| *g).count();
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for (p, _) in &smoke.walk_log {
                    lo = lo.min(p.y);
                    hi = hi.max(p.y);
                }
                eprintln!(
                    "WALK TEST: moved {:.0} cm horizontally in 6 s, height {:.0} -> {:.0} (range {:.0}..{:.0}), grounded {}/{} frames",
                    Vec3::new(end.x - start.x, 0.0, end.z - start.z).length(),
                    start.y,
                    end.y,
                    lo,
                    hi,
                    grounded,
                    smoke.walk_log.len()
                );
                // Overview from above.
                player.fly = true;
                player.fly_pos = Vec3::new(1500.0, 14_000.0, 9000.0);
                player.yaw = 0.0;
                player.pitch = -0.55;
                smoke.stage = 3;
                smoke.frames = 0;
            }
        }
        3 if smoke.frames > 200 && stream.in_flight.is_empty() && stream.pending.is_empty() => {
            let name = format!("{}-overview.png", smoke.prefix);
            shoot(&mut commands, name);
            smoke.stage = 4;
            smoke.frames = 0;
        }
        4 if smoke.frames > 90 => {
            if smoke.shots.is_empty() {
                commands.write_message(AppExit::Success);
            } else {
                smoke.stage = 5;
                smoke.frames = 0;
                smoke.stable = 0;
            }
        }
        5 => {
            let i = smoke.shot_index;
            if i >= smoke.shots.len() {
                if smoke.frames > 130 {
                    commands.write_message(AppExit::Success);
                }
            } else if smoke.frames == 1 {
                let (pos, yaw, pitch) =
                    (smoke.shots[i].pos, smoke.shots[i].yaw, smoke.shots[i].pitch);
                player.fly = true;
                player.fly_pos = pos / CM;
                player.yaw = yaw;
                player.pitch = pitch;
            } else if smoke.frames > 60 && smoke.stable > 60 {
                let name = format!("{}-{}.png", smoke.prefix, smoke.shots[i].name);
                shoot(&mut commands, name);
                smoke.shot_index += 1;
                smoke.frames = 100;
                smoke.stable = 0;
                if smoke.shot_index < smoke.shots.len() {
                    smoke.frames = 0;
                }
            }
        }
        _ => {}
    }
}

impl Player {
    fn placed_or_fly(&self) -> bool {
        self.placed || self.fly
    }
}

fn hud(
    stream: Res<Stream>,
    player: Res<Player>,
    time: Res<Time>,
    mut windows: Query<&mut Window>,
    mut tick: Local<u32>,
) {
    *tick += 1;
    if !(*tick).is_multiple_of(30) {
        return;
    }
    let p = observer_cm(&player);
    for mut w in &mut windows {
        w.title = format!(
            "Gothic Reforged | {} | {:.0} fps | tiles {} loading {} | pos {:.0},{:.0},{:.0} m | F fly  V camera  click=look",
            if player.fly {
                "FLY"
            } else if player.grounded_label() {
                "WALK"
            } else {
                "AIR"
            },
            1.0 / time.delta_secs().max(1e-4),
            stream.spawned.len(),
            stream.in_flight.len() + stream.pending.len(),
            p.x * CM,
            p.y * CM,
            p.z * CM,
        );
    }
}

impl Player {
    fn grounded_label(&self) -> bool {
        self.ctrl.grounded
    }
}
