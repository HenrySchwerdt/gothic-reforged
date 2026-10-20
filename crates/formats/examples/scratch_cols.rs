use gothic_formats::{vfs::VirtualFileSystem, xcmsh::Xcmsh};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    let slot = vfs.resolve_name(&a[1]).expect("not found");
    let m = Xcmsh::parse(&vfs.read(slot).unwrap()).unwrap();
    for (i, s) in m.submeshes.iter().enumerate() {
        // histogram of c4.a by normal.y bucket; c5 channels
        let mut h = vec![(0u64, 0f64, 0f64, 0u64); 5];
        let mut c4rgb = [0u64; 3];
        let mut c5 = [0f64; 4];
        for k in 0..s.positions.len() {
            let ny = s.normals[k][1];
            let b = ((ny.clamp(0.0, 0.999)) * 5.0) as usize;
            h[b].0 += 1;
            h[b].1 += s.colors4[k][3] as f64;
            h[b].2 += s.colors5[k][3] as f64;
            for c in 0..3 { c4rgb[c] += s.colors4[k][c] as u64; }
            for c in 0..4 { c5[c] += s.colors5[k][c] as f64; }
        }
        println!("sub {i} {} n={}", s.material, s.positions.len());
        for (b, (n, a4, a5, _)) in h.iter().enumerate() {
            if *n > 0 { println!("   ny[{:.1}..] n={n} mean c4a={:.0} c5a={:.0}", b as f32 / 5.0, a4 / *n as f64, a5 / *n as f64); }
        }
        let n = s.positions.len() as f64;
        println!("   mean c4rgb={:.0},{:.0},{:.0} c5={:.0},{:.0},{:.0},{:.0}", c4rgb[0] as f64 / n, c4rgb[1] as f64 / n, c4rgb[2] as f64 / n, c5[0]/n, c5[1]/n, c5[2]/n, c5[3]/n);
    }
}
