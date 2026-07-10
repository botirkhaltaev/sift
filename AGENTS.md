# AGENTS.md

Guidelines for AI agents working on the sift codebase.

## Project Overview

Sift is an indexed code search engine written in Rust, built around **composable on-disk indexes**. It builds indexes tuned to the search workload, then uses them to narrow candidate files before running the full regex engine.

The core architecture treats code search like database query execution: multiple index configurations can coexist, each narrowing candidates independently, with the `Indexes` registry intersecting their results. Today, Sift ships a runtime-width N-gram index that defaults to trigram width. `IndexConfig` records configured/persisted index identity, `IndexStore` owns build/open/update transactions, and `Index` is the opened query-time runtime dispatch. Future index types (AST indexes, dependency graphs, vector indexes) slot into the same architecture.

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
| `crates/core/` | `sift-core`: composable index registry, query planning, candidate narrowing, search engine |
| `crates/core/src/candidates/` | Index-agnostic candidate description, planning, and resolution |
| `crates/core/src/index/` | `IndexConfig` / `Index` dispatch, `Indexes` registry, `IndexStore`, snapshot persistence |
| `crates/core/src/index/ngram/` | N-gram index: generic implementation plus trigram specialization (first shipped index type) |
| `crates/core/src/grep/` | Grep search API and matcher execution |
| `crates/cli/` | `sift-cli`: `sift` binary (clap CLI over core) |
| `fuzz/` | `cargo-fuzz` targets (standalone package, nightly) |
| `benchsuite/` | Comparative `rg` vs `sift` benchmarks |
| `scripts/` | `bench.sh`, `fuzz.sh`, `install.sh` |
| `skills/` | Agent usage skill for searching with `sift` (`npx skills`); CLI development → `crates/cli/AGENTS.md` |
| `docs/` | Performance snapshots, compatibility matrix |

## Key Conventions

- **No `unsafe`** except in `index/mmap.rs` (documented safety invariant).
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

`IndexStore::open_or_create` → `IndexStore::build(configs, build_config)` → `Indexes::open` → `Query::new` → `Query::candidates` / `Query::search`. CLI: `RunConfig` → `Run::execute` → `SearchPrinter`. See `crates/core/README.md`.

## Architecture & Design

Prefer the best current design over backward compatibility. Do not preserve old
APIs, signatures, names, or structures by default when a cleaner architecture is
available. Preserve compatibility only when explicitly requested or when there is
a concrete persisted-data, shipped-behavior, external-consumer, or migration
requirement.

Write idiomatic Rust. Prefer strong domain types, explicit ownership, clear error
boundaries, and small composable interfaces. Redesign weak abstractions instead
of layering new behavior on top of them.

**Keep the design general, and keep the code simple.** Prefer the smallest API
that expresses the domain concept. Do not add layers, wrappers, or special-case
branches for one caller, one test, one benchmark, or one feature flag.

### Composition over specialization

Callers compose domain operations. Callees expose general operations; they do
not grow boolean forks or parallel code paths for each use case.

- Model real alternatives with domain types (enums/structs), then let the caller
  pass the choice.
- Do not bake a use case into a callee when the caller can compose existing
  operations (`extract` → `lookup` → `intersect`, walk → filter → materialize).
- Avoid helpers, method names, or signatures that overfit one caller or one
  implementation detail.

### Naming

Name types and functions after the **domain concept**, with short simple words.
Do not name things after the mechanism, the caller, or how they differ from a
sibling (`*_casei_*`, `*_with_*`, `*_for_ascii_*`, `helper_*`, `utils`).

Do not use `_for_*` in method names to restate an argument
(`posting_ids_for_literal(lit, …)` → `posting_ids(lit, …)`). The parameter
already says what was passed; the method name should say what is returned or
done.

When adding request/config structs, name them after the domain decision they
represent, not the mechanical data they carry. Avoid vague bundles such as
`Context`, `State`, `Read`, or `Options` unless those are the actual domain
terms. Prefer names like `CandidateSource`, `ScanScope`, and
`IndexCoverage` that tell callers how to reason about the API.

Do not expose low-level planner knobs through higher-level APIs as loose fields.
Group related inputs behind a domain type owned by the layer making the
decision, and make each field describe a stable concept rather than a temporary
implementation detail.

When behavior has distinct cases, model those cases directly with domain types.
Use enums for real alternatives, structs for coherent grouped data, and options
structs for configurable behavior. Avoid boolean flags when a named domain type
would make intent clearer.

Separate domain decisions from side effects. Prefer pure, testable logic that
returns decisions or actions, with I/O, filesystem access, spawning, logging,
locking, and channel communication kept at clear orchestration boundaries.

## Function Evolution

**Evolve existing functions and APIs. Do not create new ones alongside them.**

