// Shared deformation for Gothic color and depth/shadow prepass vertex stages.
const TAU: f32 = 6.28318530718;
const PI: f32 = 3.14159265359;

fn hash11(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// Port of `SpeedTreeWind::gust` (crates/environment/src/wind.rs): one gust per slot, random
// start, duration and strength inside the `GustStrength` / `GustDuration` ranges.
fn gust(wind: array<vec4<f32>, 7>, t: f32) -> f32 {
    let period = wind[0].y;
    let slot = floor(t / period);
    var g = 0.0;
    for (var k = 0; k < 2; k = k + 1) {
        let s = slot - 1.0 + f32(k);
        let start = (s + hash11(s)) * period;
        let dur = max(mix(wind[0].z, wind[0].w, hash11(s + 17.0)), 0.01);
        let str = mix(wind[1].x, wind[1].y, hash11(s + 41.0));
        let u = (t - start) / dur;
        if u > 0.0 && u < 1.0 {
            g = max(g, str * sin(PI * u));
        }
    }
    return g;
}

// Displacement (world metres) of a vertex: vegetation wave + grass response (mode 1) or the
// SpeedTree sway (modes 2 and 3). `weight` is vertex alpha, `m` the model matrix.
fn wind_offset(
    wp: vec3<f32>,
    normal: vec3<f32>,
    weight: f32,
    m: mat4x4<f32>,
    wind_mat: vec4<f32>,
    veg: vec4<f32>,
    veg_amp: vec4<f32>,
    player: vec4<f32>,
    wind: array<vec4<f32>, 7>,
    t: f32,
) -> vec3<f32> {
    let mode = wind_mat.x;
    if mode < 0.5 || weight <= 0.0 {
        return vec3<f32>(0.0);
    }
    if mode < 1.5 {
        // ge_vegetation.fx "APPLAY WIND" (cm formulas evaluated on world centimetres).
        let x = wp.x * 100.0;
        let z = wp.z * 100.0;
        let p1 = t * veg.z;
        let p2 = t * veg.w;
        let a = veg_amp.x * 0.01;
        let b = veg_amp.y * 0.01;
        var off = vec3<f32>(0.0);
        off.z = weight * (a * cos(z * veg.x + p1) + b * sin(z * veg.y + p2));
        off.x = weight * (a * sin(x * veg.x + p1) + b * cos(x * veg.y + p2));
        if player.w > 0.5 {
            let range = veg_amp.z * 0.01;
            let push = veg_amp.w * 0.01;
            let d = (wp + off) - (player.xyz + vec3<f32>(0.0, range, 0.0));
            let dist = length(d);
            if dist > 0.0001 {
                let fall = (range - min(range, dist)) / range;
                off.x = off.x + d.x / dist * fall * push;
                off.z = off.z + d.z / dist * fall * push;
            }
        }
        return off;
    }
    let base_s = clamp(wind[0].x, 0.0, 1.0);
    let s = clamp(wind[0].x + gust(wind, t), 0.0, 1.0);
    let seed = hash11(m[3].x * 0.137 + m[3].z * 0.173);
    let height = wind_mat.y * 0.01 * length(m[1].xyz);
    let bend = sin(wind[1].z * mix(wind[2].x, wind[2].y, s) * pow(max(s, 0.0001), wind[1].w));
    // Fixed base-strength rate avoids the elapsed-time phase drift of frequency(s) * t.
    let hz = clamp(mix(wind[3].x, wind[3].y, base_s) / 100.0, 0.1, 4.0);
    let osc = 0.75 + 0.25 * sin(TAU * (hz * t + seed));
    let vib = sin(mix(wind[2].z, wind[2].w, s)) * sin(TAU * (hz * 3.0 * t + seed * 7.0));
    let lim = 0.35 * height * weight;
    let sway = clamp(height * weight * (bend * osc + vib), -lim, lim);
    let dir = vec3<f32>(sin(wind[3].z), 0.0, cos(wind[3].z));
    var off = dir * sway;
    if mode > 2.5 {
        let k = pow(max(s, 0.0001), wind[3].w);
        let rock = mix(wind[4].x, wind[4].y, k) / max(wind[4].y, 0.001)
            * mix(wind[4].z, wind[4].w, k);
        let rust = mix(wind[5].x, wind[5].y, k) / max(wind[5].y, 0.001)
            * mix(wind[5].z, wind[5].w, k);
        let f_rock = mix(wind[6].x, wind[6].y, base_s) / 100.0;
        let f_rust = mix(wind[6].z, wind[6].w, base_s) / 100.0;
        let ph = fract(dot(wp.xz, vec2<f32>(0.37, 0.29)) + seed);
        let flutter = wind_mat.z * 0.5
            * (rock * sin(TAU * (f_rock * t + ph)) + rust * sin(TAU * (f_rust * t + ph * 3.0)));
        off = off + normal * flutter;
    }
    return off;
}
