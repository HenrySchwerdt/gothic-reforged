//! Measures the triangle winding of a static `.xcmsh` and of an actor `.xact` against their stored normals
//! (evidence for `docs/research/COORDINATES.md`): winding_probe MESH.xcmsh ACTOR.xact
use gothic_formats::PakArchive;
fn score(pos: &[[f32; 3]], nrm: &[[f32; 3]], idx: &[u32]) -> (f64, usize) {
    let (mut agree, mut n) = (0f64, 0usize);
    for t in idx.chunks_exact(3) {
        let p = |i: u32| pos[i as usize];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cr = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let mut nv = [0f32; 3];
        for &i in t {
            for k in 0..3 {
                nv[k] += nrm[i as usize][k];
            }
        }
        let d = cr[0] * nv[0] + cr[1] * nv[1] + cr[2] * nv[2];
        agree += if d > 0.0 { 1.0 } else { -1.0 };
        n += 1;
    }
    (agree, n)
}
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut mp = PakArchive::open(dir.join("_compiledMesh.pak"))?;
    let at = mp.entries.iter().position(|e| e.path.to_lowercase().ends_with(&a[1].to_lowercase())).unwrap();
    let x = gothic_formats::xcmsh::Xcmsh::parse(&mp.read_payload(at)?)?;
    let (mut s, mut n) = (0.0, 0);
    for sub in &x.submeshes {
        if sub.normals.len() == sub.positions.len() {
            let (ss, nn) = score(&sub.positions, &sub.normals, &sub.indices);
            s += ss;
            n += nn;
        }
    }
    println!("static {}: {} tris, (cross . normal) agreement {:+.3} (+1 = numeric cross follows the normal)", a[1], n, s / n as f64);
    let mut ap = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = ap.entries.iter().position(|e| e.path.to_lowercase().ends_with(&a[2].to_lowercase())).unwrap();
    let act = gothic_formats::xact::Actor::parse(&ap.read_payload(at)?)?;
    let (mut s, mut n) = (0.0, 0);
    for m in &act.meshes {
        for sub in &m.submeshes {
            if sub.normals.len() == sub.positions.len() {
                let (ss, nn) = score(&sub.positions, &sub.normals, &sub.indices);
                s += ss;
                n += nn;
            }
        }
    }
    println!("actor {}: {} tris, agreement {:+.3}", a[2], n, s / n as f64);
    Ok(())
}
