#!/usr/bin/env bash
# Run sift Criterion benchmarks.
# Usage: ./scripts/bench.sh [core|query|index|grep|cli]
#   core  — all sift-core benchmarks (default)
#   query — sift-core query benchmark
#   index — sift-core index benchmark
#   grep  — sift-core grep benchmark
#   cli   — sift-grep CLI benchmark
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
  core)  exec cargo bench -p sift-core "$@" ;;
  query) exec cargo bench -p sift-core --bench query "$@" ;;
  index) exec cargo bench -p sift-core --bench index "$@" ;;
  grep)  exec cargo bench -p sift-core --bench grep "$@" ;;
  cli)   exec cargo bench -p sift-grep --bench cli "$@" ;;
  *)     echo "Usage: $0 [core|query|index|grep|cli]" >&2; exit 1 ;;
esac
