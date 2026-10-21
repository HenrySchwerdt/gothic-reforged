//! The gameplay camera solver: `gCCameraAI_PS::OnProcess` (GameDll:0x100215c0) without engine
//! plumbing. Deterministic, Bevy-free, centimetres, Y up, camera looks along its local -Z.
use crate::config::CameraConfig;
use crate::dialog::{self, DialogActor, Rng, ShotChoice};
use crate::math::{ease, lerp2, relax, relax_tau, relax_vec3};
use crate::preset::{CameraMode, CameraPreset, PresetKey, PresetTable};
use crate::world::CameraWorld;
use glam::{Mat3, Quat, Vec2, Vec3};

/// Pitch limit of the orbit, degrees.
pub const MAX_ELEVATION_DEG: f32 = 89.0;
/// Distance the camera keeps from walls (the near plane radius).
pub const WALL_MARGIN: f32 = 25.0;
/// Shortest boom the collision may leave.
pub const MIN_BOOM: f32 = 30.0;
/// Overshoot past the minimum zoom that switches to first person.
pub const FIRST_PERSON_OVERSHOOT: f32 = 200.0;
/// Zoom distance per wheel notch.
pub const ZOOM_STEP: f32 = 50.0;
/// First-person wall push-out (half of the 35 unit box).
pub const FIRST_PERSON_WALL: f32 = 17.5;
/// Seconds the distance filter stays fast after a teleport.
pub const TELEPORT_FAST_SECONDS: f32 = 2.0;
/// Look-back mode: seconds before a deferred mode request is honoured.
pub const LOOK_BACK_DEFER: f32 = 2.0;

/// What the camera needs to know about the hero this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HeroView {
    /// World position of the entity origin (feet).
    pub position: Vec3,
    /// Height of the local node box (head = position + height up).
    pub box_height: f32,
    /// Head position override (species head bone); defaults to `position + box_height`.
    pub head: Option<Vec3>,
    /// The hero is moving (not standing).
    pub moving: bool,
    /// Horizontal speed, cm/s.
    pub speed: f32,
    /// Combat mode flag (selects the first-person fight preset).
    pub in_combat: bool,
    /// A ranged weapon is drawn.
    pub ranged: bool,
    /// Hero is dead or down (first person is refused, the body stays visible).
    pub down: bool,
    /// Movement mode keeps the body visible in first person (Jump, Swim, Fall: modes 5, 6, 0xd).
    pub keep_body_visible: bool,
}

impl HeroView {
    fn head_pos(&self) -> Vec3 {
        self.head
            .unwrap_or(self.position + Vec3::Y * self.box_height)
    }
}

/// Per-frame input.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraInput {
    /// Frame time, seconds.
    pub dt: f32,
    pub hero: HeroView,
    /// Mouse movement in counts this frame (x right, y down).
    pub look: Vec2,
    /// Wheel notches this frame (positive zooms in).
    pub zoom: f32,
    /// Look-back key held.
    pub look_back: bool,
    /// First-person toggle pressed this frame.
    pub toggle_first_person: bool,
    /// A GUI page is open (blocks zoom).
    pub gui_open: bool,
    /// Invert vertical mouse look.
    pub invert_y: bool,
    /// Current positions of the dialog partners while a dialog camera runs.
    pub dialog: Option<(DialogActor, DialogActor)>,
}

/// Result of a camera update, ready for the render camera and the other subsystems.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraOutput {
    pub position: Vec3,
    /// Camera orientation (forward is -Z).
    pub rotation: Quat,
    /// Vertical field of view, degrees.
    pub fov_deg: f32,
    pub z_near: f32,
    /// Alpha of the hero body (fades out as the camera comes close).
    pub hero_alpha: f32,
    /// The hero body (and weapon slots) should be hidden: first person.
    pub hide_hero: bool,
    /// Audio listener position.
    pub listener: Vec3,
    /// Audio speaker point while a dialog runs.
    pub speaker_point: Option<Vec3>,
    /// `gCFocus_PS` mode: 1 in first person, 2 otherwise.
    pub focus_mode: u8,
    pub mode: CameraMode,
    /// The weapons and held items stay visible although the body fades or hides (first person,
    /// special bow mode): `EnableItemRendering(true)`.
    pub items_visible: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct Tremor {
    amp: Vec3,
    intensity: f32,
    duration: f32,
}

