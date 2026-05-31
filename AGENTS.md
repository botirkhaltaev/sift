# AGENTS.md

Guidelines for AI agents working on the sift codebase.

## Project Overview

Sift is an indexed regex search engine for codebases, written in Rust. It builds on-disk indexes tuned to the search workload, then uses them to narrow candidate files before running the full regex engine. The shipped index type is a trigram index, achieving up to 60x speedup over ripgrep on indexed queries. The `IndexKind` enum and `Index` enum provide static dispatch; adding a new index kind means adding a variant to each.

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
| `crates/core/src/index/` | `Index` enum, `IndexKind` enum, `IndexStore`, and `Indexes` registry |
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

`IndexStore::open_or_create` → `IndexStore::build(kinds, config)` → `Indexes::open` → `SearchQuery::new` → `SearchQuery::run(SearchExecution)`. The `--indexes` flag on `sift build` selects which `IndexKind` variants to build (defaults to all). See `crates/core/README.md`.

## Architecture & Design

Prefer the best current design over backward compatibility. Do not preserve old
APIs, signatures, names, or structures by default when a cleaner architecture is
available. Preserve compatibility only when explicitly requested or when there is
a concrete persisted-data, shipped-behavior, external-consumer, or migration
requirement.

Write idiomatic Rust. Prefer strong domain types, explicit ownership, clear error
boundaries, and small composable interfaces. Redesign weak abstractions instead
of layering new behavior on top of them.

Keep APIs general and composable. Avoid helpers, method names, or signatures that
overfit one caller, one test, one branch, or one implementation detail. Name
types and functions after the domain concept they model, not the incidental
mechanism they use.

When behavior has distinct cases, model those cases directly with domain types.
Use enums for real alternatives, structs for coherent grouped data, and options
structs for configurable behavior. Avoid boolean flags when a named domain type
would make intent clearer.

Separate domain decisions from side effects. Prefer pure, testable logic that
returns decisions or actions, with I/O, filesystem access, spawning, logging,
locking, and channel communication kept at clear orchestration boundaries.

## Function Evolution

Do not create `*_with_*`, `*_locked`, `*_async`, `*_new`, or similarly named
parallel variants when the new function is the old function plus one extra
feature, mode, lock, flag, or parameter. This creates duplicate execution paths
and weakens the domain model.

If a different signature is needed, evolve the original API around the domain
concept. Use a domain enum, options struct, or grouped parameter type as
appropriate rather than creating parallel variant functions.

If behavior gains another input or mode:
- Evolve the original function body so it owns the concept.
- Introduce a domain type that represents the concept.
- Use a small private helper named after the **domain operation** it performs,
  not after how it differs from the variant it serves.

Examples of **bad** names that flag the pattern:
- `build_locked` (the variant adds a lock)
- `current_with_lease` (the variant adds a lease)
- `run_search_with_index` (the variant adds an index)
- `open_with_lease`

Examples of **good** names that describe the domain action:
- `publish_snapshot` (it writes files and commits)
- `resolve_candidates` (it looks up matching files)
- `build_index_metadata`

When a lifecycle function needs to write to either a directory or a
snapshot store, use a domain enum instead of `*_to_dir` / `*_into` variants:

```rust
// Do this:
pub fn build(config: &IndexConfig<'_>, dest: IndexDestination) -> Result<Self>;

// NOT this (parallel variants):
fn build(config, output_dir) -> Result;     // directory
fn build_into(config, writer, ns) -> Result; // snapshot
```

Small local helpers are acceptable only when they remove duplication inside one
function or one orchestration path, and their name describes what they do, not
how they differ from an alternate path.

## IndexSource / IndexDestination

TrigramIndex lifecycle functions (`build`, `open`, `update`) and IndexKind
lifecycle functions (`build`, `open`, `update`) use `IndexSource` and
`IndexDestination` domain types instead of parallel variants:

- `IndexSource` — describes where index data is read from:
  `Directory(&Path)` or `Snapshot { reader, namespace }`.
- `IndexDestination` — describes where index data is written to:
  `Directory(&Path)` or `Snapshot { writer, namespace }`.

Each function dispatches internally on the enum variant. See
`crates/core/src/index/mod.rs` for the type definitions,
`crates/core/src/index/trigram/lifecycle.rs` for the TrigramIndex lifecycle,
and `crates/core/src/index/kinds.rs` for IndexKind dispatch.

## Module Organization

Organize modules by domain responsibility, not by Rust item category. Avoid
catch-all files such as `types.rs`, `traits.rs`, `helpers.rs`, or `utils.rs`
unless the domain itself is genuinely that narrow. Prefer file/module names that
describe the behavior or concept they own. Use nested modules when a domain has
clear subdomains, such as `snapshot/store/disk.rs` and
`snapshot/store/memory.rs`.

## Do NOT

- Skip CI checks (`fmt`, `clippy`, `test`) before pushing.
- Add dependencies without justification.
- Commit secrets, `.env` files, or editor-specific directories.
- Use `#[allow]` attributes.
