//! Prints how the materials of actors resolve to textures (debug aid).
use gothic_formats::{PakArchive, xact::Actor, xshmat::Xshmat};
use std::collections::HashMap;

fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let mut anim = PakArchive::open(format!(r"{dir}\_compiledAnimation.pak"))?;
    let mut mat = PakArchive::open(format!(r"{dir}\_compiledMaterial.pak"))?;
    let img = PakArchive::open(format!(r"{dir}\_compiledImage.pak"))?;
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
    let mi: HashMap<String, usize> = mat.entries.iter().enumerate().rev().map(|(i, e)| (base(&e.path), i)).collect();
    let ii: HashMap<String, usize> = img.entries.iter().enumerate().rev().map(|(i, e)| (base(&e.path), i)).collect();
    for want in std::env::args().skip(1) {
        let at = anim.entries.iter().position(|e| e.path.eq_ignore_ascii_case(&format!("{want}.xact"))).expect("actor");
        let actor = Actor::parse(&anim.read_payload(at)?)?;
        println!("== {want}: {} materials", actor.materials.len());
        for (i, m) in actor.materials.iter().enumerate().skip(1) {
            let listed = actor.material_file(i).unwrap_or("?");
            println!(" mat {i} {:?} listed {listed} diffuse_layer {:?}", m.name, m.diffuse_texture());
            let mut cands = vec![];
            if let Some(t) = m.diffuse_texture() {
                cands.push(format!("{t}_A.xshmat"));
                cands.push(format!("{t}.xshmat"));
            }
            cands.push(listed.to_string());
            for c in cands {
                match mi.get(&c.to_lowercase()) {
                    Some(&k) => {
                        let x = Xshmat::parse(&mat.read_payload(k)?)?;
                        let stem = x.diffuse().map(|d| d.stem().to_string());
                        let found = stem.as_ref().map(|s| ii.contains_key(&format!("{}.ximg", s.to_lowercase())));
                        println!("    {c} -> stem {stem:?} image_found {found:?}");
                    }
                    None => println!("    {c} -> (no material file)"),
                }
            }
        }
    }
    Ok(())
}
