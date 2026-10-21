//! Headless test rig for VR physics (also used by the melee, NPC and archery crates):
//! tracked frames are written directly, physics runs at 90 Hz with one fixed step per update.
use crate::VrPhysicsPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_vr::{Hand, PendingXrFrame, Pose, VrPlugin, XrBackendKind, XrFrame};
use std::time::Duration;

pub const HZ: f64 = 90.0;

pub struct Rig {
    pub app: App,
    pub frame: XrFrame,
}

impl Rig {
    pub fn new(setup: impl FnOnce(&mut Commands) + Send + Sync + 'static) -> Self {
        Self::with_plugins(setup, |_| {})
    }

    /// Like [`Rig::new`], with extra plugins added before startup.
    pub fn with_plugins(
        setup: impl FnOnce(&mut Commands) + Send + Sync + 'static,
        plugins: impl FnOnce(&mut App),
    ) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / HZ,
            )))
            .insert_resource(Time::<Fixed>::from_hz(HZ))
            .add_plugins(VrPlugin {
                backend: XrBackendKind::External,
            })
            .add_plugins(VrPhysicsPlugin::default());
        plugins(&mut app);
        let setup = std::sync::Mutex::new(Some(setup));
        app.add_systems(Startup, move |mut commands: Commands| {
            if let Some(f) = setup.lock().unwrap().take() {
                f(&mut commands);
            }
        });
        app.finish();
        let mut frame = XrFrame::default();
        for h in Hand::BOTH {
            frame.hand_mut(h).pose = Some(Pose::new(
                Vec3::new(0.2 * h.sign(), 1.2, -0.3),
                Quat::IDENTITY,
            ));
        }
        let mut rig = Self { app, frame };
        // Let the first fixed steps run (time starts at zero).
        rig.step(3);
        rig
    }

    pub fn step(&mut self, n: usize) {
        for _ in 0..n {
            self.app.world_mut().resource_mut::<PendingXrFrame>().0 = Some(self.frame);
            self.app.update();
        }
    }

    pub fn set_hand(&mut self, hand: Hand, pose: Pose) {
        self.frame.hand_mut(hand).pose = Some(pose);
    }

    pub fn grip(&mut self, hand: Hand, on: bool) {
        self.frame.hand_mut(hand).grip = if on { 1.0 } else { 0.0 };
    }

    /// Position of a physics hand.
    pub fn hand_pos(&mut self, hand: Hand) -> Vec3 {
        let mut q = self
            .app
            .world_mut()
            .query::<(&Transform, &crate::PhysicsHand)>();
        q.iter(self.app.world())
            .find(|(_, h)| h.hand == hand)
            .map(|(t, _)| t.translation)
            .unwrap()
    }

    pub fn transform(&self, e: Entity) -> Transform {
        *self.app.world().get::<Transform>(e).unwrap()
    }

    pub fn find_named(&mut self, name: &str) -> Entity {
        let mut q = self.app.world_mut().query::<(Entity, &Name)>();
        q.iter(self.app.world())
            .find(|(_, n)| n.as_str() == name)
            .map(|(e, _)| e)
            .unwrap_or_else(|| panic!("no entity {name}"))
    }
}

impl Rig {
    /// Spawn something now (applied immediately).
    pub fn spawn(&mut self, f: impl FnOnce(&mut Commands) -> Entity) -> Entity {
        let e = {
            let mut commands = self.app.world_mut().commands();
            f(&mut commands)
        };
        self.app.world_mut().flush();
        e
    }
}
