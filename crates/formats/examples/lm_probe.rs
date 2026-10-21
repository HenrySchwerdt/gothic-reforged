//! Debug: print mesh submesh/uv2 stats. Usage: lm_probe <mesh basename>
use gothic_formats::{vfs::VirtualFileSystem, xcmsh::Xcmsh};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut v = VirtualFileSystem::open_data_dir(std::path::Path::new(
        "D:/SteamLibrary/steamapps/common/Gothic 3/Data",
    ))
    .unwrap();
    let s = v.resolve_name(&a[1]).expect("nf");
    let b = v.read(s).unwrap();
    let m = Xcmsh::parse(&b).unwrap();
    if let Some(p) = a.get(2) { dump_json(&m, p); }
    for (i, s) in m.submeshes.iter().enumerate() {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for u in &s.uv2 {
            for k in 0..2 {
                lo[k] = lo[k].min(u[k]);
                hi[k] = hi[k].max(u[k]);
            }
        }
        println!(
            "sub{i} v={} idx={} uv2={} lo={lo:?} hi={hi:?} mat={} uv0={:?}",
            s.positions.len(), s.indices.len(), s.uv2.len(), s.material, s.uvs.first()
        );
        println!("  uv2[0..6]={:?}", &s.uv2[..s.uv2.len().min(6)]); println!("  c5={:?}", s.colors5.iter().take(12).collect::<Vec<_>>()); println!("  c4={:?}", s.colors4.iter().take(6).collect::<Vec<_>>());
    }
}
#[allow(dead_code)]
fn unused() {}
pub fn dump_json(m: &Xcmsh, path: &str) {
    let mut o = String::from("[");
    for s in &m.submeshes {
        o += &format!("{{\"uv2\":{:?},\"i\":{:?},\"p\":{:?}}},", s.uv2, s.indices, s.positions);
    }
    o.pop();
    o += "]";
    std::fs::write(path, o).unwrap();
}
