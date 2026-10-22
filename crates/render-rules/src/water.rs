//! Source identity and planar-height rules for water placement.

/// A confirmed sea-water resource has this exact basename form in the Ardea world index.
pub fn is_confirmed_ocean_resource(resource: &str) -> bool {
    has_numbered_xcmsh_basename(resource, "g3_world_oceanmesh_")
}

/// The installed river depth-hull family is separate from visible river/water meshes.
pub fn is_river_depth_resource(resource: &str) -> bool {
    has_numbered_xcmsh_basename(resource, "g3_world_rivermesh_01_depth_")
}

fn has_numbered_xcmsh_basename(resource: &str, prefix: &str) -> bool {
    let basename = resource.rsplit(['/', '\\']).next().unwrap_or(resource);
    let basename = basename.to_ascii_lowercase();
    let Some(number) = basename
        .strip_prefix(prefix)
        .and_then(|name| name.strip_suffix(".xcmsh"))
    else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
}

/// Mean height in metres for transformed vertex heights in centimetres, if they form a
/// horizontal water plane within one centimetre. Non-finite and empty inputs are rejected.
pub fn planar_water_height_m(heights_cm: impl Iterator<Item = f32>) -> Option<f32> {
    let (mut min_y, mut max_y, mut sum_y, mut count) =
        (f32::INFINITY, f32::NEG_INFINITY, 0.0f64, 0usize);
    for y in heights_cm {
        if !y.is_finite() {
            return None;
        }
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        sum_y += f64::from(y);
        count += 1;
    }
    if count == 0 || !(max_y - min_y).is_finite() || max_y - min_y > 1.0 {
        return None;
    }
    let height_m = (sum_y / count as f64 * 0.01) as f32;
    height_m.is_finite().then_some(height_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_checks_empty_finite_range_and_units() {
        assert_eq!(planar_water_height_m(std::iter::empty()), None);
        assert_eq!(planar_water_height_m([0.0, f32::NAN].into_iter()), None);
        assert_eq!(planar_water_height_m([0.0, f32::INFINITY].into_iter()), None);
        assert_eq!(planar_water_height_m([0.0, 1.01].into_iter()), None);
        let height = planar_water_height_m([-453.0, -453.5, -453.25].into_iter()).unwrap();
        assert!((height - -4.5325).abs() < 0.0001);
        assert!(planar_water_height_m([1.0, 2.0].into_iter()).is_some());
    }

    #[test]
    fn source_names_require_the_confirmed_basename_patterns() {
        assert!(is_confirmed_ocean_resource("G3_World_Oceanmesh_068.xcmsh"));
        assert!(is_confirmed_ocean_resource(
            "Data/World/G3_World_Oceanmesh_051.XCMSH"
        ));
        assert!(!is_confirmed_ocean_resource("G3_World_Oceanmesh_test.xcmsh"));
        assert!(!is_confirmed_ocean_resource("G3_World_Oceanmesh_068.xlmsh"));
        assert!(!is_confirmed_ocean_resource(
            "G3_World_Rivermesh_01_Depth_118.xcmsh"
        ));

        assert!(is_river_depth_resource(
            "G3_World_Rivermesh_01_Depth_118.xcmsh"
        ));
        assert!(is_river_depth_resource(
            "Data/World/G3_World_Rivermesh_01_Depth_007.XCMSH"
        ));
        assert!(!is_river_depth_resource("G3_World_Rivermesh_01.xcmsh"));
        assert!(!is_river_depth_resource(
            "G3_Myrtana_01_LandscapeWatermesh_01.xcmsh"
        ));
        assert!(!is_river_depth_resource(
            "G3_World_Rivermesh_01_Depth_test.xcmsh"
        ));
    }
}
