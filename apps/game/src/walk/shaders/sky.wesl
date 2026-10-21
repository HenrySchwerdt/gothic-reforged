// Procedural sky dome: zenith/horizon gradient, warm haze toward the sun, sun disc, a few
// layered clouds. Unlit; goes through the standard tonemapping/debanding epilogue only.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::pbr_input_new,
};

struct SkyParams {
    // xyz direction to the sun
    sun_dir: vec4<f32>,
    // colSky (eCSkydome SkyColor), w = cloud thickness
    zenith: vec4<f32>,
    // colHaze (HazeColor)
    horizon: vec4<f32>,
    // fog / below-horizon colour, w > 0.5: cloud texture bound
    ground: vec4<f32>,
    // colSun (sun colour faded by height), w unused
    sun_color: vec4<f32>,
    // colClouds
    cloud: vec4<f32>,
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
    // ge_skydome.fx VS: haze weight from the elevation, sky colour lit by the sun angle
    // (0.5 * (1.3 + d.s)), sun lobe pow(d.s, 300) * 0.5. The raw haze ratio of the original
    // exceeds 1 over the whole dome, so the weight is reconstructed as (ratio - 1) * 2 [I].
    let y = clamp(h, 0.0, 1.0);
    let sun_amount = max(dot(dir, sun), 0.0);
    let ratio = 0.3 * (1.0 - pow(y, 0.7)) / max(0.2 * (1.0 - y), 0.00001);
    let hz = clamp((ratio - 1.0) * 2.0, 0.0, 1.0);
    let k = 0.5 * (1.3 + dot(dir, sun));
    var col = mix(sky.zenith.rgb * k, sky.horizon.rgb, hz);
    col = col + sky.sun_color.rgb * 0.5 * pow(sun_amount, 300.0);
    // Sun disc.
    let disc = smoothstep(0.9994, 0.9998, sun_amount);
    col = mix(col, sky.sun_color.rgb * 3.0, disc * step(0.0, sun.y));

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
        let lo = 0.62 + (0.45 - sky.zenith.w) * 0.6;
        let cover = smoothstep(lo, lo + 0.33, d);
        let shade = mix(vec3<f32>(0.7, 0.74, 0.85), vec3<f32>(1.0, 0.99, 0.97), smoothstep(0.45, 0.9, tone));
        let lit = sky.cloud.rgb * shade * (0.85 + 0.3 * pow(sun_amount, 3.0));
        col = mix(col, lit, cover * smoothstep(0.015, 0.2, h));
    }
    // Below the horizon blend into the ground/fog colour.
    col = mix(col, sky.ground.rgb, smoothstep(0.0, -0.12, h));

    var pbr = pbr_input_new();
    var out: FragmentOutput;
    // The values are gamma-space like every colour of the original; convert for the linear target.
    let lin = select(pow((max(col, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4)), col / 12.92, col <= vec3<f32>(0.04045));
    out.color = main_pass_post_lighting_processing(pbr, vec4<f32>(lin, 1.0));
    return out;
}
