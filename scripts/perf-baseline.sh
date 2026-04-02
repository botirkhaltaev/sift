#!/usr/bin/env bash
# Record a profiling baseline: Linux `perf` when available, otherwise sift-profile metrics.
# Usage: from repo root, `./scripts/perf-baseline.sh`
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

export RUSTFLAGS='-C force-frame-pointers=yes'
cargo build --profile profiling -p sift-core --features profile --bin sift-profile >/dev/null
BIN="${repo_root}/target/profiling/sift-profile"

if ! command -v perf >/dev/null 2>&1; then
	echo "perf(1) not found — printing sift-profile metrics (install linux-tools / perf on Linux for perf.data)."
	echo "=== literal_narrow (parity, SIFT_ITERS=50000) ==="
	SIFT_ITERS=50000 "${BIN}" literal_narrow
	echo "=== no_literal (parity, SIFT_ITERS=50000) ==="
	SIFT_ITERS=50000 "${BIN}" no_literal
	echo "=== build (SIFT_LARGE=1, SIFT_ITERS=1) ==="
	SIFT_LARGE=1 SIFT_ITERS=1 "${BIN}" build
	exit 0
fi

mkdir -p "${repo_root}/target"
run_perf() {
	local name="$1"
	shift
	perf record -g -F 997 --output="${repo_root}/target/perf-${name}.data" -- "$@"
	perf report --stdio --no-children -i "${repo_root}/target/perf-${name}.data" | head -n 40
}

echo "=== perf: literal_narrow (SIFT_ITERS=200000) ==="
SIFT_ITERS=200000 run_perf literal-narrow "${BIN}" literal_narrow

echo "=== perf: no_literal (SIFT_ITERS=100000) ==="
SIFT_ITERS=100000 run_perf no-literal "${BIN}" no_literal

echo "=== perf: build (SIFT_LARGE=1, SIFT_ITERS=1) ==="
SIFT_LARGE=1 SIFT_ITERS=1 run_perf build-large "${BIN}" build

echo "Raw perf.data files under target/perf-*.data — run: perf report -i target/perf-literal-narrow.data"
