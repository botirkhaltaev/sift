# sift reference

## Commands

| Command | Purpose |
|---------|---------|
| `sift update` | Upgrade the installed **binary** (latest GitHub release) |
| `sift index build [PATH]` | Create an index (fails if one already exists) |
| `sift index update [PATH]` | Incrementally refresh an existing index |
| `sift PATTERN [PATH...]` | Search (indexed or walk mode) |

## Common flags

| Flag | Purpose |
|------|---------|
| `-i` | Case-insensitive |
| `-w` | Whole word |
| `-F` | Fixed string (no regex) |
| `-c` | Count matches per file |
| `-l` | List matching files only |
| `-L` / `--follow` | Follow symlinks (index and search) |
| `-g GLOB` | Filter paths by glob |
| `-A` / `-B` / `-C` | Context lines |
| `--json` | JSON Lines output |
| `--stats` | Summary on stderr |
| `-0` / `--null` | NUL-separated paths |
| `--no-filename` | Omit path prefix (not `-h`) |
| `-j N` / `--threads N` | Rayon thread count |
| `--sift-dir DIR` | Index directory (default `.sift`) |

Patterns: positional, or `-e PATTERN`, or `-f FILE`. Multiple patterns are OR’d unless configured otherwise.

## Index

```bash
sift --sift-dir .sift index build .
sift --sift-dir .sift index update .
sift --sift-dir .sift index build --indexes trigram .
```

- `PATH` defaults to `.`; can be a single file (indexes parent directory).
- `--indexes` selects index kinds (default: all; shipped: `trigram`).
- Search paths must lie under the indexed **corpus root** when an index exists.

## Binary upgrade

```bash
sift update
# or
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
```

Environment: `SIFT_REPO`, `SIFT_VERSION`, `PREFIX`, `BIN_DIR` (same as install.sh).

## Daemon

After `index build` / `index update` or search, sift may spawn `sift-daemon` to refresh the index when files change. Disable for automation:

```bash
export SIFT_NO_DAEMON=1
```

## Limitations

- Requires `sift index build` for indexed speedup.
- No PCRE2 (`-P` not supported).
- Patterns without indexable literals may full-scan at roughly ripgrep speed.
- Not a replacement for `git log` / history search.

## Differences from ripgrep

| Topic | ripgrep | sift |
|-------|---------|------|
| Index | None | `.sift` via `index build` |
| `-h` | No filename | Help |
| Tool upgrade | Package manager | `sift update` |

Full matrix: [docs/rg-compat-matrix.md](../../docs/rg-compat-matrix.md).

## Install from source

```bash
cargo build --release -p sift-grep
```

Binary name: `sift` (package `sift-grep` on crates.io).
