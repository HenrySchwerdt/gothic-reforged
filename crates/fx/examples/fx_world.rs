use bevy::math::Vec3;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_fx::space::point_to_view;
use gothic_fx::world::{ScanArea, scan};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).unwrap();
    let origin = Vec3::new(92_400.0, 0.0, -11_000.0);
    let t = std::time::Instant::now();
    let w = scan(
        &mut vfs,
        &ScanArea {
            center: origin,
            radius: 20_000.0,
            document: &|p| p.contains("ardea") || p.contains("sysdyn"),
        },
    );
    println!(
        "{} emitters, {} effects in {:?}",
        w.emitters.len(),
        w.effects.len(),
        t.elapsed()
    );
    let mut effects = w.effects.clone();
    effects.sort_by(|x, y| {
        point_to_view(x.matrix.w_axis.truncate(), origin)
            .length()
            .total_cmp(&point_to_view(y.matrix.w_axis.truncate(), origin).length())
    });
    for e in effects
        .iter()
        .filter(|e| a.get(2).is_none_or(|n| e.effect.contains(n.as_str())))
        .take(25)
    {
        let p = point_to_view(e.matrix.w_axis.truncate(), origin) * 0.01;
        println!(
            "{:30} {:28} view m ({:.1}, {:.1}, {:.1}) static={}",
            e.name, e.effect, p.x, p.y, p.z, e.is_static
        );
    }
    for e in &w.emitters {
        let p = point_to_view(e.matrix.w_axis.truncate(), origin) * 0.01;
        println!("{} view m ({:.1},{:.1},{:.1})", e.name, p.x, p.y, p.z);
    }
}
