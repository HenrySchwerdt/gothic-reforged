# VR mode (PC VR first)

Issues: #8 (epic), #2 VR-01, #3 VR-02, #4 VR-03, #5 VR-04, #6 VR-05, #7 VR-06.

Everything here is developed and tested with **synthetic content only** (greybox boxes,
capsule dummies, procedural weapons). No Gothic 3 assets, data or decompiled code are used.
Integration with the real game data (skeleton mapping, item stats, the damage formula) is a
local maintainer step; the hooks for it are listed below.

## Crates

| Crate | Issue | Contents |
| --- | --- | --- |
| `crates/vr` (`gothic-vr`) | VR-01, VR-02 | XR input model (head, two controllers, buttons, analog hysteresis), backend seam, desktop fallback (keyboard/mouse headset emulator, input scripts), stereo rig, controller buttons → core `Command`s, comfort locomotion driving `gothic_physics::CharacterController`, full-body IK, greybox level, debug rendering |
| `crates/vr-physics` (`gothic-vr-physics`) | VR-03 | avian3d 0.8, physics hands (strength-limited PD force/torque), grip points, two-handed grabs, throwing, procedural props, headless test rig |
| `crates/vr-melee` (`gothic-vr-melee`) | VR-04 | swept blade/blunt/pierce contact model, stabs (stick/pull-out), block/parry, `HitEvent`, hitzones, training dummy |
| `crates/vr-npc` (`gothic-vr-npc`) | VR-05 | active ragdoll (balance, stagger, fall, get-up), combat AI (telegraphs, attack slots, blocking, recoil), Bevy plugin |
| `crates/vr-archery` (`gothic-vr-archery`) | VR-06 | quiver/nock/draw/release, ballistic arrows that stick, aim assist, damage hook |
| `apps/vr-sandbox` (`gothic-vr-sandbox`) | all | greybox level with everything above |

The mechanics are Bevy-free modules (`input`, `desktop`, `locomotion`, `body`, `pd`, `grab`,
`contact`, `ragdoll`, `ai`, `bow`, `ballistics`) with thin Bevy/avian plugins on top, per
`docs/ARCHITECTURE.md`. Units: metres in tracking space, the Bevy world and avian; the Gothic
character controller and its `CollisionWorld` stay in centimetres (`gothic_vr::CM_PER_M`).

## Running

```sh
cargo run -p gothic-vr --example greybox                 # VR-01/02 demo, desktop fallback
cargo run -p gothic-vr --example greybox -- --stereo     # side-by-side eye views
cargo run -p gothic-vr --example greybox -- --headless   # scripted CI run (exit code)
cargo run -p gothic-vr-sandbox                           # everything: props, dummies, duel, range
cargo run -p gothic-vr-sandbox -- --peaceful --strength 120
cargo run -p gothic-vr-sandbox -- --headless             # scripted CI duel (exit code)
```

On Linux the windowed builds use X11 (`libxkbcommon-x11` is needed at runtime). Without a GPU,
Mesa's lavapipe works (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`, e.g. under
`xvfb-run`); `--screenshot out.png [--screenshot-frame N]` captures a frame and exits.

### Desktop debug controls

| Input | Emulates |
| --- | --- |
| W A S D | left thumbstick (move, head-relative) |
| Q / E | right thumbstick X (snap turn 30°) |
| T (hold, release) | right thumbstick forward: teleport arc, release to jump |
| right mouse + move | head yaw/pitch |
| mouse move | active hand in the view plane — fast mouse moves are sword swings |
| mouse wheel | active hand depth (pull back toward / over the shoulder) |
| Tab | switch the active hand |
| G / F | toggle grip of the active / other hand |
| left mouse | trigger of the active hand |
| Space / R | A (jump) / B (draw weapon command) |
| Left Shift | left stick click (sprint) |
| C | crouch (lowers the head: sneaking) |
| Esc | menu button |
| sandbox: 1 / 2 | active hand to the quiver / right hand to the bow string |
| sandbox: H, F1 | NPC hitzones, stereo |

