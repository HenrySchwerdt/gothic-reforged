# Fuzzing the file-format parsers

Every parser in `crates/formats` must handle arbitrary bytes:
- return `Ok` or `Err`, never panic;
- never allocate without bound;
- never loop forever.

Two layers check this, both on synthetic inputs only (no game files):

1. **Property tests** (`crates/formats/tests/robustness.rs`, part of `cargo test`). For each parser:
   - random bytes;
   - *structured* inputs: valid outer layers around random payloads;
   - mutated and truncated seeds;
   - every prefix of every seed.

   Each parse must finish in under 5 s, and the process peak RSS must stay under 1 GiB.
2. **libFuzzer targets** (`crates/formats/fuzz`, cargo-fuzz, nightly). There is one target per
   parser. Each target runs the parser on the raw input *and* on its structured form, with the
   seed corpus, a dictionary of magics and names, and `-malloc_limit_mb=512 -rss_limit_mb=2048
   -timeout=10`.

## Targets

| Target | Parser (entry point and the follow-up calls a loader makes) |
| --- | --- |
| `pak` | `PakArchive::read` + `read_payload` (G3V0 archive index, stored and zlib members) |
| `genome` | `genome::GenomeFile::parse` (GENOMFLE envelope + string table) |
| `ximg` | `ximg::Ximg::parse` + `to_dds` |
| `xcmsh` / `xlmsh` / `xnvmsh` | static mesh, LoD mesh, collision mesh (+ `triangles_cm`) |
| `xact` / `xmot` / `emfx` | actor (FXA), motion (LMA), EMFX container |
| `xshmat` / `xlmp` | shader material, lightmap |
| `template_summary` / `template_document` | `.tple` summary and full property decode |
| `world_node` / `world_props` / `vegetation` | world documents, with property sets, vegetation extraction |
| `spt` | SpeedTree `.spt` + `trunk_capsule_cm` + `spt_gen::generate` |
| `igl` / `theme` | image group list, UI theme |
| `stringtable` / `quest` / `info` / `gdini` | UTF-16 string table, quest / info / generic `.ini` records |
| `remaster_mesh` / `dds` | remaster overrides (`GRMO`, DDS) + `to_bytes` |
| `propstream` | `read_sub_class` and `decode_value` over the property-stream grammar |
| `weather` | `weather::parse` (`WeatherStates.g3ws`) + scale sampling (`at`, including out-of-range and NaN times) and environment lookups |
| `terrain_name` | `terrain::parse_cell_name` (landscape cell resource names) |

The shared harness is `gothic_formats::synth`:
- `run(target, bytes)` calls the parser;
- `structured(target, bytes)` maps arbitrary bytes to a format-shaped input, for example a
  GENOMFLE envelope with a valid table, the target's magic (`GENOMEDL`, `GENOMETP`, `G3IMG`
  header fields, `gena`/`FXA `, `LMA `, `__IdvSpt_02_`, `GRMO`, the DDS header, the class
  accessor) and class names in the string table;
- For `weather` and `igl`, `structured` builds a complete valid file: counts, property picks,
  string indices, scale keys, tile rectangles and so on all come from the fuzzer bytes. The
  fuzzer therefore explores the decoding, not only the magic checks.
  `structured_generators_produce_valid_files` in `tests/robustness.rs` keeps these
  generators valid.
- `seeds(target)` provides the seed inputs, including a valid two-member G3V0 archive.

The layouts come from `docs/research/*.md` and the parsers' own synthetic tests.

## Running

```sh
cargo install cargo-fuzz                        # once; needs a nightly toolchain
crates/formats/fuzz/run_all.sh 600 2            # every target 10 min, 2 in parallel
cd crates/formats/fuzz && cargo +nightly fuzz run -O xcmsh corpus/xcmsh -- -dict=dict.txt -max_total_time=60
cargo +nightly fuzz tmin -O xcmsh artifacts/xcmsh/crash-...   # minimise a finding
```

`run_all.sh` writes the seed corpus (`cargo run -p gothic-formats --example fuzz_seeds`),
builds the targets, and prints `ok` or `FAIL` per target. Logs go to `fuzz/logs/` and
findings to `fuzz/artifacts/<target>/`. A finding becomes a regression test in
`tests/robustness.rs` next to its fix.

CI (`.github/workflows/fuzz.yml`) runs every target for 60 s on PRs that touch
`crates/formats`, and for 10 minutes nightly or via *Run workflow*.
- **Corpus:** each target's corpus is kept in the Actions cache (`fuzz-corpus-<target>-*`).
  Every scheduled or manual run restores the newest one, fuzzes, minimises it with
  `cargo fuzz cmin` and saves it, so coverage accumulates over the nights. PR runs only
  restore it.
- **Findings:** crash inputs are uploaded as `fuzz-<target>` artifacts. When a nightly run
  fails, the `findings` job opens the tracking issue *Fuzzing: nightly findings* the first time
  and adds a comment with the run link after that.

## Findings so far

| Target | Finding | Fix |
| --- | --- | --- |
| `pak` | A zlib member's header could claim up to 256 MiB uncompressed. `read_payload` reserved that much before decoding a few bytes (700 MB RSS peak in the fuzzer). | Reserve at most `stored_size × 1032` (the deflate maximum ratio). The size check rejects the member as before. Regression test `zlib_member_cannot_claim_a_huge_size`. |

No panics, hangs or other memory blow-ups were found in the 60 s and 10 min runs per target
(numbers in the PR).
