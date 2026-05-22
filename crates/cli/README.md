# sift-cli

Grep-like CLI for indexed codebase search. Thin wrapper over `sift-core` — parses flags with clap, maps them to `SearchOptions`, and prints matches.

## Usage

```bash
# Build an index
sift --sift-dir .sift build /path/to/corpus

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
| [`src/main.rs`](src/main.rs) | `Cli` (clap Parser), `build` subcommand, search mode dispatch |
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
cargo build --release -p sift-cli
cargo test -p sift-cli
```

Release binary name: `sift` (see `Cargo.toml` `[[bin]]`).