### With a headset

Not possible in this build yet: no OpenXR integration for Bevy 0.20 has been released
(`bevy_mod_openxr` 0.6 targets Bevy 0.19). `GOTHIC_VR_BACKEND=openxr` is recognised and logs
that it falls back to the desktop backend. The backend seam is ready: a headset backend only
writes `gothic_vr::PendingXrFrame` (head + controllers in tracking space) before
`VrSet::Input` and drives the `XrEyeCamera`s' projection/targets. Details and the steps to
enable it in `docs/vr/OPENXR.md`.

## Tests

```sh
cargo test -p gothic-vr -p gothic-vr-physics -p gothic-vr-melee -p gothic-vr-npc -p gothic-vr-archery
```

All tests are headless and deterministic (fixed 90 Hz steps; avian runs inside a
`MinimalPlugins` app via `gothic_vr_physics::testing::Rig`). In CI they run in the separate
`vr` job on Windows and Linux, together with the two headless demos; see `docs/CI.md` for
costs and caching.

| Area | Tests |
| --- | --- |
| input / desktop / bindings / rig | hysteresis, edges, sanitising NaN, emulator reach, scripts, commands, eye poses |
| locomotion (greybox stairs/slopes) | head/hand-relative, walk/run/sprint, snap pivot about the head, smooth turn + vignette, crouch → sneak, seated lift, stairs, 25° vs 60° ramp, wall pushes the view back, teleport valid/invalid, determinism |
| body IK | recorded pose fixtures (`crates/vr/tests/fixtures/poses`): bone lengths, reach, feet on the floor, one foot at a time, idle look-around, turn in place, crouch, walking, reach/swing |
| physics hands | PD follow, walls stop the hand, grab/snap/carry, lag vs weight and strength, two hands, throw, stuck-hand snap, determinism |
| melee | fast swing cuts, slow swing harmless, waggle, flat = blunt, mace, no tunnelling, self-hits, re-hit cooldown, stab sticks / wiggle / pulls out, slow poke, block protects, counter-swing parries, determinism; plus physics hand + sword vs dummy |
| NPC | ragdoll stands, collapses, light hit staggers and recovers, heavy hit fells and gets up, push and grab, hitzones; AI telegraphs on the right side, edge-first strikes, attack slots, distance, blocking, recoil, disabled when down; headless duel (NPC hits player, player hit staggers NPC, overhead blocked → recoil) |
| archery | quiver over the shoulder only, nocking, draw power, small draw drops, off-axis loses the arrow, damage hook, ballistic range, drag, orientation, wall sticking, ricochet, body hit + carried along, no tunnelling, aim assist, determinism; physics range (10 m hit at full draw, half draw falls short) |

## Integration hooks for the real game

* **Damage**: melee produces `gothic_vr_melee::HitEvent` / `HitMessage`
  `{ attacker, victim, bone, damage_type (Edge/Blunt/Point), impact_speed, point, direction, quality }`;
  archery additionally `ArrowHitMessage { damage }` from the `ArrowDamage` trait (default
  `(ammo_damage + bow_damage) * draw_factor`). Feed these into the original damage formula.
* **Strength**: `gothic_vr_physics::VrStrength` (Gothic `STR`) limits arm force and torque
  (`pd::arm_limits`).
* **Skeleton**: `gothic_vr::body::Joint` / `gothic_vr_melee::Bone` are a generic humanoid;
  map them to Gothic skeleton bones for rendering, hitzones and ragdoll proportions.
* **Commands**: `gothic_vr::VrCommands` maps buttons to `gothic_runtime::Command`s and can
  send `HeroTransform` every frame (`send_hero_transform`).
* **Weapons**: replace `props::*` specs and `profiles::profile_for` with data from the item
  templates (mass, length, grip, blade/point geometry).
