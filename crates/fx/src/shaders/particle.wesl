// Particle billboards (ge_particle.fx family): the sub-image texture sampled twice (two frames
// blended by the vertex-encoded weight), multiplied by the vertex colour. Unlit; fog optional.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::{pbr_input_new, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
};

struct FxParams {
    // x: alpha reference (0 = no alpha test), y: fog enabled, z: additive blend, w: unused
    misc: vec4<f32>,
};
@group(3) @binding(0) var<uniform> fx: FxParams;
@group(3) @binding(1) var tex: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // The second-frame weight travels in the normal's x (a unit vector, so it survives the
    // vertex stage's normalisation).
    let w = clamp(in.world_normal.x, 0.0, 1.0);
    let a = textureSample(tex, samp, in.uv);
    let b = textureSample(tex, samp, in.uv_b);
    var t = mix(a, b, w) * in.color;
    if fx.misc.x > 0.0 && t.a < fx.misc.x {
        discard;
    }
    var pbr = pbr_input_new();
    if fx.misc.y > 0.5 {
        pbr.material.flags = STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    }
    pbr.world_position = in.world_position;
    pbr.frag_coord = in.position;
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, t);
    if fx.misc.z > 0.5 {
        // Additive: the pipeline blends premultiplied, so add = colour * alpha with alpha 0.
        out.color = vec4<f32>(out.color.rgb * out.color.a, 0.0);
    }
    return out;
}
