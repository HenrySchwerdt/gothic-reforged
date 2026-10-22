// Quest ground-cover shader (apps/quest/src/vegetation.rs). Vertex stage: the original vegetation wave
// (`ge_vegetation.fx` "APPLAY WIND", constants of `eCVegetationAdmin::Invalidate` 0x10279e10) and the grass response
// to the player (`Render.GrassResponseRange/Intensity` of ge3.INI). The vertex alpha carries the wind weight
// `((y - y_min) / height)^2 * WindStrength` (`eCVegetation_GridNode::CacheInGeometry` 0x1027d830).
// Fragment stage: the same original gamma-space lighting as quest.wesl, alpha test, and the vegetation dissolve between
// the fade start and the view range (`eCVegetation_Grid::Render` 0x1027c9a0) done as an ordered dither (alpha-tested
// pass only: no blending, no sorting).
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{view, globals},
};

struct VParams {
    // rgb: material tint
    tint: vec4<f32>,
    // xy: uv scale
    uv_scale: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    sh: array<vec4<f32>, 4>,
    fog_col: vec4<f32>,
    // x: alpha cutoff (0 = opaque), y: dissolve start (m), z: dissolve end = view range (m), w: unused
    misc: vec4<f32>,
    // x: fog start (m), y: fog end (m)
    fog: vec4<f32>,
    // vegetation wave: xy angular wave numbers (1/cm), zw phase rates (rad/s)
    veg: vec4<f32>,
    // xy wave amplitudes (cm), z grass response range (cm), w grass response intensity (cm)
    veg_amp: vec4<f32>,
    // x: eye height above the ground (m), used to find the player's feet for the grass response
    player: vec4<f32>,
};

@group(3) @binding(0) var<uniform> p: VParams;
@group(3) @binding(1) var tex: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_no_morph, vertex_no_morph.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    var weight = 0.0;
@if(VERTEX_COLORS)
    weight = vertex.color.a;

@if(VERTEX_NORMALS) {
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
}

@if(VERTEX_POSITIONS) {
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    if weight > 0.0 {
        // cm formulas evaluated on world centimetres
        let x = wp.x * 100.0;
        let z = wp.z * 100.0;
        let t = globals.time;
        let p1 = t * p.veg.z;
        let p2 = t * p.veg.w;
        let a = p.veg_amp.x * 0.01;
        let b = p.veg_amp.y * 0.01;
        var off = vec3<f32>(0.0);
        off.z = weight * (a * cos(z * p.veg.x + p1) + b * sin(z * p.veg.y + p2));
        off.x = weight * (a * sin(x * p.veg.x + p1) + b * cos(x * p.veg.y + p2));
        // Grass response: pushed away from a point `range` above the player's feet, linear fall-off.
        let range = p.veg_amp.z * 0.01;
        let push = p.veg_amp.w * 0.01;
        let feet = vec3<f32>(view.world_position.x, view.world_position.y - p.player.x, view.world_position.z);
        let d = (wp.xyz + off) - (feet + vec3<f32>(0.0, range, 0.0));
        let dist = length(d);
        if dist > 0.0001 && dist < range {
            let fall = (range - dist) / range;
            off.x = off.x + d.x / dist * fall * push;
            off.z = off.z + d.z / dist * fall * push;
        }
        wp = vec4<f32>(wp.xyz + off, wp.w);
    }
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

fn ign(q: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(q, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let c = textureSample(tex, samp, in.uv * p.uv_scale.xy);
    if p.misc.x > 0.0 && c.a < p.misc.x {
        discard;
    }
    let to_cam = view.world_position.xyz - in.world_position.xyz;
    let dist = length(to_cam);
    // Dissolve: 1 before the start, 0 at the view range (render-rules vegetation::dissolve), dithered.
    let fade = clamp(1.0 - (dist - p.misc.y) / max(p.misc.z - p.misc.y, 0.001), 0.0, 1.0);
    if fade < ign(in.position.xy) {
        discard;
    }
    let albedo = c.rgb * p.tint.rgb;
    var n = normalize(in.world_normal);
    if !front {
        n = -n;
    }
    let ndv = max(dot(n, to_cam / max(dist, 0.001)), 0.0);
    let amb = p.sh[0].rgb + p.sh[1].rgb * n.x + p.sh[2].rgb * n.y + p.sh[3].rgb * n.z;
    let ao = in.color.r;
    let light = amb * (ao * (0.25 * ndv + 0.75)) + p.sun_col.rgb * max(dot(n, p.sun_dir.xyz), 0.0);
    var g = albedo * light;
    let f = clamp((dist - p.fog.x) / max(p.fog.y - p.fog.x, 1.0), 0.0, 1.0);
    g = mix(g, p.fog_col.rgb, f);
    var out: FragmentOutput;
    out.color = vec4<f32>(pow(clamp(g, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)), 1.0);
    return out;
}
