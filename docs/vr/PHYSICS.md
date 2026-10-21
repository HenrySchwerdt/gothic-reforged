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

## Two stacks in one game (VR-09)

The VR crates (`vr-physics`, `vr-melee`, `vr-npc`, `vr-archery`) keep using avian3d
directly; `gothic-physics` stays the game's physics. They solve different problems:
`gothic-physics` has the static level (`CollisionWorld`), the ported Gothic 3 character
controller, verlet ragdolls and crowd separation, but no rigid-body dynamics, joints or
contact solver; the VR layer needs exactly those (PD-driven hands, props with mass, stab
joints, arrows). Porting them into `gothic-physics` would mean writing a second rigid-body
engine; wrapping avian behind a `gothic-physics` trait would hide the components the VR
plugins query every step. Instead `crates/vr-bridge` (`gothic-vr-bridge`) keeps the two
consistent where they meet:

| Seam | Adapter |
| --- | --- |
| units | `units`: centimetres / sim yaw (`Pos::yaw_to`) <-> metres / Bevy yaw, consistent with `hero_transform` |
| level | `LevelChunkMessage::{Loaded, Unloaded}` streams the same triangle chunks the `CollisionWorld` gets into avian static trimeshes (`load_both` feeds both from one call) |
| actors | every actor the core spawns gets a kinematic avian proxy (`RuntimeActor`, `Body` layer) with melee hitzones that follows `Core::sim.world.actors` each fixed step and despawns with the actor |
| damage | a player `HitMessage` on a proxy becomes `Command::Attack { target }`; the core applies reach, strike gap and the original damage (hits below `BridgeSettings::min_quality` are dropped) |
| time | `CorePlugin` sets `Time<Fixed>` to 30 Hz, the VR physics needs 90 Hz: `CoreTickPlugin` ticks the core at 30 Hz from `Update` with the same `CoreCommand` / `CoreEvent` messages |

Tests (`crates/vr-bridge/tests/runtime_actor.rs`, synthetic core with no game data): a
physics hand swings a physics sword into an actor spawned through `Command::SpawnNpc`, the
hit reaches the core as `Attack` and the actor's hit points drop; proxies follow core moves
and despawn with the actor; a crate rests on a streamed chunk and falls when it is unloaded;
the core advances one second per 90 physics frames while the fixed step stays at 90 Hz.

Limits: proxies are kinematic, so hands cannot shove core actors (use `gothic-vr-npc`'s
active ragdoll as a character's body for that and report its position to the core); the
proxy hitzones are a standing humanoid, not the animated skeleton; arrow hits
(`ArrowHitMessage`) are not mapped yet.
