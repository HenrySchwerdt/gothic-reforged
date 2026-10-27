# Coordinates, handedness and rotations

One convention, one module: `crates/coords` (`gothic-coords`). Every conversion of Gothic 3 data into engine
space goes through it; nothing else may negate X, add half turns or conjugate a quaternion "to make it look right".

## 1. The convention

| | Gothic 3 / Genome data | Engine (Bevy / glam / avian) |
|---|---|---|
| handedness | **left-handed**, Y up, +Z forward for a camera | right-handed, Y up, -Z forward |
| unit | 1 = 1 cm | 1 = 1 m (the world is built in cm and scaled by 0.01 at the render/physics boundary: `CM_TO_M`) |
| matrices | row-vector (X/Y/Z basis rows, then translation); `bCMatrix` is 16 x f32 | column-vector (glam) |
| rotation about +Y by a positive angle | turns +Z towards +X | turns +Z towards -X |
| triangles | front faces **clockwise** (D3D9 default; Engine.dll talks to Direct3D 9 only, ENGINE_MAP "Library use") | counter-clockwise |
| audio listener | passed to FMOD unchanged, left-handed (`UpdateListener`, Engine 0x100547b0, ENGINE_MAP; AUDIO_FORMATS "Listener") | |

Sources for the Gothic side: D3D9-only renderer (ENGINE_MAP), row-vector placements (TERRAIN_FORMAT), `bCMatrix` /
`bCQuaternion` / `bCEulerAngles` value types (PROPERTY_STREAMS, SCRIPT_ENGINE_INTERFACE), authored skeleton facing -X
(ACTOR_ANIMATION_FORMAT), and the winding measurement in section 3.

**The conversion is the reflection `F = diag(-1, 1, 1)` (mirror X).** It changes handedness. It is *not* the textbook
"flip Z" conversion; the two differ by a half turn about Y, which is only a rigid motion of the whole world, so a camera
placed through the same `F` sees the original picture. Consequences (all tested in `crates/coords/tests/convention.rs`):