Do not create `*_with_*`, `*_locked`, `*_async`, `*_new`, `*_casei_*`, or
similarly named parallel variants when the new function is the old function plus
one extra feature, mode, lock, flag, or parameter. That duplicates execution
paths and weakens the domain model.

If a different signature is needed:
- Change the original function to take a domain type for the new concept.
- Put the behavior in that one function body (match on the domain type).
- Delete the old shape rather than leaving a wrapper.

### No free helper functions

Do not add module-level free functions (`fn intersect_sorted_ids(...)`,
`fn resolve_*_from_args(...)`, `fn helper_*`) to share logic. Put behavior on
the type that owns the data (methods), or inline it at the single call site.

Nested closures or tiny blocks inside one function are fine when they remove
local duplication. A separate free function or a second method named after how
it differs from the first is not.

Examples of **bad** names that flag the pattern:
- `build_locked` (the variant adds a lock)
- `current_with_lease` (the variant adds a lease)
- `run_search_with_index` (the variant adds an index)
- `open_with_lease`
- `posting_ids_for_ascii_casei_literal` (parallel path for one mode)
- `intersect_sorted_ids` (free helper instead of a type method / inline)

Examples of **good** names that describe the domain action:
- `publish_snapshot` (it writes files and commits)
- `resolve_candidates` (it looks up matching files)
- `build_index_metadata`
- `posting_ids` with a `GramMatch` (or similar) argument

When a lifecycle function needs to write to either a directory or a
snapshot store, use a domain enum instead of `*_to_dir` / `*_into` variants:

```rust
// Do this:
pub fn build(config: &IndexConfig<'_>, dest: IndexDestination) -> Result<Self>;

// NOT this (parallel variants):
fn build(config, output_dir) -> Result;     // directory
fn build_into(config, writer, ns) -> Result; // snapshot
```

## IndexSource / IndexDestination

N-gram lifecycle functions (`build`, `open`, `update`) and `IndexConfig`
lifecycle functions (`build`, `open`, `update`) use `IndexSource` and
`IndexDestination` domain types instead of parallel variants:

- `IndexSource` — describes where index data is read from:
  `Directory(&Path)` or `Snapshot { reader, namespace }`.
- `IndexDestination` — describes where index data is written to:
  `Directory(&Path)` or `Snapshot { writer, namespace }`.

Each function dispatches internally on the enum variant. See
`crates/core/src/index/mod.rs` for the type definitions,
`crates/core/src/index/ngram/mod.rs` for the NGramIndex lifecycle,
and `crates/core/src/index/kinds.rs` for configured/runtime index dispatch.

## Module Organization

Organize modules by domain responsibility, not by Rust item category. Avoid
catch-all files such as `types.rs`, `traits.rs`, `helpers.rs`, or `utils.rs`
unless the domain itself is genuinely that narrow. Prefer file/module names that
describe the behavior or concept they own. Use nested modules when a domain has
clear subdomains, such as `snapshot/store/disk.rs` and
`snapshot/store/memory.rs`.

## CLI Crate

The shipped binary lives in `crates/cli/` (`sift-grep`). It follows the same
domain-type rules as core; see [`crates/cli/AGENTS.md`](crates/cli/AGENTS.md).
Clap parses `*Decl` flag groups; **`Argv` resolves effective runtime values**
(ripgrep last-wins). Do not add `resolve_*_from_args` free-function helpers.

## Do NOT

- Skip CI checks (`fmt`, `clippy`, `test`) before pushing.
- Add dependencies without justification.
- Commit secrets, `.env` files, or editor-specific directories.
- Use `#[allow]` attributes.
- Add free helper functions or parallel `*_with_*` / use-case-specific APIs —
  evolve the existing domain API instead (see Architecture & Design / Function
  Evolution).
- Overfit an API to one caller or test; keep operations general and let callers
  compose.

## Cursor Cloud specific instructions

- **Toolchain:** the workspace is `edition = "2024"`, so it needs Rust ≥ 1.85. The
  cloud VM's default was pinned to an older `1.83.0`; the environment now defaults
  to `stable` (`rustup default stable`). If a build fails with
  `feature edition2024 is required`, run `rustup default stable`.
- **Build / lint / test:** use the commands in `README.md` / the "Build & Test"
  section above (`cargo build --workspace`, `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`). No services or external deps needed.
- **Running the CLI:** the dev binary is `target/debug/sift` (bin name `sift`,
  crate `sift-grep`). You must build an index before searching, and search paths
  must sit under the indexed corpus root.
  - `index build` is async via a background daemon by default; pass `--wait` to
    build synchronously, or set `SIFT_NO_DAEMON=1` to disable the daemon.
  - Point `--sift-dir` at a writable index dir, e.g.:
    `target/debug/sift --sift-dir /tmp/demo/.sift index build --wait /tmp/demo`
    then `target/debug/sift --sift-dir /tmp/demo/.sift "pattern" /tmp/demo`.
