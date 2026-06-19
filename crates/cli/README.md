# sift-grep

Grep-like CLI for indexed codebase search. Thin wrapper over `sift-core`: parses flags with clap, maps them to `SearchOptions`, and prints matches.

## Usage

```bash
# Create an index (blocking)
sift --sift-dir .sift index build /path/to/corpus

# Create an index asynchronously (watch daemon required)
sift --sift-dir .sift index build --lazy /path/to/corpus

# Refresh an existing index (async by default)
sift --sift-dir .sift index update .

# Refresh synchronously
sift --sift-dir .sift index update --wait .

# Upgrade the binary
sift update

# Search
sift --sift-dir .sift "pattern" [PATH...]

# Common flags
sift -i "pattern"          # case-insensitive
sift -F "literal.string"   # fixed string (no regex)
sift -w "word"             # whole-word match
sift -c "pattern"          # count matches per file
sift -l "pattern"          # list matching files only
sift --json "pattern"      # JSON output
```

## Structure

| File | Description |
|------|-------------|
| [`src/main.rs`](src/main.rs) | thin binary entrypoint |
| [`src/lib.rs`](src/lib.rs) | `main_entry`, `update`, `index` subcommands, search dispatch |
| [`src/cli.rs`](src/cli.rs) | `Cli` (clap Parser), `Commands` / `IndexCommands` |
| [`tests/`](tests/) | Domain-focused integration tests spawning the real `sift` binary |

## Integration Tests

| Test file | Coverage |
|-----------|----------|
| `integration_search.rs` | Core search correctness |
| `integration_patterns.rs` | Pattern syntax, `-e`, `-f`, `-F` |
| `integration_output.rs` | Output formatting, `--heading`, `--column`, `--vimgrep` |
| `integration_paths.rs` | Path scoping and resolution |
| `integration_context.rs` | `-A`/`-B`/`-C` context lines |
| `integration_glob.rs` | `-g` glob filtering |
| `integration_ignore.rs` | `.gitignore`, `--no-ignore`, hidden files |
| `integration_json.rs` | `--json` output format |
| `integration_stats.rs` | `--stats` flag |
| `integration_modes.rs` | `-c`, `-l`, `-L`, `-o`, `--count-matches` |
| `integration_null_color.rs` | `-0`/`--null`, `--color` |

## Build & Test

```bash
cargo build --release -p sift-grep
cargo test -p sift-grep
```

Release binary name: `sift` (see `Cargo.toml` `[[bin]]`).
