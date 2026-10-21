//! Bevy integration: reads [`VrInputRes`], writes [`CoreCommand`] messages.
//!
//! The host keeps [`InteractTargets`] (what is near the hero, refreshed from its world
//! entities and the core's `ObjectRegistered` / `ItemRegistered` ids) and [`InteractHero`]
//! (inventory facts) up to date. Runs after `VrSet::Locomotion` (fresh input and the
//! locomotion offset for world poses); the classic button bindings and these interactions
//! send into the same message stream.
use crate::mapper::{HeroState, InteractMapper, Tracked};
use crate::targets::Target;
use bevy::prelude::*;
use gothic_runtime::CoreCommand;
use gothic_vr::{Hand, VrInputRes, VrPlayer, VrSet};

#[derive(Resource, Debug, Default, Clone)]
pub struct InteractTargets(pub Vec<Target>);

#[derive(Resource, Debug, Default, Clone)]
pub struct InteractHero(pub HeroState);

#[derive(Resource, Debug, Default, Clone)]
pub struct InteractMapperRes(pub InteractMapper);

/// Add after `gothic_vr::VrPlugin`.
pub struct VrInteractPlugin;

impl Plugin for VrInteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InteractTargets>()
            .init_resource::<InteractHero>()
            .init_resource::<InteractMapperRes>()
            .add_message::<CoreCommand>()
            .add_systems(
                Update,
                interact.after(VrSet::Locomotion).before(VrSet::Sync),
            );
    }
}

fn interact(
    time: Res<Time>,
    input: Res<VrInputRes>,
    player: Option<Res<VrPlayer>>,
    targets: Res<InteractTargets>,
    hero: Res<InteractHero>,
    mut mapper: ResMut<InteractMapperRes>,
    mut out: MessageWriter<CoreCommand>,
) {
    let input = &input.0;
    let world = |p: gothic_vr::Pose| player.as_ref().map_or(p, |pl| pl.world(&p));
    let tracked = Tracked {
        head: world(input.head()),
        hands: Hand::BOTH.map(|h| input.hand_pose(h).map(world)),
    };
    for c in mapper
        .0
        .update(input, &tracked, &targets.0, &hero.0, time.delta_secs())
    {
        out.write(CoreCommand(c));
    }
}
