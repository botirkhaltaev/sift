# sift — agent notes

Short orientation for tools and contributors. Product direction and phased roadmap live in **`plan.md`** when that file exists.

## Layout

| Path | Role |
|------|------|
| `crates/core` | `sift-core` — index build, `Index`, `CompiledSearch`, search pipeline |
| `crates/cli` | `sift-cli` — `sift` binary (clap), thin wrapper over core |
| `fuzz/` | `cargo-fuzz` (excluded from workspace) — `fuzz/README.md` |
| `scripts/` | `bench.sh`, `profile.sh`, `fuzz.sh` |
| `skills/` | Optional agent skills — `skills/README.md` |
| `crates/core/benches/README.md` | Criterion + profiling |

## CI-equivalent checks

Same as `.github/workflows/ci.yml` (Ubuntu, macOS; stable Rust):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

**Mandatory precommit procedure**: Run all three CI-equivalent commands above **before pushing**. Fix any failures locally, then push.

Bench / `sift-profile`: package + features in `crates/core/benches/README.md` and `crates/core/README.md`. Fuzz is manual: `./scripts/fuzz.sh`.

## Conventions

- No `unsafe`. Workspace clippy is strict; CI uses `-D warnings`.
- Small, focused changes; follow existing patterns in the crate you touch.
- Do not commit `target/`, `.cursor/`, local `.sift/` (see `.gitignore`).
- Prefer fixing lints over `#[allow(clippy::…)]` unless there is a rare, documented reason.
- Larger roadmap slices: **one branch per slice**, PR, merge, then start the next slice from an updated default branch — details in `plan.md` when present.

### Branch names

Use **short, descriptive kebab-case** names so history and open PRs stay readable. Prefer a **type prefix** when it fits:

| Prefix | Use for |
|--------|---------|
| `feat/` | New behavior, flags, or API |
| `fix/` | Bug fixes, regressions |
| `docs/` | Documentation only |
| `chore/` | Tooling, CI, refactors with no user-visible change |

**Good:** `feat/stats-elapsed`, `fix/ignore-git-without-repo`, `docs/rg-compat-matrix`  
**Avoid:** opaque labels like `phase-4` or `wip` with no topic — they force readers to open the PR to learn what changed.

Rename a local branch before push: `git branch -m old-name new-name`, then `git push -u origin new-name` (and delete the old remote branch if it was already pushed).

## Core API (entry points)

`IndexBuilder::build`, `Index::open`, `CompiledSearch::new`, then indexed `run_index` or walk-based search as in `crates/core/README.md`.
