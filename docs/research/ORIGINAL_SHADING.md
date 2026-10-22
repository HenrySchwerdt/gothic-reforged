# Original Gothic 3 shading (lighting, fog, post)

2026-10-20. Sources: (a) `Shader.Cache` in the install root, which is a compiled D3D9 cache; (b) the shader *sources* the install ships uncompressed in `Data/Materials.pak` (`ShaderMaterial/Effects/ge_default.fx`, `ge_globals.fx`, `ge_skydome.fx`, `ge_leaf.fx`, `ge_vegetation.fx`, `ge_water.fx`, `Effects/ImageProcessing/IP_*.fx`); (c) `Ini/ge3.INI`. The equations below are my own restatement derived from reading the source and from a local SM3 disassembler (`.local/sc/dis.py`, not committed); no shader text or bytecode is stored in the repository. The disassembly and the HLSL agree.

## Shader.Cache container

`GENOMESC` header, then a flat sequence of entries `[~61 byte header: key hash, size words][u16 len + "ge_default.fx\0"][7 bytes][u32 blob size][D3D9 token stream]`. 7025 blobs: 586 `vs_3_0` + 6439 `ps_3_0`; only SM3 is cached. Entries per effect file: `ge_default.fx` 6950 (uber shader, permutations selected by `XFX_*` defines: lightmap, vertex-lightmap, instance-lightmap, specular, normal map, dir light, point light, shadow variants, rim light, subsurface, terrain shadows), `ge_leaf.fx` 29, `ge_particle.fx` 22, `ge_vegetation.fx` 12, `ge_water.fx` 8, `ge_skydome.fx` 4. Every blob carries a `CTAB` comment with constant names/registers (`arrSHCofficients`, `DirLight`, `PointLight`, `colFog`, `vecCamPos`, `vecConstantColor`, `colRimLight`, `bRimLightSkin`, `fGlobalAlpha`, `smpLightMap`, `smpShadowMap`, `smpTerrainShadowMap`, `vecShadowDensity`...), which is how permutations are identified. The key hash is a permutation hash; it was not needed because the source is available.

## Pixel lighting (ge_default, all permutations share this core)

All maths is in **gamma space on the raw texture values** (no sRGB decode, no tone mapping in the scene pass; the display gamma does the rest):

```
V = normalize(cam - P);  N = shading normal (normal-mapped when the material has one)
ndv = max(N.V, 0)
sh  = SH0 + SH1*N.x + SH2*N.y + SH3*N.z            // arrSHCofficients[4], rgb, linear 2-band hemisphere ambient
light = vertexLight.rgb                            // COLOR1.rgb * 4 (baked vertex light, mostly 0 on architecture)
      + lightmap(uv2).rgb * 4                      // only for lightmapped instances (local static lights)
      + sh * (AO * (0.25*ndv + 0.75))              // AO = COLOR1.a (stream 5 alpha), full strength
      + DirLight.colDiffuse * max(N.L,0) * shadow  // shadow = cascade/terrain-shadow term, x saturate(4*N.L) when normal mapped
      + PointLight.colDiffuse * sat(1 - d*invRange) * max(N.Lp,0)   // up to one dynamic point light per pass
rim (only XFX_PS_RIMLIGHT permutations):
  skin:     light += light*colRimLight.rgb * pow(1-ndv, colRimLight.a) * 5
  non-skin: light += (clamp(light,0,0.5)*c.r + c.g) * smoothstep(c.r, c.g, 1-ndv) * c.b     // c = (RimLightStart 0.65, RimLightEnd 2.2, RimLightIntensity 1.0) from ge3.INI
color = tex.rgb * light + specular           // tex alpha * fGlobalAlpha = output alpha
specular (XFX_SPECULAR): phong, power = SpecularPower*128, normalised (0.159*p+0.318), * N.L / max(N.V,eps) (saturated, "Fresnel"), colour = DirLight.colSpecular * (spec mask from texture alpha) * vecConstantColor
out = lerp(colFog, color, fog)                // fog = saturate(dist * a + b), radial linear, from the VS
```
The pass also writes a second render target (`oC1`): a linear depth value for soft particles / DOF.

