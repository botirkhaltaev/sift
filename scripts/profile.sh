#!/usr/bin/env bash
# Profile sift Criterion workloads with system profilers (heaptrack, perf, samply, flamegraph).
#
# Usage:
#   ./scripts/profile.sh grep case_insensitive_alternation
#   ./scripts/profile.sh index alternation --out /tmp/sift-profile
#   ./scripts/profile.sh --suite
#   ./scripts/profile.sh analyze /tmp/sift-profile/<run>/heaptrack.gz
#
# Targets match ./scripts/bench.sh: core|query|index|grep|cli (core is not profiled directly).
#
# Outputs per run (under --out, default .sift-profile/<timestamp>-<target>-<filter>/):
#   summary.md          — tool status, hyperfine timing, heaptrack hotspot extract
#   hyperfine.txt       — wall-clock baseline
#   heaptrack.gz        — raw heaptrack capture (if heaptrack installed)
#   heaptrack-hotspots.txt
#   perf.data           — perf record (if perf works on this kernel)
#   perf-report.txt
#   samply/             — samply profile dir (if samply + perf work)
#   flamegraph.svg      — if cargo-flamegraph + perf work
#
# Prerequisites (Linux):
#   heaptrack, hyperfine, perf, samply, cargo-flamegraph
#   perf/samply/flamegraph need kernel-matched linux-tools (see summary when missing).
#
# Examples:
#   ./scripts/profile.sh grep case_insensitive_alternation
#   ./scripts/profile.sh candidates all_indexed_complete
#   ./scripts/profile.sh --suite --out /tmp/sift-profile
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

CRITERION_ARGS="${CRITERION_ARGS:---warm-up-time 2 --measurement-time 5 --sample-size 20 --noplot}"
PROFILE_TOOLS="${PROFILE_TOOLS:-heaptrack,hyperfine,perf,samply,flamegraph}"

usage() {
  cat <<'EOF'
Usage: ./scripts/profile.sh <target> <bench-filter> [--out DIR] [--skip-build]
       ./scripts/profile.sh --suite [--out DIR] [--skip-build]
       ./scripts/profile.sh analyze <heaptrack.gz>

  target        query | index | grep | cli  (same packages as bench.sh)
  bench-filter  Criterion filter, e.g. case_insensitive_alternation or grep_indexed/literal
  --suite       profile default hot workloads (see SUITE below)
  analyze       print sift-specific heaptrack hotspots from an existing .gz

Suite workloads (SUITE):
  grep   grep_indexed/case_insensitive_alternation
  index  index_candidates/case_insensitive_alternation
  grep   grep_indexed/literal
  index  index_candidates/alternation
  candidates candidate_planner/all_indexed_complete

Environment:
  CRITERION_ARGS   extra args passed to the bench binary (default: short warm profile run)
  PROFILE_TOOLS    comma-separated subset: heaptrack,hyperfine,perf,samply,flamegraph
EOF
}

have_tool() {
  command -v "$1" >/dev/null 2>&1
}

tool_enabled() {
  [[ ",$PROFILE_TOOLS," == *",$1,"* ]]
}

perf_usable() {
  if ! have_tool perf; then
    return 1
  fi
  perf --version >/dev/null 2>&1 || return 1
  perf stat -e cycles:u true >/dev/null 2>&1
}

timestamp() {
  date -u +"%Y%m%dT%H%M%SZ"
}

sanitize_filter() {
  echo "$1" | tr '/:' '__'
}

bench_crate_args() {
  local target="$1"
  case "$target" in
    query) echo "-p sift-core --bench query" ;;
    index) echo "-p sift-core --bench index" ;;
    grep) echo "-p sift-core --bench grep" ;;
    cli) echo "-p sift-grep --bench cli" ;;
    candidates) echo "-p sift-core --bench candidates" ;;
    *) echo "unknown target: $target" >&2; return 1 ;;
  esac
}

full_bench_name() {
  local target="$1"
  local filter="$2"
  case "$target" in
    query) echo "query_compile/$filter" ;;
    index) echo "index_candidates/$filter" ;;
    grep)
      if [[ "$filter" == grep_* ]]; then
        echo "$filter"
      else
        echo "grep_indexed/$filter"
      fi
      ;;
    cli) echo "$filter" ;;
    candidates) echo "candidate_planner/$filter" ;;
    *) echo "$filter" ;;
  esac
}

build_bench() {
  local target="$1"
  local crate_args
  crate_args=$(bench_crate_args "$target")
  # shellcheck disable=SC2086
  CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release $crate_args
}

