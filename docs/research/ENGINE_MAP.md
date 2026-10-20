# Engine map (Engine.dll, Game.dll, GUI.dll, GFC.dll, SharedBase.dll)

Written 2026-10-20 from a full Ghidra 12.1.4 headless decompilation of the installed 1.75.20108.14 build. This file contains only my own prose, addresses, offsets, constants and tables; no decompiled bodies and no original content. The decompiled bodies live in the gitignored `.local/research/engine/` (see "Using the dump"). Evidence tags: **[V]** read directly in code (address given), **[I]** inferred from names/structure, **[U]** unknown. Pseudo-C parameter and register names are unreliable (`in_EAX`, `unaff_ESI`), but called API names, string constants, member offsets (`this+0xNN`) and control flow are real. Addresses are virtual addresses (image base `0x10000000`, so `0x100c0a60` = RVA `0xc0a60`); they are the addresses in the on-disk DLLs, every DLL has the same preferred base so always name the DLL.

## Using the dump

| Path (under `.local/research/engine/`) | Content |
|---|---|
| `<Dll>/src/<Class>/<method>.c` | One file per function; header line has address, size, class, name; second line the Ghidra signature. Name collisions get `_<addr>` appended. Unnamed functions are `_unclassified/FUN_<addr>.c` or `<Class>/vfn_<slot>_<addr>.c` (a vtable slot of an RTTI class whose function had no demangled name). Template classes are directories like `bTPropertyType_gCNPC_PS_int_`. |
| `<Dll>/index.json` | dict keyed by address: `name, class, size, file, callees[], callers[], imports[], strings[]` (strings: up to 24 per function; imports: other DLL APIs incl. d3d9/D3DX9/FMOD/PhysX/Bink). |
| `<Dll>/functions.jsonl` | same records as JSON lines. |
| `<Dll>/classes.jsonl` | every RTTI `vftable` (class, address, slot function addresses) - use it to find virtual overrides and to resolve `(**(code**)(*(int*)this + 0xNN))` calls: slot index = NN / 4. |
| `out/g3.igl`, `out/igl_groups.json`, `parse_igl.py` | the image group list (item icon atlas mapping), see Q1. |
| `scripts/DumpEngine.java`, `run_import.ps1`, `run_dump.ps1`, `cls.py`, `idx.py` | tooling. `analyzeHeadless` projects are in `C:\g3ghidra\proj_<Dll>` (Ghidra refuses project paths with a leading `.`). |

Regenerate: `run_import.ps1 -Dll Engine -Mem 10g` then `run_dump.ps1 -Dll Engine` (default analyzers incl. RTTI, demangler; decompiler-parameter-ID not enabled). Full analysis of Engine.dll (6.9 MB) and Game.dll (4.6 MB) finished in a few minutes with 10 GB heap; the earlier REA provider timeout was only the provider's 330 s start-up limit. Dump throughput is about 100 functions/s with 5 decompiler threads.

Function counts (non-external functions found by Ghidra):

| DLL | functions | with demangled name | in an RTTI/namespace class | RTTI classes |
|---|---|---|---|---|
| Engine.dll | 22536 | 12210 | 14589 | 577 (+ templates) |
| Game.dll | 14292 | 5757 | 8838 | 383 |
| GUI.dll (FFGUI toolkit) | 2716 | 289 | 1586 | 36 |
| GFC.dll (MFC-like wrappers) | 643 | 560 | 401 | - |
| SharedBase.dll | 4462 | 3813 | 3443 | - |

Library use (call/jump sites to imports): Engine.dll links Direct3D 9 only through `Direct3DCreate9` (6 sites) and then talks to the device through COM vtables in `eCDX9Device` (233 methods) and the other `eCDX9*` wrappers; it uses D3DX9_40 (100 sites: `D3DXCreateEffect*`, `D3DXCompileShader`, texture/surface loading, mesh simplify/UV atlas), FMOD Ex (273 sites: System/Channel/ChannelGroup/Sound), DirectInput 8, Bink (16), PhysX only through `PhysXLoader!NxCreatePhysicsSDK`/`NxGetUtilLib` and `NxCooking` (cooking of convex/triangle meshes); the Novodex descriptor classes (`NxBoxShapeDesc` ...) are statically linked. Game.dll imports 6005 Engine.dll symbols and 1497 GFC.dll symbols and nothing graphic or audio directly.

Layer picture: SharedBase (`bC*`: strings, streams, property objects, delegates, memory, message/error admin) -> Engine (`eC*`: entities/property sets, resources+caches, gfx, audio, input, GUI admin, physics) -> Game (`gC*`: game property sets, session/world/project, HUD, NPC/inventory/quest logic) -> Script.dll/Script_Game.dll (see SCRIPT_ENGINE_INTERFACE.md).

---

## Answered questions

### Q1. Item icon atlas cell order (`gCInventoryStack::CreateIcons`)

Short answer: **the cell order is not computed at run time and not alphabetical; it is stored as data in `g3.igl` (an "image group list") inside `Data/Library.pak`.** Each item icon is looked up by file name in that list and the list carries the exact pixel rectangle of every icon on its atlas texture. So the `g3_hud_it_*` atlases need no heuristics: parse `g3.igl` once.

Call chain (all **[V]**):

1. `gCInventoryStack::GetIcon` (Game `0x100c0a40`) calls `CreateIcons` (Game `0x100c0a60`) and returns the "grey" bitmap object at `this+0x68` when `this[0x54] != 0` or `*(this+0x58) > 0`, otherwise the normal one at `this+0x60`. `CreateIcons` is lazy: if both bitmaps already have a handle it returns; else it calls `GetIconFileNames` and `CFFGFCBitmap::Create(name)` for both.
2. `gCInventoryStack::GetIconFileNames` (Game `0x100c0ae0`): takes the stack's template entity (`this+0xc`), its `gCItem_PS` (property-set id `0x30`), and reads the icon string at `gCItem_PS+0x2c` (the template property `Texture`/icon, a `.tga` file name such as `IT_1H_FlameSword_01.tga`) and the item category enum at `+0x28`. Empty string -> per-category default entry (table at Game `0x103f3c10`, one `bCString` per category). If category == 9 and the item has a linked skill-type template that carries property set `0x65` (`gCMagic_PS`) at `gCItem_PS+0x58`, or the template at `+0x64` carries `0x66` (`gCSkill_PS`), a second name is produced by splitting the path and inserting `_Grey` before the extension (`IT_Perk_Foo.tga` -> `IT_Perk_Foo_Grey.tga`); otherwise both names are identical. The grey variants exist as separate tiles in the list (121 tiles with `_Grey`, almost all in `it_perks`) and are used for not-yet-learned skills/recipes.
3. `CFFGFCBitmap::Create` (GFC `0x100020c0`) -> FFGUI module `CreateBitmap` -> the renderer-side `eCGUIAdmin::LoadBitmapA` (Engine `0x100def40`) -> `eCImageAdmin::FindImageIndex` (Engine `0x10109850`): walks all groups (stride `0x78` bytes per `eCImageGroup`), then a temporary-texture list, then tries to import a loose temp texture. `eCImageGroup::FindImageIndex` (Engine `0x1010b4a0`) compares **case-insensitively on the "cleaned" name**: `eCImageGroup::CleanImageName` (Engine `0x1010b6d0`) runs `_splitpath` and keeps only file name + extension, so directory part and drive are ignored. `eCImageAdmin::GetImageInfo` (Engine `0x10109a50`) returns the image rectangle (`bCRect`) and the composite-texture index.
4. The groups come from `eCImageAdmin::LoadGroupList` (Engine `0x10108d70`), called once in `gCGUIManager::Create` (Game `0x1007b220`) with the file name literal `g3.igl`. The file is in `Data/Library.pak` (member `g3.igl`, 101080 bytes). `eCImageGroup::Read` (Engine `0x1010d640`) reads version 2 records.

`g3.igl` layout (decoded and checked: the parser consumes exactly up to the string table; reference parser `.local/research/engine/parse_igl.py`, result `.local/research/engine/out/igl_groups.json`):

* Magic `GENOMFLE`, `u16` version `1`, `u32` offset of the string table (`0x5888` here). Header word at `0x0e` is the group count (13), then the group name string ids.
* String table (at the end): `deadbeef` marker, `u8 1`, `u32` count (1208), then `count` strings, each `u16 length` + bytes (Latin-1, no terminator).
* Group blocks start at `0x2c` and follow each other up to the string table. Per group: `u32 version=2`, four `u16` string ids (group name, compiled dir `Data\gui\icons\compiled\`, import dir e.g. `Data\gui\icons\item\weapons`, file prefix `g3_hud_it_weapons`), `u32 maxTexW`, `u32 maxTexH` (1024, 1024), 9 more `u32` (rect/margins/flags, purpose [U]: `-1,-1,1,1,0xba9624,0,0,1,1`), `u32 tileCount`, then `tileCount` records of 20 bytes: `u16 imageStringId` (`#G3:\Data\gui\icons\item\...\IT_xxx.tga`), `u16 textureStringId` (`Data\gui\icons\compiled\/g3_hud_it_weapons_00.dds`), `u32 left, top, right, bottom` (right/bottom exclusive).
* Groups present (tile counts): `it_alchemy` 88, `it_armor` 41, `it_artefacts` 132, `it_documents` 23, `it_misc` 146, `it_perks` 148, `it_recipe` 65, `it_spells` 94, `it_weapons` 164, `hud_icons` 68, `hud_backgrounds` 48 (textures `g3_hud_backgrounds_00..17`), `hud_maps` 5, `hud_slides` 70.
* Item icons are 64x64 tiles with a 1 px margin: the n-th tile of a 1024-wide atlas sits at `left = 1 + 66*(n mod 15)`, `top = 1 + 66*(n div 15)` (15 columns; checked on alchemy and weapons: `IT_1H_FlameSword_01` is record 11 of `it_weapons`, rect `(727,1)-(791,65)`). Record order is the build-tool order: sub-folder by sub-folder (e.g. `Weapons_1H`, `Weapons_2H`, ... `Weapons_Staffs`) with NTFS-style upper-case ordering inside a folder (`IT_Meatbugragout` before `IT_Meatbug_Fried`). This explains why the order is not plain alphabetical and why there are extra cells that no template references.

Rust port implication: replace the category-glyph fallback by `g3.igl` lookup: key = lower-cased `basename(item.icon)` -> (atlas dds file stem, rect). The same file also gives the exact rectangles of the HUD icon atlas (`hud_icons`, 68 named tiles), of the panel backgrounds (`hud_backgrounds`, 48) and of slideshow/map images, replacing the alpha-layout rectangle recovery in `crates/ui/src/atlas.rs`.

### Q2. NPC appearance chain (head / body / hair / beard / material switch)

Short answer: the engine has **no appearance logic of its own**. An NPC is an ordinary entity whose `gCInventory_PS` has 19 `gCInventorySlot` objects (`gESlot` 0..18, see below). Each slot may carry a *default item template*. When the entity's children become available the inventory spawns every default item as a child entity and "links" it to its slot; the link kind of the slot decides whether the item's visual becomes a **skinned body part** (Head, Body) or a **bone attachment** (Hair, Beard, Helmet, weapons). The per-variant look is just the integer property `MaterialSwitch` on the item's own visual property set, which is forwarded to the shader's switch sampler. Everything is data driven; nothing derives from NPC name, gender or species. (This confirms and explains the empirical chain in `ACTOR_ANIMATION_FORMAT.md` "NPC appearance chain".)

`gESlot` values **[V]** (enum table in the Game class dump + `gCInventorySlot::GetBone`, Game `0x100bd8d0`): 0 None, 1 RightHand, 2 LeftHand, 3 EquipLeftHand, 4 Beard, 5 BackLeft, 6 BackRight, 7 Bow, 8 Crossbow, 9 Armor, 10 Belt, 11 Amulet, 12 Ring1, 13 Ring2, 14 Ammo, 15 Hair, 16 Head, 17 Body, 18 Helmet.

Slot -> skeleton node and link kind (`GetBone`, **[V]**): Beard -> `Slot_Beard` (kind 1), Hair(15) -> `Slot_Hair` (kind 1), Head(16) -> `Slot_Head` (kind 2), Body(17) -> `Slot_Body` (kind 2), Helmet(18) -> `Slot_Helmet` (kind 1), RightHand -> `Slot_RightHand_Weapon`, LeftHand -> `Slot_LeftHand_Weapon` or `Slot_LeftHand_Shield` (use type 9), BackLeft -> `Slot_MeleeBackLeft` or `Slot_ShieldBack`, BackRight -> `Slot_MeleeBackRight`, `Slot_AxeBack` (use types 3 / 0x34) or `Slot_StaffBack` (0xc / 0x33), Bow -> `Slot_Bow`, `Slot_CrossBow` for use type 6. Kind 0 (no bone): None, EquipLeftHand, Armor, Belt, Amulet, Rings, Ammo. Kind 1 = attach the item entity to a bone; kind 2 = merge the item's actor as a body part.

Order of operations **[V]**:

1. `gCInventory_PS::OnChildrenAvailable` (Game `0x100b3580`): for non-template entities calls `UnlinkDefaultItems`, then (if the entity differs from its template) `PatchTreasureSets` and `PatchDefaultItems`, then `LinkDefaultItems` (Game `0x100b5530`).
2. `LinkDefaultItems` iterates `gESlot` 1..18; for each slot that has no stack linked and whose slot object has a default template that is not yet linked, it calls `gCInventorySlot::LinkToSlot` (Game `0x100bd410`). (Equipped stacks from savegames/scripts go through `LinkStackToSlot`/`EquipStackToSlot` to the same function.)
3. `LinkToSlot` spawns the item entity from the template as a child of the NPC (`SpawnItem`, Game `0x100be3f0`), or, if the slot already has one, `DetachItemFromSkeleton` first; then `AttachItemToSekelton` (Game `0x100bdb70`, sic) and stores the slot id into the item's `gCItem_PS` (`+0x144`). `ReBuildSlot` (Game `0x100bd6f0`) re-finds the item among the NPC's children by comparing that stored slot id after a load.
4. `AttachItemToSekelton`: requires that the NPC has an `eCVisualAnimation_PS` (property set id 100) and the item has `gCInteraction_PS` (id 0x31, `+0x58` = use type) and `gCItem_PS`; asks `GetBone(slot, useType)`; kind 1 -> `eCVisualAnimation_PS::AddAttachment` (Engine) with the bone name; kind 2 -> `eCVisualAnimation_PS::AddBodyPart(itemEntity, "Slot_Head"/"Slot_Body", ...)` (Engine `0x1002c3c0`). The item entity itself is set to collision group 5, picking off, rendering off, processing disabled (its geometry is drawn through the NPC), unless it is a light (`eCDynamicLight_PS` enabled flag mirrors rendering).
5. `eCVisualAnimation_PS::AddBodyPart` -> `eCVisualAnimationFactory::AddBodyPart` (Engine `0x1003a270`): removes any previous part with the same key, then creates one `eCVisualAnimationLoD` per LoD level from the item's actor file names (item `eCVisualAnimation_PS` at `+0x9c` = LoD0 actor, `+0xa0` = LoD1 actor) and copies the item's `MaterialSwitch` int (`eCVisualAnimation_PS+0x90`, written by `SetMaterialSwitch` Engine `0x1000a8c0`) into the LoD object at `+0x24`. If a base actor instance exists the new part's actor instance is created and linked to it (skeleton shared, `CreateInstance` of the wrapper). The key of the part is the bone-node string (`Slot_Head` / `Slot_Body`), so a second head replaces the first.
6. Rendering: `eCAnimation_HardRenderActor::Render` (Engine, `Render.c`) passes the LoD object's `+0x24` to `RenderPrimitive`, which sets it as the shader *switch* (`eCShaderBase::SetSwitch` Engine `0x1023c660` -> every shader element's virtual `SetSwitch`, in particular `eCColorSrcSampler::SetSwitch` Engine `0x1026a150`). A texture sampler of type 2 ("switch sampler") holds an array of textures (`+0xc4`, count `+0xc8`). The index is computed from the switch value by the sampler's `SwitchRepeat` property (`+0x60`, enum `eEColorSrcSwitchRepeat`): **0 Repeat = `switch mod count`; 1 Clamp = clamp to `[0,count-1]`; 2 PingPong = triangular wave with period `count-1`**. Then the texture at that index replaces the sampler's current texture. Materials whose samplers are not switch samplers ignore the value. Therefore the number `n` in the template name suffix `_S<n>` is the sampler index (0-based: `_S18` -> 18) and **resolution of "fewer variants than switch" depends on the sampler's `SwitchRepeat` mode stored in the `.xshmat` of that material** **[I]** (`ActorLibrary::switch_stem` should read that property instead of guessing `_S<n+1>` names; our existing 1-based file-name guess corresponds to list position n of an authored list, which is data in the material, not a naming rule).
7. Hair/beard items are kind-1 attachments: their own visual is a static/dynamic mesh (`eCVisualMeshStatic_PS` / `eCVisualMeshDynamic_PS`, base `eCVisualMeshBase_PS`, `MaterialSwitch` setter Engine `0x10044ed0`, property-change hook `OnNotifyPropertyValueChangedExitEx` Engine `0x1020e590`) attached with identity offset to `Slot_Hair`/`Slot_Beard` (`AddAttachment`); the same switch mechanism applies.
8. The head and body *do* come as separate actors (LoD0/LoD1 actor files) merged into the NPC skeleton (EMotionFX actor instance shares bones); facial animation uses the separate facial actor file name (`eCVisualAnimationFactory::GetFacialActorFileName` / `SetFacialActorFileName`, `eCVisualAnimation_PS::SetFacialAnimFilePath`, `PlayFacialMotion`), see ANIM_AUDIO.

Property-set type ids (engine class ids; useful when reading `GetPropertySet(N)` calls): see the table at the end of this file.

Rust port implications: implement the 19-slot `gESlot` list and the kind-1/kind-2 split; Head/Body = body parts keyed by `Slot_Head`/`Slot_Body` (replacement semantics), everything else = bone attachments; pass the item's integer `MaterialSwitch` to the part's switch samplers and apply the sampler's own repeat mode (Repeat/Clamp/PingPong) instead of a file-name rule; do not look at NPC gender/species.

---

# Gothic 3 Engine.dll - RENDERING reference

Scope: material/shader binding, lighting constants, fog + weather, sky, shadows, LOD selection, render queues/ops, post-processing.
Source: Ghidra headless pseudo-C under `.local/research/engine/Engine/` plus byte-level reads of the shipped `Engine.dll` data section (constants) and of `Ini/ge3.INI` (the game's own config).
Everything below is my own prose; no decompiled bodies are reproduced.

Evidence tags: **[V]** verified by reading the .c, the instruction bytes or the data section; **[I]** inferred from names/shape/call graph; **[U]** unknown.
Offsets written `+0xNN` are real struct offsets from the decompilation; the field *meaning* is [I] unless stated.
Addresses are Engine.dll VAs (image base 0x10000000).
Constants that the decompiler shows as `_DAT_xxxxxxxx` are often **8-byte doubles** (x87 `fld qword`); I checked the opcodes before quoting values.
World units: roughly 1 unit = 1 cm (the ge3.INI far plane 10000 is about 100 m).

Companion document already in the repo: `docs/research/ORIGINAL_SHADING.md` (HLSL-level lighting maths from Shader.Cache / Materials.pak). This file is the engine-side half: who computes the constants, in which order, with which numbers.

---------------------------------------------------------------------

## 0. Frame overview

One frame per camera ("frustum database", eCFrustumDatabase) is driven from eCRenderSystemAdmin. [V]

1. `eCRenderSystemAdmin::Render` 0x101ae430 (world) or `RenderEntity` 0x101ae710 (single entity, preview/UI path): bail out if the application is not active or the device needs a reset; refresh frustum data; run the PVS prefetcher (eCPVSPrefetcher3); extract visible items (`ExtractFromFrustum`, or `ExtractFromEntities` for the single-entity path); then call `eCFrustumDatabase::Execute` 0x101a0530 (7385 bytes, the real frame body). Timing counters are accumulated per phase.
2. Extraction yields one `eCFrustumElementItem` per visible entity. `Execute` asks every element to `Request` (0x101a35c0) its work: LOD update, solid ops, alpha ops, refracted ops, z-pass membership, illumination refresh (flag bits in 7.2).
3. `Execute` then binds the scene render target, clears (colour 0xff000000, depth 1.0; stencil too in the "clear all" mode), executes the global fog, renders solids, decals, occlusion/z passes, optional low-poly far camera, refraction capture, alpha, refracted ops, post-render module hooks (7.3).

Engine modules (SpeedTree, vegetation, skydome, particles, terrain) draw through the module admin hooks `DoBeginRenderSolid / DoEndRenderSolid / DoBeginRenderAlpha / DoEndRenderAlpha / DoPostRender` (global module admin at 0x10614b38) called from the same function. [V call sites; which module draws where is I]

---------------------------------------------------------------------

## 1. Materials and shader binding

### 1.1 Class map [V unless noted]

| Class | Role |
|---|---|
| eCMaterialAdmin (ctor 0x10005b70, PostInitialize 0x10132d70 8951 B) | Legacy effect pool (`Data/Materials/Effects/Shadowmap_V|P|P_a .fx/.bfx`), debug "indicator" materials (colour mapping, untextured, pixel/object overdraw, alpha-vs-solid, quality, LOD, performance, ambient occlusion), placeholder materials (`SetXmatMissingMaterial`, `SetXimgMissingMaterial`, `SetLoadingResMaterial`), and `ActivateDepthBufferFillMaterial` 0x1012ff80 (used when an object is drawn with partial fade, see 7.1). |
| eCShaderMaterialAdmin (ctor 0x10235ae0, Create 0x10239840, PostInitialize 0x10239340) | Modern path used by game content: owns global shader constants, shader pool/cache, current shader version, render-state save/restore, Begin/EndMaterial, Begin/EndZPass. |
| eCResourceShaderMaterial_PS (ConvertFrom 0x101df990, DoLoad 0x101e0720, SetShader 0x101e0660) | Resource wrapping a material (`.xshmat`, see docs/research/XSHMAT_FORMAT.md); holds the eCShaderBase at +0x38. ConvertFrom is the legacy-material importer. |
| eCShaderBase (ctor 0x1023bbc0, Create 0x1023c420, CompileVersion 0x1023cfd0, GetPasses 0x1023d510, ExecutePass 0x1023d760, SetShaderRenderStates 0x1023d0e0, Read 0x1023d9e0) | Abstract graph shader: blend mode (+0x48), mask-reference byte (+0x4c), use-depth-bias (+0x5c), max shader version (+0x54), per-version implementation slots (+0x80..+0x90), fallback material (+0xb8). |
| eCShaderDefault / Skin / Leaf / Water / SkyDome / Vegetation / Particle | The material families. Each declares extra constants and its HLSL macro set in `GetDeclarations`. Default and Skin share a property block (bRimLightSkin, colRimLight, fSubSurfaceRollOff). Water: vecWaterConstants, vecDepthColorHalfLife, colReflectionModulate, shore fade + specular macros, static/flowing bump. Vegetation: fAlphaDisolveEnd, fAlphaDisolveRcpStartDifference, vecAmplitude, vecPhase, vecPlayerPos, vecWaveLength. Particle: fSoftParticleScale, bBlendModeAdd, distortion macro. SkyDome: colSun, colSky, colHaze, colClouds, vecSunNormal, fApparentDepth. |
| eCShaderVersionDefault_3_0, Leaf_3_0, Particle_3_0, SkyDome_3_0, Vegetation_3_0, Water_3_0, eCShaderVersionIndicator_1_1, eCShaderVersionBase | Per-shader-model implementation objects. Only the debug Indicator exists for 1_1; every real material has only a 3_0 implementation. |
| eCColorSrc{Base,Constant,Sampler,CubeSampler,SkydomeSampler,Blend,Combiner,Mask,Proxy,VertexColor}, eCTexCoordSrc{Base,Color,BumpOffset,Oscillator,Rotator,Scale,Scroller,Proxy} | Nodes of the material graph. They are **HLSL text generators**, not runtime evaluators: `GetDeclarations` contributes macro declarations (`XFX_VS_INPUT_TEX%i`, `XFX_VS_TEX%i`, `XFX_TANGENTSPACE`, `XFX_TANGENTSPACE_EYE`, `XFX_PS_VERTEXCOLOR`), `GetImplementation`/`GetPSImplementation` returns an expression string (cube sampler yields a texCUBE call keyed by `XFX_PS_REFLECT`). Blend/Combiner combine two child expressions; Mask thresholds one (fMaskThreshold); BumpOffset is parallax (fParalaxAmount); Oscillator/Rotator/Scale/Scroller produce per-material UV transforms (matOscillate, matRotate, vecScaleAmount, vecScrollAmount) refreshed on time (`OnIsTimedChanged` 0x10237f40, `Process` 0x102388b0). |
| eCGfxMaterial (Execute 0x10094e20) | Old fixed-function material wrapper (specular reflection, texture blocks, unit numbers). Not on the shader path. [I] |

### 1.2 Shader versions and the Shader.Cache [V]

- Version enum names (string table at 0x105f9df4): `1_1`, `1_4`, `2_0`, `3_0` (indices 0..3). The admin keeps max (+0x84), min (+0x88), current (+0x8c). `SetCurrentShaderVersion` 0x10238090 clamps the request into [min,max] and logs "Can't set shader version ... forced to ... (min, max)" if it had to change it. Side effects: below index 2 (older than 2_0) it disables static shadows and deletes the terrain shadow map; at index >= 2 static and terrain shadows follow application settings (application+0x98 struct, +0x1a9 / +0x1a8); at index >= 3 detail shadows follow setting +0x112; an "instancing/hardware" flag at the engine settings is forced. Changing the version clears all shaders' constant tables and reloads the pool.
- **Shader.Cache**: stream magic `GENO` `MESC` (0x4f4e4547, 0x4353454d), 16-bit version **0x0175**, then pool header, an MD5 of a global key string (+2 bytes) used as a project fingerprint, then N entries (id, MD5, bytecode, element), then an id-remap table (preprocessor-id to shader-id, which lets differently spelled but identical macro sets share bytecode). The real file in the install root starts `47 45 4e 4f 4d 45 53 43 75 01 ...` and is 24 MB (the repo's analysis counted 7025 blobs: 586 vs_3_0 + 6439 ps_3_0). `ReadShaderPool` 0x10238930 rejects wrong magic/version/fingerprint, and warns "Some shaders couldn't be loaded due to newer *.fx file on disk" when a source is newer.
- Cache locations: the install-root `Shader.Cache` is the prebuilt one; a writable copy is looked up in the user's Documents folder (CSIDL 5) under `\Gothic3\`. If fewer than **0x32 (50)** shaders end up loaded, the engine reads a material-list ini, shows a modal "Compile shader material cache, this may take several minutes!!!" box, compiles every listed material's system dependencies (`CompileAllSystemDependencies` 0x10236440) and writes the cache (`SaveShaderPool` 0x10238f90). ge3.INI `Render.DisableShaderCache=false` toggles cache I/O.
- `CompileShaderPoolEllement` 0x102366b0: cache key = MD5(shader file name + function name + 4-byte macro hash + all macro name/value pairs). Lookup order: known-bad map, preprocessor-id map, then the device preprocesses the HLSL, whitespace is stripped and a second MD5 over (file, function, hash, preprocessed text) is looked up, and only then a real compile runs; failures are remembered in the error map.

### 1.3 How a draw call binds a material [V unless noted]

Call chain for one mesh batch:

1. `eCRenderOp2::Render` 0x101ac140. Skips if the op is already rendered (flag bit 0); marks rendered and records the "immediate" bit (bit 1); forces culling mode 0; in the debug "marked context" forces the lowest mip; then calls `eCShaderMaterialAdmin::BeginMaterial` 0x102378e0 when the op carries a material, or `EndMaterial` 0x10237a60 when it does not; finally calls the render context's virtual `Render` (vtable +0x8c).
2. `BeginMaterial`: errors if a z-pass is open; resets the first eight sampler states; snapshots the current render states (9 dwords) to the save block (+0x630 and a second copy at +0x984); when a lightmap-combination mode is active it saves the master fogging value and forces fogging on; zeroes the per-batch dirty state (user matrices, instance counters) and sets all 0x180 bytes of the sampler slot cache to 0xff. `EndMaterial` restores the snapshot.
3. `eCRenderContextMesh::Render` 0x101d9210: looks up the record triple (main vertex-pool item, optional lightmap pool item, flag dword; 12-byte records at context+0x20, count short at +0x24). If the op's immediate bit is set it sets the world matrix and calls `eCVertexbufferPoolItemMain::Execute` 0x101da8f0 with the entity's `eCIlluminated_PS` (entity+0x180), the lightmap item and a context parameter; otherwise `AddInstance` 0x101db250 queues it for hardware instancing (sorted by lod/record, directional shadow map pointer and other keys in `SortInstances` 0x101dab50; flushed by `ExecuteAllInstances` 0x101db2f0, which `Execute` calls between solid sub-lists).
4. `Execute`: builds the vertex-stream setup (main stream, plus a second stream with lightmap UVs when the lightmap item has data), makes sure the material resource is resident (state 3 means "queued"; it waits/loads), warns "Cant render mesh with material:%s TransformationType is not set to default" for graphs that expect legacy transforms, then asks the shader for the **pass list**: `shader->GetPasses(kind 2, illumination block = illuminated_PS+0x30, lightmap combination, terrain shadow map, 0)`; for every pass `shader->ExecutePass(i, ...)` which ends in `eCGfxVisual::Execute` (the indexed draw).
5. `eCShaderBase::GetPasses` 0x1023d510 clamps the requested version to the admin's current version, picks the highest implemented version at or below it, applies the lightmap-combination fallback (combination 0 uses the passed value; combination 6 with the "indicator" flag becomes 7), and asks the version object (vtable +0x10 / +0x38) to fill the static pass table `ms_arrShaderPasses` (12-byte entries). `ExecutePass` 0x1023d760 ensures the fallback material resource is loaded, sets depth bias per material (`EnableDepthBias`), and calls the version's executor (vtable +0x14).
6. The 3_0 executor (eCShaderVersionDefault_3_0 vfn_5, 0x102461a0) decodes the bit-packed pass descriptor to an index into pre-allocated vertex-shader and pixel-shader tables (0x30-byte entries; if a slot is empty it compiles on demand through the pool), then applies `SetShaderRenderStates` and draws. From the shift/mask arithmetic: bits 2-3, 4-5 and 8-9 are small enums (directional-light/shadow mode, point-light mode, lightmap/static-light mode), bits 10 and 11 are booleans, bits 6-7 a 0..2 sub-mode. [V arithmetic, I semantics; the real permutation list is in Shader.Cache itself]
7. z-pass and shadow-map passes are compiled through macro switches `XFX_ZPASS`, `XFX_SHADOWMAPPASS`, `XFX_SHADOWMAPPASS_DIRECTIONAL`, `XFX_SHADOWMAPPASS_PROJECTED`, `XFX_SHADOWMAPPASS_POINT` (strings in the HLSL driver at 0x10246ca0, which also sets the generic `XFX_VS_` / `XFX_PS_` families). [V]

`SetShaderRenderStates` 0x1023d0e0 turns the shader **blend mode** (+0x48) into a state record: mode 1 = alpha-test using the mask-reference byte (+0x4c) scaled by the current global alpha, compare "greater-equal"; mode 2 = alpha blend (also with mask reference when set); modes 3..8 = fixed blend combinations (the sky uses mode 7 for stars and sun, mode 5 for the moon); default = opaque unless the shader says it is blended or the global alpha differs from 1, in which case it falls into the alpha-blend record. The global alpha (shader admin +0x450, uploaded as fGlobalAlpha) is what the object fade-out writes (7.1). [V structure, I enum names]

### 1.4 Global shader constants owned by eCShaderMaterialAdmin [V]

`Create` 0x10239840 registers, by name: matWorld, matViewProjection, matView, matInvView, matProjection, matInvProjection, vecCamPos, vecFogAndDepthOffset, colFog, vecVPosToRefractmap, fLeafLoDSizeAdjustment, fGlobalAlpha, arrUserMatrices4x3 (count from macro `XFX_4X3_USERMATRIX_COUNT`), vecTerrainShadowScale, vecTerrainShadowPos, arrInstanceMatrices, arrInstanceStaticLightData, arrInstanceGlobalAlpha (count from `XFX_MAX_SHADER_INSTANCES`).

Light/shadow constants are separate shader-element constants registered in the same table: `DirLight` (colDiffuse, colSpecular, vecInvDir), `PointLight` (colDiffuse, colSpecular, vecPosAndInvRange), `arrSHCofficients` (eCShaderConstantSphericalHarmonics, up to 4 float4), `arrDirShadowMapPCFOffset` and `arrLsPShadowMapPCFOffset` (up to 2 float4 each), `matShadowMap`, `matDetailShadowMap`, `smpShadowMap`, `smpDetailShadowMap`, `smpPointShadowMap`, `smpTerrainShadowMap`, `vecShadowDensity`, `vecShadowFadeOut`, `vecShadowParams`, `fPntShadowMapBiasRcpCubeSize`, `vecShadowMapLightPosAndRcpRange`, lightmap sampler, refractmap sampler, depth-image sampler, `vecRcpViewPortSize`, SpeedTree `arrWindMatrices`, `arrLeafAngleMatrices`, `vecLeafClusters`.

Upload pattern: each element has an "apply" virtual (slot 2) that copies its value into a constant-buffer image at `table+0x38 + register*16` and widens the dirty register range kept in the table header (+0x20 / +0x24); only the dirty span goes to the device. Each admin constant also has a version counter that is bumped on change. [V for the SH and PCF elements; same pattern elsewhere, I]

Camera (`OnCameraChanged` 0x10237360): stores view, view*projection, projection, inverse projection and uploads them **transposed**; a flag (+0x628) selects whether the "view-projection" slot holds the plain view matrix instead (the skydome wants that). vecCamPos comes from camera +0x144..+0x14c. [V]

Refraction map (`SetRefractmap` 0x10236c40): vecVPosToRefractmap = (1/texW, 1/texH, (viewportW-1)/texW, (viewportH-1)/texH). [V]

### 1.5 Rust port implications (section 1)

- A material is a **graph that compiles to a permutation key**, not a fixed shader. Effective key: shader family (default / skin / leaf / water / skydome / vegetation / particle), blend mode, texture-count macros, tangent-space/bump/parallax/reflect/rimlight/specular/skinning/wind options, light configuration (directional on/off + shadow type, point on/off + shadow, lightmap combination, static-light data), pass kind (final / z-pass / shadow directional / projected / point). The original ships ~7000 SM3 blobs. A wgpu port can use shader defs or override constants, but should key pipelines on this same set so sorting matches.
- A batch is N passes from `GetPasses`; static-light-only and dynamic-light shaders are separate permutations, not branches.
- Keep the global constant set and meanings (fog as `(end/(end-start), -1/(end-start))`, refractmap scale using viewport minus one, transposed matrices if reusing the HLSL).
- Reproduce Begin/EndMaterial state save/restore: a material must not leak blend / alpha-test / fog state; z-pass and normal material are mutually exclusive (the original errors).
- Shader.Cache is only compiled D3D9 bytecode; the HLSL source ships in Materials.pak (ORIGINAL_SHADING.md already used it). Do not try to execute the cache.

---------------------------------------------------------------------

## 2. Lighting constants

### 2.1 Key functions

| Address | Function | Note |
|---|---|---|
| 0x100e4920 | eCIlluminationAdmin::GetIlluminationAt (1087 B) | CPU illumination sample at a point (gameplay/particles). [U] not decoded |
| 0x100e5620 | GetDynamicStaticLightData (544 B) | per-object static-light packet, see 2.3 |
| 0x100e4d60 | GetEffectedDynamicLights | dynamic point lights touching an object |
| 0x100e5aa0 | OptimizeLightSets | merges light sets |
| 0x100e7c70 | eCLightSet::UpdateIllumination (1221 B) | builds the light arrays, see 2.2 |
| 0x100ee450 | eCIlluminated_PS::UpdateIllumination | per-object refresh, see 2.4 |
| 0x100ecf50 | eCHemisphere_PS::UpdateSHHemisphere (870 B) | 4-vector SH ambient, see 2.5 |
| 0x100edbf0 | unnamed helper | adds one directional colour to the SH accumulator |
| 0x10241bd0 / 0x10241c70 | SH constant create / apply | arrSHCofficients |
| 0x10241cd0 / 0x10241fb0 | DirLight / PointLight constants | colDiffuse, colSpecular, vecInvDir / vecPosAndInvRange |
| 0x10094570 | eCGfxLight::Execute | legacy fixed-function light, not on the shader path [I] |
| 0x100f31a0 .. | eCStaticLight_PS | thin: IsObjectInRange/IsBoxInRange; heavy work is offline lightmapping |

### 2.2 Light sets [V]

`eCLightSet::UpdateIllumination` walks the set members and fills two arrays of fixed 0x34-byte records:

- Directional lights (property-set type id 0x10): diffuse colour (light +0x28..0x30, w = 1) times intensity (+0x44); specular colour (+0x38..0x40) times the same intensity; direction vector (+0x5c); a ref-counted pointer to the light's shadow map (+0x80).
- Point lights (type id 0x11; only when enabled, otherwise the log says "Disable point light still exist in lightset!"): position transformed with a translate/scale pair into set space, colour times intensity, a scalar at +0x2c, three components copied at +0x20..+0x28, shadow-map pointer at +0x30.

The same function calls `UpdateSHHemisphere`, so each light set carries an ambient SH. [V call; per-set ownership I]

### 2.3 Per-object static-light packet [V]

`GetDynamicStaticLightData` collects, for the object's world AABB, registered static lights (property-set type id **0x48**), tests them with a box-vs-light check, ranks them by a weight function (`qsort` with `SortWightedLights` 0x100e55e0; candidate buffer capped at 10, then trimmed to **3**). It writes a **0x70-byte packet = 7 float4**, structure-of-arrays:

- float4 #0, #1, #2: world X, Y, Z of lights 0..2 (the 4th lane unused).
- float4 #3, #4, #5: light R, G, B each multiplied by the constant **2.0** (double at 0x104885f0).
- float4 #6: 1 / (range^2) per light.

`SetShaderInstanceStaticLightData` 0x10236fb0 copies the packet for instance i to offset i*0x70 of the instance array (for instanced draws); non-instanced draws use the same packet as a constant. So local static lights are **not only baked**: the pixel shader evaluates up to three analytic point lights per object in addition to lightmaps.

### 2.4 Illuminated objects [V]

`eCIlluminated_PS` properties: EnableDynamicLighting, EnableAmbient, StaticIlluminated, CastShadows, ReciveShadows, ReciveTreeShadows, CastStaticShadows, ReciveStaticShadows, DirectionalShadowType.
`UpdateIllumination`: phase A (dirty flag +0xb8; only when the object's light mode value equals 1, i.e. dynamically lit): fetch the static-light packet into +0x3c (bool at +0x39 = "has static lights"), fetch effected dynamic lights, clear dirty, clear the occlusion query. Phase B (no query pending, object has a directional shadow map whose detail flag is set): register the object as an occlusion-test item on that shadow map, later resolve the query via `eCOcclusionProcessor::ResolveOCQuery`; the result lands in +0x38 ("not visible in the shadow's region, skip"). `Request` flag 0x80 triggers this once per element per frame.

### 2.5 SH hemisphere ambient [V]

`eCHemisphere_PS` stores three colours and a few orientation fields (setters: SetGeneralAmbient +0x28, SetBackLight +0x38, SetGroundLight +0x48, SetSunLight colour + direction at +0x68, SetIntensity +0x60, SetBackLightAxisDirectionOffset euler at +0x54, SetSunLightAxisDirectionOffset +0x74/+0x78/+0x7c). `UpdateSHHemisphere`:

1. Build a rotation from the back-light axis offset composed with the owner entity's world matrix; the rotated Z axis (negated, then normalised) is the back-light direction.
2. Scale the three colours by the intensity (+0x60).
3. Zero a 64-byte accumulator (4 float4) at +0x84; write the general ambient as the constant (L0) term; then call the helper for the back light along its direction, for the ground light along straight down (0, -1, 0), and, if the sun-light colour has a non-zero squared magnitude, for the sun light along the rotated sun axis (yaw then pitch from +0x74/+0x78).
4. The helper adds colour x **0.2820950** to float4 #0 and colour x **0.4886025** x dir.{x,y,z} to float4 #1, #2, #3 (the real-SH L0 and L1 constants; floats at 0x10426c94 and 0x10426c98). Shader side: `sh0 + sh1*N.x + sh2*N.y + sh3*N.z` (ORIGINAL_SHADING.md).

The weather system rewrites hemisphere colours each frame (3.3), so SH is recomputed only when the properties notify, not per object.

### 2.6 Lightmaps [V classes, U runtime details]

Classes: eCLightmap (`FillVertices` 0x101d3520 fills the lightmap UV stream and rejects incompatible meshes: "Lightmap %s is incompatible to mesh %s! Please delete lightmap ..."), eCLightmapBitmap, eCLightmapBatcher, eCLightmapTexture, eCResourceLightmap_PS (`Read` 0x101cbb70, `DoSave`, `CreateAmbientOcclusionGrid`), eCVertexbufferPoolItemLightmap (`CreateBuffers` 0x101da160: separate vertex stream), eCVertexProcessorLightmap, offline bakers (eCStaticLightPreprocessor::ComputeStaticLighting logs per-LOD lightmap level; eCAmbientOcclusionPreprocessor / Grid; image filters eCIPImageFilterLightmapLight / LightmapBorders with PCF shadow matrices).

Binding: lightmaps live under `Data\Lightmaps` (string in `eCIlluminationAdmin::Invalidate` 0x100e66f0). Each LOD stage of a mesh may have its own lightmap pool item (the triple in 1.3). The shader receives the texture through eCShaderEllementConstantLightmap and the UVs from the second vertex stream; the pass descriptor's lightmap-combination field (eEShaderMaterialLightmapCombi, 0..7) picks none / lightmap only / lightmap + dynamic, with or without static lights (exact mapping I). Visual properties eELightmapType and eELightmapAmbientOcclusion live on eCVisualMeshBase_PS. Pixel maths (lightmap rgb x4, AO in COLOR1 alpha) is in ORIGINAL_SHADING.md and docs/research/LIGHTMAP_FORMAT.md.

### 2.7 Rust port implications (section 2)

- Inputs per pass: 4-term SH ambient (L0, L1 with 0.282095 / 0.488603), one directional light (diffuse, specular, inverse direction), one dynamic point light (position + inverse range, diffuse, specular), plus the **three analytic static point lights per object** (position, colour x2, 1/range^2) and baked lightmaps/AO. The packet is computed per object from its world AABB, not per vertex.
- Lights are gathered per light set; a simpler per-object nearest-N is acceptable but keep the caps (3 static, 1 dynamic point, 1 directional).
- Multiply colours by their Intensity property before upload; static-light RGB gets the extra x2.
- Recompute SH only when the hemisphere or weather properties change.

---------------------------------------------------------------------

## 3. Fog and weather

### 3.1 Fog [V]

- `eCGfxFog` (Execute 0x1008f420, SetFog 0x1008f3e0): enabled flag, colour (BGR bytes), start, end. `Execute` is idempotent (skips if already active and clean), sets the device fog states (`eCDX9Device::SetFog` 0x100a26e0 / `SetMasterFogging` 0x1009eb90) and then calls `eCShaderMaterialAdmin::OnFogChanged` 0x10237270.
- `OnFogChanged`: enabled -> colFog = (R,G,B)/255 (double 255.0 at 0x10488480), vecFogAndDepthOffset.x = end/(end-start), .y = -1/(end-start). Disabled -> x = 1.0, y = 0.0. Shaders compute fog = saturate(dist*y + x) (radial, per vertex; ORIGINAL_SHADING.md) and blend `lerp(colFog, colour, fog)`. [V; which gfx-fog field is start/end is I]
- Fog override: `SetMasterFogging(on)` is forced by BeginMaterial in lightmap-combination mode, by BeginZPass, and by the skydome (so the sky does not receive distance fog).
- Fog parameters come from weather: zone properties FogColor, FogStart, FogEnd (with Overwrite flags; setters 0x1028bcc0..0x1028be10). [V names]

### 3.2 Weather classes

| Class | Role |
|---|---|
| eCWeatherAdmin (Process 0x1028d070 6208 B, LoadWeatherStates 0x1028c870, SaveWeatherStates 0x1028ccb0, RegisterWeatherZone 0x1028cf00) | Owns weather states, environments and zones; each frame computes a blended state and pushes it into lights, hemisphere, skydome, fog and particle emitters. Loads `Data/Library/WeatherStates.g3ws`. |
| eCWeatherEnvironment (Process 0x1028f780, AddWeatherStateBlendFactor 0x1028fb50) | Named environment (default name "Myrtana") = a set of weather-state weights and probabilities. |
| eCWeatherStates (SetCurrentDayTime 0x102915c0, Create 0x10290d10, Start/Stop/UpdateLoopSound) | One named state: values keyframed over day time plus particle (rain/snow) parameters ParticlesPerSecond, SpeedMinMax, Size, DirectionScale, TextureTile, SpawnPointDizzer, JitterPower, JitterSpeed, SplashDuration, SplashTextureTile, SplashSizeScale and a looping ambience (LoopSample, LoopVolume). |
| eCWeatherZone_PS (AddWeatherStateBlending 0x102970b0, Box/Circle/Rect/SphereBlendFactor, DoOverwrites 0x10296ed0) | World volume that contributes a weather state and/or overwrites fields. |

### 3.3 `eCWeatherAdmin::Process` order of operations [V structure]

1. Resolve the "World_MCP" world-control entity (it owns the DirectionalLight, Hemisphere, Skydome property sets) through an entity proxy, lazily.
2. Day time = member +0x30 wrapped to [0,1). Mode (+0x284): **0** probe position fixed at +0x274; **1** probe = camera position (application +0x34 override camera else +0x94 main camera, position at +0x8c); **2** preview: every state gets its own current day time and all are blended with equal normalised weights; **3** fixed day-time constant (DAT 0x10488614).
3. For modes 0/1: process every environment (`Process(dt)`), set every state's current day time (interpolates its keyframes), then walk zones in priority order (lazy sort), asking each for a blend factor at the probe point (AABB containment, then shape function). Accumulated weight is capped at 1.0 (the last contributing zone is clipped). Each contributing zone calls `AddWeatherStateBlending` (with its overwrites). Whatever weight remains goes to the highest-probability environment (a stored threshold at +0x25c, else the first environment, else the admin default at +0x18c).
4. The blended record sits at admin +0x34 (about 0x8c bytes). Layout from `DoOverwrites`: colour at +0x00 (sun diffuse), +0x10 (sun specular), scalar +0x20 (intensity), colours +0x28, +0x38, scalar +0x48, colour +0x4c, scalars +0x5c, +0x60, colours +0x64, +0x74, +0x88, scalar +0x84. Overwrite modes per field: **1 replace, 2 multiply, 3 add** (0 none). [V]
5. Push to the engine through property writes (so notifications fire): DirectionalLight Color, SpecularColor, Intensity, DirectionOffset (euler), ShadowDensity; Hemisphere GeneralAmbient, BackLight, Intensity, BackLightAxisDirectionOffset, SunLight, SunLightAxisDirectionOffset (several colours are products/sums of state colours via `GetProduct`/`GetSum`, i.e. ambient parts are derived, not stored individually; exact formula I); Skydome SkyColor, HazeColor, SunColor, CloudColor, SunNormal plus sun / moon rotation matrices (yaw = 2*pi*day time, pitch for elevation; moon phase uses (t - 0.5)*2). Particle emitters get state parameters scaled by blend weight; ambience loop volume is set via FMOD `Channel::setVolume` with the same weight, sounds start/stop when a weight crosses zero.
6. The name of the dominant zone/environment is stored at +0x28c for script/UI queries.

### 3.4 Zone shapes [V box, I others]

`GetBoxBlendFactor`: transform the probe into zone-local space; take absolute coordinates; 0 outside OuterExtends on any axis; 1 strictly inside InnerExtends on all axes; between: for each axis with positive span take (|p|-inner)/(outer-inner), square, sum, and the result is 1 - sqrt(sum) clamped at 0. Rect/Circle/Sphere variants use InnerRadius/OuterRadius analogously.

### 3.5 Rust port implications (section 3)

- Store the fields explicitly: sun diffuse/specular, intensity, direction offset, shadow density, ambient general/back/ground/sun, sky/haze/sun/cloud colours, fog colour/start/end, particle parameters.
- Pipeline: keyframes on a 0..1 day axis -> zone/environment weights (box falloff above, total <= 1, remainder to default environment) -> overwrite stage (replace / multiply / add) -> one blended record -> lights, hemisphere SH, sky, fog.
- Sun and moon direction are computed from day time at runtime, not keyframed.
- Where the day-time keyframes actually live (world environment resource) is [U]; `WeatherStates.g3ws` holds rain/snow particle state only.

---------------------------------------------------------------------

## 4. Sky

### 4.1 Classes and addresses

| Address | Function | Note |
|---|---|---|
| 0x102709c0 | eCSkydome_PS::OnRender (1493 B) | opaque part: stars, sun, moon, dome |
| 0x10270fa0 | OnRenderAlpha (1790 B) | alpha part (clouds / lens flare / sun occlusion) [I] |
| 0x1026f440 | CreateBuffers (2280 B) | dome mesh and billboards (dome tessellation constants 24 and 25 appear in this function [I: rings x segments]) |
| 0x1026fdd0 | CreateShader (2627 B) | builds the sky material graph at run time: eCShaderSkyDome with BlendMode, SunOcclusionOnly, BillboardOnly options; samplers (ImageFilePath), scrolling Direction/Scale, CombinerType |
| 0x10271ca0 | CreateStars | random star billboards |
| 0x10271e80 / 0x10272060 / 0x10271f60 | CreateSun / CreateMoon / CreateSunOcclusion | billboard quads |
| 0x10272160 | CreateLensFlare (1817 B) | composes lens-flare textures into an atlas (names must end with '0', pattern `%s%i%s`) |
| 0x10272880 | CreateAbsorbationImage (2791 B) | derives an absorption image from the cloud texture, 4.3 |
| 0x10273540 / 0x10273700 | CreateSkydomeEnvCubemap / UpdateSkydomeEnvCubemap (3967 B) | sky into a cube map for reflections |
| 0x100e6230 | eCIlluminationAdmin::UpdateSkydomeCubemap | triggers the update through the World_MCP entity |
| 0x1026e4c0 / 0x1026e7d0 / 0x1026eb30 | eCSkyObject CreateBuffers / FillVertexBuffer / FillIndexBuffers | generic sky mesh (vertex buffer n*128 bytes, index buffer n*12 bytes) |
| 0x1024cba0 / 0x1024d030 | eCShaderSkyDome ctor / GetDeclarations | colSun, colSky, colHaze, colClouds, vecSunNormal, fApparentDepth; macros `XFX_SKYDOME_BILLBOARD`, `XFX_SUN_OCCLUSION_BILLBOARD` |

Properties of eCSkydome_PS: SunSize, MoonSize, LensFlareTextures, LensFlareSize, CloudTexture1, CloudTexture2, MoonTexture, SunTexture, StarTexture. [V]

### 4.2 Draw order and state (OnRender) [V]

1. Active camera (or the sky's own cube camera when rendering reflections, flag +0x2b8); rebuild buffers if the viewport size changed.
2. `BeginMaterial`; remember master fogging and stencil; **force fogging on**; set **master stencil testing mode 2** with compare 3, reference 1, ops 1/1/1, masks 0xffffffff so sky pixels carry stencil value 1.
3. Camera: view matrix with translation zeroed (sky follows the camera) and a copy of the projection with the explicit depth tweak (constants 1.0 and 0) so the dome sits at the far plane.
4. Draw stars (blend mode 7, tint +0x270), sun (mode 7, tint +0x1c), moon (mode 5, tint +0x34), each with its own world matrix (+0x1b0, +0x1f0, +0x230).
5. Dome: two cloud layers with UV scale 1/size (+0x78, +0x7c) and scroll direction (+0x70); sky/haze/sun/cloud colours (+0x5c..+0x6c) are copied into the constant blocks of both the dome and cloud material instances. Run every pass of the dome shader and draw the dome visual. Restore stencil and fogging, `EndMaterial`.

Dome shader maths (VS horizon-haze weight, sun lobe pow(max(dot,0),300), PS scrolling cloud layers) is in ORIGINAL_SHADING.md; the engine only supplies the constants. [V]

### 4.3 Absorbation image [V structure, I purpose]

`CreateAbsorbationImage(cloudTexture)` uses the file `Data/_Intern/_Absorbation_New.tga` as an on-disk cache: it compares archive file times and only regenerates when the cached file is missing or older than the cloud texture. Regeneration: two A8R8G8B8 images (format 0x15), copy the cloud texture, then for every texel add up **32 red-channel samples along each of four directions** (+x, -x, +y, -y; wrap-around via modulo on width/height, unrolled in groups of 8, four groups), multiply each sum by **0.03125 = 1/32** (double at 0x10488940), and pack the four results into the four channels. Net effect: per texel, the mean cloud opacity along four directions, a precomputed directional integral so the shader can darken sun / sky colour through clouds without ray marching. [V arithmetic; use I]

### 4.4 Environment cube map [V]

`CreateSkydomeEnvCubemap(size)`: square cube, one mip, format 0x15, with a dedicated eCGfxCamera. `UpdateSkydomeEnvCubemap` renders the six faces with a 90 degree projection; the projection constants -1.001001 / 1.001001 equal f/(f-n) for **n = 1, f = 1000**. Materials sample the result through eCColorSrcSkydomeSampler / eCShaderEllementConstantSamplerSkydome (expression `texCUBE(... , XFX_PS_REFLECT)`; property MinSize). The illumination admin drives the refresh.

### 4.5 Time-of-day driving [V]

The sky reads no clock. Everything is pushed by `eCWeatherAdmin::Process` (3.3): SkyColor, HazeColor, SunColor, CloudColor, SunNormal and the sun/moon rotation matrices from day time. Stars are static random billboards (random positions from `rand()/32767`, size drawn from 0.01 .. 0.02); their brightness is the tint alpha supplied by the weather state. Cloud motion is the scroll direction in the cloud samplers.

### 4.6 Rust port implications (section 4)

- Sky is a camera-centred pass: drop translation, push depth to far, additive-style billboards for stars/sun/moon, dome + two scrolling cloud layers, write stencil marker, bypass distance fog.
- Per-frame inputs: sky colour, haze colour, sun colour, cloud colour, sun direction, sun/moon billboard transforms, cloud scroll direction and the two UV scales. Everything else is texture data (cloud 1/2, star, sun, moon, lens-flare atlas).
- The absorbation image can be generated at load from the cloud texture (32 taps per direction, scale 1/32) or on the GPU.
- Reflection probe: 6 faces, FOV 90, near 1, far 1000, refreshed at a low rate.

---------------------------------------------------------------------

## 5. Shadows

### 5.1 Classes [V]

eCDepthMap (base, seen in RTTI checks inside BeginZPass) -> eCShadowMapBase (Create 0x10103b40, SetSize 0x10103cb0, RenderShadowMap 0x10103d50) -> eCShadowMapDirectionalLightBase / eCShadowMapDirectionalLight (UpdateInternals 0x10104610, Update 0x10104d10, RenderShadowScene 0x10104bd0, IsShadowCaster 0x10104290, GetTexelsPerUnit 0x10104210, AddOcclusionTestItem 0x101043e0) -> eCShadowMapDirectionalLightLsPSM (UpdateShadowMatrix 0x101066f0, CalculateFocusedShadowPoints 0x101063e0, OnPostExtraction 0x10106ff0, UpdateCamera 0x10106d30, Update 0x10106ec0, Create 0x10105520). Separate: eCShadowMapPointLight (UpdateCamera 0x10107590), eCShadowMapSpotLight (UpdateCamera 0x10107cf0), eCTerrainShadowMap (Update 0x101084d0, CreateRenderTarget 0x101082e0, AddTreeShadow 0x101083e0). Frame side: eCFrustumDatabase::ExtractFromShadowMap 0x1019f460, ExecuteShadowMap 0x101a2220, AddShadowCasterEntity 0x1019f3b0; occluder proxies for casters: eCOcclusionProcessor::RenderShadowMapOccludees 0x101a4dc0.

### 5.2 Which shadow maps exist [V values, I naming]

Owned by `eCDirectionalLight_PS` (OnShadowSettingsChanged 0x100e8de0, UpdateShadowMaps 0x100e8630):

| Slot | Created when | Type | Resolution | Role |
|---|---|---|---|---|
| light +0x80 | illumination admin flag +0xa4 (set by `EnableDetailShadows`) on, shader-admin value +0x98 != 7 (7 behaves as "off"), light enabled | **LsPSM** (light-space perspective) | **0x580 = 1408** | main camera-following high-detail shadow (the create call receives the quality value) |
| light +0x90 | illumination admin flag +0xa5 (`EnableStaticShadows`) on | eCShadowMapDirectionalLight, orthographic | **0x400 = 1024** | camera-centred orthographic map; half-extent at light +0x94; centre snapped to a texel step of `(2*halfExtent/size) * (+0x98 factor)` so it does not shimmer |
| light +0x74 array (count +0x78) | same flag; created on demand | eCShadowMapDirectionalLight, orthographic | power of two **64 .. 512** (created at 512, resized from the cluster box's largest xy extent) | "static" **cluster** maps for objects with DirectionalShadowType == 1: an object joins the cluster whose box contains it, or the cluster whose merged box stays below an extent of **1700** (double at 0x10488c18) with the best volume ratio; otherwise a new map is created. A cluster map is released when its texel density drops below the camera-centred map's. |
| point light +0xb4 | light's ShadowCaster flag (+0x78) and shader-admin point quality (+0x9c) != 7 | eCShadowMapPointLight (cube) | **0x200 = 512** per face | **at most 2** at a time (static counter `m_iPointLightShadowMaps`; a third logs "more the 2 shadowmaps not allowed!"); position copied from light +0x80..0x88, range from +0x54 |
| spot | eCShadowMapSpotLight | perspective | [U] | camera properties Perspective, Position, FieldOfView; near plane constant **50** and far **6000** are referenced in `UpdateCamera` [V refs, I roles] |
| terrain shadow | `EnableTerrainShadows(true)` on the illumination admin (+0xa0) | eCTerrainShadowMap, orthographic top-down | **0x200 = 512** | tree shadows onto terrain, 5.5 |

Gating: `SetCurrentShaderVersion` (1.2) disables static+terrain shadows below index 2, detail shadows below index 3. User option ShadowQuality 0..4 (Off, Low, Medium, High, VeryHigh) in ge3.INI presets (Low 0, Medium 1, ...) selects the quality integers the shader admin holds; 7 = off. [V ini; mapping I]

### 5.3 LsPSM matrix construction

- `ms_fDetailShadowRadius` = **2200.0** [V: static float in .data at 0x105e38bc; nothing in the DLL writes it; initial value 2200]. The focus box around the camera uses this radius; caster boxes are intersected with the view frustum body (`CalculateFocusedShadowPoints` works from the 8 unit-cube corners with coordinates in {-1,+1}).
- `UpdateShadowMatrix` builds shadow view/projection (stored at +0x84 and +0xc4), warps with a near-plane parameter derived from the focus depth range and radius/1000, uses a depth compression scale of **0.005** (float at 0x105efa78) on z, and writes the texture matrix with half-texel bias (0.5/size) and constants -0.5 and 2e-5. [V constants present; roles I, exact LsPSM n_opt formula U]
- `OnPostExtraction` adjusts ClipDepth / Perspective on the shadow camera after the casters are known; property names `Perspective`, `ClipDepth`, `Position`, `ViewMatrix`, `ProjectionMatrix`.
- Receiver side constants: `matShadowMap` (LsPSM) and `matDetailShadowMap` (ortho detail), `arrLsPShadowMapPCFOffset` / `arrDirShadowMapPCFOffset` (**2 float4 each = four 2D PCF taps**, one-texel steps), `vecShadowDensity` (weather ShadowDensity), `vecShadowFadeOut`, `vecShadowParams`, `fPntShadowMapBiasRcpCubeSize` for point lights.

### 5.4 Caster and receiver selection [V flag path]

- `UpdateInternals` walks the light's registered illuminated property sets and accepts an object as caster if its CastShadows byte (+0x20) is 1, its light-mode value (+0x28) is not 2, the owner entity flag word has bit **0x100000** (global flag constant DAT 0x1060dc18) and `IsShadowCaster(worldBox)` passes (it rejects objects far outside the map's box in light space or tiny compared with the map extent [I]). Accepted boxes are merged into the shadow volume and the near depth is pulled back to the nearest caster.
- Receivers (ReciveShadows) get a directional map pointer through `eCIlluminated_PS::SetDirectionalShadowMap`; that pointer is also a sort key in the instancing sorter (1.3). DirectionalShadowType == 1 uses cluster maps; other values use the camera-centred map (`UpdateShadowMaps` branches on field +0x28 == 1).
- Hardware occlusion tests for casters: `RenderShadowMapOccludees` draws proxy boxes with queries; the result sets the receiver's skip flag (2.4).
- Cluster maps update only when marked dirty (+0x168); `RenderShadowScene` sets scissor/viewport to the shadow target and renders with the shadow-map macros.

### 5.5 Terrain shadow map and tree shadows [V]

`eCTerrainShadowMap::Update` 0x101084d0: 512x512 target; centre = camera position with x and z rounded to whole units (texel-stable); world side = 2 * half-extent (+4); a small time-based jitter (`GetTickCount`, sin/cos scaled by +0x20) is added before publishing; publishes `SetTerrainShadowMapData(pos, 1/side)` (vecTerrainShadowPos / Scale) to the shader admin; if the snapped centre moved (or the target is new) it re-renders: orthographic top-down camera (view = translation, projection = uniform scale 1/half-extent), clear 1.0, SpeedTree casters drawn by `eCSpeedTreeAdmin::EndRenderTerrainShadows` 0x10209590 / `eCSpeedTreeRenderBuffer::RenderTerrainShadows` 0x10207960. Materials sample it through `smpTerrainShadowMap`, applied only when the terrain-shadow object exists (eCShaderEllementConstantTerrainShadowMap).

### 5.6 Z-pass types [V]

`BeginZPass(depthMap, flag)` 0x10237ab0 derives a pass type from the depth map's RTTI: **1 point light, 2 LsPSM or spot, 3 orthographic directional, 4 generic/occluder**. It forces fogging on, resets samplers 0..3, **disables colour writes** (types 3..6 plus the generic case), disables alpha writes unless the type is 1 or 4, and sets a depth-write state block whose cull mode depends on `flag` (double-sided vs. single). `EndZPass` 0x10237d80 restores.

### 5.7 Rust port implications (section 5)

- Three directional systems: (a) camera-following light-space-perspective map, 1408^2, radius 2200; (b) a few cached orthographic cluster maps, 64..512^2, cluster extent cap 1700, DirectionalShadowType 1; (c) terrain tree-shadow ortho map 512^2. Point lights: cube 512^2 per face, hard cap of 2 casters. Spot: near 50, far 6000.
- 4-tap PCF at one-texel offsets; weather ShadowDensity as strength; fade-out constant for distance.
- Casters filtered by entity cast-shadow flag, light mode != 2, box test, optional occlusion test. Receivers pick a map by DirectionalShadowType.
- Dedicated depth-only pipelines per light type, colour writes off. ShadowQuality 0 turns the lot off.

---------------------------------------------------------------------

## 6. LOD selection

### 6.1 Pipeline per visible element [V]

1. `eCFrustumElementItem::UpdateNodeData` 0x101a3390: for entities carrying flag 0x8000000 or 0x10000000 (two "ranged" categories; flag constants initialised at DAT 0x1060de70 / 0x1060de78) store the distance from the active database camera (db +0x134, position +0x8c) to the entity position (+0x70) in element +0x3c. If the entity has both 0x80000 and 0x100000 use its bounding sphere at +0x100, otherwise the sphere at +0xd8, and compute `CalculateAreaFactor`, stored as element value #0 (area factor). If the area factor is at or above the double **3.0e-6** (0x104888c0) and the entity has children, the children are processed too; entities without the render flag (0x200000) get area 0.
2. `CalculateLoDArea` 0x101a2f00: **LoD input = db global LoD factor (+0x1a4) x area factor x entity LoD scale (+0x128) x entity-type LoD scale (type object +0x48)**, stored in element +0x08. [V] The database factor comes from `GlobalVisualLoDFactor` (application settings +0x110, ge3.INI `ObjectDetails.fGlobalVisualLoDFactor_*`).
3. `UpdateLoD` (render-context virtual +0x78, called from `Request` with a pointer to element +8, the `eSEntityLoDInfoDesc`): meshes run `eCRenderContextMesh::UpdateLoD` 0x101d8c90 which stores `CalculateLoDStage` 0x101d9080 into `context+0x2d`.
4. `eCFrustumElementItem::CullSmallObject` 0x101a2f30 -> `eCFrustumFilter::CullSmallObjects` 0x101ac6d0 computes the **fade-out value** in [0,1] stored at element +4 (clamped by an entity-level override at entity +0x34; incremented statistic when 0). Values other than 1 pass through the smoothing function at 0x10039530 = **0.5 * (1 + sin(pi * (x - 0.5)))** (a cosine ease; derived from the instruction bytes). Frozen debug contexts clamp to 0.5.
5. `Request` consumes the fade: **if fade != 1 the element is removed from the solid/z-pass lists and forced into the alpha list**; the op carries the fade and it becomes the shader's global alpha (`SetFadeOutValue`/`GetFadeOutValue` on eCRenderOp / eCRenderOp2; `FillAlphaRenderOps` writes it, or 1.0 when database flag +0x1aa says "alpha at full strength").

### 6.2 Area factor and mesh stage thresholds [V]

- `CalculateAreaFactor` 0x101ac5a0: sphere centre translated into camera space, projected on the camera forward vector (filter +0x28) to get depth z (z == 0 -> 1e-5); **factor = pi * (r / z)^2 clamped to [0,1]**. Not field-of-view dependent (zoom does not change LoD).
- `CalculateLoDStage` 0x101d9080 input `a` = element +8. If the context has no LoD stages (count byte +0x2c == 0) -> stage 0. Otherwise: stage 0 if `a > 0.25`; else the first i in 0..3 such that `upper_i >= a > lower_i` with the table **upper = 0.25, 0.04125, 0.0078125, 0.003407; lower = 0.04125, 0.0078125, 0.003407, 0.00095** (floats at 0x104263d8 and 0x104263dc). So stage 0: (0.04125, 0.25] and above; stage 1: (0.0078125, 0.04125]; stage 2: (0.003407, 0.0078125]; stage 3: (0.00095, 0.003407]. If `a` < **0.00095** (double at 0x10488a30) the last stage (count-1) is chosen. Result is clamped to count-1. A statistic is bumped.
- Because `a` includes the global LoD factor, ge3.INI presets matter: VeryHigh 1.0; High/Medium/Low 0.01 (an object needs 100x the screen area to keep the same stage). [V ini; interpretation I]

### 6.3 Small-object culling and distance fade [V]

`CullSmallObjects(areaFactor, entity)`:
- Entities with flag 0x8000000 or 0x10000000 use a **distance range**: base range = (application settings +0x1a8) + entity +0x12c for 0x8000000, or the world object's +0x78 for 0x10000000; fade window = **0.15 x range** for 0x8000000 (double 0.15 at 0x10488570) or settings +0x1a4 for 0x10000000; value = 0 if entity distance (+0x188) exceeds the range; linear ramp to 1 over the window just inside the range; 1 otherwise. (ge3.INI keys: `fProcessingRangeFadeOutRange_*` 200/300/500/700, `fRangedBaseLoDOffset_*` 800/400/100/-200.) [V structure; mapping to ini keys I]
- All other entities: threshold **S = entity +0x12c x filter +0x34 x entity-type +0x4c** (filter +0x34 is the database's screen-cull factor, presumably `fScreenObjectDistanceCulling_*`: 0.002 VeryHigh, 0.008 High, 0.02 Medium, 0.045 Low [I]). If the area factor is 0 -> 0; if it is below S - 0.4 S (double 0.4 at 0x10488760) -> 0 (culled); if above S -> 1; between, `1 - (S - area) / (0.4 S)` (linear band [0.6 S, S]). Then the ease of 6.1 step 4.
- `fLowPolyObjectDistanceCulling_*` (0.01) belongs to the far low-poly pass (7.3) [I].

### 6.4 Mesh/animation LOD resources [V names, U details]

- eCResourceMeshLoD_PS (Read 0x101cf980, AddLoDMesh, AddLoDMeshComplexFile, GetBoundary; properties LoDScale and a bounding box) lists per-stage mesh files. eCRenderLoDMesh (Prefetch 0x101d96c0) prefetches stage meshes. eLoDMesh::eCProgMeshData (Crunch, Mend, WriteBackProgMesh, SaveComplex) is the offline progressive-mesh (edge-collapse) generator; eCSplitMeshUniformGrid / QuadTree split big meshes; `eCDX9Device::GenerateLODMesh` falls back to D3DXSimplifyMesh.
- eCVisualAnimationLoD: characters have separate actor files per LOD (`%s_LOD%i%s`); animation files without LoD (v1.0) are rejected as obsolete. ge3.INI `enuMeshLoDQualityStage_*` / `enuAnimationLoDQualityStage_*` (0 Normal, 1 High, 2 UltraHigh) pick the quality stage; their precise effect is [U].
- Application settings relevant here (from `eCApplication::OnCaps`): EnableStaticVisualsRendering, EnableDynamicVisualsRendering, EnableLightMaps, EnableOcclusionCulling, OCCheckThreshold, OCPixelOverdrawThreshold, EnableObjectVPTHCCulling, DisableAlphaRendering, DisableStencilTesting, DisableModuleRendering, EntityROI, ScreenObjectDistanceCulling (+0x10c), GlobalVisualLoDFactor (+0x110), FarClippingPlane (+0x108), FarClippingPlaneLowPoly, RenderDebug. ge3.INI far planes: 10000 / 8000 / 6000 (High / Medium / Low), low-poly far planes 100000 / 50000 / 25000.

### 6.5 Vegetation grid [V]

- eCVegetation_PS (properties ViewRange, FadeOutStart, GridNodeSize, UseDefaultViewRange) owns an eCVegetation_Grid (+0x5c). Property handler 0x1027eda0: ViewRange / FadeOutStart changes copy into the grid (+0x10 view range, +0x48 fade start, +0x4c fade end); GridNodeSize calls `SetNodeDimension` (clears the cache, rebuilds).
- `OnRender` 0x1027e8a0 runs only if the admin vegetation switch (+0x10, the "Toggle Vegetation" debug switch) is on. With UseDefaultViewRange and a view range differing from the global one (admin +0x38; ge3.INI `VegetationViewRange.fViewDistance_*`: VeryHigh 7500, High 6500, Medium 5000, Low 4000): FadeOutStart = global x random(**0.65**, **0.75**) (a fresh `rand()` value) and the view range becomes the global.
- `eCVegetation_Grid::Render` 0x1027c9a0: compute a cache rectangle around the camera (`CalculateCacheRect`, `CacheIn` builds missing node geometry), build a view-projection frustum, begin the shader material, upload **alpha dissolve start = fade start, end = view range** to the vegetation shader, set the culling sphere radius = view range x (1.0, or **0.45** when camera flag +0xe1 is set, presumably a secondary camera), and render every grid cell that intersects both the sphere and the frustum via `eCVegetation_GridNode::Render`. `eCVegetationAdmin::GetMaterial` 0x1027a480 caches the material (MaskReference property); `GetVertexDecl` 0x1027a360.
- Fade is an **alpha dissolve in the pixel shader** (constants fAlphaDisolve*), not geometry LOD [I]. Player push on grass uses vecPlayerPos; ge3.INI `Render.GrassResponseRange` default 130.0.

### 6.6 SpeedTree [V names, I behaviour]

- eCSpeedTree_PS, eCResourceSpeedTree_PS, eCWraper_SpeedTreeRT (wraps `CSpeedTreeRT`), eCSpeedTreeRenderBuffer (RenderBranches 0x10206e10, RenderFronds 0x10207190, RenderLeafs 0x10207510, billboards), eCSpeedTreeAdmin (OnBeginRenderSolid 0x10208f60, OnEndRenderSolid 0x10209200, EndRenderTerrainShadows 0x10209590), eCRenderContextSpeedTree (FillSolidRenderOps always emits one op with fade 1.0, Render queues the tree into its render buffer, UpdateLoD forwards the camera position).
- LOD: `eCWraper_SpeedTreeRT::UpdateLoD` 0x1020b050 writes the camera position into the SpeedTree RT instance and asks the SDK to recompute and apply the LOD (the SDK expects 0..1). The distance curve therefore lives in the tree asset; the engine adds only fLeafLoDSizeAdjustment and the leaf-cluster / angle matrices.
- Wind: `OnBeginRenderSolid` advances each wind instance by frame time / 1000 (application +0x5ec milliseconds -> seconds), builds the wind matrix array (4 matrices + 4 identity) and the leaf angle matrices. Vegetation (non-tree) wind uses vecAmplitude, vecPhase, vecWaveLength with `SetTime` 0x10249bc0.
- Trees are also rendered into the terrain shadow map (5.5).

### 6.7 Rust port implications (section 6)

- Implement the exact chain: area = pi*(r/z)^2 (z along camera forward) clamped [0,1]; LoD input = area x global factor x entity scale x type scale; stage thresholds 0.25 / 0.04125 / 0.0078125 / 0.003407 / 0.00095; below 0.00095 the last stage; clamp to available stages. Take the global factor from the ge3.INI presets.
- Implement fade-out separately: S threshold scaled by the screen-cull constant; ramp band [0.6 S, S]; ease 0.5(1+sin(pi(x-0.5))); fade < 1 means alpha list with global alpha = fade (and the depth-prefill rule of 7.1). No hard popping.
- Vegetation: grid cells, per-cell sphere + frustum culling, pixel-shader dissolve between fade start and view range, fade start jittered 65-75% of view range, view range 4000-7500 from the quality option.
- SpeedTree: use the asset's own LOD curve; wind advanced with real seconds; separate branch / frond / leaf / billboard buffers; trees cast into the terrain shadow map.

---------------------------------------------------------------------

## 7. Render queues, ops, culling, post-processing

### 7.1 Contexts and ops [V]

- eCRenderContext (base: Create/Destroy/UpdateDepthSortValue/GetDepthSortValue/SupportsZPass/SupportsOccluderZPass) -> eCRenderContextMesh, eCRenderContextAnimation, eCRenderContextLegacy, eCRenderContextSpeedTree, eCRenderContextDebug. Mesh context: record array (+0x20; 12-byte records), count (+0x24), stage count (+0x2c), current stage (+0x2d).
- eCRenderOp (older per-entity op: ExecuteMeshPath 0x101abec0, ExecuteOther 0x101abb00, Execute 0x101abd10, ExecuteZPass 0x101abcb0) and eCRenderOp2 (24-byte op: context, record index, material, entity, flags, fade value; Render 0x101ac140, RenderRefractionMask 0x101ac220, SupportsZPass, IsPrefetching, IsRenderedImmediately). `eCRenderOp::Execute` bumps a recursion guard on the context, runs the mesh path then the "other" path (animation etc.), draws bounding lines / names / axes when the matching database toggles are on, and decrements the guard.
- **Partial fade rule** (ExecuteMeshPath / ExecuteOther): when the op's fade differs from 1 the engine temporarily activates the depth-buffer-fill material (`ActivateDepthBufferFillMaterial(true)`), renders the context once (depth only), deactivates it, then renders normally with the shader global alpha (+0x2994 in the shader admin) set to the eased fade, and resets it to 1.0 after. Semi-transparent opaque objects therefore get a depth prefill and avoid sorting artefacts. [V]
- `FillSolidRenderOps` 0x101d8cb0: for each record whose stage byte equals the current LOD stage, whose pool item reports data available (virtual +0x64) and whose optional lightmap item is available, **and only when incoming fade == 1.0 and the record's alpha flag (0x100) is clear**, push an op (context, record index, material from the pool item, entity, flags 0, fade 1.0). `FillAlphaRenderOps` 0x101d8dd0: same availability test, requires record flag 0x200 clear and either fade != 1 or alpha flag 0x100 set; carries the fade (or 1.0 if database flag +0x1aa). `FillRefractedRenderOps` 0x101d8f20: same pattern for refractive records (0x200). `RenderZPass` 0x101d9170 draws z-only for records of the current stage (the +0x68 / +0x69 flags choose primary/secondary depth writers). `RenderRefrationMask` 0x101d92f0 draws a record in z-pass mode into the refraction mask.

### 7.2 Request flags per element (0x101a35c0) [V]

0x01 update LOD; 0x02 solid ops (db lists +0x148 and +0x160); 0x04 alpha ops (db +0x13c and +0x16c); 0x08 z-pass candidate (db +0x154 context list); 0x10 debug context; 0x20 refracted ops (db +0x184); 0x40 stamp frame number; 0x80 refresh illumination. If the element's fade is below 1 (and not 1), 0x04 is forced on and 0x02 / 0x08 removed; when 0x20 is requested the 0x08 bit is dropped.

### 7.3 `eCFrustumDatabase::Execute` order [V sequence, I list identities]

1. Select the render target (application scene target +0xb88 when the current target is the backbuffer), copy the viewport, clear (colour 0xff000000, depth 1.0, +stencil in clear-all mode; flags 7 vs 1). Execute fog.
2. Build the request flags from database booleans and loop over all elements calling `Request`; accumulate statistics.
3. Optional stencil setup (compare 3, ref 1; skipped when db +0x1ab is set), `DoBeginRenderSolid` unless db +0x1ac; render solid list A (db +0x148) with `eCRenderOp2::Render`; module hook; `BeginMaterial` + `ExecuteAllInstances` (flush instanced draws); reapply render states (alpha test / blend / master fogging); render list B (db +0x178: module/legacy ops); flush again; statistics; `DoEndRenderSolid`; `eCDecalAdmin::RenderDecals`.
4. Occluder z-pass: `BeginZPass` (type 4) + `eCOcclusionProcessor::RenderDynamicOccludees` + `EndOcclusionMaterial`; then a depth-sorted list (db +0x160, 0x18-byte ops sorted with a comparator) rendered with `Render`; optionally `eCCameraBase::RenderLowPoly` draws the far low-poly world with a second camera (db camera +0x134); a second z-pass block follows when occluders are used.
5. Refraction capture: copy the colour buffer into the refraction texture and `SetRefractmap(image)` so water / glass can sample it.
6. Alpha: `DoBeginRenderAlpha`; alpha list (db +0x184, then +0x13c via `ExecuteRenderList(list, true)` where the flag sorts back-to-front; comparators `SortInverseZ` 0x1019e360 and `SortMaterial` 0x1019e390); instance flush; `DoEndRenderAlpha`; refracted list (db +0x16c); `SetRefractmap(null)`; `DoPostRender` on modules.
7. Restore original target and viewport if switched. Post-processing (7.7) runs afterwards, outside this function.

### 7.4 Z-pass and refraction mask [V/I]

`SupportsZPass` / `SupportsOccluderZPass` mark contexts that may participate. `eCRenderOp::ExecuteZPass` and `eCRenderContextMesh::RenderZPass` draw through the `XFX_ZPASS` permutation with colour writes off (5.6). The refraction mask re-renders z-pass geometry of refractive contexts so the refractive shader samples only inside the mask. [I]

### 7.5 Occlusion processor [V usage, U numbers]

eCOcclusionProcessor (RenderOccludees 0x101a5260, RenderDynamicOccludees 0x101a4830, RenderShadowMapOccludees 0x101a4dc0, StartOCQuery 0x101a41e0, ResolveOCQuery 0x101a4390, ClearOcclusionList 0x101a40c0, CreateOccludeeGfxResources 0x101a3d70, BeginOcclusionMaterial 0x101a44c0) uses real **D3D9 hardware occlusion queries** from a reuse pool (finished queries are recycled, `CreateOcclusionQuery` only when none are free), drawing box proxies with an occlusion material that forces stencil mode and disables colour writes. `ResolveOCQuery` returns "not ready" (0xffffffff) until the GPU has finished, so visibility lags by at least a frame. Settings: EnableOcclusionCulling, OCCheckThreshold, OCPixelOverdrawThreshold (OnCaps). In the frustum database: `TraverseNode`, `CullFrustum`, `CullCamera`, `PullUpVisiblity`, VPT hierarchy culling (`CullVPTHC`, `CullTreeHC`, option EnableObjectVPTHCCulling [I]).

### 7.6 Iterators [V names]

eCIterator (CollectEntity, CollectEntities, CollectScreenRect, CollectGameContexts, CollectStaticContexts, PickScene) is the spatial query helper (frustum / screen rectangle / ray) used for picking, AI queries and tools, not for the main render path. eCContactIterator belongs to physics. [I]

### 7.7 Post-processing [V names, I behaviour]

- eCPostProcessingAdmin (PlayVideo / StopVideo, ActivateEffect) owns eCPostProcessingEffectMain (Create 0x1019c090 loads three textures - noise, feedback, colour LUT - and builds render targets; Execute 0x1019ba00 chains filters over the scene target; setters SetHDRIGamma, SetHDRIBlueShiftIntensity / Threshold ...) and eCPostProcessingEffectDefault.
- Filters (eCIPImageFilter*): MaskHightLights (bloom threshold, fMultiplier), Blur, DOF (arrPoisson disk, fMaxBlur), EdgeBlur, HDRI (fExposure, fGamma, fVignette, fAberration, fBSThreshold / fBSIntensity blue shift), Feedback (fIntensity, fIntensity2; low-HP hurt overlay), Noise (film grain, fThreshold, fIntensity), FinalBlend; LightmapLight / LightmapBorders are offline.
- ge3.INI defaults [V]: Bloom mode 1 (BloomThreshold 0.59, BloomIntensity 1.70, BloomAberration 2.75), mode 2 HDR (HDRExposure 2.85, HDRGamma 0.60, HDRVignette 0.30, HDRBlueShiftThreshold 4.0, HDRBlueShiftIntensity 1.0, HDRGlowThreshold 0.69, HDRGlowIntensity 3.00, HDRGlowAberration 2.75); Noise threshold 7.00 / intensity 0.15; Feedback HP threshold 0.20, red / green / gray intensity 0.50. The scene pass has no tone mapping (8-bit target) and writes a second target with linear depth for soft particles / DOF (ORIGINAL_SHADING.md).

### 7.8 Rust port implications (section 7)

- Mirror the list structure: opaque (fade 1, non-alpha) -> instanced flush -> decals -> z-prepass / occluders -> optional low-poly far pass -> refraction capture + refracted objects -> alpha (back-to-front) -> post. In Bevy: Opaque3d / AlphaMask3d / Transparent3d phases plus a custom refraction capture. The "fade < 1 means transparent with global alpha" rule and the depth-prefill-then-fade rule for faded opaque meshes must be copied to avoid popping and sorting artefacts.
- Keep per-element request flags (solid / alpha / z-pass / refract / LOD / illumination) so features can be toggled independently.
- Occlusion can be GPU-query based with one-frame lag, or Hi-Z; treat unresolved results as "keep previous visibility".
- Post stack: bloom (threshold 0.59 mask -> blur -> additive) or the HDR chain, DOF, grain, feedback overlay, colour LUT, vignette.

---------------------------------------------------------------------

## 8. Verification ledger

| Claim | Evidence |
|---|---|
| Version names 1_1, 1_4, 2_0, 3_0 | string table at 0x105f9df4 [V] |
| Shader.Cache magic `GENOMESC`, version 0x0175 | `ReadShaderPool` comparisons + the real file header bytes [V] |
| Fog constants x = end/(end-start), y = -1/(end-start) | `OnFogChanged` [V] |
| SH constants 0.2820950 and 0.4886025 | floats at 0x10426c94 / 0x10426c98 [V] |
| Static-light packet = 7 float4, colour x2.0, max 3 lights | `GetDynamicStaticLightData` + double 2.0 at 0x104885f0 [V] |
| LoD thresholds 0.25 / 0.04125 / 0.0078125 / 0.003407 / 0.00095 | data 0x104263d8..0x104263e8, double at 0x10488a30; opcodes are `fld qword` [V] |
| Area factor = pi*(r/z)^2 clamped | `CalculateAreaFactor` [V] |
| Small-object band [0.6 S, S] | double 0.4 at 0x10488760 (opcode dd05) [V] |
| Ranged window = 0.15 x range | double 0.15 at 0x10488570 (opcode dd05) [V] |
| Cosine ease 0.5(1+sin(pi(x-0.5))) | instruction bytes of 0x10039530 [V] |
| ms_fDetailShadowRadius = 2200 | float at 0x105e38bc [V] |
| LsPSM 1408^2, detail ortho 1024^2, clusters 64..512, terrain 512, point cube 512 (max 2) | literals in OnShadowSettingsChanged / UpdateShadowMaps / eCPointLight_PS::UpdateShadowMaps / EnableTerrainShadows [V] |
| Cluster extent cap 1700 | double at 0x10488c18 [V] |
| Vegetation fade start 0.65..0.75 x view range; radius factor 0.45 | floats at 0x10488730 / 0x105ef9ac / 0x10488708 [V] |
| Reflection cube near 1, far 1000 | projection constants +-1.001001 [V arithmetic] |
| Spot shadow near 50, far 6000 | constants referenced in eCShadowMapSpotLight::UpdateCamera [V refs, I roles] |
| Absorbation scale 1/32, 32 taps x 4 directions | double 0.03125 at 0x10488940 + loop structure [V] |

Unknowns worth a second pass: (a) exact LsPSM n_opt formula in `UpdateShadowMatrix`; (b) default half-extent (+0x94) of the 1024 detail map and the terrain map's half-extent (+4); (c) full meaning of the pass-descriptor bit fields and eEShaderMaterialLightmapCombi values; (d) which database list (+0x13c / +0x148 / +0x154 / +0x160 / +0x16c / +0x178 / +0x184) is drawn exactly where; (e) `GetIlluminationAt` CPU sampling; (f) occlusion thresholds; (g) mesh/animation LoD "quality stage" semantics; (h) where the day-time keyframes of the weather states are stored; (i) the formula for derived ambient colours inside `eCWeatherAdmin::Process`.

# Gothic 3 engine reference: animation (part B) and audio (part C)

Source: Ghidra pseudo-C of the original Engine.dll / Game.dll under `.local/research/engine/`, cross-checked with raw constant reads from the shipped DLLs and with the existing runtime FMOD capture in `docs/research/AUDIO_FORMATS.md` section 7. Everything below is paraphrased; no decompiled bodies are copied.

Conventions
* Addresses are virtual addresses in the named DLL (Engine image base 0x10000000, Game likewise; both are mapped at their preferred base, so VA = what is in the index files).
* [V] verified in code (or a constant read straight from the binary), [I] inferred from code shape, names and data, [U] unknown.
* Unnamed Game helpers are cited as `Game FUN_xxxx`; I gave them working names in quotes.
* Pseudo-C caveat: FMOD C++ members and many x87 float paths were lost by the decompiler (argument lists are truncated). Where that matters I recovered the arguments by scanning the machine code for pushed immediates and by reading the referenced constants; those cases say so.
* Struct offsets (`this+0xNN`) are real; field meanings are my naming.

---------------------------------------------------------------------------------------------------

# PART B: ANIMATION

## B1. Object model and who calls whom

| Layer | Class | Role |
| --- | --- | --- |
| Entity property set | `eCVisualAnimation_PS` (Engine, 134 functions, vtable property set type id 100) | Per entity: actor instance, 8 motion slots, body parts, attachments, look-at constraints, ragdoll/physics controller, LoD level, facial animation. `OnProcess` 0x10032d40 drives everything each frame. |
| Assembly | `eCVisualAnimationFactory` (embedded in the property set at +0x94) | Owns the actor instance, the main LoD list and one LoD list per body part. `CreateActorInstance` 0x10039b60, `AddBodyPart` 0x1003a270, `RemoveBodyPart` 0x1003a3b0, `StartFacialAnimation` 0x1003a780, `StopFacialAnimation` 0x1003a8d0, `UpdateRenderMatrices` 0x1003aba0, `RemoveNotUsedBones` 0x1003a4d0. |
| LoD wrapper | `eCVisualAnimationLoD` | One actor file (one LoD level) plus a render hook. `SetActorFileName` 0x1003b680, `GetActorWrapper` 0x1003b5a0, `SetAnimationRenderHook` 0x1003b9d0. |
| EMotionFX wrapper | `eCWrapper_emfx2Actor` (64 functions), `eCWrapper_emfx2Motion` (13), `eCWrapper_emfx2Services` (6) | Thin layer over EMotionFX v2 (statically linked into Engine.dll; EMotionFX classes live at 0x102dxxxx..0x102fxxxx). Handles motion slots, weights, speed, fade-out, import of `.xact`/`.xmot`, hard-skinning buffer creation. |
| Admin | `eCAnimationAdmin` (module GUID {1748AB76-778E-45d1-B8BA-163A2F42A7C6}) | Motion lookup by name, blending table, variation table, missing-animation log, render entry. `eCAnimationCache` is an empty registration shell (Process is a 5-byte stub) [V]. |
| Render | `eCAnimation_HardwareSkinner` (11), `eCAnimation_HardRenderActor` (14), `eCAnimation_BaseRenderActor` (6) | D3D9 hardware skinning per LoD of an actor. |
| Resources | `eCResourceAnimationActor_PS` (`.xact`, optional `.xlac` look-at side file), `eCResourceAnimationMotion_PS` (`.xmot`) | Native extensions `.xact`, `.xmot`; proprietary (import) extensions `.fxa`, `.fxm` [V: string table near 0x42c8bd]. |
| Game | `gCCharacterAnimation` (Game, only a destructor stub at 0x100dcb10 is classified) | In reality the 0x100d94b0..0x100dd870 cluster of unclassified Game functions is the locomotion animation mixer; it lives inside `gCCharacterMovement_PS` at +0x268. `gCCharacterMovement_PS::ProcessMovementAnimation` 0x100e1a50 is its per-frame entry. Script-driven clips go through `gCScriptProcessingUnit` (`sAIPlayAniStart` 0x1016a360, `sAIPlayAniItlLoop` 0x1016d800, `sAICombatMove*`). |

Everything after the one-time load is data-driven: Game never creates blend trees; it feeds clip names and weights into the 8 motion slots of the EMotionFX actor instance and lets EMotionFX evaluate the skeleton.

## B2. The eight motion slots and slot ownership [V unless marked]

`eEMotionType` has eight values and `PlayMotion`/`StopMotion`/`StopAtLoopEnd` reject anything >= 8 (0x10030860, 0x10030980, 0x100309d0). The wrapper stores one EMotionFX motion-instance pointer per slot at wrapper +0x10 + 4*slot.

| Slot | Debug label (`ShowDebug` 0x1002c810) | Used by |
| --- | --- | --- |
| 0 | Primary | main body clip, script clips, idle, turn, first locomotion clip |
| 1 | Secondary | locomotion mix clip 2 |
| 2 | Third | locomotion mix clip 3 |
| 3 | Fourth | locomotion mix clip 4 |
| 4 | Overlay 1 | aim pose (`UpdateAim`), script overlay clips (name contains `_O_`, or the overlay flag) |
| 5 | Overlay 2 | aim up/down pose, second overlay |
| 6 | (no label, nothing found) | [U] probably unused |
| 7 | (facial) | the dynamically built lip-sync motion (`PlayFacialMotion` 0x10030a20 plays into slot 7 and `StopFacialMotion` 0x10030b60 stops slot 7) |

Per slot the property set keeps a 12-byte descriptor (`eSMotionDesc`, array at this+0xe8, count at this+0xec): resource reference, a name string, and an **owner** int (`SetMotionOwner` 0x1002bfa0, `GetMotionOwner` 0x1002bfc0). The owner is a cooperative lock between systems; observed values [I from usage]:
* 2: free / just stopped (script `sAIPlayAniStart` and `UpdateAim` set it after stopping a slot)
* 3, 6, 7, 8: transitional states of the locomotion mixer (8 is immediately rewritten to 3)
* 4: owned by a script animation (`sAIPlayAniStart` writes 4 as its last step)
* 9: idle / stand state of the mixer (`FUN_100d9940`)
* 10: directional locomotion (`FUN_100da480`)
* 11 (0xb): turn-in-place (`FUN_100d9d80`)
Each mixer entry point first reads the owner of slot 0 and decides whether it may take over (fade the previous clip out) or must leave the slot alone.

The state machine in `OnProcess` has two extra string fields (this+0x144 and +0x148, a float at +0x14c and a flag at +0x150): when the remaining time of the slot-0 clip drops below the float, it fades slot 0 out over exactly that time and starts the other clip; otherwise at remaining time 0 it starts the first clip. This is a two-clip alternation helper for props and mover animations [I]; characters do not use it.

## B3. EMotionFX wrapper semantics

### B3.1 Playing a clip
`eCVisualAnimation_PS::PlayMotion(type, desc)` 0x10030860: validates the actor, requires the slot's name string non-empty, resets the primary-slot loop marker (this+0x154 = -1) for slot 0, makes sure the `.xmot` resource is loaded (waits if streaming, forces a load otherwise through the main cache) and calls `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0. The wrapper stops any previous instance in the same slot with zero fade (instant), then asks EMotionFX to play the new motion instance and stores it. For slot 0 it also remembers the playback mode.

The descriptor passed from callers is a seven-dword struct; all call sites in Game fill it consistently, so the layout is [V by usage, names I]:

| Offset | Meaning | Values seen |
| --- | --- | --- |
| +0x00 | fade-in time in seconds | 0 almost everywhere; `sAIPlayAniStart` writes 0.1 s (Game const 0x102baeec) for the `_Begin_` overlay case [I] |
| +0x04 | play mode | 0 forward, 1 backward, 2 ping-pong (the wrapper builds a different EMotionFX playback object per value; RunStopPrevention treats value 1 as backward) |
| +0x08 | play speed multiplier | 1.0 default, script supplies the animation speed factor |
| +0x0c | max loops | -1 infinite, 1 once; the script path also uses 2 [U why] |
| +0x10 | target weight | 1.0, or the aim blend weight |
| +0x14 | (unused) | 0 |
| +0x18 | (flag) | 1 |

The facial call passes {0, 0, 1.0, 1, 1.0, 0, 1}: forward, once, full weight.

### B3.2 Instance fields the wrapper manipulates [V offsets, I names]
EMotionFX motion-instance block: +0x08 current play time (double); +0x20 play speed (float); +0x24 current weight; +0x28 target weight; +0x2c weight change rate per second; +0x34 current loop index; +0x38 max loops (-1 = infinite); +0x40 internal state (set to 5 by SwitchFadeOut); +0x4c mix flag (additive-like blend, set by `SetBlendMode`); +0x54 paused; +0x56 "fading out, remove when weight reaches 0". The motion duration is read through the instance's motion at +0x14 of the motion block (float seconds).

Rules:
* `SetTargetWeight` 0x10049ca0 clamps to [0,1] and sets **both** current and target weight and zero rate: it is an instant weight change, not a fade [V]. The mixers call it with 0 together with StopMotion, and with 1.0 to re-assert an owned clip.
* `StopMotion(type, fade)` 0x10047910 forwards `(instance, fade)` to the actor instance's motion system and clears the slot pointer; fade 0 removes the instance immediately. The wrapper keeps a flag (wrapper +0x78) meaning "a motion was just (re)started, run the stop-prevention test" [I].
* `StopAtLoopEnd` 0x100479c0 sets max loops to current loop index + 1 when the clip is currently running (not paused, not already finished), so it plays to the end of the current cycle and stops [V]. `sAIPlayAniItlLoop` uses it for non-looping or last repetitions.
* `PauseMotion` 0x10047980 only writes the paused flag.
* `IsActorPlaying(slot)` 0x1004a040 is "instance exists, is not past its max loops and is not paused". `IsMotionRunning(slot)` 0x1004a090 additionally asks EMotionFX whether the instance is still registered. `HasMotionInstance` 0x1004a0d0 clears the stored pointer if EMotionFX has dropped the instance.
* `SetBlendMode` 0x10049d10: only mode value 2 sets the mix flag (additive-like); anything else clears it. `GetBlendMode` returns 2 when the flag is set, else 1 (0 when no instance).
* `GetPlayTime/SetPlayTime/GetPlaySpeed/SetPlaySpeed/GetMaxTime` are direct reads/writes of the fields above (0x10049ff0, 0x10049fd0, 0x10049fb0, 0x10049f80, 0x1004a010).

### B3.3 SwitchFadeOut (the cross-fade primitive) 0x10049dd0
Called as (slot, flag, seconds). If the slot is playing, the wrapper moves the instance pointer into a dedicated "fading" pointer (wrapper +0x28) and clears the slot, so a new clip can be started in the same slot at once. The old instance gets target weight 0 and: with seconds <= 0 the weight is set to 0 immediately; otherwise rate = -(current target weight) / seconds and the removal flag is set. In addition, if the clip has a finite remaining time shorter than the fade (remaining / play speed < fade), the instance's play speed is overwritten with (time left / fade) so the clip is slowed to last about as long as the fade (exact for an old speed of 1.0) [V]. Only one fading instance is tracked at a time [V]; a second SwitchFadeOut before the first one completes loses the pointer (the older instance is then not tracked by the wrapper any more) [I].
The fade-in of the new clip comes from its descriptor (+0x00) or is a hard cut; there is no automatic fade-in in the wrapper [I]. In practice the visual cross-fade is the old clip fading out while the new clip is already at full weight, with the two weights summing above 1; EMotionFX normalises by blend mode [U].

### B3.4 Stop prevention 0x10047600 and the per-frame update 0x10047a00
`Process(worldMatrix, updateAll, skipMotionUpdate, timeScale)`:
1. Frame delta = application scaled frame time (eCApplication +0x5c0) times the per-entity time scale (property set +0x188, set by `SetLocalTimeScale` 0x100301b0, which also forwards the scale to the physics controllers) [V]. The local time scale is therefore the only animation-speed knob outside the clip play speeds.
2. If the "just started" flag is set and neither slot 0 nor slot 4 is playing, the delta is forced to 0 (the pose freezes, nothing blends) [V]; otherwise `RunStopPrevention(delta)` runs.
3. Stop prevention: when slot 0 is in its final loop (max loops == loop index + 1), has a non-zero play speed and the time to the end (remaining/speed) is smaller than this frame's delta, the play time is clamped to duration minus epsilon (or to epsilon for backward playback) and the play speed is set to 0. This holds the last pose instead of wrapping to the first frame or popping when the instance is removed [V].
4. Unless `skipMotionUpdate`, the actor's motion manager is updated by delta; if `updateAll` the transforms are updated/propagated (FUN_10046580/100465d0/102de770/102de8f0) and the bounding box is refreshed (copied from the instance at +0x1d8 into wrapper +0x30).
`OnProcess` passes `updateAll` only when the entity is inside the processing range or a ragdoll/attachment forces it, and `skipMotionUpdate` when the ragdoll owns the skeleton; so off-range characters do not evaluate animation at all [V from the gate in OnProcess].

## B4. Fade lengths, G3_Ani_Blendings.txt and variations (eCAnimationAdmin)

* **Standard fade time** = `eCAnimationAdmin` +0x58 = **0.2 s** [V: initial value read from the binary at 0x10426ddc]. All mixers use it for takeovers; `sAIPlayAniStart` uses it via the table below. This equals the 0.2 s cross-fade already in the Rust `Animator`.
* `Initialize` 0x10020a20 parses `G3_Ani_Blendings.txt` (file in the archive; it may be a GENOMFLE-wrapped or a plain text file; plain text is read in full, limit 512 MB). The text must contain `*Version 1`; each entry starts with `*Entry->` followed by a clip-name key (the part of the name before the first `%`, upper-cased) and **two integers**, stored as bytes: intro blend and extro blend. The tables are three hash maps of 0x26f5 (9973) slots each (blend map, and two byte maps) [V].
* `GetAniIntroBlending` 0x100213a0 / `GetAniExtroBlending` 0x10021450: cut the clip name at the first `%`, upper-case, look it up; result = stored byte / **15.0** seconds [V: the divisor is the double 15.0 at 0x10488808], i.e. the table is expressed in 1/15 s frames. Missing key: the standard 0.2 s.
* `SetAniVariationMax` 0x100211c0: on first sight of a clip key (name up to `%`), read underscore-separated word index 11 of the file name as an integer and store it as that key's variation maximum.
* `GetAniVariation` 0x10021290: if the key has a stored maximum m, it writes a random two-digit number in [0, m] into the two characters before the trailing underscore of the key (positions len-3 and len-2) using `rand()`. The Rust equivalent: pick `rand % (m+1)`, format `%02d`.
* The wrapper stores intro/extro values per actor (wrapper +0x6c/+0x70) through `SetIntroExtroBlending(slot, name)` 0x10049d80 (name form, via the admin) or 0x10049d50 (float form). Only Game reads them back (`GetIntroBlending` 0x10049db0 / `GetExtroBlending` 0x10049dc0), to compute the fade it passes to `SwitchFadeOut`; the engine itself never applies them [V: no other readers].
* Fade chosen when a new script clip replaces an old one (`sAIPlayAniStart`): fade = extro of the **old** clip; if the new clip name does not contain `_Begin_` the fade becomes max(old extro, new intro); for a `_Begin_` clip it is the constant 0.1 s (Game 0x102baeec) [V logic, I on the semantics of the constant].
* Missing clips are logged once through `AddMissingAnimation` 0x10021580 / `IsAnimationMissed` 0x100215b0, listed at shutdown (`PreShutdown` 0x10020ef0), and never retried.
* `QueryMotionDataEntity` 0x10021070: takes the file stem of the requested name (path stripped via the archive manager), appends the native extension `.xmot`, and asks the resource admin for that data entity with the given cache behaviour. The archive's file path cache keys names **up to the first `%`** (`eCFilePathCache::InsertFileName` 0x10299570 allows several files to share the prefix and reports duplicates only when no `%` is present) [V]. This is why real files are named like `Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_160` while code asks for `..._Fwd_00_` + `.xmot`.

### Meaning of the part after the `%` [V/I]
`<name-before-%>%_<VV>_<Pn>_<S>[_R]`
* `VV`: two-digit variation maximum (word 11, see above).
* `Pn`: pose again (P0, P1, ...).
* `S`: **nominal ground speed in cm/s** of the clip (word 13). `Game FUN_100dade0` parses it with find-%, then skips three underscores and reads a float. Locomotion clips carry real speeds (Walk 160, Run 400, Sprint 600, side steps 200/300, 350 for parade walk ...) and non-locomotion clips carry 0 [V from hero-motions list and code]. Optional trailing `_R`: [U] (59 + 49 hero clips; possibly "root-free").

## B5. Motion name grammar used by Game [V]

Two builders exist.

1. Script/AI names (`gCScriptProcessingUnit::GetAniName` 0x1016f840):
 `<ActorWord1>_<AniState>_<LeftUse>_<RightUse>_<Pose>_<Action>_<Phase>_<N|O>_<Dir>_<NN>_`
 where ActorWord1 is the second underscore-separated word of the actor file name (for `G3_Hero_Body_Player` that is `Hero`), AniState comes from the table `g_pstrAniStateString` indexed by the entity's AniState property (the enum names: Stand, Sneak, Parade, Kneel, SitGround, SitStool, SitBench, SitThrone, SleepBed, SleepGround, TiltOrcBoulder, HoldOrcBoulder, LiftOrcBoulder, SitKnockDown, LieKnockDown, LieKnockOut, LieStraightDead, LieDead, LiePiercedKO, LiePiercedDead, TalkStand, TalkSit*, Wade, Swim, ...), the two use fields are the use-type names of the items in inventory slots 2 and 1 (indexed through `g_ppszUseTypes`, 0x39 entries; value >0x38 or missing slot -> `None`), Pose is `P0..` from the NPC's primary pose (`gCNPC_PS::GetPrimaryPoseExt`; `P0` if the entity has no NPC set), Action and Phase come from the script call (action `g_pstrActionString`), `N` normal or `O` overlay/off-hand, Dir is Fwd/Back/Left/Right, the numeric field is the variation (initially `00`).
2. Locomotion lookup (`Game FUN_100d94b0`, "FindMotion"): `<Prefix>_<AniState>_<Use1>_<Use2>_<Pose>_<Action>_<Phase>_N_<Dir>_<NN>_` + ext with prefix from the per-actor cache described next, plus an optional "ambient action" variant (`g_pstrAmbientActionString`) instead of the action word when the ambient action index != 2. The resolved resource is cached in a small array per (actor prefix, argument tuple, variation); when the variation argument is -1 the cache picks `rand()` over the cached count.

* **Per-actor prefix cache** (`Game FUN_100da260`): word 1 of the visual's file name (`Hero`, `Orc`, ...) is the prefix; if the NPC's gender/race int (gCNPC_PS +0x148) is 1 the prefix becomes `Babe`, if 2 `Zombie` [V]. The pair (original word, substituted word) keys a global list (DAT_103f46d8/103f46dc) of cache objects reference-counted per user. A special case in `FindMotion`: for (state Stand, action Move, phase 8 or 12, pose P1 with no use items) the **original** word is used instead of the substituted one [V], so Babe/Zombie share the base move clips in that case.
* **Phase indices used by the locomotion code** [V numerically, I for names]: action 0x38 = `Move`; phase 8 = `Walk`, 9 = `Run`, 10 = `Sprint`, 0xc = `Stand` (the zero-speed clip used for speeds below walk speed), 0xf = `Turn90`. Direction codes passed in: 1 Fwd, 2 Back, 3 Left, 4 Right (0..3 in the string builder: Fwd/Back/Left/Right). Other phases that exist in the data: Begin, Loop, End, Hit, Recover, Raise, PowerRaise, Aim, Ambient, plus talk gestures (Angry, Sad, Nervous, Preach, ...).
* Fallbacks when a script clip is missing (`sAIPlayAniStart`): (1) try the randomised variation, (2) the same name with variation `00`, (3) replace the second word group (the left-hand use type, the segment after the second underscore) by `None`. After that the call fails.
* The "I" (intro) form: for a script clip that is `_N_`, a mirrored `_I_` clip is queued at the same time as the primary with the `N`->`I` swap and the primary slot is stopped; used for entering a stance [I].

## B6. Locomotion mixer (Game, hosted in gCCharacterMovement_PS) [V unless marked]

Entry: `gCCharacterMovement_PS::ProcessMovementAnimation` 0x100e1a50.
1. `CalcAniVelocity` 0x100e1cd0 produces a smoothed local velocity (this+0x14c): when the movement smoothing factor (this+0x78) is zero it takes the raw frame displacement / frame time; otherwise it blends toward it with a damped formula (constants 0.025, 4.0, 100000.0 near Game 0x103080a0..b0). The local time scale of the rigid body divides it.
2. If the movement mode is one of the walking-like modes (modes 2,3,4,5,7,10 in `gCCharacterMovement_PS`) and the horizontal speed >= a floor (Game double at 0x10308118 = 0.01), it calls the **directional mixer** `FUN_100da480(visual, velocity, facing)`.
3. Else if there is a turn request (this+0x15c nonzero or turning by more than |10| deg/s -- constant Game 0x10308188 = 10.0) and the slot-0 owner is not 11, or turning in place: **turn mixer** `FUN_100d9d80(rate)`.
4. Else: **idle mixer** `FUN_100d9940`.

### B6.1 Idle/stand mixer FUN_100d9940
If a ragdoll is active nothing happens. Decide by the slot-0 owner: owner 2 with a running clip at non-zero speed -> leave it; owners 3/10/11 -> take over; owner 9 (already idle) -> keep asserting weight 1.0 and re-roll when the clip is within the standard fade (0.2 s) of its end *and* longer than 2*0.2+0.1 = 0.5 s, or when the hand-item combination changed (`FUN_100dabb0` compares the use-type ids of the left/right item slots with the cached ones). Takeover: fade the old slot-0 clip out (0.5 s when it was not an idle clip -- constant Game 0x102bc518 = 0.5; the standard 0.2 s when it was), stop slots 0..3 with weight 0, set owner 9, then look up `Action=Ambient (0x7a), Phase=Loop (0xc), pose=P?` clip (`FUN_100dac70(0x7a, 0xc, variant 5, 9, -1, ...)`); on a 1-in-7 `rand()%7==6` roll (only when the "no-variation" flag is clear) variant 0x10 (an alternate idle) is tried first. The chosen clip is also written to the NPC property at +0x1ac (current animation name for scripts), `SetMotion`, `SetIntroExtroBlending`, and played looped (max loops -1, speed 1.0, weight 1.0).

### B6.2 Directional locomotion FUN_100da480 (the core)
Inputs: desired local velocity vector and facing vector. Steps:
1. Normalise both on the horizontal plane. cos(theta) = dot(velocity dir, facing dir) clamped to [-1,1]; lateral share = sqrt(1 - cos^2). The horizontal speed is `GetMagnitude2D(velocity)`.
2. **Hysteresis**: when the slot-0 owner is already 10, the clip is still running and the new direction differs from the stored direction (this+0x24) by less than about 50 degrees (dot >= 0.6427 = cos 50 deg; Game double 0x10308548) and the equipment key (state + hand items) is unchanged, the mixer keeps the clips and only updates weights/speeds. Otherwise the old slot-0 clip is faded out over the standard 0.2 s and the mixer restarts [V].
3. Per axis (forward/back with sign of cos; left/right by the cross-product sign) it computes the axis speed (|cos| * speed and |sin| * speed) and calls `FUN_100dae20` (forward axis) / `FUN_100db250` (lateral axis) to bracket it with clips from the data (all pose-aware): 
 * speed below the **Walk** clip's nominal speed: blend **Stand** (speed 0, `Move_..._Stand`) with Walk, weights (1-t) and t, t = speed / walkSpeed;
 * between Walk and **Run**: linear blend, t = (v - vWalk)/(vRun - vWalk), clamped [0,1];
 * between Run and **Sprint**: same;
 * above the fastest available clip: that clip alone at play-rate ratio v / vClip (so a clip can be sped up arbitrarily);
 * when only Walk exists and v > vWalk: Walk only with ratio v / vWalk.
 The nominal speeds come from the file names (B4). A missing Run or Sprint clip just truncates the ladder. The lateral axis uses side-step clips (Left 200/300, Right) with the same ladder.
4. `FUN_100db690` merges both axes into a list of up to four entries (24 bytes each: weight, resource, rate ratio, vx, vy, normalised weight); the axis shares come from cos^2 / sin^2 [I on the exact merge], so four clips (slots 0..3) is the hard cap.
5. `FUN_100dc1d0` applies the list: per slot, if the slot already holds the same clip only the weight changes (`SetTargetWeight`); otherwise stop the old clip, `SetMotion`, play it looping (-1) with the weight in the descriptor, and **phase-sync**: the new clip's play time is set to (clip duration / previous total duration) * previous play time, so a change of clip keeps the foot cycle. Slot 0 also writes the clip name into the NPC (+0x1ac) and sets intro/extro. Slots beyond the list are stopped and given owner 2.
6. Play rates: each clip's rate = (duration_i * ratio_i / sum_j(w_j * duration_j * ratio_j)... ) * (desired ground speed / blended nominal velocity magnitude) [V structure]: every active clip advances the same fraction of its cycle per second (time-normalised blend), and the common rate is scaled so that the weighted mean of the clips' nominal planar velocity (vx, vy vectors per clip) equals the actual speed. Therefore walking at 120 cm/s with a 160 cm/s clip plays it at 0.75 [I from the shipped data note "played at speed/nominal rate"].
7. Footsteps: `FUN_100dc5d0` watches the normalised phase of slot 0 (play time / duration). Phase above **0.6** (double at Game 0x10308308) sets a step flag +1, phase between **0.1** (0x10308130) and 0.6 sets -1; on a change of the flag it fires the left/right step event for the gait found in the clip name (`_Walk_`, `_Run_`, `_Sprint_`; helpers 0x100dd6c0 / 0x100dd730 / 0x100dd7a0 call `FUN_100dd0a0/dd1e0/dd320`, which build effect names). Effect name: `EFF_Step_<Human|Orc|Default>_<Walk|Run|Sprint>_<Left|Right>_<Surface>_01` with race `Human` for Hero/Slave/Fat, `Orc` for Orc, `Default` otherwise, and the surface from `FUN_100dd870` indexed by the ground material enum: 1 Wood, 2 Metal, 3 Water, 4 Stone, 5 Earth, 6 Ice, 7 Leather, 8 Clay, 9 Glass, 10 Flesh, 11 Snow, 12 Debris, 13 Foliage, 14 Magic, 15 Grass, 19 Damage, 20 Sand; default by climate (session climate 1 -> Snow, 2 -> Sand, else Earth) [V].

### B6.3 Turn in place FUN_100d9d80
Looks up `Move` action, phase 0xf (`Turn90`), direction Left/Right by the sign of the turn rate, plays slot 0 looping and sets the play speed to (turn rate converted to degrees by `FUN_10039c70`) / (90 / clipDuration), i.e. the clip is assumed to rotate **90 degrees** over its duration (constant Game double 0x103081d0 = 90.0) [V constant, I meaning]. It stops slots 1..3. Takeover timing: from owner 9 it waits 1000*fadeTime*... (`fade * 1000` ms elapsed, Game 0x10308038 = 1000.0) before starting.

### B6.4 Footprint: speed and root motion
Positions come from the movement system, **not from the clips** [V]: `gCCharacterMovement_PS` integrates velocity, `ProcessControledRotation` 0x100e3000 rotates, and animation play rates are scaled to match. There is no extraction of root translation from the clip (see B13).

## B7. Script-driven animation (sAIPlayAni*)
`sAIPlayAniStart` 0x1016a360 resolves the clip name through GetAniName (B5), `GetAniVariation`, `.xmot`, the three fallbacks, stores the clip name in the NPC property +0x1ac (primary slot only), calls `SetAniVariationMax`, stops slots 1..3 (and slot 0 with `SwitchFadeOut` of the chosen duration) when the primary slot is targeted, sets the slot's clip and fills the descriptor: play mode = 1 (backward) when the AI instruction flag at +0x10 is set, speed from the instruction (+0x14), max loops 2 when the instruction's +0xc is 0 and -1 otherwise, weight 1.0, then `PlayMotion` and `SetMotionOwner(slot, 4)`. `sAIPlayAniItlLoop` 0x1016d800 runs each frame while the clip plays: it computes the remaining time (remaining / play speed) and, depending on loop/stop mode, calls `StopAtLoopEnd` and ends the instruction when the remaining time drops below the standard fade (0.2 s) or the scaled frame time. Combat move instructions (`sAICombatMoveStart` 0x1016abb0, `...StartRecover` 0x1016e360, `...ItlLoop` 0x1016dd00) use the same naming and blend rules with weapon-specific speed multipliers computed in script (see `COMBAT_AI_RULES.md` 9.5 for the multiplier).

## B8. Aim overlay (UpdateAim 0x10030230)
Runs from `OnProcess` when the aim flag (this+0x17b) is set. It calls the script function `OnGetAimAngle` in `gCScriptAdmin`, which returns an angle; the pitch is mapped through a clamped arcsin-like function (angle in [-pi/2, pi/2]) into this+0x180. Fraction f = |pitch| / (pi/2). The clip names are derived from the current primary clip name (this+0x184): first 8 underscore-separated words + `Fwd_00_` (centre pose) and + `Up_00_` or `Down_00_` (sign of pitch). Slot 4 gets the Fwd pose with weight 1 - f (looping, max loops -1), slot 5 gets the Up/Down pose with weight 1.0 but only when f > 0. Both slots are given owner 2 afterwards. It is skipped while a slot is still finishing (`remaining time > frame time`). [V flow, I weights].

## B9. Properties and physics (summary)
The property set exposes (all `Set*` setters 0x1000a2b0..0x1000a8c0): `ResourceFilePath`, `FacialAnimFilePath`, `MaterialSwitch`, `PhysicsControled`, `ConstrainRootToWorld`, `InterBoneCollision`, `SkeletonCollisionEnabled`, `BoneRaycastsEnabled`, `BonesAreBreakable`, `BonesBreakMaxForce/Torque`, `BoneLinearDamping`, `BoneAngularDamping`, swing/twist/joint spring (+enabled, +damping), `BoneProjectionDistance`, `SpringParentNodesOnly/Scale`, `CreateSpringMassCloth`, `DisablePhysicLodding`, `BoneShapeGroup/Material`, `SkeletonShapeGroup`, `ClothEffector`. `OnNotifyPropertyValueChangedExitEx` 0x10034640 rebuilds the actor instance when `ResourceFilePath` changes and swaps the facial actor when `FacialAnimFilePath` changes.
* Ragdoll: `SetRagDollEnabled(enable, fade, flag)` 0x100323f0 requires entity flag 0x80 (physics-capable) and builds an NxActor-based controller (`CreatePhysicsController` 0x10031170) from the skeleton's `_PHX_PARENT` bone sub-skeletons (`CountSubSkeletons` 0x10032670). Disabling with a fade > 0 keeps the controller and fades it out over the time (time scale shrinking), with 0 destroys it and recreates the idle controller. `StopRagDoll` 0x10034550.
* `ProcessPhysicsLOD` 0x10031d40: a physics controller is only kept for entities within `eCAnimationAdmin` physic-controller distance (+0x5c = **2000 cm** [V initial value 0x104885b4]) minus a 250 cm hysteresis (double 0x10488bf8). While in ragdoll a timer (this+0x130) accumulates scaled frame time; the code compares it with 2.0 s and 15.0 s (double 0x10488808 is shared with the blend-table divisor), applies a decaying local time scale (1 - t/5 s) to the physics controllers and calls `StopRagDoll` once the fade-out counter drops under 0.1 [I; the exact state machine is [U]].
* Bone matrices for skinned meshes: `GetBoneMatrix` 0x1002c5c0, 0x1002c6a0; bone raycasts via `IntersectPhysicsRayNearestHit` 0x10032a00.
* Frame effects: `UpdateFrameEffects` 0x1002eff0 (4.5 KB) spawns effects on bones during animation (chest/basket handling for containers via the script `IsBasket`) [I].

## B10. Attachments and look-at constraints
* Attachments (`eCActorAttachment` 0x1002a3b0..): (entity GUID or proxy, bone name, offset matrix, "once" flag). `Process` 0x1002a5b0 resolves the bone node, multiplies the node's world matrix with the offset and the actor world matrix and writes the entity's world matrix; warns "Bone not found" / "Entity ... not valid" otherwise. `ProcessOnce` 0x1002a820 for one-shot attachments, `AddAttachment` 0x10030da0, `AddOnceAttachment` 0x10030f50, `RelinkAttachments` 0x10031110. In `OnProcess` the once-list is flushed first, the persistent list is processed every frame, and an attachment whose property id became invalid is removed.
* Look-at: `eCLookAtConstraint::SetTargetPos` 0x1002acd0 lazily creates a `LookAtConstraintEx` (EMotionFX LookAtConstraint, controller 0x102e2460, 0x110-byte object) for a named node with min/max angle limits and an interpolation speed taken from the actor's `.xlac` side file (`ImportLookAtConstraints` 0x100228c0: keys `InterpolationSpeed`, `MinConstraints`, `MaxConstraints`, default limits (-360,-360,0) .. (360,360,0)). `OnProcess` converts the target (focus entity position, or a named bone of the target, via `FUN_1002a2d0`) into the owner's local space with the inverse world matrix and feeds all constraints. `SetLookAtTarget` 0x10032250/0x10032210 selects target entity/bone; `Enable` 0x1002afa0 toggles.

## B11. Actor assembly and LoD
* `eCResourceAnimationActor_PS::DoLoad` 0x10022dd0 (format v >= 2 only; "v1.0 is no longer supported (no LoD-support)"), `DoSave` 0x10023a90 writes `%s_LOD%i%s` per LoD; `GetDependencies` 0x10024490 lists `*.xmot` motions that belong to the actor. The wrapper imports through `ImportEMFX2Actor` 0x1004ad20 and `CleanUpHierachy` 0x1004bfa0 (renames/relinks to a `_ROOT` hierarchy).
* `CreateActorInstance` 0x10039b60: clones the main LoD actor into an instance, then for every body part LoD creates an instance whose skeleton is merged into the main instance (`FUN_1003b070`/`FUN_102e4b00`/`FUN_102dffa0`: link parts by node name), invalidates the matrix cache and looks up the root node named by a configured string to set the instance's "root node" index (+0x208).
* Body parts (`eCVisualAnimation_PS::AddBodyPart` 0x1002c3c0, factory 0x1003a270) store a part name -> (normal LoD, optional second LoD for the alternative/hard variant) pair and merge the new part into the live instance immediately. `RemoveNotUsedBones` 0x1003a4d0 re-parents/removes nodes that no remaining LoD refers to (compares by node name id at +0x188, stride 400 bytes/node). `SetBodyPartVisible` 0x1002c5a0 toggles per-part rendering (render list flag at +0xc of each 16-byte part record).
* **LoD level selection** `UpdateLoD` 0x1002c780 with the entity's screen-size factor f: level 0 if f > **0.25**; level 1 if 0.04125 < f <= 0.25; level 2 if 0.003407 < f <= 0.04125; level 3 if 0.001903 < f <= 0.003407 (table at Engine 0x10426df8..0x10426e08; anything else falls back to 0, so the table is bounded below at 0). The level is stored in a 16-bit field (+0x19c) and clamped to LoD count-1 at render time [V]. The user option `ObjectDetails.enuAnimationLoDQualityStage_*` and the global LoD factor scale f upstream [I].
* Material switches (known) go through `eCVisualAnimationLoD::SetMaterialSwitch` as noted by the lead.

## B12. Skinning and rendering [V]
* Data: `FillHardBuffersAndPrimitves` 0x10048860 builds, per LoD, one vertex buffer and one index buffer plus a list of "hard primitives" (24 bytes: start/count/material/bone-palette range). **Vertex format is 68 bytes**: position (12), four bone weights (4 floats, 16 bytes), four bone indices (4 bytes), normal (12), vertex colour (default `0xff000000` when the mesh has no colour layer), UV0 (8), tangent (12) [V by field accounting]. Influences are sorted by weight, **only the 4 strongest are kept** and renormalised by their sum (`if more than 4`) [V]. The bone indices written are local indices into a per-primitive palette (`submesh bone list`), taken from each submesh's node ids.
* Index buffer format code 0x66 (a D3DFMT_INDEX32-class value) [I].
* Runtime: `eCAnimationAdmin::Render` 0x100215f0 -> `eCAnimation_HardwareSkinner::Render` 0x10043f30 -> `InsertOrGetActor` 0x10044210 (creates/caches the `eCAnimation_HardRenderActor` per LoD resource) -> `Render` 0x100428f0. For the current LoD it binds the vertex/index buffers, sets the world matrix to identity, and for each primitive uploads the **bone palette**: for every palette entry it writes the 4x4 skinning matrix `skinMatrix[nodeIndex]` (64 bytes) into the shader admin's matrix array (shader admin +0x474, count +0x480). The skin matrices are refreshed at most once per frame by `eCVisualAnimationFactory::UpdateRenderMatrices` 0x1003aba0 (compares the frame id in `eCAnimationAdmin+0x68`), which copies 64 bytes from every node record (+0x84 within a 400-byte record) of the instance. Material must have transformation type "skinned" (2) in `eCShaderDefault`; otherwise `RenderPrimitive` 0x10042d60 warns and skips. The shader pass loop is `Begin(2 = skinned, illumination, terrain shadow map)`, per pass `SetPass`, then `eCGfxVisual::Execute`.
* Render passes: when the entity's shadow flag is on and not in the terrain shadow pass, the admin first renders the actor with colour writes disabled (a depth/shadow prepass), then renders normally. Two predicates split the primitives into the opaque and the blended group (`cVar5 == 0/1` against the master blend state).
* Palette size cap: the palette per primitive is whatever EMotionFX submesh splitting produced; I found no hard-coded maximum in Engine.dll [U]. The Rust skinned-mesh path (4 influences, shared joint entities) is equivalent in output.
* Software skinning exists too (`FillSoftBuffers` 0x1004a5e0, `UpdateDynamicSoftBuffer` 0x10047b00) for dynamic meshes (cloth/ragdoll helpers) [I].

## B13. Root motion [I]
No code path was found that reads root translation/rotation deltas out of a motion and applies them to the entity: `Actor::Process` receives the entity world matrix and the transform of the root is written back from it; `ConstrainRootToWorld` pins the root to the entity. The movement system plus the speed-normalised play rates (B6) replace root motion, and the data confirms clips carry nominal speeds in their names. For attack/hit clips the root offset is part of the pose (the entity moves by script `Teleport`/movement commands) [I]. The Rust viewer's `--root-motion` flag should stay off by default.

## B14. Facial animation and lip-sync [V unless marked]
* Two actors: the main body actor (+ head part) and an optional **facial actor** (`FacialAnimFilePath`, `SetFacialActorFileName` 0x1003aa80). `StartFacialAnimation` 0x1003a780 creates instances of the facial LoDs and merges them into the body instance; `StopFacialAnimation` 0x1003a8d0 removes them (`RemoveNotUsedBones`) and clears the render hooks. A flag at factory +0x1c says whether the face is currently attached.
* `PlayFacialMotion(motion resource, lipsync)` 0x10030a20: start the facial animation if not yet started, relink attachments and look-at constraints (the head skeleton changed), stop any previous facial motion in slot 7, allocate a new `eCWrapper_emfx2Motion` and call `CreateLipSyncMotion`; on failure stop the facial animation again, otherwise play the new motion in slot 7 forward, once, weight 1.
* **Lip-sync data**: `eCVisualAnimationLipSync` (Read 0x10027690) = u16 version (1), u32 count, then `count` 8-byte pairs {float seconds, u32 viseme index} -- the `.xlip` file; the same layout as documented in AUDIO_FORMATS section 2.
* **Phoneme table** (Engine 0x105e7d68, 40 entries of {label, viseme}), used by `FindePhoneByLabelString` 0x10027650 (case-insensitive; unknown label -> viseme 1, the rest/`x` entry): x 1, IY 5, IH 10, EH 11, AE 10, AH 11, UW 9, UH 9, AA 10, AO 11, EY 11, AY 11, OY 13, AW 9, OW 13, l 12, r 8, y 5, w 9, ER 8, m 1, n 7, NG 4, CH 3, j 3, DH 7, b 1, d 7, g 4, p 1, t 7, k 4, z 6, ZH 3, v 2, f 2, TH 7, s 6, SH 3, h 4. So there are **14 visemes (0..13)**; the shipped `.xlip` files only reach 12, so 13 (OY/OW) is probably never produced offline [I].
* `eCWrapper_emfx2Motion::CreateLipSyncMotion` 0x10052cb0: takes the facial **"_LipSync" source motion** (a motion whose node tracks contain one key per viseme: any node whose position, rotation or third (morph-like) key list has at least 14 keys is treated as a viseme track) and creates a new motion in which each such track gets one key **per lip-sync entry**: key time = the entry's time, key value = the source track's key number `viseme` (position keys are copied as three floats; rotation keys are re-encoded as 16-bit quaternion components). The result is an ordinary EMotionFX motion played once; between keys EMotionFX interpolates (linear/slerp), so visemes blend over the gap between consecutive entries and the mouth holds the last viseme until the clip ends. Tracks that are not viseme tracks (< 14 keys) are not copied (the face expression comes only from the visemes) [I]. The built motion lives in `eCVisualAnimation_PS` +0x198 and is released on stop.
* Who calls `PlayFacialMotion` is not in Game.dll [U]: it is invoked from the script/dialog layer (Script.dll, not decompiled) when a dialog line starts; the `.xlip` path is delivered with the audio start message in the Rust plugin.

## B15. Cache and misc
* `OnEnterProcessingRange` 0x10034450 / `OnExitProcessingRange` 0x10034600 attach/detach physics and can start ragdoll; `OnCacheIn` 0x10033730 / `OnCacheOut` 0x10033790 acquire/release the actor resources (and call `StopFacialMotion`); `GameReset` 0x1002b700 stops all slots; `Invalidate`/`Destroy`.
* `GetStatus` 0x10033e10 and `GetOnScreenDebugLines` 0x10033b20 print per slot "[progress %] (weight %) clipname" for the debug overlay (labels Anim #1..#4, Over #1, Over #2).

## B16. Rust port implications (animation)
* Keep the 8-slot model per character: slots 0..3 locomotion/primary, 4..5 overlay/aim, 7 facial. An `Owner` int per slot with the same takeover rules prevents the script system and the locomotion mixer from fighting.
* Implement the **name grammar** and the `%`-prefix lookup exactly: key = name up to the first `%`, then nominal speed from word 13, variation maximum from word 11. Build an index once (`HashMap<Key, Vec<ClipFile>>`) and resolve `<Prefix>_<State>_<Use1>_<Use2>_<Pose>_<Action>_<Phase>_N_<Dir>_<NN>_` with the three-step fallback (random variation, variation 00, `None` for the second use word).
* Blend rules to port: standard fade 0.2 s; blend table entries are bytes / 15 s; fade-out = old clip weight ramps to 0 linearly over the fade while its play speed is reduced so it ends with the fade; new clip normally starts at full weight (or with a descriptor fade-in); `SetTargetWeight` is instant; one-shot clips freeze on their last pose (stop prevention) instead of looping.
* Time scaling: dt = frame time * game time scale * per-entity local time scale; skip animation entirely for out-of-range entities.
* Locomotion: reproduce the ladder Stand -> Walk -> Run -> Sprint per axis with linear weights between nominal speeds, extrapolate by play-rate ratio above the last clip, 50 degree direction hysteresis, phase-synced clip swaps, normalised-cycle play rates (all active clips share the cycle rate), up to 4 simultaneous clips, footstep events at phase 0.6 and 0.1 (flag flip) with effect names `EFF_Step_<Human|Orc|Default>_<Gait>_<Side>_<Surface>_01`.
* Turn-in-place: `Turn90` clip, play speed = turn_rate_deg / (90 / clip_length).
* Aim: two overlay slots with weights (1-f) and 1, names derived from the current primary clip.
* Lip-sync: parse `.xlip` ({f32 time, u32 viseme}), map phoneme labels with the 40-entry table if you generate your own, build a facial motion by sampling the 14 viseme keys of the `_LipSync` source motion at the entry times, play once on the facial slot; do not interpolate pose-only tracks that have fewer than 14 keys.
* LoD thresholds as in B11 (screen-size factor).
* Skinning: max 4 influences, sorted and renormalised; palette per primitive; one skin-matrix refresh per frame; shadow prepass.
* Do not implement root motion; move characters from the movement system and scale clip speed.
* Ragdoll: only inside 2000 cm of the camera (250 cm hysteresis), 15 s life, local time scale decay.

---------------------------------------------------------------------------------------------------

# PART C: AUDIO

## C1. Classes and addresses

| Class | Address of key functions | Role |
| --- | --- | --- |
| `eCAudioSystem` (17) | `Create` 0x100571d0, `Update` 0x10057690, `SetSpeakerMode` 0x10057670, `SetListenerAttributes` 0x100575f0/0x10057630, `GetChannelCounts` 0x10057570, `GetPlayingChannelCount` 0x100575b0, `GetCPUUsage` 0x10057500, `GetRAMUsage` 0x10057540 | owns the FMOD `System*` (at +0xc) |
| `eCAudioModule` (25, engine module) | `Initialize` 0x10054630, `CreateChannelGroups` 0x10054bc0, `Process` 0x10054770, `UpdateListener` 0x100547b0, `UpdateOcclusion` 0x100548e0 / 0x10054810, `PlaySample2D` 0x10054d50, `PlaySample3D` 0x10054f80, `PlayStream2D` 0x100551e0, `PlayStream3D` 0x100552f0, `CreateStream` 0x10055440, `GetChannelGroup` 0x10054ba0, `GetChannelGroupPriority` 0x10054bb0 | game-facing API |
| `eCAudioChannelGroup` | `SetVolume` 0x100541b0, `SetPitch` 0x100541d0, `OverrideMute` 0x100541f0, `GetVolume/GetPitch` | wraps FMOD `ChannelGroup` (at +0xc) |
| `eCAudioChannel` (52) | `SetVolume` 0x10053ba0, `SetPan` 0x10053b80, `SetFrequency` 0x10053bc0, `SetLoopMode` 0x10053be0, `SetLoopCount` 0x10053c40, `SetPriority` 0x10053c60, `SetDelay` 0x10053c80, `SetLocation` 0x10053cd0, `SetVelocity` 0x10053d20, `SetMinMaxDistance` 0x10053d40, `SetCone` 0x10053d70, `SetConeOrientation` 0x10053db0, `SetSpeakerLevels` 0x10053dd0, `SetChannelGroup` 0x10053e00, `Pause` 0x10054020, `Mute` 0x10054000, `Stop` 0x10054040, `SetPosition` 0x10054060, `IsPlaying` 0x10053f70, `GetPosition` 0x10053ed0, `GetDuration` 0x10053e80, sync-point and end-of-sound callbacks 0x10054090/0x100540c0 | wraps FMOD `Channel` (at +0xc) |
| `eCAudioSound` (31) | `Create` 0x10055f60, `Play` 0x10056740 / 0x100567d0 (with group), `SetDefaults` 0x10056210/0x10056250, `SetLoopCount` 0x10056370, `SetMode`/`ModifyMode` 0x10056400/0x10056420, `SetMinMaxDistance` 0x100563d0, `SetCone` 0x10056390, `SetVariations` 0x10056330, sync points `AddSyncPoint` 0x10056640 etc., `Lock/Unlock` | wraps FMOD `Sound` (at +0xc) |
| `eCAudioStreamer` (9) | `OpenFile` 0x10056bf0, `ReadFile` 0x10056ea0, `SeekFile` 0x10056ee0, `CloseFile` 0x10056f30 | FMOD user file system on top of the game archive |
| `eCAudioEmitter_PS` (43) | `OnProcess` 0x10057ef0, `CreateSound` 0x10058330, `SpawnSound` 0x10058500, `Decay` 0x10058260, `ApplyDefaults` 0x10058140 | entity property set for world sound emitters |
| `eCResourceSound_PS` (31), `eCResourceAudioSample_PS` (22), `eCResourceSoundList_PS` (26), `eCSoundCache` | `eCResourceSound_PS::ApplyDefaults` 0x101b9c60, `eCResourceAudioSample_PS::CreateSample` 0x1005a4b0, `eCResourceSoundList_PS::SelectSample` 0x101bbb20, `LoadSample` 0x101bbce0 | sound resources |
| Game: `gCSVMManager` (45), `gCEffectCommandPlayVoice` (11), `gCEffectCommandPlaySound` (18), `gCEffectCommandMusicTrigger` (23), `gCMusic*`, `gCAmbient*` | listed below | |

## C2. FMOD initialisation (eCAudioSystem::Create 0x100571d0) [V from pushed immediates and constants; the decompiler dropped the arguments]
Required FMOD Ex version **0x44005** (4.44.05; the code compares `getVersion` to it and reports "incompatible FMOD dll version" otherwise).
1. `FMOD_Memory_Initialize(NULL, 0, alloc, realloc, free, memtype = -1)` with the engine allocators (`FModAllocMemory` 0x1016b030, `FModReallocMemory` 0x10057190, `FModFreeMemory` 0x100571b0), so all FMOD memory goes through `bCMemoryAdmin`.
2. `FMOD_System_Create`; version check.
3. Driver probing: `getNumDrivers`, `getDriverCaps(0)`. If the driver reports the **emulated hardware** capability (bit 2), `setDSPBufferSize(1024 samples, 10 buffers)` is applied [I on the cap bit]. `getDriverInfo(0)` -> if the device name contains **"SigmaTel"** the software format is forced: `setSoftwareFormat(48000, PCMFLOAT, 0, 0, resampler 1 = linear)` [V: the string compare at 0x10304b60 and the push sequence]. A call with the value 2 or 6 (chosen by a flag in the create struct; most likely `setOutput` NOSOUND=2 vs DSOUND=6, or a speaker mode) and a speaker-mode call with the driver caps' speaker mode precede `init` [U which].
4. `System::init(maxchannels = 64, flags = 0, extradriverdata = NULL)` -- tried first with the driver's speaker mode and, if it fails, retried twice after `setSpeakerMode(stereo 2)` [V: three `init(64,0,0)` call sites]. Error text: "Could not initialize audio driver".
5. `set3DSettings(dopplerscale = 1.0, distancefactor = 100.0 (float at Engine 0x1042677c), rolloffscale = 1.0)`. Error text "Could not set distance factor". **All 3D distances are in centimetres** and the doppler is effectively off because no velocities are supplied.
6. `setFileSystem(open, close, read, seek, NULL, NULL, blockalign = -1)` with the four `eCAudioStreamer` callbacks (0x10056f60, 0x100570a0, 0x10056fc0, 0x10057030). The audio streamer reads through the game's virtual file system / archives, so music and speech `.ogg` files stream straight out of the paks (files inside paks are located through `eCArchiveManager` and read through `eCArchiveFile`).
7. Logs "Audio system initialized!", the output type and the speaker mode strings. The runtime capture (docs AUDIO_FORMATS 7) shows the effective result: speaker mode stereo (2), 64 channels.
`eCAudioModule::Initialize` 0x10054630 skips all of this when the application's "no audio" flag (app +0xb4) is set (logs "Skipping creation of the audio system") [V].

## C3. Channel-group topology and volume categories [V]
`CreateChannelGroups` 0x10054bc0 creates five FMOD channel groups under the master group, in this order (enum `eEAudioChannelGroup` indexes `eCAudioModule` +0x20 + 8*i = {group*, priority byte}):

| Enum | Name | FMOD priority (lower = more important) | Initial volume = ge3.INI value / 255 |
| --- | --- | --- | --- |
| 0 | Master | 0 | Master (setup+4) |
| 1 | Voice | 1 | setup+0xc |
| 2 | Music | 2 | setup+8 |
| 3 | FX (group name string `FX`) | 3 | setup+0x10 |
| 4 | Ambient | 4 | setup+0x14 |

All four sub-groups are added to the master group; the divisor is the double **255.0** (Engine 0x10488480). The capture confirms exact values (music 0.2196 = 56/255, FX 0.8039 = 205/255, voice/ambient/master 1.0 in the default config). The channel priority of every channel is taken from its group's priority byte (`SetPriority(GetChannelGroupPriority(group))`), which is how FMOD steals channels when more than 64 are requested: master > voice > music > fx > ambient. `eCAudioChannelGroup::OverrideMute` 0x100541f0 mutes every channel in the group and the group itself (menu <-> game toggle).
The options UI strings give the user-facing categories: `HUD_Audio_Volume_Master/Voice/Music/Effects/Ambient` and preview loops `GUI_Loop_Ambient.wav`, `GUI_Loop_FX.wav`, `GUI_Loop_SVM.wav`.

## C4. FMOD calls and flag sets [V unless marked]
* Mode bits (FMOD Ex 4.4): 1 LOOP_OFF, 2 LOOP_NORMAL, 8 2D, 0x10 3D, 0x20 HARDWARE, 0x40 SOFTWARE, 0x80 CREATESTREAM, 0x100 CREATESAMPLE, 0x4000 ACCURATETIME [I], 0x100000 3D_LOGROLLOFF, 0x200000 3D_LINEARROLLOFF, 0x2000000 IGNORETAGS [I].
* `eCAudioSound::Create` 0x10055f60: asks the global module for the FMOD system, applies three global debug switches (DAT_1060daf0/1/2: refuse samples, refuse streams, force streams to samples), ORs **0x2000000** into the requested mode and calls `System::createSound(path, mode, NULL)` (path = archive-relative file name; the user file system finds it).
* `eCAudioModule::CreateStream` 0x10055440: `Create` with a caller-supplied mode: **0xC8 for 2D streams** (stream + software + 2D) in `PlayStream2D`, **0xD0 for 3D streams** (stream + software + 3D) in `PlayStream3D`; music fragments and transitions use **0xC9** (stream + software + 2D + loop off) [V `gCMusicFragment::CreateSound` 0x100f2530]; the emitter streaming path uses 0xD0; the audio sample resource path (`CreateSample`) uses **0x4150** = sample + software + 3D + accurate time. With the IGNORETAGS bit the capture logs 0x20000d0 and 0x2004150 [V].
* **Playing** (`eCAudioSound::Play` 0x10056740 / 0x100567d0): `System::playSound(FMOD_CHANNEL_FREE (-1), sound, paused = true, &channel)`, and the group overload then attaches the channel to a group. Every sound starts **paused** and is unpaused after the caller has set the group, priority, loop mode, min/max distance, 3D position and volume [V].
* `PlaySample3D` 0x10054f80 (group, name, position, volume, min, max, optional channel): query the resource data entity (must contain an audio sample property set), make sure the sample is loaded/cached in, take the `eCAudioSound`, `ModifyMode(clear 8 (2D), set 0x10 (3D))`, play paused, then `SetChannelGroup`, `SetPriority`, `SetLoopCount(0)`, `SetLoopMode(1 = off)`, `SetMinMaxDistance(min, max)`, `SetLocation(pos)` (FMOD `Channel::set3DAttributes`, velocity NULL), `SetVolume`, `Pause(false)`, `UpdateOcclusion`. `PlayStream3D` is the same sequence via `Channel` direct calls with mode 0xD0; `PlayStream2D` skips the distance and location steps.
* `SetLocation` 0x10053cd0 additionally refreshes occlusion for that channel.
* `eCAudioChannel::SetLoopMode` 0x10053be0: reads the channel mode, keeps the non-loop bits and writes the loop bits from the argument (1 = off, 2 = normal) [I]. `SetLoopCount` is the FMOD loop count (-1 forever).
* **Rolloff**: sounds created through the `eCResourceSoundList_PS` path pick the rolloff flag per resource -- logarithmic (`0x100000`, FMOD's inverse model) unless the resource's `UseLinearRollOff` is set, then linear (`0x200000`). Sounds created through the sample/stream paths set no rolloff bit, which is FMOD's default **inverse** model [V from the flags, capture agrees]. The inverse model with `distancefactor 100`: gain = min / (min + rolloff*(d - min)) for d > min, 1 inside `min`, no further attenuation past `max` (and no cut-off at max in the inverse model). With cm units: voice min 500 cm (5 m), FX min 150 cm, ambient bird min 500 cm.
* **Doppler / velocity**: none; `SetVelocity` exists but the listener update passes no velocities [V in `UpdateListener`].
* **Cones**: `Sound::set3DConeSettings(inside angle, outside angle, outside volume)` from the resource (`InsideConeAngle`, `OutsideConeAngle`, `OutsideConeVolume`; defaults 360/360).
* `Sound::setDefaults(freq, volume, pan, priority)`, `Sound::setVariations(frequency variance, volume variance)`, `Sound::setLoopCount`, `Sound::set3DMinMaxDistance` are applied once when a sample is loaded (`eCResourceSoundList_PS::LoadSample` 0x101bbce0).
* `eCAudioModule::Process` 0x10054770: optionally `UpdateListener`, optionally `UpdateOcclusion` (both auto-update flags default on), then `System::update`.

## C5. Listener and occlusion [V/I]
* `UpdateListener` 0x100547b0: takes the active camera entity's inverse world matrix (so it is derived from the camera's world matrix) and calls `set3DListenerAttributes(listener 0, position, velocity NULL, forward, up)`. Coordinates are **passed through unchanged** (left-handed, FMOD default) per the capture.
* `UpdateOcclusion(Channel*, bool)` 0x100548e0: for 3D channels only; reads the listener (`get3DListenerAttributes`) and channel (`get3DAttributes`) positions, computes an approximate distance (`GetMagnitudeApprox`/squared magnitude) and a smoothed occlusion value (lerp helper 0x10055b00), and writes `set3DOcclusion(direct, reverb)` after reading the previous values with `get3DOcclusion`. No FMOD geometry and no raycast is used [V: no geometry imports, no physics calls]. The exact curve is [U]; it is distance based low-pass only, with the "bool" selecting immediate versus smoothed update [I]. The all-channels variant 0x10054810 iterates the first 64 channels (`getChannel`, `isPlaying`) in a rolling window (member +0x1c remembers the next index) to spread the cost.

## C6. Sound resources
* `eCResourceSound_PS` (extension from `GetNativeFileExt`): properties and **defaults** (ApplyDefaults 0x101b9c60): Volume 1.0, UseStream true, UseHardware false, Pan 0, Frequency 44100 Hz (float 0x10488630), Priority 128, volume/pan/frequency variance 0, LoopMode (enum `eESoundLoopMode`), LoopRepeats, LoopForever false, Use3D true, UseLinearRollOff false, MinDistance 200 (Engine data float, run-time modifiable), MaxDistance 1,000,000, Inside/OutsideConeAngle 360, OutsideConeVolume 1.0.
* `eCResourceAudioSample_PS`: one `eCAudioSound` instance per resource, created lazily with mode 0x4150 from the resource's file path (+ native extension `.wav`/`.ogg` selected by `IsNativeFileType`; the extension table `.ogg .wav .mp3` is at Engine 0x431420).
* `eCResourceSoundList_PS`: a list of 20-byte entries {name, weight, ... , `eCAudioSound*` at +0x10}. `SelectSample` 0x101bbb20: probability of entry i = weight_i / (N * maxWeight), cumulative, compared against `rand()/32767`; since sum(weight)/(N*maxWeight) <= 1, the remainder yields "no sound" (-1) -- **a weighted random with a possible silent outcome** [V]. `LoadSample` builds the mode from the resource flags: (UseStream ? 0x80 : 0x100) | (UseHardware ? 0x20 : 0x40) | (UseLinearRollOff ? 0x200000 : 0x100000) | (Use3D ? 0x10 : 0x8) | 1, then applies the defaults above.
* `eCSoundCache` is an empty cache registration.

## C7. World emitters: eCAudioEmitter_PS [V]
Properties (docs AUDIO_FORMATS 4): `Sound` (sample name), `Enabled`, `AutoReset`, `AutoKill`, `UseStream`, `Volume`, `MinDistance`, `MaxDistance`, `SpawningMode`, `SecondsBetweenRepeats` (range), `RepeatProbability` (range), `UseMaxRepeats`, `MaxNumRepeats` (range). Defaults (`ApplyDefaults` 0x10058140): Volume 1.0, MinDistance **1000**, MaxDistance 1,000,000, Enabled true, AutoReset true, AutoKill false, UseStream false, `SpawningMode` = 2 (repeat), seconds-between-repeats range [1, 5], repeat probability [1, 1], max repeats range [0, 5], UseMaxRepeats false.
`OnProcess` 0x10057ef0 each frame: if the emitter is in a decaying state (state bits) wait for the channel to stop; if enabled it ensures a channel and a sound (`CreateSound` 0x10058330 resolves the sample through the resource admin, or when `UseStream` creates a 3D stream with mode 0xD0 from the file path), updates the channel position to the entity's world position, and if the channel is not playing:
* mode 0 (once): after the first spawn, auto-kill the entity (not in the editor) or reset;
* mode 1 (loop): `SpawnSound` with `Channel::setLoopCount(-1)` and loop mode 2;
* mode 2 (repeat): count down the inter-repeat timer; when it hits zero roll `RepeatProbability` (random in range vs `rand`); a failed roll re-arms the timer; honours `MaxNumRepeats` (then auto-kill/reset);
`SpawnSound` 0x10058500 plays the sound (group Ambient = 4 via `Play(..., 4)`), sets loop params, location, `set3DMinMaxDistance(MinDistance, MaxDistance)`, volume (the property), unpauses, increments the spawn count and re-rolls the timer. `Decay` 0x10058260 moves the emitter's entity to the world root so the sound can finish after the owner is removed.
Group used: **Ambient** (priority 4) [V: `Play(…, 4)`].

## C8. Voice and dialog playback [V/I]
* Dialog/SVM lines: `gCScriptProcessingUnit::StartOutput` 0x1016fc50 -> `CreateSoundAndChannel` 0x10169100 (one `eCAudioChannel` at +0x6c and `eCAudioSound` at +0x70 per processing unit) -> `eCAudioModule::PlayStream3D(group = 1 Voice, path, speaker position, volume **1.0**, min **500 cm**, max **1,000,000 cm**)`. Position = speaker's world matrix translation; for the player in first-person camera mode the camera position is used instead. `UpdateChannel` 0x1016fd90 keeps the channel at the speaker's moving position each frame (guarded by a global enable flag); `IsOutputPlaying` 0x1016fc30, `StopOutput` 0x1016fd70. This exactly matches the capture (voice priority 1, 500 / 1e6, mode 0x20000d0).
* `gCEffectCommandPlayVoice::Execute` 0x10059030 (effect-template voice): path template with the literal token `(voice).wav` (`Clone` 0x10058f10 shows it): the token `(voice)` is replaced by the NPC's `Voice` string (gCNPC_PS +0x18) lower-cased and the whole path is lower-cased; played through `PlayStream3D(group 1, volume/min/max from the command, position = entity world translation + optional matrix translation)`.
* `gCSVMManager` (singleton, 0x1018b850): the editor/database side of "SVM" (standard voice messages). It owns `Data/Strings/SVMAdmin.dat` (voices -> blocks -> SVM names), builds string keys `SVM_<voice>_<name>` (`GetStringID` 0x1018bd80, `FormatSampleName` 0x1018bc90) and sample names `INFO_<...>` (`FormatSampleName` 0x1018bb90 uses the current **audio language** from `eCLocAdmin`, the speaker entity name and a per-info format string, strips the `INFO_` prefix for the file name, and appends `.wav` only when asked). Language selection therefore lives in `eCLocAdmin::GetCurrentAudioLanguage`; speech files are looked up by "<language>/<…>" stems [I]. The runtime does **not** play SVMs from this class; the script layer resolves a key and calls `StartOutput` [I].
* Lip-sync source: the `.xlip` file next to each speech `.ogg` (B14). Who picks it [U].
* Speech priority: because the voice group is priority 1 and channels in FMOD are allocated from 64, dialog lines are never stolen by FX/ambient.

## C9. Effect commands PlaySound / MusicTrigger
* `gCEffectCommandPlaySound::Execute` 0x100582c0: position = entity world translation (+ matrix translation); `SelectSample` 0x10057f40 = the same weighted-with-silence choice as C6; then `PlaySample3D(group = 3 FX, name, position, Volume, MinDistance, MaxDistance)`. Group FX, priority 3 (matches footsteps/impact captures).
* `gCEffectCommandMusicTrigger::Execute` 0x1005c2b0: builds a `gSMusicTrigger` from the command descriptor (Location, Situation, DayTime, Variation, Transition, TransitionMode, ConnectIn/Out, Entrance, Exit, IsStopTrigger, DisableTriggers) and sends it to the music sequencer.

## C10. Music system [V structure, I for details]

### C10.1 Objects
`gCMusicModule` (engine module, g3.mul loaded by `PostInitialize` 0x10100f80) -> `gCMusicSystem` -> `gCMusicSequencer` (49 functions) with three `gCMusicChannel`s (primary, next-fragment, transition; sequencer +0x2c, +0x30, +0x34). Data (g3.mul, documented in AUDIO_FORMATS 3): `gCMusicFragment` (Location, Situation, DayTime, Variation, NextVariation, Sample, Volume, LeadInEnd, LeadOutBegin, Duration, link sequence from sync points), `gCMusicTransition` (ConnectIn/Out, FadeIn/Out begin/end, Offset, Timing, Sample), `gCMusicTrigger`, `gCMusicLink` (Offset, ConnectIn/Out, IsPriority, Type), lists with `Matches`/`Find*` helpers.

### C10.2 State and triggers
The sequencer's logical state is three strings: **Location** (+0x10), **Situation** (+0x14), **DayTime** (+0x18). `gCMusicModule::SetLocation` 0x10100ee0 / `SetSituation` 0x10100e90 / `SetDayTime` 0x10100e30 each build a trigger that changes only that field (the others empty = unchanged) and call `Trigger` 0x100febd0. `Process` 0x10100fc0: when auto-update is on (and not in the editor) the location string is refreshed from the current session's location name every frame; then `StartDueEvents` and `UpdateChannels`.
`gCMusicSequencer::Trigger`: ignored when triggers are not accepted; for a non-stop trigger, ignored when the current fragment is playing and the state already matches (`IsInState` 0x100fde20); a stop trigger is ignored when nothing is playing; if the "waiting for begin" latch (+0x45) is set the trigger is dropped. Otherwise it executes (`ExecuteTrigger` 0x100fec60) and applies the new state (`ApplyState` 0x100feed0, logs `[Trigger] old => new`).

### C10.3 Fragment choice
`FindFragment` 0x100fdfd0 fills empty trigger fields from the current state, then `gCMusicFragmentList::FindFragment` (match = case-insensitive equality on every non-empty field of Location/Situation/DayTime/Variation) and `SelectFragment` among the matches by `gEMusicSequence`: 0 = take the first fragment (in list order) that has not been played in this cycle, mark it "Shuffled", reset all marks when the list is exhausted (in-order round robin) [V]; 1 = random among the unmarked ones [I]; others [U].
If nothing matches, a **fallback chain** is applied repeatedly (`Game FUN_100fda80`): replace Variation by the default variation, then DayTime by the default daytime, then Location by the default location, then Situation by the default situation, retrying the search after each substitution; exhaustion logs "Could not find any fallback fragment. Giving up". Default strings are runtime globals (`g_strDefaultMusic*`) [U values; the data indicates day / explore / myrtana, see AUDIO_FORMATS 3].

### C10.4 Transition choice and timing
Fragment playing + new fragment: `FindLinks` 0x100fe4a0 picks an **exit link** of the current fragment (gCMusicFragment::FindLink with the trigger's exit-timing enum `gEMusicTriggerTime`, from the current primary time in ms) and an **entrance link** of the new fragment (`gEMusicFragmentPosition`). Link types: 0 Exit, 1 Entrance, 2 Loop, 3 Generic (an Exit/Generic link must lie later than the current time; `FindNextExit` 0x100f5bf0). `FindTransition` 0x100fe150/0x100fe230 selects the stinger by mode (`gSMusicTrigger` +0x14): 0 default (the transition with both connectors = 0x29a, i.e. "any to any"), 1 by the connectors of the two links, 2 by explicit connect-out/connect-in from the trigger, 3 by name; if none is found the default any-to-any transition is used. `FindFallback` 0x100fe290 retries with the next later exit when the exit is too early or the link connectors do not match the transition's connectors ("Exit too early", "Link combination does no longer match").
Two events are queued, each with a due time in ms measured on the primary channel: a transition event (`QueueTransition` 0x100fe680: fade-in/fade-out enabled flags) at the exit time and a fragment event (`QueueFragment` 0x100fe590) at the entrance time. Events are kept sorted (qsort) in a list at sequencer +0x38; `StartDueEvents` 0x100fe920 runs the first event whose due time <= the primary channel position (`GetPosition(ms)`), but drops it if the state no longer matches (obsolete event). If nothing is playing the fragment is queued at time 0.
`Execute(fragment event)` 0x100fea10: `QueueFragment` + `StartNextStream` on the **next** channel, swap next/primary channel pointers, delete all pending events, and call `TriggerFollow` 0x100ff040 which re-issues a trigger from the fragment's own follow rule (`GetFollowTrigger` 0x100f2420: location/situation/daytime empty, Variation = the fragment's `NextVariation`, transition mode "by name"=3 or default; `NextVariation` == the fragment's own variation means loop itself). `Execute(transition event)` 0x100feb10: plays the transition stinger on the third channel and tells it which channels to fade (src = old primary, dst = new fragment channel) and whether fade-in/out are enabled. `Execute(stop event)` stops all three channels and clears events.

### C10.5 Channels, fades and volume
`gCMusicChannel` (33 functions): each holds an `eCAudioChannel` plus `eCAudioSound`, the fragment or transition currently playing, and a queue (`QueueFragment` 0x100fc140, `QueueTransition` 0x100fc210). A queued item creates its sound through `gCMusicFragment::CreateSound` / `gCMusicTransition::CreateSound` with mode **0xC9**, plays it **paused** in the **Music group (enum 2)** (`Play(..., 2)`), sets the channel volume to the fragment/transition `Volume` and the start position (ms) with `SetPosition(ms)`; `StartNextStream` 0x100fc3c0 stops the old channel, moves the queued item in and unpauses.
Fades are applied only by the transition channel's `UpdateEnvelope` 0x100fc4f0 every frame while it plays: src volume = `gCMusicTransition::CalcSrcVolume` (1 until FadeOutBegin, then linear to 0 at FadeOutEnd, 0 afterwards; positions in ms of the transition channel), dst volume = `CalcDstVolume` (0 until FadeInBegin, linear to 1 at FadeInEnd, 1 after); `ApplyFadeVolume` 0x100fc560 sets each fragment channel's volume to `fragment.Volume * fade` [V]. The fade-in/out enable flags decide whether each side applies its curve. All curves are **linear in amplitude** (not equal-power) [V]. Everything is millisecond-based on FMOD channel positions. When the primary channel stops playing, `UpdateChannels` 0x100fdf40 stops all channels. Group volume (Music, from ge3.INI/255) multiplies the result outside this system.
* `LeadInEnd` / `LeadOutBegin` are only stored, sync points inside the fragment sample (`ImportLinks` 0x100f26a0, names parsed as `%02d:%02d:%03d` minutes:seconds:millis) define the exit/entrance links [I].

### C10.6 Music triggers in the world
Triggers reach the sequencer from (a) `gCMusicModule::SetLocation` (auto-updated from the session location), (b) scripts (`HandleMusicSituation`, `Music*` script functions), (c) `gCEffectCommandMusicTrigger` effects, (d) world music-zone volumes [U for the zone mapping].

## C11. Ambient system [V]
* Module `gCAmbientModule` (g3.ams loaded in `PostInitialize` 0x101b7b20): `Process` 0x101b7ba0 each frame: `SetClimate(session climate +0x290)`, `UpdateEnvironment` 0x101b7c00, then `gCAmbientSequencer::Process(frameTime)`.
* **Environment selection** (`UpdateEnvironment`): the session keeps six counters (+0x88..+0x9c) of how many ambient volumes of each environment type the camera/player is currently inside; the environment is the index of the **largest** counter, but only when that count exceeds 4; otherwise environment 2 (default). The climate (`eEAmbientClimate`) and the daytime come from session state.
* Library `gCAmbientLibrary` (environments per climate and environment type: `MinNumSounds`, `MaxNumSounds`, `MinSpawnDelay`, `MaxSpawnDelay`, `SpawnProbability`) and `gCAmbientSoundList` of `gCAmbientSound` (Sample, Volume, MinDistance, MaxDistance, Weight, Duration), selected per (climate, environment, daytime) at random (`SelectSound` 0x101bead0 uniform over matches).
* `Process` 0x101ba9a0: let n = sounds currently playing in the Ambient channel pool. If n < MinNumSounds spawn one immediately. Otherwise decrement the start delay by the frame time; when it reaches 0 and n < MaxNumSounds roll `rand()/32767 <= SpawnProbability` and spawn; in either spawn case or after a failed roll re-arm the delay with a random value in [MinSpawnDelay, MaxSpawnDelay] (log "Starting delay: %4.2f seconds").
* `SpawnSound` 0x101baac0: picks a sound and a random ambient emitter entity from the library (`FindObject`), takes the centre of that entity's world-tree bounding box as position and calls `PlaySample3D(group 4 Ambient, sample, position, sound.Volume, sound.MinDistance, sound.MaxDistance, reusable channel from FindChannel)`. Birds: volume 0.2-0.3, min 500 cm, max 1,000,000 cm in the capture.
* **Water beds** (`UpdateWaterSounds` 0x101bab90) run as 2D looped streams (mode 0xC8, `Pause(true)`, `SetLoopMode(2)`, unpause), on the Ambient group: `water_coast_01.ogg` plays while the camera's distance to the ocean (navigation-mesh query `GetDistanceToOcean`) is < **10,000 cm** with volume (1 - d/10000) * **0.3**; `water_oceanwaves_01.ogg` plays while d < **2,000 cm** with volume (1 - d/2000) * **0.2**; `water_river_01.ogg` plays while the normalised river distance <= 1.0 with volume (1 - 1.4*v) * **0.18** (clamped by the setter) [V constants, I on river normalisation]. Each is stopped when out of range. The capture's "coast water 2D linear loop" is this.

## C12. Behaviour summary numbers (cheat sheet)
* FMOD: 64 channels, stereo, software mix, 48 kHz float on SigmaTel only, doppler 1.0, distancefactor 100, rolloff 1.0, inverse rolloff default, listener = active camera, no velocities, occlusion distance-based only.
* Groups: Master>Voice(prio 1)/Music(2)/FX(3)/Ambient(4); volumes = ge3.INI/255.
* Voice: 3D stream, min 500 cm, max 1e6, volume 1.0. FX: 3D sample, min 150 for steps, volume by command. Ambient: 3D sample, per-sound min/max, water as 2D loops. Music: 2D streams, 0xC9, linear fades, ms timing.

## C13. Rust port implications (audio)
* Model 5 groups with the same priorities and volume scalars; give every voice **priority 1**, FX 3, ambient 4, music 2, 64 hardware-like voices with priority-then-quietest stealing (already in `mix.rs`).
* Implement FMOD's **inverse** rolloff in centimetres (distance factor 100, rolloff 1.0) as the default; only resources with `UseLinearRollOff` use linear. No doppler, no velocity. Optionally implement the distance-only occlusion as a low-pass/attenuation with smoothing.
* Voices: dialog lines start paused, play 3D at the speaker, min 500 cm, no cut-off; player in first-person uses the camera position.
* `PlaySound` effect: weighted selection with a "no sound" remainder; `(voice)` template substitution from the NPC `Voice` property, lower-case path.
* Sound-list defaults and `eCAudioEmitter_PS` defaults (min 1000 cm, repeat range 1..5 s, mode 2 repeat).
* Music: three channels (primary, next, transition), logical state (Location, Situation, DayTime), partial triggers, fallback chain (variation -> daytime -> location -> situation), exit/entrance link selection from sync points, due-time events on the primary channel in ms, linear fades from the transition's FadeIn/Out begin/end values, the follow trigger re-issued from `NextVariation`. Do not use a generic equal-power cross-fade; the original is linear and transition-sample driven.
* Ambient: environment by max-count rule (threshold > 4, default env 2), `Min/MaxNumSounds`, delay range, spawn probability, uniform sound pick, water beds with the above distance ramps.
* Lip-sync: feed `.xlip` into the facial motion builder (B14); the phoneme table is only needed if you ever regenerate lip-sync.

## Open items (from this section)
1. [U] Exact `UpdateOcclusion` curve and the exact FMOD speaker-mode/output-type branch in `Create`.
2. [U] Values of the default music strings (`g_strDefaultMusic*`) and `gEMusicSequence` modes >= 2.
3. [U] Phase/Action/Pose string tables (`g_pstrPhaseString` etc.): only indices used by the locomotion code are known (action 0x38 = Move, phases 8/9/10/0xc/0xf). The table can be rebuilt from the clip names (the shipped hero list shows every phase token).
4. [U] Meaning of the optional trailing `_R` after the nominal speed in clip names, and of `eSMotionDesc` max-loops value 2.
5. [U] Who calls `PlayFacialMotion` (Script.dll) and how the `.xlip` path is chosen.
6. [I] Directional-mixer merge formula for the two axes (`FUN_100db690`).

# Camera, character movement, world and streaming (Parts D, E, F)

Source: Ghidra pseudo-C export of Engine.dll, Game.dll, GUI.dll, GFC.dll, SharedBase.dll (image base 0x10000000, address = VA), plus read-only numeric data from the installation (`Data/Templates.pak` template `Essentials/Camera/Camera_PC_Camera.tple`, `Ini/ge3.INI`). Everything is written in my own words; no decompiled bodies are reproduced. This file lives under gitignored `.local/`.

Tags: **[V]** verified in code or data (address or file given), **[I]** inferred from control flow and naming, **[U]** unknown. Units are Gothic units (1 unit = 1 cm); angles in degrees unless stated; `this+0xNN` offsets are real struct offsets. Where the existing docs (`docs/research/PHYSICS_MOVEMENT.md`) already hold a number, it is repeated only briefly and referenced.

Decompilation gaps that matter: `gCCameraAI_PS::CorrectCameraPosition` (0x10026b80, 16 KB), `gCCameraAI_PS::PostInitializeProperties` (0x10030e80, 25 KB) and `gCGameApp::OnInitializeEngine` (0x1000c980, 28 KB) timed out in Ghidra. For the first I only have the call list (sphere/entity queries and rays); for the second I recovered the property-name table from the instruction stream and take defaults from the shipped template instead; for the third I recovered the configuration key list from string pushes.

Two handy facts used throughout:

* The game smooths almost everything with one "relax" filter. With a time constant `tau = relax * 0.025 s`, the step is `a = boost * dt / tau`, `new = (a * target + current) / (a + 1)`, and `boost = (4 * e + 1) / (e + 1)` with `e = (target - current)^2 / 100000`. So large errors are chased up to 4 times faster than small ones. It appears in camera distance, azimuth/elevation, FOV, camera position, ground height and animation velocity [V: 0x10026860 UpdateDistance, 0x10026690/0x10026750 UpdateDeltaRotX/Y, 0x10026a40 UpdateFOV, 0x10026350 UpdateCameraPosition, 0x100e1cd0 CalcAniVelocity, 0x100e94f0 PutToGround].
* Two small helpers recur: FUN_10038a50 is a plain lerp, FUN_10039c90 is a cosine ease on [0,1] (`(cos(x)+1)/2` shaped, used as fade/blend curve) [I], FUN_10039c40 is clamp01.

---

# PART D. CAMERA

## D.1 Class map

| Class | Where | Role |
| --- | --- | --- |
| `eCCameraBase` (73 fns, Engine) | 0x1005e9a0..0x10061210 | The actual render camera: view/projection matrices, FOV, aspect, near/far (`ClipDepth`), picking (`ScreenToWorld`, `WorldToScreen`, `PointToRay`), frustum database owner, helpers (`LookAt`, `Move*`, `RotateAngular*`). |
| `eCGfxCamera` / `eCGfxCameraStates` (Engine) | 0x1008db00.. / 0x1008dfe0.. | Thin API-device mirror of the camera (view matrix, projection matrix, depth-bias projection) and of render states. No game logic. Master/active camera statics. |
| `gCCameraAI_PS` (136 fns, Game) | 0x10020310..0x10037470 | The gameplay camera brain. A property set attached to the entity template `PC_Camera` (`Camera_PC_Camera.tple`). Drives `eCCameraBase` every frame from the player entity. |
| `gCCameraModeParams` (Game) | property object, 2 fns | A bag of float parameters (one object per camera mode, held in 27 `CamMode*` properties of the camera property set). |
| `CIdvCamera` (Engine, 2 fns) | 0x102beed0 | SpeedTree/IDV runtime shim; irrelevant for gameplay [I]. |

`gSession::CreateCameraEntity` (0x10177150) looks up exactly one entity named `PC_Camera` and fatals if the world has none or more than one ("current world has no camera!") [V]. `CreatePlayerEntity` (0x101772e0) does the same for the hero (`PC_Hero`, fatal on 0 or >1) [V]. The camera entity must be cached in before use.

## D.2 eCCameraBase (Engine)

Defaults set in `InitializeCamera` (0x10060c40): view/projection identity, near clip 1, far clip 2000 (`ClipDepth` property pair), aspect 1, perspective on, scale (1,1,1) [V]. These are overwritten at start-up by the engine setup.

Start-up values from `Ini/ge3.INI` `[Engine.Setup]`, read in `eCApplication::OnInitializeEngine` (0x1000c3e0) [V keys, V ini values]: `Camera.FOV=60`, `Camera.ZNear=25.0`, `Camera.ZBias=0.05` (depth bias; the ini note says ZNear and ZBias must change together). Far clip comes from the quality sliders (`Option.Sliders`): `fFarClippingPlane` 10000 (high), 8000 (medium), 6000 (low); a second, much longer far plane for the low-poly world proxy: 100000 / 50000 / 25000 [V ini]. Gameplay overrides the near plane (D.5).

Update path: `Update(flags)` (0x1005f4e0) recomputes the view matrix when bit 1 is set (`CalcViewMatrix` 0x1005f0a0: rotations X, Y(-), Z(-) composed from three stored angle fields at +0x98/+0x9c/+0xa0, translation derived from the position field +0x8c) and recomputes viewport/aspect/projection when bit 2 is set (aspect from the window rect, `CalcProjectionMatrix` 0x1005f270). The perspective matrix is built from `tan(FOV/2)`, near/far (`Far/(Far-Near)`), plus an extra depth-bias term from field +0xdc; orthographic mode uses 2/size scaling. Field offsets of interest: world matrix +0x0c, projection +0x4c, position +0x8c, FOV +0xd0 (radians), aspect +0xd4, perspective flag +0xd8, near +0xb8, far +0xbc, frustum databases +0xe8/+0xec [V offsets].

`eCCameraBase::Init` (0x10060fd0) forces a PVS prefetch (`eCPVSPrefetcher3::ForcePrefetch`) if near or far changed [V]; the prefetcher sphere radius is the far plane (F.5).

The gameplay camera only touches the engine camera through: `SetPosition`, `LookAt`, the vtable "update view" (slot 0x6c) and "update projection" (0x70), `SetFieldOfView` (radians), `SetClipDepth`, and (free-fly mode) `MoveLocal`, `RotateAngularRel` [V in ProcessCurrentMode 0x10022d10, OnProcess 0x100215c0].

## D.3 gCCameraAI_PS: state and inputs

### Modes

`gECameraMode` was not enumerated statically elsewhere; `ShowDebug` (0x10022b10) prints the names in value order, which fixes the enum [V]:

0 Normal, 1 Melee, 2 Ranged, 3 Magic, 4 Dialog, 5 Inventory, 6 Swim, 7 Dive, 8 Interaction, 9 Fall, 10 Death, 11 FirstPerson, 12 Shoulder, 13 LookBack.

Each of modes 0..12 owns a `gCCameraModeParams` property (`CamModeNormal` at property offset +0x14, `Melee` +0x24, `Ranged` +0x34, `Magic` +0x44, `Dialog` +0x54, `Inventory` +0x64, `Swim` +0x74, `Dive` +0x84, `Interaction` +0x94, `Fall` +0xa4, `Death` +0xb4, `FirstPerson` +0xc4, `FirstPersonFight` +0xd4, `FirstPersonRnged` +0xe4, `Shoulder` +0xf4) [V props table]. Fourteen further presets exist for dialogs: `DialogFaceL/R` (+0x104/+0x114), `DialogFace2L/R` (+0x124/+0x134), `DialogShoulder1L/2L/1R/2R` (+0x144/+0x154/+0x164/+0x174), `DialogSideL/R` (+0x184/+0x194), `DialogLerp1..4` (+0x1a4..+0x1d4) [V]. The mapping of enum values to the three first-person presets is [I]: `SetMode` (0x1002c4b0) selects the first-person/fight/ranged preset from combat state when mode 11 is requested; mode 13 (LookBack) reuses the current preset with a held-key flag (see below).

The script API is `PSCamera.SetMode(mode, bool, bool)` (23 call sites in the scripts, per SCRIPT_ENGINE_INTERFACE). `SetMode` has no native callers [V index], so every mode switch is script-driven except the few internal ones (first person via zoom, look back via key, dialog via `SetDialogRoles`).

`SetMode` behaviour [V 0x1002c4b0]:

* Ignored if the camera has no valid target or is disabled (+0x3c4).
* While LookBack is active (+0x3c3) any other request is deferred until the look-back timer (+0x3cc) exceeds 2 s; when it ends, the saved four look-back values (+0x3a0..+0x3ac) are restored into the params object.
* If currently in first person (or entered via zoom, +0x3c5) and the target mode is not FirstPerson (11), the request is only honoured for modes Ranged/Dialog/Inventory/Swim/Death-like groups when `bKeep1stPerson=true` in the `[Camera]` config section (modes 2 conditional on a ranged flag, 4, 5, 6, 10 are the "keep first person" group) [I on group meaning, V on config key `bKeep1stPerson`]. The first-person weapon slots are re-shown afterwards (inventory slots 0x10 and 0xf).
* The selected params object is stored (+0x1e4), the wished distance (+0x1f4) is set to `BestRange` of the mode unless the caller passed "keep distance" (arg 3, only honoured if +0x206), the rotation-offset duration (+0x3b0) takes `RotOffsetDuration` and the translation velocity (+0x3b4) takes `VeloTrans`... see D.4 for how those two are used. It also pre-computes a start-up camera position (+0x310) from the player's current heading, the mode's azimuth/elevation and `BestRange`, so mode changes start from a sane pose.
* The rotation-offset timer (+0x224) resets to 0.

Other flags worth knowing: +0x46c dialog camera active; +0x45c, +0x460, +0x464..+0x468 dialog role bookkeeping; +0x4d4 "first person allowed by script"; +0x4d5 hero-fade-out flag; +0x3c1 "in first person via zoom"; +0x3c6 debug overlay; +0x203 camera disabled; +0x202 sets in the debug-fly state `0x2a` key.

### Input

`OnAction` (0x1002b6c0) receives `eSGameKeyMessage` (type id at +0, flags +0xc: bit0 pressed, bit1 pressed-before, bit3 analog/axis, value +0x10) [V]. Handled ids:

| Id | Effect |
| --- | --- |
| 1, 2, 3/6, 4/5, 0xd, 0xe | free-fly camera movement flags (forward, backward, left, right, up, down) at +0x1fc..+0x201, only meaningful when there is no player target |
| 9, 10 (mouse X / key look right-left) | azimuth rate input +0x20c: analog axis value x 550 x dt; key variant x 550 x dt with sign |
| 7, 8 (mouse Y / look up-down) | elevation rate input +0x210: axis value x 50 x dt (inverse sign for 8) |
| 0xb | speed boost for free-fly: toggles +0x208 between 1 and the configured accelerator |
| 0x11 / 0x12 (zoom in / zoom out) | change the wished distance +0x1f4: wheel/digital step is `value * 100 * dt` for continuous input or a `0.5 * value` per notch for discrete input (bit3). Clamped to the mode's `[MinRange, MaxRange]` and to `fMaxDistToPlayer` (+0x544, ini 800). Zooming in below MinRange accumulates the overshoot in +0x3d0; when it exceeds 200 and the camera is not in first person, `SetMode(11)` is requested and +0x3c5 marks "first person by zoom"; zooming out while in that state drains the accumulator back by 0.5/notch and returns to the previous mode (+0x3b8) at 0. The accumulator is ignored while any GUI page is open. |
| 0x28 (look back) | mode 13 while held; releasing returns to the previous mode and clears the look-back timer; ignored in mode states 0xc/0xd/0xe of the movement component |
| 0x29 (first-person toggle) | toggles first person when allowed and no look-back is active (both entering and leaving use the same "start position" repositioning logic) |
| 0x2a | debug: sets +0x202 |

Mouse sensitivity and smoothing come from the options: sensitivity x/y is stored as `2*s + 0.5` at +0x548/+0x54c (0.5..2.5 for s in 0..1), smoothing x/y as `2*s` at +0x550/+0x554 and acts as a multiplier on the relax time constants (0 = unfiltered) [V setters 0x1002cce0..0x1002cdd0]. Invert flags have their own setters (`SetInvertXAxis/YAxis`), and the option list also has `AltCamera` and `CamLookaroundInverse` HUD options (gCHUDOptionControlsAltCamera / CamLookaroundInverse) [V names].

Config (`Ini/ge3.INI` `[Camera]`, read in `RegisterConsoleCommands` 0x10021fa0 once, on first `OnProcess`) [V keys; V ini values]: `fMoveSpeed=100`, `fMoveAccelerator=50`, `fMoveDeccelerator=0.3` (free-fly only, offsets +0x1e8, +0x1ec, +0x1f0); `fAziSpeedScale=0.1` (+0x358), `fElevSpeedScale=0.07` (+0x35c); relax factors `fRelaxFOV=25` (+0x360), `fRelaxDist=60` (+0x364), `fRelaxXAxis=8` (+0x368), `fRelaxYAxis=15` (+0x36c), `fRelaxPlayer=2` (+0x370), `fRelaxCamera=20` (+0x374); `fMaxDistToPlayer=800` (+0x544). Time constants are therefore: FOV 0.625 s, distance 1.5 s, azimuth 0.2 s, elevation 0.375 s, pivot/player 0.05 s, camera position 0.5 s (each times the mouse smoothing multiplier where applicable). All can be changed live with console commands (`OnConsoleCommand` 0x1002ecc0: `fFieldOfView`, `fMoveSpeed`, `fRelax*`, `fAziSpeedScale`, `fElevSpeedScale`).

## D.4 Per-frame update order (gCCameraAI_PS::OnProcess, 0x100215c0) [V]

1. First call only: read the `[Camera]` config.
2. `ShowDebug`, then base property-set processing; a rolling frame-time average/min/max is logged when its counter is non-zero.
3. If the dialog camera is active, refresh listener and speaker positions (audio listener = camera/listener entity position, `UpdateListenerPosition` 0x10025c90 and `UpdateSpeakerPosition` 0x10025ff0; the speaker point is the entity origin plus up to 350 units up or the box height, whichever is smaller). Otherwise copy the camera entity world matrix to +0x47c.
4. If the camera is disabled (+0x203) and not in a dialog: stop here.
5. If there is no player target: free-fly branch (move along local axes by `fMoveSpeed * boost`, rotate by the accumulated mouse deltas, scaled by constants 0.05 / 0.07 internal factors) and finish.
6. Player branch. If the player's world matrix is invalid it prints "Player matrix invalid!!!" and writes the last good matrix (+0x4dc) back. The node boundary must be valid.
7. `UpdatePlayerPosition` (0x10025410): computes the pivot (see below).
8. `ProcessCurrentMode` (0x10022d10): the main solver (see below).
9. `GetFirstPersonMat`, then the hero fade: distance from camera to head is clamped to [20, 100], normalised to 0..1 over 80 units and passed through the cosine ease; that factor times the base alpha (+0x51c) is applied to the hero and all attached children (`SetChildrenRenderAlphaValue`), the item meshes are hidden or shown (`EnableItemRendering`), the hero is fully hidden in first person unless dead/down or in certain movement modes (5, 6, 0xd) where the body must remain; a separate time-based fade (0.5 s, +0x4d8) exists for the "hero fade-out" flag [V structure, I on purpose].
10. Report the player position to the weather admin (`SetPlayerPosition`) and set the focus system mode (`gCFocus_PS::SetCurrentMode`: mode 1 when in first person, 2 otherwise).
11. Finally the camera entity is given the new world matrix (virtual slot 0xcc), and the per-frame inputs (rotation deltas, free-fly flags) are cleared.

### Pivot (UpdatePlayerPosition)

* Head matrix: entity world matrix with an extra translation of `box.maxY - box.minY` up the local axis; for a few creature meshes the bone named `<Species>_Head` is used instead, with the species names Boar->"Keiler" and Alligator->"Aligator" translated to the German skeleton names [V 0x1002b2e0].
* Target offset: three vectors blended by the distance ratio `t = clamp01((currentDistance - MinRange)/(MaxRange - MinRange))` between the mode's min-range and max-range values, applied in the player's local frame (offsets in the params object at +0x50..+0x64 for the target offset, +0x68..+0x7c for the camera offset, +0x80/+0x84 FOV min/max, +0x88 `TargetOrientationIsRelevant`, +0x89 `IsDialogPreset` [I offsets from usage, V names]).
* In third person a ray (nearest hit, ignoring player and camera entities through a filter callback) is cast from the head toward the pivot, shortened by the capsule (the ray end uses 0 when the hero is in combat mode 1 and not standing, otherwise a constant of ~0 offset; `S_FilterShape` 0x10021460 excludes a collision group). The result feeds two smoothed vectors (pivot smoothing and the "look target"), with the player relax factor and a velocity-dependent gain: gain grows with the squared offset (1 + 3 * e / (e + 1) style, divisor 10000 for the squared distance) and with the dot product of camera velocity against the offset (0.1 weighting). Net effect: the pivot follows the hero rigidly (relax 2 -> 0.05 s) while the aimed-at point is smoothed more strongly [I].
* In first person the pivot is simply the head position and the camera is anchored there.

### Solver (ProcessCurrentMode, wished translation/rotation)

* Time accumulates in +0x3cc (used for LookBack and dialog timing).
* `GetCurrentWishedTranslation` (0x10023190) in normal play: reads the mouse-driven delta rotations; they are filtered by `UpdateDeltaRotX/Y` (azimuth tau = smoothing x 8 x 0.025, elevation tau = smoothing x 15 x 0.025). Rotation target per frame = axis value (already per-frame scaled by 550/50 x dt) x mode `VeloTrans`-style factor at params +0x0c (name `VeloTrans`) x `fAziSpeedScale` or `fElevSpeedScale` x mouse sensitivity (0.5..2.5). Elevation (total pitch, +0x228) is accumulated from the filtered elevation delta and hard-clamped to [-89, +89] degrees. A small compensation: when the player is moving sideways, the camera yaw can swing by up to 80 degrees (constant 80, sign of the current azimuth delta) to keep the hero visible [I].
* Wished distance = `GetCurrentWishedDistance` (0x10026ae0): if the hero is standing (or has no movement component) use the zoomed value (+0x1f4); if the hero is moving use `max(MinRangeOnMove, zoomed)`; always capped by `fMaxDistToPlayer` (800). `UpdateDistance` (0x10026860) filters the effective distance with relax 60 (1.5 s) but snaps in immediately if the collision solver wants it closer; after a teleport (global timer > 2 s since marked) it uses a fast relax of 20 [V].
* Camera position = pivot + rotated offset: the unit direction from the pivot is built with `LookAt` plus the filtered azimuth (`BestAzi` in the preset adds to it) and the filtered elevation (`BestElev` adds), scaled by the distance. Then target and camera offsets from D.3 are added.
* `UpdateCameraPosition` (0x10026350) smooths the final position with relax 20 (0.5 s) but, in first person, uses a factor that depends on horizontal speed (up to 25 at speed 0, 10 when the movement mode is Sprint; speed normalised by 50) and ramps the vertical weight by `(-0.75 * ease) + 1`, so first-person bobbing/lag is tight.
* Collision: `CorrectCameraPosition` (0x10026b80, body not decompiled) uses `eCPhysicsScene::CollectEntitiesInSphere`, `TraceRayFirstHit`, `TraceRayNearestHit` and `eCCollisionShape::GetGroup` (call list only) [V list, U details]: the camera boom is shortened to the first blocker between pivot and wished position. In first person `MaintainWallDistance` (0x1002aa10) additionally collects triangles in a 35-unit AABB around the camera, finds the nearest triangle along the inverse normal within 17.5 units and pushes the camera out by the remainder (17.5 minus distance), then verifies with a ray from the head that the push did not cross geometry [V constants 35, 17.5].
* Camera shake: `AddTremor` (0x10024f40, called only from `gCEffectCommandEarthquake::Execute`) stores a per-axis amplitude (attenuated linearly with squared distance of the shake origin to the camera: `1 - d^2/r^2`), a decay rate `1/(duration*1000)` and sets intensity 1; stronger new hits overwrite per-axis maxima and bump intensity to `(old+1)*0.5`. `ApplyTremor` (0x10025260) each frame adds a random per-axis offset in [-1,1] (one axis alternates sign) times amplitude times intensity to the camera position and decreases intensity by `scaledFrameTime * decay` [V].
* Orientation: camera looks from its position toward the (smoothed) look-at point; elevation offset from `RotOffsetXMin/MaxRange` (pitch bias interpolated by distance ratio) and `RotOffsetYMin/MaxRange` (yaw bias) are applied over `RotOffsetDuration` seconds (timer +0x224 runs to `RotOffsetDuration`), so e.g. Melee tilts the camera by +5 degrees up close and -5 at long range [V usage, I on timing].
* FOV: target FOV = lerp(`FOVMinRange`, `FOVMaxRange`, distance ratio), forced to the preset's min value in first person and dialog; clamped into [10, 180] (fallbacks 10 / the max), then `UpdateFOV` (0x10026a40) relaxes toward it with tau 0.625 s; the camera FOV is only rewritten when the filtered value moved by more than a tiny epsilon [V].
* Clip depth: near plane is `Camera.ZNear` from engine setup normally; first person forces near 13 (float constant 13 at 0x10308524) [V]. Far is left as the camera's configured far.
* Dialog pre-roll: after `SetDialogRoles`, `+0x3e8` counts up to 18 s and is used as the progress of the two "Lerp" presets (see D.6).

## D.5 Per-mode default parameters (shipped template)

`Camera_PC_Camera.tple` (16 419 bytes) is the only place that fixes the real numbers [V parsed from the file; reproduced values are shipped data]. All 27 presets share the same 33 fields in this order: `VeloTrans`, `VeloRot`, `BestRange`, `MinRange`, `MinRangeOnMove`, `MaxRange`, `BestAzi`, `MinAzi`, `MaxAzi`, `BestElev`, `MinElev`, `MaxElev`, `RotOffsetXMinRange`, `RotOffsetXMaxRange`, `RotOffsetYMinRange`, `RotOffsetYMaxRange`, `RotOffsetDuration`, `TargetOffsetX/Y/Z Min/Max Range`, `CamOffsetX/Y/Z Min/Max Range`, `FOVMinRange`, `FOVMaxRange`, `TargetOrientationIsRelevant`, `IsDialogPreset`. Unlisted fields are 0 (all camera/target offsets, azimuth and elevation windows are -180..180 / -90..90 for the free modes).

Free-form modes (`MinAzi/MaxAzi` = -180/180, `MinElev/MaxElev` = -90/90, `BestAzi` 0, `MinRangeOnMove` 300, `MaxRange` 1500, `RotOffsetDuration` 1, flags 0):

| Mode | VeloTrans | VeloRot | BestRange | MinRange | BestElev | RotOffsetX min/max | FOV min/max | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Normal | 0.5 | 1 | 150 | 100 | 30 | -10 / +5 | 65 / 55 | default exploration |
| Melee | 0.5 | 1 | 450 | 150 | 30 | +5 / -5 | 60 / 65 | pulled back for combat |
| Ranged | 0.5 | 1 | 300 | 200 | 30 | 0 / -3 | 60 / 65 | TargetOffsetY 50 (at min range) .. 120 (at max range) |
| Magic | 1.0 | 1 | 150 | 100 | 30 | -5 / -5 | 45 / 45 | faster follow, narrow FOV |
| Dialog | 0.5 | 1 | 150 | 100 | 30 | -5 / -5 | 45 / 45 | |
| Inventory, Swim, Dive, Interaction, Fall, Death, Shoulder | 0.5 | 1 | 150 | 100 | 30 | -5 / -5 | 45 / 45 | all identical to Dialog |
| FirstPerson, FirstPersonFight, FirstPersonRnged | 1.5 | 10 | 150 | 100 | 30 | -5 / -5 | 70 / 70 | VeloRot 10 = very tight rotation follow |

So in normal play the camera idles 150 units behind the pivot at 30 degrees elevation (before the elevation filter, mouse input shifts it), pulls out to at least 300 while the hero moves (`MinRangeOnMove`), can be zoomed between 100 and 800 (global cap) and never exceeds 1500 by preset. FOV in Normal is about 64.9 at the default 150 and falls to 55 at 1500. Note that `Camera.FOV=60` in the ini is the engine default before the mode solver takes over.

Dialog presets (all `IsDialogPreset` 0 in the data, TargetOffset/CamOffset 0, FOV 60/65, VeloTrans 0.5, VeloRot 1):

| Preset | BestRange | MinRange | MaxRange | BestAzi | BestElev | MinElev/MaxElev |
| --- | --- | --- | --- | --- | --- | --- |
| DialogFaceL / FaceR | 200 | -50 | -50 | 0 | 0 | -90 / 90 |
| DialogFace2L / Face2R | 250 | -50 | 250 | -60 / +60 | 5 | 0 / 0 |
| DialogShoulder1L / 1R | 200 | 105 | 150 | -40 / +40 | 25 | 0 / 0 |
| DialogShoulder2L / 2R | 250 | 250 | 250 | -60 / +60 | 5 | 0 / 0 |
| DialogSideL / SideR | 400 | 400 | 250 | -120 / +120 | 5 | 0 / 0 |
| DialogLerp1 | 500 | 250 | 150 | +75 | 28 | 0 / 0 |
| DialogLerp2 | 450 | 250 | 150 | -75 | 28 | 0 / 0 |
| DialogLerp3 | 200 | 350 | 150 | -80 | 10 | 0 / 0 |
| DialogLerp4 | 200 | 350 | 150 | +80 | 10 | 0 / 0 |

For dialogs the angles are relative to the axis listener -> speaker (azimuth positive to one side, elevation above eye height), and Min/MaxRange behave as an interpolation window for the FOV/offset blend, not as a zoom window; the negative -50 entries in FaceL/FaceR disable the range blend [I].

## D.6 Dialog camera [V unless marked]

* `SetDialogRoles(listener, speaker)` (0x1002ce40; callers: `gCInfoManager_PS::ShowDialogManager`, `gCInfoScript_PS::Execute` / `OnCommandCompleted`, console) counts role changes (+0x464), tracks whether the same listener repeats (+0x468), and if the new listener has property set 0x3c (a creature template with that set) swaps the roles (+0x45c) so the camera never shoots from behind a giant. It then refreshes listener/speaker positions and either starts the dialog camera the first time (`StartDialogCamera(true)`) or re-cuts (`false`). A re-cut is skipped if the same pair was just shot (current preset is one of four shoulder/face presets and the dialog timer is below 18).
* `StartDialogCamera` (0x1002cfc0) warns and aborts when listener and speaker share a position, or either entity is gone ("one of the dialog partners is gone or invalid"). It chooses a left/right side flag (+0x460) from which preset the camera was last in (the angle sign alternates between cuts), calls `FindPreset` (0x1002d1a0) and installs the preset as the current params object. Previous presets are remembered (+0x470, +0x474) to avoid repeating the same shot.
* `FindPreset` randomises: it draws uniform randoms and with probabilities near 0.3 and 0.25 (constants 0.3, 0.25 at 0x103080bc, 0x10308284) prefers shoulder/side shots, otherwise a face shot; large creatures (local node box volume above 262 144, i.e. about 64^3) force the side/far presets; when the listener/speaker distance is below 2000 the nearer presets are allowed. `DialogPresetIsValid` (0x1002dca0) rejects a candidate when the geometry would put the camera inside something. `R`/`L` pick the mirrored variants.
* While a dialog camera runs, `GetCurrentWishedTranslation` computes position directly from listener and speaker: for the face/shoulder/side presets it rotates the speaker -> listener direction by `BestAzi` (about Y, applied with sign -1) and `BestElev` (about X), scales by `BestRange` and offsets from the listener or speaker position. For the two Lerp presets it computes two such poses and blends them by `t = ease(clamp01(timer/18))`, i.e. a slow 18-second camera drift between two framings. If the listener is closer than 15 units horizontally and a speaker head exists the camera is placed at the speaker's head matrix (a fallback first-person-like view).
* Audio: the audio listener is moved with the dialog camera; `EntityMovedBigDistance` (0x10025160) teleports camera internals (pivot, wished position, smoothed values) to the entity position and resets the audio mixer snapshot when the hero is moved far in one frame (called from teleport paths) [V].

## D.7 How player state feeds the camera [V]

Reads: player world matrix and local node boundary (height), the movement component (`IsStanding`, `GetMovementMode` for hero fade rules and sprint tuning, combat mode via `gCEntity::GetCombatMode`), `IsFirstPersonCameraAllowed` (script callback queried when the camera wants first person; the script returns false when the hero is down, dead or in knock-down tasks, per SCRIPT_PORT_PLAN), the inventory (to hide/show weapon slots 0x10 and 0xf in first person), the physics scene (rays), config keys and GUI state (any open page blocks zoom). Writes: the camera entity matrix, the audio listener, `eCWeatherAdmin::SetPlayerPosition`, focus mode, hero render alpha.

## D.8 Rust port implications (camera)

* Implement the mode solver as data: load the 27 presets from `Camera_PC_Camera.tple` at runtime (fields above), keep the enum 0..13, and drive position/rotation from pivot + (azimuth, elevation, distance) state. Do not hardcode the table; keep the 33-field order.
* Reproduce the "relax" filter exactly (boosted exponential with the 0.025 s unit) for distance, yaw, pitch, FOV and camera position; it gives most of the original feel. Use `dt` of the game frame (the original clamps frame time to 10..120 fps via `FpS.Min/Max`, and uses scaled time only for tremor and a few timers).
* Keep two separate smoothed points: pivot (fast, 0.05 s) and camera (slow, 0.5 s), plus the clamp of pitch to +-89 degrees and the distance rules (rest 150, 300 while moving, wheel step 100 units/s, hard cap 800, first-person switch when zooming 200 units past the minimum).
* Camera collision: sphere/ray test boom from pivot to wish position, snap closer instantly and relax outward; first-person wall push-out at 17.5 within a 35 box. The exact algorithm of `CorrectCameraPosition` is not recoverable from the export; treat the above as a behavioural spec and tune against video.
* The hero's alpha fade (20..100 units, eased) and the near plane (25 normal, 13 first person), FOV law (lerp over the distance window, 60-70 range) are cheap to match.
* Dialog shots: model presets as (azimuth, elevation, distance) relative to the speaker->listener axis, random selection with the 0.3/0.25 weights, 18-second Lerp blend, mirrored by side flag; the dialog camera is entered by the script-driven info command, so expose `SetDialogRoles` as a host command.
* Camera shake: port `AddTremor`/`ApplyTremor` semantics (squared-distance falloff, random per-axis jitter, linear decay) if earthquakes are scripted.

---

# PART E. CHARACTER MOVEMENT

Known numbers for the hero and template groups are already in `docs/research/PHYSICS_MOVEMENT.md` (speeds, accelerations, slope law, step height, jump/fall, capsules). This part adds the code map, mode transitions, rotation, animation coupling and the script event, and re-checks the numbers against the code.

## E.1 Class map

| Class | Fns | Role |
| --- | --- | --- |
| `gCCharacterMovement_PS` | 192 | The mover. 36 tunable properties (offsets below), mode machine, ground following, sliding, swimming, stuck detection. |
| `gCMovementBase_PS` | 12 | Tiny base: `MoveWorld`, `RotateLocal(Y)`, `GetAt/Up/RightVectorWorld`. |
| `gCCharacterControl_PS` | 27 | Player input -> wish direction/goal pose. Property-set type 22 (0x16) = "controlled by player". |
| `gCCharacterSensor_PS` | 25 | Smooths goal position/rotation and tests soft collisions (`ProcessPlayerMovements` 0x100eec80, `TestSoftCollisions` 0x100ef4c0, `SmoothGoal` 0x100eeb40). |
| `gCCharacterAnimation` | 1 | Empty shim. Animation coupling lives in `gCCharacterMovement_PS::ProcessMovementAnimation` and `eCVisualAnimation_PS`. |

Property offsets of `gCCharacterMovement_PS` (from the property table) [V]: DoHeightCorrection 0x14, ApplyGravity 0x15, IsQuadruped 0x16, QuadrupedSlopeInertia 0x18, StepHeight 0x1c, ForwardSpeedMax 0x20, StrafeSpeedMax 0x24, BackwardSpeedMax 0x28, TurnSpeedMax 0x2c, TurnSpeedModifier 0x30, MoveAcceleration 0x34, MoveDecceleration 0x38, TurnAcceleration 0x3c, TurnDecceleration 0x40, SlowModifier 0x44, FastModifier 0x48, SneakModifier 0x4c, SteepGroundAngleMin 0x50, SteepGroundAngleMax 0x54, WalkDownSpeedScale 0x58, SensorAffectsDirection 0x5c, SensorMinSlideAngle 0x60, SensorAdvanceDuration 0x64, SensorInertia 0x68, SlideInertia 0x6c, SlideSpeedMax 0x70, GroundSlopeTransInertia 0x74, AniVelocityInertia 0x78, ForceGroundAlignment 0x7c, JumpStartUpVelocity 0x80, JumpSteerScaleFactor 0x84, FallSteerScaleFactor 0x88, SwimDepth 0x8c, CanBePushedWhileIdle 0x90, LastFallVelocity 0x94, TreatWaterAsSolid 0x98. Runtime fields: ground/water/ceiling spatial state at +0x9c.. (floor y at +0x9c, ceiling-ish +0xa0), movement mode +0x100, last mode +0x104, current speed modifier +0x160, current speed +0x158, current angular velocity +0x15c, local velocity at +0x140..+0x148, smoothed animation velocity +0x14c, rigid body pointer +0x170.

## E.2 Movement modes and what changes them [V]

Mode values (field +0x100, `gECharMovementMode`), from the `Is*` predicates (0x100e8250.. 0x100e8330): 1 Standing, 2 Sneaking, 3 Walking, 4 Running, 5 Sprinting, 6 Jumping, 7 Swimming, 8 Diving, 0xc Sliding, 0xd Falling, 0xe disabled (scripted/animation-driven). Mode 0 is "uninitialised" (set to Standing or Running on first process, E.4). Mode 10 is a wading-like state [I]: it is chosen by `CheckChangedMovementStates` when the water above the feet exceeds the movement shape's step height but is below the swim depth; it uses the Walk speed modifier (`SlowModifier`), counts as a "moving state" and as a "cannot jump" state. Modes 9 and 0xb exist in `CanJump`'s exclusion list but are not produced by any function I could find [U]. The rule "movement is self controlled" is simply mode < 0xc (0x100e7620).

`SetMovementMode` (0x100e7630) is the single entry point. Steps, in order [V]:

1. Skip when the application is paused in a way that disallows it, or when the entity has not entered processing range, or when the mode did not change.
2. Standing requested: refuse if the floor under the character is too steep (`CanWalkOnFloor`, `SteepGroundAngleMax`, and the "ground checked" bit) or if the current mode is swimming/wading (7 or 10) [prevents standing in water]. Run `FindFloorWaterCeiling` plus `PutToGround` when the "needs re-ground" flag is set. Reset speed accumulators, ground-slope scale and velocity vectors.
3. Disabled (0xe): `StopMovement` is called; leaving 0xe also calls a helper to re-enable physics.
4. Sliding (0xc): remember whether the slide starts frontally (dot of the slope direction with the character's forward vector < 0 means the character faces uphill) and snap to ground.
5. Apply the new mode: `SetShapeByMode` (swap the collision capsule/shape set), `SetSpeedByMode`, `SetFlagsByMode` (rigid-body flags: for modes standing/walking/running/sneaking/sprinting/wading a "kinematic ground follow" combination, for jumping gravity on, for falling/sliding gravity applied with different flag sets), `SetEffectsByMode` (slide dust effect `eff_ani_slide_*`, and a warning if the NPC lacks `gCEffect_PS`). Entering jump remembers if the previous mode was sprint (+0x3d1) so a sprint-jump carries sprint steering.
6. The mode is stored (+0x100), the previous one in +0x104. If leaving a grounded mode for 0xc/0xd the reference height for fall damage (+0x390 "fall start height", from the ground sample +0x9c) is stored; arriving back in a grounded mode while player-controlled re-seeds the goal position with the current world position.
7. **Script notification:** a script callback named `OnMovementModeChanged` is invoked on the owning entity through the script admin (virtual call with the entity); the damage/fall rules live in that script function (floor(160*(t-0.9)^2) for fall time `t` above 0.9 s per COMBAT_AI_RULES 5.1) [V call, V name]. After the call it resets: last-landing timer (+0x3bc = -1), sensor state, some counters (+0x398, +0x3c4, +0x3c8), the "half speed" timer; the sprint-jump flag is cleared unless the new mode is jump or fall.

Transitions are made by `CheckChangedMovementStates` (0x100e5cd0), run every `OnProcess` (E.5):

* Ground lost / re-found handling around `FindFloorWaterCeiling` (0x100eb030, 1.5 KB: a ray cast down from the shape's box, with an "emergency" second cast from 4 m above (constant 400) when no ground entity is found; max ray length constant -100 000, so effectively unbounded); if the floor cannot be found it logs "lost ground" and sets the lost-ground flags.
* Water: the water level below the surface comes from the gCWaterZone data cached in the spatial state (+0x9c floor, +0xa0 ceiling/water, `GetWaterDepth` 0x100e9f70 = 57-byte accessor). With the `+0x18c` depth value (from property set 5, the navigation component) it chooses: **Diving (8)** when the water column reaches `SwimDepth`(+0x8c)... more precisely when the head-to-surface difference is at or beyond the shape's height-derived limit, **Swimming (7)** when depth >= SwimDepth - 10 hysteresis (the 10 is subtracted when already swimming), **Wading (10)** when depth >= the shape step height (or >= `StepHeight`-like value, 85 default if the entity has no player-control set), otherwise the previous grounded mode (or Running 4 if the previous mode was invalid) [I on mapping of 8 vs 7; V on SwimDepth hysteresis 10 and depth thresholds]. Falling into deep water while Falling (0xd) only switches to swim once the depth margin exceeds 2 units.
* Slide: if not jumping/sliding/falling and `CanSlide` (0x100ea700) says yes, enter Slide (0xc); non-human NPCs whose shape needs "complex static collision tests" enter slide immediately, others log "illegal slide ground for zone nsc". `CanSlide` conditions [V]: player-control set present; navigation set not in navigation control; the character not currently in the post-landing grace timer; not swimming; ground angle above `SteepGroundAngleMin`..`SteepGroundAngleMax`, with a (angle - min)/(max - min) test against 0.7 (constant at 0x10308078) and a speed-fraction test (current speed over forward max <= 0.3), and a minimum slope scale of 0.05 (constant 0x103082b8). Slide height tolerance is doubled while sliding.
* Jump (6): ends when the vertical velocity drops below 0.5 or 0.2 s after take-off without it -> Falling (0xd). Landing requires ground contact (or 0.3 s without vertical progress) [PHYSICS_MOVEMENT.md E5cd0]. Falling (0xd) is entered when feet are more than `StepHeight` (twice that in slide) above the ground (`ledge` detection includes the smoothed ground offset +0x38c) [V].

## E.3 Speeds, acceleration, turn rate, gravity, steps [V; numbers repeated for completeness]

* **Speed selection** (`SetSpeedByMode` 0x100e9390, `GetSpeedByMode` 0x100e9450): speed modifier = 1 (Standing/Running default), `SneakModifier` (mode 2), `SlowModifier` (mode 3 Walk and mode 10), `FastModifier` (mode 5 Sprint), `JumpSteerScaleFactor` (6), `FallSteerScaleFactor` (0xd), 0 in the disabled mode. Max ground speed = `ForwardSpeedMax` x modifier, blended with strafe and backward maxima by `GetCurrentMaxSpeedOfDir` (0x100eb800, 1.1 KB). Hero: forward 400, strafe 280, backward 280; modifiers walk 0.375, sprint 1.5, sneak 0.7 -> 150 / 400 / 600 / 280 cm/s.
* **Translation** (`ProcessControledTranslation` 0x100e2410): speed approaches the directional max by `MoveAcceleration` (1000 cm/s^2 hero, so run speed in 0.4 s; 250 on legacy templates) and decays by `MoveDecceleration` (-500) near the goal (`GetBrakingDistance` 0x100e8180, `GetPredictedDistanceByTime`); the player stops instantly without wish direction. Slope scaling and sliding as in PHYSICS_MOVEMENT.md (steep 20..49 degrees hero; slide max 800, inertia 0.4; slope-scale smoothing 0.15).
* **Rotation** (`ProcessControledRotation` 0x100e3000, 4 KB; `CalcNewRotationAngleAndVelocity` 0x100e49f0): angular velocity changes by `TurnAcceleration` x dt toward the sign of the remaining angle (999 999 on most templates, i.e. effectively instant) and by `TurnDecceleration` when reversing; when braking is enabled it starts decelerating once the remaining angle is below `v^2/(2*decel)`. It is clamped to `TurnSpeedMax` (200 deg/s, multiplied by `TurnSpeedModifier` 0.5 while Sprinting, so 100 deg/s) and the per-frame step is the mean of old and new velocity x dt, clipped so it cannot overshoot the target angle (1-degree dead zone snaps). `ProcessControledRotationLookAt` (0x100e4060) and `...ToTarget` (0x100e3fb0) handle look-at alignment and combat facing; `ProcessQuadrupedXRotation` (0x100e4550) pitches four-legged bodies along the ground using `QuadrupedSlopeInertia` 5 and `CalcQuadrupedDistances` (0x100ec5d0).
* **Gravity**: `ApplyGravity` (property) enables the rigid body's gravity; the scene gravity is (0,-981,0) cm/s^2 per world data (see PHYSICS_MOVEMENT.md). Vertical speed is clamped to -1500 at the end of `ProcessPhysicsMovements` (0x100e5110). `JumpStartUpVelocity` 600 (hero) gives apex 183.5 and 1.22 s of air time.
* **Step height / ground snap**: `StepHeight` 65 (hero and most humans; legacy 60). `PutToGround` (0x100e94f0) does the vertical follow: it samples ground at the shape position with a ray from `StepHeight`+0.01 above, caps corrections at `StepHeight` (warning "illegal height correction (greater than stepheight)" otherwise), and smooths the resulting height with the relax filter: time constant between 2 and 10 (x0.025 s; jump mode uses the minimum 2) for the foot height and 0.125 s (constant 5 x 0.025) for the secondary ground offset, `GroundSlopeTransInertia` for the slope scale, and a ground-ray ignore mask 0x20b9 (+0x8000 for complex shapes). Slopes steeper than `SteepGroundAngleMax` (49 hero) cannot be stood on (`CanWalkOnFloor` 0x100ea420 tests the angle between the normal and up).
* **Capsule/collision radius**: shapes are chosen per mode in `SetShapeByMode` (0x100e8ca0) and `CreateDependantShapes` (0x100e8370). If an entity has no movement shape it creates a default one (log: "has no movement shape(s)! creating default shape") and warns when a height correction would degenerate the shape. Hero numbers (cylinder 133.68 high, radius 33.16, centre y 96.94) are in PHYSICS_MOVEMENT.md section 2 [V].
* **Stuck handling**: `CheckCharacterGotStuck` (0x100e6760) logs "character %s got stuck at position" and teleports to the last good position [I on teleport]. `OnTouch`/`OnUntouch`/`OnIntersect` (0x100ebf10, 0x100ec150, 0x100ec210) feed soft-collision pushing for `CanBePushedWhileIdle` characters.
* **World/entity checks**: `OnProcess` (0x100e2120) warns "entity %s has wrong orientation, what the heck has happened" when the entity's up axis deviates more than 0.5 from vertical, for non-flying movement.

## E.4 Per-frame movement pipeline [V]

`gCCharacterMovement_PS::OnProcess` (0x100e2120): [skip for entities with property set 0x2d in state 9 and no collision] -> orientation sanity check -> base process -> if the application is paused or the physics scene is disabled, stop -> `CreateDependantPSets` (lazily binds the rigid body, sensor and control sets; for the player it logs "player has no gCCharacterSensor_PS") -> first-pose init (`InitFirstPose`) -> store the rigid-body velocity as the local velocity -> `CheckCharacterGotStuck` -> `CheckChangedMovementStates` -> `CorrectWeaponTrigger` (keeps the weapon trigger volumes glued to the hand) -> `ProcessSensorMovements` (player: `gCCharacterSensor_PS::ProcessPlayerMovements`) -> `CalcNextSteps` -> `ProcessMovements` (`ProcessControledMovements`: zone validation, `ProcessControledTranslation`, `ProcessControledRotation`, `ContributeFloorMovements` for moving platforms; if the resulting velocity is zero and no goal is pending, the rigid body is put to sleep) -> `ShowDebug` -> `SaveFrameStates` -> clear transient flags; for the player entity the control property set's pending-key byte is cleared.

If the owner is in the state "script/animation controlled" (property set 0x2d with state 9) the component only wakes the body, sets the mode to Running if it was 0 and never moves it.

### Input to wish direction (gCCharacterControl_PS)

`OnAction` (0x100de480) receives the same `eSGameKeyMessage` as the camera: ids 1 (forward), 2 (backward), 3 (strafe left), 4 (strafe right), 5/6 (turn), 7/8 (look vertical), 0xd, 0xe. It stores the pressed key, press state and duration (properties `PressedKey`, `IsPressed`, `IsPressedBefore`, `DurationPressedMSecs`), calls the script function `OnPlayerGameKeyPressed` on the entity and forwards the message to the entity's property set 0x15 (the movement component) [V].

`ProcessMovements` (0x100ddd40) builds the wish vector each frame [V]: the frame of reference is the active camera's inverse view matrix (or the character's own matrix when `ControlFrameOfReference` selects it; free-camera mode swaps the reference so keys steer relative to the character), forward/right axes are flattened to the horizontal plane when their vertical component is below 0.9999, each pressed key adds its axis, the sum is normalised and converted to a look-at orientation; then it sets the wished movement mode (the property `WishedMovementMode`/`SetWishedMovementMode`, held at +0xac) if the current mode is a grounded one (not jump, swim, wade, or >= 0xc) and calls `SetGoalPose`. With no key down the remaining speed is braked to a goal at `GetBrakingDistance` ahead. NPC navigation uses the same path via `SetGoalPosition`/`SetGoalRotation`/`SetGoalPose`. When the entity's navigation set is in navigation control (set 5 flag at +0x1d0) the control layer only resets the alignment look-at.

### Walk / run / sprint selection

Mode 3/4/5 are chosen by the script layer (walk toggle, sprint key, run default) and passed as the wished mode; the movement component never auto-promotes run to sprint. The session keys list contains `Walk`, `WalkToggle`, `Forward`, `Backward`, `StrafeLeft/Right`, `TurnUp/Down`, `Up`, `Down` (ini `Controls`), consistent with a walk toggle (hold-to-walk or toggle) [V names].

## E.5 Speed-driven locomotion vs root motion [V]

Locomotion is velocity driven, not root-motion driven. `ProcessMovementAnimation` (0x100e1a50) calls `CalcAniVelocity` (0x100e1cd0): the world-space displacement since the last frame divided by the scaled frame time, minus the velocity of the floor entity if standing on a mover, filtered with `AniVelocityInertia` (5 x 0.025 = 0.125 s) into the "animation velocity" (+0x14c). For ground movement modes (walk, run, sprint, sneak, swim, wade) with speed above 0.01 it passes that local velocity and the entity's right vector to the visual animation set, which picks/blends clips by speed and direction (clip nominal speeds in the data, e.g. `Move_Run_Fwd_..._400`, `Move_Sprint_Fwd_..._600`, played at rate `speed / nominal`, per PHYSICS_MOVEMENT.md). With no translation, a turn-in-place (angular speed above 10 deg/s, or accumulated turn in the frame converted to angular speed) selects turn animations. The default blend time is `AnimationBlending.DefaultFadeTime=0.2 s`; `AnimationBlending.PhysicControllerDistance=1200` (ini) marks how close a character must be to get physics-controller (ragdoll-capable) animation [V ini].

Combat and interaction animations ("state anims") are driven by the animation state machine through the scripts and affect movement only via `EnableMovementFromSPU`, `EnableCombatMovementFromSPU`, `EnableCombatRotationFromSPU` (flags that let the animation system own translation/rotation) and the mode `0xe` [I]. Hence root motion exists only for those scripted animations.

## E.6 Sensor (gCCharacterSensor_PS) [V structure]

The sensor is a smoothed shadow of the character used for slope-aware steering and soft collisions. `ProcessPlayerMovements` (0x100eec80, 2.1 KB) advances the sensor goal using `SensorAdvanceDuration`, damps it with `SensorInertia`, and optionally lets it steer the movement direction (`SensorAffectsDirection`) when the ground angle exceeds `SensorMinSlideAngle`; `TestSoftCollisions` (0x100ef4c0, 1.8 KB) queries the physics scene for nearby dynamic characters/obstacles and pushes the goal apart (the soft part of "walls and characters"). `IsHeightDifferenceValid` rejects goals with large height jumps. Property offsets of the sensor are not needed for the port beyond the three named properties above [I].

## E.7 Rust port implications (movement)

* Keep it a custom kinematic controller: no engine capsule controller exists in the original, so skin width and slope limit are replaced by the game's own rules (already how `crates/physics` is written).
* Port the mode machine exactly (1..8, 10, 0xc, 0xd, 0xe) and make `SetMovementMode` the only writer; fire an `OnMovementModeChanged(entity)` event to scripts after the state is applied, with the previous mode accessible (script reads `GetLastMovementMode`).
* Add the missing pieces: swim/wade/dive thresholds (SwimDepth 155 hero; 10-unit hysteresis; water level from water zones), turn-rate model (acceleration 999 999 = instant, max 200 deg/s, 100 while sprinting, per-frame clamp to the remaining angle), animation-velocity filter (0.125 s) feeding a speed/direction blend, and the 4 m-above emergency ground ray.
* Ground follow smoothing constants (foot height relax 2..10 x 0.025 s; secondary offset 0.125 s) explain why steps look soft; reuse them in place of the stand-in `ground_follow_speed`.
* Wish direction from camera: flatten camera forward/right to the horizontal plane, sum pressed axes, normalise; camera-relative vs character-relative selected by a flag (used in free-camera mode).
* Fall/landing damage and landing grace (0.3 s) come from the same component plus the script; keep `LastFallVelocity` and the fall-start height.

---

# PART F. WORLD AND STREAMING

## F.1 Object model: workspace, project, database, world, sector, layer [V]

| Class | Key functions | Behaviour |
| --- | --- | --- |
| `gCWorkspace` (45 fns, singleton, GUID {4C00B23D-D802-45fd-8B14-F2E25413E11D}) | `GetInstance` 0x10195960, `Create` 0x10196960 ("Workspace/"), `InsertNewProject/ImportProject` 0x10195ee0/0x10195dc0, `ImportDatabase`, `DoLoadData` 0x101965c0 | Root. Holds projects and databases. |
| `gCProject` (36) | `Create` 0x10191110, `ImportWorld/InsertNewWorld` 0x10190a50/0x10190b50, `Activate` 0x10190f00 ("ActiveProject"), `DoLoadData` 0x10190de0 | A list of worlds. |
| `gCDatabase` (55) | `Create` 0x1018f3d0, `Activate` 0x1018f260 ("ActiveDatabase"), `ReadTemplateLayerHeaders` 0x1018e9c0 ("TemplateHeaders"), `ImportTemplateLayer` 0x1018ed20, `SplitTemplatePath` 0x1018e1d0, `GetSaveGamePath` 0x1018e580 | Template database: `gCTemplateLayer`s holding `.tple` entities; mounts `_compiledMesh/_compiledMaterial/...` paths. |
| `gCWorld` (87) | `DoLoadData` 0x1019e8b0, `DoSaveData` 0x1019ecd0, `LoadGameWorld` 0x1019d170, `SaveGameWorld` 0x1019c6b0, `RestartWorld` 0x1019a300, `OnActivate` 0x1019fb00, `ReleaseData` 0x1019fde0, `DoAfterWorldActivate` 0x1019f610, `CreateSystemLayer/Sector/TempLayer` 0x10199c50 / 0x10199bb0 / 0x1019a060 | A world is a version-36 (0x24) file naming a config file with section `Common` and key list `Sector.List`; sector entries containing `SysDyn` mark the system dynamic sector. Local settings are kept in `_local.ini` (section `LocalConfig`, key `ActiveSector`). |
| `gCSector` (75) | `Create` 0x10194760, `DoLoadData` 0x10193c40, `OnActivate` 0x10194190, `OnEnable` 0x10192d50, `ReleaseData` 0x10194480, `BackupFiles/RestoreFiles` 0x101923b0 / 0x101929b0, `ImportGeometryLayer/ImportDynamicLayer` 0x10193750 / 0x10193290 | A sector owns geometry layers (static, "_cstat") and dynamic layers. Properties `VisualLoDFactor`, `ObjectCullFactor`. |
| `gCLayerBase` (27) | `SetSector`, `Lock/Unlock`, `IsDataAvailable`, `CanGotoState`, `QueryProgress` | Common base, load/activate state machine (`CanGotoState`, `QueryProgress`). |
| `gCGeometryLayer` (23) | `Activate` 0x100c7660, `DoLoadData` 0x100c7800, `OnCreateContext` 0x100c7b60, `CreateSpatialEntity` 0x100c7a10; property `GeometryType` | Static scene data in an `eCGeometrySpatialContext` (the "nodes"/cells). Culling entities are obsolete (message). |
| `gCDynamicLayer` (28) | `CreateDynamicEntity` 0x100c57f0, `DoLoadData` 0x100c5a50 ("RootEntity"), `IsSysLayer` 0x100c5600, `IsSysTempLayer` 0x100c56a0 | Dynamic entities (NPCs, items, spawned objects) in an `eCEntityDynamicContext`; layer names containing "temp" are skipped by saves. |
| `gCTemplateLayer` (36) | `GetTemplateEntity` 0x100ca5c0, `InsertNewTemplateEntity` 0x100cb040 ... , `GetFileNames` 0x100ca860, `ReadAll` 0x100cad60, `IsReadOnly` 0x100ca7a0; extension `.tple` | Holds the template entities (`_PC_Camera.tple` etc.). |
| `gCSectorPersistence_PS` (11) | `CollectSectors` 0x101a18f0, `UpdateSectors` 0x101a1800, `UpdateSector` 0x101a17a0 | A property set on the system entity that stores each sector's enabled flag in the savegame: `Collect` records (sector name, enabled) for all sectors; `Update` re-applies them via `gCSector::OnEnable`. |

Startup of the data model (`gCGameApp::CreateWorkspace` 0x100062c0, config section `[Project.Setup]`) [V]: `Workspace.Name=Data`, `Workspace.SaveGames=Save`, `Workspace.Backup=Backup`, `Workspace.Scripts=Scripts`, and the compiled-asset sub folders `Workspace.Compiled.Mesh/Material/Animation/Image/Physic` (ini values `_compiledMesh`, `_compiledMaterial`, `_compiledAnimation`, `_compiledImage`, `_compiledPhysic`); a missing section falls back to defaults with a warning. It creates the workspace with its database ("Database") and registers the compiled paths; logs "workspace '%s' in path '%s' created/loaded".

Sector semantics: sectors are not loaded/unloaded by distance. A sector has an `enabled` bool (`gCSector+0x4c`, set by `OnEnable`) and its layers are activated at world activation (`gCSector::OnActivate` activates dynamic layers first, then static layers, unless `ms_bEnableLayerActivation` is false; static layers with load state 1 are skipped). `OnEnable(flag)` enables or disables the entity contexts of every geometry and dynamic layer (`eCContextBase::Enable`) and flags the navigation admin that the nav scene must be rebuilt. The enabled flag is gameplay-controlled (console `ToggleSector`/`ToggleFolder`, scripts) and persists via `gCSectorPersistence_PS` [V]. `gCSector::DoLoadData` (0x10193c40) reads a name, a dynamic layer list (u32 count then names) and a geometry layer list; when `ms_bStaticLayerSpeedUp` is set, geometry layers whose file name contains "_cstat" are read eagerly as a block, each constructing an `eCGeometrySpatialContext` and reading its property data from the archive (`.lrgeodat`) [V]. The actual spatial paging is then done by the PVS prefetcher (F.5).

## F.2 Session and application

### Startup order (as far as visible) [V unless noted]

1. `eCApplication::Create` (Engine 0x10010950): install error callbacks -> virtual call that fills the application desc -> read `Documents\Gothic3\` config files (user ini, `UserOptions.ini`, missing -> message box "Configfile missing") -> `CreateEngine` (0x10010e40) -> create workspace (virtual; implemented by `gCGameApp::CreateWorkspace`) -> `eCModuleAdmin::DoOnAppInitializeModules`.
2. `CreateEngine`: apply `OnInitializeEngine` configuration (ge3.ini sections `Engine.Setup`, `Window`, `Game`, `Language`; Game-level `gCGameApp::OnInitializeEngine` 0x1000c980 first reads `IgnoreUserOptions` and `__UserOptions.ini`, then `Options.Difficulty` (`Difficulty`, `ExtendedContent`, `AIMode`), `Options.Controls` (mouse sensitivity/smoothing/invert, `AltCamera`, `CamLookaroundInverse`, custom key bindings: `SessionKey.*` with `Key1.Type/Offset`, `Key2.*`), `Options.Audio` (Master/Voice/Music/FX/Ambient volumes, `Speaker`, `VoiceLanguage`, `SubtitleLanguage`), `Options.Video` (resolution, refresh rate, brightness/contrast/gamma, VSync), `Options.Details` presets (Low/Medium/High/VeryHigh via `AutoDetection` 0x1000c070 which builds user options by hardware level) and the slider sections.
   Then CreateEngine: graphics subsystem (`CreateGraphicsSubsystem`), thread priority (`Threads.Priority`), memory/FPU debug toggles, gamma/brightness, `Cache.Size*` overrides and `GetOptimalCacheSizes` (F.6), per-cache thread counts (log line "Threads: Image, Material, Mesh, Sound, Vertexpool, CollisionMesh, Animation"), `Physics.Threads` (ini 2), `eCPhysicsScene::SetIsPhysicsEnabled`, HDR/bloom/noise/feedback parameters, `eCFrustumDatabase::UpdateFromAppTemplate`, then `CreateModules`.
3. `eCApplication::CreateModules` (0x126e0): module admin Create -> `DoInitializeModules` (each `eCEngineComponentBase::Initialize`, logged "Module Intitializing: %s...") -> `DoPostInitializeModules` ("Module PostIntitializing"). The registered engine components (all derive from `eCEngineComponentBase`) are: Engine: PerformanceAdmin, AnimationAdmin, AudioModule, DecalAdmin, GfxAdmin, GUIManager, GUIModule, IlluminationAdmin, ImageProcessingAdmin, InpAdmin, IteratorAdmin, MaterialAdmin, ParticleModule, PhysicsScene, PostProcessingAdmin, RenderSystemAdmin, ResourceAdmin, SpeedTreeAdmin, SceneAdmin, ShaderMaterialAdmin, VegetationAdmin, WeatherAdmin. Game: AIHelperAdmin, DummyCollectorAdmin, EffectModule, EnclaveAdmin, gCGUIModule, MusicModule, NavigationAdmin, NPCAdmin, ScriptAdmin, **gCSession**, BoidsAdmin, InteractionAdmin, TreasureSetAdmin. Their exact registration order is [U] (it is data in the module admin's creation list, not visible in a single function). `gCSession::PostInitialize` (0x101775b0) reads `[Game]` (`TestMode`, `NoMenu`, `PlayerStart=PC_Hero`), registers events (`RegisterEvents` 0x10174500, 6 KB, console/debug actions such as "Show NPC Movements", "Toggle NPC PutToGround"), creates the GUI manager and session keys (`gCSessionKeys::Create`) and enables input.
4. `gCGameApp::Create` (0x100137c0): registers console classes (world, `ToggleSector`, `ToggleFolder`, navigation, quest log, NPC information), sets the `Backup`/`Scripts` workspace names, calls the base `eCApplication::Create`. `OnEngineCreated` (0x1000c750) adds the engine actions Start/Stop/Pause Game and `dViewGameKeys`.
5. `gCGameApp::StartSession` (0x10013d90): refuses to run when physics is disabled (message box "you fool :) ! you can't start the game with physics disabled (see ge3.ini: physics.disabled=true)"), toggles full-screen as needed and calls `gCSession::Start` (0x10173d80): command-line handling, optional intro video `G3_Intro.bik`, tutorial `TUT_Start`, script callbacks `OnInit` and `OnGameStartUp` (and `OnReturnFromMenu` when coming back from the menu), thread pool enabled again (`eCThreadPool::EnableAllThreads`), navigation scene compile (`CompileNavigationScene`), creation of the `PC_Camera` and `PC_Hero` entities, clock set (`gCClock_PS::SetHour/SetFactor`), `GotoStartPosition` (0x101737d0, entity `PlayerStart`) or `GotoLoadedPosition` (0x10173ab0). `gCSession::Stop` (0x101741c0) removes player and camera, fires `OnPlayerChanged`, and disables the thread pool.

### Session API

`gCSession` also owns `StartControlNPC/StopControlNPC` (control another character, 0x10173290/0x10173540), `IsSaveAllowed` (0x101743a0), `OnResetCamera`, `OnResetPlayer`, cheat and console handlers (`OnCheat` 0x10171430, `OnGotoEntity`, `OnGiveItem`, `SpawnEntity`).

## F.3 Save game read/write and version handling [V]

* Location: `SHGetFolderPath(CSIDL_PERSONAL=5)` + `\gothic3\` (`GetSaveGameFolder` 0x101992c0). The workspace save sub folder (`Workspace.SaveGames=Save`) holds the sector data copies for the current game; `gCWorld::BackupFiles/RestoreFiles` (0x1019aa50/0x1019af70) copy sector/layer files into numbered `_#n` backup directories before and after a session.
* Files: each save is a pair of files with extensions `.g3savcp` and `.g3savcpdat`; the alternate (extended content) build uses `.g3savcpx` and `.g3savcpxdat`, chosen by an engine-setup flag at +0x1ea (the "extended content" switch) [V extension strings at Game.dll 0x2fd8a0..]. The first holds the header and thumbnail; the second the world state.
* Header (`gCSaveGameHeader::Read` FUN_10197530 and `GetSaveGameHeader` 0x10198c40) [V]: magic `GENO` + `MESG` (two u32 0x4f4e4547, 0x4753454d), then a u16 **header version**: accepted values are 0xaf (175) for the extended build and 0xb0 (176) for the normal/CP build; any other value logs "This savegame hasn't a valid version: %d", and a mismatching one logs "made by NORMAL CP / EXTENDED CP version" (warning only; loading continues) [V]. Then two strings (save name, description), a date/time, four u32 (the world's four version numbers `GetWorldVersion` 0x10198c00 = gCWorld fields +0xb8..+0xc4), an MD5-based signature compared against a recomputed hash of the fixed string `G3.CP.ONUS.ToO`, the version u16, date, both strings and the four numbers (property id equality; mismatch marks the header invalid), a u32 length + blob (extra data), a flag and an embedded screenshot (`eCGfxImageData`). The container itself is a GENOMFLE archive: `GetSaveGameHeader` opens the save file and checks the 8-byte magic `GENOMFLE` (u32 0x4f4e4547, 0x454c464d) followed by two u32 (header block offset and size).
* `SaveGameWorld` (0x1019c6b0): deletes old files, opens both files, writes the header (four world version numbers, a position placeholder), then for every non-temporary dynamic layer of every sector calls `eCEntityDynamicContext::WriteSaveGame` (recording the stream position per layer name in a hash), then for each static `eCGeometrySpatialContext` that is a "hybrid" context collects entities that have an interaction property set and writes `entity id` + the interaction PS save block (door/chest/lock states; fatal error "this type of entity must have an interaction PS !! Collection is incorrect !" if one lacks it), writes the table offset, thumbnail, and renames temp files over the old ones (`DeleteFileA`/`MoveFileA`). Entity patching is disabled during writes. Each property set implements `WriteSaveGame`/`ReadSaveGame` with relevance flags (`GetSaveGameRelevance`): implemented for Clock, DamageReceiver, InfoManager (runtime info packets, with the warning "Found %ld Info runtime packets, but %ld Infos are available"), Interaction, Inventory, Lock, NPC, Navigation, PlayerMemory, ScriptRoutine, Statistics (plus QuestManager, Party, Mover, Door, Dialog, AIZone, Anchor, Arena, Enclave, Item, TouchDamage, SectorPersistence, TriggerBase/Trigger through `GetSaveGameRelevance`) [V names].
* `LoadGameWorld` (0x1019d170): reads and validates the header, opens both files, deactivates all non-system dynamic layers and the system layer/temp layer (progress callback with sector counts), kills every dynamic entity, re-activates the layers from the stored data (`eCEntityDynamicContext::ReadSaveGame` per layer stream offset found in the hash), restores hybrid-entity interaction states (warning "hybridentity with id: %s or interaction ps not found ! Skipping information"), calls `DoAfterWorldActivate` (0x1019f610: adds the `World_MCP` entity components, sets engine caps from the frustum capability template), caches in geometry contexts (`eCGeometrySpatialContext::CacheIn`), forces a PVS prefetch and finally logs "Loadgame: %s loaded !". **Old save patch:** if the loaded header version is below 0x6b (107) the loader logs "old savegame found, patching slots, calling OnChildrenAvailable for all npc's" and re-runs the character child setup for every dynamic entity (the hero by name included) [V string and threshold; the accepted set in `Read` is only 0xaf/0xb0, so 0x6b applies to a field on the world, [U which field]].
* `RestartWorld` (0x1019a300) reloads without a save: kill entities, enable sectors, `UnloadCaches`, `DoAfterWorldActivate`.
* `GetSaveGameList` (0x10199030) lists the save folder with `_findfirst`/`_findnext` and `ExistsSaveGame`; `CreateSaveGameImageFromBB` captures the back buffer for the thumbnail; `DeleteSaveGame` removes the pair.

## F.4 Navigation (summary)

* `gCNavigationAdmin` (61 fns, component): `CompileNavigationScene` 0x1011ed90 (also run at session start; logs critical doors/NPCs with sector names; writes `_CriticalNPCs.txt`), `Process` 0x10120720 (per-frame; resets navigation on teleports, handles `LastUseableNavigationPosition/ZoneID/ZoneIsPath` and `CurrentZoneEntityProxy` of controlled characters, uses `GetDCCRadius` from the script for door/character radius), `RunDynamicNavigation`, `GetNearestNavigationPoint`, `GetRandomNavigationPoint`, `CheckAllControlledDCCs`.
* `gCNavigationMap` (64 fns): `CompileStaticNavigationScene` 0x101294d0 (11 KB), `BuildLists` 0x101236d0 (registers nav helpers into zones), `WriteLists` 0x10127630 (`/NavigationMap.xnav`), `FindWay` 0x1012efc0 and 0x10131090 (A* over zone graphs, "DEADLOCK PREVENTION (3 sec.)"), `TestEntityObstacles`.
* `gCNavZone_PS` (60): zone polygon with top tolerance (`TopToleranz`), `Test3DPointInZone`, stretch intersections, evade/cluster lists; `gCNavPath_PS` (58): path zones (`Test3DPointOnPath`, `CalcNearestPathPoint`, `GetCosts`); `gCNavHelper_PS`: links two entities (`LinkedToEntityID`, `LinkedToSecondEntityID`) for doors/ladders; `gCNavOffset_PS`: offset poses for approach animations; `gCNavigation_PS` (111): per-NPC component (zone tracking, `OnEnterProcessingRange` floor detection type 1 and 2, `OnEnterArea/OnLeaveArea` script callbacks, `FillNextForNavigationPath`, `FillNextForPreferredPath`, `RunSimpleMovement`). Navigation data is compiled from authored zones/paths in the sectors, so the port's grid-based nav is a substitute [I]; the original file format is `NavigationMap.xnav` [V name].

## F.5 Streaming around the player

Two independent mechanisms exist [V]:

**(a) Spatial paging with the PVS prefetcher** (`eCPVSPrefetcher3`, 26 fns; cell items `eCPVSCellItem`, 35 fns; job `eCPVSPrefetcherJob`). `eCRenderSystemAdmin::Render` calls `eCPVSPrefetcher3::Process` (0x1aabe0) once per frame per `eCFrustumDatabase` (main world and low-poly world have separate databases). `Process` returns when the prefetcher is disabled (`EnablePrefetcher`, ini `Render.EnablePVSPrefetcherThread`) or the database is unregistered; it builds a sphere from the camera: radius = camera far clip (times 0.85 if the camera is in the alternate "orthogonal/special" state, constant 0.85), centre = camera position, and calls `Execute` (0x1a98b0). Cell size is `eCFrustumDatabase+0x1b8`, default 10 000 units (set in `eCFrustumDatabase::Create`), configured by `Render.PrefetchGridCellSize=10000` (main) and `Render.PrefetchGridCellSizeLowPoly=34000` (low poly), "corresponds with landscape patch size" per the ini comment [V]. `CalculateRectIndices` (0x1aa690) converts the sphere to a cell rectangle: centre +-0.4 x radius, offset by 1 000 000 so indices are positive, divided by the cell size, expanded by one cell on every side. `RecalculateGrid` (0x1aa750) re-evaluates only when the rect changed, with extra hysteresis on camera movement of `max(1300, 0.4 x radius)` or `max(1300, 0.2 x radius)` depending on a database flag; it creates `eCPVSCellItem`s (hash table of 967 buckets, 80-byte cells with 3D bounds from -1e6..+1e6 vertical) for new rectangle cells and calls `CacheOut` / deletes items for cells that left the rectangle (fatal "Cell should be removed !!" when something still references a cell scheduled for removal). `Execute` additionally builds an `eCPVSPrefetcherJob` (thread-pool worker, `WaitForEmptyThreadQueue`) that scans entities in the spatial contexts of the scene admin (static contexts with graphs and eCGeometrySpatialContext nodes) and creates `CacheIn` work items; spatial contexts whose bounding rect no longer intersects the cell rectangle and which are not hybrid are `CacheOut`'d, adjusting a running byte budget (`GetNodeFileSize`) and a context counter. A guard fatals on a camera position beyond 1 000 000 on any axis ("illegal pvssphere position detected").
Resulting numbers: with far clip 10 000 (high) the rectangle is +-4 000 around the camera plus one 10 000 cell margin, i.e. effectively a 3x3 block of 100 m cells; at far 6 000 it is the same 3x3 block; the low-poly proxy uses far 100 000 and 34 000-unit cells -> +-40 000 around the camera, 5x5 cells [I from formula]. Camera movement below 1300 units (main, with far 10 000 the 0.2 x radius = 2 000 -> 2 000; the max of the two applies) does not re-evaluate [V formula].

`eCPVSCellItem` (0x101a6ff0..): per-cell static-hierarchy KD tree (`BuildStaticHierachy` 0x101a7520, 2.2 KB, uses vertex data lock/unlock), `CacheInEntity/CacheOutEntity`, `EnableDatabaseSet/DisableDatabaseSet` (a cell can belong to up to 32 frustum databases, one bit each via `+0x21c`), `RenderOccludees`.
`eCGeometrySpatialContext` (35 fns): `CacheIn` 0x1021c830 / `CacheOut` 0x1021c880 swap the node file (`ReadNodes` 0x1021c080 handles versions via `ReadNodesV83` 0x1021be70, `ReadNodesV56` 0x1021bc30 and `ReadNodesOld` 0x1021b9c0 - the three generations in WORLD_SURVEY), `GetNodeFileSize` 0x1021b6c0, hybrid contexts (`EnableHybridContext`, `IsHybridContext`) mix dynamic state and static nodes (saved through interaction PS).

**(b) Entity processing range** (`eCEntityAdmin`, 35 fns): `Process` (0x10230a70) each frame takes the active camera position (or the player's, depending on app state), keeps two regions: the dynamic ROI sphere (radius from `SetROISphere`; engine default 400, but `Entity.ROI=4000` from ge3.ini is applied at start-up and clamped to the far plane) and a static ROI sphere (`SetROIStaticSphere`, default 600) both extended by a hysteresis constant of 250 (`eCEntityAdmin+0x40`): the region is recomputed only when the camera moved farther than 250 from the last centre. In the recompute it fetches the valid PVS cells (`GetValidCells`), walks each cell's static KD hierarchy culling regions against the ROI box (`CullProcessingRangeRegion` returns inside/partial/outside), collects entities of the static cells (`CollectStaticProcessingRangeEntity`, flag masks 0x100100 required and 0x40 excluded) and dynamic entities of the dynamic contexts (`CollectDynamicProcessingRangeEntity`, mask 0x100108); an entity enters processing range if its own processing-range box (entity field +0xe8, from the per-entity `ProcessingRange` property) intersects the ROI. Entities that left get `ExitProcessingRange` (virtual 0x128) and entities that entered `EnterProcessingRange` (0x124), which raise the script callbacks `OnEnterProcessingRange`/`OnExitProcessingRange` and per-property-set hooks (navigation floor detection, interaction, collision shapes: a shape complains when cached out while in range). Dirty entities between recomputes are handled by `UpdateProcessingRangeEntity`. Fading when leaving: `ObjectDetails.fProcessingRangeFadeOutRange_*` in the option sliders (VeryHigh 200, High 300, ... [V ini]) and `eCEntity::SetProcessingRangeOutFadingEnabled`. Also in the same options: `fScreenObjectDistanceCulling` (VeryHigh 0.002, High 0.008), `fRangedBaseLoDOffset` (VeryHigh 800, High 400), `fGlobalVisualLoDFactor` (VeryHigh 1.0, High 0.01), mesh/animation LoD quality stage 0..2 [V ini].

## F.6 Resource and cache system

* `eCResourceAdmin` (19 fns, GUID {56E2CF85-389E-42c7-BB73-366C8B0C53F4}) owns the `eCMainCache`; `Process` (0x101e8070) copies each cache's usage numbers into statistic fields and then runs `eCMainCache::Process`; `QueryResourceDataEntity` (0x101e83b0) resolves a resource id to a data entity ("second chance: try to resolve existing resource"); editor mode is a flag.
* `eCMainCache` (28 fns) owns seven caches (property objects at +0x10..+0x28): image, mesh, sound, material, collision mesh, animation, speed tree. `SetCacheSizes` (0x101f2eb0): image budget P, and sets mesh/material/etc.; total = 3P + others; soft limit total - 0.15 x total (i.e. 85%) and hard limit total + 0.05 x total (105%); a "spare" pool (+0x54) can be lent across caches by `RequestMemory` (0x101f2a70) / `ReleaseMemory` (0x101f2b90) by cache type 1..7. `PurgeCaches` / `PurgeImageCache` clear all or images only.
* Budgets: ge3.ini keys read in `eCApplication::OnInitializeEngine`: `Cache.SizeImage`, `SizeMesh`, `SizeSound`, `SizeMaterial`, `SizeCollisionMesh`, `SizeAnimation`, `SizeSpeedTree`; otherwise `GetOptimalCacheSizes` (0x10014a80) decides from `GlobalMemoryStatusEx` [V]: failure/default -> image 132 MB (336 MB for extended content), mesh 88 MB, sound 21 MB, material 55 000 (count), collision mesh 25 MB, animation 37 MB; machines with under about 1.5 GB physical use 99 MB (282 MB extended) / 52 / 17 / 30 000 / 18 / 31 MB; otherwise each value is its base value plus base times a factor derived from the total virtual address space (capped near 4 GB, scaled by 1/2^31 minus 1) [V values, I on the exact factor]. Speed tree gets 1 000 000.
* `eCBaseCache` (21 fns): per-cache worker thread pool (`SetThreadNumber`, max 10, "10 threads are set" warning above), `CacheInResource` (0x101efa80) logs "resource '%s', {%s} cached in" and, for resources in state 2 ("can be loaded"), queues a work item on the pool; state 3 = loading, 6 = resident; synchronous callers wait via `WaitUntilNonProcessing`; resources with lazy-load behaviour may stall the thread queue (warning text). `CacheOutResource` unloads. `Process` (0x101f0020) is the eviction policy: scan entries; those with reference count 1 (only the cache holds them) are candidates, released first when above the soft limit; otherwise each resource gets a score from (1 - age/maxAge) + (1 - rank/size) + pressure factor (1.0 below the soft band, 0.75 between 85% and 100%, 0.4 above 100%, 0 above 105%), averaged over 3; those scoring below 0.25 are unloaded; large pressure forces eviction by size until under the soft limit. `eCImageCache::PostInitialize` (0x101f1200) reads `.High`/`.Medium` quality and per-image `NoMip`/`MipMapCount` override sections (imagequality.ini) [V names]; `SetLoDQualityStage` sets the mip stage. `eCMeshCache` is just an empty subclass of the base.
* Threads: `CreateEngine` logs and applies a thread count per cache type (image, material, mesh, sound, vertex pool, collision mesh, animation); `eCMainCache::SetDisableThreading` property exists; `Threads.Priority` raises process priority; `eCThreadPool::EnableAllThreads` is toggled around session start/stop to pause background loading [V].
* Other config: `Render.PrefetchGridCellSize(+LowPoly)`, `Entity.ROI`, `Render.EnablePVSPrefetcherThread`, `Physics.Threads`, `FpS.Min=10`, `FpS.Max=120`, `FpS.Fixed=-1`, `Timer.bIsSmooth=true`, `Timer.ThreadSafe=true`.

## F.7 Scene admin and entity admin [V]

* `eCSceneAdmin` (49 fns): registry of contexts (`RegisterSpatialContext`, dynamic contexts), name index (`RegisterNameInfo`, `GetEntityByName`, `GetEntitiesByName`, `GetUniqueEntityByName`, `GetEntityCountByPartName`), template entity registry (`RegisterTemplateEntity`, `GetTemplateEntityByName`), `Trigger`, `ClearRegisteredEntities` ("Dynamic" / "Spatial"), `Shutdown`. It also exposes `GetAccessToEntityAdmin`, `GetDynamicEntityContext(i)` and the count, used by the save system.
* `eCEntityAdmin`: kill list and killed-entity callback (`KillEntities`, `AddKilledEntity`, `RemoveEarlyKilledEntity`, `SetKilledEntityCallback` set by `gCWorld::OnActivate`), `OnEntityProcessingChanged`, processing-range lists (F.5b).

## F.8 Rust port implications (world and streaming)

* Mirror the data model: workspace -> project -> world (version 36 file, `Common`/`Sector.List`) -> sectors (geometry layers = CStat cells, dynamic layers = lrentdat) and template database layers (`.tple`). The existing VFS/importers already cover most of the file formats; what is missing is the enabled-flag persistence per sector and "SysDyn" system layer handling.
* Streaming: implement a 2D cell grid with 10 000-unit cells, rectangle = camera +-0.4 x far plane, plus one cell margin, with hysteresis `max(1300, 0.2-0.4 x radius)`; cache out geometry contexts that leave the rect unless hybrid; prefetch on a worker thread. Add a second, coarse grid (34 000 cells) for far proxy geometry if you build a low-poly world.
* Separate "geometry cached in" from "entity in processing range": the ROI sphere is 4000 (Entity.ROI) with a 250 hysteresis, entity-specific `ProcessingRange` boxes decide entry. NPC simulation, AI and navigation hooks should key off enter/exit events, and the same events must reach scripts (`OnEnterProcessingRange`, `OnExitProcessingRange`).
* Savegames: if binary save compatibility is ever wanted, the format is GENOMFLE container + header magic GENOMESG with version u16 175/176, four world version numbers, MD5 signature seeded with a fixed string, and per-layer dynamic-context blobs plus interaction states for hybrid static entities. For the port itself, a new format is easier; keep the same split (header with screenshot, world blob, per-layer entity blobs, sector enabled flags, interaction states) and keep a version u16 and per-property-set relevance flags.
* Caches: a budgeted LRU with 85%/105% bands, reference-count-1 eviction priority, per-type budgets (image 132 MB (336 extended), mesh 88 MB, sound 21 MB, collision 25 MB, animation 37 MB), and worker thread pools of size up to 10. Bevy asset server plus a manual budget layer is enough; do not unload resources that are referenced.
* Startup: keep the phase order (config -> engine/graphics/caches -> modules init -> post-init -> workspace -> app-init -> session start -> create camera+player -> `OnInit`/`OnGameStartUp` -> goto start position / loaded position). Session start must refuse to run with physics disabled.

---

# Open items [U]

* Body of `CorrectCameraPosition` (camera collision) and exact semantics of the dialog `Min/MaxRange` window.
* The exact per-field mapping of the three first-person presets to combat states, and `gECameraMode` value usage by every script call.
* What produces movement modes 9 and 0xb; precise Swimming vs Diving discrimination (depth margins).
* Whether the game overrides the default ROI/PVS radii at runtime beyond the ini value, and the registration order of engine modules.
* Which world field the savegame "version below 0x6b" check reads, and what happens on a header MD5 mismatch (the decompilation continues after the compare, suggesting it is advisory).

# Gothic 3 original UI stack (Engine / GUI / GFC / Game DLLs)

Scope: how the shipped 1.75.14 game builds, loads, lays out, draws and drives its UI. Written in
own words from the Ghidra pseudo-C under `.local/research/engine/` plus byte-level checks of
`gui.pak`, `ge3.INI` and the user's `UserOptions.ini`. No decompiled bodies are reproduced.
Builds on `docs/research/UI_ORIGINAL.md` (art, screen geometry, colours) and does not repeat it.

Evidence legend: **[V]** verified in code or in real bytes, **[I]** inferred from structure/names,
**[U]** unknown. Addresses are VAs (image base 0x10000000, so address = VA). DLL in brackets.

Temporary artefacts written while researching (all under `.local/research/engine/out/`, gitignored):
`theme_g3.thm` (extracted), `parse_thm.py` (independent parser that consumes the whole file to the
last byte), `thm.json` (parsed result), `g3.gui.res` (extracted), `parse_res.py`, `res_dialogs.json`
(dialog -> C++ class table), `res_ids.json` (ResID -> ResName), `pe.py` (tiny PE reader used to
read pointer tables out of the DLLs).

---------------------------------------------------------------------------------------------------

## 1. Layering of the GUI stack

Five layers, strictly downward calls; the upper layers never see Win32 windows (the whole toolkit is
a software GUI drawn through D3D9 sprites).

| Layer | DLL | Main classes | Role |
|---|---|---|---|
| Game HUD | Game | `gCGUIManager`, `gCGUIModule`, `gCHUD*` (about 170 classes), `gCGUIEntityRenderer`, `gCInventoryStack` (icons) | Game-specific pages/views/menus and their binding to gameplay objects |
| GFC | GFC | `CFFGFCWnd`, `CFFGFCView`, `CFFGFCDialog`, `CFFGFCListCtrl`, `CFFGFCTabCtrl`, `CFFGFCStatic`, `CFFGFCButton`, `CFFGFCSlider`, `CFFGFCProgressBar`, `CFFGFCBitmap`, `CFFGFCImageList`, `CFFGFCDeviceContext`, `CFFGFCDataExchange` | MFC-style C++ wrappers (message maps, DDX, `OnInitDialog`, `Create(resourceId, parent)`); each wrapper holds a handle to an `IFFGUI*` interface object living in GUI.dll |
| FFGUI | GUI | `CFFGUIModule`, `CFFGUIWindow`, `CFFGUIDesktop`, `CFFGUIView`, `CFFGUIDialog`, `CFFGUIStatic`/`Button`/`CheckBox`/`RadioButton`/`EditBox`/`ListBox`/`ListCtrl`/`HeaderCtrl`/`TabCtrl`/`ProgressBar`/`Slider`/`ScrollBar`/`MenuBar`/`AnimateCtrl`, `CFFGUITheme`, `CFFGUICursor`, `CFFGUIBitmap`, `CFFGUIImageList`, `CFFGUIDeviceContext`, `CFFGUIParserNode` | The actual widget toolkit: window tree, message dispatch, theme-driven drawing, resource-XML parsing. About 2.7k functions, almost all tiny COM-like thunks (interface vtables) |
| Engine GUI adapter | Engine | `eCGUIModule`, `eCGUIAdmin`, `eCGUISubAdmin`, `eCGUIDatabase`, `eCGUIRenderer`, `eCGUIStreamer`, `eCThemeAdmin`, `eCImageAdmin`/`eCImageGroup`, `eCFontAdmin`/`eCGfxFont`/`eCDX9Font`, `eCLocAdmin`/`eCLocString`, `eCCursor` | Implements the abstract services FFGUI needs (renderer, font+texture database, theme streamer) on top of the engine's D3D9 mixer, and owns module life cycle |
| Engine core | Engine | `eCGfxSprite`, `eCGfxMixerAdmin`, `eCArchiveManager`/virtual file system | Sprite batches, viewport, file access |

Key facts:

* **[V] eCGUIModule is an engine component** (vftable `eCEngineComponentBase` plus `IFFGFCClient`).
  Its constructor (0x100df070 [Engine]) embeds an `eCGUIAdmin` at +0x18 which is itself a composite of
  `eCGUIDatabase` (+0x1c), `eCGUIRenderer` (+0x24) and `eCGUIStreamer` (+0x84); `eCGUISubAdmin`
  (6 tiny functions, 0x100e2100..) only hands out the three sub-objects. Module life cycle:
  `Initialize` 0x100dfc60, `PostInitialize` 0x100dfc80, `Shutdown` 0x100dfdf0, `PreShutdown` 0x100dfd90.
* **[V] The engine reaches FFGUI through a single singleton.** Every `eCGUIModule` method that touches
  UI (`EnableInput` 0x100df270, `ShowCursor` 0x100df360, `ActivateTheme` 0x100df220, `HandleMessage`
  0x100dfb70, `Draw` 0x100df530, `ReSize` 0x100df5c0) calls `CFFGUIModule::GetInstance()` and then a
  vtable slot on the `IFFGUIModule` sub-object (slot offsets 0x14 enable-input, 0x1c/0x20 begin/end draw,
  0x24 handle-message, 0x54 show-cursor, 0x58 set-size, 0x94 read-resources, 0xbc/0xc4 select/activate theme).
  `PostInitialize` registers the `eCGUIAdmin` as the toolkit's admin interface (slot 0x4), sets size and
  finally reads resources.
* **[V] PostInitialize order** (0x100dfc80): log "Creating GUI", copy the application's "keep aspect"
  flag (application+0x281) into module+0x9e, zero the aspect padding, `eCGUIDatabase::Create`
  (registers default fonts, see section 9), `eCGUIRenderer::Create`, hand admin to FFGUI, enable draw,
  mark module active (+0x8c), `ReSize`, set input, then `LoadResources` (g3.gui.res).
* **[V] Frame loop.** `eCGUIModule::Render` 0x100df380 returns early if the application is in editor mode
  (application vslot 0x26c) or the module is inactive, else `Draw` 0x100df530 brackets FFGUI's
  begin/draw/end calls with `bCTimer` time stamps (GUI cost counter at +0x90, active when a profiler
  flag is on). The Game's own overlay `gCGUIModule::Draw`/`OnPostRender` (0x1007a560 / 0x1007a550 [Game])
  lazily creates `gCGUIEntityRenderer` and calls `DrawFocusNames` (floating names, section 7.5).
  Both are skipped when engine-setup flag +0x1ac ("hide GUI") is set.
* **[V] Input path.** Win32 messages -> `eCGUIModule::HandleMessage` -> (if mouse scale on) coordinates
  converted to virtual space by `ToVirtualCoords` 0x100dfbd0 -> `IFFGUIModule` handle-message ->
  FFGUI window tree -> GFC `WindowProc`/`MsgProc` (0x10003920 / 0x100037e0 [GFC], a 2.8 KB switch) ->
  virtual handlers on HUD classes. **[V]** Message numbers copy the Win32 common-control values
  (list controls are queried with `0x100e` = LVM_GETITEMRECT; ESC is read as WM_KEYUP 0x101 with key
  0x1b; custom game messages 0x8001 = QuickSave, 0x8002 = QuickLoad reach `gCGUIManager::HandleMessage`).
* **[V] GFC wrappers are address-adjusted COM-style calls**, hence the decompiler noise of
  `*(vtable + 0x118)(...)` in HUD code. HUD classes override message-map style virtuals by slot number:
  slot 0x108 = `OnInitDialog`, 0x110 = on-show, 0x114 = on-hide, 0x118 = set-entity/refresh(entity, mode),
  0x11c = re-layout/refresh, 0x120 = back/close request (returns true if it consumed it), 0x124 =
  page-state notify(page, previousPage); 0x128/0x12c/0x130 on list controls = draw-amount / draw-slot-number /
  draw-flag-overlay helpers (section 8). [I] for the semantic names, [V] for the call patterns.

**Rust port implications**
* Do not port FFGUI/GFC literally. Keep the data model (resource tree with class/placement/style/id,
  theme atlas, image groups) and implement it in the existing `crates/ui` (which already has atlas.rs/hud.rs/screens.rs).
* The HUD layer is a thin binding layer: model each page/view as data (resource id + role) plus a small
  Rust "controller" that reads game state. The deep C++ class tree has no value to copy.
* The only engine-level behaviours worth preserving exactly: virtual 1024x768 space with aspect padding,
  theme-driven default widgets (needed for modal boxes and options), D3D-style text drawing flags (Win32 DT_*).

---------------------------------------------------------------------------------------------------

## 2. Resources and bootstrap

### 2.1 Files involved
| File | Reader | Notes |
|---|---|---|
| `gui.pak: resources/g3.gui.res` | `eCGUIModule::LoadResources` 0x100df840 -> `CFFGUIModule` read-resources (vslot 0x94) | XML; 62 dialogs, 424 controls (see 2.3) |
| `gui.pak: themes/g3/compiled/theme_g3.thm` + `theme_g3_00.dds` | `eCThemeAdmin::LoadCompiledTheme` 0x1010fce0 | compiled GENOMFLE descriptor + 2048x2048 DXT5 atlas (section 3) |
| `Library.pak: g3.igl` | `eCImageAdmin::LoadGroupList` 0x10108d70, called from `gCGUIManager::Create` 0x1007b220 [Game] | image group list = item/HUD icon and background atlases (lead decoded it; `out/parse_igl.py`, `out/igl_groups.json`) |
| `Data/Strings/stringtable.*` | `eCLocAdmin::Read` 0x102aa170 | all UI text (section 9.3); existing `crates/formats/src/stringtable.rs` |
| `Ini/ge3.INI` + `Documents/gothic3/UserOptions.ini` | `eCVirtualFileSystem::SetRootPath` 0x1029c350 (merge, `IgnoreUserOptions`), `gCGameApp::OnInitializeEngine` 0x1000c980 | option storage (section 11) |
| `Ini/log.ini` | `gCGUIManager::ImportLogSettings` 0x1007cf10 / `ExportLogSettings` 0x1007d570 | message-log filters ("Enabled" per category) |

### 2.2 g3.gui.res loading [V]
`LoadResources` opens the resource path (a global string at 0x10607430 [Engine]) through the archive
manager, handles the GENOMFLE string-table wrapper if present (the shipped `.res` is plain XML, so the
legacy "no magic -> raw bytes" branch runs), reads the whole file into a string and hands it to
`CFFGUIModule::ReadResources`. Failure only logs a warning. `LoadResources` runs exactly once, at the end
of `PostInitialize`, so all dialogs exist before the Game's `gCGUIManager::Create`.

Document shape (also see UI_ORIGINAL.md): `<resources>` with `<user NextID>`, `<controls>` (every control
ever drawn, flat), `<dialogs>` (62 top-level `window` nodes, each with nested `window` children = the same
controls re-listed with their dialog-local placement), `<bitmaps/>`, `<cursors/>`, `<themes/>` (the last three
are empty in the shipped file: **[V]** no theme/cursor/bitmap definitions come from XML).

### 2.3 Attribute vocabulary (read by `CFFGUIParserNode` + window factory) [V]
Per `window` node: `GUI.Window.Class` (factory key: Static, Button, CheckBox, RadioButton, EditBox, ListBox,
ListCtrl, HeaderCtrl, TabCtrl, ProgressBar, Slider, ScrollBar, MenuBar, AnimateCtrl, Desktop, Dialog, View),
`GUI.Window.Name` (initial text; for statics also the German design text), `GUI.Window.Style` (bit field),
`GUI.Window.ID`, `GUI.Window.Placement` ("l,t,r,b" parsed with `%d,%d,%d,%d`; the loader subtracts to x,y,w,h),
`GUI.Window.Anchor`, `GUI.Window.Dock`, optional `GUI.Window.Font.*` (Face, Height as unsigned 32 of a negative
number, Weight, Italic, CharSet, Quality, OutPrecision, PitchAndFamiliy [sic], Width), and class specific keys:
`GUI.Static.Bitmap`, `GUI.Static.TextColor`, `GUI.View.Bitmap`, `GUI.ProgressBar.{Bitmap,Color}.{Bar,Track}`
`/Margin/BarBelowTrack/Marquee.*`, `GUI.TabCtrl.{ItemSize,ItemSpacing,ItemPadding,MinTabWidth,IconSize,IconFill,Item.*}`,
`GUI.ListCtrl.{View,FullRowSelect,HotTrack,OneClickActivate,DrawEmptyItems,*Icon/List/Tile.Size/Spacing}`,
`GUI.AnimateCtrl.Video`. GFC-only keys on the same node (consumed by the generator, not the toolkit):
`GFC.Class`, `GFC.BaseClass` (CFFGFCView or CFFGFCDialog), `GFC.Variable` (C++ member name), `ResID`, `ResName`.
Interesting gotchas:
* The window factory (0x1000ed70 / 0x10001480 [GUI]) clears style bit 28 (0x10000000) when constructing, and GFC
  `Create` calls pass 0x10000000 again, so bit 28 is "visible" exactly as Win32 WS_VISIBLE. **[V]** for the mask, **[I]** meaning.
* Static style low bits observed in the file: 0x01 centred text, 0x04 vertically centred, 0x10 draws the bitmap
  (`GUI.Static.Bitmap`), 0x20 right aligned ("value" cells of the character sheet). **[I]** (the subtitle code toggles bit 1 at run time).
* Image references in XML are file paths like `\icons\misc\g3_hud_icon_misc_compass.tga`; resolution goes through the image
  group list by **base file name only**, case-insensitive (section 2.5).
* `ResName`/`ResID` pairs: dialogs 30000+, controls 20000+. The C++ code instantiates views by numeric resource ID
  (`CFFGFCWnd::Create(resId, parent)`), so the numbering is load-bearing: e.g. 0x755c = `IDD_PAGE_MAIN_2` (30044),
  0x7564 = `IDD_PAGE_INVENTORY_2`, 0x7575 = `IDD_VIEW_TALK_2` (the dump `out/res_ids.json` has all 62).

### 2.4 Bootstrapping order in the Game [V]
1. Engine: `eCGUIModule::PostInitialize` (above) creates database/renderer, loads g3.gui.res.
2. Game: `gCGUIManager::Create` 0x1007b220 (only when not in editor mode and not yet created):
   (a) `eCImageAdmin::LoadGroupList("g3.igl", ...)`;
   (b) allocates and builds two top-level GFC dialogs: the **"Menu" dialog** `gCHUDDialogMenu2` (0x70 bytes, pointer at
   manager+8) and the **"HUD" dialog** `gCHUDDialogMain2` (0xb0 bytes, pointer at manager+4). Both are created with the
   visible style 0x10000000 over the desktop client rect and are immediately hidden;
   (c) publishes the menu dialog pointer in a global used by file/save screens; (d) imports/exports `ini/log.ini`.
3. Theme: the `g3` compiled theme is loaded by `eCGUIStreamer::LoadTheme` 0x100e1840 (create theme in `eCThemeAdmin`,
   try `LoadCompiledTheme`, fall back to source import+compile, then push to FFGUI via `UpdateTheme` 0x100e1b50 which copies
   cursors/elements into `IFFGUITheme`). **[U]** where the string "g3" is chosen (not in the XML, not in any Game string; it is
   the only theme in gui.pak).
4. `gCHUDDialogMain2::OnInit` (0x1008aa40 [Game]) builds the persistent in-game layer, see section 5.

### 2.5 Image lookup (g3.igl) [V]
`eCGUIAdmin::LoadBitmapA` 0x100def40 -> `eCImageAdmin::FindImageIndex` 0x10109850. Resolution order: special
name `bink` = video placeholder; then every `eCImageGroup` in list order (`eCImageGroup::FindImageIndex` 0x1010b4a0 compares
`CleanImageName` results: **path stripped, file name + extension kept, case-insensitive**); then the list of ad-hoc temp
textures; finally `CreateTempTexture` which tries to load a loose image by path. The result carries (atlas texture index,
source rect, UV rect) and a white modulation colour. Consequences:
* Lookup keys are file base names such as `IT_1H_OrcSlayer_01.tga`, `G3_HUD_Marker.tga`, `G3_HUD_Logo_Start.tga`.
* There are 13 groups in the shipped list (9 item groups of 1024x1024 atlases with 64 px cells, plus `hud_icons`,
  `hud_backgrounds`, `hud_maps`, `hud_slides`); same-named images in an earlier group shadow later ones.
* The `texture` field in a tile is the DDS atlas path; the tile rect (l,t,r,b in atlas pixels, inclusive-exclusive as exported) is the
  source rectangle.

**Rust port implications**
* Build a `HashMap<lowercase base name, (atlas, rect)>` from igl at load; drop directory parts when resolving gui.res
  `GUI.*.Bitmap` attributes and item `Texture` strings. Keep the first-wins rule.
* Treat `bink` images as "video placeholder"; AnimateCtrl uses it for intro and credits.

---------------------------------------------------------------------------------------------------

## 3. Compiled theme `theme_g3.thm` (field by field)

Source functions [Engine]: `eCThemeAdmin::ReadCompiled` 0x10111ab0 (container), `Read(STheme)` 0x10111f40,
`Read(SMetrics)` 0x10112090, `Read(SCursor)` 0x10112130, `Read(SElement)` 0x101121b0, `Read(SFont)` 0x10112320,
writer mirrors at 0x101123d0..0x10112610. Everything below was cross-checked by parsing the real file
(53 510 bytes) with an independent parser that ends exactly at the footer offset. **[V]** unless marked.

All integers little endian. "str" = a **u16 index into the footer string table** (this is how `bCString`
serialises inside a GENOMFLE container whose footer flag is 1).

### 3.1 Container
| Offset | Size | Value (real file) | Meaning |
|---|---|---|---|
| 0x00 | 8 | `GENOMFLE` | magic (u32 0x4F4E4547 + u32 0x454C464D) |
| 0x08 | 2 | 1 | container version |
| 0x0A | 4 | 0xA0C1 | absolute offset of the footer |
| 0x0E | ... | | main stream (theme data, section 3.2); ends exactly at footer offset |
| footer | 4 | 0xDEADBEEF | marker (loader treats a mismatch as fatal error) |
| +4 | 1 | 1 | string-table mode flag (1 = strings in table; the loader's else-branch reads inline strings) |
| +5 | 4 | 165 | string count |
| +9 | ... | | count x { u16 byteLength; bytes (Latin-1/ASCII, no terminator) } |

String table content (real file): `[0]="g3"`, `[1]="/data/gui/themes/g3/compiled//theme_g3_00.dds"` (atlas, note the double slash),
`[2..164]` = the 163 element source bitmap paths in element order (e.g. `/data/gui/themes/g3/elements/theme_g3_element_brush_white_solid.bmp`).
Strings are not de-duplicated by content beyond what the writer did; the atlas string is index 1 and referenced by all 7 cursors and 163 elements.

The same footer layout is used by `g3.igl` (and by stringtable binary and other GENOMFLE archives); a single Rust helper
`read_string_table_footer()` can serve all of them.

### 3.2 Main stream
```
u32  version            = 1                       (STheme)
str  themeName          = "g3"
i32  legacyGroupIndex   = 10                      (read into a local and discarded by the loader)
-- metrics --
u32  version            = 1
u32  count              = 58                      (loader rejects anything else)
i32  metric[58]                                    raw dump, order in 3.3
-- cursors --
u32  count              = 7                       (loader rejects anything else)
7 x SCursor
-- elements --
u32  count              = 163                     (loader rejects anything else)
163 x SElement
```
The stream is consumed exactly up to the footer offset (0xA0C1) after the 163rd element, which is a good self-check for a parser.

**SCursor** (fixed 42 bytes: 4 + 2 + 16 + 12 + 8)
```
u32  version = 1
str  texture                      (atlas path, string index 1)
i32  left, top, right, bottom     (source rect in the 2048x2048 atlas; all cursors are 128x128)
12 B color                        = u32 junk(0x101B0088) | u8 r,g,b,0 | u32 alpha     (see 3.5)
i32  hotspotX, hotspotY           (all 0 in the shipped theme)
```
Cursor order (name table at 0x104267bc [Engine]): 0 Pointer, 1 Grab, 2 Wait, 3 HSize, 4 VSize, 5 TLSize, 6 TRSize.
Real rects: Pointer (299,303,427,431) tint white a=255; Grab same rect a=127; Wait (559,303,687,431) white a=255;
HSize (689,303,817,431) tint red a=127; VSize (819,303,947,431) tint green a=127; TLSize (1878,1,2006,129) tint yellow a=127;
TRSize (429,303,557,431) tint cyan a=127. [I] the coloured ones are placeholder resize cursors drawn tinted; the in-game cursor is the Pointer.

**SElement** (variable size: 4+2+2+16+4+4+4 + 6*24 + (font? 4+2+len+32+1))
```
u32  version = 2
str  sourceBitmap                 (e.g. ".../theme_g3_element_static_text.bmp"; the element identity is the table position, not this string)
str  atlasTexture                 (always string index 1)
i32  left, top, right, bottom     (source rect in the atlas)
i32  fontRef                      (>= 0: an SFont follows this element; value is the writer's font index, 26 or 27 here; the loader
                                   re-creates the font and overwrites it with the runtime font index)
u32  format                       (text format flags = Win32 DT_* : 0, 4 = DT_VCENTER, 5 = DT_CENTER|DT_VCENTER, 36/37 add DT_SINGLELINE 0x20)
u32  stateCount = 6               (loader rejects anything else)
6 x { 12 B fontColor ; 12 B textureColor }       state order: Normal, Disabled, Hidden, Focus, MouseOver, Pressed
[if fontRef >= 0] SFont
```
Distribution in the real file: format 5 x100, 0 x42, 4 x13, 37 x6, 36 x2. Element 32 (`Static.Text`) has the body text colour
`#E0C495` (matches the reference screenshots) in its Normal font colour.

**SFont**
```
u32  version = 1
u16  byteLength ; UTF-16LE face name        ("Times New Roman" for all elements)
i32  height     (negative = em pixels: -16 for 161 elements, -18 for 2 button elements)
i32  width      (0)
i32  weight     (700)
i32  charSet    (1 = DEFAULT_CHARSET)
i32  outPrecision (0)
i32  quality    (0)
i32  pitchAndFamily (0)
i32  mipLevels  (1)
u8   italic     (0)
```
Fonts are de-duplicated by descriptor in `eCThemeAdmin` (`FindFontIndex` 0x1010f1c0, `CreateFontA` 0x1010f270 -> `eCGfxFont` D3DX font), so
the file has only 2 distinct fonts (-16 and -18 Times New Roman bold).

### 3.3 Metrics (58 x i32), names from the pointer table at 0x10426900 [Engine]
Index: name = real value.
0 Screen.X = 1280, 1 Screen.Y = 1024, 2 Cursor.X = 32, 3 Cursor.Y = 32, 4 Border.X = 1, 5 Border.Y = 1, 6 Frame.Fixed.X = 3,
7 Frame.Fixed.Y = 3, 8 Frame.Size.X = 4, 9 Frame.Size.Y = 4, 10 Menu.Y = 30, 11 Caption.Y = 29, 12 Caption.Icon.X = 26, 13 Caption.Icon.Y = 26,
14 Caption.Button.X = 26, 15 Caption.Button.Y = 26, 16 VScroll.Track.X = 17, 17 VScroll.Thumb.Y = 17, 18 VScroll.Arrow.Y = 17, 19 HScroll.Track.Y = 17,
20 HScroll.Thumb.X = 17, 21 HScroll.Arrow.X = 17, 22 CheckBox.Box.X = 17, 23 CheckBox.Box.Y = 17, 24 RadioButton.Box.X = 17, 25 RadioButton.Box.Y = 17,
26..55 = Border.{Window,Desktop,Dialog,Frame,Control,Static,Button,CheckBox,RadioButton,Slider,ProgressBar,ScrollBar,ListBox,EditBox,ToolBar}.{X,Y}
(15 pairs, all 3 in the shipped theme; `Border.Window.X/Y` = 26/27 ... `Border.ToolBar.X/Y` = 54/55), 56 Menu.Icon.X = 16, 57 Menu.Icon.Y = 16.
Note the cursor draw size (32x32) lives here, not in the cursor record; **[I]** `Screen.X/Y` is only the compile-time default.

### 3.4 Element table (position = identity; names from 0x104269e8 [Engine], 163 entries; verified 163/163 against the file's bitmap names)
Each visual part of a widget has a `.Back` and a `.Front` element (two draw layers); position ranges:

| Idx | Family | Idx | Family |
|---|---|---|---|
| 0 | Brush.White.Solid (2x2 white texel used for flat fills) | 61-72 | MenuBar.MainMenu/PopupMenu Bar/Item/Arrow.Right/Check |
| 1-8 | Border.Window TopLeft..BottomRight | 73-80 | ProgressBar.Hor/Ver Progress/Track |
| 9-16 | Border.Client (same eight) | 81-96 | ScrollBar.Hor/Ver Arrow/Thumb/Track |
| 17-28 | Caption Bar, Icon, Button.{Close,Minimize,Maximize,Restore} | 97-104 | TrackBar.Hor/Ver Thumb/Track |
| 29-31 | Window.Back, Dialog.Back, Desktop.Back | 105-112 | ToolBar.Hor/Ver Bar/Button |
| 32 | Static.Text | 113-120 | HeaderCtrl Main/Item/Arrow.Up/Down |
| 33-42 | Button.{Button,CheckBox,RadioBox,Check,Radio} Back/Front | 121-130 | ListCtrl Main + Item.Details/Icons/List/Tiles |
| 43-46 | EditBox Main/Text | 131-158 | TabCtrl Hor/Ver Bar, Button, Tab L/C/R (T/C/B), Arrows |
| 47-50 | ListBox Main/Item | 159-162 | ToolTipCtrl Main/Item |
| 51-60 | ComboBox Edit/List/Arrow.Down | | |

Every element has all 6 state colour pairs; the art itself is a single atlas rect per element (state variation is by tint colour and, for
checkable controls, by the toolkit choosing `Back`/`Front`).

### 3.5 The 12-byte colour object [V]
`bCByteAlphaColor` is serialised by dumping the in-memory object: 4 bytes of vtable pointer (always 0x101B0088, the `bCByteAlphaColor` vftable in
SharedBase -> **ignore on read, write the same constant**), then `r, g, b` bytes + 1 pad byte, then a u32 whose value is the alpha 0..255.
Evidence: element 32 font colour bytes `E0 C4 95 00` = the known body text colour; alpha values seen: 0x0A, 0x78, 0x80, 0x96, 0xC8, 0xD2, 0xFF.

### 3.6 Loading semantics (what the engine does after reading)
`LoadCompiledTheme` (0x1010fce0): for each of the 7 cursors and 163 elements it calls `eCImageAdmin::ImportImage(groupIdx, textureString, rect, ...)` to
register the rectangle in the atlas group, so the theme atlas is just another image group. The toolkit then draws each element by stretching /
nine-slicing its rect (borders have fixed sizes from the metrics). The theme atlas `theme_g3_00.dds` is 2048x2048 DXT5 (UI_ORIGINAL.md).
Paths are derived in `GetThemePaths` 0x1010eb70 from the theme name: `/data/gui/themes/<name>/compiled/theme_<name>.thm` and `..._00.dds` (lower-cased).

**Rust port implications**
* A ~120 line parser is enough: footer string table -> main stream as above; expose `Theme { metrics: [i32;58], cursors: [Cursor;7], elements: [Element;163] }`.
  Add the element and cursor name tables as `const` arrays (they are not in the file).
* Reject files whose counts differ from 58/7/163 and whose per-state count is not 6, as the original does.
* The colour object parser must skip 4 bytes then read rgb(+pad) and u32 alpha; do not interpret the first dword.
* Use synthetic fixtures (hand-built stream with 1 cursor) for tests; the real file must stay under `.local/`.
* The default modal boxes and the options screens can be drawn from this theme + `Times New Roman` bold; the in-game panels do not use theme elements (they use icon-group backgrounds).

---------------------------------------------------------------------------------------------------

## 4. Coordinates, virtual resolution and scaling

Functions (all [Engine] `eCGUIModule`): constructor 0x100df070, `SetVirtualResolution` 0x100df330, `GetVirtualResolution` 0x100df2e0,
`EnableDrawScale` 0x100df2c0, `EnableMouseScale` 0x100df2a0, `ReSize` 0x100df5c0, `ToVirtualCoords` 0x100dfbd0, `Invalidate` 0x100df1c0.

* **[V] Defaults:** virtual size (module+0x94/+0x98) = 1024 x 768; draw scaling (+0x9c) on; mouse scaling (+0x9d) on; auto aspect (+0x9e, copy of the
  application's "keep aspect" setting = ge3.INI `GUI.AutoAspect=true`) on. Padding (+0xa0/+0xa4) 0.
* **[V] SetVirtualResolution** only has an effect when auto aspect is OFF. **EnableDrawScale(false)** is ignored while auto aspect is ON.
* **[V] ReSize** (called after device init, on resize and from `DoRender`): reads the D3D viewport (falls back to 800x600 if no device).
  - draw scaling off: toolkit size = viewport, render scale 1.
  - draw scaling on, auto aspect off: toolkit size = virtual (1024x768); renderer scale = viewport / virtual per axis (anisotropic stretch).
  - draw scaling on, auto aspect on: compute the ratio between viewport aspect and virtual aspect. If the viewport is wider than 4:3 the module
    adds extra **virtual width** (+0xa0), if taller it adds extra **virtual height** (+0xa4), so the toolkit size becomes (1024 + padX, 768 + padY) and the
    renderer scale becomes viewport / (virtual+pad) -> uniform scale, no stretching, extra space at the right/bottom of the layout. **[I]** exact rounding.
    `GetVirtualResolution` returns the padded size.
* **[V] Mouse:** `ToVirtualCoords` only converts for mouse messages 0x200..0x20D, via the renderer's inverse of the scale vector (and is skipped when
  mouse scale is off).
* **[V]** The game's own screens are authored for 1024x768; the HUD dialogs get their rectangle from the desktop client rect minus margins (0x1008b160 / 0x1008bbf0).
  A page with `Anchor=5` and 1024x768 placement therefore fills the (possibly padded) toolkit rectangle; individual HUD views are placed with absolute 1024x768
  coordinates. **[U]** whether widescreen shifts them (the view classes read no virtual size) -- in the reference widescreen setups the layout stays top-left.
* Draw: the renderer works in sprite space with `eCGfxSprite::Draw`; `DrawSprite` 0x100e1450 resolves the texture node via `eCGUIDatabase::GetTextureNode`
  and applies the current transform; `SetClipRect` 0x100e1690 uses the viewport scissor; `PushStates`/`PopStates` 0x100e1110/0x100e12f0 save viewport, scissor and render states.

**Rust port implications**
* One `UiScale { virtual: (1024,768), pad: (px,py), scale: f32 }`; pad extends only the axis that is longer than 4:3, scale = min(viewport/virtual) for uniform scaling.
  Mouse input is divided by the same scale. Keep every layout number in the 1024x768 space (as `crates/ui` already does).
* Decide explicitly where wider-than-4:3 extra space goes (the original appends it right/bottom of the layout; anchoring is a deliberate improvement, not parity).

---------------------------------------------------------------------------------------------------

## 5. Game GUI object model and page handling

### 5.1 gCGUIManager (the single entry point for gameplay) [V]
Singleton owned by the session layer; layout: +4 HUD dialog (`gCHUDDialogMain2`), +8 Menu dialog (`gCHUDDialogMenu2`).
| Function | Address [Game] | Behaviour |
|---|---|---|
| `Create` | 0x1007b220 | see 2.4 |
| `OpenMenu` / `CloseMenu` / `IsMenuOpen` | 0x1007c2e0 / 0x1007c380 / 0x1007c2c0 | Menu dialog has 14 child pages (array at +0x10) and a current-page field at +0x48 (-1 = closed). Opening plays a 500 ms animate on the HUD logo, resets the page to 0 (main menu), shows and raises the window. Closing hides it, notifies the open sub page ("back" slot), sets every page's state, then restores gameplay input (below) |
| `OpenInventory/Spells/Skills/Quests/Book/Letter/Map` | 0x1007b5d0 / 0x1007b660 / 0x1007b6f0 / 0x1007b780 / 0x1007b810 / 0x1007b8f0 / 0x1007b9d0 | select the corresponding HUD page, set its entity (the player) via slot 0x118(entity, mode). Book/Letter/Map pass an extra mode argument 4/5/3 to the documents/inventory page = which sub view to show |
| `OpenDialog` | 0x1007b530 | page 6 |
| `BeginTrade` / `BeginLoot` | 0x1007bfd0 / 0x1007c0b0 | pages 5 / 7; the entity argument goes to the page twice (player inventory + partner) |
| `OpenAlchemy/Cooking/Smithing` | 0x1007bab0 / 0x1007bb90 / 0x1007bc70 | pages 8 / 9 / 10 (synth) |
| `OpenBless/Poison/Sharpen/Fry` | 0x1007bd50 / 0x1007bdf0 / 0x1007be90 / 0x1007bf30 | pages 11-14 (transform) |
| `OpenTutorial` / `ShowPicture` | 0x1007c4b0 / 0x1007c550 | pages 15 / 16 |
| `ClosePage` / `CloseDialog` | 0x1007c1e0 / 0x1007c190 | back to "no page" (-1) |
| `ToggleHUD` / `ToggleQuickSlots` | 0x1007b3c0 / 0x1007b440 | flip two bool flags on the persistent main-page view (hide-all, hide-quickslots) and re-evaluate visibility; ToggleHUD also fades the logo layer in 500 ms |
| `ShowCrossHair` / `SetCrossHairSize` | 0x1007d970 / 0x1007d9b0 | 200 ms fade of the 32x32 crosshair view; size forwarded to `gCHUDCrossHair` |
| `StartSay` / `StopSay` / `ShowSubtitle` | 0x1007c500 / 0x1007c520 / 0x1007c5a0 | subtitle layer (section 7.6) |
| `PrintGameMessage` | 0x1007c5c0 | adds a line to the on-screen message area (7.7) |
| `HandleMessage` | 0x1007d9e0 | key ESC, QuickSave (0x8001), QuickLoad (0x8002) |
| `OnProcess` | 0x1007dea0 | each frame: if the player has a focus entity (gCFocus_PS 0x3b) it re-arms a short timer (0.2 s) and updates the focus bar; when the timer expires the focus bar fades |
| `UpdatePlayer` | 0x1007df60 | re-binds the HUD and the current page to the (new) player entity |

### 5.2 Input mode switch when a page opens/closes [V] (`FUN_1008b540` 0x1008b540 [Game])
Going from "no page" to any page: `eCGUIModule::ShowCursor(true)`, `EnableInput(true)`, session property `State` := 6 (a `gESession_State` value used
for GUI/pause) and the camera AI property-set flag at +0x203 := 1 (camera stops reacting to look input). Closing last page reverses all four.
`gCGUIManager::CloseMenu` does the same restoring (`Session::Resume`). [I] State 6 = "game suspended by GUI".

### 5.3 ESC handling [V] (0x1007d9e0)
ESC arrives as WM_KEYUP(0x1b): if the menu dialog is closed -> try to close the active HUD page (`FUN_1008b270`: asks the page's "back" slot, else deselects the tab and sets page -1; tutorial pages 15/16 are not closable this way) and if nothing was open -> `OpenMenu`.
If the menu is open: ask the current menu sub page to consume it (`FUN_1008c070`, e.g. a modal confirm), else `CloseMenu`.
QuickLoad (0x8002) is refused while dialog info state is running or a page is open; QuickSave checks `gCSession::IsSaveAllowed`; both print a localized message
(`HUD_QuickSave`, `HUD_File_SaveFailed`, `HUD_File_LoadFailed`, repeated 8 times as filler lines with message type 0xd = error).

### 5.4 HUD dialog (`gCHUDDialogMain2`) [V unless noted]
* `OnInit` 0x1008aa40: creates the persistent layer view from resource 0x755c (`IDD_PAGE_MAIN_2` = `gCHUDPageMain2`, pointer at +0x7c), then 17 page views at +0x38..+0x78
  (factory 0x1008ac00 [Game], a switch by page index), three logo statics (`G3_HUD_Logo_Start/Title/Version.tga`, class `gCHUDLogo`, +0x2c..+0x34), and the crosshair
  (`gCHUDCrossHair`, 32x32 at (8,8)).
* Page index table (index -> resource -> class): 0 `IDD_PAGE_INVENTORY_2` gCHUDPageInventory2; 1 `IDD_PAGE_SPELLS_2` gCHUDPageSpells2; 2 `IDD_PAGE_DOCUMENTS_2` gCHUDPageDocuments2;
  3 `IDD_PAGE_SKILLS_2` gCHUDPageSkills2; 4 `IDD_PAGE_QUESTS_2` gCHUDPageQuests2; 5 `IDD_PAGE_TRADE_2` gCHUDPageTrade2; 6 `IDD_PAGE_DIALOG_2` gCHUDPageDialog2;
  7 `IDD_PAGE_LOOT_2` gCHUDPageLoot2; 8/9/10 `IDD_PAGE_SYNTH_2` gCHUDPageSynth2 with a hotspot label `HL_AlchemyTable` / `HL_Cauldron` / `HL_Smithy`;
  11-14 `IDD_PAGE_TRANSFORM_2` gCHUDPageTransform2 (four crafting-transform variants; script call `OnPlayerTransformStack`);
  15 `IDD_PAGE_TUTORIAL_2` gCHUDPageTutorial2 (property `StatePosition`); 16 `IDD_PAGE_SLIDESHOW_2` gCHUDPageSlideshow2.
* `SetPage` (0x1008b340): ignores no-op changes; page 15 (tutorial) disables the layer below and remembers the previous page (+0x88) instead of replacing it; otherwise
  notifies old/new page (slots 0x114 hide / 0x110 show + 0x118 refresh for 7 entity slots), re-evaluates HUD visibility (`FUN_1008b4d0`) and stores the page (+0x84).
  For pages 0..5 it may open the one-time **tip window** (loc string from a 6-entry table, suppressed by the "no more tips" check box `HUD_NoMoreTips`; `FUN_1008b710` is the global
  tips-enabled flag). **[U]** the tip string keys (table filled at run time).
* Commands (0x1008b7c0): command ids 0x4fb0, 0x501b, 0x5025, 0x5026, 0x5043 = close-page buttons (deselect tab, page -1); 0x504b = continue after tutorial (re-enable layer, back to previous page).
  Notify (0x1008b900): tab control id 0x4faf selection change (code -0x227) switches the page to the tab item's data value.
* **Visibility table** (data at 0x102d31b4 / 0x102d31c0 [Game], 6 bool flags per page; read in `gCHUDPageMain2::OnPageChanged` 0x10088a30): flag0 = focus (NPC health) bar,
  flag1 = health, mana and endurance bars, flag2 = quickslot bar, flag3 = page-strip tab bar, flag4 and flag5 = the two message/log areas.
  Default when no page is open: 1,1,1,0,1,1. Pages 0-4 (inventory, spells, documents, skills, quests): 0,1,1,1,0,1 (bars + quickslots + page strip stay visible, focus bar and the first log hidden).
  Pages 5,7-13,16 (trade, loot, synth, transform, slideshow): all zero. Page 6 (dialog): only flag0. Pages 14 and 15: default set. `ToggleHUD` forces all six flags off;
  `ToggleQuickSlots` forces flag2 (quickslots) and flag4 off. Each change animates (AnimateWindow, 200 ms; Win32 AW_ACTIVATE/AW_HIDE/AW_BLEND-style flags 0x10010 / 0x90000 / 0xA0000 / 0x80000, i.e. fade or slide).
  Invalid page values (> 16) log `gCHUDPageMain2::OnPageChanged -> Detected invalid page value`.

### 5.5 Persistent layer `gCHUDPageMain2` (`IDD_PAGE_MAIN_2`) [V]
Initialisation (0x10088c60) creates seven child views: endurance 0x7561, mana 0x7560, health 0x755f, focus 0x7562, quickslots 0x755d, page strip 0x755a, talk/subtitle 0x7575.
The resource also holds statics for the compass (16,660)-(116,760) with bitmap `G3_HUD_Icon_Misc_Compass`, "Game Messages" (87,308)-(937,524) and "Log" (15,219)-(884,471) plus
placeholders for the bars (positions in UI_ORIGINAL.md). `vfn_70` (0x10088970) is "set entity": rebinds the three player bars to the entity proxy and refreshes the character static (0x100a2b90).

### 5.6 Menu dialog (`gCHUDDialogMenu2`) [V names, I roles]
Pages by resource name: Intro (`gCHUDMenuIntro2`, one AnimateCtrl = Bink intro video), Main (`gCHUDMenuMain2`, 10 buttons + 7 statics), Options (`gCHUDMenuOptions2`), Audio, Video, Controls, ControlsCustom (a
list control + button, a *dialog* not a view), Performance, PerformanceCustom, Difficulty (`gCHUDMenuDifficulty2`: radio buttons Easy/Medium/Hard, check boxes AIMode/ExtendedContent),
Files (`gCHUDMenuFiles2`: list + load/save/delete buttons), Credits (AnimateCtrl), Loading (progress bar + two statics), Confirm (482x143 modal with 3 buttons). The Menu dialog keeps the page index of the open sub page and raises it with "Open(pageId)".

**Rust port implications**
* Model the HUD as an explicit state machine: `ActivePage: Option<PageId>` (17 ids), `MenuPage: Option<MenuPageId>`, the six visibility flags per page as a data table,
  `hud_hidden` and `quickslots_hidden` toggles. Port the visibility table values verbatim (they are small).
* Page open => cursor visible + game input suspended + camera look disabled; close => reverse. Encode as one `InputFocus` resource instead of four calls.
* ESC rule: page -> close page; else menu; menu: sub page consumes first. Tutorial and slideshow pages are not ESC-closable.

---------------------------------------------------------------------------------------------------

## 6. HUD class catalogue (names, roles, game bindings)

(All classes in [Game] except GFC bases. A class "binding" is the gameplay object the class reads; property set ids are `eCEntity::GetPropertySet` ids, verified through each class's
`GetPropertySetType` constant: gCNPC_PS 0x1e, gCInventory_PS 0x1f, gCItem_PS 0x30, gCInteraction_PS 0x31, gCDamage_PS 0x33, gCDamageReceiver_PS 0x34, gCQuestManager_PS 0x35, gCQuest_PS 0x36,
gCInfoManager_PS 0x37, gCInfo_PS 0x38, gCFocus_PS 0x3b, gCPlayerMemory_PS 0x3c, gCLock_PS 0x45, gCLetter_PS 0x4f, gCBook_PS 0x50, gCMap_PS 0x51, gCMagic_PS 0x65, gCSkill_PS 0x66, gCRecipe_PS 0x6d.)

Naming pattern: `Menu*` = full-screen menu pages (Menu dialog), `Page*` = full-screen in-game layers (HUD dialog, thin containers that create views), `View*` = framed panels with a background bitmap
(listed in UI_ORIGINAL.md with positions), `ListCtrl*`/`Table*`/`TabCtrl*`/`Static*`/`ProgressBar*` = leaf controls, `Option*` = a slider-style option value object (not a window).

| Class (suffix 2 omitted) | Resource / region | Role and binding |
|---|---|---|
| gCHUDDialogMain2 / gCHUDDialogMenu2 | "HUD" / "Menu" dialogs | roots, 5.4 / 5.6 |
| gCHUDPageMain2 | persistent HUD layer | creates bars, quickslots, page strip, talk; visibility table; entity = player (and focus entity) |
| gCHUDPageInventory2 / Spells2 / Documents2 / Skills2 / Quests2 | pages 0-4 | each hosts the "character" view (left) + one list view (right) + item/skill detail view; tab strip index 0..4 selects them |
| gCHUDPageTrade2 / Loot2 / Dialog2 / Synth2 / Transform2 / Tutorial2 / Slideshow2 | pages 5-16 | see 5.4; Trade holds two inventories + transaction view; Loot holds loot pool + player inventory |
| gCHUDViewBarHealth2 / Mana2 / Endurance2 / Focus2 | 201x19 bars | contain a `gCHUDProgressBarNPC` (a CFFGFCProgressBar subclass with an entity proxy and a key string "HP" / "MP" / "SP"); Focus bar uses key "HP" of the focus entity |
| gCHUDViewBarQuickslots2 | (252,688) 520x56 | one `gCHUDListCtrlInventory` list (10 slots) rendering `gCHUDListCtrlInvQuickSlots` items |
| gCHUDViewBarPages2 | (946,76) 62x500 | `gCHUDTabCtrlPages` (5 page tabs) + exit button; tab data = page index |
| gCHUDViewBarSynthPages2 / TransformPages2 | same area | reduced strip for crafting |
| gCHUDViewCharacter2 | left panel 41 statics | attribute sheet: level/XP/next XP/learning points, HP/MP/SP etc. (`gCPlayerMemory_PS::GetAttribute` by name, `gCAttribute::GetLongName` for labels), 6 protection values via script `GetProtectionForHUD`, XP-next via script `GetXPNext`; labels `GO_Level`, `GO_XP`, `GO_XPNext`, `GO_LearningPoints`, headings `HL_Experience`/`HL_Attributes`/`HL_Armor` |
| gCHUDViewInventory2 / TradeInventory2 / LootInventory2 / SynthInventory2 / TransformInventory2 | right panels | category `gCHUDTabCtrlInventory` (5 tabs) + `gCHUDListCtrlInvStacks` / `InvTradeStacks` / `InvLootStacks`; gold bar `HUD_GoldAmount` (+ static `gCHUDStaticInvCash`) |
| gCHUDViewItem2 / Recipe2 / Skill2 / Spell2 | detail boxes | `gCHUDTableItemStats` / `TableRecipeStats` / `TableSkillStats` / `TableSpellStats` rows for the selected stack; title from `ITEMDESC_<name>` loc key; requirement lines `HUD_Req_*`, `HUD_Damage_*`, `HUD_Bonus_Skill`, `HUD_In_Inventory`, `HUD_Ingredients` |
| gCHUDViewDocuments2 / Document2 / Book2 / Letter2 | documents | list of documents (`gCHUDListCtrlInvDocuments`, tabs `gCHUDTabCtrlDocuments`, category static `gCHUDStaticDocCategory`); reader panels bind `gCLetter_PS` / `gCBook_PS` text (`eCLocString`) |
| gCHUDViewSkills2 / Spells2 | lists | 7 and 3 `ListCtrl` groups (skills by branch, spells by circle) with `gCHUDListCtrlInvSkills`/`InvSpells` drawing grey (unlearned) vs coloured icons |
| gCHUDViewQuests2 / Map2 / World2 | quest log | `gCHUDListCtrlQuests` + `gCHUDTabCtrlQuests` (Aktuell/Erfolgreich/Gescheitert), `gCHUDStaticQuest`, `gCHUDStaticQuestLocation`, reputation list (`HUD_Reputation`), time-of-day strip with four bitmaps (`G3_HUD_View_Quests_Time_Dawn/Day/Dusk/Night.tga`); empty strings `HUD_NoQuests`, `HUD_NoQuestSelected`, `HUD_NoEntries`; world map view = `gCHUDStaticMap` + 25 location buttons (`G3_HUD_Icon_Location_*`) as fast-travel/overview markers |
| gCHUDViewLocation2 | (344,508) 158x68 | location name/subtitle statics |
| gCHUDViewTradeVendor2 / TradeTransaction2 | trade | vendor panel (list of vendor stacks) / strip with two small lists, transfer buttons, gold balance; texts `HUD_Trade`, `HUD_Close`, `HUD_GoldBalance`, `HUD_Gold_Value`, `HUD_TakeAll` (loot) |
| gCHUDViewLootPool2 | loot | list of container stacks, "take all" |
| gCHUDViewAmount2 | (368,297) 287x173 | modal amount chooser (slider + 2 buttons) for splitting/dropping/buying stacks |
| gCHUDViewDialog2 / Talk2 | dialog | response list (`gCHUDListCtrlDialog`, bound to `gCInfoManager_PS`) / NPC subtitle area |
| gCHUDViewTutorial2 | tip window | text + "no more tips" check box + OK |
| gCHUDLabel, gCHUDButton, gCHUDLogo, gCHUDCrossHair, gCHUDTextEffect, gCHUDLog | helpers | generic statics: label, button, start logos, crosshair (fades, resizable), animated text effect, message log |
| gCHUDStaticCompass / StaticMap / StaticNPC / StaticInvIcon / StaticInvCash / StaticInvCategory / StaticTalk | statics | compass needle, minimap with markers, entity-bound NPC static, item icon holder, gold holder, category icon, subtitle line |
| gCHUDOption* | no window | option value objects, section 11 |
| gCHUDFileManager | none | thin wrapper over save-game list (QuickSave etc.) |

**Rust port implications**
* Use this table to name the Rust "controllers"; actual data (positions, fonts) are in g3.gui.res and UI_ORIGINAL.md.
* Prefer gameplay-side views (`snapshot` structs from the existing sim/gameplay crates) over entity proxies: the original binds by entity proxy + property set id.
* Script hooks that the HUD calls and the Rust port must expose: `OnPlayerGetGoldCost(stackIdx?, amount)`, `OnPlayerGetDamage(damageType, flags)`, `GetProtectionForHUD(type)`,
  `GetXPNext`, `OnPlayerTransformStack`. They are looked up by name in `gCScriptAdmin` (FUN_10038b40).

---------------------------------------------------------------------------------------------------

## 7. HUD behaviours

### 7.1 Bars (health/mana/endurance/focus) [V]
`gCHUDProgressBarNPC` (vtable inits 0x100a5990, 0x100a59d0; paint hook 0x100a5db0) wraps a CFFGFCProgressBar, an entity proxy (+0x10) and a key string (+0x2c):
init sends the control a float-colour message (0x414) from constants (0.5 / 2.0 [I] tint), range 0..100, position 100. Views set the key in `OnInitDialog`: health "HP" (0x10083940), mana "MP" (0x10083ac0),
endurance "SP" (0x10083700), focus "HP" (0x100838f0).
The shared update function `FUN_100a5a50` (0x100a5a50, 858 bytes):
* current/max values: if the entity has a gCPlayerMemory_PS (0x3c) the values come from the player's attribute object (virtual getters on the attribute interface: max at +0x64, current at +0x60) for the key; otherwise for key "HP" and an entity with gCNPC_PS (0x1e) the value is read from its gCDamageReceiver_PS (0x34): current at +0x14, max at +0x18.
* colour: HP bar uses one colour normally and another when the NPC property set flag +0x188 bit 4 is set (poisoned/burning style status); SP bar similarly with bit 8 (exhausted). Two palette entries each (runtime-initialised palette near 0x103f2a88..0x103f2adc [U values]).
* text overlay: when the progress bar's "show numbers" flag (+0x30) is set the control text is "cur / max" (format %d / %d style), else empty.
* **Low-health screen feedback:** for the player's HP bar, if engine setup +0x184 ("Feedback" option) is 1, the function gets the main post-processing effect and sets two intensities
  (`eCPostProcessingEffectMain::SetFeedbackIntensity1/2`): normalised health h = cur/max; threshold derived from max and a setup float at +0x188; intensity1 = 1 - h/threshold when h is below the threshold (else 0); if status bit 4 (poison)
  is set, intensity2 = 1 - intensity1 and intensity1 is raised to at least that. [I] the exact threshold curve, [V] the call structure.
The page layer also forwards "entity changed" to all three bars (0x10088970).

### 7.2 Quickslots [V]
`gCHUDViewBarQuickslots2` (resource `IDD_VIEW_BAR_QUICKSLOTS_2`, one `ListCtrl` 480x48 at (22,5)). Items are drawn by an owner-draw handler on `gCHUDListCtrlInvQuickSlots` (0x100a0dd0): fetch the
stack (inventory stack list from the control's data pointer), compute item rect with list message 0x100e, inset 6x4, then three helper draws on the control (the icon itself comes from the list's image list): amount number (virtual 0x128 with the stack's *Amount*), state/flag overlay (0x130 with Quality OR'd with the item's gCItem_PS +0x18 flag word, e.g. equipped markers [I]) and the slot number (0x12c with index+1).
Quick slot assignment is data on the stack: property `QuickSlot` (`gCInventoryStack` +0x44).

### 7.3 Compass [V]
`gCHUDStaticCompass` (init 0x100a68e0): bitmap `G3_HUD_Icon_Misc_Compass_Needle.tga` plus the static's own background bitmap (rose). Paint hook (0x100a6940 -> 0x100a6960): take the active camera entity's world matrix,
forward vector (x,z), normalise, compute signed angle against the reference (0,0,1) using dot product and a cross-product sign test (angle := 2*pi - angle when negative side), then draw the bitmap rotated by that angle about its centre
(`CFFGFCDeviceContext::DrawBitmap(bitmap, rect=null, flags=0, angle)`). The rotation is therefore *camera yaw*, not player yaw.

### 7.4 Minimap / world map [V for structure]
`gCHUDStaticMap` (init 0x100a86a0; paint 0x100a87d0): members: `gCMap_PS` pointer (+0x10, the map property set: map bitmap name, world bounds, localised map name), entity pointer (+0x14, the player), zoom (+0x18),
arrow bitmap `G3_HUD_Arrow.tga`, marker bitmap `G3_HUD_Marker.tga`. Paint: `gCMap_PS::CalcUV(playerWorldPos)` gives normalised map position (u,v); the visible window size is the control client rect scaled by a zoom factor
(zoom * 1/ constant) centred on the player; arrow drawn rotated by the camera yaw using the same dot/cross logic as the compass; then `FUN_100a8a20` iterates the map's marker list (count at +0x38 of the map PS) and draws each marker with `FUN_100a8a60`.
The `hud_maps` image group of g3.igl holds the map pictures (`G3_Map_*.tga`, 5 tiles of 512x512 in the shipped list).

### 7.5 Floating focus names [V]
`gCGUIEntityRenderer` (0x78 bytes, created lazily in `gCGUIModule::Draw`). Mode field (+0x74): 1 in game (only the *focused* entity's name is drawn; `IsFocus` 0x1007a040 checks gCFocus_PS), 2 in editor (all names), 0 off. `AddFocusName` (0x10079790) queues (entity, alpha);
`DrawFocusNames` (0x10079810) draws each queued entry then clears the list. `DrawFocusName` (0x10079cb0):
* skips entities with no attitude (`GetAttitude` 0x1007a0c0 returns -1 for invalid/dead/state-3 NPCs);
* name string = loc lookup of the entity name with a fixed prefix (`FUN_...`/global string at 0x102ce1b0 [Game], e.g. an `ENT_`-style prefix [U]) through `eCLocString`;
* position from `CalcTextRect` 0x10079870: looks up a bone on the entity (error "Could not find bone: %s" when missing), projects its world position to screen, places the text rect centred above it;
* colour from a 3-way palette indexed by attitude (0xC-byte `bCByteAlphaColor` table at 0x103f2a88; tables 0x102bc454 / 0x102bc478 map attitude -> palette index, one for normal, one for "important" NPCs / dead);
  fade alpha multiplied by the entry's alpha;
* drawn as five passes: black outline copies at (-1,-1), (0,+2), (+2,0), (0,-2) then the coloured text on top (cheap 4-direction outline).
Attitudes (GetAttitude): based on gCNPC_PS (0x1e) switch of the NPC's attitude-to-player (hostile/friendly/neutral...), gCLock_PS (0x45) for locked containers (`PS+0x18==0` -> attitude 4), and special-case for containers that still hold non-quest items (-> 8). **[I]** exact enum names.

### 7.6 Subtitles / NPC talk line [V]
`gCGUIManager::StartSay(name, text, force)` -> `FUN_1008b6c0`: if the Talk view exists and page != 16, and (engine setup +0x1bc "subtitles on" or `force`) then `FUN_10088930`: set text of the Talk static (at +0x228 of the main page), run `FUN_1009eed0`
(measure the text with `DT_CALCRECT|DT_SINGLELINE`(0x420) in a device context of the control; if the text fits on one line keep it centred (style bit 1), otherwise left align / wrap), and fade the static in over 200 ms. `StopSay` fades it out over 200 ms.
`ShowSubtitle(text)` (0x1007c5a0) just does the layout+text part. The Talk region is (227,100)-(797,181), text height 14 px bold. `gCHUDStaticTalk` is the plain static class for this area.

### 7.7 Game messages [V for mechanism]
`PrintGameMessage(text, type)` (0x1007c5c0): ignored when engine setup +0x1ac (hide GUI) is on; otherwise first-use initialises a static table of per-type entries (type enum `gEGameMessageType`; values seen 4 = save ok, 0xd = error) each holding a value 10 (a lifetime in seconds [I]) and one of a few palette colours; the line then goes to `gCHUDLog` which draws into the "Game Messages" rectangle (87,308)-(937,524) [I: fade by lifetime]; `gCHUDLog` also owns a second area ("Log" 15,219-884,471) used for debug/system lines driven by `ini/log.ini` filters (`Log` 0x1007cdf0).
`gCHUDTextEffect` is the animated text label used for level-up style floating text. [I] for gCHUDLog internals (small functions only).

**Rust port implications**
* Bars: bind to normalised (cur,max,status flags) per stat; implement the low-health vignette as a post-process parameter (two floats) instead of by-widget code.
* Compass/arrow: rotation = camera yaw, bitmap centre pivot. Map: UV from world position using the map entity bounds (gCMap_PS properties: bitmap name, `bCVector`, `eCLocString`).
* Focus name: 4-direction black outline, attitude-coloured, faded by focus time; hide when GUI hidden flag set.
* Subtitles: single-line centre, multi-line left; honour the "subtitles" option and a force flag for scripted lines.
* Message list: simple FIFO with TTL and fade; categories drive colour.

---------------------------------------------------------------------------------------------------

## 8. Inventory, trade and loot lists

### 8.1 Stack data model used by the controls [V]
`gCInventoryStack` (a `gCInventorySlot` subclass, properties named in the setters 0x1001e7f0..0x1001e950): +0x0c template entity proxy (the item template), +0x3c `Amount`, +0x40 `Quality`,
+0x44 `QuickSlot`, +0x48 `SortIndex`, +0x50 stack type (`gEStackType`, 2 = "already used/known" in the loot-attitude check), +0x54 `Learned` (bool), +0x58 `ActivationCount`, +0x5c `TransactionCount`,
+0x60 normal icon bitmap, +0x68 grey icon bitmap. Comparators (`CompareName/Damage/Protection/Type/Quality/Value/...`, 0x100c2460..) implement the sort modes.
`gCInventory_PS` (0x1f) owns the stack array (+0x30 pointer, +0x34 count) and provides `SetTransactionAmount`(0x100b70c0), `TransferTransactionItemsTo` (0x100b7170), `GetStackCategory`.

### 8.2 Icon resolution [V]
* `gCInventoryStack::GetIconFileNames` (0x100c0ae0): fetch the template's gCItem_PS (0x30); icon name = its string property at +0x2c (the `Texture` template field, e.g. `IT_1H_OrcSlayer_01.tga`); if empty fall back to a per-category default name from a table indexed by the item category field (+0x28, `gEItemCategory`).
  For category 9 (magic/rune/scroll group) when the template references a gCMagic_PS (0x65) or the item's other link has a gCSkill_PS (0x66), a second *grey* icon name = file name + `_Grey` + extension (via `_splitpath/_makepath`); otherwise both names are the same.
* `CreateIcons` 0x100c0a60 creates two `CFFGFCBitmap` (normal at +0x60, grey at +0x68) by name; the GFC bitmap resolves through `eCGUIAdmin::LoadBitmapA` -> image group list (section 2.5), i.e. via the `it_*` atlases.
* `GetIcon` 0x100c0a40 picks: normal icon if `Learned` or `ActivationCount > 0`, otherwise the grey icon. `FUN_100a2c90` / `FUN_100a34e0` are the two call sites (image-list replace for list items; static-bitmap set for the single `StaticInvIcon`).
* The mapping name -> atlas cell is therefore *entirely* the igl group lists (item groups: it_alchemy 88 tiles, it_armor 41, it_artefacts 132, it_documents 23, it_misc 146, it_perks 148, it_recipe 65, it_spells 94, it_weapons 164 in the shipped list).

### 8.3 List controls [V for call structure]
All inventory lists are CFFGFCListCtrl subclasses in icon view with owner-draw item painting; the control keeps a pointer to the stack list in its data (+0x14 -> {array +0x30, count +0x34}); the paint slot receives (index-of-item, draw-info) and queries the item rect via message 0x100e.
* `gCHUDListCtrlInvStacks` (draw 0x100a1450): item rect inset 6/4 px; draws the amount number for categories other than 9 and 10 (virtual 0x128 with `Amount`), (when use type != 0x72) a quality/flag overlay (0x130), and the quickslot digit (0x12c with `QuickSlot` +0x44); the icon bitmap itself is placed through the list's image list.
* `gCHUDListCtrlInvTradeStacks` (0x100a2100): draw-info inset 4; the number shown depends on the list mode field (+0x38): mode 0 = amount minus pending transaction count (remaining), mode 1 = transaction count (what is in the basket); transaction count -1 or > amount is clamped to the amount.
* `gCHUDListCtrlInvLootStacks` (key handler 0x100a1790): digits 0-9 jump to the n-th item, key 8 (backspace) selects none.
* `gCHUDListCtrlInvDocuments` (count overlay 0x100a2720): shows a count only if > 1; below 1000 prints the number; 1000..999 999 prints a scaled value with the localized suffix `HUD_Thousand`, from 1 000 000 `HUD_Million`; colour 0xC7A648 (gold) opaque; small 10 px font.
* `InvSkills`, `InvSpells`, `InvTransforms`: same pattern with grey icons for not-yet-learned entries.
* Lists rebuild lazily: the OnPaint hook (0x100a2680) logs "Rebuilding modified inventory" when the stack list was invalidated.

### 8.4 Item statistics tables [V]
`gCHUDTable` is a two-column text table; `TableStackStats` (gold cost row calls script `OnPlayerGetGoldCost(amount, transaction type)` 0x100a5650), `TableItemStats`
(damage row calls script `OnPlayerGetDamage(damageType, flags)` using gCDamage_PS 0x33 +0x1c; flags OR'd with the stack's Quality), `TableRecipeStats`, `TableSkillStats`, `TableSpellStats` build label/value rows with the keys `HUD_Damage_*`, `HUD_Req_*` (`HUD_Req_Skill`, `HUD_Req_MP`, `HUD_Req_Knowledge`), `HUD_Bonus_Skill`, `HUD_GoldValue`, `HUD_In_Inventory`. Item title/description via `ITEMDESC_<ItemName>` (`FUN_1007edb0`).

### 8.5 Trade flow [V]
Trade page (5): player inventory on the right, vendor on the left, transaction strip below. Moving an item only edits `TransactionCount` (`SetTransactionAmount(stackIndex, n)`); gold is another stack with template `It_Gold` whose transaction amount is adjusted to balance (`FUN_100811a0`: computes difference between the two sides' totals and sets the gold stack's transaction amount on the poorer side if enough). Confirm calls `TransferTransactionItemsTo(other, mode)` in both directions (mode 2 = sell basket, 0 = buy basket in 0x1008a0b0) then rebuilds lists. Cancel/close zeroes all transaction counts (0x100a1f20). Prices come from script (`OnPlayerGetGoldCost`), not data.
Texts: `HUD_Trade` (button), `HUD_Close`, `HUD_GoldBalance`, `HUD_Gold_Value`.

**Rust port implications**
* Icon lookup: `ItemTemplate.texture` -> igl base-name lookup; if missing use category default; grey variant name `<stem>_Grey<ext>` only for magic/skill linked items; choose by `learned || activation_count > 0`.
* Implement transaction counts as a separate overlay (basket) on stacks; commit atomically. Count overlay formatting (`1.2k` style via loc strings thousand/million) is data-driven.
* Keep stat row builders pluggable: they call into the script VM (already ported Script_Game) -- expose `OnPlayerGetGoldCost` etc. as callable script entry points.

---------------------------------------------------------------------------------------------------

## 9. Dialog UI, text rendering, fonts, localisation

### 9.1 Dialog UI [V for structure]
* Page 6 (`gCHUDPageDialog2`) hosts `IDD_VIEW_DIALOG_2` (`gCHUDViewDialog2`: one `gCHUDListCtrlDialog` at (22,534)-(1002,747), transparent band at the bottom). The Talk (subtitle) line stays in the persistent layer.
* `gCHUDPageDialog2` refresh (`FUN_1009ec90`): clears the list, reads the world entity's gCInfoManager_PS (0x37, via `gCWorld::GetCurrentWorld`), iterates its info list (array +0x38, count +0x3c) and adds a row for every info whose type (+0x30) is 3 or 4 (the two "player may choose" kinds); row data = the `gCInfo_PS`. Then selects row 0 and focuses the list.
* Default selection (`FUN_1009ee40`): when the view opens it selects the first row whose info type (+0x28) is in 0x33..0x35, i.e. the "end/back" style entries; otherwise 0.
* Activation (`FUN_1009ee20` = list activate): `gCInfoManager_PS::ExecuteDialogInfo(info)` (0x101cce40 [Game]); *all dialog logic lives in gCInfoManager_PS, the UI is a list of choices*.
* `gCDialogManagerCtrl` (0x10072da0..) is an older owner-drawn dialog control (`ListInfos`, `OnNotifyList`, `AddInfo`) that is still compiled but not used by the 2.x HUD [I].
* Subtitles: 7.6. Voice-over is driven elsewhere (StartSay is called alongside audio).

### 9.2 Text rendering [V]
* All GUI text is drawn through `eCGUIRenderer::DrawTextA` (0x100e1610 -> UTF-8/ANSI to UTF-16 -> renderer slot 0x14) and `CalcTextRect` (0x100e1530/0x100e14d0, adds `DT_CALCRECT` 0x400). Flags are plain Win32 `DT_*` bits (centre 1, vcentre 4, singleline 0x20, calcrect 0x400). Colours are packed ARGB.
* Font objects are `eCGfxFont` (0x1008f6b0; wraps `eCDX9Font` 0x100add40 = a `D3DXCreateFontW` font, with `GetGlyphData` for the sprite batch) created from `eSFontDesc` (height, width, weight, mip levels, italic, charset, precision, quality, pitch) in `eCThemeAdmin::CreateFontA` and de-duplicated. `eCSystemFont`/`eCVertexFont` exist for the 3D/debug text path.
* **Fonts used:** window fonts come from g3.gui.res (`Times New Roman`, weight 400/700, em heights -14/-16/-18/-24, see UI_ORIGINAL.md). `eCGUIDatabase::Create` (0x100e0960) creates the two default GUI fonts: size 16 bold-ish ("menu font") and size 14, with the face taken from the application's `GUI.DefaultFont` setting (`Comic Sans MS` in the shipped ge3.INI, `Lucida Sans Unicode` alternative, or `Gothic3` = the bundled `Gothic3.ttf` registered privately with `AddFontResourceEx(FR_PRIVATE)`), falling back to `Arial Unicode MS` / `Tahoma`. `GUI.DefaultFontBold` selects bold for the Gothic3 face.
* Text is not cached per string: glyph quads come from the D3DX glyph cache texture; `GetFontHeight` uses a fixed sample string to measure.

### 9.3 Localisation [V]
Every user-visible string is `eCLocString(key).GetString()` -> `eCLocAdmin` lookup in the single binary string table (`ReadBinary` 0x102abdb0 inside GENOMFLE; fallback ini/CSV readers). Keys seen in the HUD: `HUD_*` (about 108, listed in section 12), `GO_*`, `HL_*`, `ITEMDESC_<ItemName>`, `PROT_*`, plus entity names (`<prefix><entityName>`).
Text and audio language are independent (`[Language] Text= Audio=` read by `eCLocAdmin`; UserOptions `SubtitleLanguage`/`VoiceLanguage`); the language option object reads `Language.Subtitle.SupportedCount` and `Subtitle.Entry_n` (ge3.INI) to build its list.
Existing parser: `crates/formats/src/stringtable.rs`.

**Rust port implications**
* Text: replace D3DX font with a glyph atlas renderer; keep the DT_* flag semantics (centre, vcentre, singleline, wrap) and the `-height = em pixels` convention (Times New Roman bold ~ Liberation/other metric-compatible if the original font is unavailable; Gothic3.ttf is optional).
* Subtitles must wrap at the Talk rect (570 px at 1024 virtual) with the single-line centre rule.
* Reserve a `UiText::get(key)` over the existing string table; do not hardcode German.

---------------------------------------------------------------------------------------------------

## 10. Cursor handling [V unless noted]

* The cursor is *software* drawn by FFGUI from the theme: `CFFGUICursor` objects fed from `IFFGUITheme` cursors (7 slots: Pointer, Grab, Wait, HSize, VSize, TLSize, TRSize; theme file section 3.2). `eCCursor` (0x100bd0c0..) is the engine-side data class (hotspot, position, dimensions, visibility, Render 0x100bd250), used by `eCGUIStreamer::UpdateTheme` when pushing cursors into FFGUI.
* Draw size is `Cursor.X/Y` = 32x32 px from the metrics (the 128x128 atlas rect is down-scaled); hotspot (0,0); modulation colour from the cursor's colour object.
* Visibility: `eCGUIModule::ShowCursor(bool)` -> IFFGUIModule slot 0x54; input routing: `EnableInput(bool)` slot 0x14. They are called only through `FUN_1008b540` (page open/close) and `gCGUIManager::CloseMenu`: the cursor is hidden during normal gameplay (mouse look) and shown whenever a HUD page or menu is open. The Windows system cursor is hidden/confined by `eCApplication::ConfineCursor` [I].
* Mouse coordinates are converted to virtual coordinates only for mouse messages and only when mouse scaling is on (section 4).

**Rust port implications**
* Draw the cursor as a 32x32 sprite from the theme atlas (Pointer rect 299,303-427,431 in the real file) or from `theme_g3_00.dds` via the parsed rect; hide it unless `InputFocus::Gui`.

---------------------------------------------------------------------------------------------------

## 11. Option menus and the engine settings they write

### 11.1 Mechanics [V]
* Settings live in two layers: `ge3.INI` (defaults, sections `[Engine.Setup]`, `[Option.Sliders]`, `[Options.Low|Medium|High|VeryHigh]`, `[Options.Gamma]`, `[Language]`, `[Game]`, `[Camera]`) and `Documents/gothic3/UserOptions.ini` (user overrides, written by the menu). `eCVirtualFileSystem::SetRootPath` merges both unless `IgnoreUserOptions=true`.
* Option pages are views with sliders and value statics. Each page exposes an "export to config" virtual (e.g. 0x10098480 for Performance Custom) that writes the current slider positions with `eCConfigFile::SetValue(section, key, int)`; a mirror "import" reads them.
* `gCHUDMenuOptions2` (OK/Apply, 0x10091380): if the options page is visible hands over to the sub page; otherwise if any option changed asks `HUD_Confirm_Restart`; on "yes" the menu dialog serialises all pages into an `eCConfigFile` and `gCWorld::WriteConfigFile("UserOptions.ini")`, then terminates the application (`eCMessageHandler::Terminate`) -- **some changes require restart**; on no it asks every page to revert.
* Each `gCHUDOption*` (small vtable; slot 1 = produce the value caption for the slider, slot 3 = apply, slot 4 = caption loc key, slot 7 = "value differs from live engine state?", slot 8 = revert; roles [I], call structure [V]) maps a slider integer to engine state by reading/writing `eSSetupEngine` fields obtained from `eCApplication::GetSetupEngineDesc` / `GetEngineSetup`. Offsets below are bytes into that structure. Many options use a static counter so the first read after startup initialises the slider from the live engine state ("changed" flag only after the user touched it).

### 11.2 UserOptions.ini real contents (user install, for the key spelling)
```
[Options.Difficulty]  Difficulty=Medium  ExtendedContent=true  AIMode=true
[Options.Controls]    MouseSensitivityX/Y  MouseInvertX/Y  MouseSmoothingX/Y  AltCamera  CamLookaroundInverse  Controls=Standard|Custom
[Options.Audio]       MasterVolume VoiceVolume MusicVolume FXVolume AmbientVolume (0..255)  Speaker  VoiceLanguage  SubtitleLanguage  Subtitle
[Options.Video]       Resolution.Width/Height/RefreshRate  Brightness Contrast GammaRed GammaGreen GammaBlue  VSync
[Options.Details]     Performance=Low|Medium|High|VeryHigh|Custom
[Options.Custom]      DistanceHigh DistanceLow ObjectDetails TextureQuality TextureFilter VegetationQuality VegetationViewRange ShadowQuality
                      Bloom DepthOfField LensFlare Antialiasing Noise Feedback ColorGrading
```
(Custom bindings for keys: `SessionKey.<Action>.Key1.Type/Offset`, `Key2.*`, e.g. `SessionKey.Forward`, `SessionKey.StrafeLeft`; `Controls.ini` + `events.ini` define defaults.)

### 11.3 Slider semantics (from ge3.INI comments and the option classes)
| Key | Values | Engine effect (option class, verified calls) |
|---|---|---|
| DistanceHigh | 1 Low, 2 Medium, 3 High | far clipping plane via `[Option.Sliders] DistanceHigh.fFarClippingPlane_{Low,Medium,High}` = 6000 / 8000 / 10000 (`gCHUDOptionPerformanceDistanceHigh`, 0x100ac2d0) |
| DistanceLow | 0 Off, 1 Low, 2 Medium, 3 High | low-poly (far mesh) clipping plane `DistanceLow.fFarClippingPlaneLowPolyMesh_*` = 25000 / 50000 / 100000; also drives the depth-of-field focal range / depth output offset (`SetFocalRange`, `SetDepthOutputOffset`, 0x100acac0) |
| ObjectDetails | 1 Low .. 4 VeryHigh | sets six `[Option.Sliders] ObjectDetails.*_<level>` values: `fScreenObjectDistanceCulling`, `fProcessingRangeFadeOutRange`, `fRangedBaseLoDOffset`, `fGlobalVisualLoDFactor`, `enuMeshLoDQualityStage`, `enuAnimationLoDQualityStage`, `fLowPolyObjectDistanceCulling` (class 0x100ad960, 4.5 KB; writes ~30 setup fields) |
| TextureQuality | 1 Low, 2 Medium, 3 High | `eCImageCache` LoD quality stage (resource cache): Low = lowest stage, Medium = stage 1, High = stage 2 (0x100af5f0) |
| TextureFilter | 0 Linear, 1..4 Anisotropic 2x/4x/8x/16x | `SetUserTextureFilter` on the graphics admin (0x100af560) |
| VegetationQuality | 0 Off .. 3 High | `eCVegetationAdmin::SetQuality` + setup +0x115 (0x100afa30) |
| VegetationViewRange | 1 Low .. 4 VeryHigh | `eCVegetationAdmin::SetDefaultViewRange` from `VegetationViewRange.fViewDistance_{Low,Medium,High,VeryHigh}` = 4000 / 5000 / 6500 / 7500 (0x100afbe0) |
| ShadowQuality | 0 Off, 1 Low, 2 Medium, 3 High, 4 VeryHigh | `eCIlluminationAdmin` flags: static shadows, terrain shadows, detail shadows (`IsStaticShadowsEnabled/IsTerrainShadowsEnabled/IsDetailShadowsEnabled`); level 4 additionally needs setup +0x113 (e.g. cascaded/extra shadow feature) (0x100af090) |
| LensFlare | 0 / 1 | setup +0x199 |
| Bloom | 0 Off, 1 Bloom, 2 HDR | setup +0x117 (bloom on) and +0x140 (HDR on): 0 = both off, 1 = +0x117, 2 = both (0x100abb20/0x100abb60); captions `HUD_Slider_Value_EffectBloom` / `..._EffectHDR` |
| DepthOfField | 0 / 1 | setup +0x118 |
| Antialiasing (EdgeBlur) | 0 Off, 1 FXAA ("ES2x" in the ini comment), 2 ES4x | captions `HUD_Slider_Value_FXAA` / `..._Off`; 9 setup field accesses (post-process edge blur) [I] exact offsets |
| Noise | 0 / 1 | setup +0x170 (film grain) |
| Feedback | 0 / 1 | setup +0x184 (low-health vignette, 7.1) |
| ColorGrading | 0 / 1 | setup +0x198 |
| VSync | on/off | setup +0x1bd |
| Subtitle | on/off | setup +0x1bc (read by `StartSay`) |
| AltCamera | on/off | setup +0x20c |
| CamLookaroundInverse | on/off | setup +0x20d |
| Brightness / Contrast / Red / Green / Blue Scaling | float | `[Options.Gamma]` scaling constants map slider ranges to the hardware gamma ramp values (`BrightnessScaling=0.2` etc.) (class 0x100b0370) |
| Resolution | list from adapter caps | `Resolution.Width/Height/RefreshRate` (class 0x100b0d00; combo built from the device's mode list) |
| Master/Voice/Music/FX/Ambient volume | 0..255 | `Sound.*Volume` (engine setup) and `Options.Audio`, see audio notes |
| Speaker | 0..N | speaker mode (`HUD_Audio_Speaker_Setup`) |
| Language | list | `eCLocAdmin::SetCurrentLanguage` (0x100ab600) from `Language.Subtitle.Entry_n` |

Preset buttons on `gCHUDMenuOptionsPerformance2` (Low/Medium/High/VeryHigh/Custom radio buttons) copy the ge3.INI `[Options.<Preset>]` table into the custom sliders (all 14 keys above, `ColorGrading` is custom-only) and store `Options.Details/Performance`.

**Rust port implications**
* Implement `GraphicsSettings` with these exact fields and the preset tables; read the same section/key names for compatibility with real `UserOptions.ini`. Map the integer scale to Bevy settings (shadow cascades/ map size, MSAA/FXAA, bloom, DoF, view distances (use the `[Option.Sliders]` numbers as engine units cm), anisotropy, LoD bias).
* Options requiring restart in the original (display, textures, vegetation) may be applied live; keep the "changed" bookkeeping only for UI.
* Volume sliders: 0..255 range preserved on disk; convert to linear amplitude at the audio crate boundary.

---------------------------------------------------------------------------------------------------

## 12. Localisation key inventory (HUD-side, seen as literals)

Menu: `HUD_Menu_New_Game/Load_Game/Save_Game/Options/Credits/Quit_Game/Back/Keymapping/ResetWarning`, `HUD_Menu_Options_{Audio,Video,Controls,Performance,Difficulty}`, `HUD_Confirm_{Restart,Quit,Delete,Overwrite,NewGameABYes,NewGameABNo,NewGameExit}`,
`HUD_Request_{Ok,Cancel,Yes,No,Ignore}`, `HUD_File_{Load,Save,Delete,SaveGame_Empty,SaveGame_Format,SaveGame_Format2,LoadFailed,SaveFailed}`, `HUD_QuickSave`, `HUD_QuickSave_Game`, `HUD_Difficulty_{Easy,Medium,Hard,AIMode,Extended_Content}`,
`HUD_Controls_{Standard,Custom,Keyboard,Mouse,Alt_Camera,CamLookaroundInverse,Custom_Request_A,Custom_Request_B}`, `HUD_Performance_*` (one per slider), `HUD_Slider_Value_{On,Off,Bloom,EffectBloom,EffectHDR,FXAA,DepthOfField}`, `HUD_Video_{Resolution,VSync}`, `HUD_Audio_{Speaker_Setup,Subtitle}`.
In-game: `HUD_GoldAmount`, `HUD_GoldValue`, `HUD_GoldBalance`, `HUD_Gold_Value`, `HUD_Trade`, `HUD_Close`, `HUD_TakeAll`, `HUD_Thousand`, `HUD_Million`, `HUD_Ok`, `HUD_Cancel`, `HUD_NoMoreTips`, `HUD_NoQuests`, `HUD_NoQuestSelected`, `HUD_NoEntries`, `HUD_Quests`, `HUD_Reputation`,
`HUD_Req_{Knowledge,MP,Skill}` (+ `HUD_Req_` prefix), `HUD_Damage_` (prefix), `HUD_Bonus_Skill`, `HUD_In_Inventory`, `HUD_Ingredients`, `HUD_Missing{Attrib,Item,Skill,Spell}` (shown by inventory/info checks).

---------------------------------------------------------------------------------------------------

## 13. Consolidated Rust port implications

1. **Data first.** The UI is almost completely data-defined: g3.gui.res (layout/styles), theme_g3.thm (default widget art, fonts, colours), g3.igl (icons/backgrounds), string table (text), ini files (options). Implement the three parsers (gui.res XML, thm, igl) as bounded independent readers with synthetic fixtures; the `thm` layout in section 3 and the igl layout from `out/parse_igl.py` share the GENOMFLE footer string-table helper.
2. **Virtual 1024x768** with aspect padding on the longer axis; mouse inverse; no per-view anchoring needed except for full-screen pages (Anchor=5).
3. **Page state machine** with the 17 pages, the 6-flag visibility table, ESC rules and the input-focus toggle (cursor + gameplay input + camera). Persist the toggles `hud_hidden`, `quickslots_hidden`.
4. **Binding layer** by plain Rust snapshot structs: player attributes (HP/MP/SP, level, XP, learning points, protections via script), inventory stacks (Amount/Quality/QuickSlot/SortIndex/Learned/ActivationCount/TransactionCount), focus entity (name key, attitude, HP), quest list/time of day, map position+yaw, dialog options (info list with type 3/4 filter), message queue.
5. **Icons** by base-name lookup in igl with grey variants for linked magic/skill items; stack counts with thousand/million suffix strings.
6. **Trade** as a basket overlay with script-priced gold; **loot** like trade without gold balance and with take-all.
7. **Options** keyed exactly like UserOptions.ini + presets from ge3.INI; keep numeric semantics of every slider (section 11.3), expose them to render settings.
8. **Text** with Win32-style DT flags semantic; Times New Roman bold fallback; subtitle centre/left rule; localisation exclusively through keys.
9. **Not worth porting:** FFGUI/GFC class hierarchy, message maps, DDX, the in-DLL editors (`eCGUIConsole*`, `eCGUIObjectInspectorDialog`), `gCDialogManagerCtrl`, source-theme import/export (`ImportThemeSettings`, `CompileTheme`).

---------------------------------------------------------------------------------------------------

## 14. Unknowns / next steps

* [U] Where the theme name `g3` and the initial `ActivateTheme` call originate (probably the executable or `CFFGUIModule` defaults).
* [U] Palette colour values for bars, focus names and message types (objects are filled at start-up in .bss, not present in the file image); get them from reference screenshots or a runtime trace.
* [U] Tip-window string keys (table at 0x103f27b8 [Game] filled at run time) and which pages trigger tips beyond pages 0..5.
* [U] Anchor/Dock numeric semantics (values 0 and 5 seen on pages; need `CFFGUIWindow` layout code, 0x1000f650 for parsing, resize handler in vfn_21/22).
* [U] Exact widescreen behaviour of HUD views (are the bars pinned to the right edge of the padded rect?). `IDD_VIEW_BAR_HEALTH_2` has a placement of x=915..1116 in the XML, i.e. it is moved at run time (UI_ORIGINAL.md reports (807,688) in screenshots), so placement code exists in the page layer.
* [U] The mapping of eSSetupEngine offsets (+0x113, +0x115, +0x117, +0x118, +0x140, +0x170, +0x184, +0x188, +0x198, +0x199, +0x1ac, +0x1bc, +0x1bd, +0x20c, +0x20d) to named `[Engine.Setup]` keys; they are reflected properties, resolvable from the property type table of `eSSetupEngine` in the Engine DLL if needed.
* [I] Atlas-cell mapping for items is fully solved by g3.igl (section 2.5/8.2); UI_ORIGINAL.md "Item icons - status" can be closed.


---

## Appendix: property-set type ids (`GetPropertySetType` return values)

Used by `entity->GetPropertySet(id)` calls (decimal; hex in parentheses). [V]

9 (0x9) gCDummyCollectorNegZone; 10 (0xa) gCDummyCollectorNavPath; 11 (0xb) gCDummyCollectorPrefPath; 12 (0xc) gCNavHelper_PS; 13 (0xd) eCRigidBodyBase_PS; 14 (0xe) eCCollisionShapeBase_PS; 14 (0xe) gCDummyCollectorCollisionShape; 15 (0xf) eCDynamicLight_PS; 16 (0x10) eCDirectionalLight_PS; 17 (0x11) eCPointLight_PS; 19 (0x13) eCHemisphere_PS; 20 (0x14) gCMovementBase_PS; 21 (0x15) gCCharacterMovement_PS; 22 (0x16) gCCharacterControl_PS; 23 (0x17) gCCharacterSensor_PS; 24 (0x18) gCCameraAI_PS; 25 (0x19) eCEditorEntityPropertySet; 26 (0x1a) gCAIHelper_Label_PS; 27 (0x1b) gCAIHelper_FreePoint_PS; 30 (0x1e) gCNPC_PS; 31 (0x1f) gCInventory_PS; 32 (0x20) gCClock_PS; 33 (0x21) eCEffectBase_PS; 36 (0x24) eCEventDebugger_PS; 37 (0x25) eCEventLogic_PS; 38 (0x26) gCEventScript_PS; 39 (0x27) eCEventFilter_PS; 40 (0x28) eCTriggerList_PS; 41 (0x29) eCTriggerCombiner_PS; 42 (0x2a) eCSkydome_PS; 44 (0x2c) eCEngineCaps_PS; 45 (0x2d) gCScriptRoutine_PS; 46 (0x2e) eCSpeedTree_PS; 47 (0x2f) eCSpeedTreeWind_PS; 48 (0x30) gCItem_PS; 49 (0x31) gCInteraction_PS; 50 (0x32) gCTouchDamage_PS; 51 (0x33) gCDamage_PS; 52 (0x34) gCDamageReceiver_PS; 53 (0x35) gCQuestManager_PS; 54 (0x36) gCQuest_PS; 55 (0x37) gCInfoManager_PS; 56 (0x38) gCInfo_PS; 57 (0x39) gCInfoScript_PS; 59 (0x3b) gCFocus_PS; 60 (0x3c) gCPlayerMemory_PS; 61 (0x3d) gCDummyCollectorNavOffset; 62 (0x3e) gCDummyCollectorEnclave; 63 (0x3f) gCArena_PS; 65 (0x41) eCVegetation_PS; 66 (0x42) eCVegetationBrush_PS; 67 (0x43) eCVegetationRubber_PS; 68 (0x44) gCDialog_PS; 69 (0x45) gCLock_PS; 70 (0x46) gCDoor_PS; 71 (0x47) eCStaticLight_PS; 72 (0x48) eCStaticPointLight_PS; 73 (0x49) eCStaticSpotLight_PS; 74 (0x4a) eCIlluminated_PS; 75 (0x4b) gCStatistics_PS; 76 (0x4c) eCPhysicsScene_PS; 77 (0x4d) gCParty_PS; 78 (0x4e) gCAnchor_PS; 79 (0x4f) gCLetter_PS; 80 (0x50) gCBook_PS; 81 (0x51) gCMap_PS; 82 (0x52) eCLinkContainer_PS; 82 (0x52) gCDummyCollectorLinkContainer; 83 (0x53) gCProjectile_PS; 85 (0x55) eCStrip_PS; 86 (0x56) eCForceField_PS; 87 (0x57) eCParticle_PS; 87 (0x57) gCDummyCollectorParticle; 94 (0x5e) gCAIZone_PS; 95 (0x5f) eCAudioEmitter_PS; 96 (0x60) gCEffect_PS; 98 (0x62) eCWeatherZone_PS; 99 (0x63) eCPrecipitation_PS; 100 (0x64) eCVisualAnimation_PS; 101 (0x65) gCMagic_PS; 102 (0x66) gCSkill_PS; 103 (0x67) gCMover_PS; 104 (0x68) gCTeleporter_PS; 105 (0x69) gCArmorSet_PS; 106 (0x6a) gCSectorPersistence_PS; 107 (0x6b) gCFlock_PS; 108 (0x6c) gCDummyCollectorTreasureSet; 109 (0x6d) gCRecipe_PS; 110 (0x6e) gCWaterZone_PS; 111 (0x6f) gCDistanceTrigger_PS; 112 (0x70) gCTimeZone_PS; 113 (0x71) eCDecal_PS; 114 (0x72) gCMagicBarrier_PS

## Cross-check against ACTOR_ANIMATION_FORMAT.md 'NPC appearance chain'

Confirmed by code: equipped Head/Body/Hair/Beard items carry the visual; Hair/Beard attach to `Slot_Hair`/`Slot_Beard`; Head/Body are actor body parts keyed `Slot_Head`/`Slot_Body`; `MaterialSwitch` is the item's integer property; no gender/species input. Correction/refinement: the switch value indexes the switch sampler's texture list with the sampler's own repeat mode (Repeat/Clamp/PingPong, `eCColorSrcSampler::SetSwitch` 0x1026a150), so the `_S<n+1>` file-name rule is only a proxy for list position n and the fallback for missing variants must come from the material's SwitchRepeat setting.
