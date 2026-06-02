# sift reference

## Common flags

| Flag | Purpose |
|------|---------|
| `-i` | Case-insensitive |
| `-w` | Whole word |
| `-F` | Fixed string (no regex) |
| `-c` | Count matches per file |
| `-l` | List matching files only |
| `-L` / `--follow` | Follow symlinks (build and search) |
| `-g GLOB` | Filter paths by glob |
| `-A` / `-B` / `-C` | Context lines |
| `--json` | JSON Lines output |
| `--stats` | Summary on stderr |
| `-0` / `--null` | NUL-separated paths |
| `--no-filename` | Omit path prefix (not `-h`) |
| `-j N` / `--threads N` | Rayon thread count |

Patterns: positional, or `-e PATTERN`, or `-f FILE`. Multiple patterns are OR’d unless configured otherwise.

## Build

```bash
sift --sift-dir .sift build [PATH]
sift --sift-dir .sift build --indexes trigram .
```

- `PATH` defaults to `.`; can be a single file (indexes parent directory).
- `--indexes` selects index kinds (default: all; shipped: `trigram`).
- Re-running `build` on an existing index runs an incremental update.

## Search paths

With an index, every search path must resolve under the **corpus root** stored in the index metadata. Absolute and relative paths are accepted if they stay under that root.

With no index (walk mode), search uses the **current working directory** as root; path rules differ from indexed search.

## Daemon

After `build` or search, sift may spawn `sift-daemon` to refresh the index when files change. Disable for automation:

```bash
export SIFT_NO_DAEMON=1
```

## Limitations

- Requires a prior `build` for indexed speedup (large wins on literal-heavy queries).
- No PCRE2 (`-P` not supported).
- Patterns without indexable literals may full-scan at roughly ripgrep speed.
- Not a replacement for `git log` / history search.

## Differences from ripgrep

| Topic | ripgrep | sift |
|-------|---------|------|
| Index | None | `.sift` via `build` |
| `-h` | No filename | Help |
| Best performance | Always walk | After `build` |

Full matrix: [docs/rg-compat-matrix.md](../../docs/rg-compat-matrix.md).

## Install alternatives

```bash
# Pin a release tag in the install URL if needed
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh

# From source (only when user requests)
cargo build --release -p sift-grep
```

Binary name: `sift` (package `sift-grep` on crates.io).
