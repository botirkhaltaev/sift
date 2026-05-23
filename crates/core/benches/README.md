# Benchmarks

Criterion benchmark suite for `sift-core` and `sift-cli`.

## Running

```bash
# Criterion (statistical) — core
cargo bench -p sift-core --bench search
./scripts/bench.sh

# Criterion (statistical) — cli
cargo bench -p sift-cli --bench cli
./scripts/bench.sh cli

# Save / compare baselines
./scripts/bench.sh -- --save-baseline main
./scripts/bench.sh -- --baseline main

