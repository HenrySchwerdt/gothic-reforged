# Character physics and movement (Gothic 3)

Tags: **V** = read from shipped data or instructions (address / file given), **I** = inferred from behaviour of the code or data, **T** = tuned value with no original (kept visible in `MovementProfile`), **U** = unknown. No decompiled bodies are reproduced; Game.dll addresses are RVAs. Original data files stay under `.local/`.

## 1. What the original does

- **No PhysX character controller.** Engine.dll and Game.dll import PhysX only through `PhysXLoader` (`NxCreatePhysicsSDK`); the string tables contain no `NxController`, `NxCapsuleController` or `createController`, only `NxCapsuleShapeDesc` (a plain shape) [V string search of `Engine-strings.json`, `Game-strings.json`]. Movement is written by the game: `gCCharacterMovement_PS` (Game.dll) moves a rigid-body capsule (`eCRigidBody_PS`, `TotalMass` 80, `AngularDamping` 0.1, `LinearDamping` 0 on PC_Hero [V template]) with a velocity per tick and finds the ground with its own ray traces (`PutToGround` G:e94f0, `eCPhysicsScene::TraceRayFirstHit`). So "PhysX CCT semantics" (skin width, slope limit, step offset) are replaced here by the game's own rules below.
- **Scene constants** (`eCPhysicsScene_PS` of the world `sysdyn_{9A103CC2-...}`) [V world data]: `GravityVector` (0, -981, 0) cm/s^2, `DefaultSkinWidth` 0.001, `BounceTreshold` -2, `SimulationFPS` 30, `DefaultSleepLinVeloSquared` 0.0225, `DefaultSleepAngVeloSquared` 0.0196, `MaximumAngularVelocity` 7, friction scalings 1.
- **Movement mode** (`gECharMovementMode`, field +0x100) [V from the `Is*` predicates e8250..e8330 and `SetSpeedByMode` e9390]: 1 Standing, 2 Sneaking, 3 Walking, 4 Running, 5 Sprinting, 6 Jumping, 7 Swimming, 8 Diving, 0xd Falling; 0xc is the slide mode (entered from `CanSlide`, G:ea700, in `CheckChangedMovementStates` G:e5cd0) [I]; 0xe is a disabled/script state (no speed) [I].
- **Speed by gait** [V `SetSpeedByMode` e9390 / `GetSpeedByMode` e9450]: the current speed modifier is 1 for Standing/Run, `SneakModifier` for Sneak, `SlowModifier` for Walk (and mode 0xa), `FastModifier` for Sprint, `JumpSteerScaleFactor` in the jump mode, `FallSteerScaleFactor` while falling, 0 in mode 0xe. The maximum ground speed is `ForwardSpeedMax * modifier` (`GetCurrentMaxSpeedOfDir` G:eb800 blends `ForwardSpeedMax`, `StrafeSpeedMax` and `BackwardSpeedMax` by the angle between the wanted direction and the facing; a straight-up/down direction gives half of forward) [V structure, I exact blend; this port uses an ellipse].
  - Hero: 400 x 1.0 = **400 cm/s run**, 400 x 1.5 = **600 cm/s sprint**, 400 x 0.375 = **150 cm/s walk**, 400 x 0.7 = 280 cm/s sneak, strafe/backward 280. The shipped hero clips agree: `Move_Run_Fwd_..._400`, `Move_Run_Left/Right_..._300`, `Move_Sprint_Fwd_..._600`, `Move_Walk_..._160` (nominal clip speed, played at `speed / nominal` rate) [V `hero-motions.txt`].