Differences to our previous static shader, which explain the "flat" look: we used Bevy PBR in *linear* space on sRGB-decoded textures, vertex AO at 0.7 strength, a hemisphere ambient 650 lux and a strong sun; the original is gamma space, AO at full strength, no PBR specular, a small ambient (so shadows and AO go deep), and the sun is the only strong term.

## Vertex stage (ge_default VS)

`o.color1 = float4(4,4,4,1) * COLOR1` (so baked vertex light is overbright x4, AO alpha is untouched); radial fog `saturate(length(cam-P)*vecFogAndDepthOffset.y + vecFogAndDepthOffset.x)`, i.e. linear in distance, 1 near / 0 far; world normal from `mul(N, (float3x3)World)` (skinning/instancing/wind variants in `ge_globals.fx`). Vegetation (`ge_leaf`/`ge_vegetation`) lights in the VS with the same SH + one dir + point lights and the PS only does `tex * vertexColor * constant` plus fog.

## Sky dome (ge_skydome)

VS colour for view direction d, up y = d.up, sun s: `hz = 0.3*(1-pow(y,0.7)) / max(0.2*(1-y), 1e-5)`, `k = 0.5*(1.3 + d.s)`, `sky = hz*colHaze + (1-hz)*k*colSky + colSun * 0.5 * pow(max(d.s,0),300)`. The PS adds two scrolling cloud density layers (4 samplers) with `colClouds`, and blends `lerp(sky, cloudColor, cover)`. Gradient colours come from the environment keyframes (not decoded yet).

## Post-processing (ge3.INI + IP_*.fx)

User options (`UserOptions.ini`, Performance=High) select `Bloom=1` ("Bloom": plain additive bloom) or `Bloom=2` (HDR). Bloom: `MaskHighLights` keeps `max(lum-0.59,0)*mult` (BloomThreshold 0.59), blurs it, then `scene += bloom*(1-scene)` with chromatic aberration (BloomIntensity 1.7, aberration 2.75). HDR mode (VeryHigh): `1-exp(-2.85*c)`, blue-shift, vignette 0.30 (`pow(1-r^2,0.3)`), gamma 0.60. Colour grading (`ColorGrading=1`) is a 16x16x16 LUT texture (`G3_Color_LUT_LDR.tga`) applied in gamma space; DOF at High. Brightness/contrast/gamma in `UserOptions.ini` are 0 (neutral).

## Daytime colours

Sun, ambient, fog, sky and haze colours are keyframed per weather/daytime in a world environment resource that was not located in `Templates.pak`/`Library.pak` (`WeatherStates.g3ws` holds only rain/snow particle state; `eCWeatherZone_PS` templates carry optional overwrites `LightDiffuseColor`, `AmbientBackLightColor`, `AmbientGeneralColor`, `FogColor`, `SkyColor`, `HazeColor`, `CloudColor` and a `WeatherEnvironment` name such as "Myrtana"). Until it is decoded the runtime uses constants **calibrated against the reference screenshot** (see `apps/game/src/walk/gmat.rs::light_env`, env overrides `GR_SUN`, `GR_AMB`).

## Implementation here

`apps/game/src/walk/shaders/gothic.wesl` (static, terrain, foliage) and `char.wesl` (characters and props, `StandardMaterial` entities are converted to `CharMaterial` at spawn by `convert_standard`; `GR_NOCHAR=1` disables it) implement the core above: gamma-space albedo (`linear_to_srgb` of the sRGB-decoded sample), SH ambient, `N.L` sun with Bevy's cascaded shadow lookup, full-strength AO, lightmap x4, rim for characters, then `srgb_to_linear` and Bevy's distance fog. Not implemented: per-material specular, point-light pass, bloom/LUT/DOF/vignette, per-daytime keyframes.

