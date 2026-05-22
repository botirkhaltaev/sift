# Benchmarks

Criterion benchmark suite for `sift-core` and the `sift-profile` hot-loop profiling binary.

## Running

```bash
# Criterion (statistical)
cargo bench -p sift-core --bench search
./scripts/bench.sh

# Save / compare baselines
./scripts/bench.sh -- --save-baseline main
./scripts/bench.sh -- --baseline main

# sift-profile (TSV output)
./scripts/profile.sh list
./scripts/profile.sh run literal_narrow
./scripts/profile.sh search-only no_literal
./scripts/profile.sh flamegraph literal_narrow

# Large corpus
SIFT_PROFILE_LARGE=1 ./scripts/profile.sh run literal_narrow
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

For CPU/call-stack analysis beyond timings:

```bash
# Flamegraph (recommended)
./scripts/system-profile.sh literal_narrow

# Linux perf
./scripts/system-profile.sh --perf no_literal
```

Open the generated `flamegraph.svg` — wide boxes indicate hot stacks. Compare `search-only` vs full `run` to isolate `run_index` vs planning.
