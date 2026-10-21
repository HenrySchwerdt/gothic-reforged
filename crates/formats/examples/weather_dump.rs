//! Debug: print the particle parameters of every weather state. Usage: weather_dump <Data dir>
use gothic_formats::vfs::VirtualFileSystem;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).expect("vfs");
    let slot = vfs
        .iter()
        .find(|(p, _)| p.to_ascii_lowercase().ends_with("weatherstates.g3ws"))
        .map(|(_, s)| s)
        .expect("g3ws");
    let data = gothic_formats::weather::parse(&vfs.read(slot).expect("read")).expect("parse");
    for s in data.states.iter().take(1) {
        for i in 0..=24 {
            let t = i as f32 / 24.0;
            println!(
                "t {t:.3} sky {:?} haze {:?} sunc {:?} cloud {:?} extraA {} extraB {} dens {}",
                s.scales.sky_color.at(t),
                s.scales.haze_color.at(t),
                s.scales.sun_diffuse.at(t),
                s.scales.cloud_color.at(t),
                s.scales.extra_a.at(t),
                s.scales.extra_b.at(t),
                s.scales.cloud_density.at(t)
            );
        }
    }
    for s in &data.states {
        println!(
            "{}: pps {} speed {:?} size {:?} dirscale {} tile {} dizzer {} jitter {} {} splash {} tile {} scale {:?} loop {:?} vol {}",
            s.name, s.particles_per_second, s.speed_min_max, s.size, s.direction_scale,
            s.current_texture_tile, s.spawn_point_dizzer, s.jitter_power, s.jitter_speed,
            s.splash_duration, s.splash_texture_tile, s.splash_size_scale, s.loop_sample, s.loop_volume
        );
    }
}

#[allow(dead_code)]
fn unused() {}
