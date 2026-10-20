# XACT / XMOT actor and motion formats

2026-10-20. Readers: `crates/formats/src/emfx.rs` (shared container walker), `xact.rs`, `xmot.rs`. Viewer: `apps/game/src/actor_viewer.rs`. Survey: `gothic-inspector actor-survey|motion-survey DATA_DIR [OUT.json]`.

Everything here was derived from byte observations of the installed 1.75 `_compiledAnimation.pak` (387 `.xact`, 5937 `.xmot`, nothing else in the archive). Public leads (Baltram/rmtools mimicry readers, g3dit) were consulted only to learn what to look for; both are GPL and no code was copied. Engine.dll was not decompiled. "Verified" below means: checked by byte accounting over all installed files and/or by the rendered result. "Inferred" means a plausible meaning that the data does not prove.

Naming hint: the two container magics `FXA ` and `LMA ` match the legacy EMotion FX actor / motion file families (middleware used for Gothic 3 animation). That is an inference from the fourcc and the `id, size, version` chunk framing, not something read from the engine.

## Coverage (all installed files)

| Set | Parsed | Notes |
| --- | ---: | --- |
| `.xact` | 387 / 387 | 371 skinned meshes, 0 rigid, 899,929 vertices, 1,316,369 triangles, max 20 raw influences per vertex (reduced to 4 for rendering) |
| `.xmot` | 5937 / 5937 | 5,015,089 keys over 374,573 node records, 153 pose-only motions, longest 21 s, 5645 motions use a 0.04 s (25 Hz) key interval |

Every `.xact` ends with exactly 4 unconsumed bytes inside its declared FXA region; every `.xmot` is consumed to the byte. Coverage means structural acceptance with all counts, indices, bone references and totals cross-checked; it does not mean every field has a known meaning (see "Unknown").

Strictness exceptions that were needed to reach 100 percent, all found in real files and covered by tests or the survey: negative skin weights (legitimate, e.g. 1.029 / -0.129), non-finite UV floats (sanitised to 0), non-finite key values in 3 motions (`Fat_Stand_None_2H_*`, `Orc_Parade_*`: the key is dropped), a stale declared size in the exporter info chunk (2 Diego head actors, see below), and a second UV set (8 actors).

## Envelope

Both types are `GENOMFLE` v1 files (`genome.rs`), but unlike meshes/materials the body is **not** a property tree with string-table indices. Strings inside the container are inline `u32 length + bytes` (Latin-1, no terminator). The Genome string table only carries `.xshmat` names in `.xact` (one per real material) and is empty in `.xmot`. All numbers are little endian.

Body prefix before the container is variable (`.xmot` LMA magic at body offset 36 to 64, actors about 60 to 190 bytes): `u16 0x0036 (xact) / 5 (xmot)`, a few opaque words, a Windows FILETIME (modification time), a size word, for actors an AABB initialised to +-FLT_MAX and a table of four 28-byte records (observed only in the skeleton actors, meaning unknown), and for actors the marker `"gena", u16 4, u32 size`. The readers do not interpret this prefix; they locate the container magic in the first 512 bytes.

Container framing (verified on all files):

```
actor : "FXA " u8 hi=1 u8 lo=1 , chunks...   region ends at (offset of "FXA ") + 4 + gena.size
motion: "LMA " u8 hi=1 u8 lo=1 u8 0 , chunks...   region ends at the Genome string table
chunk : u32 id, u32 size, u32 version, data[size]
```

## XACT chunks

| id | version | count/file | Meaning | Status |
| ---: | ---: | --- | --- | --- |
| 16 | 1 | 1 | exporter info: `u32, u32`, then 3 strings (tool, source `.max` path, build date) | structure verified; values unused |
| 0 | 3 | 1..many | skeleton node | verified, below |
| 6 | 5 | per material | material | verified, below |
| 7 | 4 | per texture layer | material texture layer | verified, below |
| 3 | 3 | per mesh | mesh (all submeshes) | verified, below |
| 4 | 1 | per skinned mesh | skin influences | verified, below |
| 8 (88 B) / 9 (72 B) | 1 | about 60 files | per-node records, likely limits/constraints | **not decoded** |

