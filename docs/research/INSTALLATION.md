# Local Gothic 3 installation inventory

Observed read-only on 2026-10-20 (Europe/Berlin). This is an inventory of the
user-selected installation, not a claim about every Gothic 3 release.

## Confirmed target

- Installation: `D:\SteamLibrary\steamapps\common\Gothic 3`
- `Data`: 28 regular files, 3,480,589,376 bytes in total.
- Archives: 20 `.pak` bases, five `.p00` files, three `.cpt` files.
- Patch archives present: `_compiledMesh.p00`, `Music.p00`,
  `Projects_compiled.p00`, `Speech_English.p00`, `Speech_German.p00`.
- CPT files present: `Infos.cpt`, `Projects_compiled.cpt`, `Templates.cpt`.
  Their format is explicitly unsupported by the current importer.
- Both English and German speech archives are installed.

## Version and language evidence

`Ini/ge3.INI` contains `Version=1.75`. Its `[Language]` section contains
`Text=German`, `Audio=German`, `ShowSubtitle=true`, and five subtitle languages:
English, Italian, French, German, Spanish.

Windows file version resources, read using PowerShell's `VersionInfo`:

| File | FileVersion / ProductVersion |
| --- | --- |
| `Gothic3.exe` | `1, 0, 0, 1` |
| `Gothic3Final.exe` | `1, 75, 20108, 14` |
| `Engine.dll` | `1, 75, 20108, 14` |

The differing launcher resource version means that identifying the installed
patch level from `Gothic3.exe` alone would be misleading. The Rust scanner
currently records binary SHA-256 identities and INI evidence; it does not yet
parse Windows version resources. No game executable was run by the scanner.

## Rust scanner

`gothic_importer::scan(root: &Path) -> anyhow::Result<InstallationReport>`
performs a deterministic, read-only inventory. The CLI in `apps/inspector`
owns output writing; the library itself only reads.

- Archives under `Data` are recursively found, sorted, and indexed using
  `gothic_formats::PakArchive`. Supported filename extensions are `.pak` and
  `.p00` through `.p99`, case-insensitively. CPT entries remain visible with
  `status = "unsupported_cpt"`.
- Each successfully indexed archive exposes its G3V0 header, entry count,
  deleted entry count, stored and uncompressed size totals, and extension
  histogram. Indexing does not decode or validate every member payload.
- Malformed or unsupported PAK archives get `status = "error"` plus the parser
  error; other archives continue to be inventoried.
- Root and optional `bin` EXE/DLL files receive SHA-256 hashes using a streaming
  64 KiB buffer. No original binary or archive is copied into the repository.
- `Ini/*.ini` version/language/subtitle configuration evidence includes source
  paths and one-based line numbers. UTF-16 BOM files are decoded; other files
  are read as UTF-8 with replacement of invalid sequences. Individual INI
  files are limited to 4 MiB.
- Symlinks and Windows reparse points are skipped. Recursive data traversal
  has a depth limit of 32. Missing Data is a warning. Inaccessible directories
  can fail the scan; INI and binary read errors become report warnings.
- Report paths are relative to the resolved installation root and use `/`.
  The report has no timestamp, so an unchanged installation yields identical
  serialized content. This is not an atomic filesystem snapshot; stop game
  patching or mod installation before comparing fingerprints.

## Remaining work

- Implement native PE version-resource parsing if version identities are needed
  without PowerShell.
- Determine CPT structure from format evidence; never parse it as PAK by suffix
  substitution.
- Correlate content tables and runtime observations with the game architecture.
  Installed filenames alone do not establish rendering or gameplay behavior.
- Add content dependency import and explicit patch precedence after verifying
  that precedence against format and/or original runtime evidence.