#[derive(Debug, Clone)]
struct DialogState {
    listener: DialogActor,
    speaker: DialogActor,
    preset: PresetKey,
    right: bool,
    timer: f32,
    last_speaker: u64,
    look_at: Vec3,
    saved_zoom: f32,
    saved_mode: CameraMode,
}

/// Camera state. Create with [`CameraState::new`], place with [`CameraState::reset`], call
/// [`CameraState::update`] once per frame.
#[derive(Debug, Clone)]
pub struct CameraState {
    pub config: CameraConfig,
    pub presets: PresetTable,
    mode: CameraMode,
    previous_mode: CameraMode,
    preset: PresetKey,
    /// Wished (zoomed) distance: `+0x1f4`.
    zoom_distance: f32,
    /// Filtered boom length: `+0x1f8`.
    distance: f32,
    /// Orbit azimuth (camera around pivot) and elevation above the horizon, radians.
    yaw: f32,
    elevation: f32,
    delta_yaw: f32,
    delta_elev: f32,
    look_back_yaw: f32,
    pivot: Vec3,
    cam_pos: Vec3,
    fov: f32,
    rot_offset: Vec2,
    rot_timer: f32,
    overshoot: f32,
    first_person_by_zoom: bool,
    look_back_active: bool,
    look_back_timer: f32,
    pending_mode: Option<CameraMode>,
    teleport_timer: f32,
    enabled: bool,
    initialised: bool,
    dialog: Option<DialogState>,
    dialog_roles_changes: u32,
    dialog_right: bool,
    tremor: Tremor,
    rng: Rng,
    last_hero: HeroView,
    time: f32,
    /// `EnableSpecialBowMode` (`+0x4d5`): the aim mode of the bow fades the hero out.
    special_bow: bool,
    /// Seconds in the special bow mode, at most [`crate::aim::SPECIAL_BOW_FADE_SECS`] (`+0x4d8`).
    special_bow_time: f32,
    /// `EnableTempFirstPerson` (`+0x4d4`).
    temp_first_person: bool,
}

fn dir_from_angles(yaw: f32, elev: f32) -> Vec3 {
    Vec3::new(yaw.sin() * elev.cos(), elev.sin(), yaw.cos() * elev.cos())
}

/// Rotation looking from `from` to `to` (forward -Z, up +Y).
pub fn look_rotation(from: Vec3, to: Vec3) -> Quat {
    let f = (to - from).normalize_or(Vec3::NEG_Z);
    let mut r = f.cross(Vec3::Y);
    if r.length_squared() < 1e-8 {
        r = Vec3::X;
    }
    let r = r.normalize();
    let u = r.cross(f);
    Quat::from_mat3(&Mat3::from_cols(r, u, -f))
}

impl CameraState {
    /// New camera with the given tunables and presets.
    pub fn new(config: CameraConfig, presets: PresetTable) -> Self {
        let n = presets.get(PresetKey::Normal).clone();
        Self {
            config,
            presets,
            mode: CameraMode::Normal,
            previous_mode: CameraMode::Normal,
            preset: PresetKey::Normal,
            zoom_distance: n.best_range,
            distance: n.best_range,
            yaw: 0.0,
            elevation: n.best_elev.to_radians(),
            delta_yaw: 0.0,
            delta_elev: 0.0,
            look_back_yaw: 0.0,
            pivot: Vec3::ZERO,
            cam_pos: Vec3::ZERO,
            fov: n.fov[0],
            rot_offset: Vec2::ZERO,
            rot_timer: 0.0,
            overshoot: 0.0,
            first_person_by_zoom: false,
            look_back_active: false,
            look_back_timer: 0.0,
            pending_mode: None,
            teleport_timer: 0.0,
            enabled: true,
            initialised: false,
            dialog: None,
            dialog_roles_changes: 0,
            dialog_right: false,
            tremor: Tremor::default(),
            rng: Rng::new(0x9E37_79B9),
            last_hero: HeroView::default(),
            time: 0.0,
            special_bow: false,
            special_bow_time: 0.0,
            temp_first_person: false,
        }
    }

