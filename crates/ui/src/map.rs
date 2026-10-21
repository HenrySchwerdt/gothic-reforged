//! The world map (`gCMap_PS`, `gCHUDStaticMap`, E34): the map sheet that ties a map picture to a
//! rectangle of world space, the markers of its entities, the zoom window around the player and
//! the 23 location buttons of the quest page's world view (`IDD_VIEW_WORLD_2`).
//!
//! Map space is the ground plane: x east, z north. The sheet stores its top-left and
//! bottom-right corners, `CalcUV` turns a world position into texture coordinates with v
//! pointing down. The sheet of a level is generated from its marker entities
//! (`GenerateMarkers`): the square around their bounding box.
use bevy::prelude::*;

/// A map picture and the world rectangle it covers (the fields of `gCMap_PS`).
#[derive(Clone, Debug, PartialEq)]
pub struct MapSheet {
    /// Lower-case `g3.igl` picture name (`g3_map_world.tga`).
    pub bitmap: String,
    /// Localised name of the map.
    pub name: String,
    /// World x / z of the top-left corner.
    pub top_left: Vec2,
    /// World x / z of the bottom-right corner.
    pub bottom_right: Vec2,
}

impl MapSheet {
    /// Port of gCMap_PS::CalcUV (Game:0x101a42e0): normalised picture coordinates of a world
    /// position, `None` for a degenerate sheet.
    pub fn calc_uv(&self, x: f32, z: f32) -> Option<Vec2> {
        let dz = self.top_left.y - self.bottom_right.y;
        let dx = self.bottom_right.x - self.top_left.x;
        if dz == 0.0 || dx == 0.0 {
            return None;
        }
        let u = (x - self.top_left.x) / dx;
        let v = 1.0 - (z - self.bottom_right.y) / dz;
        Some(Vec2::new(u, v))
    }

    /// Port of gCMap_PS::Contains (Game:0x101a4240).
    pub fn contains(&self, x: f32, z: f32) -> bool {
        self.top_left.x <= x
            && x <= self.bottom_right.x
            && z <= self.top_left.y
            && self.bottom_right.y <= z
    }

