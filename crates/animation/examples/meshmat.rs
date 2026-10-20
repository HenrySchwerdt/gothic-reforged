use gothic_formats::{PakArchive, xcmsh::Xcmsh, xshmat::Xshmat};
use std::collections::HashMap;
fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let mut meshp = PakArchive::open(format!(r"{dir}\_compiledMesh.pak"))?;
    let mut mat = PakArchive::open(format!(r"{dir}\_compiledMaterial.pak"))?;
    let img = PakArchive::open(format!(r"{dir}\_compiledImage.pak"))?;
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
    let mi: HashMap<String, usize> = mat
        .entries
        .iter()
        .enumerate()
        .rev()
        .map(|(i, e)| (base(&e.path), i))
        .collect();
    let ii: HashMap<String, usize> = img
        .entries
        .iter()
        .enumerate()
        .rev()
        .map(|(i, e)| (base(&e.path), i))
        .collect();
    for want in std::env::args().skip(1) {
        let at = meshp
            .entries
            .iter()
            .position(|e| base(&e.path) == format!("{}.xcmsh", want.to_lowercase()))
            .expect("mesh");
        let m = Xcmsh::parse(&meshp.read_payload(at)?)?;
        for s in &m.submeshes {
            let f = format!("{}.xshmat", s.material.to_lowercase());
            let r = mi.get(&f).map(|&k| {
                let x = Xshmat::parse(&mat.read_payload(k).unwrap()).unwrap();
                let st = x.diffuse().map(|d| d.stem().to_string());
                (
                    x.blend_mode,
                    st.clone(),
                    st.map(|s| ii.contains_key(&format!("{}.ximg", s.to_lowercase()))),
                )
            });
            println!(
                "{want}: material {:?} -> {r:?} bounds {:?} {:?}",
                s.material, s.bounds_min, s.bounds_max
            );
        }
    }
    Ok(())
}
