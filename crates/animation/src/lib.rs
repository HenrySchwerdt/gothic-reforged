//! Character animation for Gothic 3 actors: `.xact` skinned meshes driven by `.xmot` motions.
//!
//! * [`library::ActorLibrary`]: cached loading of actors, motions, textures and weapons.
//! * [`animator::Animator`]: cross-fading playback, masked overlay layer, root motion.
//! * [`locomotion`]: input-driven state machine (idle, walk, run, sprint, strafe, turn in
//!   place, jump, fall, land, sneak) that selects motions by the name grammar of
//!   [`names`].
//! * [`runtime::ActorSpawner`]: spawns characters composed of a body plus parts (head, armor)
//!   sharing one skeleton, with weapon attachments.
//!
//! Add [`AnimationPlugin`], insert an [`library::ActorLibrary`] and spawn with
//! [`runtime::ActorSpawner`]; a physics/character controller only has to write
//! [`locomotion::LocomotionInput`].
use bevy::{
    app::{App, Plugin, PostUpdate, Update},
    camera::visibility::VisibilitySystems,
    ecs::schedule::IntoScheduleConfigs,
    ecs::system::Res,
    transform::TransformSystems,
};

pub mod animator;
pub mod appearance;
pub mod clip;
pub mod library;
pub mod locomotion;
pub mod names;
pub mod pose;
pub mod rig;
pub mod runtime;

pub use animator::{Animator, DEFAULT_FADE, Motion, Overlay};
pub use library::{ActorLibrary, AssetStores, Composition};
pub use locomotion::{Locomotion, LocomotionInput, Stance};
pub use runtime::{ActorSpawner, AnimationStats, AttachSpec, NpcSpec, WeaponSpec};

#[derive(Default)]
pub struct AnimationPlugin;

fn has_library(library: Option<Res<ActorLibrary>>) -> bool {
    library.is_some()
}

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationStats>()
            .add_systems(Update, runtime::locomotion_system.run_if(has_library))
            .add_systems(
                PostUpdate,
                (
                    runtime::animate_system,
                    (
                        runtime::write_joints_system,
                        runtime::weapon_mount_system,
                        runtime::attachment_system,
                        runtime::clear_dirty_system,
                    )
                        .chain()
                        .after(TransformSystems::Propagate)
                        .before(VisibilitySystems::CheckVisibility),
                )
                    .chain()
                    .before(VisibilitySystems::CheckVisibility),
            );
    }
}
