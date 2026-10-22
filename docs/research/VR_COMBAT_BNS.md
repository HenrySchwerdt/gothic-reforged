# VR melee/bow combat, Blade & Sorcery / Boneworks style

Research note, 2026-10-21. Goal: fix weak hands, wrong weapon-in-hand orientation, and torso clipping the camera.

Source quality warning: B&S and Boneworks internals are not publicly documented. Neither KospY, Warpfrog nor Stress Level Zero has published a hand-physics write-up that I could find. Tags: [S] = stated by a cited source; [C] = community report; [R] = my recommended starting value, NOT sourced, must be tuned on device. Do not treat [R] numbers as facts.

## 1. Physics hands

- Pattern used by open toolkits: the hand is a dynamic (non-kinematic) rigidbody that is driven toward the tracked controller pose, never parented to it. Godot XR Tools moved from a CharacterBody hand to a RigidBody hand that receives force and torque toward the tracked pose; with "careful choosing" of strength, little lag [S] https://forum.godotengine.org/t/improving-hand-physics-in-vr/94661 . Auto Hand sets hand rigidbody velocity toward the follow transform and teleports back when beyond a max follow distance [S] (search summary of https://earnestrobot.notion.site/Hand-967e36c2ab2945b2b0f75cea84624b2f , page itself JS-only, not fetched).
- Failure modes reported: hands lag when the player body moves (force does not account for player velocity) and trail on fast moves [S] same Godot thread. Fix: add the rig's (player) velocity as feed-forward and drive in the tracking-space frame, not world.
- Joint-drive form (Unity ConfigurableJoint): force = spring*(targetPos - pos) + damper*(targetVel - vel), clamped by maximumForce [S] https://docs.unity3d.com/2022.3/Documentation/Manual/configurable-joints-driving-forces.html . That is a PD controller with a force cap; the cap is the knob that makes hands "powerful vs. weak".
- Velocity-setting hands break when joints add mass (hand sways around goal); fix was raising joint mass scale [S] via https://gamedev.net/forums/topic/705383-precisely-controlling-velocity-of-rigidbodies-that-are-using-joints/5419694/ . Prefer force/torque PD over setting velocity when objects are attached.
- Stability at high gain: use implicit/"stable PD", which evaluates using next-step predicted pos/vel and tolerates very high gains and large steps [S] https://tml.stanford.edu/publications/2015/stable-proportional-derivative-controllers (Tan, Liu, Turk 2011). For a 1-body hand this reduces to: a = Kp*e + Kd*(v_t - v) scaled by effective mass, with Kd >= 2*sqrt(Kp) (critical damping) [R, standard control theory].
- B&S exposes four multipliers (position spring, position damper, rotation spring, rotation damper) under experimental physics options. Spring = pull of physical hand toward VR hand (higher recovers lag faster); damper = the "inertia" that causes weapon lag; players run spring ~1.3-1.45, damper ~0.35 for crisper feel [C] https://steamcommunity.com/app/629730/discussions/0/4407417406445050782 . Lesson: expose these as data per weapon and global.
- Weight/lag: B&S weapons visibly drag behind the hand if swung faster than their simulated weight allows, and hit weaker [C] https://steamcommunity.com/app/629730/discussions/1/1814296273133353033 . Implement by capping force/torque so heavy item (mass m) cannot be accelerated faster than F_max/m; the visible hand stays at the controller (ghost) while the physics weapon lags. Boneworks: per-weapon weight changes handling, sledgehammer hard one-handed, easier two-handed [S] https://roadtovr.com/boneworks-physics-driven-mechanics-gameplay-overview/ .
- Collision: Godot XR Tools community suggestions: hands push bodies but cannot exert more than player mass allows (mass 70-110 kg suggested), drop hand collider while holding, or collide only when nothing is held [S] Godot thread above. Boneworks arms/torso can wedge between pipes (hand collision really blocks) [S] Road to VR link.
- Attachment: Auto Hand attaches a held object to the hand with a fixed joint [S] (search summary above). Two hands: distribute force across multiple grab sources [S]. Godot XR Tools two-hand grabs weight position/angle/aim influence per grab point [S] https://github.com/godotvr/godot-xr-tools/blob/docs/_docs/objects/grab_point.md .
- Two-hand recipe [R]: primary hand holds via joint at its grip point; off hand drives only the weapon's aim (rotation of handle axis toward off-hand point) while its position is a sliding constraint along the axis. Weapon orientation = look-at from primary grip to off-hand grip; mass effective doubles hand authority (sum of both hands' force caps), so heavy weapons need two hands.

