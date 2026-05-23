# sift-profile

Hot-loop profiling binary for `sift-core`. Built only with `--features profile`.

## Usage

```bash
# List available scenarios
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- list

# Run a scenario
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow

# Search-only (cleaner for perf attribution)
cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- search-only no_literal

# Flamegraph
cargo flamegraph --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow

# System profiler (macOS)
sample $(cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow) 10000

# /usr/bin/time (RSS + wall+sys+user)
/usr/bin/time -l cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow

# Large corpus (8k files)
SIFT_PROFILE_LARGE=1 cargo run --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow
```

## Modules

| File | Description |
|------|-------------|
| [`main.rs`](main.rs) | CLI entry point — `list`, `run`, `search-only`, `build`, `hints` subcommands |
| [`corpus.rs`](corpus.rs) | Synthetic corpus materialization (parity, filter, large) |
| [`scenarios.rs`](scenarios.rs) | Scenario definitions — pattern + `SearchOptions` combinations |
| [`run.rs`](run.rs) | Pipeline execution — build index, warmup, timed iteration loop |
| [`metrics.rs`](metrics.rs) | Per-iteration timing collection and aggregation |
| [`stats.rs`](stats.rs) | TSV output formatting (`profile\tkey\tvalue` lines) |

## Environment Variables

| Variable | Effect |
|----------|--------|
| `SIFT_PROFILE_LARGE=1` | Use large synthetic corpus (8k files) |
| `SIFT_PROFILE_CORPUS` | External corpus root (skip materialization) |
| `SIFT_PROFILE_INDEX` | External index directory |
| `SIFT_PROFILE_ITERS` | Fixed iteration count |
| `SIFT_PROFILE_LOOP_SECS` | Timed loop duration (overrides `ITERS`) |
| `SIFT_PROFILE_WARMUP` | Warmup iterations before recording |
| `SIFT_PROFILE_RSS=1` | Print resident set size (Linux/macOS) |
