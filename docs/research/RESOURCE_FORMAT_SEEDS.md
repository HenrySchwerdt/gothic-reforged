# Resource import seeds: XIMG and XCMSH

Research date: 2026-10-20. This is a bounded format investigation, not a complete resource loader. Original payloads and external source files remain in ignored `.local/research/`. No assets or third-party implementation code are embedded in the project.

## XIMG: concrete envelope supported by local byte checks

Source archive: `D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledImage.pak`. A read-only Python archive walker selected 26 payloads below 1 MB at regular intervals in size-sorted order; it is not an exhaustive image inventory. All selected payloads matched the following layout and compressed-size calculation. The complete bounded observations (names, sizes, hashes, dimensions, mip counts) are in `.local/research/ximg-survey.json`; the survey script is `survey_ximg.py`.

| Absolute byte offset | Type / length | Observed interpretation |
| ---: | --- | --- |
| 0 | 8 bytes | `GENOMFLE` |
| 8 | u16 LE | envelope version 1 |
| 10 | u32 LE | absolute offset of string-table trailer |
| 14..38 | 24 bytes | opaque resource fields; preserve until understood |
| 38 | 5 bytes | `G3IMG` |
| 43..47 | 4 bytes | opaque image version/flags |
| 47 | u32 LE | base width |
| 51 | u32 LE | base height |
| 55..63 | 8 bytes | opaque |
| 63 | u32 LE | mip count |
| 67..71 | 4 bytes | opaque |
| 71 | 4 ASCII bytes | `DXT1`, `DXT3`, or `DXT5` in the sample |
| 75..87 | 12 bytes | opaque |
| 87..table_offset | variable | compressed mip chain |
| table_offset | u32 LE | `0xDEADBEEF` marker |
| table_offset+4 | u8 | table version 1 |
| table_offset+5 | u32 LE | string count |
| table_offset+9 onward | repeated | u16 LE byte length, then string bytes |

For mip level `i`, use width `max(1, width >> i)` and height `max(1, height >> i)`. A BC block covers 4x4 pixels. Byte size is `max(1, ceil(width_i/4)) * max(1, ceil(height_i/4)) * block_size`, where block size is 8 for DXT1 and 16 for DXT3/5. All 26 sampled payloads satisfy `87 + sum(mip_sizes) == table_offset`. Observed mip counts are 1 through 11; dimensions include 4x4, 64x1024, 128x512, and 1024x1024. Do not infer mip count from the maximum dimension: e.g. 64x1024 had 7 mips.

The independent XimgHandler implementation identifies smallest-to-largest mip storage and offsets the base image past smaller mips. An initial Rust implementation should expose borrowed mip ranges by logical level; a DDS adapter will need largest-to-smallest order. The size checks corroborate the total chain layout, not pixel orientation or the visual correctness of individual mip levels. No image was rendered in this investigation.

Small reproducible sample: `_Intern/blue.ximg`, archive payload offset 593630188, 120 stored/decoded bytes, no archive compression, SHA-256 `0f92c68f2f6f4f7c91642f866928e3d932f41626600300387605672c8155dc83`. Base dimensions 4x4, 3 DXT1 mips, image bytes `[87,111)`, string trailer `[111,120)`. Local payload `.local/research/sample.ximg`; metadata `.local/research/resource-samples.json`. Its three mip blocks each occupy 8 bytes; base mip candidate is `[103,111)`.

Suggested bounds: exact supported magic/version, nonzero bounded dimensions, explicit FourCC allowlist, checked arithmetic, bounded positive mip count, no repeated 1x1 levels beyond supported policy, complete byte ranges, exact trailer offset, bounded string count/length and complete table end. Unknown envelope variants should fail explicitly. This schema describes the observed Gothic 3 layout; it does not describe Risen GR01IM04 files.

## XCMSH: next bounded parser seed

