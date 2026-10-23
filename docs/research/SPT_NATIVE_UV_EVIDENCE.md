# Native evidence for SPT atlas UVs

## Shipped leaf shader

The original `Data/Materials.pak` contains `ShaderMaterial/Effects/ge_leaf.fx` (11,903
bytes) and its included `ge_globals.fx` (19,513 bytes). In `ge_leaf.fx`, the default
vertex UV macro resolves to the input UV (line 69), the vertex shader copies it into the
output (line 318), and `XFX_PS_TEX0` names the interpolated input UV (line 61). The pixel
output takes the externally supplied `XFX_PS_DIFFUSE` expression at line 370; this file
does not define that expression. `ge_globals.fx` defines the input macro as `input.vTex0`
at line 69. There is no visible UV arithmetic or V remap in these paths. They do not reveal
how the original runtime populated that vertex input or expanded the diffuse expression.

## Bounded Engine.dll export inventory

The existing `.local/research/native-pe-inventory.json` records seven PE files. Its
`Engine.dll` entry identifies `D:\SteamLibrary\steamapps\common\Gothic 3\Engine.dll`,
6,901,760 bytes, SHA-256
`ac0fb50520deb4e9acde483d15f257492fc8158adc0cb7357dc88363a8ece54c`, image base
`0x10000000`, and 16,224 named exports. The following narrow subset contains candidate
identifiers only; addresses are image base plus the recorded export RVA:

| Export identifier | Address |
| --- | ---: |
| `?CreateLeafGeometry@eCSpeedTreeRenderBuffer@@...` | `0x10204ee0` |
| `?CreateFrondGeometry@eCSpeedTreeRenderBuffer@@...` | `0x10204260` |
| `?CreateBranchGeometry@eCSpeedTreeRenderBuffer@@...` | `0x10203590` |
| `?GetLeafAngleMatrices@eCSpeedTreeAdmin@@...` | `0x10208ae0` |
| `?GetLeafBillboardTable@eCSpeedTreeAdmin@@...` | `0x102089c0` |
| `?Create@eCShaderLeaf@@...` | `0x10257b60` |
| `?AccessScale@eCTexCoordSrcScale@@...` | `0x10261410` |
| `?AccessAngle@eCTexCoordSrcRotator@@...` | `0x1012e460` |
| `?AccessOffset@eCTexCoordSrcScroller@@...` | `0x10261410` |

The identifiers show that leaf/frond geometry and texture-coordinate-source classes are
exported. The inventory is names and addresses only: it does not establish these
functions' behavior, call sites, or whether any are used for the SPT atlas path. A
case-insensitive substring scan of all 16,224 export names found no identifier containing
`flip`; this bounded negative result does not rule out an unnamed or differently named
conversion.

## Remaining unknown

Neither the shader source nor the export-name inventory establishes whether the original
SDK/runtime flips leaf rectangles, atlas images, or vertex UVs before `ge_leaf.fx` sees
them. The native functions were not decompiled or traced, and no native runtime capture was
performed. Resolving that question requires focused dataflow evidence from the geometry
creation path (or a matching original-render capture), not inference from export names or
the visually denser atlas cell.

## Cached `ge_leaf.fx` UV bytecode

A bounded read-only disassembly of the 29 cached `ge_leaf.fx` Shader.Cache entries found
28 vertex-shader blobs and one pixel-shader blob. All 28 vertex variants declare
`texcoord0` in `v4` and copy `v4.xy` directly into four output UV registers; no other
instruction in those variants references `v4`. The single pixel-shader blob samples the
incoming `v0` UV directly and has no coordinate remap. This corroborates that the cached
shader stages do not visibly apply a V flip. The machine-readable offsets, lengths,
classification, and input-reference counts are retained in the ignored
`.local/qa/tree-diagnostic/leaf-cache-uv-summary.json`; the disassembler was applied only
to those 29 ranges, and `dec.pkl` was not opened or unpickled.

This is cached permutation evidence, not proof of which permutation the live renderer
selected. It also does not reveal how the SPT/SDK populated vertex UVs or whether geometry
creation remapped leaf rectangles before the shader input. Runtime UV orientation therefore
remains unresolved.

## Bounded `CreateLeafGeometry` native fallback

