# Sift

**Indexed regex search for codebases.** Build a trigram index once, then search it with a grep-like CLI or the `sift-core` library — up to 60× faster than ripgrep on indexed queries.

---

## Quick Start

### Install (GitHub Release)

```bash
curl -fsSL https://raw.githubusercontent.com/botirk38/sift/v0.1.2/scripts/install.sh | sh
```

Installs to `$HOME/.local/bin/sift`. Override with `PREFIX=/usr/local`.

### From Source

```bash
cargo build --release -p sift-cli
./target/release/sift --sift-dir .sift build /path/to/corpus
./target/release/sift --sift-dir .sift "pattern"
```

Patterns use Rust `regex` syntax by default. Use `-F` for fixed strings, `--` to disambiguate from subcommands (e.g. `sift -- build`).

---

## Architecture

```
sift/
├── crates/
│   ├── core/           # sift-core — trigram index, query planner, search engine
│   └── cli/            # sift-cli — grep-like CLI over sift-core
├── fuzz/               # cargo-fuzz targets (standalone, nightly)
├── benchsuite/         # rg vs sift comparative benchmarks
├── scripts/            # bench.sh, profile.sh, fuzz.sh, install.sh
├── skills/             # Agent skills (skills.sh / npx skills)
└── docs/               # Performance snapshots and compatibility matrix
```

## Crates

| Crate | Package | Description |
|-------|---------|-------------|
| [`crates/core`](crates/core/) | `sift-core` | Trigram index builder, query planner, and parallel search engine |
| [`crates/cli`](crates/cli/) | `sift-cli` | `sift` binary with ripgrep-compatible flags |
| [`fuzz/`](fuzz/) | — | LibFuzzer targets for `sift-core` (excluded from workspace) |

---

## How It Works

1. **Build** — walk the corpus respecting `.gitignore` rules, extract overlapping byte trigrams from every file, and persist three memory-mapped tables (`files.bin`, `lexicon.bin`, `postings.bin`).
2. **Plan** — extract required literals from the regex pattern, decompose them into trigram arms, and intersect posting lists to narrow the candidate set.
3. **Search** — scan only candidate files with the full regex engine, optionally parallelized via Rayon when the candidate count justifies it.

Queries that yield trigram hits skip most of the corpus entirely. Full-scan fallback (e.g. `\p{Greek}`) still matches ripgrep performance.

---

## Performance

Benchsuite snapshot against the Linux kernel corpus:

| Search Class | Speedup vs `rg` | Mechanism |
|---|---:|---|
| Indexed literals | ~60× | Trigram narrowing eliminates most files |
| Indexed word matches | ~60× | Whole-word literal shaping stays cheap |
| Indexed alternation | ~31× | Multi-arm candidate narrowing |
| Full-scan Unicode | ~1.0× | Near parity — regex engine scans |
| Full-scan no-literal | ~1.1× | Comparable full-scan performance |

Correctness parity: **11/11** benchmarks. See [`crates/core/benches/README.md`](crates/core/benches/README.md) for the full benchmark and profiling workflow, and [`benchsuite/`](benchsuite/) for the comparative suite.

---

## Differences from ripgrep

- Requires a **prior index** (`sift build`) before searching.
- Search paths must sit **under** the indexed corpus root.
- Uses `--no-filename` instead of `-h` (which is help).

See [`docs/rg-compat-matrix.md`](docs/rg-compat-matrix.md) for the full flag compatibility matrix.

---

## Requirements

| Component | Version |
|-----------|---------|
| Rust | 2024 edition (stable) |
| OS | Linux, macOS, Windows (CI-tested) |

---

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

CI runs fmt, clippy (`-D warnings`), and tests on Linux, macOS, and Windows — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

---

## License

MIT OR Apache-2.0 — see [`Cargo.toml`](Cargo.toml).
