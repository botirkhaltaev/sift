# Benchmarks

Criterion benchmark suite for `sift-core` and `sift-cli`, plus the `sift-profile` hot-loop profiling binary.

## Running

```bash
# Criterion (statistical) — core
cargo bench -p sift-core --bench search
./scripts/bench.sh

# Criterion (statistical) — cli
cargo bench -p sift-cli --bench cli
./scripts/bench.sh cli

# Save / compare baselines
./scripts/bench.sh -- --save-baseline main
./scripts/bench.sh -- --baseline main

# sift-profile (TSV output)
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- list
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- search-only no_literal

# System profiling (macOS sample, flamegraph)
sample sift-profile 10000 -maybeFile -F runtime 2>/dev/null | xcrun symbolicate
cargo flamegraph --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow

# Large corpus
SIFT_PROFILE_LARGE=1 cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow
```

## Scenario Matrix

### Query Planning (trigram/verify paths)

| Scenario | Pattern | Options |
|----------|---------|---------|
| `literal_narrow` | `beta` | default (narrowable) |
| `literal_narrow_large` | `beta` | default, 8k files |
| `word_literal` | `beta` | `WORD_REGEXP` |
| `line_literal` | `beta` | `LINE_REGEXP` |
| `fixed_string` | `beta.gamma` | `FIXED_STRINGS` |
| `casei_literal` | `beta` | case-insensitive |
| `smart_case_lower` | `beta` | smart-case (lowercase → ci) |
| `smart_case_upper` | `Beta` | smart-case (uppercase → cs) |
| `required_literal` | `[A-Z]+_RESUME` | default |
| `no_literal` | `\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}` | full scan |
| `alternation` | `ERR_SYS\|...` | default |
| `alternation_casei` | `ERR_SYS\|...` | case-insensitive |
| `unicode_class` | `\p{Greek}` | default |

### Filter + Query (SearchFilter paths)

| Scenario | Filter | Corpus |
|----------|--------|--------|
| `glob_include` | `**/*.txt` glob | filter_corpus |
| `glob_exclude` | `!**/*.txt` glob | filter_corpus |
| `glob_casei` | `**/*.TXT` ci-glob | filter_corpus |
| `hidden_default` | `HiddenMode::Respect` | filter_corpus |
| `hidden_include` | `HiddenMode::Include` | filter_corpus |
| `ignore_default` | DOT+VCS+EXCLUDE | filter_corpus |
| `ignore_custom` | custom `.ignore` | filter_corpus |
| `scoped_search` | scope: `subdir/` | filter_corpus |

### Output Mode (run_index branches)

| Scenario | Mode | Notes |
|----------|------|-------|
| `only_matching` | `OnlyMatching` | `-o` equivalent |
| `count` | `Count` | `-c` equivalent |
| `count_matches` | `CountMatches` | `--count-matches` |
| `files_with_matches` | `FilesWithMatches` | `-l` equivalent |
| `files_without_match` | `FilesWithoutMatch` | `-L` equivalent |
| `max_count_1` | `Standard` | `-m 1` per-file cap |

## Corpus Fixtures

| Corpus | Description |
|--------|-------------|
| parity | 2 files — fast turnaround for quick iteration |
| filter_corpus | 12 files with mixed extensions, hidden files, `.gitignore` markers |
| large | ~8k files × 100 lines — enable with `SIFT_PROFILE_LARGE=1` |

## System Profiling

For CPU/call-stack analysis beyond Criterion timings, use macOS built-in tools:

```bash
# macOS sample (stacks every 1ms)
sample sift-profile 10000 -maybeFile 2>/dev/null | xcrun symbolicate

# Flamegraph (requires cargo-flamegraph)
cargo flamegraph --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow

# Time + RSS
/usr/bin/time -l cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow
```

Open the generated `flamegraph.svg` — wide boxes indicate hot stacks. Compare `search-only` vs full `run` to isolate `run_index` vs planning.
