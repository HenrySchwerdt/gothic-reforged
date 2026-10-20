//! Camera modes and the 29 `gCCameraModeParams` presets of `Camera_PC_Camera.tple`.
use gothic_formats::propstream::{DecodeStats, PropertySet};
use gothic_formats::template::TemplateDocument;

/// Failure to read the camera template.
#[derive(Debug)]
pub struct PresetError(pub String);

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PresetError {}

/// `gECameraMode` (names printed by `gCCameraAI_PS::ShowDebug`, GameDll:0x10022b10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CameraMode {
    #[default]
    Normal = 0,
    Melee = 1,
    Ranged = 2,
    Magic = 3,
    Dialog = 4,
    Inventory = 5,
    Swim = 6,
    Dive = 7,
    Interaction = 8,
    Fall = 9,
    Death = 10,
    FirstPerson = 11,
    Shoulder = 12,
    LookBack = 13,
}

impl CameraMode {
    /// Mode from the script-visible integer; `None` outside 0..=13.
    pub fn from_index(i: i32) -> Option<Self> {
        use CameraMode::*;
        Some(match i {
            0 => Normal,
            1 => Melee,
            2 => Ranged,
            3 => Magic,
            4 => Dialog,
            5 => Inventory,
            6 => Swim,
            7 => Dive,
            8 => Interaction,
            9 => Fall,
            10 => Death,
            11 => FirstPerson,
            12 => Shoulder,
            13 => LookBack,
            _ => return None,
        })
    }

    /// Whether this is a first-person view.
    pub fn is_first_person(self) -> bool {
        self == CameraMode::FirstPerson
    }
}

/// One slot of the 29 `CamMode*` properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetKey {
    Normal,
    Melee,
    Ranged,
    Magic,
    Dialog,
    Inventory,
    Swim,
    Dive,
    Interaction,
    Fall,
    Death,
    FirstPerson,
    FirstPersonFight,
    FirstPersonRanged,
    Shoulder,
    DialogFaceL,
    DialogFaceR,
    DialogFace2L,
    DialogFace2R,
    DialogShoulder1L,
    DialogShoulder2L,
    DialogShoulder1R,
    DialogShoulder2R,
    DialogSideL,
    DialogSideR,
    DialogLerp1,
    DialogLerp2,
    DialogLerp3,
    DialogLerp4,
}

impl PresetKey {
    /// All keys in property-table order.
    pub const ALL: [PresetKey; 29] = [
        PresetKey::Normal,
        PresetKey::Melee,
        PresetKey::Ranged,
        PresetKey::Magic,
        PresetKey::Dialog,
        PresetKey::Inventory,
        PresetKey::Swim,
        PresetKey::Dive,
        PresetKey::Interaction,
        PresetKey::Fall,
        PresetKey::Death,
        PresetKey::FirstPerson,
        PresetKey::FirstPersonFight,
        PresetKey::FirstPersonRanged,
        PresetKey::Shoulder,
        PresetKey::DialogFaceL,
        PresetKey::DialogFaceR,
        PresetKey::DialogFace2L,
        PresetKey::DialogFace2R,
        PresetKey::DialogShoulder1L,
        PresetKey::DialogShoulder2L,
        PresetKey::DialogShoulder1R,
        PresetKey::DialogShoulder2R,
        PresetKey::DialogSideL,
        PresetKey::DialogSideR,
        PresetKey::DialogLerp1,
        PresetKey::DialogLerp2,
        PresetKey::DialogLerp3,
        PresetKey::DialogLerp4,
    ];

