# Benchsuite

Comparative benchmarks: `sift` vs `ripgrep` on real-world code-search workloads. Adapted from the ripgrep benchsuite.

## Prerequisites

- `rg` (ripgrep) in `PATH`
- `sift` binary at `../target/release/sift` (or `--sift-binary`)
- `git`, `curl`, `gunzip` for corpus downloads

## Usage

```bash
# Download corpora
python3 benchsuite/benchsuite --download linux        # ~1 GB
python3 benchsuite/benchsuite --download subtitles-en  # ~500 MB
python3 benchsuite/benchsuite --download all

# Run benchmarks
python3 benchsuite/benchsuite --dir /tmp/benchsuite
python3 benchsuite/benchsuite --dir /tmp/benchsuite linux_literal  # specific family

# More iterations
python3 benchsuite/benchsuite --dir /tmp/benchsuite --warmup-iter 3 --bench-iter 5

# Raw CSV output
python3 benchsuite/benchsuite --dir /tmp/benchsuite --raw /tmp/results.csv
```

## Indexing

`sift` requires a per-corpus trigram index. The benchsuite builds each index once on first use and caches it for subsequent runs.

## Custom Binary

```bash
python3 benchsuite/benchsuite --sift-binary /path/to/sift --dir /tmp/benchsuite
```
