# G3V0 archive reader — initial evidence

Primary references consulted 2026-10-20:

- [G3Archive](https://github.com/gekonnn/G3Archive), especially `src/classes/G3Pak_Archive_Header.cs`, `G3Pak_FileTableEntry_Header.cs`, `G3Pak_DirectoryEntry.cs`, `G3Pak_FileEntry.cs` and `G3Pak_FileString.cs`. MIT license; attribution in `THIRD_PARTY_NOTICES.md`.
- Local `Data/Quests.pak` byte header and the remaining installed archives, checked using our own Rust implementation.
- Native FileSystem.dll analysis is documented separately in `NATIVE_FINDINGS.md` with retained local REA evidence.

The implementation is an independent Rust reader informed by that documented layout. It does not execute or load game DLLs.

## Layout supported

Little endian 48-byte header: u32 version (observed 0), product bytes `G3V0`, u32 revision (observed 0), encryption, compression, opaque/reserved value; then u64 file-table, folder-table and volume offsets. Do not interpret the reserved field as necessarily zero: Quests.pak has bytes `3f 3c 32 00` there.

The file-table offset leads to a recursive root entry. Every entry begins with 36 opaque metadata bytes (three u64 timestamps, two u32 values and u32 attributes). Attribute 0x10 selects a directory. A directory has a name, a u32 child-directory count with recursive entries, then a u32 child-file count with entries. A file has u64 payload offset, stored length, decoded length, u32 encryption, u32 compression, name string, comment string. Names are archive-root-relative, not joined with the parent directory name. Strings are u32 byte length, Latin-1 bytes, then one NUL for nonempty strings. Empty strings have no NUL.

Payload compression 0 is plain and 2 is zlib. Other methods and encryption fail explicitly. The reference defines deleted flag 0x8000; this is exposed, but deletion semantics in the installed patch records remain unresolved. Eight `_compiledMesh.p00` entries have offset/stored/decoded size all zero: they are indexed as payloadless records, and reading them fails explicitly. Flag 0x2000 occurs on both payloadless and backed entries, so it is not treated as a deletion flag. Indexing does not imply the payload format is supported. CPT is explicitly unsupported, and mount/overlay precedence is not implemented.

## Bounds and scope

- Index reads use a buffered file, not a whole archive allocation.
- Metadata tail <=64 MiB; <=1,000,000 nodes; <=64 directory depth; <=16 KiB per string.
- Payload reads are lazy and limited to 256 MiB stored/decoded; exact decoded length is required.
- Payload offsets must fit the pre-table data region with checked addition.
- Unsafe/ambiguous paths and case-insensitive duplicate member paths fail indexing.
- Extraction writes one caller-selected new file; there is no bulk unpack into an untrusted path tree.

No mesh, material, animation, terrain, quest, localization or save decoder is claimed here. Coordinates and effective patch precedence remain research tasks.
