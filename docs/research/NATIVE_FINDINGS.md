# Native investigation: Gothic 3

Recorded 2026-10-20. Scope: local static inspection of the installed launcher and FileSystem.dll, plus PE import/export tables of seven core binaries. No target execution, installation changes, injected code, patches, or runtime performance claims.

## Identity and available tooling

Installation: `D:\SteamLibrary\steamapps\common\Gothic 3`.

REA MCP is available with Ghidra 12.1.4 and a configured JDK 21.0.12.1. Native x86 PE analysis, string/name searches, cross references, decompilation, evidence export and snapshots worked on this Windows machine. The Windows Ghidra provider does **not** admit database mutations. Pseudocode is recovered analysis, not original source. Compiler register conventions are imperfectly recovered (`in_EAX`, `unaff_EDI`); do not treat those parameter lists as C ABI definitions. Runtime behavior remains unobserved.

PowerShell FileVersion metadata reports `1, 75, 20108, 14` for Gothic3Final.exe and the core game DLLs. Gothic3.exe reports `1, 0, 0, 1`. Version resources are build-local metadata, not an independently authenticated release history.

| Artifact | Bytes | Named exports | SHA-256 |
| --- | ---: | ---: | --- |
| Gothic3.exe | 147456 | not inventoried | `2bd2f04b1904b53776c6f3bba22fc4f0ce9d127c44af61e3291e8f279f84bed8` |
| Gothic3Final.exe | 196608 | 0 | `5f1c521aeb89cdfd00919d5c48c1e29bb36c07d05c76af76ed55c486269211a0` |
| FileSystem.dll | 176128 | 41 | `2aa50477552d760c3c2932e10c7f4adda1cc30c55866876602a1a20b8f1aab61` |
| Engine.dll | 6901760 | 16224 | `ac0fb50520deb4e9acde483d15f257492fc8158adc0cb7357dc88363a8ece54c` |
| Game.dll | 4640768 | 8569 | `4491a10cdd5908a6f6d9111e0bd5406add7a79901eeeb8d80f769858147a6ca4` |
| Importer.dll | 667648 | 241 | `a8696074ba2b0faffd9b70f85ff32eaaf3d679b755386e69e3cccaefe268e606` |
| SharedBase.dll | 937984 | 4765 | `3087bb4e6c8c7f3bfcf87f79ba889a8c9df223cb71d977963e97dfb51206d4a4` |
| Script.dll | 458752 | 2141 | `1f7477b13a94b241abdd31f07ce43991539592c6d1492a70a823c89e037623dc` |

The seven-core-binary table is from a read-only Python PE header/export parser in `.local/research/inspect_pe.py`; its complete rows are `.local/research/native-pe-inventory.json`. This supplemental inventory is separate from REA Evidence records. It counts named exports, including aliases/data; these counts are not function counts.

## Findings and evidence ledger

### Launcher: answered

Gothic3.exe is the launcher, not the main game implementation. At `0x402173`, recovered code calls `CreateProcessA("Gothic3Final.exe", ...)`, waits for the child process and returns. String xref `0x41a7ec -> 0x4021ce` led to this function. REA found 791 procedures and 374 defined strings in the launcher; these counts include imports and do not guarantee discovery of every function/string.

Evidence: overview `ev_19cafa574e6ce43ad099db9823e8cb16165735fe72f3f49efa48693093fcf27a`; xref `ev_5d17d83bd03f4474255a1b6e90cadbaec65709ff139d29158e5913b9ad1d44e1`; decompilation `ev_95d5d9c7f4ec4f8f0a37eee6102d10c55583598762c04a815cc42af62b69ef5c`.

Gothic3Final.exe imports SharedBase, Importer, Game, Engine and fmodex. Engine.dll additionally imports FileSystem.dll, Direct3D9/d3dx9_40, DirectInput8, PhysXLoader/NxCooking and Bink. These are static dependencies; observed performance and subsystem execution remain unknown.

### FileSystem API: answered at interface level

FileSystem.dll has named MSVC exports for `CFFFileSystemModule::CreatePackFile` (`0x10001b10`), `ReleasePackFile` (`0x10001b20`), `MountPackFile` (`0x10001b70`), `UnmountPackFiles` (`0x10001bb0`), `CreateVolume`, `MountDirectory`, `CreateFileA` and enumeration. The PE image base is `0x10000000`. REA observed 814 procedures and 423 defined strings.

Evidence: procedure search `ev_b645f27022b4e6f9989dbfae580c2cfcfb2c04938389679198f84d3cbe611579`; symbol inventory `ev_e493cc40ce2f3633c72e831d5f1d00e9d63f9cbcd6f78bd138bac75199efd917`; overview `ev_83449bd8b836944d0cb611505f0f389c4fe476097b7f8bba85b23da800b831a0`.

### PAK table layout: partially answered

The following are static reads visible in recovered code. Correlate them with actual archive bytes and bounded parser tests; they do not establish all field meanings or archive compatibility.

