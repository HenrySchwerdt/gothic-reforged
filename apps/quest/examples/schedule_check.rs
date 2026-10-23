//! Host-side check: builds the Quest app's plugins and initializes every schedule, so system parameter conflicts
//! (Bevy B0001) show up with system names on the PC instead of as a crash on the headset.
use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((TransformPlugin, AssetPlugin::default()))
        .init_asset::<bevy::shader::Shader>()
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>();
    gothic_quest::build(&mut app);
    app.world_mut()
        .resource_scope(|world, mut schedules: Mut<Schedules>| {
            for (label, schedule) in schedules.iter_mut() {
                println!("init {label:?}");
                schedule.initialize(world).unwrap();
            }
        });
    println!("all schedules initialized without conflicts");
}
