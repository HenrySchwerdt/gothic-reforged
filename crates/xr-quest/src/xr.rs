//! OpenXR session, per-eye swapchains, reference space and controller actions.
#![allow(unsafe_code)]

use anyhow::{Context, Result, anyhow};
use bevy::math::{Quat, Vec2, Vec3};
use bevy::transform::components::Transform;
use openxr as xr;

use crate::graphics::{self, WgpuGraphics};

pub const EYE_COUNT: usize = 2;

/// Asymmetric field of view in radians (OpenXR convention: left/down negative).
#[derive(Clone, Copy, Debug, Default)]
pub struct Fov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HandState {
    pub active: bool,
    pub grip: Transform,
    pub aim: Transform,
    pub trigger: f32,
    pub squeeze: f32,
    pub stick: Vec2,
    pub primary: bool,   // A / X
    pub secondary: bool, // B / Y
    pub stick_click: bool,
    /// Grip angular velocity (scaled axis, rad/s, tracking space) from `xrLocateSpace` velocity, if valid.
    pub angular_velocity: Option<Vec3>,
}

pub struct Eye {
    pub swapchain: xr::Swapchain<xr::Vulkan>,
    pub size: (u32, u32),
    pub textures: Vec<wgpu::Texture>,
    pub views: Vec<wgpu::TextureView>,
    pub format: wgpu::TextureFormat,
    pub acquired: Option<usize>,
}

struct Hand {
    grip_space: xr::Space,
    aim_space: xr::Space,
}

pub struct XrRuntime {
    pub instance: xr::Instance,
    pub system: xr::SystemId,
    pub session: xr::Session<xr::Vulkan>,
    pub waiter: xr::FrameWaiter,
    pub stream: xr::FrameStream<xr::Vulkan>,
    pub stage: xr::Space,
    pub eyes: Vec<Eye>,
    pub running: bool,
    pub exit: bool,
    pub system_name: String,
    pub refresh_rates: Vec<f32>,
    action_set: xr::ActionSet,
    a_grip: xr::Action<xr::Posef>,
    a_aim: xr::Action<xr::Posef>,
    a_haptic: xr::Action<xr::Haptic>,
    a_trigger: xr::Action<f32>,
    a_squeeze: xr::Action<f32>,
    a_stick: xr::Action<xr::Vector2f>,
    a_primary: xr::Action<bool>,
    a_secondary: xr::Action<bool>,
    a_stick_click: xr::Action<bool>,
    hands: [Hand; 2],
    hand_paths: [xr::Path; 2],
    pub fb_refresh: bool,
    pub space_kind: &'static str,
    /// Added to tracked y when the runtime has no floor-level space.
    pub floor_fix: f32,
    pub recenters: u32,
}

#[cfg(target_os = "android")]
pub struct AndroidHandles {
    pub vm: *mut std::ffi::c_void,
    pub activity: *mut std::ffi::c_void,
}
#[cfg(not(target_os = "android"))]
pub struct AndroidHandles;

