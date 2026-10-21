//! The plugin turns `PendingXrFrame`s into `CoreCommand` messages.
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand};
use gothic_sim::types::ActorId;
use gothic_vr::{Hand, PendingXrFrame, Pose, VrPlugin, XrBackendKind, XrFrame};
use gothic_vr_interact::{InteractTargets, Target, TargetKind, VrInteractPlugin};

#[derive(Resource, Default)]
struct Sent(Vec<Command>);

fn collect(mut sent: ResMut<Sent>, mut msgs: MessageReader<CoreCommand>) {
    sent.0.extend(msgs.read().map(|m| m.0.clone()));
}

#[test]
fn pointing_at_an_npc_and_pulling_the_trigger_sends_confirm() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        VrPlugin {
            backend: XrBackendKind::External,
        },
        VrInteractPlugin,
    ))
    .init_resource::<Sent>()
    .add_systems(Last, collect);
    app.world_mut().resource_mut::<InteractTargets>().0 = vec![Target::new(
        TargetKind::Npc { actor: ActorId(2) },
        Vec3::new(0.3, 1.3, -2.5),
        0.4,
    )];
    let mut frame = XrFrame::default();
    frame.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::new(0.3, 1.3, -0.3), Quat::IDENTITY));
    for pull in [0.0, 0.0, 1.0, 1.0, 0.0] {
        frame.hand_mut(Hand::Right).trigger = pull;
        app.world_mut().resource_mut::<PendingXrFrame>().0 = Some(frame);
        app.update();
    }
    let sent = &app.world().resource::<Sent>().0;
    assert_eq!(
        format!("{sent:?}"),
        format!("{:?}", [Command::Confirm { focus: 3 }])
    );
}
