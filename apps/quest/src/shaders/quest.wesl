// Cheap Quest surface shader implementing the original Gothic 3 lighting core (docs/research/ORIGINAL_SHADING.md):
// gamma-space albedo (textures are sampled as raw UNORM, no sRGB decode), SH ambient * AO * (0.75 + 0.25 N.V) + sun
// N.L, optional second layer blended by vertex alpha, linear distance fog, final gamma 2.2 -> linear for the sRGB
// swapchain. No PBR, no shadows, no clustering.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};
import bevy_pbr::render::mesh_view_bindings::view;

struct QParams {
    tint: vec4<f32>,
    uv_scale: vec4<f32>,
    uv_scale_b: vec4<f32>,
    tint_b: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    sh: array<vec4<f32>, 4>,
    fog_col: vec4<f32>,
    // x: alpha cutoff (0 = opaque), y: 1 = blend second layer by vertex alpha, z fog start, w fog end (m)
    misc: vec4<f32>,
    // detail (x2 modulated) maps: uv scale a.xy / b.zw; flags x = detail on layer a, y = layer b
    uv_scale_d: vec4<f32>,
    flags: vec4<f32>,
};

@group(3) @binding(0) var<uniform> p: QParams;
@group(3) @binding(1) var tex: texture_2d<f32>;
@group(3) @binding(2) var samp: sampler;
@group(3) @binding(3) var tex_b: texture_2d<f32>;
@group(3) @binding(4) var detail_a: texture_2d<f32>;
@group(3) @binding(5) var detail_b: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let c = textureSample(tex, samp, in.uv * p.uv_scale.xy);
    if p.misc.x > 0.0 && c.a < p.misc.x {
        discard;
    }
    var albedo = c.rgb * p.tint.rgb;
    if p.flags.x > 0.5 {
        albedo = albedo * textureSample(detail_a, samp, in.uv * p.uv_scale_d.xy).rgb * 2.0;
    }
    if p.misc.y > 0.5 {
        var cb = textureSample(tex_b, samp, in.uv * p.uv_scale_b.xy).rgb * p.tint_b.rgb;
        if p.flags.y > 0.5 {
            cb = cb * textureSample(detail_b, samp, in.uv * p.uv_scale_d.zw).rgb * 2.0;
        }
        albedo = mix(albedo, cb, clamp(in.color.a, 0.0, 1.0));
    }
    var n = normalize(in.world_normal);
    if !front {
        n = -n;
    }
    let to_cam = view.world_position.xyz - in.world_position.xyz;
    let dist = length(to_cam);
    let ndv = max(dot(n, to_cam / max(dist, 0.001)), 0.0);
    let amb = p.sh[0].rgb + p.sh[1].rgb * n.x + p.sh[2].rgb * n.y + p.sh[3].rgb * n.z;
    let ao = in.color.r;
    let light = amb * (ao * (0.25 * ndv + 0.75)) + p.sun_col.rgb * max(dot(n, p.sun_dir.xyz), 0.0);
    var g = albedo * light;
    let fog = clamp((dist - p.misc.z) / max(p.misc.w - p.misc.z, 1.0), 0.0, 1.0);
    g = mix(g, p.fog_col.rgb, fog);
    var out: FragmentOutput;
    out.color = vec4<f32>(pow(clamp(g, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)), 1.0);
    return out;
}
