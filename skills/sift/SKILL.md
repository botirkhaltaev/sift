---
name: sift
description: >-
  Searches codebases with the sift CLI (indexed grep): build a .sift index,
  run ripgrep-like queries, list matching files, or emit JSON. Use when
  exploring a repository, finding symbols or patterns, or when the user
  mentions sift, indexed search, or .sift. Not for developing sift
  (crates/cli, sift-core, clap, cargo test).
disable-model-invocation: true
---

# sift

Indexed grep for codebases. Build an index once, then search with ripgrep-like flags. Without an index, sift falls back to a slower directory walk from the current working directory.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
sift --version
```

Upgrade the binary: `sift update` (or re-run the install curl command).

Do not `cargo build` unless the user is in the sift source tree and asked to build from source.

## Quick start

```bash
cd /path/to/repo
sift --sift-dir .sift index build .
sift "pattern" [PATH...]
sift -l "pattern"
sift -F "literal.string"
```

Refresh the index after large changes: `sift --sift-dir .sift index update .` (async) or `sift index update --wait .` (blocking).

Both `index build` and `index update` are async via the daemon by default. Use `--wait` for blocking behavior.

Default index directory is `.sift` (override with `--sift-dir` on every command).

## Agent workflow

```text
- [ ] cd to repository root
- [ ] Check for .sift/ (or confirm --sift-dir)
- [ ] If no index: sift --sift-dir .sift index build . (async by default; --wait for blocking)
- [ ] After repo changes: sift --sift-dir .sift index update . (or --wait for blocking)
- [ ] Narrow with sift -l "pattern" [PATH...]
- [ ] Full search; use -F for literals with regex metacharacters
- [ ] Use --json only when parsing output programmatically
```

## Indexed vs walk

**Index present** (`.sift` built): fast trigram narrowing; search paths must be under the indexed corpus root.

**No index**: walk mode from **cwd** only—comparable to scanning without indexing. Always `cd` to the repo root and run `index build` before serious exploration.

## Rules

- Global `--sift-dir` before index commands: `sift --sift-dir .sift index build .`
- `sift index build` creates an index (async via daemon by default); `--wait` forces blocking
- `sift index update` refreshes asynchronously; `--wait` blocks until done
- Search auto-indexes unindexed files touched during a walk when the daemon is enabled
- `sift update` upgrades the **binary**, not the index
- Rust `regex` syntax by default; `-F` for fixed strings
- Literal subcommand in pattern position: `sift -- index build` or `-e index`
- `-h` is help (not “no filename”); use `--no-filename` instead
- Scripts and CI: `export SIFT_NO_DAEMON=1` to avoid background index daemon

## Out of scope

Developing or debugging sift itself (CLI crate, clap, integration tests, `cargo test`) is **not** covered here. Use the repository’s `AGENTS.md` and `crates/cli/AGENTS.md` for that work.

## Additional resources

- [reference.md](reference.md) — flags, daemon, limitations, rg differences
- [README.md](../../README.md) — user quick start
- [docs/rg-compat-matrix.md](../../docs/rg-compat-matrix.md) — flag parity with ripgrep
