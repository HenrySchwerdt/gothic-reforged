# Souls mode: tuning table (`crates/combat-souls`)

The Souls mode (`--combat souls`) changes only the *flow* of a fight. Damage numbers always
come from the ported Gothic pipeline through `HitSink`. Every number below is a field of
`gothic_combat_souls::Tuning` (or `FoeProfile` / `FeedbackTuning`), chosen for feel and
meant to be changed through profiles. Times are in seconds unless the unit says frames
(30 fps animation frames; one 60 Hz tick is half a frame). Distances are in cm.

## Hero

| Field | Default | Meaning |
| --- | --- | --- |
| `max_stamina` | 100 | Stamina bar |
| `stamina_regen` | 48 /s | Full regen rate |
| `stamina_delay` | 0.6 | Pause after spending before regen starts |
| `stamina_regen_start` | 0.35 | Regen starts at 35 % of the full rate... |
| `stamina_ramp` | 0.5 | ...and reaches full rate after this long (a short breath, then a fast refill) |
| `exhausted_delay` | 0.9 | Extra pause when the bar was emptied (`Exhausted` event) |
| `guard_regen_mult` | 0.5 | Regen multiplier while the guard is up |
| `sprint_drain` | 10 /s | Sprint cost |
| `roll_cost` / `backstep_cost` | 14 / 9 | Dodge costs |
| `guard_stamina_ratio` | 0.45 | Share of a blocked hit's damage taken from stamina (min 6) |
| `sprint_hold_frames` | 10 f | Holding dodge longer than this sprints instead of rolling on release |
| `parry_window` | frames 2–9 | Frames after the guard press in which a hit is parried |
| `guard_raise_frames` | 4 f | Frames for the guard to come up |
| `guard_counter_window` | 20 f | After a block, a heavy becomes a guard counter |
| `guard_arc_deg` | 80° | Half-angle the raised guard covers |
| `max_poise` / `poise_reset` | 40 / 6 | Poise and the time without hits before it refills |
| `hit_stop_scale` | 0.5 | Share of an attack's own hit-stop applied |
| `buffer_secs` | 0.4 | A buffered **attack** press stays valid this long |
| `buffer_dodge_secs` | 0.25 | A buffered **roll** stays valid this long. It is shorter so a late press does not roll you out of the fight after the moment has passed |
| `walk/run/strafe/back/sprint_speed` | 150 / 400 / 280 / 280 / 600 cm/s | Locomotion |
| `turn_free_deg` / `turn_locked_deg` / `turn_action_deg` | 900 / 700 / 360 °/s | Turn rates |
| `charge_frames` / `charge_max_frames` | 12 / 40 f | Heavy charge |
| `weapon_reach` / `hero_radius` / `riposte_reach` | 190 / 35 / 230 | Reach |

## Lock-on

| Field | Default | Meaning |
| --- | --- | --- |
| `lock_range` | 1500 | A new lock picks foes within this range, in sight, inside a ~100° view cone, scored by angle off the view centre plus a little distance |
| `lock_break_range` | 2300 | The lock breaks at once beyond this (hysteresis over `lock_range`) |
| `lock_los_grace` | 1.0 | Without line of sight (a pillar), the lock survives this long; seeing the target again resets the timer |
| `lock_retarget_on_kill` | true | When the locked foe dies, the lock moves to the next foe in view; otherwise it drops |
| `lock_switch_max_deg` | 100° | Switching (wheel / right-stick flick) only reaches foes within this angle of the current target, so it never jumps to one behind the hero. Within ~3° the nearer foe wins. If the target died this tick, a switch relocks on the nearest |

