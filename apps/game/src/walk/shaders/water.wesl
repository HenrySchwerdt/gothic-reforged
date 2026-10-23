// Animated water: two scrolling DXT5nm normal maps, fresnel reflection of the sky, a sun
// glint and a depth-free colour gradient. Alpha-blended.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::{pbr_input_new, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
    view_transformations::depth_ndc_to_view_z,
};
import bevy_pbr::prepass::utils::prepass_depth;

struct WaterParams {
    sun_dir: vec4<f32>,
    deep: vec4<f32>,
    shallow: vec4<f32>,
    sky: vec4<f32>,
    sun_color: vec4<f32>,
    // x: wave strength, y: normal-map Y sign, z: base opacity, w: shoreline fade distance (m)
    misc: vec4<f32>,
};
@group(3) @binding(0) var<uniform> water: WaterParams;
@group(3) @binding(1) var tex_n0: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;
@group(3) @binding(3) var tex_n1: texture_2d<f32>;

fn decode_normal(t: vec4<f32>) -> vec3<f32> {
    let xy = vec2<f32>(t.a, t.g) * 2.0 - 1.0;
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let t = globals.time;
    let p = in.world_position.xz;
    let n0 = decode_normal(textureSample(tex_n0, samp, p * 0.11 + vec2<f32>(t * 0.018, t * 0.011)));
    let n1 = decode_normal(textureSample(tex_n1, samp, p * 0.27 + vec2<f32>(-t * 0.021, t * 0.016)));
    let n2 = decode_normal(textureSample(tex_n0, samp, p * 0.63 + vec2<f32>(t * 0.05, -t * 0.03)));
    var nt = normalize(vec3<f32>((n0.xy + n1.xy + 0.5 * n2.xy) * water.misc.x, 1.0));
    nt.y = nt.y * water.misc.y;
    // Water surfaces are horizontal; tangent space == world XZ.
    let n = normalize(vec3<f32>(nt.x, nt.z, nt.y));

    let v = normalize(view.world_position.xyz - in.world_position.xyz);
    let ndv = clamp(dot(n, v), 0.0, 1.0);
    let fres = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    let r = reflect(-v, n);
    let sun = normalize(water.sun_dir.xyz);
    // Sky reflection: horizon colour near the horizon, sky colour above.
    let refl_sky = mix(water.shallow.rgb * 1.4, water.sky.rgb, clamp(r.y * 1.5, 0.0, 1.0));
    let body = mix(water.deep.rgb, water.shallow.rgb, pow(ndv, 1.5));
    var col = mix(body, refl_sky, clamp(fres * 1.6, 0.0, 1.0));
    let spec = pow(max(dot(r, sun), 0.0), 220.0);
    col = col + water.sun_color.rgb * spec * 2.5;
    // Bevy uses reverse-Z: prepass clear/far depth is 0. Convert both values with Bevy's
    // projection helper before subtracting so the fade uses linear view-space metres. Water
    // opts out of the prepass, so this is the opaque scene behind it, never water self-depth.
    let scene_depth_ndc = prepass_depth(in.position, 0u);
    var shoreline = 1.0;
    if scene_depth_ndc > 0.0 {
        let scene_view_z = depth_ndc_to_view_z(scene_depth_ndc);
        let water_view_z = depth_ndc_to_view_z(in.position.z);
        let scene_gap = max(water_view_z - scene_view_z, 0.0);
        shoreline = clamp(scene_gap / max(water.misc.w, 0.001), 0.0, 1.0);
    }
    let alpha = clamp(mix(water.misc.z, 1.0, fres) * shoreline, 0.0, 1.0);

    var pbr = pbr_input_new();
    pbr.material.flags = STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.world_position = in.world_position;
    pbr.frag_coord = in.position;
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, vec4<f32>(col, alpha));
    return out;
}
