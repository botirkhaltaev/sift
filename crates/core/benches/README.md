# Benchmarks

Criterion suite for `sift-core` (and `sift-grep` CLI benches). Designed so each
function is one domain operation under samply / xctrace `--profile-time`.

## Corpus scale

Search and planner benches use a **monorepo-scale** synthetic tree, not a toy
corpus. Defaults match mid-size codebases; override with `SIFT_BENCH_SCALE`:

| `SIFT_BENCH_SCALE` | Files × lines/file | Approx lines | Use |
|--------------------|--------------------|--------------|-----|
| unset / `default` | 32 000 × 160 | ~5.1M | Normal local profiling |
| `stress` / `large` | 64 000 × 200 | ~12.8M | Kernel-ish stress |
| `ci` / `small` | 8 000 × 100 | ~0.8M | Fast smoke / CI |

Fixtures are cached under `$CARGO_TARGET_DIR/sift-bench-fixtures/` (first run
materializes + indexes; later runs reopen). Tiny ignore/filter corpora stay
named `*_tiny` and are **not** search signal.

Index *build* benches that rematerialize inside `b.iter` keep the medium
`BUILD` scale (8k×100) so samples stay tractable; `prebuilt_search_scale`
builds the same dimensions as grep/candidates.

## Layout (`sift-core`)

| Target | Groups (filter ids) | `iter` measures |
|--------|---------------------|-----------------|
| `query` | `query_compile/*` | `Searcher` construction |
| `index` | build / open / update / candidates / explain | named lifecycle op |
| `grep` | `grep_search/*`, `grep_pipeline/*`, `grep_walk_tiny/*` | search-only, plan+search, tiny walk |
| `candidates` | `candidate_planner/*`, `candidate_planner_tiny/*` | `CandidatePlanner::resolve` |

### Stable `grep` ids

- `grep_search/literal`
- `grep_search/required_literal`
- `grep_search/alternation`
- `grep_search/case_insensitive`
- `grep_search/full_scan`
- `grep_search/invert_match`
- `grep_pipeline/literal`
- `grep_pipeline/full_scan`
- `grep_walk_tiny/literal`

### Stable `candidates` ids

- `candidate_planner/use_index_literal`
- `candidate_planner/all_indexed_complete`
- `candidate_planner/lazy_merge_index_and_walk`
- `candidate_planner_tiny/walk_fallback_empty_index`

### Stable CLI e2e large ids

- `e2e/subprocess/indexed_large/literal`
- `e2e/subprocess/indexed_large/required_literal`
- `e2e/subprocess/indexed_large/full_scan`
- `e2e/subprocess/indexed_large/alternation`

## Running

```bash
./scripts/bench.sh              # all sift-core (default search scale)
./scripts/bench.sh grep
./scripts/bench.sh candidates
SIFT_BENCH_SCALE=ci ./scripts/bench.sh grep   # faster smoke
SIFT_BENCH_SCALE=stress ./scripts/bench.sh grep

./scripts/bench.sh cli          # includes e2e/subprocess/indexed_large/*

./scripts/bench.sh grep -- --save-baseline pre-opt --noplot
./scripts/bench.sh grep -- --baseline pre-opt --noplot
```

## Profiling

System-profile Criterion binaries (not guesses from docs). `[profile.bench]`
already keeps line tables (`debug = 1`).

```bash
./scripts/profile.sh --bench grep --profile-time 30 -- grep_search/full_scan
./scripts/profile.sh --bench candidates --profile-time 30 -- candidate_planner/use_index_literal

# Optional clearer stacks:
./scripts/profile.sh --bench grep --frame-pointers --profile-time 30 -- grep_search/full_scan

# CLI escape hatch (not the default loop):
./scripts/profile.sh --cli -- target/release/sift --sift-dir /tmp/x.sift -n beta
```

`profile.sh` prefers samply and falls back to `xctrace` Time Profiler on macOS.
Log findings in [`PROFILING.md`](PROFILING.md) before changing product code.
Criterion HTML reports live under `target/criterion/`.

## Fixture rules

- **Build benches** materialize corpus + build index inside `b.iter` (BUILD scale).
- **Search / open / candidate / resolve benches** use the cached search-scale fixture outside `b.iter`.
- Shared helpers live in `common/` (`criterion_config`, `fixtures`).