- **Acceleration** [V `ProcessControledTranslation` G:e2410]: the scalar speed grows by `MoveAcceleration` x frame time (hero 1000 cm/s^2, i.e. 0.4 s to run speed; legacy templates 250) up to the directional maximum; when the goal is within `GetBrakingDistance` (NPC goal movement) it changes by `MoveDecceleration` (-500). Without a wanted direction no velocity is added (instant stop for the player) [V/I]. The player's `gCCharacterControl_PS` (property-set type 22) suppresses the 0.3 floor on the slope scale that NPCs have [V G:ebc80].
- **Slopes** [V `CanWalkOnFloor` G:ea420, `CalcGroundSlopeTransScale` G:ebc80, `CanSlide` G:ea700]: the angle `a` between the ground normal and up decides everything. Walkable when `a <= SteepGroundAngleMax` (hero 49 deg; legacy templates 65). Speed scale: uphill 1 below `SteepGroundAngleMin` (20 deg), then falling linearly to 0 at the maximum; downhill `1 + WalkDownSpeedScale * (a - Min)/(Max - Min)` (`WalkDownSpeedScale` 0.7), `2 * WalkDownSpeedScale` past the maximum; NPCs never go below 0.3. The scale is smoothed (`GroundSlopeTransInertia` 0.15 s). Steeper ground starts the slide mode: speed approaches `SlideSpeedMax` (800 cm/s) with `SlideInertia` 0.4 (the smoothing law itself is I).
- **Step height** [V `StepHeight` 65 cm hero/most humans, 60 on legacy human templates]: `CheckChangedMovementStates` starts the fall mode when the feet are more than `StepHeight` above the ground (doubled in slide mode) and `PutToGround` keeps the character on ground within that distance; obstacles lower than the step are therefore walked over (I: stairs and curbs up to 65 cm are not blockers, anything higher blocks).
- **Jump / fall** [V]: `JumpStartUpVelocity` 600 cm/s (hero; 500 on legacy) gives apex 183.5 cm and 1.22 s air time at 981 cm/s^2. The jump mode ends and the fall mode starts when the vertical speed is below 0.5 cm/s or 0.2 s after take-off without it (G:e5cd0). The vertical velocity is clamped at **-1500 cm/s** (G:e5110 end of `ProcessPhysicsMovements`). `LastFallVelocity` stores the vertical speed at the end of a fall (G:e5110 +0x94). Landing needs ground contact or 0.3 s without vertical progress. Fall damage rule (script, `OnMovementModeChanged`, `COMBAT_AI_RULES.md` 5.1): `floor(160 * (t - 0.9)^2)` for `t > 0.9` s in the fall mode [V].
- **Swimming**: `SwimDepth` 155 cm (hero), 160-300 others; water as ground only for `TreatWaterAsSolid` (bloodflies) [V]. The water level comes from the water meshes of the loaded tiles (see section 4, `move_states.rs`).
- **Quadrupeds** (`IsQuadruped`, `QuadrupedSlopeInertia` 5): body pitch follows the ground; legs/body shape are horizontal capsules [V flags, I use]. Wolf forward max 550, bison/boar/trolls 400 [V template groups]. Not special-cased yet.

## 2. Template data (decoded with `gothic-inspector tple-dump`)

24 distinct `gCCharacterMovement_PS` sets over 6244 templates. Dominant set (726 + 252 + 65 + ... NPCs, and PC_Hero): StepHeight 65, Forward/Strafe/Backward 400/280/280, TurnSpeedMax 200 deg/s, TurnSpeedModifier 0.5, MoveAcceleration 1000, MoveDecceleration -500, TurnAcceleration 999999, Slow/Fast/Sneak 0.375/1.5/0.7, SteepGround 20/49, WalkDownSpeedScale 0.7, SlideSpeedMax 800, SlideInertia 0.4, GroundSlopeTransInertia 0.15, AniVelocityInertia 5, JumpStartUpVelocity 600, Jump/FallSteerScaleFactor 1, SwimDepth 200 (hero 155), CanBePushedWhileIdle false. Deviations: legacy `_deleted` templates (StepHeight 60, accel 250, steep 30/65, jump 500, swim 160), wolves (forward 550), goblins (strafe 110), ogres (strafe 200), trolls/ogres/golems/demons (swim 250-300), bloodfly (fast 2.5, water solid), cutscene snapper (ForceGroundAlignment).

Collision capsule (first capsule of `eCCollisionShape_PS`, class `eCCollisionShape` shape type Capsule: height, radius, 3x3 orientation, centre) [V byte scan of the templates; which of the two capsules is the movement shape is I]:

| template | cylinder height | radius | centre y | total height |
| --- | --- | --- | --- | --- |
| PC_Hero | 133.68 | 33.16 | 96.94 | 200.0 (feet at -3.06) |
| human NPC (bandit, orc, skeleton) | 129.3-134.1 | 34.0-35.3 | 96.9-100.6 | ~200 |
| golem | 188.1 | 35.3 | 126.3 | |
| scavenger | 72.9 | 40.3 | 76.7 | |
| quadrupeds (wolf/boar/bison/snapper/shadowbeast), horizontal (matrix z flipped) | 123.9-152.7 | 29.3-55.2 | 78.9-107.8 (+ z offset) | |

A second, larger capsule (hero r 70.8, h 152.4; bandit r 66.2) is present in every human template; it is the character-vs-character / hit shape (I, group enums `Player`=2, `NPC`=3, `Movement_ZoneNPC`=11, `RagDoll`=17 exist in `eECollisionGroup` / `eEShapeGroup`).

## 3. Ragdolls

The original uses real PhysX ragdolls: `eCNovodexPhysicsController` (EMotionFX), `eCVisualAnimation_PS::SetRagDollEnabled/StopRagDoll/GetRagDollController`, `eCSphericalJoint(Desc)` with swing/twist limits, collision group `RagDoll` (17), shape groups `PhysicalBodyPart`/`KeyframedBodyPart`, script state `ZS_RagDollDead` (COMBAT_AI_RULES 2.11: lethal damage ragdolls; "dead as ragdoll" for non-player `ZS_DiveIn`) [V strings + script rules]. The per-bone joint limits come from the actor physics setup, which is **not decoded** [U]. `gothic_physics::ragdoll::Ragdoll` is a verlet stand-in on a 16-point humanoid skeleton (gravity, 0.1 damping, sleep, floor collision); body proportions are T. It is not bound to the skinned skeleton yet.

## 4. Implementation (crates/physics, crates/nav)