pub fn init(android: &AndroidHandles, app_name: &str) -> Result<(XrRuntime, WgpuGraphics)> {
    #[cfg(target_os = "android")]
    let platform = unsafe { xr::AndroidPlatformInfo::new(android.vm, android.activity) };
    #[cfg(not(target_os = "android"))]
    let platform = {
        let _ = android;
    };

    let entry = unsafe { xr::Entry::load(&platform) }.context("load OpenXR loader")?;
    let avail = entry
        .enumerate_extensions()
        .context("enumerate extensions")?;
    log::info!(
        "OpenXR runtime exts: vulkan_enable2={} fb_display_refresh_rate={} fb_foveation={} meta_touch_plus={}",
        avail.khr_vulkan_enable2,
        avail.fb_display_refresh_rate,
        avail.fb_foveation,
        avail.meta_touch_controller_plus
    );
    let mut enabled = xr::ExtensionSet::default();
    enabled.khr_vulkan_enable2 = true;
    #[cfg(target_os = "android")]
    {
        enabled.khr_android_create_instance = true;
    }
    if avail.fb_display_refresh_rate {
        enabled.fb_display_refresh_rate = true;
    }
    if avail.ext_local_floor {
        enabled.ext_local_floor = true;
    }
    // Quest 3 / 3S controllers are Touch Plus (`/interaction_profiles/meta/touch_controller_plus`); the runtime also
    // maps the classic touch profile onto them, we suggest both.
    let touch_plus = avail.meta_touch_controller_plus;
    if touch_plus {
        enabled.meta_touch_controller_plus = true;
    }
    let fb_refresh = avail.fb_display_refresh_rate;
    let instance = entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: app_name,
                application_version: 1,
                engine_name: "bevy",
                engine_version: 20,
                api_version: xr::Version::new(1, 0, 34),
            },
            &enabled,
            &[],
            &platform,
        )
        .context("xrCreateInstance")?;
    let props = instance.properties()?;
    log::info!(
        "OpenXR runtime: {} {}",
        props.runtime_name,
        props.runtime_version
    );
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .context("xrGetSystem")?;
    let sys_props = instance.system_properties(system)?;
    log::info!("OpenXR system: {}", sys_props.system_name);

    let (gfx, vk_info) = graphics::init_graphics(&instance, system, app_name)?;
    let (session, waiter, stream) = unsafe {
        instance
            .create_session::<xr::Vulkan>(system, &vk_info.info)
            .context("xrCreateSession")?
    };

    // Swapchain format.
    let formats = session.enumerate_swapchain_formats()?;
    log::info!("swapchain formats: {formats:?}");
    let (vk_fmt, wgpu_fmt) = formats
        .iter()
        .find_map(|f| {
            let w = graphics::vk_format_to_wgpu(*f)?;
            (w == wgpu::TextureFormat::Rgba8UnormSrgb).then_some((*f, w))
        })
        .or_else(|| {
            formats
                .iter()
                .find_map(|f| graphics::vk_format_to_wgpu(*f).map(|w| (*f, w)))
        })
        .ok_or_else(|| anyhow!("no supported swapchain format"))?;

    let view_cfg = instance
        .enumerate_view_configuration_views(system, xr::ViewConfigurationType::PRIMARY_STEREO)?;
    let mut eyes = Vec::new();
    for (i, v) in view_cfg.iter().enumerate().take(EYE_COUNT) {
        let (w, h) = (
            v.recommended_image_rect_width,
            v.recommended_image_rect_height,
        );
        log::info!(
            "eye {i}: recommended {w}x{h} max {}x{} samples {}",
            v.max_image_rect_width,
            v.max_image_rect_height,
            v.recommended_swapchain_sample_count
        );
        let (w, h) = scaled(w, h);
        let sc = session.create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED,
            format: vk_fmt,
            sample_count: 1,
            width: w,
            height: h,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })?;
        let images = sc.enumerate_images()?;
        let mut textures = Vec::new();
        let mut views = Vec::new();
        for img in images {
            let t = graphics::wrap_swapchain_image(&gfx.device, img, wgpu_fmt, w, h, "xr-eye")?;
            let v = t.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu_fmt),
                ..Default::default()
            });
            textures.push(t);
            views.push(v);
        }
        eyes.push(Eye {
            swapchain: sc,
            size: (w, h),
            textures,
            views,
            format: wgpu_fmt,
            acquired: None,
        });
    }

    // LOCAL_FLOOR follows the system recenter (Meta docs, migration guide: local space is redefined on system
    // recenter actions) and puts y = 0 on the floor; STAGE is fixed to the Guardian and never recenters.
    let (stage, space_kind, floor_fix) = match session
        .create_reference_space(xr::ReferenceSpaceType::LOCAL_FLOOR, xr::Posef::IDENTITY)
    {
        Ok(s) => (s, "LOCAL_FLOOR", 0.0),
        Err(e) => {
            crate::alog(&format!(
                "LOCAL_FLOOR unavailable ({e}); falling back to LOCAL + 1.6 m"
            ));
            (
                session
                    .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)?,
                "LOCAL",
                1.6,
            )
        }
    };
    crate::alog(&format!("reference space: {space_kind}"));

    // Actions.
    let hand_paths = [
        instance.string_to_path("/user/hand/left")?,
        instance.string_to_path("/user/hand/right")?,
    ];
    let action_set = instance.create_action_set("gameplay", "Gameplay", 0)?;
    let a_grip = action_set.create_action::<xr::Posef>("grip_pose", "Grip pose", &hand_paths)?;
    let a_aim = action_set.create_action::<xr::Posef>("aim_pose", "Aim pose", &hand_paths)?;
    let a_haptic = action_set.create_action::<xr::Haptic>("haptic", "Haptic", &hand_paths)?;
    let a_trigger = action_set.create_action::<f32>("trigger", "Trigger", &hand_paths)?;
    let a_squeeze = action_set.create_action::<f32>("squeeze", "Squeeze", &hand_paths)?;
    let a_stick =
        action_set.create_action::<xr::Vector2f>("thumbstick", "Thumbstick", &hand_paths)?;
    let a_primary = action_set.create_action::<bool>("primary", "A/X", &hand_paths)?;
    let a_secondary = action_set.create_action::<bool>("secondary", "B/Y", &hand_paths)?;
    let a_stick_click =
        action_set.create_action::<bool>("stick_click", "Thumbstick click", &hand_paths)?;

    let p = |s: &str| instance.string_to_path(s);
    let mut b: Vec<xr::Binding> = Vec::new();
    for side in ["left", "right"] {
        b.push(xr::Binding::new(
            &a_grip,
            p(&format!("/user/hand/{side}/input/grip/pose"))?,
        ));
        b.push(xr::Binding::new(
            &a_aim,
            p(&format!("/user/hand/{side}/input/aim/pose"))?,
        ));
        b.push(xr::Binding::new(
            &a_haptic,
            p(&format!("/user/hand/{side}/output/haptic"))?,
        ));
        b.push(xr::Binding::new(
            &a_trigger,
            p(&format!("/user/hand/{side}/input/trigger/value"))?,
        ));
        b.push(xr::Binding::new(
            &a_squeeze,
            p(&format!("/user/hand/{side}/input/squeeze/value"))?,
        ));
        b.push(xr::Binding::new(
            &a_stick,
            p(&format!("/user/hand/{side}/input/thumbstick"))?,
        ));
        b.push(xr::Binding::new(
            &a_stick_click,
            p(&format!("/user/hand/{side}/input/thumbstick/click"))?,
        ));
        let (pr, se) = if side == "left" {
            ("x", "y")
        } else {
            ("a", "b")
        };
        b.push(xr::Binding::new(
            &a_primary,
            p(&format!("/user/hand/{side}/input/{pr}/click"))?,
        ));
        b.push(xr::Binding::new(
            &a_secondary,
            p(&format!("/user/hand/{side}/input/{se}/click"))?,
        ));
    }
    instance.suggest_interaction_profile_bindings(
        p("/interaction_profiles/oculus/touch_controller")?,
        &b,
    )?;
    if touch_plus {
        match instance.suggest_interaction_profile_bindings(
            p("/interaction_profiles/meta/touch_controller_plus")?,
            &b,
        ) {
            Ok(()) => crate::alog("bindings suggested for meta/touch_controller_plus"),
            Err(e) => crate::alog(&format!("touch_controller_plus bindings rejected: {e}")),
        }
    }
    session.attach_action_sets(&[&action_set])?;

    let mk = |action: &xr::Action<xr::Posef>, path| {
        action.create_space(&session, path, xr::Posef::IDENTITY)
    };
    let hands = [
        Hand {
            grip_space: mk(&a_grip, hand_paths[0])?,
            aim_space: mk(&a_aim, hand_paths[0])?,
        },
        Hand {
            grip_space: mk(&a_grip, hand_paths[1])?,
            aim_space: mk(&a_aim, hand_paths[1])?,
        },
    ];

    let mut refresh_rates = Vec::new();
    if fb_refresh {
        if let Ok(r) = session.enumerate_display_refresh_rates() {
            refresh_rates = r;
        }
        log::info!("display refresh rates: {refresh_rates:?}");
    }

    Ok((
        XrRuntime {
            instance,
            system,
            session,
            waiter,
            stream,
            stage,
            eyes,
            running: false,
            exit: false,
            system_name: sys_props.system_name,
            refresh_rates,
            action_set,
            a_grip,
            a_aim,
            a_haptic,
            a_trigger,
            a_squeeze,
            a_stick,
            a_primary,
            a_secondary,
            a_stick_click,
            hands,
            hand_paths,
            fb_refresh,
            space_kind,
            floor_fix,
            recenters: 0,
        },
        gfx,
    ))
}