## 2. Weapon grips ("handles")

- B&S handle = authored object on the weapon with an Axis Length (slide range along the handle axis), an Artificial Distance bias (priority for which handle is grabbed first), and an Activate Handle dependency (support handle usable only if main held). Handles are named in item JSON [S] https://nexusmods.com/bladeandsorcery/mods/10151 . The Item component also has HolderPoint, ParryPoint, SpawnPoint; mass/drag come from JSON overriding the rigidbody [S] https://warpfrog.notion.site/36aae880ea464a78834d1d716b30c6d9 .
- Players can slide grip along the handle without dropping [S] https://vrscout.com/news/hands-on-blade-sorcery/amp/ . Boneworks lets almost any collider be grabbed at many points (overhand/underhand dagger) [S] Road to VR link.
- Godot XR Tools: per-hand grab points with optional hand snap and pose, handle name to prevent double-grab of the same logical handle [S] grab_point.md above.
- Recommended data model for us [R]: `Grip { local_pos, local_rot (hand pose frame), axis (unit, in weapon space), slide_min, slide_max, kind: Primary|Support|Pommel, hand_mirror: bool }`. Orientation fix (your "weapon orientation wrong"): do not hold at the grabbed pose. At grab, project the hand position onto the axis (clamped to slide range) and set the joint frame to the AUTHORED grip rotation, so blade axis is aligned with the forearm direction as authored; optionally blend from the free-grab rotation to authored over ~100-150 ms [R]. For Gothic assets without authored grips, auto-derive: axis = principal axis (longest extent) of mesh AABB; grip = point of min cross-section within the lower 30% (swords/axes) or center (staffs, 2H) [R].

## 3. Hit and damage

- Damage should come from relative velocity at the contact point and blade geometry: B&S tracks blade angle versus the target; shallow-angle strikes do little [C] https://steamcommunity.com/app/629730/discussions/1/1814296273133353033 . Mods add CCD for weapons because fast swings phase through bodies [C] https://steamcommunity.com/app/629730/discussions/0/4407417406445050782 .
- Recommended [R]: per-weapon `Damager` zones (edge, flat, tip) each with local direction vectors. On contact: v_rel = v_weapon_point - v_target_point; slash = |v_rel . edge_normal| dominant and v_rel perpendicular to blade axis; pierce = v_rel . tip_dir > 0.7*|v_rel| and tip zone hit. Damage = base * clamp((|v_rel| - v_min)/(v_ref - v_min),0,1)^~1.5 with v_min ~1.5 m/s, v_ref ~6 m/s [R]; scale by min(1, effective_mass ratio), and by edge alignment factor (cos of angle to edge normal).
- Stabbing/imbedding: B&S stabbing needs a deliberate thrust; weapons require speed [C] https://steamcommunity.com/app/629730/discussions/0/2154350647523285380/?l=english (summary). Implementation [R]: when pierce criteria pass and speed > ~2 m/s, create a prismatic joint (slide along blade axis, limited depth) between weapon and target body part with friction (joint damping); break when joint force exceeds threshold (avian JointForces pattern, see section 8) or when player pulls back > N m/s.
- Parry/block: purely weapon vs weapon rigid collision with hands as the driven bodies; a good parry emerges if weapon colliders are solid and the hand PD is strong enough to resist (hence the force cap matters). Optional assist [R]: lower friction on blade flats; spawn spark/sound from impulse magnitude.
- NPC reactions: active ragdoll = two skeletons, animated hidden target + physical ragdoll whose joints use angular drives toward target rotations [S] https://github.com/sergioabreu-g/active-ragdolls ; strength is the per-joint drive and is the knob for stagger/pinned (scale drive strength by 0..1 over time). No source found on B&S's NPC blending; use [R]: on hit, impulse at contact point on that limb, drop strength of hit limb and its parent chain to ~0.2 for 0.2-0.4 s, recover linearly; on death set strength 0 (full ragdoll). Hit-stop/slow-motion: 60-100 ms at time scale ~0.2 on heavy hit or kill [R, convention].

## 4. Body / avatar