Implementation added after this research: `gothic-formats::genome::GenomeFile` validates the v1 envelope and complete bounded raw string table. `gothic-formats::ximg::Ximg` parses the observed DXT1/3/5 mip subset and rewraps blocks in DDS order through `to_dds()`. `gothic-inspector ximg-dds` creates a local output file without overwrite. These are independent Rust implementations. DDS headers follow Microsoft's [DDS_HEADER](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header) and [DDS_PIXELFORMAT](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-pixelformat) documentation. Final validation results are recorded in `../HANDOFF_CLAUDE.md`.

Small sample from `_compiledMesh.p00`: `G3_Master_Collisions_01/G3_Myrtana_Ruins_Wall_07_COL.xcmsh`, payload offset 1504643, 2855 bytes, no archive compression, SHA-256 `ba1c9b07d723e5630b10049de629b51ce57d4b1d79aea0a47c898e4655c1b16c`. Payload is `.local/research/sample.xcmsh`.

It has the same `GENOMFLE` envelope version 1. Trailer offset is 2731; marker/version match above; 6 strings terminate exactly at byte 2855:

- `eCResourceMeshComplex_PS`
- `BoundingBox`
- `bCBox`
- `ResourcePriority`
- `float`
- `G3_Architecture_Bridge_Large_01_COL.xshmat`

The property stream begins at absolute byte14 and uses u16 indices into that trailer string table, corroborated by the first class-name index 0. A generic Genome envelope/string-table parser is a useful first layer shared by mesh and material formats. Do not parse XCMSH strings as the archive's u32/Latin-1/NUL string format.

The external mesh reader provides a candidate recipe involving versioned property sets and typed vertex arrays. Stream tags include indices, positions, normals and UVs. This is a lead, not a locally validated mesh schema; version branches, vertex stride, index winding, coordinate conversion, bounds, material resolution and trailing arrays need individual checks. Prefer independently recovering reads from the installed Engine.dll and correlating one sample before implementation.

## Primary references and provenance

- [Baltram/rmtools XimgHandler](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/QImageIOPlugin/source/ximgplugin/ximghandler.cpp): independent tool author's Gothic 3 decoder, consulted for image offsets and mip ordering.
- [Baltram/rmtools Genome file wrapper](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/mimicry/source/Mimicry/mi_genomefile.cpp): independent tool author's envelope/string handling, corroborated on the local sample trailer.
- [Baltram/rmtools mesh reader](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/mimicry/source/Mimicry/mi_xcmshreader.cpp): candidate for subsequent mesh investigation.
- [Gothic Modding Community object persistence](https://auronen.cokoliv.eu/gmc/genome/general_info/object_persistence/): primary community documentation of its object-persistence reconstruction; applies to Gothic 3/Risen 1, with explicit exclusions for newer games.

The rmtools repository license is [GPL-3.0](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/LICENSE.txt). Source downloads were retained only under ignored local research; no source lines were copied into project implementation. The exact Git commit is pinned above. Future code reuse needs its own license decision; the intended implementation path is an independent byte parser backed by local data and tests.

## Remaining native analysis

Engine.dll SHA-256 and targeted `Read` seed are in `NATIVE_FINDINGS.md`. The initial targeted native image query imports and analyzes the whole 6.9 MB DLL before returning any procedure, which is materially slower than FileSystem.dll. Its eventual evidence/status is recorded separately below once drained. No runtime behavior or throughput was measured.

### Native query result: blocked by startup timeout

REA's targeted `procedure_pseudo_code` request for `0x101b3f50` returned `provider_timeout` after its configured 330000 ms Ghidra startup deadline. No image procedure pseudocode or partial native schema was returned. This was initial whole-binary analysis, not target execution. Do not retry the identical query with the same profile expecting a cached completed program. REA remediation is a longer `REA_GHIDRA_STARTUP_TIMEOUT_MS` in the environment that starts its server, or explicitly selected disabled expensive analyzers using `REA_GHIDRA_DISABLED_ANALYZERS`; that changes the analysis profile and requires a fresh import. No server configuration was changed here.

The complete returned failure/diagnostics are retained in `.local/research/engine-analysis-status.json`. The session evidence bundle was exported to `.local/research/engine-attempt-evidence.json`, and the target was closed. The actionable XIMG schema above comes from explicit local byte observations plus independent decoder documentation, not a successful Engine.dll decompilation.
