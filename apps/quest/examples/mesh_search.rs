//! Host tool: lists static meshes and actors whose name contains the needles (reads the game install read-only).
use gothic_animation::ActorLibrary;
fn main() -> anyhow::Result<()> {
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut lib = ActorLibrary::open(dir)?;
    for needle in std::env::args().skip(1) {
        println!("== {needle}");
        for n in lib.static_mesh_names(&needle).iter().take(60) {
            println!("mesh  {n}");
        }
        for n in lib.actor_names(|a| a.to_lowercase().contains(&needle.to_lowercase())).iter().take(40) {
            println!("actor {n}");
            for p in lib.texture_problems(n) {
                println!("   problem {p}");
            }
        }
    }
    Ok(())
}