    /// Port of the rectangle step of gCMap_PS::GenerateMarkers (Game:0x101a43d0): the square
    /// around the bounding box of `points` (x, z), side `max(dx, dz) * (scale + margin)`, centred on
    /// the box. `scale` is the `gSMapGenerate` scale; the engine's additive margin constant is not
    /// readable from the decompiled code, so it is a parameter [I].
    pub fn around(points: &[Vec2], scale: f32, margin: f32) -> Option<(Vec2, Vec2)> {
        let first = points.first()?;
        let (mut lo, mut hi) = (*first, *first);
        for p in points {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let side = (hi.x - lo.x).max(hi.y - lo.y) * (scale + margin);
        let half = side * 0.5;
        let c = (lo + hi) * 0.5;
        Some((
            Vec2::new(c.x - half, c.y + half),
            Vec2::new(c.x + half, c.y - half),
        ))
    }
}

/// A marker on the map (an entity of the level with a `_`-separated name).
#[derive(Clone, Debug, PartialEq)]
pub struct MapMarker {
    pub label: String,
    pub pos: Vec2,
    /// Markers whose name contains `$` are not shown.
    pub visible: bool,
}

impl MapMarker {
    /// The marker of an entity named `name` at (x, z): the label is the part after the second
    /// underscore (`???` when there is none), `$` hides it (`GenerateMarkers`).
    pub fn from_entity(name: &str, x: f32, z: f32) -> Self {
        let label = name
            .match_indices('_')
            .nth(1)
            .map_or("???", |(i, _)| &name[i + 1..])
            .to_string();
        Self {
            visible: !label.contains('$'),
            label,
            pos: Vec2::new(x, z),
        }
    }
}

/// What the host tells the map pages: the sheet of the current level and the player.
#[derive(Resource, Clone, Debug, Default)]
pub struct MapData {
    pub sheet: Option<MapSheet>,
    pub markers: Vec<MapMarker>,
    /// Player x / z.
    pub player: Vec2,
}

/// Zoom steps of the map page's wheel / buttons.
pub const ZOOM_MIN: f32 = 1.0;
pub const ZOOM_MAX: f32 = 4.0;
pub const ZOOM_STEP: f32 = 0.5;

/// The picture rectangle (in UV) the map control shows: `view_px` is the control's client size,
/// `tex_px` the picture's size, `zoom` 1 = pixel for pixel when the picture is larger than the
/// control. The window is centred on `center` and slides back inside the picture at its edges.
/// Port of the window computation of gCHUDStaticMap's paint (Game:0x100a87d0).
pub fn view_window(center: Vec2, view_px: Vec2, tex_px: Vec2, zoom: f32) -> Rect {
    let span = (view_px / tex_px / zoom.max(0.01)).min(Vec2::ONE);
    let lo = (center - span * 0.5).clamp(Vec2::ZERO, Vec2::ONE - span);
    Rect::from_corners(lo, lo + span)
}

/// Position of a UV point inside a window, in pixels of the control (`None` outside).
pub fn to_view(uv: Vec2, window: Rect, view_px: Vec2) -> Option<Vec2> {
    let rel = (uv - window.min) / window.size();
    ((0.0..=1.0).contains(&rel.x) && (0.0..=1.0).contains(&rel.y)).then_some(rel * view_px)
}

// ------------------------------------------------------------------ quest page locations
/// A location button of the world view: lower-case name (`g3_hud_icon_location_<name>.tga`),
/// position inside the 442x392 view (the control rectangles of `IDD_VIEW_WORLD_2`; the 18x18
/// icon's top-left). The button order of the resource is alphabetical, which `g3.igl`'s icon
/// names confirm.
pub const LOCATIONS: [(&str, f32, f32); 23] = [
    ("ardea", 360.0, 186.0),
    ("bakaresh", 308.0, 254.0),
    ("benerai", 212.0, 238.0),
    ("bensala", 252.0, 266.0),
    ("braga", 164.0, 222.0),
    ("faring", 328.0, 118.0),
    ("fireclan", 284.0, 22.0),
    ("geldern", 192.0, 134.0),
    ("gotha", 296.0, 130.0),
    ("hammerclan", 324.0, 58.0),
    ("innosmonastery", 276.0, 42.0),
    ("ishtar", 36.0, 318.0),
    ("kapdun", 312.0, 210.0),
    ("lago", 240.0, 234.0),
    ("montera", 272.0, 162.0),
    ("morasul", 152.0, 314.0),
    ("nemora", 172.0, 174.0),
    ("okara", 256.0, 114.0),
    ("reddock", 336.0, 190.0),
    ("silden", 220.0, 110.0),
    ("trelis", 208.0, 170.0),
    ("vengard", 356.0, 146.0),
    ("wolfclan", 312.0, 78.0),
];

/// The world view's two radio buttons (`Main` / `World`): which locations show.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct WorldLocations {
    /// "Alle" (true) shows every location button, "Haupt" only those with a running quest.
    pub all: bool,
    /// Lower-case names of locations with a running quest (fed by the host).
    pub with_quest: Vec<String>,
}

impl Default for WorldLocations {
    fn default() -> Self {
        Self {
            all: true,
            with_quest: Vec::new(),
        }
    }
}

