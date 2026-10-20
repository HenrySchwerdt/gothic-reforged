//! Camera tuning read from `Ini/ge3.INI` `[Camera]`.

/// Tunables of the gameplay camera (defaults are the shipped `ge3.INI` values).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraConfig {
    /// `fAziSpeedScale`.
    pub azi_speed_scale: f32,
    /// `fElevSpeedScale`.
    pub elev_speed_scale: f32,
    /// `fRelaxFOV`.
    pub relax_fov: f32,
    /// `fRelaxDist`.
    pub relax_dist: f32,
    /// `fRelaxXAxis` (azimuth).
    pub relax_x_axis: f32,
    /// `fRelaxYAxis` (elevation).
    pub relax_y_axis: f32,
    /// `fRelaxPlayer` (pivot).
    pub relax_player: f32,
    /// `fRelaxCamera` (camera position).
    pub relax_camera: f32,
    /// `fMaxDistToPlayer`.
    pub max_dist_to_player: f32,
    /// `Camera.ZNear` of `[Engine.Setup]`.
    pub z_near: f32,
    /// Near plane in first person (constant in the engine).
    pub z_near_first_person: f32,
    /// Mouse sensitivity multiplier (`2 * s + 0.5` of the option slider, 0.5..2.5).
    pub sensitivity: [f32; 2],
    /// Mouse smoothing multiplier on the relax constants (`2 * s`; 0 = unfiltered).
    pub smoothing: [f32; 2],
    /// `bKeep1stPerson`.
    pub keep_first_person: bool,
    /// Radians of look rotation per mouse count at sensitivity 1 (tuned; the original scales raw
    /// axis values by 550 and `dt`, whose device unit is not recoverable).
    pub radians_per_count: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            azi_speed_scale: 0.1,
            elev_speed_scale: 0.07,
            relax_fov: 25.0,
            relax_dist: 60.0,
            relax_x_axis: 8.0,
            relax_y_axis: 15.0,
            relax_player: 2.0,
            relax_camera: 20.0,
            max_dist_to_player: 800.0,
            z_near: 25.0,
            z_near_first_person: 13.0,
            sensitivity: [1.5, 1.5],
            smoothing: [1.0, 1.0],
            keep_first_person: false,
            radians_per_count: 0.0016,
        }
    }
}

impl CameraConfig {
    /// Read `[Camera]` and `Camera.ZNear` from the text of `ge3.INI`; absent keys keep defaults.
    pub fn from_ini(text: &str) -> Self {
        let mut c = Self::default();
        let mut section = "";
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(';') || line.is_empty() {
                continue;
            }
            if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = s;
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            let num = v
                .trim_end_matches(['f', 'F'])
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<f32>().ok());
            let slot = match (section, k.trim()) {
                ("Camera", "fAziSpeedScale") => &mut c.azi_speed_scale,
                ("Camera", "fElevSpeedScale") => &mut c.elev_speed_scale,
                ("Camera", "fRelaxFOV") => &mut c.relax_fov,
                ("Camera", "fRelaxDist") => &mut c.relax_dist,
                ("Camera", "fRelaxXAxis") => &mut c.relax_x_axis,
                ("Camera", "fRelaxYAxis") => &mut c.relax_y_axis,
                ("Camera", "fRelaxPlayer") => &mut c.relax_player,
                ("Camera", "fRelaxCamera") => &mut c.relax_camera,
                ("Camera", "fMaxDistToPlayer") => &mut c.max_dist_to_player,
                ("Engine.Setup", "Camera.ZNear") => &mut c.z_near,
                ("Camera", "bKeep1stPerson") => {
                    c.keep_first_person = v.starts_with("true") || v == "1";
                    continue;
                }
                _ => continue,
            };
            if let Some(n) = num {
                *slot = n;
            }
        }
        c
    }

    /// Mouse option sliders (0..1) to the stored multipliers.
    pub fn set_mouse_options(&mut self, sensitivity: [f32; 2], smoothing: [f32; 2]) {
        self.sensitivity = sensitivity.map(|s| 2.0 * s.clamp(0.0, 1.0) + 0.5);
        self.smoothing = smoothing.map(|s| 2.0 * s.clamp(0.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shipped_camera_section() {
        let ini = "[Camera]\nfMoveSpeed=100.0\nfAziSpeedScale=0.10000\nfRelaxCamera=20.0\nfRelaxPlayer=2.0\nfRelaxYAxis=15.0\nfRelaxXAxis=8.0\nfRelaxDist=60.0\nfRelaxFOV=25.0\nfMaxDistToPlayer=800.0f\n[Engine.Setup]\nCamera.ZNear=25.0\n";
        let c = CameraConfig::from_ini(ini);
        assert_eq!(c.max_dist_to_player, 800.0);
        assert_eq!(c.relax_camera, 20.0);
        assert_eq!(c.z_near, 25.0);
        assert_eq!(c, CameraConfig::default());
    }

    #[test]
    fn mouse_options_map_to_original_ranges() {
        let mut c = CameraConfig::default();
        c.set_mouse_options([0.0, 1.0], [0.0, 0.5]);
        assert_eq!(c.sensitivity, [0.5, 2.5]);
        assert_eq!(c.smoothing, [0.0, 1.0]);
    }
}
