//! Debug: print per-vertex streams of an xcmsh. Usage: uv2_probe <file.xcmsh>
use gothic_formats::xcmsh::Xcmsh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).expect("read");
    let m = Xcmsh::parse(&bytes).expect("parse");
    for (i, s) in m.submeshes.iter().enumerate() {
        println!("sub {i} verts {} mat {} uv2 {} colors4 {} colors5 {}", s.positions.len(), s.material, s.uv2.len(), s.colors4.len(), s.colors5.len());
        for k in 0..s.positions.len().min(40) {
            println!("  p {:?} uv {:?} uv2 {:?} c4 {:?} c5 {:?}", s.positions[k], s.uvs.get(k), s.uv2.get(k), s.colors4.get(k), s.colors5.get(k));
        }
    }
}
