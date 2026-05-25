# Benchmarks

Criterion benchmark suite for `sift-core` and `sift-cli`.

## Layout

Benchmarks mirror the `src/` module layout and exercise only public APIs.

| File | What it measures |
|------|------------------|
| `query.rs` | `QueryPlanner` decisions, `PatternCompiler` shaping/compilation, `SearchQuery::new` |
| `index.rs` | `TrigramIndexBuilder::build`, `TrigramIndex::open`, `Indexes::open`, `Index` trait methods, `candidates`, `explain`, save/reopen |
| `grep.rs` | `SearchQuery::run` (indexed search / walk search), `CandidateFilter` paths, output modes |

Storage is benchmarked indirectly through `index.rs` build/open/save/reopen paths — no direct storage benchmarks.

## Running

```bash
# All core benches
cargo bench -p sift-core

# Per-target
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep

# CLI benches
cargo bench -p sift-cli --bench cli
./scripts/bench.sh cli

# Save / compare baselines
./scripts/bench.sh -- --save-baseline main
./scripts/bench.sh -- --baseline main
```

Pass Criterion flags after `--`: `cargo bench -p sift-core --bench query -- --help`

## Fixture Rules

- **Build benches** materialize corpus + build index inside `b.iter`.
- **Search/open/candidate benches** build fixtures outside `b.iter` and reuse the index inside `b.iter`.
- Shared fixtures live in `common/mod.rs`.

## Profiling

```bash
# Compile bench binaries without running
cargo bench -p sift-core --no-run
cargo bench -p sift-cli --no-run

# Smoke run each target
cargo bench -p sift-core --bench query -- --noplot
cargo bench -p sift-core --bench index -- --noplot
cargo bench -p sift-core --bench grep -- --noplot
cargo bench -p sift-cli --bench cli -- --noplot
```

Profile selected hot paths with `samply`, Instruments, or `sample <pid>` on macOS.
See Criterion HTML reports in `target/criterion/` for statistical breakdowns.
See [`PROFILING.md`](PROFILING.md) for detailed profiling findings and optimization opportunities.