| quantity | conversion | function |
|---|---|---|
| position, direction, normal, tangent xyz | `(x,y,z) -> (-x,y,z)` | `vec3`, `arr3` |
| tangent handedness `w` | negated | `tangent4` |
| axial vector (cross product, rotation axis, a blade's `blade x edge` "flat") | `(x,y,z) -> (x,-y,-z)` | `axial` |
| unit quaternion | `(x,y,z,w) -> (x,-y,-z,w)` = `F R F` | `quat`, `quat_xyzw` |
| matrix | `F M F` | `mat4`, `affine` |
| heading (angle about +Y) | negated | `heading` |
| AABB | `(-max.x, ..)..(-min.x, ..)` | `aabb` |
| origin-relative world point (cm) | `(o.x - x, y - o.y, z - o.z)` | `Frame::point`, `Frame::mat4` |
| scale, UV, vertex colour | unchanged | |
| winding | see section 3: static meshes keep their indices, actor meshes swap | `flip_winding` |
| engine yaw / sim yaw | `sim` measures `atan2(dz,dx)` in the mirrored world, engine yaw 0 faces -Z | `sim_yaw_to_engine`, `engine_yaw_to_sim`, `yaw_of_dir`, `forward3` |

Where each kind of data enters: world tiles / placements / collision / nav (`gothic-cook` + `world_loader.rs`, mirrored
at cook or load time), static item meshes (cooked mirrored), actor skeletons, clips and skinned meshes (cooked in the
Gothic frame, mirrored on load by `gothic-animation` through `Rig::from_actor` / `Clip::from_motion` / `skinned_mesh`).
The simulation (`gothic-sim`) already works in the mirrored, origin-relative centimetre space.

## 2. Asymmetric-fixture tests (`gothic-coords`)

Involutions of every function, `quat == F R F` against matrices, composition, `heading`, axial vs polar cross products,
**left hand stays left** (a character's right hand is on its right in both handednesses), **sign text reads left to
right** (texture `u` still grows to the viewer's right), **winding** (a closed mesh keeps inside/outside), **sword edge
faces forward** (and the blade/edge/flat frame stays proper only if the flat normal is converted as axial), `Frame`
round trips, yaw helpers against `Quat::from_rotation_y(yaw) * -Z`.

## 3. Measured: triangle winding differs between formats

`cargo run -p gothic-cook --example winding_probe -- <mesh>.xcmsh <actor>.xact` computes, over all triangles, the sign of
`(b-a) x (c-a) . n_vertex` (+1 = numeric cross follows the stored normal):

| data | result |
|---|---|
| static `.xcmsh` (`g3_weapon_orc_halberd_01`, 514 tris) | **-1.000** |
| actor `.xact` (`G3_Hero_Body_Player`, 6082 tris) | **+0.997** |

`F` flips the sign of the numeric cross product. So static meshes keep their indices after mirroring (Bevy front =
counter-clockwise, as `cook/mesh.rs` and `world_loader.rs` always did), but **actor meshes need the swap**. The desktop
actor path mirrored without swapping (back faces, invisible only because `actor_standard_material` has `cull_mode:
None`); fixed in `skinned_mesh` (`flip_winding`).

Cooked static meshes (`.gmsh`) are already in engine space, checked on the Quest pack:
`g3_weapon_orc_halberd_01` raw x range [-23.6, 43.5] -> cooked [-43.5, 23.6].

## 4. Audit

Scope: `crates/*`, `apps/game`, `apps/quest`, `crates/vr*`, `crates/xr-quest`. Searched `from_rotation_*`, `FRAC_PI_*`,
`PI`/`TAU`, `to_radians`, `mirror`, `flip`, `-x` negations (`Vec3::new(-`, `[-v[0]`, `-p.x`, `NEG_X`), quaternion component
swaps, `* -1.0`, `180`, `90`, `hack`, `fix`, `workaround`, `half turn`: about 500 hits, of which about 330 are not tests or
comments. Classes: (a) correct use of the central conversion, (b) duplicate of the conversion, (c) ad-hoc compensation,
(d) unrelated (angle wrap, procedural geometry, VR rig maths in engine space, test fixtures).

Counts: (a) 31 sites, (b) **54 sites replaced by the module**, (c) 12 sites (2 fixed at the root, 10 documented), (d) the rest.

### (b) duplicates, now delegating to `gothic-coords`

| where | what |
|---|---|
| `animation/pose.rs` | `mirror_quat_x`, `Trs::mirror_x` |
| `animation/clip.rs`, `rig.rs` | clip key translations; bind poses (via `Trs::mirror_x`) |
| `animation/library.rs` | actor bounds, static bounds (`aabb`), static + skinned vertex / normal mirrors (4 closures) |
| `cook/mesh.rs`, `cook/cook.rs` | vertex, normal, tangent, terrain bounds, nav and collision triangles (6) |
| `apps/game/world_loader.rs` | `Origin::point`, `flip()` + the `T * F * M * F` matrix twice, vertex/normal/tangent mirrors (7), collision triangles (2), vegetation quaternion |
| `apps/game/walk/env.rs` | light position map |
| `apps/game/play/mod.rs` | `sim_to_bevy_yaw`, `bevy_to_sim_yaw`, `angle_delta`, `yaw_of_dir`; five `Vec3::new(-yaw.sin(), 0, -yaw.cos())` forward formulas (`cam.rs` x2, `hero.rs` x2, `qa/bow.rs`, `qa/magic.rs`) |
| `vr-bridge/units.rs` | `sim_yaw_to_bevy`, `bevy_yaw_to_sim` (were a second copy of the sim yaw maths) |
| `fx/space.rs`, `fx/params.rs` | `MIRROR` matrix (`to_view`, `to_view_local`, `point_to_view`), `mirrored_x` boxes and vectors |
| `quest/actor.rs`, `quest/npcs.rs` | `mirror_quat` + `node_local` (x2), `ardea.rs` mirror / AABB, `grip.rs` frame mirror, `examples/pack_veg.rs` (position, quaternion) |
| `nav/examples/ardea_path.rs` | Gothic -> cooked point |

### (a) correct use (kept)

`world_loader` placements via `Frame`, `play/objects.rs` item heading `-def.heading` (matches `gothic_coords::heading`:
the Genome matrix row 2 is the forward axis, the mirrored world turns the other way), `Rig::facing_in_pose` (the facing fix is
measured from the mirrored foot/toe or spine/head vectors, no constant), `play/npcs.rs` spawn yaw through
`sim_to_bevy_yaw`, `quest/retarget.rs` `bind_facing` and its `Quat::from_rotation_arc(facing, -Z)`, `vr-*` rig maths
(already engine space), camera `reset`, OpenXR poses (engine space by definition of the stage).

### (c) ad-hoc compensation

| site | verdict |
|---|---|
| `quest/ardea.rs` `spawn_static_mirrored` + `grip.rs` `GripFrame::mirrored()` + `hero_check` re-mirrors + `flat: -m(flat)` | **Root cause: double mirror.** The cooked static mesh is already mirrored (section 3), the quest then mirrored it again with a winding swap, and the grip frame, the prop visual and the diagnostics were all re-derived in the un-mirrored (wrong-handed) frame. A halberd/axe head or a sword's flat side ended up on the wrong side, compensated by the rotation derived from the doubly mirrored frame. **Fixed**: weapons and arrows use `spawn_static`; `visual()` uses the frame as measured; the re-mirror helpers are deleted. |
| `animation/library.rs` `skinned_mesh` (mirror without winding swap) | **Root cause: wrong assumption that the reflection fixes the winding of every format** (it does for `.xcmsh`, not for `.xact`). Hidden by `cull_mode: None`. **Fixed**: `flip_winding`. The quest NPC path (`mirror_mesh_x`) already swapped, now documented as actor-only. |
| `play/ranged.rs` `BOW_IN_HAND_TURN` (half turn of the drawn bow about the Z axis of `Slot_LeftHand_Weapon`) | **Not a mirror error**: skeleton, clips and bow mesh all go through one `F`, and the identity link is what `gCInventorySlot::GetBone` / `AttachItemToSkeleton` do. The offset is inferred from the `bow_faithful` geometry (string hand 171 degrees off without it, 9 with it). Kept, marked `[I]`; the missing datum is the item-side attachment transform of the original (see open items). |
| `quest/retarget.rs` `THUMB_OPP_SIGN` ("picked from the grip render") | A sign chosen by eye; the thumb axes come from mirrored bone rest frames. Kept; to be derived from the hand rest frame in the grip lab. |
| `audio/plugin.rs` `swap(left_ear_offset, right_ear_offset)` | Documented against `rodio-0.22 spatial.rs`; a property of the audio backend, not of Gothic data. Correct to keep. |
| `walk.rs` `NORMAL_Y_SIGN = 1.0` | No-op constant; the tangent `w` sign is handled by `tangent4`. |
| `quest/weapons.rs` fallback `from_rotation_y(PI)` | Only used when the grip analysis fails; harmless, left. |
| `quest/hero.rs` mirror camera `rotate_y(PI)`, `actor_viewer.rs` `yaw + PI` | Camera framing (look back at the avatar), not data compensation. |
| `vr-ui/wrist.rs`, `vr-physics/props.rs` quarter turns | Engine-space VR panel / capsule axis orientation, no Gothic data. |
| `camera/state.rs`, `camera/dialog.rs` azimuth / `rot_y(deg)` / `mirrored()` presets | **Unverified**: preset azimuths in degrees are used with Bevy's `from_rotation_y` sign. If the original measures them in the left-handed frame the sign flips. No decompiled evidence recorded yet; do not "fix" without it. |
| `environment` sun azimuth (`SUN_ROT`, `to_sun`) | **Unverified** against `eCHemisphere_PS` / `DirectionOffset` (euler); the east/west sense of shadows may be mirrored. |

## 5. Rules for new code

1. Data from a Gothic file is converted once, at the boundary, with `gothic_coords`. A function named `mirror_*`, a
   `Vec3::new(-v.x, ..)` or a `Quat::from_xyzw(q.x, -q.y, ..)` outside `crates/coords` is a bug.
2. Cooked static meshes (`.gmsh`) and the world are engine space. Cooked actors/motions are Gothic space until the
   animation crate mirrors them.
3. A constant half turn is not a fix. If a Gothic object is 180 degrees off, one of: a missing/double `F`, a vector that is
   axial (use `axial`), the wrong winding class, or a genuinely different engine convention (then cite the decompiled
   function next to the constant).
4. OpenXR poses (grip/aim) are already right-handed engine space (-Z forward, spec: XR_KHR_... pose conventions); the
   grip-to-weapon frame (`-Z` out of the fist, `-Y` along the fingers) is a hand convention, not a Gothic one.

## 6. Open items

* Original item-to-slot attachment transform (bow half turn); needs `eCActorAttachment::Process` (Engine 0x1002a5b0) and the
  `gCInventorySlot` item offset, not the bone only.
* Camera / dialog azimuth sign and the sun azimuth sense (see audit).
* Desktop actor materials could cull back faces now that the winding is right (`actor_standard_material`).