- `MovementProfile` carries every constant above with its tag; `MovementProfile::hero()` / `human_npc()` and `from_lookup` (template property names).
- `CharacterController`: ground / slide / jump / fall modes; sub-stepped; walls: capsule push-out with the lower sphere raised by `StepHeight` (obstacles under the step are climbed); ground: 5 rays in the footprint from `StepHeight` above the feet, averaged normals; `ground_follow_speed` 1000 cm/s is the only T value (hides the pop of `PutToGround`). `step_gait` for the hero/NPC driver, `step` keeps the old `wish` API. Events: `StepReport{landed (fall_time, impact_speed, damage), took_off, started_sliding}`, `ctrl.last_landing`.
- `crowd::separate`: capsule footprints do not overlap; idle characters are immovable (`CanBePushedWhileIdle` false) so the mover is deflected.
- `gothic-nav`: 40 cm cell mesh grown from seeds over the collision geometry, cells follow the controller's rules (step, angle <= 40.3 deg = where the NPC uphill scale hits its 0.3 floor, capsule clearance); A* with slope-weighted cost, string pulling by walkable-line tests, `PathFollower`, sampling-based local avoidance, `SharedNav` for the simulation. The original's authored nav zones (`gCNavigation_PS`, `gCNavZone`) are not decoded [U].

### Integration in the game

- `apps/game/src/walk.rs` `player_motion`: hero speeds now `Gait::Run` / `Gait::Sprint` through `step_gait` (400 / 600 cm/s, was invented 380 / 650), strafe / backward limited relative to the camera facing.
- `crates/runtime/src/world.rs` `PlayWorld`: `move_towards` follows A* paths with local avoidance and never ends a step inside another actor; `path_length` uses the mesh; both fall back to the old straight line where the mesh does not know the ground (so unseeded interiors do not freeze NPCs).
- `apps/game/src/play/nav.rs`: grows the shared mesh around the hero within the loaded collision tiles (seeds: hero and every NPC), links it into `PlayWorld`, and keeps the hero out of standing NPCs (`crowd::separate`).
- `apps/game/src/play/move_states.rs`: the host side of `OnMovementModeChanged` (S:adbf0). Mode changes and the landing of the step report reach `Sim::movement_mode_changed` (`crates/sim/src/moves.rs`): fall damage by the fall time (`floor(160 (t-0.9)^2)`, halved by `Perk_Acrobatic`, none into deep water or onto a slide), `Perk_Acrobatic` for a 7.45 s fall into water, `PS_Swim` (weapon away, swim ani state); the animation side plays `Fall_Hit` (hero stands still while it plays), `Slide_Begin` / `Slide_End`, the wade / swim pose clips and the dive-in clip. `OnSlide` / `PS_Slide_Jump`: jump out of a slide after 0.6 s. Water level: highest lake / river water-mesh surface under the hero (tile triangles, `WaterSurfaces` in `walk.rs`), else the sea level; dive with the jump (`Up`) / duck (`Down`) keys. Ragdoll binding is not wired.

## 5. Measurements (headless, cooked cache `desktop-high`, release build)

`cargo run --release -p gothic-nav --example ardea_path` (3x3 cells around Ardea, 56 679 terrain + 487 857 prop collision triangles, 0.18 s):

| item | value |
| --- | --- |
| nav bake, 11 000 cm disc, 40 cm cells | 210 526 walkable cells, 3-9 s single thread (machine load dependent); the game grows it incrementally, 1500 cells per frame |
| hero start -> Hamlar | 1248 cm path (straight line), 0.30 ms; controller walk 3.2 s, 376 cm/s mean incl. acceleration |
| perimeter point -> Hamlar (stand-in for the city gate, 8683 cm straight) | 9586 cm, 23 waypoints, 26 ms; controller walk 23.9 s, 397 cm/s, 366 cm total climb, steepest cell step 47 deg, 0 falls, 0 slides, arrived |
| old controller on the same routes | arrived; 380 cm/s invented speed, 45 cm step, 380 cm/s jump (73 cm apex), max single-frame height pop 30.6 cm vs 16.7 cm |

Unit tests: run 400 / sprint 600 cm/s, 0.4 s acceleration distance, ledge 60 cm climbed and 80 cm blocked, 150 cm drop falls while 60 cm is walked down, uphill speed at 35 deg ~0.48 x 400, 60 deg not climbable, 55 deg slides, jump apex 183.5 cm and 1.22 s, terminal fall speed 1500, fall damage, no tunnelling at sprint, ceiling, 200 characters separate, ragdoll lies flat and sleeps, nav detours around walls, 120 cm doorway passable, closed rooms unreachable.

## 6. Open items

Breath / drowning (no rule found in the script DLL); ladders (the original has none); the sprint trigger for the player [U]; turn rate (`TurnSpeedMax`, `TurnAcceleration`) is exposed in the profile but rotation is still driven by the camera/AI; quadruped body pitch; actor-physics ragdoll bone limits; authored nav zones; moving platforms (`ContributeFloorMovements`).