    /// `PSCamera::EnableSpecialBowMode` (GameDll:0x1002cc20): the script's ranged states switch
    /// it on; the hero's body fades out over half a second so the arrow flies from the crosshair.
    pub fn enable_special_bow_mode(&mut self, on: bool) {
        self.special_bow = on;
        self.special_bow_time = 0.0;
    }

    /// `IsInSpecialBowMode`.
    pub fn in_special_bow_mode(&self) -> bool {
        self.special_bow
    }

    /// `PSCamera::EnableTempFirstPerson` (GameDll:0x1002cc00): `IsInFirstPerson` reports true
    /// while it is set and `SetMode` leaves the mode alone.
    pub fn enable_temp_first_person(&mut self, on: bool) {
        self.temp_first_person = on;
    }

    /// `IsInTempFirstPerson`.
    pub fn in_temp_first_person(&self) -> bool {
        self.temp_first_person
    }

    /// Seed the shot-choice and tremor generators (tests, replays).
    pub fn seed(&mut self, seed: u32) {
        self.rng = Rng::new(seed);
    }

    /// Current mode.
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// Whether the dialog camera is running.
    pub fn dialog_active(&self) -> bool {
        self.dialog.is_some()
    }

    /// The preset in use (a dialog shot while a dialog camera runs).
    pub fn active_preset_key(&self) -> PresetKey {
        self.dialog.as_ref().map_or(self.preset, |d| d.preset)
    }

    /// Current orbit angles in degrees: (azimuth, elevation).
    pub fn orbit_degrees(&self) -> (f32, f32) {
        (self.yaw.to_degrees(), self.elevation.to_degrees())
    }

    /// Filtered boom length and zoomed (wished) boom length.
    pub fn distances(&self) -> (f32, f32) {
        (self.distance, self.zoom_distance)
    }

    /// Enable or disable the gameplay camera (`+0x203`).
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Place the camera behind the hero looking along `heading` (a horizontal world direction),
    /// at the mode's `BestAzi`/`BestElev`/`BestRange`, and snap every filter.
    pub fn reset(&mut self, hero: &HeroView, heading: Vec3) {
        let p = self.presets.get(self.preset).clone();
        let h = Vec3::new(heading.x, 0.0, heading.z).normalize_or(Vec3::NEG_Z);
        // The camera sits opposite the heading; azimuth is measured from there.
        self.yaw = (-h.x).atan2(-h.z) + p.best_azi.to_radians();
        self.elevation = p.best_elev.to_radians();
        self.zoom_distance = p.best_range;
        self.distance = p.best_range;
        self.delta_yaw = 0.0;
        self.delta_elev = 0.0;
        self.pivot = hero.head_pos();
        self.cam_pos = self.pivot + dir_from_angles(self.yaw, self.elevation) * self.distance;
        self.fov = lerp2(p.fov, ease(p.range_ratio(self.zoom_distance)));
        self.rot_offset = Vec2::ZERO;
        self.rot_timer = 0.0;
        self.initialised = true;
        self.last_hero = *hero;
    }

