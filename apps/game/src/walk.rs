//! Walkable, streamed, textured Ardea region: terrain + static scenery + capsule controller.
//!
//! `gothic-reforged --walk [--data-dir DIR] [--radius TILES] [--smoke] [--screenshot PREFIX]`
//! F toggles fly/walk, V toggles first/third person, click grabs the mouse, Esc releases.
use crate::world_loader::{
    LoaderHandle, MatData, Origin, Part, Request, Response, TILE_CM, TexData, TileKey, TilePayload,
    spawn_loader,
};
use anyhow::Result;
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::AccumulatedMouseMotion,
    light::CascadeShadowConfigBuilder,
    mesh::{Indices, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow, WindowResolution},
};
use gothic_physics::{CharacterController, CollisionWorld, ControllerConfig};
use gothic_world::{CellId, StreamingSettings, plan};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
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

#[derive(Resource, Default)]
struct Caches {
    meshes: HashMap<u64, Handle<Mesh>>,
    materials: HashMap<u64, Handle<StandardMaterial>>,
    textures: HashMap<u64, Option<Handle<Image>>>,
}

struct Pending {
    payload: TilePayload,
    next: usize,
    collision_done: bool,
    entities: Vec<Entity>,
}

#[derive(Resource)]
struct Stream {
    settings: StreamingSettings,
    resident: BTreeSet<CellId>,
    in_flight: HashSet<TileKey>,
    pending: VecDeque<Pending>,
    spawned: HashMap<TileKey, Vec<Entity>>,
    ready: bool,
    loader_ms: u128,
    records: usize,
    triangles_nav: usize,
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
    let handle: LoaderHandle = spawn_loader(data_dir.into(), Origin(ORIGIN));
    let settings = StreamingSettings {
        cell_size: TILE_CM,
        load_radius: radius,
        retain_radius: radius + 1,
        operations_per_frame: 4,
    };
    let fog_end = radius as f32 * TILE_CM * CM * 0.92;
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / Ardea walk".into(),
                resolution: WindowResolution::new(1280, 720),
                present_mode: if smoke {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }))
        .insert_resource(LoaderRes {
            requests: handle.requests,
            responses: Mutex::new(handle.responses),
        })
        .insert_resource(Stream {
            settings,
            resident: BTreeSet::new(),
            in_flight: HashSet::new(),
            pending: VecDeque::new(),
            spawned: HashMap::new(),
            ready: false,
            loader_ms: 0,
            records: 0,
            triangles_nav: 0,
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
        })
        .insert_resource(Smoke {
            enabled: smoke,
            prefix,
            stage: 0,
            frames: 0,
            frame_times: Vec::new(),
            walk_log: Vec::new(),
            stable: 0,
        })
        .insert_resource(ClearColor(Color::srgb(0.52, 0.68, 0.88)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.75, 0.82, 1.0),
            brightness: 650.0,
            ..default()
        })
        .insert_resource(FogEnd(fog_end))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                receive_tiles,
                stream_tiles,
                spawn_pending,
                input_mode,
                player_motion,
                update_camera,
                smoke_script,
                hud,
            )
                .chain(),
        )
        .run();
    Ok(())
}

#[derive(Resource)]
struct FogEnd(f32);

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, fog: Res<FogEnd>) {
    let fog_color = Color::srgb(0.70, 0.78, 0.88);
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            near: 0.05,
            far: 4000.0,
            ..default()
        }),
        Exposure::default(),
        Transform::from_xyz(0.0, 250.0, 0.0),
        DistanceFog {
            color: fog_color,
            directional_light_color: Color::srgb(1.0, 0.9, 0.7),
            directional_light_exponent: 30.0,
            falloff: FogFalloff::Linear {
                start: fog.0 * 0.35,
                end: fog.0,
            },
        },
        PlayerCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 16_000.0,
            color: Color::srgb(1.0, 0.96, 0.88),
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            maximum_distance: 160.0,
            first_cascade_far_bound: 10.0,
            overlap_proportion: 0.2,
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.85, 0.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.35, 1.05))),
        MeshMaterial3d::<StandardMaterial>(Handle::default()),
        Transform::default(),
        Visibility::Hidden,
        PlayerBody,
    ));
}

