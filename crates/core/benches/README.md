# Benchmarks

`crates/core/benches/search.rs` contains the Criterion benchmark suite for sift-core.
`crates/core/src/bin/sift_profile/` holds the `sift-profile` binary (hot-loop profiling, `profile` feature).

## Scenario matrix

The benchmark matrix is divided into three categories that reflect the runtime
execution paths:

### Query-planning (trigram/verify paths)

| Scenario | Pattern | SearchOptions |
|---|---|---|
| `literal_narrow` | `beta` | default (narrowable) |
| `literal_narrow_large` | `beta` | default, 8k files |
| `search_literal_narrow_corpus_scale` | `beta` | 100 / 1k / 8k files (sweep) |
| `word_literal` | `beta` | `WORD_REGEXP` |
| `line_literal` | `beta` | `LINE_REGEXP` |
| `fixed_string` | `beta.gamma` | `FIXED_STRINGS` |
| `casei_literal` | `beta` | case-insensitive |
| `smart_case_lower` | `beta` | smart-case (lowercase → ci) |
| `smart_case_upper` | `Beta` | smart-case (uppercase → cs) |
| `required_literal` | `[A-Z]+_RESUME` | default (requires trigram) |
| `no_literal` | `\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}` | full scan |
| `alternation` | `ERR_SYS\|...` | default |
| `alternation_casei` | `ERR_SYS\|...` | case-insensitive |
| `unicode_class` | `\p{Greek}` | default |

### Filter + query (SearchFilter paths)

| Scenario | Filter | Corpus |
|---|---|---|
| `glob_include` | `**/*.txt` glob | filter_corpus |
| `glob_exclude` | `!**/*.txt` glob | filter_corpus |
| `glob_casei` | `**/*.TXT` ci-glob | filter_corpus |
| `hidden_default` | `HiddenMode::Respect` | filter_corpus |
| `hidden_include` | `HiddenMode::Include` | filter_corpus |
| `ignore_default` | DOT+VCS+EXCLUDE | filter_corpus |
| `ignore_custom` | custom `.ignore` file | filter_corpus |
| `scoped_search` | scope: `subdir/` | filter_corpus |

### Output-mode (run_index mode branches)

| Scenario | SearchMode | Notes |
|---|---|---|
| `only_matching` | `OnlyMatching` | `-o` equivalent |
| `count` | `Count` | `-c` equivalent |
| `count_matches` | `CountMatches` | `--count-matches` |
| `files_with_matches` | `FilesWithMatches` | `-l` equivalent |
| `files_without_match` | `FilesWithoutMatch` | `-L` equivalent |
| `max_count_1` | `Standard` | `-m 1` per-file cap |

## Corpus fixtures

- **parity**: 2 files (`a/x.txt`, `b/y.txt`) — fast turnaround for quick iteration
- **filter_corpus**: 12 files with mixed extensions, hidden files, scoped subdirs,
  `.gitignore`, and `.ignore` markers — exercises all filter branches
- **large**: ~8k files × 100 lines across 256 crate dirs — for statistical significance
  on warm caches; enable with `SIFT_PROFILE_LARGE=1`

## Running

```bash
# Criterion (statistical)
cargo bench -p sift-core --bench search
./scripts/bench.sh

# Save / compare baselines
./scripts/bench.sh -- --save-baseline main
./scripts/bench.sh -- --baseline main

# sift-profile (tab-separated `profile` lines; see `sift-profile hints`)
cargo run -p sift-core --features profile --bin sift-profile -- list
cargo run -p sift-core --features profile --bin sift-profile -- hints
./scripts/profile.sh list

./scripts/profile.sh run literal_narrow
# shorthand:
./scripts/profile.sh literal_narrow

./scripts/profile.sh run glob_include
./scripts/profile.sh run count
./scripts/profile.sh search-only no_literal

# Use large corpus
SIFT_PROFILE_LARGE=1 ./scripts/profile.sh run literal_narrow

# Flamegraph
./scripts/profile.sh flamegraph literal_narrow

# Custom corpus
SIFT_PROFILE_CORPUS=/path/to/repo ./scripts/profile.sh run literal_narrow
```

## System profiling (CPU / call stacks)

`sift-profile` prints **timings** (`profile` lines). To see **which functions use CPU**, use stack sampling:

- **Script (recommended):** `./scripts/system-profile.sh` — builds with frame pointers, runs a **steady-state** search (`SIFT_PROFILE_LOOP_SECS`, default 25s), then records a **flamegraph** (`cargo install flamegraph`) or on **Linux** `perf` with `./scripts/system-profile.sh --perf no_literal`.
- **Interpretation:** Open the SVG (usually `flamegraph.svg` in the repo root) — wide boxes = hot stacks. Compare **`search-only`** vs full **`run`** to focus on `run_index` vs planning.
- **macOS:** Flamegraph may use **dtrace** and need **sudo**; or use **Xcode Instruments → Time Profiler** on `sift-profile` while it runs a long `SIFT_PROFILE_LOOP_SECS` loop.

## `sift-profile` environment (`SIFT_PROFILE_*`)

| Variable | Effect |
|---|---|
| `SIFT_PROFILE_LARGE=1` | Use large corpus (8k files) |
| `SIFT_PROFILE_CORPUS_FILES` | Custom file count |
| `SIFT_PROFILE_CORPUS_LINES` | Lines per file (large corpus) |
| `SIFT_PROFILE_CORPUS_DIRS` | Directory fan-out (large corpus) |
| `SIFT_PROFILE_FILTER_CORPUS` | Force filter_corpus (parity default) |
| `SIFT_PROFILE_ITERS` | Fixed iteration count for `run` / `build` |
| `SIFT_PROFILE_LOOP_SECS` | Timed `run` (seconds); overrides `SIFT_PROFILE_ITERS` |
| `SIFT_PROFILE_WARMUP` | `run_index` iterations before recording per-iter samples |
| `SIFT_PROFILE_RSS=1` | Print resident set size before/after (Linux/macOS) |
| `SIFT_PROFILE_CORPUS` | External corpus path |
| `SIFT_PROFILE_INDEX` | Index directory (default: `<corpus>.sift`) |