Info chunk caveat: two Diego head actors declare size 128 for 152 real bytes. The walker derives the info chunk length from its string structure when the declared size does not lead to a plausible next chunk header.

### Node (chunk 0, v3), verified by 387 files and by rendering

`vec3 position, quat rotation (x,y,z,w), quat scaleRotation, vec3 scale, 3 floats (always 0), string name, string parentName`. An empty parent name marks a root. Parents are resolved by name (first occurrence); the readers reject unknown parents and cycles. Node order in the file defines the **node index** used by skin chunks and by the skeleton arrays (the viewer relies on joint index == node index). `scaleRotation` is stored but treated as identity by the viewer; scale is 1,1,1 in the player skeletons, so this was not exercised.

Units are centimetres, Y up (the player pelvis `Hero_Spine_Spine_ROOT` sits at y = 103.36, the rendered body spans y = -2.9 to 198.8). The root `Hero_ROOT` carries rotation (0,1,0,0).

The skeleton contains **helper nodes** that are not animated: `*_ROOT`, `*_END`, `Slot_*` (weapon attachment), `Player`. Deforming nodes (`*_1`, `*_2`, ...) usually have zero local position and identity rotation because their `*_ROOT` helper already carries the offset and orientation.

### Material (chunk 6, v5) and layer (chunk 7, v4), verified

Material: `vec3 ambient, diffuse, specular, emissive; f32 shine; f32 (0); f32 opacity; f32 (1.5); u8 doubleSided, u8 wireframe, u8 transparencyType ('F' everywhere), u8 pad; string name; u32 (0)`. Material index 0 is always the exporter's `EMFX Default`; real materials start at 1.

Layer: `u8 mapType, u8 0xf7 (garbage), u16 materialNumber, f32 amount, vec2 uvOffset, vec2 uvTiling, f32 rotation, string textureName`. `materialNumber` indexes the material list. Map types seen: 1 diffuse, 2 and 9 specular, 3 normal/bump, 11 unknown (inferred only from texture names). The texture name has no extension.

**Texture resolution is inferred.** Material names in the actors are exporter leftovers (`lambert2SG`, `Hero_player`, `G3_Interacts_Animated_Barrelmetal_01_A|`). The Genome string table lists `<name>.xshmat`, but `lambert2SG.xshmat` also exists in `_compiledMaterial.pak` and belongs to a bloodfly, so using the listed name blindly yields the wrong texture. What works for the player body and head (confirmed visually): try `<layerTexture>_A.xshmat`, then `<layerTexture>.xshmat`, then the listed `.xshmat`; take the `ImageFilePath` stem from `xshmat.rs`, map it to `stem.ximg` in `_compiledImage.pak` by unique basename. Body: `Hero_player.xshmat` -> `G3_Human_Hero_Body_Diffuse_S1`. Head: layer name `G3_Human_HERO_Face_Diffuse_01` -> `G3_Human_HERO_Face_Diffuse_01_A.xshmat` -> `G3_Animation_Head_Hero_01_S1.ximg`. The layer texture name itself is not present as an image. How the engine really selects materials is unknown.

### Mesh (chunk 3, v3), verified by byte accounting on all 371 meshes and by rendering

```
u32 node            node index the mesh is attached to
u32 originalVertexCount   size of the skin table (pre-split vertices)
u32 vertexTotal, u32 indexTotal, u32 submeshCount
u32 uvSetCount      1 (about 379 actors) or 2 (8 actors)
u32 opaque tag      bytes 00 c9 + exporter garbage
per submesh:
  4 bytes: u8 materialIndex, u8 uvSetCount, 2 garbage bytes
  u32 indexCount, u32 vertexCount
  vertices: u32 originalVertex, vec3 position, vec3 normal, vec2 uv [, vec2 uv2]   36 or 44 bytes each
  indices : u32 each, local to the submesh, triangle list
```

The sum of submesh vertex and index counts equals the totals and the chunk ends exactly. `originalVertex` indexes the skin table. Vertices are in the model space of the bind pose (rendered with identity skinning, the mesh is exactly the authored T pose). The second UV set is inferred to follow the first in each vertex (consistent with chunk size only); its use (lightmap?) is unknown. Index winding follows the same convention as static XCMSH meshes: the viewer swaps indices 1 and 2 and renders double sided.

