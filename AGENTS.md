# AGENTS.md

Guidelines for AI agents working on the sift codebase.

## Project Overview

Sift is an indexed regex search engine for codebases, written in Rust. It builds on-disk indexes tuned to the search workload, then uses them to narrow candidate files before running the full regex engine. The shipped index type is a trigram index, achieving up to 60x speedup over ripgrep on indexed queries. The `SearchIndex` trait makes the system pluggable: new index kinds can be added alongside the trigram index.

## Build & Test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Run all three before pushing. CI enforces the same checks on Linux, macOS, and Windows.

## Layout

| Path | Role |
|------|------|
| `crates/core/` | `sift-core`: query planning, index-backed candidate narrowing, search engine |
| `crates/core/src/query/` | Query description, planning, candidate plans |
| `crates/core/src/index/` | Generic `SearchIndex` trait and concrete implementations |
| `crates/core/src/index/trigram/` | Trigram index: build, storage, and search |
| `crates/core/src/grep/` | Grep-style matching, scanning, output, filtering |
| `crates/cli/` | `sift-cli`: `sift` binary (clap CLI over core) |
| `fuzz/` | `cargo-fuzz` targets (standalone package, nightly) |
| `benchsuite/` | Comparative `rg` vs `sift` benchmarks |
| `scripts/` | `bench.sh`, `fuzz.sh`, `install.sh` |
| `skills/` | Agent skills (`skills.sh` / `npx skills`) |
| `docs/` | Performance snapshots, compatibility matrix |

## Key Conventions

- **No `unsafe`** except in `index/trigram/storage/mmap.rs` (documented safety invariant).
- **Strict clippy:** workspace uses `pedantic + nursery + cargo` warnings; CI uses `-D warnings`.
- Fix lints at the root cause. `#[allow]` is **never** permitted.
- Small, focused changes; follow existing patterns in the crate you touch.
- Do not commit `target/`, `.cursor/`, local `.sift/` directories.

## Branch Names

Use short, descriptive kebab-case with a type prefix:

| Prefix | Use for |
|--------|---------|
| `feat/` | New behavior, flags, or API |
| `fix/` | Bug fixes, regressions |
| `docs/` | Documentation only |
| `chore/` | Tooling, CI, refactors with no user-visible change |

## Core API Entry Points

`Indexes::open` loads all available indexes. `SearchQuery::new` compiles the regex. `SearchQuery::run(SearchExecution)` scans candidates. Currently the shipped index is the trigram index, built via `TrigramIndexBuilder::build`. See `crates/core/README.md`.

## Function Evolution

Prefer evolving existing orchestration functions and domain types over adding
parallel `*_with_*` functions or free-floating helpers.

If behavior gains another input or mode, modify the original function body or
introduce a domain object that owns the concept. Avoid overload-style variants
such as `run_search_with_index`, `run_search_walk`, or `open_*` helper functions.

Small local helpers are acceptable only when they remove duplication inside one
function and do not become alternate execution paths.

## Do NOT

- Skip CI checks (`fmt`, `clippy`, `test`) before pushing.
- Add dependencies without justification.
- Commit secrets, `.env` files, or editor-specific directories.
- Use `#[allow]` attributes.
