# NPC combat AI: how the port runs it (night P2)

Companion of `COMBAT_AI_RULES.md` (the rules, `S:` = `scripts/Script_Game.dll` RVA, `G:` = `Game.dll`),
`WIRING_COVERAGE.md` and `WIRING_GLUE_SCAN.md` (rows G4, G6, G11, C6, C7, D5). This file says which
part of the original an NPC fight is made of now, where the port still uses a stand-in, and how it
was verified. Every number below cites a function or a data file; `[I]` marks what has no source.

## 1. Layers

```
ZS_Attack family (crates/script/src/combat, ported, S:14480..1a610)
   SelectCombatMove (S:4ec30) -> FAI_Active/Passive/Ranged/Magic (S:4ffb0, S:50370, S:50550)
   queues AiCommand::{SelectWeapon, Attack, Parade, JumpBack, Shoot, Cast, CombatGoto, WaitMs, ...}
 -> crates/sim/src/sim/ai_exec.rs   executes the commands for an actor
      Attack/Parade/JumpBack: the ported _AI_* command (begin_melee_command S:381a0 family,
        begin_parade_command S:46810, begin_jump_back_command S:46560) writes Action / AniState /
        hand and returns its instruction list; crates/sim/src/sim/npc_exec.rs runs the list
      SelectWeapon: crates/script/src/combat/weapon.rs (S:49600, S:4a030, S:49890)
 -> host (apps/game/src/play/npc_moves.rs)  plays the clip of every instruction and reports
      Command::NpcMoveHit (damage moment) and Command::NpcExecDone (clip ended)
```

An instruction is one `sAICombatMoveInstr` (G:0x101696e0): the clip of the move in the phase
(`Raise` / `Hit` / `Recover` / `Begin`) named by `sAICombatMoveStart` (G:0x1016abb0:
`<Family>_<Pose>_<Left>_<Right>_P0_<Action>_<Phase>_N_<Dir>`, `Dir` Fwd/Left/Right from the angle to the
target, a missing clip is retried with `Fwd`, a missing `Fwd` clip ends the instruction at once and no
blow lands) played at the speed of `sub_42a0` (`combat::commands::animation_speed`: 0.6 one-hander,
0.7 two-hander, power attack raise 1.5, parade 2.3, stumbles 2.0 / 1.3, tired bands). The damage of the
move lands when the `Hit` clip played 60 % of its length (`sAICombatMoveItlLoop` G:0x1016dd00, constant
G:0x10308308, `HIT_FRACTION`); the `Recover` clip plays on its own (`sAICombatMoveStartRecover`
G:0x1016e360). The swing speed of an NPC is therefore clip length / speed from the animation data, the
same mover logic the hero uses (`hero_moves.rs`).

* **Headless / unshown NPCs** (`Sim::set_host_drives_npc(false)`, the default): the same instruction
  list runs on timers made of `move_duration_ms` and `combat::commands::hit_window_secs` ([I]: there
  are no clip lengths without a body). `crates/sim/tests/*` use this.
* **Hit reactions**: a hit that locks the victim (`stagger_ms`) cancels its instruction list and, for a
  shown body, queues the stumble clip of the `AssessHit` result (`QuickStumble`, `StumbleL/R` by hit
  side, `ParadeStumbleL/R`, `HeavyParadeStumble`, `PierceStumble`; `combat::commands::start_stumble`,
  `animation_speed`). The old `queue_swing` / `queue_hit_reaction` overlays for NPCs are gone.
* **Casts**: `AiCommand::Cast` -> `Raise` + `Hit` clips of `Cast` / `PowerCast`; the spell leaves when the
  `Hit` clip begins (`_AI_Cast_Execute` S:39180, `run_cast` S:38710); `Recover` follows alone.
* **Parade**: the `Begin` clip (when `begin_parade_command` says one plays), then `AniState::Parade`
  stays up for 2 s at most (`parade_hold` of `ZS_Attack_Parade`, S:17c00) or until the next instruction
  sets another pose; `LocomotionInput.parade` shows the guard pose in between.

## 2. Weapon choice (`_AI_SelectWeapon`, S:49600)

`ZS_Attack_Loop` queues `_AI_SelectWeapon` every pass. Ported decisions (`combat/weapon.rs`):