No authoritative source found for B&S/Boneworks neck offsets. Concrete guidance [R] with sourced hints:
- Head hiding: VRChat zeroes head bone scale for first person [S] https://creators.vrchat.com/avatars/avatar-components/vrc-headchop ; do the same (scale head/neck bone to ~0 or render-layer hide head mesh for the HMD camera only).
- Torso placement: anchor chest below the HMD. Eye-to-neck-base ~0.10-0.12 m down and ~0.08 m back (rotated by head yaw only), then chest/hips follow with yaw smoothing (torso yaw = slerp toward head yaw projected, ~5-8 rad/s) [R]. Looking down clips the torso because the body is placed from head pitch; fix: derive torso orientation from yaw only, keep torso forward offset so chest front is at least 0.15 m behind the eye plane when pitch is -60..-90, and optionally also near-plane push or hide the chest/neck mesh faces within 0.2 m of the camera (shader fade / render layer) [R].
- Arms: two-bone IK shoulder to hand, shoulder positions from torso frame ~0.18 m lateral, ~0.05 m below neck base [R]; if the hand is far (physics hand lagging), the visible arm follows the PHYSICS hand, so the player sees weight.
- Legs: from locomotion velocity, play walk/strafe blend with speed; crouch from HMD height ratio; hips follow head with lag [R]. Kinematic body (B&S-style capsule) is cheaper and safer on Quest than Boneworks' full physics body (arms/torso wedge, motion sickness reports [S] Road to VR link). Recommend kinematic capsule + physics hands.

## 5. Haptics