    /// The hero was moved a long way in one frame (`EntityMovedBigDistance`, GameDll:0x10025160):
    /// snap pivot and camera, run the fast distance filter for two seconds.
    pub fn notify_teleport(&mut self, hero: &HeroView) {
        self.pivot = hero.head_pos();
        self.cam_pos = self.pivot + dir_from_angles(self.yaw, self.elevation) * self.distance;
        self.teleport_timer = TELEPORT_FAST_SECONDS;
    }

    /// `PSCamera.SetMode(mode, ?, keepDistance)` (GameDll:0x1002c4b0).
    ///
    /// Ignored while the camera is disabled. During look-back the request is deferred until the
    /// look-back timer passed two seconds. In first person (entered by zoom) the requests of the
    /// "keep first person" group only update the mode to return to.
    pub fn set_mode(&mut self, mode: CameraMode, keep_distance: bool) {
        if !self.enabled {
            return;
        }
        if self.look_back_active && mode != CameraMode::LookBack {
            self.pending_mode = Some(mode);
            return;
        }
        if self.first_person_by_zoom && mode != CameraMode::FirstPerson {
            let keep_group = matches!(
                mode,
                CameraMode::Ranged
                    | CameraMode::Dialog
                    | CameraMode::Inventory
                    | CameraMode::Swim
                    | CameraMode::Death
            );
            if keep_group && self.config.keep_first_person {
                self.previous_mode = mode;
                return;
            }
            self.first_person_by_zoom = false;
            self.overshoot = 0.0;
        }
        self.apply_mode(mode, keep_distance);
    }

    fn apply_mode(&mut self, mode: CameraMode, keep_distance: bool) {
        if mode == CameraMode::LookBack {
            // Look-back reuses the current preset.
            self.mode = mode;
            return;
        }
        let key = PresetKey::for_mode(mode, self.last_hero.in_combat, self.last_hero.ranged);
        let preset = self.presets.get(key);
        if !keep_distance {
            self.zoom_distance = preset.best_range.min(self.config.max_dist_to_player);
        }
        self.preset = key;
        self.mode = mode;
        self.rot_timer = 0.0;
    }

    /// Start the dialog camera (`SetDialogRoles` + `StartDialogCamera`, GameDll:0x1002ce40/cfc0).
    ///
    /// A repeat call with the same pair inside a shoulder or face shot keeps the shot; otherwise
    /// it re-cuts. Returns false when the partners share a position (the original warns and
    /// aborts).
    pub fn set_dialog_roles(
        &mut self,
        listener: DialogActor,
        speaker: DialogActor,
        world: &dyn CameraWorld,
    ) -> bool {
        if listener.origin == speaker.origin {
            return false;
        }
        self.dialog_roles_changes += 1;
        if let Some(d) = &self.dialog {
            let same_pair = d.listener.id == listener.id && d.speaker.id == speaker.id;
            let short_shot = matches!(
                d.preset,
                PresetKey::DialogFaceL
                    | PresetKey::DialogFaceR
                    | PresetKey::DialogShoulder1L
                    | PresetKey::DialogShoulder1R
            );
            if same_pair && short_shot && d.timer < dialog::LERP_SECONDS {
                return true;
            }
        }
        let first_time = self.dialog.is_none();
        let (saved_zoom, saved_mode, last_speaker, previous, right) = match &self.dialog {
            Some(d) => (
                d.saved_zoom,
                d.saved_mode,
                d.last_speaker,
                Some(d.preset),
                d.right,
            ),
            None => (self.zoom_distance, self.mode, 0, None, self.dialog_right),
        };
        // The side alternates between cuts; a new speaker flips it again.
        let mut right = !right;
        if last_speaker != speaker.id && last_speaker != 0 {
            right = !right;
        }
        let key = dialog::find_preset(
            &self.presets,
            ShotChoice {
                first_time,
                right,
                previous,
            },
            &listener,
            &speaker,
            world,
            &mut self.rng,
        )
        .unwrap_or(if right {
            PresetKey::DialogShoulder2R
        } else {
            PresetKey::DialogShoulder2L
        });
        let pose = dialog::shot_pose(&self.presets, key, &listener, &speaker, 0.0);
        self.dialog = Some(DialogState {
            listener,
            speaker,
            preset: key,
            right,
            timer: 0.0,
            last_speaker: speaker.id,
            look_at: pose.look_at,
            saved_zoom,
            saved_mode,
        });
        self.dialog_right = right;
        self.rot_timer = 0.0;
        true
    }