/// Render-resolution scale from `QUEST_RES_SCALE` (e.g. 0.8), default 1.0.
fn scaled(w: u32, h: u32) -> (u32, u32) {
    let s: f32 = std::env::var("QUEST_RES_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(read_prop_scale());
    (
        ((w as f32 * s) as u32).max(16) & !1,
        ((h as f32 * s) as u32).max(16) & !1,
    )
}

fn read_prop_scale() -> f32 {
    #[cfg(target_os = "android")]
    {
        // `adb shell setprop debug.gr.res_scale 0.8` (readable by the app).
        if let Some(v) =
            android_system_properties::AndroidSystemProperties::new().get("debug.gr.res_scale")
        {
            if let Ok(f) = v.parse::<f32>() {
                return f;
            }
        }
    }
    1.0
}

fn to_transform(p: xr::Posef) -> Transform {
    Transform {
        translation: Vec3::new(p.position.x, p.position.y, p.position.z),
        rotation: Quat::from_xyzw(
            p.orientation.x,
            p.orientation.y,
            p.orientation.z,
            p.orientation.w,
        ),
        scale: Vec3::ONE,
    }
}

/// Pending haptic pulse per hand (amplitude * 1000), consumed by `sync_input`. Use [`haptic_pulse`].
pub static HAPTIC: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
static SQUEEZE_LOGGED: [std::sync::atomic::AtomicBool; 2] = [
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
];

/// Short vibration on a hand (0 = left, 1 = right), amplitude 0..1 (`/user/hand/*/output/haptic`).
pub fn haptic_pulse(hand: usize, amplitude: f32) {
    haptic_pulse_ex(hand, amplitude, 0.04, 0.0);
}

/// Vibration with duration (s) and frequency (Hz, 0 = runtime default). Packed: amp permille | duration ms << 10 | Hz << 26.
pub fn haptic_pulse_ex(hand: usize, amplitude: f32, duration: f32, frequency: f32) {
    let a = (amplitude.clamp(0.0, 1.0) * 1000.0) as u32;
    let d = ((duration.clamp(0.0, 2.0) * 1000.0) as u32).clamp(1, 2000);
    let f = (frequency.clamp(0.0, 1000.0) as u32).min(1000);
    let v = a as u64 | (d as u64) << 10 | (f as u64) << 26;
    HAPTIC[hand.min(1)].store(v, std::sync::atomic::Ordering::Relaxed);
}

pub struct FrameViews {
    pub display_time: xr::Time,
    pub poses: [Transform; EYE_COUNT],
    pub fovs: [Fov; EYE_COUNT],
    pub head: Transform,
}

impl XrRuntime {
    /// Handles runtime events. Returns true if the loop should exit.
    pub fn poll_events(&mut self) -> Result<()> {
        let mut buf = xr::EventDataBuffer::new();
        while let Some(ev) = self.instance.poll_event(&mut buf)? {
            use xr::Event::*;
            match ev {
                SessionStateChanged(e) => {
                    crate::alog(&format!("session state -> {:?}", e.state()));
                    match e.state() {
                        xr::SessionState::READY => {
                            self.session
                                .begin(xr::ViewConfigurationType::PRIMARY_STEREO)?;
                            self.running = true;
                            if self.fb_refresh {
                                self.request_refresh_from_env();
                            }
                        }
                        xr::SessionState::STOPPING => {
                            self.session.end()?;
                            self.running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            self.exit = true;
                        }
                        _ => {}
                    }
                }
                InstanceLossPending(_) => {
                    crate::alog("event: InstanceLossPending");
                    self.exit = true;
                }
                ReferenceSpaceChangePending(e) => {
                    crate::alog(&format!(
                        "event: ReferenceSpaceChangePending type={:?}",
                        e.reference_space_type()
                    ));
                    self.recenters += 1;
                }
                other => crate::alog(&format!("event: {}", event_name(&other))),
            }
        }
        Ok(())
    }

    fn request_refresh_from_env(&self) {
        let want: Option<f32> = std::env::var("QUEST_REFRESH")
            .ok()
            .and_then(|v| v.parse().ok())
            .or_else(|| prop_f32("debug.gr.refresh"))
            .or(Some(72.0));
        if let Some(hz) = want {
            match self.session.request_display_refresh_rate(hz) {
                Ok(_) => log::info!("requested refresh rate {hz}"),
                Err(e) => log::warn!("request refresh {hz}: {e}"),
            }
        }
        if let Ok(cur) = self.session.get_display_refresh_rate() {
            log::info!("current display refresh rate: {cur}");
        }
    }

    pub fn locate_views(&self, display_time: xr::Time) -> Result<FrameViews> {
        let (flags, views) = self.session.locate_views(
            xr::ViewConfigurationType::PRIMARY_STEREO,
            display_time,
            &self.stage,
        )?;
        if !flags
            .contains(xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID)
        {
            return Err(anyhow!("view not tracked (headset not worn?)"));
        }
        let mut poses = [Transform::IDENTITY; EYE_COUNT];
        let mut fovs = [Fov::default(); EYE_COUNT];
        for (i, v) in views.iter().enumerate().take(EYE_COUNT) {
            poses[i] = to_transform(v.pose);
            poses[i].translation.y += self.floor_fix;
            fovs[i] = Fov {
                left: v.fov.angle_left,
                right: v.fov.angle_right,
                up: v.fov.angle_up,
                down: v.fov.angle_down,
            };
        }
        let head = Transform::from_translation((poses[0].translation + poses[1].translation) * 0.5)
            .with_rotation(poses[0].rotation);
        Ok(FrameViews {
            display_time,
            poses,
            fovs,
            head,
        })
    }

    pub fn sync_input(&self, display_time: xr::Time) -> Result<[HandState; 2]> {
        self.session
            .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])?;
        let mut out = [HandState::default(); 2];
        for i in 0..2 {
            let hp = self.hand_paths[i];
            let h = &mut out[i];
            let (gl, gv) = self.hands[i].grip_space.relate(&self.stage, display_time)?;
            if gv
                .velocity_flags
                .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
            {
                let w = gv.angular_velocity;
                h.angular_velocity = Some(Vec3::new(w.x, w.y, w.z));
            }
            let al = self.hands[i].aim_space.locate(&self.stage, display_time)?;
            let valid = gl.location_flags.contains(
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
            );
            h.active = valid;
            if valid {
                h.grip = to_transform(gl.pose);
                h.grip.translation.y += self.floor_fix;
            }
            if al
                .location_flags
                .contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
            {
                h.aim = to_transform(al.pose);
                h.aim.translation.y += self.floor_fix;
            }
            h.trigger = self.a_trigger.state(&self.session, hp)?.current_state;
            h.squeeze = self.a_squeeze.state(&self.session, hp)?.current_state;
            let packed = HAPTIC[i].swap(0, std::sync::atomic::Ordering::Relaxed);
            let amp = (packed & 0x3ff) as u32;
            if amp > 0 {
                let ms = ((packed >> 10) & 0xffff) as i64;
                let hz = ((packed >> 26) & 0x3ff) as f32;
                let v = xr::HapticVibration::new()
                    .amplitude(amp as f32 / 1000.0)
                    .frequency(if hz > 0.0 {
                        hz
                    } else {
                        xr::FREQUENCY_UNSPECIFIED
                    })
                    .duration(xr::Duration::from_nanos(ms * 1_000_000));
                let _ = self.a_haptic.apply_feedback(&self.session, hp, &v);
            }
            if (h.squeeze > 0.5)
                != SQUEEZE_LOGGED[i].swap(h.squeeze > 0.5, std::sync::atomic::Ordering::Relaxed)
            {
                crate::alog(&format!(
                    "grip {} {} (squeeze {:.2}, trigger {:.2})",
                    if i == 0 { "left" } else { "right" },
                    if h.squeeze > 0.5 {
                        "pressed"
                    } else {
                        "released"
                    },
                    h.squeeze,
                    h.trigger
                ));
            }
            let s = self.a_stick.state(&self.session, hp)?.current_state;
            h.stick = Vec2::new(s.x, s.y);
            h.primary = self.a_primary.state(&self.session, hp)?.current_state;
            h.secondary = self.a_secondary.state(&self.session, hp)?.current_state;
            h.stick_click = self.a_stick_click.state(&self.session, hp)?.current_state;
        }
        let _ = (&self.a_grip, &self.a_aim);
        Ok(out)
    }

    /// Acquires and waits for the next image of each eye swapchain.
    pub fn acquire(&mut self) -> Result<()> {
        for eye in &mut self.eyes {
            let idx = eye.swapchain.acquire_image()? as usize;
            eye.swapchain.wait_image(xr::Duration::INFINITE)?;
            eye.acquired = Some(idx);
        }
        Ok(())
    }

    pub fn release_and_submit(&mut self, fv: &FrameViews) -> Result<()> {
        for eye in &mut self.eyes {
            eye.swapchain.release_image()?;
            eye.acquired = None;
        }
        let views: Vec<_> = (0..EYE_COUNT)
            .map(|i| {
                let mut t = fv.poses[i];
                t.translation.y -= self.floor_fix;
                let f = fv.fovs[i];
                xr::CompositionLayerProjectionView::new()
                    .pose(xr::Posef {
                        orientation: xr::Quaternionf {
                            x: t.rotation.x,
                            y: t.rotation.y,
                            z: t.rotation.z,
                            w: t.rotation.w,
                        },
                        position: xr::Vector3f {
                            x: t.translation.x,
                            y: t.translation.y,
                            z: t.translation.z,
                        },
                    })
                    .fov(xr::Fovf {
                        angle_left: f.left,
                        angle_right: f.right,
                        angle_up: f.up,
                        angle_down: f.down,
                    })
                    .sub_image(
                        xr::SwapchainSubImage::new()
                            .swapchain(&self.eyes[i].swapchain)
                            .image_array_index(0)
                            .image_rect(xr::Rect2Di {
                                offset: xr::Offset2Di { x: 0, y: 0 },
                                extent: xr::Extent2Di {
                                    width: self.eyes[i].size.0 as i32,
                                    height: self.eyes[i].size.1 as i32,
                                },
                            }),
                    )
            })
            .collect();
        let layer = xr::CompositionLayerProjection::new()
            .space(&self.stage)
            .views(&views);
        self.stream
            .end(fv.display_time, xr::EnvironmentBlendMode::OPAQUE, &[&layer])?;
        Ok(())
    }

    pub fn end_empty(&mut self, t: xr::Time) -> Result<()> {
        self.stream.end(t, xr::EnvironmentBlendMode::OPAQUE, &[])?;
        Ok(())
    }
}

fn prop_f32(_name: &str) -> Option<f32> {
    #[cfg(target_os = "android")]
    {
        return android_system_properties::AndroidSystemProperties::new()
            .get(_name)
            .and_then(|v| v.parse().ok());
    }
    #[allow(unreachable_code)]
    None
}

fn event_name(e: &xr::Event) -> &'static str {
    use xr::Event::*;
    match e {
        EventsLost(_) => "EventsLost",
        InteractionProfileChanged(_) => "InteractionProfileChanged",
        ReferenceSpaceChangePending(_) => "ReferenceSpaceChangePending",
        VisibilityMaskChangedKHR(_) => "VisibilityMaskChanged",
        _ => "other",
    }
}
