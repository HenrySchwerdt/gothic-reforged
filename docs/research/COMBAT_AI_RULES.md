# Gothic 3 combat, NPC behaviour and economy rules

Recovered 2026-10-20 from the installed 1.75.20108.14 build by address-scoped capstone disassembly of `scripts/Script_Game.dll` (and a few targeted probes of `Game.dll` / `Engine.dll`). REA/Ghidra was not run on Game.dll/Engine.dll (it times out). This file holds only prose, addresses, constants, tables and formulas; no decompiled bodies and no original content. Raw per-function listings and the tools that produced them live in gitignored `.local/research/rules/` (`README_TOOLS.txt`, `v.sh`, `k.sh`, `rawr.py`, `xs.py`, `impref.py`, `sd2.py`, `sp.py`; scratch sections `sec_*.md`).

Evidence legend: **V** = read from the instructions (address given), **I** = inferred (structure clear, entity roles or meaning guessed), **U** = not recovered. Addresses: `S:` = `scripts/Script_Game.dll` RVA (image base 0x10000000, same convention as GAME_RULES.md), `G:` = Game.dll RVA, `E:` = Engine.dll RVA. Enum numbering is from `.local/research/gl/enum-values.json` (implicit values continue the previous value).

Companion files: `GAME_LOGIC_MAP.md` (script host, registry), `GAME_RULES.md` (infos, quests, XP/level, teaching).

---------------------------------------------------------------------------

## 0. Calling model and engine switches

### 0.1 Script ABI [V]

Script functions are `f(unit, Entity* self, Entity* other, int arg)` with `ret 0x10`; a null Entity pointer means "use the unit's Self / Other entity". For reaction callbacks Self is the entity running the script (the one that was hit / that perceives) and Other is the instigator. State bodies (`ZS_*`, `PS_*`) are `(unit, ctx)` with `ret 8`. Helper subs take `Entity` by value. A global `None` entity is the default argument. Randomness is `Entity::GetRandomNumber(n)`; the only call in `AssessHit` is the min-damage fallback in 3.7, so **melee and ranged hits have no hit-chance roll in script** (hits are collision driven; ranged accuracy is aiming).

### 0.2 Engine setup flags read by the rules [V E:f1bf..f4a5, S: many]

`eSSetupEngine` is filled from `Ini/ge3.INI` section `[Game]`. The shipped file (Community Patch Team preset) sets:

| Offset | INI key | Default if absent | Shipped value | Meaning |
| --- | --- | --- | --- | --- |
| +0x1ea (byte) | `ExtendedContent` | false | **true** | "CPT extended content": switches many formulas (protection, NPC damage, XP, trade price, pickpocket) |
| +0x1eb (byte) | `AIMode` | false | **true** | "CPT extended A.I. mode": stamina-only parrying, parry restrictions |
| +0x1ec (u32) | `MinHitDuration` | 6 | 6 | min. duration of a hit reaction (0..15); consumer not located [U] |
| +0x1f0 (u32) | `XPModifier` | 100 | 100 | percent multiplier of kill XP (1..200, a value above 200 is reset to 100) |
| +0x1f4 (byte) | `QuickLoot` | true | true | right-click looting |

Everything below states both branches and marks the one the shipped configuration takes as **(shipped)**. A reimplementation should expose both flags; default them to true.

---------------------------------------------------------------------------

## 1. Damage pipeline overview [V]

```
engine contact / projectile / spell hit
  -> script callback OnHit        S:ad620   (picks the damage source item, calls DoLogicalDamage)
  -> DoLogicalDamage              S:b4f50   (gate: may this hit hurt at all? then calls AssessHit)
  -> AssessHit                    S:2b580   (damage number + reaction; returns a gEAction)
       callback OnDamage          S:ab220   is a thin wrapper that also ends in AssessHit
  -> engine applies PSDamageReceiver.DamageAmount to HP [I: no HP subtraction in the normal path of AssessHit]
```

Inputs per hit: the `PSDamage` property set of the damage entity (weapon, projectile, spell, natural weapon): `DamageType`, `DamageAmount`, `DamageHitMultiplier` (float), `ManaUsed` (int; spells), and `PSInteraction.GetOwner` (the real attacker). Where `DamageAmount`/`DamageHitMultiplier` are *written* for melee is inside the engine (no script writes them) [U]; for ranged the script writes the multiplier from the draw state (`PS_Ranged*`, `Shoot`, see section 9).

`gEDamageType`: None 0, Impact 1, Blade 2, Missile 3, Fire 4, Ice 5, Lightning 6, Physics 7.

### 1.1 Gate: DoLogicalDamage [V S:b4f50]

The hit is **ignored (returns 0, no damage, no reaction)** when any of the following holds (checked in this order):
- the weakness helper (section 2.4) returns 0 (immune) or the victim has no damage property set;
- the victim is the player and the invulnerability cheat is on;
- the victim, or the attacker, has status-effect bit 0x80 [I: invulnerable/ethereal; `HasStatusEffects(0x80)`];
- the victim's AI mode is Arena (5), Dead (9), Down (8) or Talk (6);
- the victim is the player while a GUI page is open;
- the victim is swimming or diving;
- the victim's AniState is SitKnockDown (15) or LieKnockDown (16) (a knocked-down character cannot be hit again);
- the attacker's current action is PierceAttack (11) or HackAttack (14) and the victim is not the attacker's current target (those two moves only hurt the target);
- the attacker is an NPC/creature (has an NPC property set) that is not the player and the victim is not that attacker's current target: ignored if the attitude is Friendly (1) or Neutral (2) [V compare chain, I pairing of the GetAttitude arguments] - accidental hits do not hurt allies or neutrals;
- the damage entity carries a spell or comes from a bare projectile (no NPC property set on the attacker): a spell/projectile of the player is discarded against a victim that is in AI mode Combat with `AttitudeToPlayer2 == Friendly`, or against a victim whose attitude would be Friendly and that is not in combat [I - the chain is long, structure verified];
- the spell is named `Spl_DestroyEvil` and the victim is not evil (2.6);
- the victim has status bit 0x40 and the attacker is farther than 700 units (V constant, I meaning);
- the victim has status bit 0x200 (soul travel / ghost form of the player): instead the script `ReturnToBody` (S:58470) runs on `PC_Hero_Ghost` and the hit is ignored.

When the gate passes, `AssessHit` runs and its return value (a `gEAction`) selects the **perception broadcast** [V]:

| AssessHit return | Meaning | Broadcast |
| --- | --- | --- |
| 32 LieKnockDown, 33 LieKnockOut, 37 LiePiercedKO | non-lethal defeat | `SendPerceptionDefeat` |
| 35 Die, 36 LieDead, 38 LiePiercedDead | killed | `SendPerceptionMurder` |
| anything else | ordinary hit | `SendPerceptionFightSound` |

Then a hit effect is started (material of the weapon in the hand slot; fist effect for unarmed), `SetLastInflictor` is set on the receiver, and `DoLogicalDamage` returns **0 for the parry results 20, 21, 24 and 46 and 1 for every other result** [V S:b5987 byte map].

---------------------------------------------------------------------------

## 2. AssessHit (S:2b580)

Roles: **victim** = Self (the entity being hit); **attacker** = owner of the damage entity (`GetOwner`; if there is no owner and the damage entity has an NPC property set, the damage entity itself). `P` = the player. Names below: `A_act` = attacker's current `gEAction` (PSRoutine.Action), `base`, `dmg` as variables of the pipeline.

### 2.1 Bookkeeping [V]
1. `PSDamageReceiver.DamageAmount := 0`.
2. victim.LastAttacker := victim.CurrentAttacker; victim.CurrentAttacker := attacker.
3. If `A_act` is SprintAttack (9) or PowerAttack (2): when the victim is the player its LastHitTimestamp is reset to 0 and the attacker's Action property is forced to PowerAttack; otherwise `A_act` is treated as 2 locally. (Sprint and power attacks therefore share all later rules.)

### 2.2 Player-only stealth/non-lethal special cases (attacker == P) [V S:2b748..2bac1]
Evaluated before the normal pipeline; each ends the function.
- **Non-lethal impact projectile.** Damage entity is a non-spell projectile, victim is humanoid, `DamageType == Impact`: `d = round(Amount * HitMult) + SkillBonus(d)`. If `victim.HP - d <= 0`: the receiver's DamageAmount is set to `d` (only if > 0) and the victim goes to **ZS_Unconscious**, returns 32 (LieKnockDown). Else if `HitMult >= 0.7`: ZS_SitKnockDown, returns 29. Otherwise fall through to the normal pipeline.
- **Backstab (victim AIMode == Routine and the victim does not have the attacker in its field of view):** if `A_act == PierceAttack (11)` and `Perk_Assassin` is active: victim HP := 0; if the victim `CanBeKilled` (2.7) then humanoid -> ZS_PiercedDead (returns 38) else ZS_RagDollDead (36); if it cannot be killed -> ZS_LieKnockDown (32). If `A_act` is PowerAttack/SprintAttack, `DamageType == Impact` and `Perk_KnockOut` is active -> ZS_LieKnockDown, returns 32, no damage.