find_bench_binary() {
  local target="$1"
  local bin_name
  case "$target" in
    query) bin_name="query" ;;
    index) bin_name="index" ;;
    grep) bin_name="grep" ;;
    cli) bin_name="cli" ;;
    candidates) bin_name="candidates" ;;
    *) return 1 ;;
  esac
  local candidate
  candidate=$(find target/release/deps -maxdepth 1 -type f -name "${bin_name}-*" ! -name "*.d" -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
  if [[ -z "${candidate:-}" || ! -x "$candidate" ]]; then
    echo "bench binary not found for $target (run build first)" >&2
    return 1
  fi
  echo "$candidate"
}

run_hyperfine() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  if ! tool_enabled hyperfine; then
    return 0
  fi
  if ! have_tool hyperfine; then
    echo "hyperfine: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running hyperfine..."
  # shellcheck disable=SC2086
  hyperfine --warmup 3 --min-runs 10 --max-runs 30 \
    --export-markdown "$out_dir/hyperfine.md" \
  "$bin --bench $full_name $CRITERION_ARGS" \
    2>&1 | tee "$out_dir/hyperfine.txt"
  {
    echo ""
    echo "## hyperfine"
    echo ""
    cat "$out_dir/hyperfine.md" 2>/dev/null || cat "$out_dir/hyperfine.txt"
  } >>"$out_dir/summary.md"
}

run_heaptrack() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  if ! tool_enabled heaptrack; then
    return 0
  fi
  if ! have_tool heaptrack; then
    echo "heaptrack: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  fi
  local gz="$out_dir/heaptrack.gz"
  echo "Running heaptrack..."
  # shellcheck disable=SC2086
  heaptrack -o "$out_dir/heaptrack" "$bin" --bench "$full_name" $CRITERION_ARGS \
    2>&1 | tee "$out_dir/heaptrack.log"
  if [[ -f "${gz}" ]]; then
  extract_heaptrack_hotspots "$gz" "$out_dir/heaptrack-hotspots.txt"
  {
    echo ""
    echo "## heaptrack hotspots (sift / core)"
    echo ""
    echo '```'
    head -60 "$out_dir/heaptrack-hotspots.txt"
    echo '```'
    echo ""
    echo "Full analysis: \`heaptrack --analyze $gz\`"
  } >>"$out_dir/summary.md"
  fi
}

extract_heaptrack_hotspots() {
  local gz="$1"
  local dest="$2"
  heaptrack --analyze "$gz" 2>/dev/null \
    | rg -i "sift_|crates/core|grep_|index_|candidate" \
    | head -80 >"$dest" || true
  if [[ ! -s "$dest" ]]; then
    heaptrack --analyze "$gz" 2>/dev/null | head -80 >"$dest" || true
  fi
}

run_perf() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  if ! tool_enabled perf; then
    return 0
  fi
  if ! perf_usable; then
    {
      echo ""
      echo "## perf"
      echo ""
      echo "skipped: \`perf\` not usable on this kernel (install matching linux-tools)."
      perf --version 2>&1 || true
    } >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running perf record..."
  # shellcheck disable=SC2086
  perf record -F 997 -g --call-graph dwarf -o "$out_dir/perf.data" -- \
    "$bin" --bench "$full_name" $CRITERION_ARGS \
    2>&1 | tee "$out_dir/perf.log"
  perf report -i "$out_dir/perf.data" --stdio --no-children \
    2>/dev/null | head -120 >"$out_dir/perf-report.txt" || true
  {
    echo ""
    echo "## perf report (top)"
    echo ""
    echo '```'
    head -80 "$out_dir/perf-report.txt"
    echo '```'
  } >>"$out_dir/summary.md"
}

run_samply() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  if ! tool_enabled samply; then
    return 0
  fi
  if ! have_tool samply; then
    echo "samply: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  fi
  if ! perf_usable; then
    {
      echo ""
      echo "## samply"
      echo ""
      echo "skipped: requires working perf."
    } >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running samply..."
  local profile_dir="$out_dir/samply"
  mkdir -p "$profile_dir"
  (
    cd "$profile_dir"
    # shellcheck disable=SC2086
    samply record -s --profile-name sift-profile \
      "$bin" --bench "$full_name" $CRITERION_ARGS \
      2>&1 | tee "$out_dir/samply.log"
  ) || true
  {
    echo ""
    echo "## samply"
    echo ""
    echo "Profile data: \`$profile_dir\` (open with \`samply load $profile_dir\`)"
  } >>"$out_dir/summary.md"
}