    /// Stop the dialog camera (`StopDialogCamera`, GameDll:0x1002dc10): back to the mode and zoom
    /// before the dialog; the filters glide the camera home.
    pub fn stop_dialog(&mut self) {
        if let Some(d) = self.dialog.take() {
            self.zoom_distance = d.saved_zoom;
            self.mode = d.saved_mode;
            self.dialog_right = d.right;
            self.dialog_roles_changes = 0;
        }
    }

    /// Earthquake shake (`AddTremor`, GameDll:0x10024f40): amplitude per axis, attenuated with the
    /// squared distance of the origin to the camera over `radius`.
    pub fn add_tremor(&mut self, origin: Vec3, radius: f32, amp: Vec3, duration: f32) {
        let falloff = if radius > 0.0 {
            (1.0 - (origin - self.cam_pos).length_squared() / (radius * radius)).max(0.0)
        } else {
            0.0
        };
        if falloff <= 0.0 || duration <= 0.0 {
            return;
        }
        let a = amp * falloff;
        self.tremor.amp = self.tremor.amp.max(a);
        self.tremor.intensity = if self.tremor.intensity > 0.0 {
            (self.tremor.intensity + 1.0) * 0.5
        } else {
            1.0
        };
        self.tremor.duration = duration;
    }

    fn apply_tremor(&mut self, dt: f32, pos: Vec3) -> Vec3 {
        if self.tremor.intensity <= 0.0 {
            return pos;
        }
        let r = Vec3::new(
            self.rng.next_signed(),
            self.rng.next_signed(),
            self.rng.next_signed(),
        );
        let out = pos + r * self.tremor.amp * self.tremor.intensity;
        self.tremor.intensity =
            (self.tremor.intensity - dt / self.tremor.duration.max(1e-3)).max(0.0);
        if self.tremor.intensity == 0.0 {
            self.tremor.amp = Vec3::ZERO;
        }
        out
    }

    fn preset(&self) -> CameraPreset {
        self.presets.get(self.active_preset_key()).clone()
    }

    /// Wished boom length (`GetCurrentWishedDistance`, GameDll:0x10026ae0).
    pub fn wished_distance(&self, moving: bool, preset: &CameraPreset) -> f32 {
        let d = if moving {
            self.zoom_distance.max(preset.min_range_on_move)
        } else {
            self.zoom_distance
        };
        d.min(self.config.max_dist_to_player)
    }

    /// Boom length filter (`UpdateDistance`, GameDll:0x10026860): closer snaps in, farther relaxes.
    fn update_distance(&mut self, wished: f32, dt: f32) {
        let wished = wished.min(self.config.max_dist_to_player);
        if wished < self.distance {
            self.distance = wished;
        } else {
            let factor = if self.teleport_timer > 0.0 {
                20.0
            } else {
                self.config.relax_dist
            };
            self.distance =
                relax(self.distance, wished, dt, factor).min(self.config.max_dist_to_player);
        }
        self.teleport_timer = (self.teleport_timer - dt).max(0.0);
    }

