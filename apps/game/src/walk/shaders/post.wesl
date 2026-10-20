// Original Gothic 3 "Bloom" post chain (Performance=High, IP_MaskHighLights.fx, IP_Blur.fx,
// IP_HDRI.fx PS_Bloom): mask = c * max(lum - threshold, 0) * intensity, Gaussian blur,
// then scene += bloom * (1 - scene) with a luminance-scaled red/blue chromatic offset.
// All maths runs on the gamma-space (display) values like the original.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct PostParams {
    // x threshold, y intensity (multiplier), z aberration, w blur step in pixels
    bloom: vec4<f32>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: PostParams;

fn to_g(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
fn to_l(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

fn mask(uv: vec2<f32>) -> vec3<f32> {
    let c = to_g(textureSampleLevel(scene, samp, uv, 0.0).rgb);
    let l = dot(c, vec3<f32>(0.3, 0.59, 0.11));
    return saturate(c * max(l - params.bloom.x, 0.0) * params.bloom.y);
}

// IP_Blur.fx 7-tap linear-sampled Gaussian (offsets 1.4118 / 3.2941 / 5.1765), applied here
// as a 5x5 tap subset of the separable kernel in one pass (3x3 for the chroma-shifted lookups).
fn blurred(uv: vec2<f32>) -> vec3<f32> {
    let step = params.bloom.w / vec2<f32>(textureDimensions(scene));
    var offs = array<f32, 5>(0.0, 1.4117647, -1.4117647, 3.2941177, -3.2941177);
    var w = array<f32, 5>(0.1964826, 0.2969070, 0.2969070, 0.0944704, 0.0944704);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        for (var j = 0; j < 5; j = j + 1) {
            let ww = w[i] * w[j];
            acc = acc + mask(uv + vec2<f32>(offs[i], offs[j]) * step) * ww;
            wsum = wsum + ww;
        }
    }
    return acc / wsum;
}

fn blurred_small(uv: vec2<f32>) -> vec3<f32> {
    let step = 2.0 * params.bloom.w / vec2<f32>(textureDimensions(scene));
    var acc = mask(uv) * 0.2964;
    for (var i = -1; i <= 1; i = i + 1) {
        for (var j = -1; j <= 1; j = j + 1) {
            if i != 0 || j != 0 {
                let k = select(0.1, 0.0952, i != 0 && j != 0);
                acc = acc + mask(uv + vec2<f32>(f32(i), f32(j)) * step) * k;
            }
        }
    }
    return acc / (0.2964 + 4.0 * 0.1 + 4.0 * 0.0952);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = to_g(textureSampleLevel(scene, samp, in.uv, 0.0).rgb);
    var b = blurred(in.uv);
    let lum = dot(b, vec3<f32>(0.3, 0.59, 0.11));
    let chroma = params.bloom.z * lum * (1.0 / vec2<f32>(textureDimensions(scene)));
    b.r = blurred_small(in.uv + chroma).r;
    b.b = blurred_small(in.uv - chroma).b;
    c = c + b * (1.0 - c);
    return vec4<f32>(to_l(saturate(c)), 1.0);
}
