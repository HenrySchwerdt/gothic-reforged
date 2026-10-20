// Alpha-tested shadow/depth prepass for characters.
import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct CharParams {
    tint: vec4<f32>,
    misc: vec4<f32>,
};
@group(3) @binding(0) var<uniform> params: CharParams;
@group(3) @binding(1) var tex: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let a = textureSample(tex, samp, in.uv).a * params.tint.a;
    if params.misc.x > 0.0 && a < params.misc.x {
        discard;
    }
    var out: FragmentOutput;
    return out;
}

@if(!PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) {
}