    fn process_input(&mut self, input: &CameraInput, dt: f32) {
        let cfg = &self.config;
        let preset = self.preset();
        let gain = cfg.radians_per_count * preset.velo_trans * 2.0;
        let yaw_target = input.look.x * gain * cfg.sensitivity[0];
        let sign = if input.invert_y { -1.0 } else { 1.0 };
        let elev_target = input.look.y * gain * cfg.sensitivity[1] * sign;
        // UpdateDeltaRotX/Y: relax factor 8 / 15 times the mouse smoothing.
        self.delta_yaw = relax_tau(
            self.delta_yaw,
            yaw_target,
            dt,
            cfg.smoothing[0] * cfg.relax_x_axis * crate::math::RELAX_UNIT,
        );
        self.delta_elev = relax_tau(
            self.delta_elev,
            elev_target,
            dt,
            cfg.smoothing[1] * cfg.relax_y_axis * crate::math::RELAX_UNIT,
        );
        // Screen-right look turns the orbit the opposite way; downward look raises the camera.
        self.yaw -= self.delta_yaw;
        let lim = MAX_ELEVATION_DEG.to_radians();
        self.elevation = (self.elevation + self.delta_elev).clamp(-lim, lim);

        self.process_zoom(input);

        if input.toggle_first_person && !self.look_back_active && !input.hero.down {
            if self.mode == CameraMode::FirstPerson {
                let back = self.previous_mode;
                self.first_person_by_zoom = false;
                self.overshoot = 0.0;
                self.apply_mode(back, false);
            } else {
                self.previous_mode = self.mode;
                self.apply_mode(CameraMode::FirstPerson, true);
            }
        }

        // Look back while the key is held; release returns to the previous mode.
        if input.look_back && !self.look_back_active && self.mode != CameraMode::FirstPerson {
            self.look_back_active = true;
            self.look_back_timer = 0.0;
            self.previous_mode = self.mode;
            self.mode = CameraMode::LookBack;
        } else if !input.look_back && self.look_back_active {
            self.look_back_active = false;
            let back = self.pending_mode.take().unwrap_or(self.previous_mode);
            self.apply_mode(back, true);
        }
        if self.look_back_active {
            self.look_back_timer += dt;
        }
        let want = if self.look_back_active {
            std::f32::consts::PI
        } else {
            0.0
        };
        self.look_back_yaw = relax(self.look_back_yaw, want, dt, self.config.relax_x_axis);
    }

    /// Wheel zoom (`OnAction` ids 0x11/0x12, GameDll:0x1002b6c0).
    fn process_zoom(&mut self, input: &CameraInput) {
        if input.gui_open || input.zoom == 0.0 || self.dialog.is_some() {
            return;
        }
        let preset = self.preset();
        let max = preset.max_range.min(self.config.max_dist_to_player);
        if self.mode == CameraMode::FirstPerson {
            if self.first_person_by_zoom && input.zoom < 0.0 {
                self.overshoot += input.zoom * ZOOM_STEP;
                if self.overshoot <= 0.0 {
                    self.overshoot = 0.0;
                    self.first_person_by_zoom = false;
                    let back = self.previous_mode;
                    self.apply_mode(back, true);
                    self.zoom_distance = preset.min_range;
                }
            }
            return;
        }
        let want = self.zoom_distance - input.zoom * ZOOM_STEP;
        if want < preset.min_range {
            self.zoom_distance = preset.min_range;
            if input.zoom > 0.0 {
                self.overshoot += preset.min_range - want;
                if self.overshoot > FIRST_PERSON_OVERSHOOT && !input.hero.down {
                    self.previous_mode = self.mode;
                    self.first_person_by_zoom = true;
                    self.apply_mode(CameraMode::FirstPerson, true);
                }
            }
        } else {
            self.zoom_distance = want.min(max);
            self.overshoot = 0.0;
        }
    }

    /// Advance the camera one frame.
    pub fn update(&mut self, input: &CameraInput, world: &dyn CameraWorld) -> CameraOutput {
        let dt = input.dt.clamp(0.0, 0.1);
        self.time += dt;
        let hero = input.hero;
        self.last_hero = hero;
        if !self.initialised {
            self.reset(&hero, Vec3::NEG_Z);
        }
        let out = if self.dialog.is_some() {
            self.update_dialog(input, dt, world)
        } else {
            self.process_input(input, dt);
            self.update_player(&hero, dt, world)
        };
        let mut out = out;
        out.position = self.apply_tremor(dt, out.position);
        out
    }

