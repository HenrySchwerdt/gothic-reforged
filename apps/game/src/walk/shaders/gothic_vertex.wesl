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
    // y: tree height in local units, z: leaf card size (m), w: unused.
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

const TAU: f32 = 6.28318530718;
const PI: f32 = 3.14159265359;

fn hash11(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// Port of `SpeedTreeWind::gust` (crates/environment/src/wind.rs): one gust per slot, random
// start, duration and strength inside the `GustStrength` / `GustDuration` ranges.
fn gust(t: f32) -> f32 {
    let period = params.wind[0].y;
    let slot = floor(t / period);
    var g = 0.0;
    for (var k = 0; k < 2; k = k + 1) {
        let s = slot - 1.0 + f32(k);
        let start = (s + hash11(s)) * period;
        let dur = max(mix(params.wind[0].z, params.wind[0].w, hash11(s + 17.0)), 0.01);
        let str = mix(params.wind[1].x, params.wind[1].y, hash11(s + 41.0));
        let u = (t - start) / dur;
        if u > 0.0 && u < 1.0 {
            g = max(g, str * sin(PI * u));
        }
    }
    return g;
}

// Displacement (world metres) of a vertex: vegetation wave + grass response (mode 1) or the
// SpeedTree sway (modes 2 and 3). `weight` is the vertex wind weight, `m` the model matrix.
fn wind_offset(wp: vec3<f32>, normal: vec3<f32>, weight: f32, m: mat4x4<f32>, t: f32) -> vec3<f32> {
    let mode = params.wind_mat.x;
    if mode < 0.5 || weight <= 0.0 {
        return vec3<f32>(0.0);
    }
    if mode < 1.5 {
        // ge_vegetation.fx "APPLAY WIND" (cm formulas evaluated on world centimetres).
        let x = wp.x * 100.0;
        let z = wp.z * 100.0;
        let p1 = t * params.veg.z;
        let p2 = t * params.veg.w;
        let a = params.veg_amp.x * 0.01;
        let b = params.veg_amp.y * 0.01;
        var off = vec3<f32>(0.0);
        off.z = weight * (a * cos(z * params.veg.x + p1) + b * sin(z * params.veg.y + p2));
        off.x = weight * (a * sin(x * params.veg.x + p1) + b * cos(x * params.veg.y + p2));
        if params.player.w > 0.5 {
            let range = params.veg_amp.z * 0.01;
            let push = params.veg_amp.w * 0.01;
            let d = (wp + off) - (params.player.xyz + vec3<f32>(0.0, range, 0.0));
            let dist = length(d);
            if dist > 0.0001 {
                let fall = (range - min(range, dist)) / range;
                off.x = off.x + d.x / dist * fall * push;
                off.z = off.z + d.z / dist * fall * push;
            }
        }
        return off;
    }
    // SpeedTree: sway along the wind direction, weight = (y / H)^2 stored in the vertex alpha.
    let base_s = clamp(params.wind[0].x, 0.0, 1.0);
    let s = clamp(params.wind[0].x + gust(t), 0.0, 1.0);
    let seed = hash11(m[3].x * 0.137 + m[3].z * 0.173);
    let height = params.wind_mat.y * 0.01 * length(m[1].xyz);
    let bend = sin(params.wind[1].z * mix(params.wind[2].x, params.wind[2].y, s) * pow(max(s, 0.0001), params.wind[1].w));
    // Keep oscillator phase rate stable as gust strength changes. Using hz(s) * elapsed_time
    // makes its derivative depend on elapsed_time and creates increasingly severe phase jitter.
    let hz = clamp(mix(params.wind[3].x, params.wind[3].y, base_s) / 100.0, 0.1, 4.0);
    let osc = 0.75 + 0.25 * sin(TAU * (hz * t + seed));
    let vib = sin(mix(params.wind[2].z, params.wind[2].w, s)) * sin(TAU * (hz * 3.0 * t + seed * 7.0));
    let lim = 0.35 * height * weight;
    let sway = clamp(height * weight * (bend * osc + vib), -lim, lim);
    let dir = vec3<f32>(sin(params.wind[3].z), 0.0, cos(params.wind[3].z));
    var off = dir * sway;
    if mode > 2.5 {
        // Leaf rocking and rustling: flutter of the cards along their normal.
        let k = pow(max(s, 0.0001), params.wind[3].w);
        let rock = mix(params.wind[4].x, params.wind[4].y, k) / max(params.wind[4].y, 0.001)
            * mix(params.wind[4].z, params.wind[4].w, k);
        let rust = mix(params.wind[5].x, params.wind[5].y, k) / max(params.wind[5].y, 0.001)
            * mix(params.wind[5].z, params.wind[5].w, k);
        let f_rock = mix(params.wind[6].x, params.wind[6].y, base_s) / 100.0;
        let f_rust = mix(params.wind[6].z, params.wind[6].w, base_s) / 100.0;
        let ph = fract(dot(wp.xz, vec2<f32>(0.37, 0.29)) + seed);
        let flutter = params.wind_mat.z * 0.5
            * (rock * sin(TAU * (f_rock * t + ph)) + rust * sin(TAU * (f_rust * t + ph * 3.0)));
        off = off + normal * flutter;
    }
    return off;
}

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
    let off = wind_offset(wp.xyz, wnormal, weight, world_from_local, globals.time);
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
