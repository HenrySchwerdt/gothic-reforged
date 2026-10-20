// Gothic 3 surface shader: up to two blended layers (vertex alpha), each with a base texture
// and an optional x2-modulated second texture (dirt/detail map), two DXT5nm normal maps,
// alpha-test, vertex AO and a distance dither fade for foliage. Lit with the Bevy PBR
// functions so cascaded shadows, ambient light and distance fog apply.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_bindings::mesh,
    mesh_view_bindings::{view, lights},
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, calculate_view},
    pbr_types::{pbr_input_new, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
};

struct GothicParams {
    tint_a: vec4<f32>,
    tint_b: vec4<f32>,
    scale_a: vec4<f32>,
    scale_b: vec4<f32>,
    scale_n: vec4<f32>,
    // x: fade start, y: fade end, z: alpha cutoff, w: normal-map Y sign
    misc: vec4<f32>,
    flags: u32,
    ao_strength: f32,
    normal_strength: f32,
    roughness: f32,
    sun: vec4<f32>,
    sh: array<vec4<f32>, 4>,
};

@group(3) @binding(0) var<uniform> params: GothicParams;
@group(3) @binding(1) var tex_a0: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;
@group(3) @binding(3) var tex_a1: texture_2d<f32>;
@group(3) @binding(4) var tex_b0: texture_2d<f32>;
@group(3) @binding(5) var tex_b1: texture_2d<f32>;
@group(3) @binding(6) var tex_na: texture_2d<f32>;
@group(3) @binding(7) var tex_nb: texture_2d<f32>;

const F_A1: u32 = 1u;
const F_B0: u32 = 2u;
const F_B1: u32 = 4u;
const F_NA: u32 = 8u;
const F_NB: u32 = 16u;
const F_VBLEND: u32 = 32u;
const F_MASK: u32 = 64u;
const F_FADE: u32 = 256u;
const F_UNLIT: u32 = 512u;
const F_NOFLIP: u32 = 1024u;

fn decode_normal(t: vec4<f32>) -> vec3<f32> {
    // DXT5nm: x in alpha, y in green.
    let xy = vec2<f32>(t.a, t.g) * 2.0 - 1.0;
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let uv = in.uv;
    let flags = params.flags;

    var ca = textureSample(tex_a0, samp, uv * params.scale_a.xy);
    var color = ca.rgb;
    var alpha = ca.a;
    if (flags & F_A1) != 0u {
        color = color * textureSample(tex_a1, samp, uv * params.scale_a.zw).rgb * 2.0;
    }
    color = color * params.tint_a.rgb;

    var w = 0.0;
    if (flags & F_VBLEND) != 0u {
        w = clamp(in.color.a, 0.0, 1.0);
        if (flags & F_B0) != 0u {
            var cb = textureSample(tex_b0, samp, uv * params.scale_b.xy).rgb;
            if (flags & F_B1) != 0u {
                cb = cb * textureSample(tex_b1, samp, uv * params.scale_b.zw).rgb * 2.0;
            }
            cb = cb * params.tint_b.rgb;
            color = mix(color, cb, w);
        }
    }

    if (flags & F_MASK) != 0u && alpha < params.misc.z {
        discard;
    }
    if (flags & F_FADE) != 0u {
        let dist = distance(in.world_position.xyz, view.world_position.xyz);
        let fade = 1.0 - smoothstep(params.misc.x, params.misc.y, dist);
        if fade < ign(in.position.xy) {
            discard;
        }
    }

    var n_geo = normalize(in.world_normal);
    if !is_front && (flags & F_NOFLIP) == 0u {
        n_geo = -n_geo;
    }
    var n = n_geo;
    if (flags & F_NA) != 0u {
        var nt = decode_normal(textureSample(tex_na, samp, uv * params.scale_n.xy));
        if (flags & F_NB) != 0u && (flags & F_VBLEND) != 0u {
            let nt_b = decode_normal(textureSample(tex_nb, samp, uv * params.scale_n.zw));
            nt = normalize(mix(nt, nt_b, w));
        }
        nt = vec3<f32>(nt.xy * params.normal_strength, nt.z);
        nt.y = nt.y * params.misc.w;
        let t = normalize(in.world_tangent.xyz);
        let b = cross(n_geo, t) * in.world_tangent.w;
        n = normalize(t * nt.x + b * nt.y + n_geo * nt.z);
    }

    // Original Gothic 3 lighting (ORIGINAL_SHADING.md): evaluated in gamma space on the raw
    // texture values, ambient = linear SH(N) * (0.75 + 0.25 N.V) * vertex AO, plus
    // sun diffuse * N.L * shadow, plus lightmap * 4; fog is applied afterwards.
    let ao = mix(1.0, in.color.r, params.ao_strength);
    let albedo_g = linear_to_srgb(color);
    let v = normalize(view.world_position.xyz - in.world_position.xyz);
    let ndv = max(dot(n, v), 0.0);
    let sh = params.sh;
    let amb = sh[0].rgb + sh[1].rgb * n.x + sh[2].rgb * n.y + sh[3].rgb * n.z;
    var light = amb * (0.75 + 0.25 * ndv) * ao;

    var pbr = pbr_input_new();
    pbr.material.base_color = vec4<f32>(color, 1.0);
    pbr.material.flags = STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.flags = mesh[in.instance_index].flags;
    pbr.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr.V = v;
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    pbr.world_normal = n_geo;
    pbr.N = n;

    let sun_l = lights.directional_lights[0].direction_to_light;
    var shadow = 1.0;
    if (pbr.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
        && (lights.directional_lights[0].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
        let view_z = dot(vec4<f32>(
            view.view_from_world[0].z, view.view_from_world[1].z,
            view.view_from_world[2].z, view.view_from_world[3].z), in.world_position);
        shadow = shadows::fetch_directional_shadow(0u, in.world_position, n_geo, view_z, in.position.xy);
    }
    light = light + params.sun.rgb * max(dot(n, sun_l), 0.0) * shadow;

    // Baked static lights (xlmp per-vertex colour, packed r/b in uv_b): lightmap * gain.
    let lm = in.uv_b;
    light = light + vec3<f32>(lm.x, sqrt(max(lm.x * lm.y, 0.0)), lm.y) * params.sun.w;

    var out: FragmentOutput;
    if (flags & F_UNLIT) != 0u {
        out.color = vec4<f32>(color, 1.0);
    } else {
        out.color = vec4<f32>(srgb_to_linear(albedo_g * light), 1.0);
    }
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