Meta: Quest 3/Touch Plus use a voice-coil motor with PCM waveform; older controllers LRA with fixed frequency and amplitude-only control; Parametric haptics recommended [S] https://developers.meta.com/horizon/documentation/unity/unity-haptics-apis/ and https://developers.meta.com/horizon/documentation/unity/unity-haptics-design-guidelines/ . OpenXR path for us: xrApplyHapticFeedback amplitude 0-1, duration, frequency.
Patterns [R]: grab = 15 ms, amp 0.3; hit = 40-80 ms, amp = clamp(impulse/I_ref, 0.2, 1.0), higher freq for metal-metal block (~250 Hz), low (~80 Hz) for body hit; slide along handle = continuous amp 0.1 proportional to slide speed; heavy weapon swing = low-level continuous proportional to angular speed squared; bow draw ramps 0.05 to 0.4 with draw, release = single 60 ms pulse 0.8 (tutorial used ramp proportional to pull, and a strong pulse on release [S] https://www.kodeco.com/579-advanced-vr-mechanics-with-unity-and-the-htc-vive-part-2/page/2?page=3 ).

## 6. Bow

- Tutorial reference: string pull = controller position in bow space, arrow local Z clamped (min -0.4 m); arrow speed = pull distance x maxShootSpeed (default 50) ; nocking by touching arrow to bow trigger; haptic ramp while pulling and pulse on release [S] Kodeco link. Another dev found speed vs draw is better exponential than linear [S] search summary of https://www.uploadvr.com/vr-archery-vive-trackers/ (weak).
- Recommended [R]: draw max 0.65-0.7 m; v = v_max * (d/d_max)^1.5, v_max ~ 55-70 m/s (for Gothic-scale, tune to range); grab string with off-hand when within 8 cm of nock point and bow is held in the other hand; nock arrow automatically from quiver/inventory on string grab (Twisted Arrow does auto-nock [S] https://roadtovr.com/twisted-arrow-puts-superpowered-bow-center-phaser-locks-next-vr-title/ ). Aim = vector from drawing hand through bow grip (or arrow shaft direction = string hand to bow hand), arrow spawn at nock; apply gravity, use CCD or raycast sweep each step. Tilt draw hand toward cheek is the comfortable pose [S] same Road to VR article.

## 7. Quest performance

- Little public data. B&S Nomad: performance was the biggest challenge; physics was compromised/optimized; spawning 50 shields caused slowdowns [S] https://www.meta.com/blog/blade-and-sorcery-nomad-full-release/ ; launch version capped on-screen enemies at 3 and disabled wounds/blood decals [S] https://www.nexusmods.com/pine/news/14583 .
- Recommended [R]: physics at fixed 72 or 90 Hz matching display (avoid interpolation artifacts on hands); prefer more substeps (4-6) over solver iterations for XPBD-style solvers [S general: https://taintedcoders.com/bevy/physics/avian]; at most 2-3 fully active ragdolls near the player (strength-driven), others pure kinematic animation or sleeping; far NPCs: no physics body at all; simplify weapon colliders to 1-3 primitives; hand colliders = 2 primitives; physics LOD by distance 8 m / 20 m.

## 8. Mapping to our stack (Bevy 0.20 + avian3d)

Avian API notes (verify against the version in Cargo.lock): joints are entities `FixedJoint::new(a,b)`, also Revolute, Spherical, Prismatic, Distance; `JointFrame` for local anchors/basis; `JointDamping`, `JointCollisionDisabled`, `JointForces` (read forces, break by adding `JointDisabled`); motors exist on revolute/prismatic [S] https://docs.rs/avian3d/latest/avian3d/dynamics/joints/index.html . External forces: `ExternalForce` etc. were removed in 0.4 in favor of `ConstantForce`/`ConstantTorque` and the `Forces` query helper for per-step forces [S] https://taintedcoders.com/bevy/physics/avian (search summary). Substeps configurable via substep count resource; ForcePlugin required.

| Technique | Avian mapping |
|---|---|
| PD hand | Hand = dynamic `RigidBody` + `Collider` on its own collision layer. Each fixed step in the schedule before physics, compute linear a_cmd = Kp*(x_t - x) + Kd*(v_t + v_rig - v), clamp |F| <= F_max, apply via `Forces` helper (impulse = F*dt). Torque: axis-angle of q_t * q^-1 (shortest arc), tau = Kp_r*angle*axis - Kd_r*omega, clamp. Gains scaled by body mass+held mass. |
| Teleport/reset | If |x_t - x| > 0.5 m or on lost contact for N frames: set Position/Rotation and zero velocity (Auto Hand style). |
| Held weapon | Grip joint: `FixedJoint` hand to weapon with `JointFrame` at authored grip; or skip joint and add weapon mass to the PD (effective mass) which gives weight lag without joint instability. Start with the joint, damping on. |
| Slide along handle | Replace FixedJoint with `PrismaticJoint` along grip axis plus rotation lock; or re-anchor each frame (update JointFrame anchor) from projected hand position. |
| Two-hand | Primary FixedJoint; secondary: apply aim torque to weapon from off-hand offset (PD on direction), or a second joint with `SphericalJoint`/prismatic and a limited compliance. |
| Hand-world collision | Layers: hand vs world solid, hand vs held weapon none (`JointCollisionDisabled`), hand vs NPC solid but force-capped. |
| Weapon CCD | Enable avian `SweptCcd` on weapon blade colliders; also manual shape cast between last and current blade segment for damage detection. |
| Damage | Read collision events/`CollisionStart` plus contact manifolds; compute v at point from `LinearVelocity`/`AngularVelocity`; implement slash/pierce tests as in section 3. |
| Imbed | Spawn `PrismaticJoint` weapon to NPC body part, break through `JointForces`. |
| Active ragdoll | Each ragdoll limb dynamic; per-joint motors/PD to animation pose via torque; strength resource per limb for stagger. Avian has revolute/spherical joints but no D6 slerp drive: do PD with `Forces` torques yourself. |
| Fidelity | Fixed step 72/90 Hz, substeps 4-6, ragdoll budget 2-3. |

### Prioritized plan (smallest first)

1. Torso clip: make torso yaw-only, offset chest back from eye plane, hide head/neck near camera. (small, fixes the visible bug)
2. Orientation: add authored/auto grip rotation and re-orient on grab (blend 120 ms); fix mirror of left-hand grips.
3. Hands as dynamic bodies with PD force/torque and force cap; show visible hand at the physics body, keep a ghost at the controller; add rig-velocity feed-forward.
4. Teleport-back threshold and held-weapon mass added to PD (weight lag), per-weapon F_max/torque_max data.
5. Hit detection by relative velocity at contact with min speed gate; damage curve; CCD on weapon colliders.
6. Slash vs pierce zones and edge-alignment factor; hit-stop 60-100 ms.
7. Grip joint slide along handle axis (prismatic) with slide ranges from grip data.
8. Two-hand aiming torque; heavy weapons gated on two hands.
9. Haptics patterns (grab, hit by impulse, slide, bow ramp/release).
10. Bow rework (auto-nock, d^1.5 speed, aim along string hand to bow hand), then NPC strength-based stagger and stabbing joints.

Open items to verify on device: Kp/Kd/F_max values (all [R]), Avian exact API names in the pinned version, whether Quest handles 90 Hz physics with our NPC counts.