    /// Property name in the template.
    pub fn property_name(self) -> &'static str {
        use PresetKey::*;
        match self {
            Normal => "CamModeNormal",
            Melee => "CamModeMelee",
            Ranged => "CamModeRanged",
            Magic => "CamModeMagic",
            Dialog => "CamModeDialog",
            Inventory => "CamModeInventory",
            Swim => "CamModeSwim",
            Dive => "CamModeDive",
            Interaction => "CamModeInteraction",
            Fall => "CamModeFall",
            Death => "CamModeDeath",
            FirstPerson => "CamModeFirstPerson",
            FirstPersonFight => "CamModeFirstPersonFight",
            FirstPersonRanged => "CamModeFirstPersonRnged",
            Shoulder => "CamModeShoulder",
            DialogFaceL => "CamModeDialogFaceL",
            DialogFaceR => "CamModeDialogFaceR",
            DialogFace2L => "CamModeDialogFace2L",
            DialogFace2R => "CamModeDialogFace2R",
            DialogShoulder1L => "CamModeDialogShoulder1L",
            DialogShoulder2L => "CamModeDialogShoulder2L",
            DialogShoulder1R => "CamModeDialogShoulder1R",
            DialogShoulder2R => "CamModeDialogShoulder2R",
            DialogSideL => "CamModeDialogSideL",
            DialogSideR => "CamModeDialogSideR",
            DialogLerp1 => "CamModeDialogLerp1",
            DialogLerp2 => "CamModeDialogLerp2",
            DialogLerp3 => "CamModeDialogLerp3",
            DialogLerp4 => "CamModeDialogLerp4",
        }
    }

    /// Whether this is one of the dialog shot presets.
    pub fn is_dialog_shot(self) -> bool {
        (self as usize) >= PresetKey::DialogFaceL as usize
    }

    /// Mirrored left/right variant (Lerp presets mirror in pairs 1<->2 and 3<->4 by azimuth sign).
    pub fn mirrored(self) -> PresetKey {
        use PresetKey::*;
        match self {
            DialogFaceL => DialogFaceR,
            DialogFaceR => DialogFaceL,
            DialogFace2L => DialogFace2R,
            DialogFace2R => DialogFace2L,
            DialogShoulder1L => DialogShoulder1R,
            DialogShoulder1R => DialogShoulder1L,
            DialogShoulder2L => DialogShoulder2R,
            DialogShoulder2R => DialogShoulder2L,
            DialogSideL => DialogSideR,
            DialogSideR => DialogSideL,
            DialogLerp1 => DialogLerp2,
            DialogLerp2 => DialogLerp1,
            DialogLerp3 => DialogLerp4,
            DialogLerp4 => DialogLerp3,
            other => other,
        }
    }

    /// Preset used for a plain mode (`SetMode`, GameDll:0x1002c4b0). First person picks the
    /// fight or ranged variant from the combat state.
    pub fn for_mode(mode: CameraMode, in_combat: bool, ranged_weapon: bool) -> PresetKey {
        match mode {
            CameraMode::Normal | CameraMode::LookBack => PresetKey::Normal,
            CameraMode::Melee => PresetKey::Melee,
            CameraMode::Ranged => PresetKey::Ranged,
            CameraMode::Magic => PresetKey::Magic,
            CameraMode::Dialog => PresetKey::Dialog,
            CameraMode::Inventory => PresetKey::Inventory,
            CameraMode::Swim => PresetKey::Swim,
            CameraMode::Dive => PresetKey::Dive,
            CameraMode::Interaction => PresetKey::Interaction,
            CameraMode::Fall => PresetKey::Fall,
            CameraMode::Death => PresetKey::Death,
            CameraMode::Shoulder => PresetKey::Shoulder,
            CameraMode::FirstPerson if ranged_weapon => PresetKey::FirstPersonRanged,
            CameraMode::FirstPerson if in_combat => PresetKey::FirstPersonFight,
            CameraMode::FirstPerson => PresetKey::FirstPerson,
        }
    }
}

/// The 33 fields of `gCCameraModeParams`, in file order. Angles in degrees, lengths in cm.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraPreset {
    pub velo_trans: f32,
    pub velo_rot: f32,
    pub best_range: f32,
    pub min_range: f32,
    pub min_range_on_move: f32,
    pub max_range: f32,
    pub best_azi: f32,
    pub min_azi: f32,
    pub max_azi: f32,
    pub best_elev: f32,
    pub min_elev: f32,
    pub max_elev: f32,
    /// Pitch bias at `[min range, max range]`.
    pub rot_offset_x: [f32; 2],
    /// Yaw bias at `[min range, max range]`.
    pub rot_offset_y: [f32; 2],
    pub rot_offset_duration: f32,
    /// Look-at offset (x, y, z) at `[min range, max range]`.
    pub target_offset: [[f32; 2]; 3],
    /// Camera offset (x, y, z) at `[min range, max range]`.
    pub cam_offset: [[f32; 2]; 3],
    /// Vertical field of view at `[min range, max range]`, degrees.
    pub fov: [f32; 2],
    pub target_orientation_is_relevant: bool,
    pub is_dialog_preset: bool,
}

impl Default for CameraPreset {
    fn default() -> Self {
        // The generic preset all non-exploration modes of the shipped template share.
        Self {
            velo_trans: 0.5,
            velo_rot: 1.0,
            best_range: 150.0,
            min_range: 100.0,
            min_range_on_move: 300.0,
            max_range: 1500.0,
            best_azi: 0.0,
            min_azi: -180.0,
            max_azi: 180.0,
            best_elev: 30.0,
            min_elev: -90.0,
            max_elev: 90.0,
            rot_offset_x: [-5.0, -5.0],
            rot_offset_y: [0.0, 0.0],
            rot_offset_duration: 1.0,
            target_offset: [[0.0; 2]; 3],
            cam_offset: [[0.0; 2]; 3],
            fov: [45.0, 45.0],
            target_orientation_is_relevant: false,
            is_dialog_preset: false,
        }
    }
}

