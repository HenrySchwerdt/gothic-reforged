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

Extracted shader sources and all original assets remain under ignored `.local/`; no
decompiled function bodies or original asset data are included here.