    fn finish_fov(&mut self, preset: &CameraPreset, dt: f32, fixed_min: bool) -> f32 {
        let target = if fixed_min {
            preset.fov[0]
        } else {
            lerp2(preset.fov, ease(preset.range_ratio(self.zoom_distance)))
        };
        let target = target.clamp(10.0, 180.0);
        self.fov = relax(self.fov, target, dt, self.config.relax_fov);
        self.fov
    }

    fn update_player(&mut self, hero: &HeroView, dt: f32, world: &dyn CameraWorld) -> CameraOutput {
        let preset = self.preset();
        let first_person = self.mode == CameraMode::FirstPerson || self.temp_first_person;
        let head = hero.head_pos();
        // Pivot: rigid in first person, otherwise relax 2 (0.05 s).
        self.pivot = if first_person {
            head
        } else {
            relax_vec3(self.pivot, head, dt, self.config.relax_player)
        };

        let view_yaw = self.yaw + self.look_back_yaw;
        let (position, rotation) = if first_person {
            self.distance = preset.min_range;
            self.cam_pos = relax_vec3(self.cam_pos, head, dt, self.config.relax_player);
            let p = world.push_out(self.cam_pos, FIRST_PERSON_WALL);
            // The view direction is the opposite of the orbit direction.
            let rot = Quat::from_euler(glam::EulerRot::YXZ, view_yaw, -self.elevation, 0.0);
            (p, rot)
        } else {
            let wished = self.wished_distance(hero.moving, &preset);
            self.update_distance(wished, dt);
            let ratio = preset.range_ratio(self.distance);
            let dir = dir_from_angles(view_yaw, self.elevation);
            let right = Vec3::new(view_yaw.cos(), 0.0, -view_yaw.sin());
            let off = |o: [[f32; 2]; 3]| {
                Vec3::new(lerp2(o[0], ratio), lerp2(o[1], ratio), lerp2(o[2], ratio))
            };
            let co = off(preset.cam_offset);
            let to = off(preset.target_offset);
            let flat_fwd = Vec3::new(-dir.x, 0.0, -dir.z).normalize_or(Vec3::NEG_Z);
            let cam_off = right * co.x + Vec3::Y * co.y + flat_fwd * co.z;
            let wished_pos = self.pivot + dir * self.distance + cam_off;
            self.cam_pos = relax_vec3(self.cam_pos, wished_pos, dt, self.config.relax_camera);
            // Collision: snap closer at once, the boom relaxes outward afterwards.
            let boom = self.cam_pos - self.pivot;
            let len = boom.length();
            if len > 1.0 {
                let d = boom / len;
                if let Some(hit) = world.ray_nearest(self.pivot, d, len + WALL_MARGIN) {
                    let allowed = (hit - WALL_MARGIN).max(MIN_BOOM);
                    if allowed < len {
                        self.cam_pos = self.pivot + d * allowed;
                        self.distance = self.distance.min(allowed);
                    }
                }
            }
            let look_point = self.pivot + right * to.x + Vec3::Y * to.y + flat_fwd * to.z;
            // Rotation offsets relax toward the ratio-interpolated values (slerp of the offset).
            self.rot_timer = (self.rot_timer + dt).min(preset.rot_offset_duration);
            let ease_t = if preset.rot_offset_duration > 0.0 {
                ease(self.rot_timer / preset.rot_offset_duration)
            } else {
                1.0
            };
            let f = (dt * preset.velo_rot * ease_t).clamp(0.0, 1.0);
            let target = Vec2::new(
                lerp2(preset.rot_offset_x, ratio),
                lerp2(preset.rot_offset_y, ratio),
            );
            self.rot_offset += (target - self.rot_offset) * f;
            let base = look_rotation(self.cam_pos, look_point);
            let rot = base
                * Quat::from_rotation_y(self.rot_offset.y.to_radians())
                * Quat::from_rotation_x(self.rot_offset.x.to_radians());
            (self.cam_pos, rot)
        };

        let fov = self.finish_fov(&preset, dt, first_person);
        let d = (position - head).length().clamp(20.0, 100.0);
        let mut alpha = ease((d - 20.0) / 80.0);
        let hide = first_person && !hero.down && !hero.keep_body_visible;
        if hide {
            alpha = 0.0;
        }
        if self.special_bow {
            // `OnProcess`: the body fades with the ease curve over `SPECIAL_BOW_FADE_SECS`.
            self.special_bow_time =
                (self.special_bow_time + dt).min(crate::aim::SPECIAL_BOW_FADE_SECS);
            alpha = crate::aim::special_bow_alpha(self.special_bow_time);
        }
        CameraOutput {
            position,
            rotation,
            fov_deg: fov,
            z_near: if first_person {
                self.config.z_near_first_person
            } else {
                self.config.z_near
            },
            hero_alpha: alpha,
            hide_hero: hide,
            listener: position,
            speaker_point: None,
            focus_mode: if first_person { 1 } else { 2 },
            mode: self.mode,
            items_visible: hide || self.special_bow || first_person,
        }
    }