impl CameraPreset {
    /// Read one `gCCameraModeParams` object; absent fields keep the generic default.
    pub fn from_property_set(ps: &PropertySet) -> Self {
        let d = Self::default();
        let f = |n: &str, dv: f32| ps.float(n).unwrap_or(dv);
        let pair = |a: &str, b: &str, dv: [f32; 2]| [f(a, dv[0]), f(b, dv[1])];
        let axis = |x: &str| -> [[f32; 2]; 3] {
            ["X", "Y", "Z"].map(|c| {
                pair(
                    &format!("{x}{c}MinRange"),
                    &format!("{x}{c}MaxRange"),
                    [0.0; 2],
                )
            })
        };
        Self {
            velo_trans: f("VeloTrans", d.velo_trans),
            velo_rot: f("VeloRot", d.velo_rot),
            best_range: f("BestRange", d.best_range),
            min_range: f("MinRange", d.min_range),
            min_range_on_move: f("MinRangeOnMove", d.min_range_on_move),
            max_range: f("MaxRange", d.max_range),
            best_azi: f("BestAzi", d.best_azi),
            min_azi: f("MinAzi", d.min_azi),
            max_azi: f("MaxAzi", d.max_azi),
            best_elev: f("BestElev", d.best_elev),
            min_elev: f("MinElev", d.min_elev),
            max_elev: f("MaxElev", d.max_elev),
            rot_offset_x: pair("RotOffsetXMinRange", "RotOffsetXMaxRange", d.rot_offset_x),
            rot_offset_y: pair("RotOffsetYMinRange", "RotOffsetYMaxRange", d.rot_offset_y),
            rot_offset_duration: f("RotOffsetDuration", d.rot_offset_duration),
            target_offset: axis("TargetOffset"),
            cam_offset: axis("CamOffset"),
            fov: pair("FOVMinRange", "FOVMaxRange", d.fov),
            target_orientation_is_relevant: ps
                .boolean("TargetOrientationIsRelevant")
                .unwrap_or(false),
            is_dialog_preset: ps.boolean("IsDialogPreset").unwrap_or(false),
        }
    }

    /// Distance ratio in `[0, 1]` over the zoom window (0 when the window is empty or reversed).
    pub fn range_ratio(&self, distance: f32) -> f32 {
        let span = self.max_range - self.min_range;
        if span.abs() < f32::EPSILON {
            0.0
        } else {
            ((distance - self.min_range) / span).clamp(0.0, 1.0)
        }
    }
}

/// All 29 presets, indexed by [`PresetKey`].
#[derive(Debug, Clone, PartialEq)]
pub struct PresetTable {
    presets: Vec<CameraPreset>,
}

impl PresetTable {
    /// Preset for a key.
    pub fn get(&self, key: PresetKey) -> &CameraPreset {
        &self.presets[key as usize]
    }

    /// Parse the bytes of `Essentials/Camera/Camera_PC_Camera.tple`.
    pub fn from_tple(bytes: &[u8]) -> Result<Self, PresetError> {
        let mut stats = DecodeStats::default();
        let doc =
            TemplateDocument::parse(bytes, &mut stats).map_err(|e| PresetError(e.to_string()))?;
        let set = doc
            .entities
            .iter()
            .find_map(|e| e.set("gCCameraAI_PS"))
            .ok_or_else(|| PresetError("template has no gCCameraAI_PS".into()))?;
        let mut presets = Vec::with_capacity(PresetKey::ALL.len());
        for key in PresetKey::ALL {
            let value = set
                .get(key.property_name())
                .ok_or_else(|| PresetError(format!("missing {}", key.property_name())))?;
            let gothic_formats::propstream::PropValue::Object(obj) = value else {
                return Err(PresetError(format!(
                    "{} is not an object",
                    key.property_name()
                )));
            };
            presets.push(CameraPreset::from_property_set(obj));
        }
        Ok(Self { presets })
    }
}

/// (key, best range, min range, max range, best azimuth, best elevation, elevation window).
type DialogRow = (PresetKey, f32, f32, f32, f32, f32, [f32; 2]);