| Fighter | Rule | Source |
| --- | --- | --- |
| no target | melee | S:4a030 |
| Mage (class 1) vs non-humanoid | magic | S:4a030 |
| Mage vs humanoid | < 4 m: player 2.5 m or less melee, else (story caster) magic; non-player: back evade free else melee; >= 4.7 m magic; between: keep (melee stays melee) | S:4a0c7, S:4a107, S:4a168 |
| Frost golem (species 49) | < 7.5 m, frozen or mana < 600: melee, else magic | S:4a1cb |
| any other | < 7 m melee; 7..10 m keep what it holds; >= 10 m Paladin magic, Skeleton / Golem / FireGolem / IceGolem melee, everyone else ranged (bow in the bow slot, else melee) | S:4a232, S:4a25f, S:4a2dc |
| spell of a mage | story casters (Xardas, Grok, Ningal, Tizgar, Saturas, Runak, only alone against the player): their summon / high spell; Goblin, FireGolem, Dragon fireball, IceGolem ice lance; below 40 % HP half the time `It_Spell_Healing`; target IceGolem fireball, FireGolem ice lance; a held fireball / ice lance is kept; else 50 / 50 | S:49890 |

`Sim::npc_select_weapon` applies the result (`_AI_HoldInventoryItems`): melee weapons are stowed in
`brain.melee_kit` while a bow or a spell is held; `It_Arrow` / `It_Bolt` go into the right hand; the spell
template is added to the inventory (`AssureItems`) and held in the right hand, a staff moves to the left
(`(Staff, Cast)` row of the hand table, COMBAT_AI_RULES 2.12). This is what makes the shaman cast
(`shaman_duel` "casts 0": the Orc shaman carries only `It_Staff_BladeStaff_01`, no rune; the original
creates the spell on demand) and what makes bow carriers shoot beyond 10 m. The presenter draws the
weapon the hands hold (`MountOverride`, `npc_weapon_mounts`; the sword goes to the back when the bow is
up). NPC mana is a fixed 1000 (S:35d40): the old "his mana was spent" check of `shaman_duel` asserted
something the original does not do and was replaced.

## 3. Other values that changed from [I] to a cited value

* Gait speeds: run 400, walk 150, sprint 600 (`ForwardSpeedMax` 400 x `SetSpeedByMode` modifiers
  0.375 / 1 / 1.5 of the 726 + 252 + 65 NPC templates, `MovementProfile::human_npc`), strafe and back 280
  (`StrafeSpeedMax`, `BackwardSpeedMax`) instead of 550 / 180 / 300. `GetCombatWalkMode` S:1b060 picks the gait
  (1 walk, 2 run, 3 sprint). Turn rate of the body 200 deg/s (`TurnSpeedMax`).
* Strike reach of an NPC: `GetAttackRange` (S:4e940) without the +50 slack the hero keeps.
* Bow: after `_AI_Shoot` the actor is busy for the raise clip of the weapon (`sAIPrepareAimInstr(1.0)`,
  S:4a8c5) instead of 1.5 s.
* Ammunition templates are `Arrow` and `Bolt` (`ZS_Attack_Shoot` S:18ed0 `AssureItems`; the port had `It_Arrow`
  / `It_Bolt`, which exist nowhere in the data: NPC archers could never shoot).
