---
name: sift
description: >-
  Search codebases with the sift CLI using indexed grep. Builds a trigram
  index once, then runs ripgrep-compatible queries 2 to 3x faster by skipping
  irrelevant files. Use when exploring a repository, finding symbols or
  patterns, grepping across a large codebase, or when the user mentions sift,
  indexed search, or .sift. Also use when the user wants faster grep results
  or is searching a repo with more than a few thousand files. Not for
  developing sift itself (crates/cli, sift-core, clap, cargo test).
metadata:
  author: botirk38
  version: "1.0.0"
  tags: grep, search, index, trigram, ripgrep, code-search
allowed-tools: Read Grep Bash(sift:*) Bash(rg:*)
---

# sift

Indexed grep for codebases. Build an index once, then search with ripgrep-like flags. Without an index, sift falls back to a slower directory walk.

## When to use

- Searching a repository for patterns, symbols, or strings
- Exploring unfamiliar codebases (find usages, definitions, imports)
- The user mentions sift, indexed search, `.sift`, or wants faster grep
- Any repo with more than a few thousand files where grep speed matters

Do not use this skill for developing or debugging sift itself. Use the repository's `AGENTS.md` for that.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
sift --version
```

Upgrade: `sift update` (or re-run the install script).

Do not `cargo build` unless the user is in the sift source tree and asked to build from source.

## How it works

1. Build an index of trigrams (overlapping 3-byte sequences) for every file
2. At query time, decompose the pattern into trigram terms and intersect posting lists
3. Search only candidate files with the full regex engine

```bash
cd /path/to/repo
sift --sift-dir .sift index build --wait .   # one-time, blocking
sift "pattern" [PATH...]                     # indexed search
sift -l "pattern"                            # list matching files
sift -F "literal.string"                     # fixed string (no regex)
```

Refresh after large changes: `sift --sift-dir .sift index update --wait .`

## Workflow

```text
1. cd to repository root
2. Check for .sift/ directory (or confirm --sift-dir location)
3. If no index: sift --sift-dir .sift index build --wait .
4. After repo changes: sift --sift-dir .sift index update --wait .
5. Narrow with sift -l "pattern" [PATH...]
6. Full search; use -F for literals with regex metacharacters
7. Use --json only when parsing output programmatically
```

## Indexed vs walk mode

**Index present** (`.sift` directory built): fast trigram narrowing, searches only candidate files. Search paths must be under the indexed corpus root.

**No index**: walk mode from cwd only, comparable to scanning without indexing. Always `cd` to the repo root and run `index build --wait` before serious exploration.

## Rules

- Global `--sift-dir` goes before subcommands: `sift --sift-dir .sift index build .`
- `index build` creates an index; `index update` refreshes it. Both are async by default, `--wait` blocks
- `sift update` upgrades the binary, not the index
- Rust `regex` syntax by default; `-F` for fixed strings
- To search for a pattern that matches a subcommand name: `sift -- index` or `sift -e index`
- `-h` is help (not "no filename"); use `--no-filename` instead
- Scripts and CI: `export SIFT_NO_DAEMON=1` to disable background daemon

## Additional resources

- [reference.md](reference.md): all flags, daemon details, limitations, rg differences
- [README.md](../../README.md): user quick start
- [docs/rg-compat-matrix.md](../../docs/rg-compat-matrix.md): flag parity with ripgrep
