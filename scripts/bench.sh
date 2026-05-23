#!/usr/bin/env bash
# Run sift Criterion benchmarks (statistical: 150 samples, 10s measurement window per case).
# Usage: ./scripts/bench.sh [core|cli]
#   core — sift-core benchmarks (default)
#   cli  — sift-cli benchmarks
# Examples:
#   ./scripts/bench.sh
#   ./scripts/bench.sh cli
#   ./scripts/bench.sh -- --save-baseline main
#   ./scripts/bench.sh cli -- --save-baseline main
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
target="${1:-core}"
shift 2>/dev/null || true
case "$target" in
  core) exec cargo bench -p sift-core --bench search "$@" ;;
  cli)  exec cargo bench -p sift-cli --bench cli "$@" ;;
  *)    echo "Usage: $0 [core|cli]" >&2; exit 1 ;;
esac