## Update 2026-10-20 (night): characters, bloom, environment

* Characters/world ambient: the environment carries `AmbientBackLightColor` besides `AmbientGeneralColor`; modelled as an SH term `back * (-sun_dir) . n` (`gmat.rs::light_env`, `GR_BACK`, calibrated 0.75 against `orig-ardea-start.png`). It lifts the sun-averted side of characters, which have no lightmap and no AO. Characters in the original also receive up to four instance static point lights (`arrInstanceStaticLightData`, not implemented).
* `vecShadowDensity` (shadow = shadow*x + y) exists in the HLSL; its value is not known, shadows are still full strength.
* Bloom (`Bloom=1`, High): `walk/post.rs` + `shaders/post.wesl`, gamma-space: mask `c*max(lum-0.59,0)*1.7`, 5x5-tap subset of IP_Blur's Gaussian (tap spacing 2.5 px is a guess; the original blur resolution is unknown), `scene += bloom*(1-scene)`, chroma offset `2.75*lum` pixels (unit guessed). `GR_NOBLOOM=1` disables. Cost about +0.3 ms GPU at 1440p.
* Colour LUT: `G3_Color_LUT_LDR/HDR.tga` (256x16, in `_compiledImage.pak/_special`) exist, but `ColorGrading` is not enabled in any ge3.INI preset or UserOptions.ini, so it is not applied. Noise also only at VeryHigh. The user's UserOptions.ini is VeryHigh (Bloom=2 HDR, vignette 0.30), so the reference screenshots likely include HDR tone mapping; not implemented.
* Daytime keyframes (sun/ambient/fog/sky): not present as data in any pak (searched Library/Templates/Projects_compiled/Workspace/Materials/Sound for Environment/DayTime/Weather); only zone overrides in world nodes. Probably hard-coded or in Engine/Game DLL.

## Update 2026-10-21: daytime keyframes found, port of the environment

The "daytime keyframes (sun/ambient/fog/sky)" are in `Library/WeatherStates.g3ws` (see `ENGINE_PORT_PLAN.md` PORT_NOTES, parser `gothic_formats::weather`, evaluation `gothic-environment`). `apps/game/src/walk/env.rs` drives the directional light, SH ambient, fog, sky and clear colour from them (`--hour H`, `--weather STATE`, `--day-speed N`, `GR_ORIG_ENV=0` restores the old constants); the `GR_SUN` / `GR_AMB` / `GR_BACK` calibration constants are only the start values before the weather data arrives. The original numbers (sun x 2.8) are not tone mapped here: the LDR chain reproduces the reference, the HDR curve does not (`GR_HDR=1`). `vecShadowDensity` is the weather ShadowDensity (0.45..0.65 by day); static point lights reach characters through `CharParams::lights`.

## Update 2026-10-22 (P7): white character surfaces

Cause of the white blades/skin in market scenes: the shared static point light set for characters held 6 lights, summed per fragment. The original packet (`GetDynamicStaticLightData`, ENGINE_MAP 2.3) keeps at most 3 lights per object, each with colour x 2 (`CHAR_LIGHTS` is now 3, `lights::MAX_STATIC`); with the warm Reddock lamps (colour 0.99/0.77/0.53, range 7.5..15 m) 6 x2 lights pushed the sum over 1. `GR_NOCLIGHTS=1` disables them for comparison, `GR_ENV_LOG=1` logs the selected set (`CHARLIGHT`). The `ge_default.fx` pixel path itself (SH * AO * VDAScale, DirLight * N.L * shadow, rim, no clamp until the 8-bit target) was re-read and matches `char.wesl`; no other attenuation exists in the HLSL. Open: the sun term (weather sun x intensity about 2.2 to 2.8 plus SH about 0.9) still exceeds 1 on very pale albedo facing the sun (bald heads in Montera); no original term that compresses it was found (the HDR curve is opt-in `GR_HDR=1`). The QA scenario `walk_to_reddock` asserts at most 5 % pure-white pixels in the hero's screen box.