run_flamegraph() {
  local out_dir="$1"
  local target="$2"
  local full_name="$3"
  if ! tool_enabled flamegraph; then
    return 0
  fi
  if ! have_tool cargo-flamegraph; then
    echo "flamegraph: skipped (cargo-flamegraph not installed)" >>"$out_dir/summary.md"
    return 0
  fi
  if ! perf_usable; then
    {
      echo ""
      echo "## flamegraph"
      echo ""
      echo "skipped: requires working perf."
    } >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running cargo flamegraph..."
  local crate_args
  crate_args=$(bench_crate_args "$target")
  local svg="$out_dir/flamegraph.svg"
  # shellcheck disable=SC2086
  CARGO_PROFILE_RELEASE_DEBUG=true \
    cargo flamegraph $crate_args \
    --output "$svg" \
    -- --bench "$full_name" $CRITERION_ARGS \
    2>&1 | tee "$out_dir/flamegraph.log" || true
  if [[ -f "$svg" ]]; then
    {
      echo ""
      echo "## flamegraph"
      echo ""
      echo "SVG: \`$svg\`"
    } >>"$out_dir/summary.md"
  fi
}

profile_one() {
  local target="$1"
  local filter="$2"
  local out_base="${3:-.sift-profile}"
  local skip_build="${4:-0}"

  local full_name
  full_name=$(full_bench_name "$target" "$filter")
  local safe
  safe=$(sanitize_filter "$full_name")
  local out_dir="$out_base/$(timestamp)-${target}-${safe}"
  mkdir -p "$out_dir"

  {
    echo "# sift profile: $full_name"
    echo ""
    echo "- target: \`$target\`"
    echo "- filter: \`$filter\`"
    echo "- full bench: \`$full_name\`"
    echo "- criterion args: \`$CRITERION_ARGS\`"
    echo "- tools: \`$PROFILE_TOOLS\`"
    echo "- commit: \`$(git rev-parse --short HEAD 2>/dev/null || echo unknown)\`"
    echo ""
    echo "## tool availability"
    echo ""
    for t in heaptrack hyperfine perf samply cargo-flamegraph; do
      if have_tool "${t/cargo-/}" || have_tool "$t"; then
        echo "- $t: yes"
      else
        echo "- $t: no"
      fi
    done
    if perf_usable; then
      echo "- perf usable: yes"
    else
      echo "- perf usable: no (install kernel-matched linux-tools)"
    fi
  } >"$out_dir/summary.md"

  if [[ "$skip_build" != 1 ]]; then
    build_bench "$target"
  fi

  local bin
  bin=$(find_bench_binary "$target")

  {
    echo ""
    echo "## command"
    echo ""
    echo "\`$bin --bench $full_name $CRITERION_ARGS\`"
  } >>"$out_dir/summary.md"

  run_hyperfine "$out_dir" "$bin" "$full_name"
  run_heaptrack "$out_dir" "$bin" "$full_name"
  run_perf "$out_dir" "$bin" "$full_name"
  run_samply "$out_dir" "$bin" "$full_name"
  run_flamegraph "$out_dir" "$target" "$full_name"

  echo ""
  echo "Profile complete: $out_dir"
  echo "Read: $out_dir/summary.md"
}

run_suite() {
  local out_base="${1:-.sift-profile}"
  local skip_build="${2:-0}"
  profile_one grep case_insensitive_alternation "$out_base" "$skip_build"
  profile_one index case_insensitive_alternation "$out_base" "$skip_build"
  profile_one grep literal "$out_base" "$skip_build"
  profile_one index alternation "$out_base" "$skip_build"
  profile_one candidates all_indexed_complete "$out_base" "$skip_build"
}

cmd_analyze() {
  local gz="$1"
  if [[ ! -f "$gz" ]]; then
    echo "file not found: $gz" >&2
    exit 1
  fi
  extract_heaptrack_hotspots "$gz" /dev/stdout
}

main() {
  local out_base=".sift-profile"
  local skip_build=0

  if [[ $# -eq 0 ]]; then
    usage
    exit 1
  fi

  if [[ "${1:-}" == analyze ]]; then
    shift
    cmd_analyze "${1:?heaptrack.gz path required}"
    exit 0
  fi

  if [[ "${1:-}" == --suite ]]; then
    shift
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --out)
          out_base="$2"
          shift 2
          ;;
        --skip-build)
          skip_build=1
          shift
          ;;
        -h | --help)
          usage
          exit 0
          ;;
        *)
          echo "unknown option: $1" >&2
          usage
          exit 1
          ;;
      esac
    done
    run_suite "$out_base" "$skip_build"
    exit 0
  fi

  local target="${1:?target required}"
  local filter="${2:?bench filter required}"
  shift 2

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --out)
        out_base="$2"
        shift 2
        ;;
      --skip-build)
        skip_build=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        echo "unknown option: $1" >&2
        usage
        exit 1
        ;;
    esac
  done

  profile_one "$target" "$filter" "$out_base" "$skip_build"
}

main "$@"
