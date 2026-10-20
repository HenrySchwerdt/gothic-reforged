// Procedural sky dome: zenith/horizon gradient, warm haze toward the sun, sun disc, a few
// layered clouds. Unlit; goes through the standard tonemapping/debanding epilogue only.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::pbr_input_new,
};

struct SkyParams {
    sun_dir: vec4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
    sun_color: vec4<f32>,
};
@group(3) @binding(0) var<uniform> sky: SkyParams;
@group(3) @binding(1) var clouds: texture_2d<f32>;
@group(3) @binding(2) var clouds_s: sampler;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn fbm(p: vec2<f32>) -> f32 {
    var a = 0.5;
    var s = 0.0;
    var q = p;
    for (var i = 0; i < 5; i = i + 1) {
        s = s + a * vnoise(q);
        q = q * 2.03 + vec2<f32>(17.1, 9.2);
        a = a * 0.5;
    }
    return s;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    let sun = normalize(sky.sun_dir.xyz);
    let h = dir.y;
    // Vertical gradient with a thicker haze band at the horizon.
    let t = pow(clamp(h, 0.0, 1.0), 0.55);
    var col = mix(sky.horizon.rgb, sky.zenith.rgb, t);
    let sun_amount = max(dot(dir, sun), 0.0);
    // Warm scattering toward the sun, strongest near the horizon.
    let haze = pow(sun_amount, 6.0) * (1.0 - 0.6 * clamp(h, 0.0, 1.0));
    col = col + sky.sun_color.rgb * 0.35 * haze;
    col = col + sky.sun_color.rgb * 0.6 * pow(sun_amount, 48.0);
    // Sun disc.
    let disc = smoothstep(0.9994, 0.9998, sun_amount);
    col = mix(col, sky.sun_color.rgb * 6.0, disc);

    // Clouds on a virtual plane above the viewer.
    if h > 0.015 {
        let plane = dir.xz / (h + 0.18);
        let tt = globals.time * 0.004;
        var d = 0.0;
        var tone = 0.0;
        if sky.ground.w > 0.5 {
            let lod = clamp((1.0 - h) * 4.0, 0.0, 5.0);
            let c0 = textureSampleLevel(clouds, clouds_s, plane * 0.16 + vec2<f32>(tt, tt * 0.4), lod).r;
            let c1 = textureSampleLevel(clouds, clouds_s, plane * 0.37 + vec2<f32>(0.37 - tt, 0.11), lod + 1.0).r;
            d = c0 * 0.7 + c1 * 0.5;
            tone = c0;
        } else {
            d = fbm(plane * 0.9 + vec2<f32>(tt, tt * 0.4)) * 1.4;
            tone = fbm(plane * 1.7 + 3.1);
        }
        let cover = smoothstep(0.62, 0.95, d);
        let shade = mix(vec3<f32>(0.62, 0.68, 0.80), vec3<f32>(1.0, 0.98, 0.94), smoothstep(0.45, 0.9, tone));
        let lit = shade * (0.9 + 0.3 * pow(sun_amount, 3.0));
        col = mix(col, lit, cover * smoothstep(0.015, 0.2, h));
    }
    // Below the horizon blend into the ground/fog colour.
    col = mix(col, sky.ground.rgb, smoothstep(0.0, -0.12, h));

    var pbr = pbr_input_new();
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, vec4<f32>(col, 1.0));
    return out;
}