fn receive_tiles(loader: Res<LoaderRes>, mut stream: ResMut<Stream>) {
    let rx = loader.responses.lock().expect("loader channel");
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Response::Ready { records, millis } => {
                stream.ready = true;
                stream.records = records;
                stream.loader_ms = millis;
                eprintln!("loader ready: {records} placed meshes indexed in {millis} ms");
            }
            Response::Failed(e) => eprintln!("loader: {e}"),
            Response::Tile(payload) => {
                let key = payload.key;
                stream.in_flight.remove(&key);
                stream.tiles_loaded += 1;
                stream.last_activity = 0;
                if stream.resident.contains(&CellId { x: key.0, z: key.1 }) {
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
    let center = stream.settings.cell_at(observer_cm(&player));
    let delta = plan(&stream.settings, &stream.resident, center);
    for c in delta.unload {
        stream.resident.remove(&c);
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
    for c in delta.load {
        if stream.in_flight.len() >= 24 {
            break;
        }
        stream.resident.insert(c);
        stream.in_flight.insert((c.x, c.z));
        let _ = loader.requests.send(Request::Tile((c.x, c.z)));
    }
}

fn texture_image(texture: &TexData) -> Option<Image> {
    let format = match &texture.fourcc {
        b"DXT1" => TextureFormat::Bc1RgbaUnormSrgb,
        b"DXT3" => TextureFormat::Bc2RgbaUnormSrgb,
        b"DXT5" => TextureFormat::Bc3RgbaUnormSrgb,
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
        anisotropy_clamp: 8,
        ..default()
    });
    Some(image)
}

fn material_handle(
    data: &Option<std::sync::Arc<MatData>>,
    caches: &mut Caches,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    double_sided: bool,
) -> Handle<StandardMaterial> {
    let Some(data) = data else {
        return caches
            .materials
            .entry(0)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.55, 0.55, 0.5),
                    perceptual_roughness: 0.95,
                    ..default()
                })
            })
            .clone();
    };
    if let Some(h) = caches.materials.get(&data.id) {
        return h.clone();
    }
    let texture = data.diffuse.as_ref().and_then(|t| {
        caches
            .textures
            .entry(t.id)
            .or_insert_with(|| texture_image(t).map(|i| images.add(i)))
            .clone()
    });
    let (alpha_mode, two_sided) = match data.blend_mode {
        Some(1) => (
            AlphaMode::Mask(f32::from(data.mask_reference.unwrap_or(128)) / 255.0),
            true,
        ),
        Some(2 | 8) => (AlphaMode::Blend, true),
        Some(7) => (AlphaMode::Add, true),
        _ => (AlphaMode::Opaque, false),
    };
    let handle = materials.add(StandardMaterial {
        base_color: if texture.is_some() {
            Color::WHITE
        } else {
            Color::srgb(0.55, 0.58, 0.5)
        },
        base_color_texture: texture,
        perceptual_roughness: 0.95,
        reflectance: 0.05,
        alpha_mode,
        cull_mode: if two_sided || double_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        },
        double_sided: two_sided || double_sided,
        ..default()
    });
    caches.materials.insert(data.id, handle.clone());
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
    .with_inserted_indices(Indices::U32(g.indices.clone()));
    let handle = meshes.add(mesh);
    // Terrain tiles are unique; only shared static geometry is worth caching.
    if !part.terrain {
        caches.meshes.insert(g.id, handle.clone());
    }
    handle
}

fn spawn_pending(
    mut commands: Commands,
    mut stream: ResMut<Stream>,
    mut collision: ResMut<Collision>,
    mut caches: ResMut<Caches>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
        let mesh = mesh_handle(part, &mut caches, &mut meshes);
        let material = material_handle(
            &part.material,
            &mut caches,
            &mut materials,
            &mut images,
            double_sided,
        );
        let mut transform = Transform::from_matrix(part.transform);
        transform.translation *= CM;
        transform.scale *= CM;
        let entity = commands
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), transform))
            .id();
        front.entities.push(entity);
        stream.parts_spawned += 1;
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
    mut player: ResMut<Player>,
) {
    let dt = time.delta_secs().min(0.1);
    if cursor
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
    let speed = if sprint { 650.0 } else { 380.0 };
    let wish = wish.normalize_or_zero() * speed;
    let jump = keys.just_pressed(KeyCode::Space);
    player.ctrl.step(&collision.0, wish, jump, dt);
}

fn update_camera(
    fog_end: Res<FogEnd>,
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
    if let Ok(mut f) = fog.single_mut() {
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
    time: Res<Time>,
) {
    if !smoke.enabled {
        return;
    }
    smoke.frames += 1;
    let loaded_all = stream.ready
        && stream.in_flight.is_empty()
        && stream.pending.is_empty()
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
            commands.write_message(AppExit::Success);
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
