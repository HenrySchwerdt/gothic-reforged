// Shared foliage distance fade for the forward and depth/shadow prepass fragments.
// The caller supplies the pass camera explicitly so both passes apply identical thresholds.
fn foliage_fade_discards(
    world_position: vec3<f32>,
    camera_position: vec3<f32>,
    fade_bounds: vec2<f32>,
    fragment_position: vec2<f32>,
) -> bool {
    let dist = distance(world_position, camera_position);
    let fade = 1.0 - smoothstep(fade_bounds.x, fade_bounds.y, dist);
    let ign = fract(52.9829189 * fract(dot(fragment_position, vec2<f32>(0.06711056, 0.00583715))));
    return fade < ign;
}
