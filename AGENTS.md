# AGENTS.md

Guidelines for AI agents working on the sift codebase.

## Project Overview

Sift is an indexed regex search engine for codebases, written in Rust. It builds a trigram index on disk and uses it to narrow candidate files before running the full regex engine — achieving up to 60× speedup over ripgrep on indexed queries.

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
| `crates/core/` | `sift-core` — trigram index, query planner, search engine |
| `crates/cli/` | `sift-cli` — `sift` binary (clap CLI over core) |
| `fuzz/` | `cargo-fuzz` targets (standalone package, nightly) |
| `benchsuite/` | Comparative `rg` vs `sift` benchmarks |
| `scripts/` | `bench.sh`, `profile.sh`, `fuzz.sh`, `install.sh` |
| `skills/` | Agent skills (`skills.sh` / `npx skills`) |
| `docs/` | Performance snapshots, compatibility matrix |

## Key Conventions

- **No `unsafe`** except in `storage/mmap.rs` (documented safety invariant).
- **Strict clippy:** workspace uses `pedantic + nursery + cargo` warnings; CI uses `-D warnings`.
- Prefer fixing lints over `#[allow(clippy::…)]`.
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

`IndexBuilder::build` → `Index::open` → `CompiledSearch::new` → `run_index` / `search_index`. See `crates/core/README.md`.

## Do NOT

- Skip CI checks (`fmt`, `clippy`, `test`) before pushing.
- Add dependencies without justification.
- Commit secrets, `.env` files, or editor-specific directories.
- Use `#[allow]` attributes without a documented reason.
