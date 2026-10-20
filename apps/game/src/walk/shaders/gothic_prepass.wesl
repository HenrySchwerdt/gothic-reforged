// Depth/shadow prepass for alpha-tested Gothic surfaces.
import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct GothicParams {
    tint_a: vec4<f32>,
    tint_b: vec4<f32>,
    scale_a: vec4<f32>,
    scale_b: vec4<f32>,
    scale_n: vec4<f32>,
    misc: vec4<f32>,
    flags: u32,
    ao_strength: f32,
    normal_strength: f32,
    roughness: f32,
};

@group(3) @binding(0) var<uniform> params: GothicParams;
@group(3) @binding(1) var tex_a0: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let a = textureSample(tex_a0, samp, in.uv * params.scale_a.xy).a;
    if a < params.misc.z {
        discard;
    }
    var out: FragmentOutput;
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
    let a = textureSample(tex_a0, samp, in.uv * params.scale_a.xy).a;
    if a < params.misc.z {
        discard;
    }
}