    fn update_dialog(
        &mut self,
        input: &CameraInput,
        dt: f32,
        world: &dyn CameraWorld,
    ) -> CameraOutput {
        let _ = world;
        let Some(mut d) = self.dialog.take() else {
            unreachable!("update_dialog without dialog")
        };
        if let Some((l, s)) = input.dialog {
            d.listener = l;
            d.speaker = s;
        }
        d.timer += dt;
        let preset = self.presets.get(d.preset).clone();
        let pose = dialog::shot_pose(&self.presets, d.preset, &d.listener, &d.speaker, d.timer);
        self.cam_pos = relax_vec3(self.cam_pos, pose.position, dt, self.config.relax_camera);
        d.look_at = relax_vec3(d.look_at, pose.look_at, dt, self.config.relax_camera);
        self.pivot = d.look_at;
        self.rot_timer = (self.rot_timer + dt).min(preset.rot_offset_duration);
        let rot = look_rotation(self.cam_pos, d.look_at)
            * Quat::from_rotation_x(lerp2(preset.rot_offset_x, 0.0).to_radians());
        let fov = self.finish_fov(&preset, dt, true);
        let speaker_point = d.speaker.speaker_point();
        let out = CameraOutput {
            position: self.cam_pos,
            rotation: rot,
            fov_deg: fov,
            z_near: self.config.z_near,
            hero_alpha: 1.0,
            hide_hero: false,
            listener: self.cam_pos,
            speaker_point: Some(speaker_point),
            focus_mode: 2,
            mode: CameraMode::Dialog,
            items_visible: false,
        };
        self.dialog = Some(d);
        out
    }

    /// Forward direction of the camera on the ground plane: the "wish" frame of reference for
    /// player movement (`gCCharacterControl_PS::ProcessMovements`, GameDll:0x100ddd40).
    pub fn ground_forward(&self) -> Vec3 {
        let d = self.pivot - self.cam_pos;
        Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::NEG_Z)
    }

    /// Host override of the orbit (scripted shots, tests): azimuth in the `view_yaw` convention
    /// and elevation above the horizon, radians. Filters keep their state.
    pub fn set_orbit(&mut self, yaw: f32, elevation: f32) {
        self.yaw = yaw;
        self.elevation = elevation.clamp(
            -MAX_ELEVATION_DEG.to_radians(),
            MAX_ELEVATION_DEG.to_radians(),
        );
    }

    /// Azimuth of the view direction in radians (hero yaw convention: rotation about Y, forward
    /// -Z at 0).
    pub fn view_yaw(&self) -> f32 {
        self.yaw + self.look_back_yaw
    }
}