impl Default for PresetTable {
    /// The numbers of the shipped template (used when the install is unavailable).
    fn default() -> Self {
        use PresetKey::*;
        let mut presets = vec![CameraPreset::default(); PresetKey::ALL.len()];
        let mut set = |k: PresetKey, f: &dyn Fn(&mut CameraPreset)| f(&mut presets[k as usize]);
        set(Normal, &|p| {
            p.rot_offset_x = [-10.0, 5.0];
            p.fov = [65.0, 55.0];
        });
        set(Melee, &|p| {
            p.best_range = 450.0;
            p.min_range = 150.0;
            p.rot_offset_x = [5.0, -5.0];
            p.fov = [60.0, 65.0];
        });
        set(Ranged, &|p| {
            p.best_range = 300.0;
            p.min_range = 200.0;
            p.rot_offset_x = [0.0, -3.0];
            p.target_offset[1] = [50.0, 120.0];
            p.fov = [60.0, 65.0];
        });
        set(Magic, &|p| p.velo_trans = 1.0);
        for k in [FirstPerson, FirstPersonFight, FirstPersonRanged] {
            set(k, &|p| {
                p.velo_trans = 1.5;
                p.velo_rot = 10.0;
                p.fov = [70.0, 70.0];
            });
        }
        let dialog: [DialogRow; 14] = [
            (DialogFaceL, 200.0, -50.0, -50.0, 0.0, 0.0, [-90.0, 90.0]),
            (DialogFaceR, 200.0, -50.0, -50.0, 0.0, 0.0, [-90.0, 90.0]),
            (DialogFace2L, 250.0, -50.0, 250.0, -60.0, 5.0, [0.0, 0.0]),
            (DialogFace2R, 250.0, -50.0, 250.0, 60.0, 5.0, [0.0, 0.0]),
            (
                DialogShoulder1L,
                200.0,
                105.0,
                150.0,
                -40.0,
                25.0,
                [0.0, 0.0],
            ),
            (
                DialogShoulder2L,
                250.0,
                250.0,
                250.0,
                -60.0,
                5.0,
                [0.0, 0.0],
            ),
            (
                DialogShoulder1R,
                200.0,
                105.0,
                150.0,
                40.0,
                25.0,
                [0.0, 0.0],
            ),
            (DialogShoulder2R, 250.0, 250.0, 250.0, 60.0, 5.0, [0.0, 0.0]),
            (DialogSideL, 400.0, 400.0, 250.0, -120.0, 5.0, [0.0, 0.0]),
            (DialogSideR, 400.0, 400.0, 250.0, 120.0, 5.0, [0.0, 0.0]),
            (DialogLerp1, 500.0, 250.0, 150.0, 75.0, 28.0, [0.0, 0.0]),
            (DialogLerp2, 450.0, 250.0, 150.0, -75.0, 28.0, [0.0, 0.0]),
            (DialogLerp3, 200.0, 350.0, 150.0, -80.0, 10.0, [0.0, 0.0]),
            (DialogLerp4, 200.0, 350.0, 150.0, 80.0, 10.0, [0.0, 0.0]),
        ];
        for (k, best, min, max, azi, elev, win) in dialog {
            set(k, &|p| {
                p.best_range = best;
                p.min_range = min;
                p.max_range = max;
                p.best_azi = azi;
                p.best_elev = elev;
                p.min_elev = win[0];
                p.max_elev = win[1];
                p.fov = [60.0, 65.0];
            });
        }
        Self { presets }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TPLE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.local/tple/Essentials/Camera/Camera_PC_Camera.tple"
    );

    #[test]
    fn mode_indices_round_trip() {
        for i in 0..=13 {
            assert_eq!(CameraMode::from_index(i).unwrap() as i32, i);
        }
        assert!(CameraMode::from_index(14).is_none());
    }

    #[test]
    fn normal_preset_numbers_from_the_shipped_template() {
        let t = PresetTable::default();
        let n = t.get(PresetKey::Normal);
        assert_eq!(
            (n.best_range, n.min_range, n.min_range_on_move, n.max_range),
            (150.0, 100.0, 300.0, 1500.0)
        );
        assert_eq!(n.fov, [65.0, 55.0]);
        // FOV at the rest distance is about 64.9 (cosine ease over the zoom window).
        let ratio = n.range_ratio(150.0);
        let fov = crate::math::lerp2(n.fov, crate::math::ease(ratio));
        assert!((fov - 64.9).abs() < 0.1, "{fov}");
    }

    #[test]
    fn installed_template_matches_the_built_in_table() {
        let Ok(bytes) = std::fs::read(TPLE) else {
            eprintln!("skipped: {TPLE} not extracted");
            return;
        };
        let loaded = PresetTable::from_tple(&bytes).unwrap();
        assert_eq!(loaded, PresetTable::default());
    }

    #[test]
    fn mirrored_is_an_involution() {
        for k in PresetKey::ALL {
            assert_eq!(k.mirrored().mirrored(), k);
        }
    }

    #[test]
    fn first_person_variant_follows_combat_state() {
        assert_eq!(
            PresetKey::for_mode(CameraMode::FirstPerson, true, false),
            PresetKey::FirstPersonFight
        );
        assert_eq!(
            PresetKey::for_mode(CameraMode::FirstPerson, true, true),
            PresetKey::FirstPersonRanged
        );
        assert_eq!(
            PresetKey::for_mode(CameraMode::FirstPerson, false, false),
            PresetKey::FirstPerson
        );
    }
}