The retained REA evidence bundle had no records, and a focused
`inspect_with_objdump` request returned `ERR_REA_WINDOWS_NATIVE` without an Evidence
ID. To avoid another full `Engine.dll` import after the earlier Ghidra import timed out,
a local LLVM objdump was run only on a synthetic PE carrier containing the verified
`.text` range for `CreateLeafGeometry`. The read-only source binary was
`D:\SteamLibrary\steamapps\common\Gothic 3\Engine.dll`, SHA-256
`ac0fb50520deb4e9acde483d15f257492fc8158adc0cb7357dc88363a8ece54c`. With image base
`0x10000000`, the function entry VA `0x10204ee0` maps to RVA and file offset
`0x204ee0`. The extracted range is `[RVA 0x204ee0, 0x205d50)` (3,696 bytes), ending at
the next named export. Its bytes match the source range; the disassembly observes
`retl $0x8` at VA `0x10205d42`, followed by INT3 padding through `0x10205d4f`.

The bounded tool was LLVM `llvm-objdump.exe`, LLVM 23.1.3, invoked with
`-d --no-show-raw-insn` on the synthetic carrier. Disassembly SHA-256:
`ffede3721559fbee1e5ab01cc193c1edfe2f95e2adb0013336347d1fef1f30f6`. The extracted
range SHA-256 is `21bba42a611c3ad8c1da00775cfe8e3ab3e4765adce3b002a1b37b9bb9f1799d`.
The ignored evidence artifacts are under
`.local/research/leaf-native/`: `engine-CreateLeafGeometry-carrier.json` records the
parent PE/RVA/file-offset mapping and hashes; `engine-CreateLeafGeometry-range.bin` is
the extracted byte range; `engine-CreateLeafGeometry-carrier.exe` is the analyst-created
carrier; and `engine-CreateLeafGeometry-objdump.txt` plus
`engine-CreateLeafGeometry-objdump-evidence.json` retain output and tool provenance.
The carrier was not executed. No original file was modified and no original content or
disassembly body is committed.

The function writes scalar values at output offsets `0x30` and `0x34`; on the inspected
float load/store paths, a loaded float reaches the store without visible intervening
float arithmetic. The disassembly does not establish that these fields are TEXCOORD
components, nor their corner association. No clearly referenced, format-identifiable
`D3DVERTEXELEMENT9` declaration table was found in this function, so no adjacent table
was extracted. A pointer literal (`0x1043d440`) is passed to an unresolved indirect call
near the function tail, but the available code does not identify its type or purpose;
it was not treated as a declaration-table reference. These bounded observations do not
prove that the SDK never remaps V:
indirect calls, upstream inputs, and the output vertex layout remain unresolved. This
fallback is analyst-run local disassembly, not an REA observation, and it does not
resolve the earlier full-import timeout.

### Focused Ghidra analysis of the synthetic carrier

The existing Ghidra 12.1.4 provider was available with no target open. It analyzed only
the same 4,608-byte synthetic carrier above, SHA-256
`abece80faddb5aaa4793fef5b9f2b5f6cd8385563dc3da44a9f58ea7c515223f`, at the mapped
function entry `0x10204ee0`; the original `Engine.dll` was not reopened. Ghidra observed
the function body through `0x10205d44`, with non-contiguous body ranges, and labeled the
function `entry` because the carrier has no original symbol table. The retained REA
Evidence ID is `ev_32cd025b70a4b6fc436291463c81fe9524e37b1d870448ac3c356b525b2f1ce5`.

Ghidra pseudocode shows repeated output records with a `0x3c`-byte stride. In the leaf
path, the field at record offset `0x30` is assigned a float converted from the low two
bits of a per-entry source byte. That same byte participates in an indexed lookup that
reaches a 16-byte record in a separate array. The field at offset `0x34` receives a
4-byte value from a per-index auxiliary array/pointer table; the assignment itself shows
no arithmetic. This gives a bounded dataflow link between a 0..3 per-entry value and one
output field, but it does
not establish that either field is a TEXCOORD, identify which selector denotes which
quad corner, or show how the auxiliary value was created. The Ghidra value-flow record is
truncated, and 93 calls lack resolved targets. Consequently this evidence still cannot
rule out an upstream V remap or prove original corner-to-atlas association.

The full retained Ghidra Evidence bundle and provider-neutral snapshot are ignored files
at `.local/research/leaf-native/leaf-carrier-ghidra-evidence.json` and
`.local/research/leaf-native/leaf-carrier-snapshot-ghidra-01.json`, respectively. They
include provider output only for the synthetic carrier. No native function body is
copied into this tracked note, and the snapshot is not an analysis of the complete
original `Engine.dll`.

Extracted shader sources and all original assets remain under ignored `.local/`; no
decompiled function bodies or original asset data are included here.
