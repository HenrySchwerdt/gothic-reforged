// Sky dome: gradient, sun glow + disc, moon, stars and a procedural cloud layer.
// Uniforms come from `EnvFrame` (see plugin.rs); everything is linear RGB.

struct Sky {
    zenith: vec4<f32>,       // rgb, w: star visibility
    horizon: vec4<f32>,      // rgb, w: cloud cover
    sun_dir: vec4<f32>,      // xyz, w: sun disc visible (0/1)
    sun_glow: vec4<f32>,     // rgb, w: lightning flash
    moon_dir: vec4<f32>,     // xyz, w: moon brightness
    cloud_color: vec4<f32>,  // rgb, w: cloud darkness
    center: vec4<f32>,       // dome centre (camera) xyz, w: seconds
    cloud_offset: vec4<f32>, // xy: cloud drift in metres
};

@group(3) @binding(0) var<uniform> sky: Sky;

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn hash3(p: vec3<f32>) -> f32 {
    return hash2(p.xy + vec2<f32>(p.z * 17.13, p.z * 3.71));
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 5; i = i + 1) {
        sum = sum + amp * value_noise(q);
        q = q * 2.03 + vec2<f32>(1.7, 9.2);
        amp = amp * 0.5;
    }
    return sum;
}

@fragment
fn fragment(
    @builtin(position) frag_coord: vec4<f32>,
    @location(0) world_position: vec4<f32>,
) -> @location(0) vec4<f32> {
    let dir = normalize(world_position.xyz - sky.center.xyz);
    let up = max(dir.y, 0.0);
    var col = mix(sky.horizon.rgb, sky.zenith.rgb, pow(up, 0.45));
    if dir.y < 0.0 {
        col = sky.horizon.rgb * (1.0 + dir.y * 0.6);
    }

    // Sun glow and disc.
    let sd = max(dot(dir, sky.sun_dir.xyz), 0.0);
    let sun_above = smoothstep(-0.1, 0.02, sky.sun_dir.y);
    col = col + sky.sun_glow.rgb * (pow(sd, 8.0) * 0.35 + pow(sd, 64.0) * 0.5) * sun_above;
    col = col + sky.sun_glow.rgb * smoothstep(0.9994, 0.9997, sd) * 30.0 * sky.sun_dir.w;

    // Moon disc.
    let md = max(dot(dir, sky.moon_dir.xyz), 0.0);
    col = col + vec3<f32>(0.8, 0.85, 1.0) * smoothstep(0.9993, 0.9996, md) * sky.moon_dir.w;

    // Stars.
    if sky.zenith.w > 0.0 && dir.y > 0.0 {
        let cell = floor(dir * 300.0);
        let s = step(0.9985, hash3(cell));
        col = col + vec3<f32>(s * sky.zenith.w * smoothstep(0.0, 0.3, dir.y));
    }

    // Cloud layer: a plane 1000 m up, drifting with the wind.
    let cover = sky.horizon.w;
    if dir.y > 0.01 && cover > 0.0 {
        let plane = dir.xz / dir.y * 1000.0 + sky.cloud_offset.xy;
        let n = fbm(plane / 1800.0);
        let density = smoothstep(1.0 - cover, 1.0 - cover + 0.3, n + cover * 0.35);
        let fade = smoothstep(0.01, 0.2, dir.y);
        let shade = sky.cloud_color.rgb * (1.0 - sky.cloud_color.w * 0.6 * n)
            + sky.sun_glow.rgb * pow(sd, 6.0) * 0.25 * sun_above;
        col = mix(col, shade, density * fade);
    }

    col = col + vec3<f32>(sky.sun_glow.w * 0.6);
    return vec4<f32>(col, 1.0);
}