* Clip lookup (`npc_moves::npc_move_clip`): the primary-pose token `Pn` of the clip name follows the clip the
  last move ended in (`GetPrimaryPoseExt`; the script's `primary_pose` is `n + 1`), which alternates the hand
  of the quick attack. The animation data has no `HackAttack` and no `SimpleWhirl` clips: the source file of
  `_AI_SimpleWhirl` is `AI_TwirlAttack.cpp`, the two-hander's guard-pose swing is filed as `WhirlAttack`, the
  dual-wielder's `Stand` swing as `WhirlAttack` (`Hit`, `Recover`); until the engine's action -> clip table is
  recovered `HackAttack` plays the `WhirlAttack` clips and `SimpleWhirl` tries `WhirlAttack`, `TwirlAttack` [I].
* Projectile interpolation uses the core's 33 ms step (`shots.rs`, WIRING_GLUE_SCAN D6).
* Line of sight (G6): `PlayWorld::line_of_sight` asks the collision world (eye to eye, 150 cm [I]) one
  frame later (`los_service`); until answered a line is free. Perception outdoors keeps the "no check in
  the same zone" rule (S:8f70).

## 4. Not done / still stand-ins

* Damage: `sim::combat::assess_hit` is still the executing implementation; `script::damage::assess_hit`
  (S:2b580) is parity-tested against it (`script_host.rs` tests). Switching the engine path needs the
  application of `DamageAmount` / the tasks AssessHit sets (`ZS_Stumble` ...) to move into the sim first.
* `WantToFlee` is a stub in the shipped scripts (S:bd420); the optional low-HP flee variant of the sim is
  off by default and not original.
* `Shoot`: the arrow still leaves at the instruction, the aim pose is the overlay of `ranged.rs` (no
  Raise / Aim clip as script instruction; `Aim` clips are mode `O` overlays with pitch variants).
* Stop distance of `CombatGoto` is 110 cm [I] (`GetCloseRange` S:... 500 would deadlock the loop: the
  instruction would end at once); jump back has no clip for humans and orcs in the animation data, so
  those fighters do not hop (the engine rule for a missing clip).
* No attack-token logic beyond `UpdateCombatState` (S:14f0: 1 fighter, 2 / 3 against the player on
  normal / hard) because the original has no other.
* NPC bows: `npc_bows` places the bow actor at the slot bone and `npcs.rs` turns an aiming archer a quarter like the hero; the bow-in-hand rendering of archers was not confirmed on screen (bows on backs are), check `GR_BOW_GIZMO=1`.
* Root motion of a clip moves the NPC through `Command::NpcNudge`; the steering toward the target
  (`EnableCombatMovementFromSPU`) is not ported.

## 5. QA

`--play --qa <scenario>` (set `GR_QA_DIR=.local/qa/p2` to keep the output apart, `--record DIR` for frames,
`.local/qa/encode.sh DIR out.mp4`):

| Scenario | What it asserts |
| --- | --- |
| `npc_orc1` | one raider of the start: ZS_Attack_Goto / Loop / a swing state entered; every swing has a clip; clip speeds are the `sub_42a0` rule; the hero's hp drops land at 0.6 x clip / speed after the `Hit` clip started |
| `npc_orc2` | two raiders: at most two engaged at once (`UpdateCombatState`) plus the same swing checks |
| `npc_shaman` | the shaman holds a spell (weapon mode Magic), casts, missiles fly and hit |
| `npc_archer` | a bow carrier draws the bow at range, arrows fly |
| `npc_orc3` | three raiders: never more than two engaged, two really are (`UpdateCombatState`; a fight now starts with `CombatState` 0 instead of 1, which had bypassed it) |
| `npc_vs_npc` | a raider and a human fight each other: both lose hp, both play attack clips |
| `shaman_duel` | (existing) the shaman casts at an attacker |

Last results (release build of the branch, `.local/qa/p2/*.log`, gitignored): `npc_orc1`, `npc_orc2`: clips and
speeds PASS, damage within 0.07 s of 0.6 x clip / speed (22 hero hits checked), at most two engaged PASS;
`npc_shaman` PASS (casts 2 -> 35, spell missile on screen for 95 frames); `npc_vs_npc` PASS; `shaman_duel` PASS
(before: casts 0). `npc_archer` needed the `Arrow` / `Bolt` fix above.

## Single damage path

`Sim::apply_attack` resolves every hit through the ported `AssessHit` (`Sim::resolve_ported` in
`crates/sim/src/sim/hit_ported.rs`); the former `sim::combat` resolver is removed. Consequence checked
against the original: a spell projectile whose `PathStyle` is Missile (3) has its base damage doubled
(`AssessHit`, Script_Game.dll S:2b580, check at S:2c0f5; recorded `[V]` in `COMBAT_AI_RULES.md` 2.5). The
power cast of Fireball flies as such a projectile, so it hits twice as hard as a plain cast (170 against 85
for the test hero); `crates/sim/tests/magic.rs::a_power_cast_costs_the_maximum_mana` asserts this. The souls
sink's non-commit preview is a nominal `amount * hit_mult`; only the commit is exact.
