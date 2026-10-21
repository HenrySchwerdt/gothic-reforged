// Characters and props rendered with the Gothic 3 lighting model (ORIGINAL_SHADING.md): gamma
// space, linear SH ambient, sun N.L with cascaded shadows, rim term, then Bevy fog.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_bindings::mesh,
    mesh_view_bindings::{view, lights},
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::{pbr_input_new, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
};

struct CharParams {
    tint: vec4<f32>,
    // x: alpha cutoff (0 = none), y: rim start, z: rim end, w: rim intensity
    misc: vec4<f32>,
    sun: vec4<f32>,
    sh: array<vec4<f32>, 4>,
    // static point lights: xyz position, w = 1 / range^2; colour x2 in light_cols
    lights: array<vec4<f32>, 6>,
    light_cols: array<vec4<f32>, 6>,
};
@group(3) @binding(0) var<uniform> params: CharParams;
@group(3) @binding(1) var tex: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let t = textureSample(tex, samp, in.uv) * params.tint;
    if params.misc.x > 0.0 && t.a < params.misc.x {
        discard;
    }
    var n = normalize(in.world_normal);
    if !is_front {
        n = -n;
    }
    let v = normalize(view.world_position.xyz - in.world_position.xyz);
    let ndv = max(dot(n, v), 0.0);
    let sh = params.sh;
    let amb = sh[0].rgb + sh[1].rgb * n.x + sh[2].rgb * n.y + sh[3].rgb * n.z;
    var light = amb * (0.75 + 0.25 * ndv);

    var pbr = pbr_input_new();
    pbr.material.base_color = vec4<f32>(t.rgb, 1.0);
    pbr.material.flags = STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.flags = mesh[in.instance_index].flags;
    pbr.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr.V = v;
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    pbr.world_normal = n;
    pbr.N = n;

    let sun_l = lights.directional_lights[0].direction_to_light;
    var shadow = 1.0;
    if (pbr.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
        && (lights.directional_lights[0].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
        let view_z = dot(vec4<f32>(
            view.view_from_world[0].z, view.view_from_world[1].z,
            view.view_from_world[2].z, view.view_from_world[3].z), in.world_position);
        shadow = shadows::fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
    }
    // vecShadowDensity: the weather ShadowDensity scales how dark shadows get (sh[0].w).
    shadow = 1.0 - (1.0 - shadow) * params.sh[0].w;
    light = light + params.sun.rgb * max(dot(n, sun_l), 0.0) * shadow;

    // Static point lights (the original's per-object packet: position, colour x 2, 1 / range^2).
    for (var i = 0u; i < 6u; i = i + 1u) {
        let lc = params.light_cols[i].rgb;
        if lc.r + lc.g + lc.b > 0.0 {
            let d = params.lights[i].xyz - in.world_position.xyz;
            let att = saturate(1.0 - dot(d, d) * params.lights[i].w);
            light = light + lc * att * att * max(dot(n, normalize(d)), 0.0);
        }
    }

    // Rim light (non-skin permutation): smoothstep on (1 - N.V) between start and end.
    let rt = clamp((1.0 - ndv - params.misc.y) / (params.misc.z - params.misc.y), 0.0, 1.0);
    let rim = rt * rt * (3.0 - 2.0 * rt) * params.misc.w;
    light = light + (clamp(light, vec3<f32>(0.0), vec3<f32>(0.5)) * params.misc.y + params.misc.z) * rim;

    var out: FragmentOutput;
    out.color = vec4<f32>(srgb_to_linear(linear_to_srgb(t.rgb) * light), t.a);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
