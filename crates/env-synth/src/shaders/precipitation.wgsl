// Particle-free precipitation overlay: procedural rain streaks, snow flakes and blowing sand
// drawn on a full-screen UI node. Three parallax layers each.

struct Precip {
    amount: vec4<f32>, // rain, snow, sand, seconds
    wind: vec4<f32>,   // x: screen-space wind (-1..1 at 20 m/s), y: lightning, z: aspect (w/h)
    sand_color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> p: Precip;

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

fn hash2(v: vec2<f32>) -> f32 {
    return fract(sin(dot(v, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(v: vec2<f32>) -> f32 {
    let i = floor(v);
    let f = fract(v);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

fn rain_layer(uv: vec2<f32>, t: f32, scale: f32, speed: f32, density: f32) -> f32 {
    var q = uv * vec2<f32>(scale * 1.6, scale * 0.25);
    q.x = q.x + uv.y * p.wind.x * scale * 0.5;
    q.y = q.y - t * speed;
    let col = floor(q.x);
    let r = hash1(col);
    if r > density {
        return 0.0;
    }
    let local = fract(vec2<f32>(q.x, q.y + r * 13.0));
    let streak = smoothstep(0.06, 0.0, abs(local.x - 0.5)) * smoothstep(0.0, 0.3, local.y)
        * smoothstep(0.75, 0.4, local.y);
    return streak;
}

fn snow_layer(uv: vec2<f32>, t: f32, scale: f32, speed: f32, density: f32) -> f32 {
    var q = uv * scale;
    q.y = q.y - t * speed;
    q.x = q.x - t * p.wind.x * speed * 0.8 + sin(q.y * 1.3 + t) * 0.15;
    let cell = floor(q);
    if hash2(cell) > density {
        return 0.0;
    }
    let centre = vec2<f32>(hash2(cell + 3.1), hash2(cell + 7.7)) * 0.6 + 0.2;
    let d = length(fract(q) - centre);
    return smoothstep(0.08, 0.0, d);
}

@fragment
fn fragment(
    @location(0) uv_in: vec2<f32>,
) -> @location(0) vec4<f32> {
    let t = p.amount.w;
    let uv = vec2<f32>(uv_in.x * p.wind.z, uv_in.y);
    var rgb = vec3<f32>(0.0);
    var a = 0.0;

    let rain = p.amount.x;
    if rain > 0.001 {
        let r = rain_layer(uv, t, 60.0, 3.2, rain) * 0.35
            + rain_layer(uv + 0.37, t, 38.0, 2.4, rain) * 0.25
            + rain_layer(uv + 0.71, t, 22.0, 1.6, rain) * 0.18;
        rgb = rgb + vec3<f32>(0.75, 0.8, 0.9) * r;
        a = a + r * 0.9 + rain * 0.06;
    }

    let snow = p.amount.y;
    if snow > 0.001 {
        let s = snow_layer(uv, t, 26.0, 0.12, snow) + snow_layer(uv + 0.5, t, 16.0, 0.08, snow) * 0.8
            + snow_layer(uv + 0.23, t, 9.0, 0.05, snow) * 0.6;
        rgb = rgb + vec3<f32>(0.95, 0.97, 1.0) * s;
        a = a + s * 0.9 + snow * 0.05;
    }

    let sand = p.amount.z;
    if sand > 0.001 {
        let flow = vec2<f32>(uv.x * 3.0 - t * (0.6 + abs(p.wind.x) * 1.5), uv.y * 14.0);
        let streaks = noise(flow) * 0.6 + noise(flow * 2.7 + 5.0) * 0.4;
        let haze = sand * (0.35 + 0.35 * streaks);
        rgb = rgb + p.sand_color.rgb * haze;
        a = a + haze;
    }

    let flash = p.wind.y;
    rgb = rgb + vec3<f32>(flash * 0.4);
    a = a + flash * 0.25;

    a = clamp(a, 0.0, 0.95);
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(rgb / max(a, 0.001), a);
}