**Lock camera** (`LockCam`, steering the ported camera solver's orbit):
- It follows the target with a 0.12 s time constant.
- The base elevation is 0.34 rad: lower for close targets, higher for targets below.
- **Occlusion lift:** while the camera cannot see the target (`drive_with(.., occluded)`), the orbit rises by up to 0.35 rad (0.25 s time constant) to look over the obstacle, and settles back afterwards.
- **Close hold:** a target closer than 60 cm in the horizontal plane (pushed into the hero) does not turn the camera, so there is no spinning.
- The boom collision itself stays in `gothic-camera`.

## Feedback (`FeedbackTuning`, presentation only)

Every impact sends `Event::Feedback { cause, hit_stop, shake }`. The core freezes the hero for
`hit_stop`. Hosts shake the camera: the plugin's `CameraShake` resource sums decaying sines,
and `CameraShake::strength` scales or disables them, for example as an accessibility option.

| Cause | Hit-stop | Shake (amplitude cm, duration s, Hz) | When |
| --- | --- | --- | --- |
| `Parry` | 0.12 | 3.0, 0.18, 26 | A swing is parried |
| `Block` | 0.05 | 1.5, 0.12, 30 | Blocked with stamina left |
| `GuardBreak` | 0.15 | 9.0, 0.40, 16 | The block emptied the bar |
| `HurtHeavy` | 0.08 | 7.0, 0.35, 18 | Hit by a `Large` / `Knockdown` attack |
| `Stagger` | — | 2.5, 0.20, 22 | The hero breaks a foe's poise |
| `Riposte` | 0.10 | 5.0, 0.30, 20 | A riposte lands |

## Foes and telegraphs (`FoeProfile`)

| Profile | Wind-up | Tracking stops | Strike | Recover | Cooldown |
| --- | --- | --- | --- | --- | --- |
| `classic()` (original feel) | 0.20 | at the swing (no cue) | 0.18 | 0.45 | 0.5 |
| `telegraphed()` attack 1 / 2 / 3 | 0.75 / 0.94 / 1.13 | 55 % into the wind-up | 0.22 | 0.9 | 1.1 |

Readability events:
- `FoeWindup` and `FoeTelegraph { lead, track, parryable }` are sent when the wind-up starts.
  `lead` is the time until the swing is live. Presentation can show the raised weapon and a
  glow (the arena's foes light up) for exactly that long.
- `FoeCommit` is sent when the foe stops turning toward the hero. From here the swing direction
  is fixed: this is the moment to roll. The arena flashes a white ring.

For `telegraphed()` there are at least 0.25 s between the commit and the swing (tested). The
classic profile tracks until it swings, so it has no commit cue, like the original.

## Arena

```sh
cargo run -p gothic-combat-souls --example arena --features demo [-- --classic] [-- --shots]
```

The arena has three free-standing pillars (`Env` with line of sight and collision). `--shots`
plays a fixed script and writes `.local/souls-*.png`:
- lock-on;
- a telegraph;
- a roll;
- an attack;
- a parry, with its camera shake;
- the lock-on strafe (actor mode shows the strafe clips);
- a pillar between the hero and the locked foe: the camera lifts within the grace period;
- the lock breaking after the grace period.

## In the game (`--play --combat souls`, SOULS-03)

The hooks above are wired into `apps/game` by `apps/game/src/play/souls_wire.rs` (logic) and
`souls_hud.rs` (HUD); `souls.rs` only calls them. Nothing changes without `--combat souls`.

| Hook | How it is wired |
| --- | --- |
| Tuning | `Tuning::default()` plus a JSON override file: `--souls-tuning <file>` or `GR_SOULS_TUNING=<file>`. Keys are the field names above (`roll_cost`, `lock_los_grace`, ...), `parry_window: [from, to]`, `lock_retarget_on_kill: bool`, `feedback.<parry\|block\|guard_break\|hurt_heavy\|stagger\|riposte>.hit_stop` and `.shake.<amplitude\|duration\|frequency>`; nested objects and dotted keys are equal, keys starting with `_` are comments. An unknown key or bad value rejects the whole file (logged, defaults kept). |
| Shake scale | `--souls-shake <0..2>` / `GR_SOULS_SHAKE` / `"shake_strength"` sets `CameraShake::strength`; `0` turns the shake off (accessibility). The flag wins over the file and the variable. |
| `Env::line_of_sight` | A ray through the physics `CollisionWorld` (both in cm; the core already lifts both points 150 cm), stopping 20 cm short of the target so the floor or a wall at its feet does not count. |
| Lock camera occlusion | Each frame before `souls::drive`, a ray from the render camera to the locked target's chest (120 cm); the result goes to `LockCam::drive_with(.., occluded)` (lift reacts one frame late). |
| Feedback | Hero hits (riposte / stagger) come from the core. NPC swings are resolved by the sim, so the glue turns their mirrored outcomes into the same `Event::Feedback` (parry, block, guard break when the bar empties, `HurtHeavy` for `Large` / `Knockdown`) with the same hit-stop. The plugin's `CameraShake` offset (cm) is added to the render camera in `PostUpdate`, only on frames in which `play_camera` placed it (not in fly mode / cinematics). |
| Telegraph / commit | In the game the NPC brain owns the swing (`brain.strike`, held 650 ms by the sim's telegraph variant); a new strike aimed at the hero sends `FoeTelegraph { lead = time to the hit, track = 0.55 x lead, parryable: true }` and `FoeCommit` at 55 % of the lead. The sim keeps turning the NPC until the hit, so in the game the commit is a **timing cue only**, and `parryable` is not known from the sim (the original parry rules decide). |
| HUD | Bottom centre: the threat line (`WIND-UP 0.4 s`, white `COMMIT - ROLL` flash, `COMMITTED`), the last impact word (`PARRY`, `BLOCK`, ...) and the lock line (`LOCK <name>`, `NO SIGHT nn%` while the line-of-sight grace runs, `CAMERA LIFT` while occluded); a `V` marker above the locked target (amber during its wind-up). Hidden whenever the flow is not driving the hero. |

Example override file:

```json
{
  "_comment": "slower stamina, longer LOS grace, softer guard-break shake",
  "stamina_regen": 40,
  "lock_los_grace": 1.5,
  "parry_window": [2, 10],
  "feedback": { "guard_break": { "shake": { "amplitude": 5 } } },
  "shake_strength": 0.5
}
```

**Tested** (no game data, synthetic `CollisionWorld` and core): `cargo test -p gothic-reforged
--features viewer -- souls` runs the LOS / occlusion rays against a wall, the outcome -> feedback
mapping and hit-stop, the shake scale (0 = no offset, fly mode untouched), the strike ->
telegraph / commit tracker, the HUD model and lines, the JSON / flag / env loading, and the
occlusion, telegraph, HUD and shake systems in a headless Bevy app.

**Not verified in-game** (needs the installed game): that the lock breaks behind real pillars
after the grace and the camera lift looks right, that the 20 cm end margin and the 120 cm chest
aim suit real geometry, how the shake feels, whether the 55 % commit cue is useful against the
real NPC swings, the HUD layout and font, and that the existing `souls_basic` / `souls_er` runs
are unchanged. To check with real data:

```sh
cargo run --release -p gothic-reforged --features viewer -- --play --combat souls --qa souls_feedback
cargo run --release -p gothic-reforged --features viewer -- --play --combat souls --souls-shake 0 --qa souls_feedback
cargo run --release -p gothic-reforged --features viewer -- --play --combat souls --souls-tuning my_tuning.json
cargo run --release -p gothic-reforged --features viewer -- --play --combat souls --qa souls_basic
```

`souls_feedback` locks onto the start raider (checks the HUD lock line), holds the guard until
it swings and checks telegraph, commit, feedback and the camera shake (none with
`--souls-shake 0`); it logs the occluded / shaken frame counts and writes
`.local/qa/souls_feedback/NN_hud_{lock,telegraph,feedback}.png`. Occlusion has no scripted
assertion yet (no known pillar spot near the start); check it by hand by locking on and walking
behind a wall.
