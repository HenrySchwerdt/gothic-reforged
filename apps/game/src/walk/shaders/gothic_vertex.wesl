// Vertex stage of the Gothic surface material: Bevy's default mesh vertex stage plus the wind of
// the original vegetation shader (`ge_vegetation.fx`, `eCVegetationAdmin::SetWind`) and the
// SpeedTree wind (`eCSpeedTreeWind_PS`). Materials without wind (wind_mat.x == 0) take the
// plain path. The vertex alpha carries the wind weight of vegetation and tree meshes.
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
};
import package::walk::shaders::gothic_wind::wind_offset;

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
    // Wind (see gothic_vertex.wesl). x: mode (0 none, 1 vegetation wave, 2 tree, 3 tree leaves),
    // y: tree height in local units, z: leaf card size (m), w: leaf flutter scale (1 = default).
    wind_mat: vec4<f32>,
    // Vegetation wave: xy angular wave numbers (1/cm), zw phase rates (rad/s).
    veg: vec4<f32>,
    // xy wave amplitudes (cm), z grass response range (cm), w grass response intensity (cm).
    veg_amp: vec4<f32>,
    // Grass response focus (world metres), w = 1 when active.
    player: vec4<f32>,
    // SpeedTree wind parameters, see gmat.rs `GothicParams::wind`.
    wind: array<vec4<f32>, 7>,
};

@group(3) @binding(0) var<uniform> params: GothicParams;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_no_morph, vertex_no_morph.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    var weight = 0.0;
    var wnormal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_COLORS)
    weight = vertex.color.a;

@if(VERTEX_NORMALS) {
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
    wnormal = out.world_normal;
}

@if(VERTEX_POSITIONS) {
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
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
}

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );

@if(VERTEX_COLORS)
    out.color = vertex.color;

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_no_morph.instance_index;

@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, world_from_local[3]);

    return out;
}
