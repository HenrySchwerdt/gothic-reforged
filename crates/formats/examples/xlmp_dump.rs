//! Debug: parse an extracted .xlmp and print a summary.
fn main() {
    if std::env::args().nth(1).as_deref() == Some("all") {
        return all();
    }
    let b = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let x = gothic_formats::xlmp::Xlmp::parse(&b).unwrap();
    println!(
        "{} scale {} blocks {}",
        x.mesh,
        x.texel_scale,
        x.blocks.len()
    );
    for (i, k) in x.blocks.iter().enumerate() {
        let lit = k.light.iter().filter(|c| c[0] | c[1] | c[2] != 0).count();
        let data = k.tiles.iter().filter(|t| !t.bgra.is_empty()).count();
        println!(
            "block {i}: v={} lit={lit} tiles={} withdata={data}",
            k.light.len(),
            k.tiles.len()
        );
    }
}
#[allow(dead_code)]
pub fn all() {
    let mut p = gothic_formats::PakArchive::open(
        "D:/SteamLibrary/steamapps/common/Gothic 3/Data/Lightmaps.pak",
    )
    .unwrap();
    let (mut ok, mut bad, mut lit, mut data, mut n) = (0, 0, 0, 0, 0);
    for i in 0..p.entries.len() {
        let name = p.entries[i].path.clone();
        if !name.ends_with(".xlmp") {
            continue;
        }
        n += 1;
        let b = p.read_payload(i).unwrap();
        match gothic_formats::xlmp::Xlmp::parse(&b) {
            Ok(x) => {
                ok += 1;
                lit += x.blocks.iter().any(|k| k.has_vertex_light()) as usize;
                data += x
                    .blocks
                    .iter()
                    .any(|k| k.tiles.iter().any(|t| !t.bgra.is_empty()))
                    as usize;
            }
            Err(e) => {
                bad += 1;
                if bad < 6 {
                    println!("{name}: {e:#}");
                }
            }
        }
    }
    println!("files {n} ok {ok} bad {bad} withVertexLight {lit} withTileData {data}");
}
