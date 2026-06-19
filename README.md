# Sift

Indexed grep for codebases. Build an index once, then search with a grep-like CLI or the `sift-core` library, up to 60x faster than ripgrep on indexed queries.

## Quick Start

### Install (GitHub Release)

```bash
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
```

Installs to `$HOME/.local/bin/sift`. Override with `PREFIX=/usr/local`.

### Updating

```bash
sift update
```

Or re-run the install script (same as a fresh install over the existing binary).

### From Source

```bash
cargo build --release -p sift-grep
./target/release/sift --sift-dir .sift index build /path/to/corpus
./target/release/sift --sift-dir .sift "pattern"
```

Patterns use Rust `regex` syntax by default. Use `-F` for fixed strings, `--` to disambiguate from subcommands (e.g. `sift -- index build`).

## Architecture

```
sift/
├── crates/
│   ├── core/           # sift-core: index-backed query planner and search engine
│   └── cli/            # sift-cli: grep-like CLI over sift-core
├── fuzz/               # cargo-fuzz targets (standalone, nightly)
├── benchsuite/         # rg vs sift comparative benchmarks
├── scripts/            # bench.sh, fuzz.sh, install.sh
├── skills/             # Agent skill for searching with sift (npx skills)
└── docs/               # Performance snapshots and compatibility matrix
```

## Crates

| Crate | Package | Description |
|-------|---------|-------------|
| [`crates/core`](crates/core/) | `sift-core` | Index-backed query planner, candidate narrowing, and parallel search engine |
| [`crates/cli`](crates/cli/) | `sift-cli` | `sift` binary with ripgrep-compatible flags |
| [`fuzz/`](fuzz/) | n/a | LibFuzzer targets for `sift-core` (excluded from workspace) |

## How It Works

Sift uses on-disk indexes to skip files that cannot match your query. The core idea is simple: build indexes based on your workload, then let the planner use them to narrow candidates before running the regex engine.

1. **Build**: walk the corpus respecting `.gitignore` rules, extract indexable features from every file, and persist them as memory-mapped tables. The shipped index type is a trigram index, which records overlapping 3-byte sequences from each file.
2. **Plan**: extract required literals from the regex pattern, decompose them into index-friendly terms, and intersect posting lists to narrow the candidate set.
3. **Search**: scan only candidate files with the full regex engine, optionally parallelized via Rayon when the candidate count justifies it.

Queries with index hits skip most of the corpus entirely. Full-scan fallback (e.g. `\p{Greek}`) still matches ripgrep performance.

The `SearchIndex` trait makes the system pluggable. New index kinds (suffix arrays, symbol tables, etc.) can be added alongside the trigram index, and the `Indexes` registry will intersect their candidate sets for even tighter narrowing.

## Performance

Benchsuite snapshot against the Linux kernel corpus:

| Search Class | Speedup vs `rg` | Mechanism |
|---|---:|---|
| Indexed literals | ~60x | Index narrowing eliminates most files |
| Indexed word matches | ~60x | Whole-word literal shaping stays cheap |
| Indexed alternation | ~31x | Multi-arm candidate narrowing |
| Full-scan Unicode | ~1.0x | Near parity, regex engine scans |
| Full-scan no-literal | ~1.1x | Comparable full-scan performance |

Correctness parity: **11/11** benchmarks. See [`crates/core/benches/README.md`](crates/core/benches/README.md) for the full benchmark and profiling workflow, and [`benchsuite/`](benchsuite/) for the comparative suite.

## Differences from ripgrep

- Requires a **prior index** (`sift index build`, or `sift index build --lazy` with the watch daemon) before searching; refresh with `sift index update` (async by default) or `sift index update --wait`.
- Search automatically queues background indexing for unindexed files touched during a walk (disable the daemon with `SIFT_NO_DAEMON=1` to skip).
- Search paths must sit **under** the indexed corpus root.
- Uses `--no-filename` instead of `-h` (which is help).

See [`docs/rg-compat-matrix.md`](docs/rg-compat-matrix.md) for the full flag compatibility matrix.

## Requirements

| Component | Version |
|-----------|---------|
| Rust | 2024 edition (stable) |
| OS | Linux, macOS, Windows (CI-tested) |

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

CI runs fmt, clippy (`-D warnings`), and tests on Linux, macOS, and Windows. See [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

Contributing and security reporting: [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md).

## What v1.0 guarantees

v1.0 is a stable baseline for indexed search, not a full ripgrep drop-in.

**In scope**

- **Indexed search** — trigram-backed candidate narrowing with full-scan fallback when the planner cannot extract literals.
- **Index lifecycle** — `sift index build`, `sift index build --lazy`, async `sift index update`, and the watch daemon for background reconciliation.
- **Documented rg flags** — behavior tracked in [`docs/rg-compat-matrix.md`](docs/rg-compat-matrix.md) with golden CLI tests for implemented rows.

**Out of scope for v1.0**

- Full ripgrep parity (ignore overrides, multiline/encoding, `--vimgrep`, `--debug`, and other matrix rows marked Missing or Partial).
- PCRE2 / `-P` and other engine-specific ripgrep features.
- A prior index is still required; sift is not a zero-setup replacement for `rg`.

Post-v1.0 work continues toward broader rg compatibility without breaking the v1.0 index format or documented CLI contracts.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE-2.0), at your option.
