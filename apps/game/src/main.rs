#[cfg(feature = "viewer")]
mod actor_viewer;
#[cfg(feature = "viewer")]
mod perf;
#[cfg(feature = "viewer")]
mod viewer;
#[cfg(feature = "viewer")]
mod walk;
#[cfg(feature = "viewer")]
mod world_loader;

fn main() -> anyhow::Result<()> {
    // Desktop releases carry this small, local-machine configuration beside the EXE. Resolve
    // it before any assets or saves are opened, since Explorer chooses an arbitrary cwd.
    #[cfg(feature = "viewer")]
    configure_runtime_root()?;
    #[cfg(feature = "ui-demo")]
    if std::env::args().any(|a| a == "--ui-demo") {
        return gothic_ui::demo::run(&std::env::args().collect::<Vec<_>>());
    }
    #[cfg(feature = "viewer")]
    if std::env::args().any(|a| a == "--actor" || a == "--zoo") {
        return actor_viewer::run();
    }
    #[cfg(feature = "viewer")]
    {
        let mut args: Vec<String> = std::env::args().collect();
        // Double-click launches the game. Explicit --viewer still selects the asset viewer.
        if args.len() == 1 {
            args.push("--play".into());
        }
        if args.iter().any(|a| a == "--walk" || a == "--play") {
            perf::early_init(&args);
            return walk::run(&args);
        }
    }
    #[cfg(feature = "viewer")]
    viewer::run()?;
    #[cfg(not(feature = "viewer"))]
    {
        use bevy::prelude::*;
        use gothic_world::{Residency, StreamingObserver, WorldStreamingPlugin};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, WorldStreamingPlugin));
        app.world_mut()
            .spawn((StreamingObserver, Transform::default()));
        for _ in 0..32 {
            app.update();
        }
        println!(
            "Gothic Reforged headless development runtime: {} resident synthetic cells.\nFaithful playable world is not implemented. Use --features viewer for asset/placement inspection.",
            app.world().resource::<Residency>().cells.len()
        );
    }
    Ok(())
}

#[cfg(feature = "viewer")]
fn configure_runtime_root() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let exe = std::env::current_exe().context("resolve game executable")?;
    let config = exe.with_file_name("runtime-root.txt");
    let root = match std::fs::read_to_string(&config) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("read runtime-root.txt beside the game EXE"),
    };
    let root = root.trim();
    ensure!(!root.is_empty(), "runtime-root.txt is empty");
    let root = exe.parent().context("game EXE has no directory")?.join(root);
    ensure!(
        root.join(".local/items.json").is_file() && root.join(".local/npcs.json").is_file(),
        "runtime-root.txt must point to a Gothic Reforged runtime with .local/items.json and .local/npcs.json: {}",
        root.display()
    );
    std::env::set_current_dir(&root)
        .with_context(|| format!("open runtime directory {}", root.display()))
}
