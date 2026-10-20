#[cfg(feature = "viewer")]
mod actor_viewer;
#[cfg(feature = "viewer")]
mod viewer;
#[cfg(feature = "viewer")]
mod walk;
#[cfg(feature = "viewer")]
mod world_loader;

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "viewer")]
    if std::env::args().any(|a| a == "--actor") {
        return actor_viewer::run();
    }
    #[cfg(feature = "viewer")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--walk") {
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
