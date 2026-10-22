//! List catalog motion names of a family containing a substring: motion_list FAMILY SUBSTR
use gothic_animation::ActorLibrary;
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let lib = ActorLibrary::open(dir)?;
    for (n, _) in lib
        .catalog
        .motions(&args[1])
        .filter(|(n, _)| n.contains(args[2].as_str()))
    {
        println!("{n}");
    }
    Ok(())
}
