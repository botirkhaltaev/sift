# Benchsuite

Comparative benchmarks: `sift` vs `ripgrep` on real-world code-search workloads. Adapted from the ripgrep benchsuite.

## Prerequisites

- `rg` (ripgrep) in `PATH`
- `sift` binary at `../target/release/sift` (or `--sift-binary`)
- `git`, `curl`, `gunzip` for corpus downloads
- `matplotlib` (optional, for chart generation)

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

## Generating Charts

After running benchmarks with `--raw`, generate comparison charts:

```bash
pip install matplotlib
python3 benchsuite/generate_charts.py /tmp/results.csv --outdir docs/benchmarks/
```

This produces:
- `bench_times.png` -- grouped bar chart of absolute times per benchmark
- `bench_speedup.png` -- horizontal bar chart of sift speedup over ripgrep

## Indexing

`sift` requires a per-corpus index. The benchsuite builds each index once on first use (`sift index build --wait`) and caches it for subsequent runs.

## Custom Binary

```bash
python3 benchsuite/benchsuite --sift-binary /path/to/sift --dir /tmp/benchsuite
```