### 2.3 Hit strength class `v` (drives stagger / knockdown) [V S:2bac1..2bbab, I for the helper semantics]
- spell damage: `v = 2`.
- projectile (non-spell): `v = 2` if `HitMult >= 0.6` else `0`.
- melee: `v = max(1, M(A_act) + R(attacker) - D(victim))` (0 if there is no attacker) where
  - `M(act)` (S:2b3a0), action id -> value: Attack(1) 2; PowerAttack(2) and SprintAttack(9): 4 normally, but 2 (3 if the attacker's StatePosition >= 2) when the attacker wields a one-hander in each hand; SimpleWhirl(6), WhirlAttack(10), PierceAttack(11), GetUpAttack(30): 2; HackAttack(14): 4; QuickAttack*(3-5), Turn*(7,8), JumpAttack(12), RamAttack(13), all other ids: 0.
  - `R(e)` (S:2afc0) = "power rank" 0..3: for the **player** from the equipped weapon perks - two 1H weapons: `Perk_1H1H_SecretBonus` 2, `Perk_1H1H_2` 1; one 1H: `Perk_1H_3` 2, `Perk_1H_2` 1; axe/2H/staff: `Perk_Axe_3`/`Perk_Staff_3` 2, `_2` 1 (the axe case id 0x33/0x34/3, staff 0xc; exact 2H handling [I]); for an **NPC** from its level: >=45 -> 3, >=30 -> 2, >=15 -> 1, else 0.
  - `D(victim)` (S:2b470) = 2 if the victim's action is GetUpParade (31); else `R(victim)` plus 1 if it holds a shield (UseType 9 in slot LeftHand) - for the player only with `Perk_Shield_2`, for an NPC always.

### 2.4 Damage-type weakness helper (S:7250) [V]
Returns 0 immune (blocked in the gate), 1 normal, 2 weak (damage x2 in 2.5). Returns 0 when the victim has no NPC property set or the damage type is None.

| DamageType | Rule |
| --- | --- |
| Impact | x2 vs Skeleton(1) and Golem(3); else 1 |
| Blade, Physics | 1 |
| Missile | immune: Golem(3), FireGolem(48); Skeleton(1) and IceGolem(49) immune unless the *damage entity name contains "Fire" or "Explosive"* (then 1); else 1 |
| Fire | immune: FireVaran(22), FireGolem(48), Dragon(52); x2 vs IceGolem(49); else 1 |
| Ice | immune if the victim has status-effect bit 2 (frozen) [I]; x2 vs FireGolem(48); immune IceGolem(49); else 1 |
| Lightning | x2 vs Golem(3); else 1 |

### 2.5 Raw damage and attacker bonuses [V]
`b = trunc(DamageAmount * DamageHitMultiplier)`. Then:
- weakness x2 if 2.4 returned 2;
- **head shot x2** if the damage entity touched the victim's collision shape (PSCollisionShape type 100) on a bone whose name contains `_Head`;
- store `b0 = b` (used by NPC formula).

Then by who attacks:

**Attacker is the player** (and not transformed):
- Spell damage: if the damage entity is a spell *projectile* (S:2af20): `b = 2 * min(b, 100)`. Attribute bonus `e = (ManaUsed == 0) ? 2.0 * b : b * INT / 100` (INT = the player's Intelligence; ManaUsed is the mana actually spent).
- Physical: `e = DEX/2` for projectile weapons, `STR/2` for melee (integer halves; `e >= 0`).
- `b += QualityBonus(b)`; `b += SkillBonus(b)`; `dmg = b + e`.
- `QualityBonus` (S:6af10) from the weapon `PSItem.Quality` flags: flag 0x100 (damaged): `-b/2`; each of the flags 0x20, 0x40, 0x80 present: `+10`.
- `SkillBonus` (S:2e410), only for the player (+25 means flat 25):
  - magic: matching school perk adds `+b` (i.e. +100%): Firemage for Innos(2) spells, Watermage for Adanos(0), Darkmage for Beliar(1);
  - bow (UseType 4 arrow): `Perk_Bow_3` sets 25, `Perk_Bow_2` +25; `Perk_Hunter_1` adds `+b` against "game" animals (S:57fc0), `Perk_Hunter_2` adds `+b` against other animals (S:57bd0); `Perk_OrcHunter` +25 vs Orc(5);
  - crossbow: `Perk_Crossbow_3` sets 25, `Perk_Crossbow_2` +25 (orc/evil bonuses as for bows [I]);
  - melee: two 1H weapons: `Perk_1H1H_SecretBonus` sets 25, `Perk_1H1H_2` +25; 1H: `Perk_1H_3` sets 25, `_2` +25; axe-class/2H: `Perk_Axe_3` sets 25, `Perk_Axe_2` +25; staff: `Perk_Staff_3` sets 25, `_2` +25;
  - any physical: `Perk_OrcSlayer` +25 vs Orc(5); `Perk_Paladin` +25 vs evil creatures (IsEvil, section 2.6).

**Attacker is not the player (or the player is transformed):** [V structure, I some entity roles]
- If the attacker has no NPC property set: `dmg` stays `b`.
- Else the weapon is *ignored* and the NPC's own Strength is used:
  - attacker Level < 2 or `ExtendedContent == 0`: `b = STR`.
  - `ExtendedContent == 1` (shipped), Level >= 2: if the attacker is not humanoid, or the damage is a projectile/spell: `b = STR`. Humanoid melee: against **a non-player victim**: weapon name "Fist" (unarmed) -> `STR/2`, else `STR`; against **the player**: unarmed -> `trunc(0.75*STR)`; Orc(5) -> `trunc(0.75*STR + b0/7)`; any other humanoid -> `trunc(0.9*STR + b0/9)`.
- Monster-vs-NPC nerf: if the victim is humanoid, the attacker is **not** humanoid, and neither is the player, `b = b/10`, or `b/2` when the attacker is a Summoned party member (party type 5) [V S:2bff3..2c07e].
- Spell projectile hits: `b *= 2` again (S:2af20 check at 2c096), and if the projectile `PathStyle == Missile (3)`: `b *= 2` [V S:2c0f5].

### 2.6 Species helper sets [V]
- `IsHumanoid` (S:10f20): species Human(0) or Orc(5) (the entity must exist).
- `IsAmbientCreature` (S:10140): Lizard, Fish, Meatbug, Vulture, Rabbit, Chicken, Molerat, Snake, Spider, Turtle, Bird, Ray, Scorpion, Seagull, Walrus, Whale, Eagle (species 24-47 minus Deer, Pig, Cow, Gargoyle, SwampLurker, id 40, Ogre).
- `IsEvil` (S:10aa0): undead (S:112f0) or species Golem(3), Demon(4), Gargoyle(38), FireGolem(48), IceGolem(49), ScorpionKing(50).

### 2.7 Protection [V S:2de50, S:2c10e..2c255]
`Prot = GetProtection(victim, damageType)`: the victim's matching protection attribute (`GetProtectionBlade/Impact/Missile/Fire/Ice/Lightning`, S:34560..34b60); Physics and None use Impact. Base value: **player** (untransformed): `PlayerMemory.Protection<Type>` (equipment-fed), divided by 5 when `ExtendedContent` is on (shipped) and capped at 80; **NPC / transformed player: `min(80, trunc(1.5 * LevelMax))`** (section 2.13); Fire and Ice get `+10` under a perk/skill condition [I: Perk_Resist* check]. The armor-perk adjustments below are applied on top.
- `ExtendedContent == 0`: robe armor (chest item `PSItem.IsRobe`) + `Perk_LightArmor`: x2; non-robe + `Perk_HeavyArmor`: `+Prot/4`; cap **90**.
- `ExtendedContent == 1` **(shipped)**: the base value is kept; with `Perk_LightArmor` (robe) the chest item's matching `PROT_<TYPE>` modifier (names `PROT_BLADE/IMPACT/MISSILE/FIRE/ICE/LIGHTNING`) is added at **+20 percent of that modifier**; with `Perk_HeavyArmor` (non-robe) at **+5 percent**; no cap [I: the six modifier slots of the item are scanned by name]. (GAME_RULES.md 4.2 describes this less precisely.)

Applying it (after 2.5):
- Percent mode: `dmg = b - trunc(Prot * b / 100)` - used when the victim is **not the player**, or `ExtendedContent == 0`, or the player is transformed, or the damage is a projectile/spell.
- Flat mode **(shipped, player victim hit by melee or a natural attack)**: `dmg = b - Prot`. If `dmg < 1`: when `b > 39`, or `A_act` is HackAttack(14)/PowerAttack(2)/SprintAttack(9): `dmg = ((rand(2) + 1) * b) / 100` (a 1-2 percent chip); otherwise dmg stays <= 0 and becomes 0.

### 2.8 Attack-move multipliers [V S:2c255..2c2c8]
On the attacker's current action (after protection): QuickAttack/R/L (3,4,5): `dmg /= 2`; HackAttack (14): `dmg *= 2`; PowerAttack (2) and SprintAttack (9): `dmg *= 2` if the attacker holds a one-hander in each hand, or its StatePosition == 2 [I: entity of the dual-wield test]; others unchanged. Negative results are clamped to 0.

### 2.9 Defence reactions [V S:2c2ed..2cb4b]
Order of checks after 2.8 (the first that applies ends the function):
1. **Magic parry** (spell damage and `CanParadeMagic(victim)`, S:dd70: victim holds a magic-parry staff, is in parade mode, attacker in its field of view): mana is charged instead of HP: `cost = dmg`; with `ExtendedContent` (shipped) `cost = trunc(dmg * f)`, `f = 1 - (INT - 200)/500` clamped to [0,1] (victim INT <= 200 pays everything, INT >= 700 pays nothing). `AddManaPoints(-cost)`; any shortfall (`min(0, mana - cost)`) is added to HP as damage. If HP > 0 the victim plays ZS_ParadeStumble and the function returns **46 (MagicParade)**.
2. **Missile parry** (`CanParadeMissile`, S:de80: the victim wields a shield together with a one-hander (UseType pair 9/2 on the hand slots), is in parade mode, attacker in its field of view): stamina cost `dmg` (`dmg/2` with `Perk_Shield_2` for the player), `AddStaminaPoints(-cost)`, the shortfall `min(0, stamina - cost)` goes to HP. If HP > 0: returns **21** (ParadeStumble) when `v > 0`, else the stagger table of the melee parry.
3. **Melee parry** (`CanParade(victim)`, S:d480: parade mode (S:d870), the victim is not itself in a HackAttack, and with `AIMode`: the attacker's action is not PierceAttack(11), WhirlAttack(10), FinishingAttack(15) or SprintAttack(9); attacker must be in the victim's field of view; plus S:d640 move check):
   - stamina cost `c = dmg/2`; with a shield (UseType 9 / 2) `c = c - c/4`, with `Perk_Shield_2` (player) `c = c/2`.
   - `AIMode == 1` **(shipped)**: `AddStaminaPoints(-c)` only (no HP loss at all, stamina is the buffer), weapon-clash effect, then the AI combat-move instruction `sAICombatMoveInstr` (G) and a stagger by `v`: `v <= 1` -> returns **20** (QuickParadeStumble), `v == 2` -> **21** (ParadeStumble), `v >= 3` -> **24** (HeavyParadeStumble); tasks ZS_ParadeStumble / ZS_HeavyParadeStumble.
   - `AIMode == 0`: `AddStaminaPoints(-c)`; the shortfall `min(0, stamina - c)` goes to HP (`AddHitPoints`); same stagger codes; if HP <= 0 the death branch of 2.10(4).
4. no parry applies: the hit lands (2.10).

### 2.10 Landing the hit: final adjustments, death, knock-out, stagger [V S:2cb4b..2d14e]
1. **Immortal protection:** when the victim is not the player and `CanBeKilled(victim)` is false (S:2da00): `dmg = min(dmg, 30)` and `dmg := 0` if it would still exceed the victim's HP (it can never be reduced to 0).
2. With `ExtendedContent` (shipped): if the victim is not the player and its AttackReason is Arena (18) (duel/arena): `dmg /= 2`; if the victim is not the player, the damage entity is a non-spell projectile and `GetCurrentDifficulty() == 2` (hard): `dmg = trunc(dmg * 1.2)` [V compare chain; the role of the compared entity is I].
3. `PSDamageReceiver.DamageAmount := dmg` (if > 0). Fire damage that `CanBurn` starts a burning status with effect `eff_magic_firespell_target_01`.
4. If `HP - dmg <= 0`: if `IsDeadlyDamage` (2.11) -> ZS_RagDollDead, returns **36** (LieDead); else if the attacker's action is PierceAttack (11) and the victim humanoid -> ZS_PiercedKO, returns **37**; else `v >= 3` -> ZS_Unconscious, returns **33** (LieKnockOut), `v < 3` -> ZS_LieKnockDown, returns **32**.
5. Survived: if the victim is itself in SprintAttack (9) the hit does not interrupt it (returns 0). If `v <= 0` -> `PipiStumble` (S:22f70). Otherwise status effects by damage type: poison (`CanBePoisoned`, bit 4) for pierce/hack hits, disease (bit 8) if `CanBeDiseased`, then:
   - PierceAttack (11) vs humanoid -> ZS_PierceStumble, returns **34**;
   - Ice damage on a victim that `CanFreeze` (S:2d7b0) -> ZS_Freeze with TaskPosition (duration units, game time [U]) `12 * max(5, dmg/20)`, returns 0;
   - weak/non-humanoid or S:3240(victim weapon class)==0 -> ZS_Stumble, returns **26**;
   - humanoid with `v >= 4` and weapon class 1 -> ZS_SitKnockDown, returns **29**;
   - `v >= 2` -> ZS_Stumble **26**; `v < 2` -> ZS_QuickStumble **25**.
   (S:3240 = `WeaponMode` of the victim: 1 melee, 2 ranged, 3 magic, 0 none/other, from the hand-item table in 2.12.)

### 2.11 Kill or knock out? IsDeadlyDamage (S:2db80) [V]
Returns "lethal" (1) when the victim `CanBeKilled` and any of: victim is a Boss (S:10bd0); name is "Xardas"; damage is a spell; damage is a non-spell projectile and the damage type is not Impact; damage is a non-Impact projectile vs a non-humanoid; for **melee**: no attacker, or attacker action is FinishingAttack (15), or the victim is not humanoid, or `GetAttitude(victim -> attacker)` is Hostile (4) or Panic (6), or `GetAttitude(attacker -> victim)` is Hostile (4). Otherwise the defeat is **non-lethal** (the victim is knocked out and later wakes up; the Defeat path, section 7.2). Impact projectiles against humanoids are never lethal.

`CanBeKilled` (S:2da00): see 7.3.

### 2.12 Hand-item combination -> weapon mode [V S:3865..40de table, S:2dd0 lookup, S:3240 mapping]
The script builds a static table keyed by the `gEUseType` of the two hand slots (key order: slot 2 = LeftHand type, slot 1 = RightHand type as passed to the lookup; the first key is the one named first below) giving a class: 0 invalid, 1 use/hold an ordinary item, 2 one-hand carry / tool idle, 3 carry, 4 **melee ready**, 5 **ranged ready**, 6 **magic ready**, 7 ammo only, 8 nothing. `WeaponMode` (S:3240): class 4 -> 1 (melee), 5 -> 2 (ranged), 6 -> 3 (magic), anything else 0. Rows (first key, second key -> class):
- 4 melee: (None,1H), (1H,1H), (Shield,1H), (Torch,1H), (None,2H), (None,Axe), (None,Staff), (None,Halberd), (None,Fist), (PhysicalFist,PhysicalFist), (None,Broom/Rake/Shovel/Pickaxe).
- 5 ranged: (Bow,None), (Bow,Arrow), (CrossBow,None), (CrossBow,Bolt); 7 ammo only: (None,Arrow), (None,Bolt).
- 6 magic: (None,Cast), (Staff,Cast).
- 2 idle: (None,None), (1H,Hammer), (Broom|Rake|Shovel|Pickaxe,None), (Torch,None), (2H,None), (Axe,None), (Staff,None), (Halberd,None); 3 carry: (CarryFront,None), (CarryShoulder,None); 0: (Shield,None).
- 1 use items: (1H,None), (None,Hammer), potions/plants/meat/fruit/bread/bottle/cup/bowl/brush/smoke/pipe/scoop/fan/saw/map/book/letter combinations (None or Torch or Staff with the item).
The table is also what makes a one-hander in the left hand alone an item-use pose rather than a weapon.

### 2.13 NPC attributes are derived from level in script [V, high value]
World NPC records carry only Level/LevelMax (hit points are 1/1 placeholders in the data); every attribute the rules read comes from these getters (all `S:`), for any entity that is **not** the player (or is a transformed player). `L` = `GetLevel` (S:36cb0): the NPC's `Level` property, **+10 on hard difficulty (EDifficulty 2), -10 on easy (0)**, minimum 1; `Lmax` = `GetLevelMax` (S:36db0): the same adjustment applied to the `LevelMax` property. (The player is never adjusted.)

| Quantity | NPC formula | Address |
| --- | --- | --- |
| Strength, Dexterity, Intelligence | `10 + trunc(1.5 * CurrentLevel)` | S:33540 / 33630 / 33720 |
| Smithing, Theft, Alchemy | `10 + trunc(0.5 * CurrentLevel)` | S:33900 / 339f0 / 33810 |
| Max hit points | `max(100, 20 * Lmax)`; **1 for ambient creatures** (2.6) | S:35660 |
| Hit points (current) | `PSDamageReceiver.HitPoints`; reset to `HitPointsMax` by `RefreshHitPoints` (S:ab840, also resets stamina) every time the NPC enters processing range, and by `Respawn` | S:35560, ab7b4 |
| Max stamina | `max(100, 10 * Lmax)`, doubled for non-humanoids | S:36440 |
| Mana / max mana | constant **1000** | S:35d40 / 35e10 |
| Protection (all six types) | `min(80, trunc(1.5 * Lmax))` | S:34560..34b60 |

`CurrentLevel` (S:36b40): for a transformed player `(LevelMax + Level)/2` (min 1); for an NPC `min(GetLevel + <a second Level read>, Lmax)` - the second term is not resolved [U], so treat `CurrentLevel ~ L` (capped by Lmax) until it is checked. NPC melee damage is `STR` (2.5), so an NPC of level L hits for about `10 + 1.5 L` before protection; `GetDiffToPlayerLevel` (S:36eb0) is used by AI decisions.

New-game player values (`OnGameStartUp` S:ad180) [V]: HP max/current 200, mana 100/100, stamina max 150 (ExtendedContent) or 100, stamina 100, STR 100, DEX 100, INT 0, Smithing/Theft/Alchemy 10, all protections 0, LP 0, chapter 1; it spawns an entity at a fixed start position and runs the first quest. The player's level, XP, LP and attributes live in `PSPlayerMemory` (not derived).

---------------------------------------------------------------------------

## 3. Experience (GiveXP) [V S:4e160, S:4e800]

`GiveXP(unit, source, receiver, amount)`; kills call it with `amount = -1`. Rules:
- `source == player` -> nothing. The receiver must be the player or a member of the player's party (leader chain compared against the player); party type Summoned (5) receives nothing [V S:4e1ec..4e305, I on the exact chain]. (Same as GAME_RULES.md 4.1.)
- If `source` is the world entity or None (quests/info scripts): `xp = 5 * amount`.
- If `amount == -1` and the source has an NPC property set (kill / defeat): `L` = source level.
  - `ExtendedContent == 0`: `base = 10*L`; ambient creature: `max(base, 25)`; otherwise `max(base, 50)`.
  - `ExtendedContent == 1` **(shipped)**: ambient creature -> **10** flat; Pig(33) or Cow(34) -> **20**; otherwise `max(10*L, 50)`.
  - `xp = 5 * floor(XPModifier * base / 500 + 0.5)`; with XPModifier = 100 this is `base` rounded to a multiple of 5.
- explicit non-negative `amount` from a non-world source: `xp = amount` unscaled.
- `PlayerMemory.XP += xp`; the message `GO_XP + <xp>` is printed. Then while `XP >= XPNext(Level)`: `Level += 1`, `LPAttribs += 10` (+1 more with `Perk_Learn`), messages `GO_LevelUp`, effect `eff_event_levelup_01`. `XPNext(L) = 250*(L+1)*(L+2)` (cumulative). The check is unrolled twice.
- **Who calls it:** `Kill` (S:1bc30) and `Defeat` (S:27510) when the killer/defeater is the player or a member of the player's party and the victim's `DefeatedByPlayer` flag is not yet set; the flag is then set, so **a victim yields XP only once** (defeat then kill pays once). `PickPocket` (S:3dd70) gives `L/7` XP in ExtendedContent mode (section 8.2); info scripts give `5*N` (GAME_RULES 4.1).

---------------------------------------------------------------------------

## 4. Death, knock-out and the player's own death

### 4.1 Kill (S:1bc30) [V]
Sets `LastFightTimestamp`, stops effects, `ResetAll`/`ResetInteraction`, drops non-combat items (`RemoveNonCombatItems`) and hand items, sets **AIMode := Dead (9)**, disables pushing, hands party leadership to another member (or empties the party), prints `GO_GameOver` (message type 13) if the victim is the player (otherwise sets CombatState), calls `PSQuestManager::OnNPCKilled`, awards XP (section 3), `NotifyEnclave`, clears the current destination point, `CleanUpPlunderInv` (strips loot not meant to be plundered). Special hard-coded case: when the victim is named "Talchef" the routines of the named mine NPCs (`Mine_Warrior_01..03`, `Mine_Elite_01..03`, `Mine_Scout_01..02`) are switched to routine "Mine_Attack" [V S:1bfb6..1c191].

### 4.2 Defeat (S:27510) [V]
For non-lethal outcomes: same cleanup as Kill but AIMode := Down (8), `MemorizePlayerCrime` for guards/bosses (attack crime, see 10.4), `OnNPCDefeated` to the quest manager, XP once (section 3), AttackReason reset. The wake-up (ZS_Unconscious / ZS_Unconscious_Loop / _End, S:26cd0..27360) is in 9.4.

### 4.3 CanBeKilled (S:2da00) [I]
Reads the NPC "can be killed" configuration (essential NPCs are immortal); the immortal rule in 2.10(1) caps damage at 30 for non-players.

---------------------------------------------------------------------------

## 5. Falling, regeneration and resource rates

### 5.1 Fall damage [V S:adbf0 OnMovementModeChanged]
When a character leaves the Falling movement mode (previous mode 0xd) into one of the landing modes and has **not** the invulnerable bit (0x80) and is not the god-mode player and not species Bloodfly(23): let `t` = seconds in the fall state (`ZS_Fall` state time). If `t > 0.9`: `dmg = floor(160 * (t - 0.9)^2)`. With `Perk_Acrobatic` the damage is halved and the ZS_Acrobat state plays. HP -= dmg (the NPC/Player must have `CanBeKilled` true to die: HP <= 0 -> ZS_RagDollDead). Examples: 1.5 s -> 57, 2 s -> 193. Land (S:ae740) / HeavyLand (S:ae9b0) states only play animations [V].

### 5.2 Tick regeneration (`OnTick` S:b0ef0, S:b0360, S:b0520) [V constants, I interval]
`OnTick` is driven by `gCNPCAdmin::TickNPCs` (G:142ab0) for entities registered with `AddTickedNPC` (G:142b70; the first tick is randomised inside the interval; the interval field is not resolved, assume 1 s [I]). For the player, only when AI mode is not Dead and (with ExtendedContent) no GUI page is open:
- **HP per tick** (S:b0360): `Perk_MasterGladiator`: ExtendedContent -> `min(HPmax/80, 5)`, but 0 while AI mode is Combat (3); no ExtendedContent -> flat 5. Without the perk: only while sitting (AniState SitGround 6, SitStool 7, SitBench 8, SitThrone 9): `min(HPmax/100, 5)`; otherwise 0 (**no passive HP regeneration in Gothic 3 without the perk**). Then `-10` inside a magic barrier, `-5` poisoned, `-5` burning (these subtract for any ticked entity).
- **Stamina per tick** (S:b0520): sprinting `-10` (`Perk_Sprinter` `-5`); with ExtendedContent `-8` (`-4`); interaction use / species Bloodfly: `-1`; jumping `0`; diseased `+1`; zone temperature above 40 or below -40: `+2` only with `Perk_ResistHeat` / `Perk_ResistCold`, else normal; otherwise **`+10`** [I on the precedence of the temperature clause].
- **Mana per tick**: S:b0790 not decoded [U].
- the player's LastHitTimestamp decays by 50 per tick (floor 0) [V]. `MinHitDuration` (6) is probably compared against it [I].

---------------------------------------------------------------------------

## 6. Perk names observed in script code
`Perk_1H_1..3`, `Perk_1H1H_1/2/SecretBonus`, `Perk_Axe_1..3`, `Perk_Staff_1..3`, `Perk_Bow_1..3`, `Perk_Crossbow_1..3`, `Perk_Shield_1/2`, `Perk_Assassin`, `Perk_KnockOut`, `Perk_Acrobatic`, `Perk_LightArmor`, `Perk_HeavyArmor`, `Perk_Sprinter`, `Perk_Sneak_1/2`, `Perk_PickPocket_1..3`, `Perk_LockPick_1..3`, `Perk_MasterThief`, `Perk_Barter`, `Perk_Excuse`, `Perk_Learn`, `Perk_MasterGladiator`, `Perk_MasterMage`, `Perk_Firemage/Watermage/Darkmage`, `Perk_Druid`, `Perk_Hunter_1/2`, `Perk_OrcHunter`, `Perk_OrcSlayer`, `Perk_Paladin`, `Perk_ResistHeat/ResistCold`, `Perk_ImmuneToDisease/Poison`, `Perk_Smith_1..3`, `Perk_Mining`, `Perk_Potion*`, `Perk_Trophy*`, `Perk_FireArrow/PoisonArrow/ExplosiveArrow/SharpArrow`, `Perk_PoisonBlade`, `Perk_SharpenBlade`, `Perk_StaffMagic`. Perks are skill items checked with `PSInventory::IsSkillActive`. Effects verified so far are listed in sections 2-3, 5, 8.

---------------------------------------------------------------------------

## 8. Economy

### 8.1 Trade price (S:6b550 OnPlayerGetGoldCost) [V]
`price = max(1, trunc(GoldValue * m))` where `GoldValue` is the item template value and `m` is built as follows (bit-exact on the structure, constants V):
- `m0 = 1.0` when the **NPC sells to the player** (inventory owner is the NPC); `m0 = 0.3` when the **player sells** (owner == player). When the player sells and the item is in trade category Trophy (2) with UseType 41 (TrophySkin) or 50 (TrophyFur), or category Valuable (1) with UseType 31 (Valuable): `m0 = 0.6`.
- barter factor `b = 1.0`; with `Perk_Barter` (player): `b += 0.1` when selling, `b -= 0.1` when buying.
- friendly NPC (`GetAttitude == Friendly`): `b += 0.1` when the player sells, `b -= 0.1` when the player buys.
- `m = m0 * b`; when the player sells and `ExtendedContent` (shipped): `m -= 0.05`.
So with the shipped flags and no perks the player receives **25 percent** of the template gold value and pays **100 percent**.

### 8.2 Pickpocketing (S:3dd70 `PickPocket`) [V]
`L = min(target.LevelMax, 50)`.
- `L >= 45` and no `Perk_PickPocket_3`: refused ("Impossible"); `L >= 30` and no `Perk_PickPocket_2`: refused ("TooHard").
- Chance: succeed if `rand(100) <= Theft/2 - L + 85` (Theft = player THF attribute, integer halves). Failure: the target gets AttackReason Theft (4) via `AssessTarget` and the conversation is ended.
- Success: the target's loot (five treasure sets via `GeneratePickpocketInventory`, S:3e1d0) is opened, `PickedPocket := true`; if the target belongs to an enclave and (not ExtendedContent or no `Perk_MasterThief`): enclave crime count Theft (3) += 1. With ExtendedContent the player gains `L/7` XP.

### 8.3 Armor reputation requirement (S:6b020, registry name `OnWantsToSell`) [V thresholds, I purpose]
When the player tries to take/equip a *generated* body-armor item (use type Body 114) belonging to a faction NPC, the gold value of the item decides the political fame (0..100, `PlayerMemory.GetPoliticalFame(NPC political alignment)`) needed; otherwise message `GO_RequiredPoliticalReputation` / `NoArmor`.
- `ExtendedContent == 0`: value <= 10000: 0; <= 65000: 25; above: 50.
- `ExtendedContent == 1` **(shipped)**: <= 15000: 0; <= 22000: 20; <= 39000: 25; <= 78000: 40; <= 120000: 50; above: 60.

### 8.4 Trade/steal flags
`OnPlayerCanTransfer` (S:6b3f0) refuses transfers from an inventory owner with status bit 0x80 [I]. Bribe, excuse and crime-conversation formulas are in 10.5.

---------------------------------------------------------------------------

## 9. NPC combat AI, move costs, ranged and magic (S: addresses; distances cm, times s)

### 9.1 Decision loop [V]
- `ZS_Attack` (S:14480) sets AIMode Combat (3); a player target sets `LastFightAgainstPlayer = Running(4)`. `ZS_Attack_Loop` (S:14d70) every pass: `_AI_SelectWeapon`, then `SelectCombatMove` (S:4ec30): AIMode Down/Dead -> 0; status 0x40 -> `FAI_Misted`; weapon mode magic -> `FAI_Magic`, ranged -> `FAI_Ranged`, else by `CombatState`: 0 `FAI_Passive`, 1 `FAI_Active`.
- **The only HP-based behaviour:** a non-Mage/non-Paladin NPC with HP < 40 percent, an `It_Potion_Health` in the inventory, a melee weapon (or no dangerous weapon) and the target farther than 700 drinks the potion (`Heal`, action 49). **`WantToFlee`, `WantToEat`, `CanSwim` are stubs returning 0 (S:bd420)**; fleeing is attitude driven: `AssessTarget` (S:31470) sends the NPC to `ZS_Flee` when its attitude to the target is Panic (6) or `CanAttack` is false (ambient animals, Deer, Pig, Cow), else `ZS_Attack`. `GetAttitude` yields Panic only if the NPC is already in AI mode Flee against its current target; the other Flee entries are `AssessActivePerception`, `AssessDefeat`, `AssessFightSound`, `AssessMurder`, `MagicFear`, `ContinueRoutine`.
- Action -> state dispatch: Attack(1), PowerAttack(2), QuickAttack(3), SimpleWhirl(6), TurnLeft/Right(7,8), SprintAttack(9; powers if within `GetAttackRange(...,2)` else `ZS_Attack_Sprint`), WhirlAttack(10), PierceAttack(11), HackAttack(14), Parade(16), Shoot(41), Cast(44), Heal(49), Wait(50), JumpBack(51), Fwd(52 `ZS_Attack_Goto`), Back(53 `ZS_Attack_GoBack`), Left/Right(54,55 strafe), Move(56 sprint). Finishing is chosen in `ZS_Attack_End`.
- Give-up rule: chasing longer than `GetFollowTime` (S:10240: Wolf 15 s (20 s unless the player sprints [I]); with AIMode: Human/Orc 5 s, others 9 s) leads to `ZS_Attack_WaitForFlee`; after 0.5 s there, if the target is farther than 2000 the attack is cancelled (`LastFightAgainstPlayer = Cancel(3)`, back to routine). Goto uses walk mode from `GetCombatWalkMode` (S:1b060: Prey reason 3; Zombie 1; Scavenger, Boar, Sabertooth, Rhino, Snapper 3; others 2) and stop distance `GetCloseRange` (Troll/Trex/Dragon 600, else 500).
- `ZS_Attack_End` (S:1a610): a defeated target with `HasAttackReasonToKill`, `CanBeKilled` and Finishing available gets a `FinishingAttack`; party members call `Kill`; the NPC may ransack the body (`ZS_RansackBody`).

### 9.2 Ranges [V]
`GetAttackRange(self, target, ..., move)` (S:4e940) = species base + `GetCombatMoveLength(move)` (120 if the length is -1). Bases (AIMode on): Golem types and Bloodfly 110; Goblin 140; Troll/Trex/Dragon 300; Minecrawler/Wolf/Ripper 170; Scavenger 115; Boar 180; Sabertooth/Shadowbeast/Varan/Snapper/Alligator/Gargoyle 200; Bison/Rhino 150; Ogre 220; ScorpionKing 120; Lurker/SwampLurker 200 (Quick) else 150; all others 200, or 50 in fist mode. `GetCombatMoveLength` (S:aa270): Zombie Attack 150; Attack/SimpleWhirl/Pierce/Hack/GetUpAttack 120; Power/Sprint/Whirl 180; Quick 60; stumble family 100; else -1.

### 9.3 Tactic selection (humanoid species: Human, Skeleton, Zombie, Demon, Orc, Minecrawler, Ogre, ScorpionKing, Stalker) [V structure, I scoring]
Each candidate move gets a score from a context snapshot (target attacking and in range, free line of sight, target is player, inside attack range -25 / range, target facing, target is Mage, engaged/approach mode); constants 85, 75, 70, 65, 55, 45, 40 and an aggression weight `W` (start 5; difficulty 0/1/2: +20/+10/+0 with minimum 20, Arena -10, Mage target -5, party condition -10, floor 0); -35 for repeating the current move or attacking while `CanHit` (min hit duration, `MinHitDuration` 6) is false. Moves scoring within 35 of the best form the shortlist; weaker ones go to a secondary list used when `Random(55) < W`. **The NPC picks from the best shortlist with probability `p = GetLevelMax(self)` percent** (Zombie forced 85; +10/-10 on difficulty 2/0; clamped 0..100), otherwise from the secondary list - so level is directly the chance of an optimal move. Candidate sets: full (Fwd, TurnL/R, Pierce, Power, Whirl, Hack, SimpleWhirl, Attack, Quick, Parade, JumpBack, Left, Right, Back), defensive (Parade, JumpBack, Left, Right, Back), offensive (Pierce, Power, Whirl, Hack, SimpleWhirl, Attack, Quick). Move availability (`CanDo`, S:4f80): Lurker/SwampLurker cannot plain-Attack; Quick needs a non-fist humanoid or a listed monster; SimpleWhirl needs a one-hander in each hand; Whirl needs 2H/staff; Pierce a 1H; Hack a 2H/staff/axe/halberd; Parade not when the target is already parading; evasion moves exclude Troll/Golem/Goblin types. Passive (S:4ffb0): back off when D < 350, approach when D > 450, else turn. Animals mostly chain SprintAttack -> PowerAttack and approach (their per-species melee branch sits behind constant-zero debug flags) [V, gameplay equivalence U].
Ranged (S:50370): D > 2000 approach, no LOS look for a new target, else Shoot. Magic (S:50550): focus spells retreat/cast at D < 350 / 350-1175 / > 1175; other spells cast with LOS (80 percent, always for "Xardas"). Misted (S:50a20): cast/shoot within 700.

### 9.4 Unconscious, death states [V]
`ZS_Unconscious_Loop` (S:26f70) per second: +10 HP and +10 stamina (+15 HP for an arena defeat); wake when the task time exceeds **5 s** (player or player-party), **20 s** (arena defeat) or **30 s**; then `ZS_Unconscious_End` (AIMode Reaction, picks "OhMyHead"/"NextTimeYoureInForIt" at 50 percent, targets its attacker, `ZS_FetchWeapon`). `Defeat` sets AIMode Down, memorises the Attack crime when the **player** defeated a non-guard, non-boss outside duel/arena, gives XP once. `ZS_Dead` (S:1b770) runs `Kill`, sets HP 0.

### 9.5 Attack stamina costs and timing [V]
`AddStaminaPoints` adds a delta for everyone; it is a no-op for the player in god mode or while `GuardStatus == Active (0)` (the player's free-action timer: `MagicKeepStamina`, S:58b70 = `(StaminaMax-200)/2 + 60` s, min 60 s).

| Move (id) | Stamina (ExtendedContent shipped / otherwise) | Hit window opens after |
| --- | --- | --- |
| Attack(1), Quick(3-5), SimpleWhirl(6), Pierce(11) | -12 / -10 (Pierce skipped when Action is Parade) | 0.25 s (Attack, SimpleWhirl), 0.10 s (Quick), 0.8 s (Pierce) |
| Power(2) | -20 | 0.10 s |
| Hack(14) | -20 at start, -20 in the second stage = -40 | wind-up in Parade pose |
| Whirl(10) | -40 (after a Parade wind-up) | 0.25 s |
| Finishing(15), Parade(16), stumbles | 0 | Parade 0.5/0.3 |
| Jump(57) -10; JumpBack(51) -10 non-ext, free shipped; Evade free | | |

GetUpAttack window 0.10 s, GetUpParade 0.25 s. Player `PS_Melee_*` states call the same `_AI_*` commands, so the **player pays the same costs** and there is no extra player-side charge. Animation speed multiplier (`sub_42a0`, S:42a0; higher = faster): weapon pose base 0.6 (1H alone, 1H+shield, 1H+torch, dual 1H) or 0.7 (2H, axe, staff, halberd) or 1.0 (fists); PowerAttack/Hack 1.5 (Orc 1.0), SimpleWhirl 1.3, Parade 1.3 (player 2.3), ParadeStumble 2.0, QuickStumble 2.0, Stumble 1.3; NPC diseased x0.75; Arena fight on hard x1.25; the **player's speed is reduced when tired**: Attack/Parade -0.10 below 20 stamina (-0.05 below 50); Power/Hack -0.10 below 30 (-0.05 below 50); Quick -0.30 below 20 (-0.15 below 40); SimpleWhirl/Pierce -0.14 below 20 (-0.07 below 45); Whirl -0.14 below 40 (-0.07 below 70).

### 9.6 Ranged [V]
Bow draw (`PS_Ranged_PowerAim`, `OnPowerAim_Loop` S:84b90): **`DamageHitMultiplier = 0.2 + 0.8 * min(1, t / 1.95)`** (t = held time; full power after 1.95 s); crossbow and reloads use 1.0. `Shoot` (S:86450) spawns the ammo projectile, velocity `multiplier * 200` (player, path style Physics 2, aimed at the camera centre pick point; NPC path style Line 0 along its forward axis, touch behaviour MaterialDependant), consumes one arrow/bolt; NPC archers re-supply ammo automatically (`AssureItems`). Fire-quality ammo needs `multiplier >= 0.5` to ignite. Damage itself is section 2 (head shot x2, DEX/2 bonus).

### 9.7 Magic [V]
- Spell numbers (damage, mana per spell, radii, cast times) are **properties of spell/projectile templates** (`PSMagic`: Min/MaxManaCost, CastType, SpellType, SpellTarget, SpellDeity, Spawn, FuncOnCast/FuncOnTargetHit; `PSDamage`), i.e. data, except the area spells below.
- Casting (`GetCastAction` S:52a10, `_AI_StartCastPhase` S:39760): Immediate -> QuickCast; Power -> PowerCast only if mana >= MaxManaCost, else normal cast; Regular -> summon anim. Load phase spends `MinManaCost` (not for Alchemy-category items: scrolls/potions cost no mana); power charge spends the difference up to `MaxManaCost`; aborting refunds `ManaUsed`. `ManaUsed` is copied into the projectile and used by AssessHit (2.5). Mana changes are player-only (NPC mana is a fixed 1000, 2.13). Scrolls (non-permanent) lose one from the stack when cast; runes are permanent. Dispatch by `SpellType`: Projectile `MagicProjectile`, Target `MagicInstant`, Summon `MagicSummon`.
- `GetPlayerSpellDurationBonus` (S:580c0): +50 percent duration with the matching deity perk (Innos Firemage, Adanos Watermage, Beliar Darkmage). `GetTransformDuration` (S:582a0): `max(INT-60, 0)+60` s, halved for large forms, x2 with `Perk_Druid`, +0.5x with `Perk_Watermage`. `MagicSpeedUp` duration `(StaminaMax-200)/2+60` s. `MagicCallAnimals`: INT > 300 -> 3, > 200 -> 2, else 1 within 25000. `MagicStun`: NPCs within 1200 -> `ZS_Stunned` for 8 s. Heal spells restore to full.
- **Area spells** (`MagicAreaDamageTick` S:5a0a0, damage goes through `AssessHit`): Hailstorm (Lightning type) and Firerain (Fire): every 400 ms, hit chance `33 + min(n,70)` percent, one random target within 1000 takes `50 + rand(50)`, duration `8 + rand(4)` s (+50 percent with the school perk). Sweep spells hit each target once as the wave front passes, damage linear from the centre value to 1 at the edge: Heatwave (Fire) 400 over 1000, Frostwave (Ice) 200 over 1000, Meteor (Fire) 1000 over 500 after a 500 ms delay, Lightning 1000 over 500 after 500 ms, IceBomb (Missile type) 600 over 1000. The caster, the player's party and entities without AI are skipped. Light and Timebubble last 80-110 s.
- **Status effects** (`HasStatusEffects` bits): 1 burning, 2 frozen, 4 poisoned, 8 diseased, 0x40 misted/controlled, 0x80 ghost/soul-travel body, 0x200 soul-travel out of body (0x10, 0x100 unknown). `CanBurn`/`CanFreeze` (S:2d530/2d7b0) fail for Golem, Demon, Troll, FireGolem, IceGolem, Dragon and for the player with `Perk_ResistHeat`/`Perk_ResistCold`; burn: Fire damage by a Missile-path projectile, fire-quality ammo (`mult >= 0.5`), or a fire weapon (quality bit 8) when `HP - damage < 100`; freeze: Ice damage, quality bit 0x10, `HP - damage < 50`. Poison: weapon quality bit 4 on a non-evil victim (ranged always, blades only if damage > 0); disease: quality bit 2; player immunity via `Perk_ImmuneToPoison/Disease`. Durations and damage per second of these effects are inside Game.dll [U]. Frozen duration is `12 * max(5, dmg/20)` (2.10).

---------------------------------------------------------------------------

## 10. Perception, crime and attitude (S: addresses)

### 10.1 Perception model [V]
- There is no radius in `SendPerception*` (S:31ff0-33170): they iterate all NPCs, filter with `CanReceivePerceptionFrom` (S:31c80) and call the receiver's `Assess*`, each doing its own distance/FOV/LOS test. `CanReceive` is false for: the player, the sender, entities without an NPC property set, AI modes Dead/Down/Talk/Abandoned, status bits 0x80/0x40, receivers in `ZS_FetchWeapon` or `ZS_Unconscious`.
- `AssessActivePerception` (S:295f0) is a poll (gate guards every 0.1 s; also from follow/guide/ransack/wait/react-to-damage callbacks). A slot scan bins the nearest NPC by attitude toward the observer with maximum ranges: player 2000; NPC already in Combat/Arena 4000; Friendly/Neutral 700; Angry 1000; Hostile/Predator/Panic 2000; dead or down 1000. Invisible to the observer: invisibility cheat, status bits, target swimming/diving, vertical offset above 1000, sneaking player closer than 1000 [roles I]; otherwise FOV plus free LOS (LOS skipped outdoors in the same zone).
- Reaction order inside the poll: (1) Panic-attitude NPC in range -> `ZS_Flee`; (2) `IsUpset(player)` -> re-attack with the stored reason; (3) corpse in range and non-humanoid -> `ZS_EatBody`; (4) Predator-attitude -> `AssessTarget` reason Prey (14); (5) Hostile-attitude -> reason Enemy (12); (6) `AssessPlayerZone`; (7) non-humanoid facing an Angry NPC -> `ZS_Threaten`; (8) `AssessPlayerWeapon`; (9) gate-guard/talk/sneak/remark block (gate guard: SecondWarn given and the player 80 closer to the guard point -> attack (GateGuard 10); status reset to Active when the player is farther than 500, to Behind when inside 500 behind the guard; important infos start a talk within 500; comments within 700).

### 10.2 Handlers and triggers [V unless marked]
| Handler (S:) | Trigger | Result |
| --- | --- | --- |
| `AssessTarget` 31470 | funnel for all attacks | Panic or `!CanAttack` -> `ZS_Flee`; else sets AttackReason (a lower value never replaces a higher one in Combat), `MemorizePlayerCrime`, `ZS_Attack`. Reasons Theft(4), UseInteractObject(5) -> crime Theft(3); Murder(13) -> crime Murder(4); all others no crime. ReactToDamage(9) notifies enclave mode 3, Enemy(12) mode 2 |
| `AssessTakeItem` 30620 / `AssessUseInteractObject` 316f0 | witness within 1000, FOV/LOS, `IsBotheredByPlayerTheftOf` (30840: not plants, `_Teleport_`, Bolt, Arrow, It_Bacon, It_Gancka_Wood; item must not be marked dropped) | theft reaction; `CallHelp` (S:332e0) only for StopThief/Theft/UseInteractObject: friendly guards within 1000 of the thief get `AssessTarget(StopThief 3)`. It_Bradley_SlaveList: attacks at once |
| `AssessPlayerSneaking` 2f5b0 | player in Sneak/Parade AniState, witness not Friendly, within 700, FOV/LOS, not in combat/flee | `ZS_ReactToSneak` |
| `AssessPlayerWeapon` 2f780 | humanoid witness (not friend-type, not slave, not gate guard), within 700, FOV/LOS, not Friendly, dangerous weapon drawn | `ZS_ReactToWeapon` (`ZS_ObserveSuspect` for Neutral slave-aligned) |
| `AssessPlayerZone` 2fca0 | zone security level Public -> observe; Enclave/Political need political attitude 1 and vertical offset <= 330 | `ZS_ObserveSuspect` / `ZS_ReactToZone` |
| `AssessFightSound` 2a510 | within 2000 of attacker or victim (4000 while the NPC's enclave is in Attack status) | join/watch/flee by attitude |
| `AssessMurder` 2eee0 | either party within 2000 with free LOS | humanoid victim Friendly to the witness -> reason Murder(13); livestock victim -> Livestock(2), no crime; same-enclave peers -> StopFight(8) |
| `AssessDefeat` 29fb0 | within 2000 | defeat comment / `StartSayDefeatComment` [I] |
| `AssessMist` 5d040 | mist spell | Friendly -> `ZS_Misted`, others `AssessTarget(ReactToDamage)` |

### 10.3 Reaction timers [V]
- `ZS_ReactToWeapon`: AIMode Reaction; line, broadcast warn; at 4 s second warning; at 8 s attack with reason ReactToWeapon (6) if the dangerous weapon is still drawn, else "WiseMove" and observe; abort if the player is farther than 1000 or lowers the weapon. **Drawing a weapon is never a crime.**
- `ZS_ReactToZone`: "GetLost" at once, "ISaidGetOutOfHere" at 4 s (enclave zones), attack with reason Intruder (11) at 8 s; keeps the player within 500.
- `ZS_ReactToDamage`: "WhatAreYouDoing", after 10 s `ZS_WatchFight`; returns if the target is farther than 550. `ZS_Threaten`: attack below 700 or after 5 s, gives up beyond 1000.

### 10.4 Attitude [V]
`GetAttitude` (S:e3b0) tries in order (first non-zero wins): friend-type; Flee mode vs its current target -> Panic; `GetOutlawAttitude` (Out alignment vs humanoid non-Out/non-Pirate -> Hostile); `GetRevolutionAttitude` (enclave based, matrix U); `GetAttitudeToPlayer`; `GetPoliticalAttitude`; `GetSpeciesAttitude`. Controlled party members are Neutral to humanoids; Summoned inherit the attitude to the player.
- Political matrix (rows = observer alignment, columns Orc, Nrd, Reb, Mid, Ass, Nom, Out, Slave, Pirate; F=Friendly, N=Neutral, H=Hostile): Orc F H H N (Ass: chapter dependent F/H) H H N F; Nrd H F N N H N H N F; Reb H N F N H N H F F; Mid N N N F N N N N F; Ass (Orc: chapter dependent) H H N F H H N F; Nom H N N N H F H N F; Out H H H H H H F H N; Slave N in all columns; Pirate vs humanoid N. Pirate alignment resolves to the NPC's enclave alignment [V S:f620 jump tables].
- `GetAttitudeToPlayer` (S:ef00): friend-type or party leader is the player -> Friendly; foreign Summoned -> Hostile; Boss: fame >= 75 Friendly else Neutral; Guard: enclave `KnownPlayerCrime` Murder -> Hostile, Theft -> Angry, else fame >= 75 Friendly else Neutral; others: own `LastPlayerCrime` Murder -> Hostile, Theft -> Angry; `LastFightAgainstPlayer` Lost/Cancel -> Angry. `GetZoneAttitudeToPlayer` (S:f410): owner Reb with Reb fame < 75 -> Angry; Nrd with enclave fame < 75 -> Angry; Orc Neutral; else Friendly.
- Species relations (`GetSpeciesAttitude` S:fbc0, used when the political attitude is 0): humanoids are Hostile to evil beings and to Goblin, Troll, Minecrawler, Wolf, Boar, Sabertooth, Shadowbeast, Ripper, Lurker, Varan, Snapper, Alligator, Trex, FireVaran, SwampLurker, Ogre, Stalker, Dragon (the Sabertooth "Zuben" is Neutral); Goblin: Human/Orc Hostile, Troll/Ogre/Stalker Friendly, Lurker types Angry; Troll/Ogre/Stalker: Human/Orc Hostile, Goblin Friendly; Minecrawler/Boar Hostile to Human/Orc; Scavenger, Rhino, Varan, Alligator, FireVaran, Bloodfly Angry toward Human/Orc; Wolf, Sabertooth, Shadowbeast, Ripper, Snapper Angry to Human/Orc and Predator toward Scavenger/Boar; Lurker types Hostile to Human, Predator toward Goblin; Trex/Dragon Angry to Human/Orc, Predator toward Bison/Rhino; Deer Panic toward Human/Orc; other fauna Neutral; same species Friendly; evil monsters Friendly to other evil beings, Hostile to humanoids.
- Helpers: `IsGuard` (NPC type Enclave and alignment matches the enclave), `HasAttackReasonToKill` (Enemy, Murder, Revolution, Kill; GateGuard/Intruder when the zone is not friendly), `HasAttackReasonToWatch` (Duel, Arena, Kill), `IsTemporaryFriendlyToPlayer`, `IsUpset` (Kill always; Enemy/Murder after 1.2 time units, low reasons within 0.24; unit of the game clock unresolved).

### 10.5 Crime bookkeeping [V]
- `gEPlayerCrime`: None 0, Livestock 1, Attack 2, Theft 3, Murder 4. `MemorizePlayerCrime` (S:37080) ignores friend-type NPCs and non-player targets; it tries `MemorizeEnclaveCrime`, else the NPC keeps `LastPlayerCrime = max(old, crime)`.
- **Attack** crime is recorded only by `Defeat` (player knocked out a non-guard, non-boss outside duel/arena) and only on the victim; witnesses react with StopFight.
- `MemorizeEnclaveCrime` (S:60670): only Theft (3) or Murder (4) reach the enclave; theft only through guards and bosses; the reporter needs political attitude 1 toward the player; sets the enclave's `KnownPlayerCrime` and `SuspectComment` and increments the crime count; hard-coded exclusions: Orc_Scout_05/06, Orc_Warrior_12, the enclave "Faring", Montera_Orc_02..05.
- `AddEnclaveTheftCount` (S:60930): adds to the owning enclave's Theft count (skipped with `Perk_MasterThief` under ExtendedContent). `NotifyEnclave` (S:5ef70): mode 1 sets Murder; modes 2/3 raise Attack/Flee status by a friend-vs-foe count and send up to 10 members.
- **Pardon** (`ChangeEnclaveStatus` to Routine, S:602d0): `PlayerFame = 100`, `KnownPlayerCrime = None`, clears suspect comment, raid and revolution, resets all members' `LastPlayerAR`, AttackReason, `LastFightAgainstPlayer`, `LastPlayerCrime`, `LastFightComment`, `LastFightTimestamp`. Fleeing status in the enclave "Ardea" bumps the player's political crime count.
- Guard news levels (S:b8b0): level 1 at Theft 20 / Murder 1, level 2 at Theft 40 / Murder 2, level 3 at Theft 60 / Murder 3 or crime already known; each level is spoken once. `HasCrimeRefuse` (S:bd1e0): personal NPC with `LastPlayerCrime != None` refuses (TheftRefuse/DontStealAnything for Theft, AngryRefuse/LeaveMeAlone otherwise).
- `_AI_Excuse` (S:3d130): needs `Perk_Excuse`; crime <= 2 always works; crime 3/4: `Random(130) <= 117` and `<= Theft` skill, so about `min(Theft,117)/130`; failure -> guard attacks. **Bribe price and any gold fine are not in the script DLL** [U] (info data or Game.dll).

---------------------------------------------------------------------------

## 11. Daily routines, party and player states (S: addresses)

### 11.1 Routine data and entry [V]
Each NPC's `gCNavigation_PS` holds a routine name plus `RoutineNames`, and anchor ids for WorkingPoint, RelaxingPoint, SleepingPoint (and arrays). An anchor (`gCAnchor_PS`: UserCount, MaxUsers, PatrolIndex, AnchorType Local 0 / Roam 1 / Patrol 2 / Event 3) owns interact points, each with a `gEUseType`. `SetRoutine` (G:25b0) only writes the name; **how a routine name resolves to the three point sets is not in script** [U]. Entry points: `Rtn_Monster` (S:86a10) -> `ContinueRoutine` (S:a5ea0); `Rtn_Dead` -> AIMode Dead, `ZS_Dead`; `Rtn_Player` (S:86ba0) -> `PS_Ghost` if status 0x80 else by weapon mode `PS_Normal/Melee/Ranged/Magic`. `OnProcessRoutine` (S:afa70) skips NPCs whose enclave is in Attack/Flee and NPCs that are Dead/Down, re-picks a destination only if the last "on destination" stamp is older than 2160 s (120 s if a debug byte is 1; default 0), runs `SelectRoutineDestination` (off-screen NPCs travel via the engine `StartGoto`) and `SelectRoutineTask`. `ContinueRoutine` order: Dead -> `ZS_Dead`; Down -> `ZS_StandUp`; enclave Attack with political attitude 1/4 -> `ZS_RevolutionAttack`; enclave Flee with attitude 1 -> `ZS_Flee`; **HP <= 60 percent and `CanHealSelf` -> `ZS_HealSelf`**; else destination and task.

### 11.2 Time of day: `GetRoutineTime` (S:a99d0) [V]
Result 0 = sleep, 1 = work, 2 = relax (hour of the day 0..24): **Human and Orc**: before 06:00 sleep, 06:00-20:00 work, after 20:00 relax. Scavenger, Boar, Bison, Rhino, Pig, Cow: sleep before 05:00 and from 23:00, work between. Wolf, Sabertooth, Ripper: sleep 04:00-06:00 else work. Shadowbeast: sleep 06:00-20:00 else work. All other species always work. The result selects which anchor is the destination (0 SleepingPoint, 1 WorkingPoint, 2 RelaxingPoint). `ZS_Sleep_Loop` and `ZS_Ambient_Sleep_Loop` poll every 60 s of state time and wake when the routine time is non-zero. The normal game clock runs 12 game seconds per real second (sleep skip uses a factor 3840 with `timer.fTimeScale=0.3`).

### 11.3 Destination and task selection [V]
`SelectRoutineDestination` (S:a7050): running arena returns early; party members of the player get no routine destination; per anchor type: Roam -> a random free roaming point of the anchor name, Patrol -> next patrol point, Local -> first free interact point (`IsFreeDestination` S:a9d00: `MaxUsers - UserCount > 0`, slots needed = party size). Failure logs "Can't find free destination for NPC". `SelectRoutineTask` (S:a9440) priority: running arena `ZS_Arena`; player's party leader is the NPC `ZS_GuidePlayer`; NPC led by the player `ZS_WaitForPlayer` (Waiting) else `ZS_FollowPlayer`; Controlled follower `ZS_FollowNPC`; otherwise the destination's UseType:

| UseType | Human (and Orc where marked *) task |
| --- | --- |
| Anvil, Forge | `ZS_Smith`* |
| GrindStone | `ZS_Grind`* |
| Cauldron / Barbecue / Alchemy | `ZS_Cook`* / `ZS_Barbecue`* / `ZS_Alchemy`* |
| Bookshelf, Bookstand | `ZS_Study`* |
| TakeStone, DropStone | `ZS_CarryStones` |
| PickOre, PickGround, DigGround | `ZS_Digger` |
| Field | `ZS_Farmer` |
| Repair, SawLog | `ZS_Woodwork`; Lumberjack `ZS_Lumberjack` |
| Bed, SleepGround | `ZS_Sleep`* |
| CleanFloor, Dance, FanBoss | `ZS_SlaveBabe` |
| Boss, Throne, Pace | `ZS_Boss`* |
| Bard | `ZS_Bard` |
| Preach / Guard / Trader / Shrine | `ZS_Preach`* / `ZS_Guard`* / `ZS_Trader`* / `ZS_Pray`* |
| AttackPoint | `ZS_RunToAttackPoint` |
| OrcDance, OrcDrum, OrcBoulder (orcs) | `ZS_OrcParty` |
| anything else or none | `ZS_HangAround` |

Orcs use `ZS_HangAround` for the unmarked rows; other species: SleepGround -> `ZS_AnimalSleep`, EatGround/Roam -> `ZS_Monster`, else `ZS_Ambient` (`ZS_Herd`, `ZS_Goblin` are never assigned by script). `ZS_HangAround` picks Stool, Bench, Waterpipe, SitCampfire (50 percent fry meat), SitGround, Smalltalk or Stand by the point's UseType; `HaveABreak` (S:8c390) inserts `ZS_Break` variants. Ambient wandering: target = random navigation point near the start at radius `rand(200)+200` (ambient creatures) or `rand(200)+300`; Pig and Cow do not wander; `GetAmbientAction` (S:884c0) chooses EatGround with 80 percent (Scavenger, Bison, Rhino, Pig) or 50 percent (Boar, Deer, Cow). Sleepers wake into `ZS_ReactToFootsteps` when the player comes within 500 (pre-check 1000); `Perk_Sneak_2` suppresses it for animals. `GetTravellingWalkMode` (S:a9c00) is 2 (fast) for Minecrawler, Scavenger, Wolf, Bison, Rhino, Snapper else 1.

### 11.4 Party and follow [V]
- `CanJoinPlayerParty` (S:a80a0): Summoned always; otherwise a waiting or out-of-range companion is dismissed and an active one blocks, so **at most one active companion**; level/guild/attitude gating is in dialog flags (`PartyEnabled`, `MobEnabled`, `SlaveryEnabled`). `Join*` sets leader = player, Waiting false, member type 1/2/3 (Party/Mob/Slave), clears the destination and starts `ZS_FollowPlayer`; `Dismiss*` clears the leader and returns to the routine; `WaitPlayerParty` sets Waiting.
- `ZS_FollowPlayer` (S:12990): checks every 1.0 s; follow distance = `(rank among non-waiting members + 3) * 150` (450, 600, 750, ...; 500 if unranked); issues a goto beyond it; switches to `ZS_WaitForPlayer` when the player swims/dives. A Mob member farther than 62000 from the enclave "Fireclan" is dismissed. `ZS_GuidePlayer` waits when the player is more than 1000 away and resumes within 800. `OnTick` removes Summoned members that are Waiting, dead or farther than 4000 from the player. Companions take part in combat as friendly entities and receive no XP of their own (XP goes to the player, section 3).

### 11.5 Player states [V unless marked]
- States: `PS_Normal`, `PS_Melee*`, `PS_Ranged*`, `PS_Magic*`, `PS_MagicPotion`, `PS_QuickUse`, `PS_GonnaLoot`, `PS_HUD*`, `PS_StopForMenu`, `PS_DuskToDawn`, `PS_Ghost`, `PS_GhostSummoner`, `PS_Interact_*` (chest, door, anvil, forge, grindstone, cauldron, barbecue, alchemy, shrine, ore, bookstand, stoneplate, saw, orc drum, water barrel, pirate treasure, seats, waterpipe, campfire, beds, orc boulder), books `BS_*`; movement-driven `PS_Swim`, `ZS_DiveIn`, `ZS_Fall`, `ZS_Slide`, `PS_Slide_Jump`. Weapon mode 0 none, 1 melee, 2 ranged, 3 magic, remembered in `LastWeaponConfig`.
- **Sneaking:** AniState Sneak (3), or Parade (4) with a melee weapon, movement mode 3; NPCs notice it only through `AssessPlayerSneaking` (10.2: within 700 plus FOV/LOS; a sneaking player closer than 1000 can additionally be hidden from the active-perception scan). Sneak speed (`SneakModifier`) and stamina cost live in the engine [U].
- **Swimming/diving:** `PS_Swim` and `ZS_DiveIn` spawn the splash effect, extinguish burning, revert transformations, holster weapons, drop a carried torch, set AniState Swim (28). Hits on a swimmer or diver are ignored (1.1). **There is no drowning or oxygen logic in script** [U for the engine]; a non-player entering `ZS_DiveIn` dies as a ragdoll.
- **Falling:** 5.1. **Sliding:** `ZS_Slide` sets Action Slide; `PS_Slide_Jump` after 0.6 s.
- **Sleeping:** `PS_Interact_SleepBed` -> `PlayerSleep` (S:76440) advances the clock to 08:00 (or to the next 08:00 when already past) and fully restores HP, stamina and mana. `PS_DuskToDawn` (S:66fc0): clock factor 3840 with `fTimeScale=0.3` until 08:00, then restores factor 12, full heal, `ContinueRoutine`.
- **Interaction requirements:** all interaction is blocked in combat ("HUD_NotDuringBattle"); Anvil needs `It_Hammer_Smith`, Forge/GrindStone need `Perk_SharpenBlade` and a blade, Cauldron needs `It_Scoop` plus cooking recipes, Alchemy needs recipes, locked chests refuse.

---------------------------------------------------------------------------

## 99. Remaining unknowns
- Melee `DamageAmount`/`DamageHitMultiplier` origin (engine), `CanBeKilled` internals, `MinHitDuration` consumer and tick interval; mana regeneration (S:b0790).
- `CurrentLevel` second term (2.13); NPC `Level`-to-stat tables beyond the formulas above are fully script-derived, but item/NPC base data (weapon damage, spell damage and mana, protection of armor items) lives in Templates (see PROPERTY_STREAMS.md).
- Exact pairing of the attitude arguments in the DoLogicalDamage friend gate; full `GetRevolutionAttitude`; `NotifyEnclave` counting; bribe price and fines; unit of the game clock used by IsUpset.
- Routine name -> point set resolution (Game.dll); per-state timers inside most `ZS_*` coroutines; sneak speed, stamina clamping, oxygen/drowning (engine); burn/poison/freeze/disease damage rates; the `sAICombatMoveInstr` phase strings and how hit-window times scale with move speed; full `ScoreMove` branches; per-species animal melee choices behind zero debug flags.
