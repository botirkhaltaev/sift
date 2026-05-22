# AGENTS.md — sift-profile

## Responsibility

Hot-loop profiling binary for `sift-core`. Produces `profile\tkey\tvalue` TSV lines for scripted benchmarking and flamegraph generation. Feature-gated behind `profile`.

## Structure

- `main.rs` — CLI entry: `list`, `run`, `search-only`, `build`, `hints`.
- `corpus.rs` — synthetic corpus materialization (parity, filter, large fixtures).
- `scenarios.rs` — scenario definitions (pattern + `SearchOptions`).
- `run.rs` — pipeline: build index → warmup → timed iteration loop.
- `metrics.rs` — per-iteration timing collection.
- `stats.rs` — TSV output formatting.

## Conventions

- Always use `./scripts/profile.sh` from the repo root for consistent environment.
- `SIFT_PROFILE_*` environment variables control corpus size, iteration count, and features.

## Do NOT

- Enable the `profile` feature in production builds.
- Change TSV output format without updating `scripts/profile.sh`.
