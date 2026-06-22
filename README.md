# Sift

Indexed grep for codebases. Build an index once, then search with a grep-like CLI or the `sift-core` library -- up to 60x faster than ripgrep on indexed queries.

## Why Sift

Tools like `grep` and `ripgrep` scan every file on every query. For large codebases this means most search time is spent reading files that cannot possibly match. Sift takes a different approach: build on-disk indexes ahead of time, then use them to skip irrelevant files entirely.

Today, Sift ships a **trigram index** -- an inverted index of overlapping 3-byte sequences that filters candidate files before the regex engine runs. This alone eliminates the majority of file reads for literal and near-literal patterns.

But trigram indexing is just the first index type. The core architecture is designed around **composable on-disk indexes for codebases** -- the same idea that makes databases fast for diverse query workloads.

## Quick Start

### Install (GitHub Release)

```bash
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
```

Installs `sift` and `sift-daemon` to `$HOME/.local/bin`. Override with `PREFIX=/usr/local`.

### Updating

```bash
sift update
```

Or re-run the install script (same as a fresh install over the existing binaries).

### From Source

```bash
cargo build --release -p sift-grep
# produces target/release/sift and target/release/sift-daemon
./target/release/sift --sift-dir .sift index build /path/to/corpus
./target/release/sift --sift-dir .sift "pattern"
```

Patterns use Rust `regex` syntax by default. Use `-F` for fixed strings, `--` to disambiguate from subcommands (e.g. `sift -- index build`).

## How It Works

### Today: Trigram Index

Sift uses on-disk indexes to skip files that cannot match your query:

1. **Build** -- walk the corpus respecting `.gitignore` rules, extract overlapping 3-byte sequences (trigrams) from every file, and persist them as memory-mapped tables.
2. **Plan** -- extract required literals from the regex pattern, decompose them into trigram terms, and intersect posting lists to produce a narrow candidate set.
3. **Search** -- scan only candidate files with the full regex engine, optionally parallelized via Rayon when the candidate count justifies it.

Queries with index hits skip most of the corpus entirely. Full-scan fallback (e.g. `\p{Greek}`) still matches ripgrep performance.

### Architecture: Composable Indexes

Under the hood, Sift is not built around trigrams specifically. The core abstractions -- `IndexKind`, `Index`, and the `Indexes` registry -- are designed so that multiple index types can coexist and cooperate:

```
                        +-----------+
          pattern  ---> |  Planner  | ---> candidate set
                        +-----------+
                              |
              +---------------+---------------+
              |               |               |
        [Trigram Index] [Index Kind B]  [Index Kind C]
              |               |               |
              v               v               v
         candidates      candidates      candidates
              \               |               /
               \              |              /
                +---  intersect / union  ---+
                              |
                        final candidates ---> regex scan
```

The `Indexes` registry opens all available index kinds under a `.sift` directory and intersects their candidate sets at query time. Multiple indexes together produce tighter narrowing than any single index alone. Each index kind decides independently how to filter candidates for a given query; the registry combines their results.

Today, `IndexKind` has one variant: `Trigram`. Adding a new index kind means adding a variant to the `IndexKind` and `Index` enums and implementing the build/open/update lifecycle. The query planner, search engine, snapshot store, and CLI work unchanged.

## Vision

The long-term goal is to treat code search the way databases treat query execution:

- **Multiple index types** -- trigram indexes for literal search, AST indexes for symbols and language-aware queries, dependency/reference graph indexes, vector indexes for semantic search, and other specialized indexes depending on workload.
- **Query planning** -- given a search pattern, determine which indexes can contribute and how to combine their candidate sets most efficiently.
- **Index composition** -- intersect or union candidate sets from different index types to achieve narrowing that no single index could provide alone.

The architectural scaffolding for this already exists: pluggable `IndexKind` dispatch, the `Indexes` registry with multi-index candidate intersection, snapshot-based atomic persistence, and an index-agnostic query planner. What remains is building the additional index types and evolving the planner to leverage them.

**None of this changes what Sift is today.** If you want a faster grep, `sift index build` and `sift "pattern"` work now. The composable index architecture is the foundation that future index types will build on.

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

## Project Layout

```
sift/
├── crates/
│   ├── core/           # sift-core: index registry, query planner, search engine
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
| [`crates/core`](crates/core/) | `sift-core` | Index registry, query planner, candidate narrowing, and parallel search engine |
| [`crates/cli`](crates/cli/) | `sift-cli` | `sift` binary with ripgrep-compatible flags |
| [`fuzz/`](fuzz/) | n/a | LibFuzzer targets for `sift-core` (excluded from workspace) |

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

## Current Scope

This release is a stable baseline for indexed search, not a full ripgrep drop-in.

**Shipped**

- **Trigram index** -- candidate narrowing with full-scan fallback when the planner cannot extract literals.
- **Index lifecycle** -- `sift index build`, `sift index build --lazy`, async `sift index update`, and the watch daemon for background reconciliation.
- **Composable index architecture** -- `IndexKind` dispatch, `Indexes` registry with multi-index intersection, snapshot-based atomic persistence. Ready for additional index types.
- **Documented rg flags** -- behavior tracked in [`docs/rg-compat-matrix.md`](docs/rg-compat-matrix.md) with golden CLI tests for implemented rows.

**Not yet shipped**

- Additional index types (AST, dependency graph, vector, etc.).
- Cross-index query planning beyond candidate intersection.
- Full ripgrep parity (ignore overrides, multiline/encoding, `--vimgrep`, `--debug`, and other matrix rows marked Missing or Partial).
- PCRE2 / `-P` and other engine-specific ripgrep features.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE-2.0), at your option.