impl WorldLocations {
    /// The location buttons to draw.
    pub fn shown(&self) -> Vec<&'static (&'static str, f32, f32)> {
        LOCATIONS
            .iter()
            .filter(|(n, _, _)| {
                self.all || self.with_quest.iter().any(|q| q.eq_ignore_ascii_case(n))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> MapSheet {
        MapSheet {
            bitmap: "g3_map_world.tga".into(),
            name: "Myrtana".into(),
            top_left: Vec2::new(-1000.0, 1000.0),
            bottom_right: Vec2::new(1000.0, -1000.0),
        }
    }

    #[test]
    fn uv_has_v_pointing_down() {
        let s = sheet();
        assert_eq!(s.calc_uv(-1000.0, 1000.0), Some(Vec2::new(0.0, 0.0)));
        assert_eq!(s.calc_uv(1000.0, -1000.0), Some(Vec2::new(1.0, 1.0)));
        assert_eq!(s.calc_uv(0.0, 500.0), Some(Vec2::new(0.5, 0.25)));
        let flat = MapSheet {
            bottom_right: Vec2::new(-1000.0, -1000.0),
            ..s
        };
        assert_eq!(flat.calc_uv(0.0, 0.0), None);
    }

    #[test]
    fn contains_checks_both_axes() {
        let s = sheet();
        assert!(s.contains(0.0, 0.0) && s.contains(-1000.0, -1000.0));
        assert!(!s.contains(1001.0, 0.0) && !s.contains(0.0, -1001.0));
    }

    #[test]
    fn the_generated_sheet_is_a_square_around_the_markers() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(400.0, 100.0)];
        let (tl, br) = MapSheet::around(&pts, 1.0, 0.0).unwrap();
        assert_eq!(tl, Vec2::new(0.0, 250.0));
        assert_eq!(br, Vec2::new(400.0, -150.0));
        let (tl2, _) = MapSheet::around(&pts, 1.0, 0.5).unwrap();
        assert!(tl2.x < tl.x, "a margin widens the sheet");
        assert!(MapSheet::around(&[], 1.0, 0.0).is_none());
    }

    #[test]
    fn marker_labels_come_from_the_second_underscore() {
        let m = MapMarker::from_entity("Map_Marker_Ardea", 1.0, 2.0);
        assert_eq!(m.label, "Ardea");
        assert!(m.visible);
        assert_eq!(MapMarker::from_entity("Marker", 0.0, 0.0).label, "???");
        assert_eq!(MapMarker::from_entity("A_B", 0.0, 0.0).label, "???");
        assert!(!MapMarker::from_entity("Map_Marker_$Secret", 0.0, 0.0).visible);
    }

    #[test]
    fn the_view_window_centres_and_stays_inside() {
        let w = view_window(
            Vec2::splat(0.5),
            Vec2::new(442.0, 392.0),
            Vec2::splat(510.0),
            1.0,
        );
        assert!((w.size().x - 442.0 / 510.0).abs() < 1e-5);
        assert!(((w.min + w.max) * 0.5 - Vec2::splat(0.5)).length() < 1e-5);
        let edge = view_window(Vec2::ZERO, Vec2::new(442.0, 392.0), Vec2::splat(510.0), 2.0);
        assert_eq!(edge.min, Vec2::ZERO, "slides back to the corner");
        let whole = view_window(
            Vec2::splat(0.3),
            Vec2::splat(600.0),
            Vec2::splat(510.0),
            1.0,
        );
        assert_eq!(
            whole.size(),
            Vec2::ONE,
            "a larger control shows the whole picture"
        );
    }

    #[test]
    fn view_positions_are_in_control_pixels() {
        let w = Rect::from_corners(Vec2::new(0.25, 0.25), Vec2::new(0.75, 0.75));
        let p = to_view(Vec2::splat(0.5), w, Vec2::new(400.0, 300.0)).unwrap();
        assert_eq!(p, Vec2::new(200.0, 150.0));
        assert!(to_view(Vec2::splat(0.9), w, Vec2::splat(100.0)).is_none());
    }

    #[test]
    fn locations_filter_by_mode() {
        let mut l = WorldLocations::default();
        assert_eq!(l.shown().len(), 23);
        l.all = false;
        assert!(l.shown().is_empty());
        l.with_quest = vec!["Ardea".into(), "gotha".into()];
        let names: Vec<_> = l.shown().iter().map(|(n, _, _)| *n).collect();
        assert_eq!(names, ["ardea", "gotha"]);
    }
}