The mesh chunk is followed in the file, outside the FXA region, by a per-vertex stream (16 bytes per vertex in the player body, 4 x u32 values 0xa3 to 0xf5 observed). Not decoded; possibly vertex colours or tangent data.

### Skin (chunk 4, v1), verified

`u32 node` (must match a mesh), then one entry per original vertex until the chunk ends: `u8 influenceCount`, then per influence `u16 nodeIndex, u16 garbage (0x13d3), f32 weight`. The entry count must equal `originalVertexCount` and every node index must be inside the skeleton. Weights sum to 1; negative weights occur (Troll). Up to 20 influences per vertex occur; the importer keeps the strongest 4 and renormalises (Bevy supports 4). A mesh without a skin chunk would be bound rigidly to its node; no such mesh exists in the installed data.

## XMOT chunks

| id | version | Meaning |
| ---: | ---: | --- |
| 1 | 3 | node record: initial pose of one animated node |
| 2 | 1 | key track belonging to the preceding node record (0 to 3 per node) |

Node record (verified): `vec3 position, quat rotation (x,y,z,w), vec3 scale, 40 opaque bytes (uninitialised exporter memory: pointer-like words), string name`. 153 motions consist of node records only (static poses, e.g. `*_Stand_*_Move_Stand_*`, `*_LieDead_*`).

Track (verified): `u32 keyCount, u32 tag`, keys. The low 16 bits of `tag` are the type, the high 16 bits are garbage: `0x504c` "LP" linear position, `0x524c` "LR" linear rotation, `0x534c` "LS" linear scale (9 tracks in the whole archive). Key = `f32 time (seconds)` followed by `vec3` (LP, LS: 16 bytes) or `quat x,y,z,w` (LR: 20 bytes). Size must equal `8 + keyCount * stride`; times are ascending. Typical key spacing is 0.04 s (25 Hz); 62 motions use 1/30 s. There is no separate frame-rate field; duration is the last key time.

There is no explicit duration, loop flag, or event data in the parts decoded.

### Motion hierarchy (the important finding)

Motion node names match actor node names (player motions: 60 of the 63 motion nodes exist in `G3_Hero_Body_Player`; 1279 motions address at least 90 percent of the player skeleton). **The transforms in an `.xmot` are relative to the nearest ancestor that is also present in the motion, not to the actual parent in the actor.** Helper nodes (`*_ROOT`, `*_END`) are absent from motions. Evidence: the motion pose of `Hero_Head_Neck_1` is position (18.12, 0, 0) while the actor stores zero for it; `bindWorld(Spine_5)^-1 * bindWorld(Neck_1)` computed through the helper chain equals (18.1, 0, 0), and the same matches for shoulders, arms, hips, toes.

To pose an actor skeleton from a motion in a normal parent/child hierarchy, give each animated node the local matrix `inverse(bindWorld(animParent)^-1 * bindWorld(actualParent)) * motionLocal`; helper nodes keep their bind local transform. This is implemented as a constant per-node prefix matrix in the viewer. Without it the character lies on its side (first attempt) because the helper rotations (~90 degrees) are not undone.

The static pose in a node record equals the bind-relative transform for nodes without a position/rotation track, so it is used as fallback.

## Viewer (`--actor`)

`gothic-reforged --actor Hero [--motion idle|walk|run|<name>|none] [--yaw DEG] [--root-motion] [--time-a S --time-b S] [--smoke] [--screenshot PATH]` (build with `--features viewer`). `Hero` expands to `G3_Hero_Body_Player` plus `G3_Hero_Head_Player`; other values are comma-separated `.xact` names (substring match, shortest wins). Joint entities mirror the node hierarchy, `SkinnedMesh` uses the bind-world inverses, tracks are sampled with lerp/slerp each frame, root position is kept at its bind value unless `--root-motion`. Smoke mode screenshots two fixed animation times (`.local/actor-smoke.png`, `.local/actor-smoke-2.png`).

Verified results: bind pose renders exactly as authored (T pose, textured); idle (`Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_01...`) and walk (`..._Move_Walk_N_Fwd_00_..._160`) look correct, with different poses at different times. Screenshots: `.local/actor-bind.png`, `.local/actor-smoke.png`, `.local/actor-smoke-2.png`, `.local/actor-walk.png`, `.local/actor-walk-2.png`.

