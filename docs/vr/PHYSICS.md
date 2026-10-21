# Physics engine choice (VR-03)

Both candidates support Bevy 0.20: `avian3d 0.8.0` and `bevy_rapier3d 0.37.0`. We use
**avian3d**, pinned `=0.8.0` with `default-features = false` and
`3d, parry-f32, default-collider, xpbd_joints, parallel, debug-plugin`.

| | avian3d 0.8 | bevy_rapier3d 0.37 |
| --- | --- | --- |
| Data model | ECS-native: bodies, colliders, joints, forces are components; other plugins query them directly | separate Rapier world mirrored from the ECS through handles |
| Joints we need | fixed (grips), prismatic with limits (stab tracks), spherical; compliance per joint | all available |
| Forces | `Forces` query data: force/torque/impulse at point per step | `ExternalForce`/`ExternalImpulse` components |
| CCD | swept CCD per body (`SweptCcd`) | CCD flag |
| Determinism | deterministic per platform; `enhanced-determinism` (libm) for cross-platform | `enhanced-determinism` feature |
| Fit with the codebase | Bevy schedules (`FixedPostUpdate`), plain components, easy headless tests with `MinimalPlugins` | heavier glue |

The Gothic character controller keeps its own collision (`gothic_physics::CollisionWorld`,
centimetres) — VR locomotion drives that controller. avian only simulates the VR-specific
rigid bodies (hands, props, weapons, NPC bone colliders) against static greybox colliders.

## Hands

* A hand is a 0.5 kg sphere, gravity off, swept CCD. A stable (implicit) PD controller
  (`pd::linear_force`, `pd::angular_torque`) computes the wished acceleration toward the
  tracked pose; the force/torque is limited by `pd::arm_limits(strength)`.
* While holding something, the drive accounts for the held mass and its inertia tensor about
  the hand (`pd::inertia_about`) and is *split* between hand and object, so the grip joint only
  carries the residual (stable even for a thin 1.4 kg blade on a 0.5 kg hand); gravity of the
  held object is fed forward (force and moment), so a strong character holds heavy weapons
  level while a weak one lets them sag and lag.
* Measured (test `heavy_weapons_lag_strength_helps`, 1.5 Hz ±0.6 m swing): sword 7 cm lag at
  STR 10, two-handed sword unmanageable one-handed at STR 10, 7 cm at STR 150.

## Collision layers

`VrLayer::{World, Hand, Prop, Body}`: hands hit everything, props hit world/hands/props,
bodies (NPC bone colliders, dummies) hit world/hands. Weapons pass through bodies on purpose:
weapon-vs-body is resolved by the melee contact model (damage, bite impulse, stab joints).
