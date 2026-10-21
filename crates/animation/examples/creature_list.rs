//! Lists motion families with their pose/action/phase combinations (creature survey helper).
use gothic_animation::ActorLibrary;
use std::{collections::BTreeMap, path::Path};
fn main() -> anyhow::Result<()> {
    let lib = ActorLibrary::open(Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data"))?;
    let only = std::env::args().nth(1);
    for f in lib.catalog.families() {
        if only.as_deref().is_some_and(|o| !o.eq_ignore_ascii_case(f)) {
            continue;
        }
        let mut combos: BTreeMap<String, usize> = BTreeMap::new();
        for (_, m) in lib.catalog.motions(f) {
            *combos
                .entry(format!(
                    "{}/{}/{}/{}/{}",
                    m.pose, m.left, m.right, m.action, m.phase
                ))
                .or_default() += 1;
        }
        let total: usize = combos.values().sum();
        println!("== {f} ({total})");
        if only.is_some() {
            for (k, v) in combos {
                println!("  {k} x{v}");
            }
        }
    }
    Ok(())
}
