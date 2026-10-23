//! Lists the motion catalog of a family (host tool, reads the user's game install read-only):
//! clip_catalog FAMILY [filter]
use gothic_animation::ActorLibrary;
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let lib = ActorLibrary::open(dir)?;
    let fam = args.get(1).map(String::as_str).unwrap_or("Orc");
    let filter = args.get(2).map(|s| s.to_lowercase());
    for (name, m) in lib.catalog.motions(fam) {
        if filter.as_ref().is_none_or(|f| name.to_lowercase().contains(f)) {
            println!("{name}  [{} {} {} {} {} {}]", m.pose, m.left, m.right, m.action, m.phase, m.dir);
        }
    }
    Ok(())
}
