//! Prints the `eCShader*` class names inside material files whose name contains a pattern.
//! Usage: mat_class PATTERN
use gothic_formats::PakArchive;

fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let pat = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let mut mat = PakArchive::open(format!(r"{dir}\_compiledMaterial.pak"))?;
    let names: Vec<(usize, String)> = mat
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.deleted() && e.path.to_lowercase().contains(&pat))
        .map(|(i, e)| (i, e.path.clone()))
        .collect();
    for (i, p) in names {
        let Ok(b) = mat.read_payload(i) else { continue };
        let mut found: Vec<String> = Vec::new();
        let mut at = 0;
        while let Some(k) = b[at..].windows(8).position(|w| w == b"eCShader") {
            let s = at + k;
            let e = b[s..]
                .iter()
                .position(|c| !c.is_ascii_alphanumeric() && *c != b'_')
                .map_or(b.len(), |e| s + e);
            let n = String::from_utf8_lossy(&b[s..e]).to_string();
            if !found.contains(&n) {
                found.push(n);
            }
            at = e.max(s + 8);
        }
        let tex = gothic_formats::xshmat::Xshmat::parse(&b)
            .map(|x| {
                x.textures
                    .iter()
                    .map(|t| format!("{:?}:{}", t.role, t.stem()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        println!("{p}: {found:?} {tex:?}");
    }
    Ok(())
}
