// Color-matched wind deformation and alpha test for depth/shadow prepasses.
import bevy_pbr::prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex};
import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
};
@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;
import bevy_render::globals::Globals;
import package::walk::shaders::gothic_wind::wind_offset;

// Bevy's prepass view layout places Globals at binding 1; the forward layout uses 11.
@group(0) @binding(1) var<uniform> globals: Globals;

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
    sun: vec4<f32>,
    sh: array<vec4<f32>, 4>,
    uv_d_scroll: vec4<f32>,
    uv_d_rot: vec4<f32>,
    uv_n_scroll: vec4<f32>,
    uv_n_rot: vec4<f32>,
    wind_mat: vec4<f32>,
    veg: vec4<f32>,
    veg_amp: vec4<f32>,
    player: vec4<f32>,
    wind: array<vec4<f32>, 7>,
};

@group(3) @binding(0) var<uniform> params: GothicParams;
@group(3) @binding(1) var tex_a0: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_no_morph, vertex_no_morph.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    var weight = 0.0;
    var wnormal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_COLORS)
    weight = vertex.color.a;

// The custom wind depth/shadow layout supplies Vertex.normal with both defines; transform it
// even when the pass has no normal output, so leaf flutter matches the color pass.
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS) {
    wnormal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {
    out.world_normal = wnormal;
}

    var wp = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );
    let off = wind_offset(
        wp.xyz,
        wnormal,
        weight,
        world_from_local,
        params.wind_mat,
        params.veg,
        params.veg_amp,
        params.wind,
        false,
    );
    wp = vec4<f32>(wp.xyz + off, wp.w);
    out.world_position = wp;
    out.position = position_world_to_clip(wp.xyz);

@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}

@if(MOTION_VECTOR_PREPASS) {
    let previous_world_from_local =
        mesh_functions::get_previous_world_from_local(vertex_no_morph.instance_index);
    var previous_wp = mesh_functions::mesh_position_local_to_world(
        previous_world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );
    let previous_off = wind_offset(
        previous_wp.xyz,
        wnormal,
        weight,
        previous_world_from_local,
        params.wind_mat,
        params.veg,
        params.veg_amp,
        params.wind,
        true,
    );
    previous_wp = vec4<f32>(previous_wp.xyz + previous_off, previous_wp.w);
    out.previous_world_position = previous_wp;
}

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_no_morph.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index,
        world_from_local[3]
    );

    return out;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

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
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    let a = textureSample(tex_a0, samp, in.uv * params.scale_a.xy).a;
    if a < params.misc.z {
        discard;
    }
}