The Genome-flavoured quaternions need no handedness change in this viewer: skin matrices cancel the bind convention, and the animated poses look anatomically correct with the data used as-is. Chirality relative to the real game (left-handed) is unverified; the static mesh viewer uses the same unflipped convention.

## Animation system (`crates/animation`)

Plugin `AnimationPlugin`, cached `ActorLibrary`, `ActorSpawner` (SystemParam), `Animator` (0.2 s cross-fade stack, masked upper-body overlay, root motion), `Locomotion` state machine driven by `LocomotionInput {velocity, grounded, stance, sneak, turn_error}`. Design: motions are sampled on the CPU (uniform 25 Hz tracks indexed in O(1), otherwise binary search; no Bevy AnimationClip because of the animated-ancestor quirk), model matrices are computed once per character on a folded skeleton (animated bones relative to their nearest animated ancestor, helpers fixed to bind), body and head/armor parts share one set of joint entities bound by node name, joints carry only a `GlobalTransform`.

Findings: (1) animated skeleton faces -X in model space; the animator rotates it to Bevy forward (-Z). (2) Locomotion clips are in place; ground speed is the trailing number of the name (cm/s), `Turn90` clips imply +-90 degrees yaw; both are reported as implied root motion. (3) Name grammar `<Actor>_<Pose>_<LeftHand>_<RightHand>_<Pn>_<Action>_<Phase>_<N|I|O>_<Dir>_<Var>_%_<Var2>_<Pm>_<Speed>`; stances map to hands: Unarmed None/None, Fist None/Fist, OneHand None/1H, Shield+1H, TwoHand None/2H, Bow Bow/Arrow, CrossBow CrossBow/Bolt, Staff None/Staff, Magic None/Cast, Torch Torch/None; pose `Sneak` has walk/turn/idle only. (4) Equipment slots `Slot_RightHand_Weapon`, `Slot_LeftHand_Shield`, `Slot_MeleeBackRight`, `Slot_ShieldBack`, ... are helper nodes; static `.xcmsh` blade axis is +Z, units cm. (5) Head material file lists are not aligned with material order; generic heads use skin atlas `G3_Animation_Head_Main_01_S1..30` (heuristic: face/neck materials), hair is flat-coloured, eyelashes hidden. (6) NPC item templates (`Body_X`, `Head_Hero_NN_Sxx`) are mapped to actors by name heuristics only.

Viewer: `--actor Hero [--sword] [--stance 1h] [--motion walk] [--speed 150] [--overlay ArmsCrossed]`, `--actor Hero --crowd N [--bench]`. Release, RTX 3060 Ti, 100 NPCs: frame 3.6 ms mean, animation systems 0.88 ms CPU total (animate 0.50 parallel, joint write 0.34, locomotion 0.04).

## Unknown / not done

- Chunks 8 and 9, the 4 x 28-byte table in skeleton actor prefixes, the per-vertex stream after the FXA region, the opaque 40 bytes in motion node records, the second UV set semantics, material layer map type 11.
- Bind pose vs. `scaleRotation`: ignored (identity in all inspected nodes, not checked for all 387).
- Normal/specular maps, animated head morphs (`*_Animated_*` heads, `*_LipSync.xmot`), weapon slots, IK helpers, additive/blended animation, motion events, root motion semantics, animation naming grammar (`<Actor>_<Pose>_<Weapon>_<Weapon>_P0_<Action>_<Phase>_<N|I|O>_<Dir>_...`) beyond what the file names show.
- Actors other than the hero were only parsed, not rendered; non-human skeletons were not checked against their motions.
- Performance of the viewer (per-frame matrix recompute on the CPU for about 240 nodes) is fine for one character, untested for crowds.
- Rendering uses stored normals with double-sided StandardMaterial, no alpha handling for hair cards beyond the BlendMode hint copied from the static viewer.

## Tests

`xact`: synthetic actor (nodes, materials, layer, mesh, skin, string table), rigid binding, cycle/unknown parent/bad layer/NaN rejection, truncation at every length, weight reduction, stale info size recovery. `xmot`: synthetic motion, bad tags/sizes/descending times/unknown chunk, truncation. Fixtures are generated in code; no original data is committed.