| Procedure | Observed reads / operations | Evidence |
| --- | --- | --- |
| `0x10005a10` | Reads a 48-byte header into object offset `+0x98`, seeks to u64 held at `+0xc0` (header offset `+0x28`), reads one string, then invokes root table reader. | `ev_4c343f1b8514c859bdcf41b63f5863d365f213e42ded194c20a08a725a806116` |
| `0x10005cd0` | Loads the byte range whose length is header `+0x28` minus header `+0x20`; conditional table-byte transform when header u32 at `+0x0c` equals 1; then invokes folder reader. | `ev_0aa05f883a881fd778e7a2bd4387c4b5fc484f3a34a4d15f457d6e8d6d372c6f` |
| `0x100049b0` | Folder: reads 36 metadata bytes, a string, child-folder collection, then child-file collection. | `ev_7ee754900b1009507e4a39240a756210c6ebfe9a8988c202a39eb1df33cffc9f` |
| `0x10004be0` | Reads u32 child-folder count then recursively reads folder records. Decompiler stack/register recovery is visibly imperfect. | `ev_0d3a225b44f991a013227a52ac0e23e49cc5d14cd46d427d7edf84e85ef6a7db` |
| `0x10004d80` | Reads u32 child-file count; each file is loaded by `0x100032f0`. | `ev_c28efd03a61fd630c6e6905ffbdad293b3278e327e95f5cc5eea6d5d22503022` |
| `0x100032f0` | File: reads 36 metadata bytes, another 32 bytes, filename string, second string of currently unknown semantics. | `ev_734f58f53ff0d2fee25881c905300fe443a944c28ccee3d3c38b8d62c540c441` |
| `0x10002230` | String: u32 length; zero returns after four bytes, nonzero reads length+1 bytes into string buffer. | `ev_d3afde43261c2f2eb3ef12798ed4f4b6853bda1e60850df22118b5f097b81b83` |

The conditional table transform walks indices `i = length..1`: subtract low byte of `i`, rotate right by `i & 7`, XOR low byte of `i`, storing to byte `i-1`. This is a static decode observation for that branch, not proof any installed archive needs it.

`0x1000a430` contains a DEFLATE-style decoder with zlib header modulus-31 checks, compression-method 8, and gzip magic `0x8b1f`. Inference: zlib/DEFLATE support is compiled into FileSystem.dll. The archive entry flag mapping and exact stream call path were not traced, so this does not prove every compressed resource is zlib-wrapped. Evidence `ev_458ff737cd8762372c9820bbb31aa89cb1f3f289b9f178a414e3627a662153a9`.

## Concrete next native targets

The export table supplies stable build-local seeds. Open the matching SHA binary in REA and query by exported decorated name or image-base plus RVA; do not reuse an absolute address across binaries/builds. These seeds have not yet been decompiled.

| Binary | Export / purpose candidate | RVA |
| --- | --- | --- |
| Engine.dll | `Read@eCResourceMeshComplex_PS` mesh deserialization | `0x1cd960` |
| Engine.dll | `Read@eCResourceImage_PS` image resource deserialization | `0x1b3f50` |
| Engine.dll | `Read@eCResourceMaterialObject_PS` material deserialization | `0x1dd740` |
| Engine.dll | `Read@eCEntityDynamicContext` dynamic entity context | `0x21e120` |
| Engine.dll | `Read@eCEntity` base entity | `0x2257f0` |
| Engine.dll | `Read@eCSpatialEntity` spatial entity | `0x228d50` |
| SharedBase.dll | `Read@bCPropertyObjectFactory` property factory | `0x59400` |
| SharedBase.dll | `Read@bCPropertyLayout` property layout | `0x58350` |
| Game.dll | `Read@gCSector`, `Read@gCSession`, `Read@gCQuest_PS`, `Read@gCInventory_PS` | see local JSON |

The full decorated names and ordinals are in the local PE JSON. Names identify candidates; they do not establish file-format schemas or gameplay semantics. Prefer the smallest resource from the actual PAK inventory and corroborate its bytes with one focused deserializer, then write an independent Rust parser with bounds, malformed-input tests and a known-resource check. Do not attempt to translate thousands of exports wholesale.

## Retained evidence and handoff status

Local ignored files (intentionally contain installation-specific data/recovered analysis):

- `.local/research/launcher-evidence.json` (5 records) and `launcher-snapshot.json`.
- `.local/research/filesystem-evidence.json` (20 records) and `filesystem-snapshot.json`.
- `.local/research/native-pe-inventory.json` and `inspect_pe.py`.

REA sessions were exported and closed. To reuse a snapshot, open the original binary with `provider_id: ghidra` and matching absolute `snapshot_path`; source hash and analysis profile must match. Evidence bundles are portable retained reports, not original game source.

Unresolved: complete archive metadata field meanings, archive priority and deletion semantics, entry compression flags, resource schemas, world coordinates/units, rendering parity, animation, physics behavior, gameplay fidelity and measured performance. No claim of a complete Gothic 3 engine or verified rewrite follows from this investigation.
