#!/usr/bin/env bash
# Profile sift Criterion workloads with system profilers.
#
# Discovers and runs every available profiler that works in this environment:
#   hyperfine, heaptrack, perf (task-clock / cpu-clock), samply, flamegraph,
#   valgrind callgrind, valgrind massif, valgrind cachegrind.
#
# Usage:
#   ./scripts/profile.sh grep case_insensitive_alternation
#   ./scripts/profile.sh index alternation --out /tmp/sift-profile
#   ./scripts/profile.sh --suite
#   ./scripts/profile.sh --ab master HEAD index case_insensitive_alternation
#   ./scripts/profile.sh analyze /tmp/sift-profile/<run>/heaptrack.gz
#   ./scripts/profile.sh doctor
#
# Targets: query | index | grep | cli | candidates  (same as bench.sh packages)
#
# Outputs under --out (default .sift-profile/<timestamp>-<target>-<filter>/):
#   summary.md              — tool status + condensed hotspots
#   hyperfine.{txt,md}
#   heaptrack.gz + heaptrack-hotspots.txt
#   perf.data + perf-report.txt + perf-script.txt
#   samply-profile.json.gz
#   flamegraph.svg
#   callgrind.out + callgrind-annotate.txt
#   massif.out + massif-peak.txt
#   cachegrind.out + cachegrind-annotate.txt
#
# Environment:
#   CRITERION_ARGS   Criterion flags (default: short profile run)
#   PROFILE_TOOLS    comma list subset of tools (default: all)
#   PERF             override path to perf binary
#   PERF_EVENT       override event (default: auto task-clock|cpu-clock)
#
# Examples:
#   PROFILE_TOOLS=heaptrack,perf ./scripts/profile.sh --suite
#   ./scripts/profile.sh doctor
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

CRITERION_ARGS="${CRITERION_ARGS:---warm-up-time 2 --measurement-time 5 --sample-size 20 --noplot}"
PROFILE_TOOLS="${PROFILE_TOOLS:-heaptrack,hyperfine,perf,samply,flamegraph,callgrind,massif,cachegrind}"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/profile.sh <target> <bench-filter> [--out DIR] [--skip-build]
  ./scripts/profile.sh --suite [--out DIR] [--skip-build]
  ./scripts/profile.sh --ab <base-ref> <head-ref> <target> <filter> [--out DIR]
  ./scripts/profile.sh analyze <heaptrack.gz>
  ./scripts/profile.sh doctor

  target        query | index | grep | cli | candidates
  bench-filter  Criterion filter substring / full name
  --suite       default hot workloads (see below)
  --ab          paired heaptrack+hyperfine+perf A/B between two git refs
  analyze       sift-focused heaptrack hotspot extract
  doctor        print tool availability and probe perf sampling

Default SUITE:
  grep   case_insensitive_alternation
  index  case_insensitive_alternation
  grep   literal
  index  alternation
  index  required_literal
  candidates all_indexed_complete
  grep   full_scan_fallback

PROFILE_TOOLS (comma-separated):
  heaptrack,hyperfine,perf,samply,flamegraph,callgrind,massif,cachegrind
EOF
}

have_tool() {
  command -v "$1" >/dev/null 2>&1
}

tool_enabled() {
  [[ ",$PROFILE_TOOLS," == *",$1,"* ]]
}

timestamp() {
  date -u +"%Y%m%dT%H%M%SZ"
}

sanitize_filter() {
  echo "$1" | tr '/:' '__'
}

# Prefer the newest /usr/lib/linux-tools/*/perf over the /usr/bin stub that
# often fails on cloud kernels whose version packages are unavailable.
resolve_perf() {
  if [[ -n "${PERF:-}" && -x "$PERF" ]]; then
    echo "$PERF"
    return 0
  fi
  local candidate
  candidate=$(find /usr/lib/linux-tools -type f -name perf 2>/dev/null | sort -V | tail -1 || true)
  if [[ -n "${candidate:-}" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  if have_tool perf; then
    command -v perf
    return 0
  fi
  return 1
}

PERF_BIN=""
PERF_EVENT="${PERF_EVENT:-}"
PERF_MODE="" # period | freq | none

probe_perf() {
  PERF_BIN=$(resolve_perf) || {
    PERF_MODE=none
    return 1
  }
  if ! "$PERF_BIN" --version >/dev/null 2>&1; then
    PERF_MODE=none
    return 1
  fi

  # Prefer period sampling on task-clock — works in VMs without HW PMU counters.
  local tmp
  tmp=$(mktemp -d)
  if [[ -n "$PERF_EVENT" ]]; then
    if "$PERF_BIN" record -e "$PERF_EVENT" -c 100000 -o "$tmp/p.data" -- true >/dev/null 2>&1; then
      PERF_MODE=period
      rm -rf "$tmp"
      return 0
    fi
  fi
  if "$PERF_BIN" record -e task-clock -c 100000 -o "$tmp/p.data" -- \
    bash -c 'i=0; while ((i<50000)); do ((i++)); done' >/dev/null 2>&1 \
    && [[ -s "$tmp/p.data" ]] \
    && "$PERF_BIN" report -i "$tmp/p.data" --stdio >/dev/null 2>&1; then
    PERF_EVENT=task-clock
    PERF_MODE=period
    rm -rf "$tmp"
    return 0
  fi
  if "$PERF_BIN" record -e cpu-clock -F 99 -o "$tmp/p.data" -- \
    bash -c 'i=0; while ((i<50000)); do ((i++)); done' >/dev/null 2>&1 \
    && [[ -s "$tmp/p.data" ]] \
    && "$PERF_BIN" report -i "$tmp/p.data" --stdio >/dev/null 2>&1; then
    PERF_EVENT=cpu-clock
    PERF_MODE=freq
    rm -rf "$tmp"
    return 0
  fi
  PERF_MODE=none
  rm -rf "$tmp"
  return 1
}

perf_usable() {
  [[ "$PERF_MODE" != "none" && -n "$PERF_BIN" ]]
}

doctor() {
  echo "# sift profile doctor"
  echo
  echo "## environment"
  echo "- host: $(uname -a)"
  echo "- kernel: $(uname -r)"
  echo "- perf_event_paranoid: $(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo n/a)"
  echo "- commit: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo
  echo "## tools"
  for t in heaptrack hyperfine samply flamegraph valgrind bpftrace pahole uftrace; do
    if have_tool "$t"; then
      local ver
      ver=$("$t" --version 2>&1 | head -1 || true)
      echo "- $t: $(command -v "$t") (${ver:-ok})"
    else
      echo "- $t: MISSING"
    fi
  done
  if have_tool cargo-flamegraph; then
    echo "- cargo-flamegraph: $(command -v cargo-flamegraph)"
  else
    echo "- cargo-flamegraph: MISSING"
  fi
  for t in callgrind_annotate ms_print cg_annotate heaptrack_print; do
    if have_tool "$t"; then
      echo "- $t: $(command -v "$t")"
    fi
  done
  echo
  echo "## apt packages useful on Ubuntu/Debian"
  echo "- heaptrack heaptrack-gui valgrind kcachegrind dwarves bpftrace uftrace sysstat"
  echo "- linux-tools-generic (provides /usr/lib/linux-tools/*/perf)"
  echo "- cargo install: hyperfine samply flamegraph"
  echo
  echo "## perf probe"
  if probe_perf; then
    echo "- binary: $PERF_BIN ($("$PERF_BIN" --version 2>&1))"
    echo "- mode: $PERF_MODE"
    echo "- event: $PERF_EVENT"
  else
    echo "- usable: no (install matching linux-tools; software events may still work)"
    resolve_perf 2>/dev/null && echo "- found binary but sampling failed: $(resolve_perf)" || echo "- no perf binary"
  fi
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
    query)
      if [[ "$filter" == query_* ]]; then echo "$filter"; else echo "query_compile/$filter"; fi
      ;;
    index)
      if [[ "$filter" == index_* ]]; then echo "$filter"; else echo "index_candidates/$filter"; fi
      ;;
    grep)
      if [[ "$filter" == grep_* ]]; then echo "$filter"; else echo "grep_indexed/$filter"; fi
      ;;
    cli) echo "$filter" ;;
    candidates)
      if [[ "$filter" == candidate_* ]]; then echo "$filter"; else echo "candidate_planner/$filter"; fi
      ;;
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
  # Prefer absolute path so tool cwd changes don't break invoke paths.
  readlink -f "$candidate"
}

write_summary_header() {
  local out_dir="$1"
  local target="$2"
  local filter="$3"
  local full_name="$4"
  local bin="$5"
  {
    echo "# sift profile: $full_name"
    echo ""
    echo "- target: \`$target\`"
    echo "- filter: \`$filter\`"
    echo "- full bench: \`$full_name\`"
    echo "- binary: \`$bin\`"
    echo "- criterion args: \`$CRITERION_ARGS\`"
    echo "- tools: \`$PROFILE_TOOLS\`"
    echo "- commit: \`$(git rev-parse --short HEAD 2>/dev/null || echo unknown)\`"
    echo "- kernel: \`$(uname -r)\`"
    echo ""
    echo "## tool availability"
    echo ""
    for t in heaptrack hyperfine samply flamegraph cargo-flamegraph valgrind; do
      if have_tool "$t" || have_tool "${t/cargo-/}"; then
        echo "- $t: yes"
      else
        echo "- $t: no"
      fi
    done
    if perf_usable; then
      echo "- perf: yes ($PERF_BIN, event=$PERF_EVENT, mode=$PERF_MODE)"
    else
      echo "- perf: no (or sampling unsupported in this VM)"
    fi
    echo ""
    echo "## command"
    echo ""
    echo "\`$bin --bench $full_name $CRITERION_ARGS\`"
  } >"$out_dir/summary.md"
}

run_hyperfine() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  tool_enabled hyperfine || return 0
  have_tool hyperfine || {
    echo "hyperfine: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  }
  echo "Running hyperfine..."
  # shellcheck disable=SC2086
  hyperfine --warmup 2 --min-runs 8 --max-runs 20 \
    --export-markdown "$out_dir/hyperfine.md" \
    --export-json "$out_dir/hyperfine.json" \
    "$bin --bench $full_name $CRITERION_ARGS" \
    2>&1 | tee "$out_dir/hyperfine.txt"
  {
    echo ""
    echo "## hyperfine"
    echo ""
    cat "$out_dir/hyperfine.md" 2>/dev/null || true
  } >>"$out_dir/summary.md"
}

extract_heaptrack_hotspots() {
  local gz="$1"
  local dest="$2"
  # Prefer heaptrack_print: `heaptrack --analyze` may auto-launch heaptrack_gui
  # and hang in headless VMs / cloud agent environments.
  if have_tool heaptrack_print; then
    heaptrack_print -f "$gz" -n 40 -a 1 -p 1 -T 1 -l 0 2>/dev/null \
      | rg -i "MOST CALLS|PEAK MEMORY|MOST TEMPORARY|peak consumption|sift_|crates/core|grep_|index_|candidate|hydrate|intersection|posting|Searcher|Matcher|InputConversion|PathBuf" \
      | head -160 >"$dest" || true
    if [[ ! -s "$dest" ]]; then
      heaptrack_print -f "$gz" -n 25 -a 1 -p 1 -T 1 -l 0 2>/dev/null | head -120 >"$dest" || true
    fi
    return 0
  fi
  # Fallback: force non-GUI analyze if possible.
  DISPLAY= HEAPTRACK_GUI=0 timeout 120 heaptrack --analyze "$gz" 2>/dev/null \
    | rg -i "MOST CALLS|peak consumption|sift_|crates/core|grep_|index_|candidate|hydrate|intersection|posting|Searcher|Matcher" \
    | head -120 >"$dest" || true
  if [[ ! -s "$dest" ]]; then
    DISPLAY= HEAPTRACK_GUI=0 timeout 120 heaptrack --analyze "$gz" 2>/dev/null | head -100 >"$dest" || true
  fi
}

run_heaptrack() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  tool_enabled heaptrack || return 0
  have_tool heaptrack || {
    echo "heaptrack: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  }
  echo "Running heaptrack..."
  # `--record-only` skips auto-launch of heaptrack_gui (hangs headless VMs).
  # shellcheck disable=SC2086
  heaptrack --record-only -o "$out_dir/heaptrack" "$bin" --bench "$full_name" $CRITERION_ARGS \
    2>&1 | tee "$out_dir/heaptrack.log" || true
  if [[ -f "$out_dir/heaptrack.gz" ]]; then
    extract_heaptrack_hotspots "$out_dir/heaptrack.gz" "$out_dir/heaptrack-hotspots.txt"
    {
      echo ""
      echo "## heaptrack"
      echo ""
      rg "allocations:|temporary|leaked|time:" "$out_dir/heaptrack.log" || true
      echo ""
      echo "### sift hotspots"
      echo ""
      echo '```'
      head -100 "$out_dir/heaptrack-hotspots.txt"
      echo '```'
      echo ""
      echo "Full analysis: \`heaptrack_print -f $out_dir/heaptrack.gz\`"
    } >>"$out_dir/summary.md"
  fi
}

run_perf() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  tool_enabled perf || return 0
  if ! perf_usable; then
    {
      echo ""
      echo "## perf"
      echo ""
      echo "skipped: no usable sampling mode (try \`./scripts/profile.sh doctor\`)."
    } >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running perf record ($PERF_EVENT / $PERF_MODE)..."
  local data="$out_dir/perf.data"
  # shellcheck disable=SC2086
  if [[ "$PERF_MODE" == period ]]; then
    "$PERF_BIN" record -e "$PERF_EVENT" -c 10000 -g --call-graph dwarf \
      -o "$data" -- "$bin" --bench "$full_name" $CRITERION_ARGS \
      2>&1 | tee "$out_dir/perf.log" || true
  else
    "$PERF_BIN" record -e "$PERF_EVENT" -F 997 -g --call-graph dwarf \
      -o "$data" -- "$bin" --bench "$full_name" $CRITERION_ARGS \
      2>&1 | tee "$out_dir/perf.log" || true
  fi
  if [[ -f "$data" ]]; then
    "$PERF_BIN" report -i "$data" --stdio --no-children 2>/dev/null \
      | head -200 >"$out_dir/perf-report.txt" || true
    "$PERF_BIN" script -i "$data" 2>/dev/null \
      | head -400 >"$out_dir/perf-script.txt" || true
    {
      echo ""
      echo "## perf report (top)"
      echo ""
      echo "event=\`$PERF_EVENT\` mode=\`$PERF_MODE\`"
      echo ""
      echo '```'
      head -100 "$out_dir/perf-report.txt"
      echo '```'
    } >>"$out_dir/summary.md"
  fi
}

run_samply() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  tool_enabled samply || return 0
  have_tool samply || {
    echo "samply: skipped (not installed)" >>"$out_dir/summary.md"
    return 0
  }
  if ! perf_usable; then
    {
      echo ""
      echo "## samply"
      echo ""
      echo "skipped: requires working perf sampling."
    } >>"$out_dir/summary.md"
    return 0
  fi
  echo "Running samply..."
  (
    cd "$out_dir"
    # shellcheck disable=SC2086
    samply record -s -r 200 --profile-name sift-profile \
      "$bin" --bench "$full_name" $CRITERION_ARGS \
      2>&1 | tee "$out_dir/samply.log"
    # samply writes profile.json.gz in cwd
    if [[ -f profile.json.gz ]]; then
      mv profile.json.gz samply-profile.json.gz
    fi
  ) || true
  {
    echo ""
    echo "## samply"
    echo ""
    if [[ -f "$out_dir/samply-profile.json.gz" ]]; then
      echo "Profile: \`$out_dir/samply-profile.json.gz\`"
      echo "Open: \`samply load $out_dir/samply-profile.json.gz\`"
    else
      echo "samply did not produce profile.json.gz (see samply.log)"
    fi
  } >>"$out_dir/summary.md"
}

run_flamegraph() {
  local out_dir="$1"
  local bin="$2"
  local full_name="$3"
  tool_enabled flamegraph || return 0
  if ! perf_usable; then
    {
      echo ""
      echo "## flamegraph"
      echo ""
      echo "skipped: requires working perf."
    } >>"$out_dir/summary.md"
    return 0
  fi
  local svg="$out_dir/flamegraph.svg"
  echo "Running flamegraph via perf script | flamegraph..."
  if [[ -f "$out_dir/perf.data" ]] && have_tool flamegraph; then
    "$PERF_BIN" script -i "$out_dir/perf.data" 2>/dev/null \
      | flamegraph >"$svg" 2>"$out_dir/flamegraph.log" || true
  elif have_tool cargo-flamegraph; then
    # Fallback: invoke cargo-flamegraph with PERF env and period mode.
    local crate_args
    crate_args=$(bench_crate_args "$(basename "$(dirname "$bin")" 2>/dev/null || echo grep)")
    # Prefer direct binary to avoid re-resolving package name mistakes.
    # shellcheck disable=SC2086
    CARGO_PROFILE_RELEASE_DEBUG=true PERF="$PERF_BIN" \
      cargo flamegraph --output "$svg" -- "$bin" --bench "$full_name" $CRITERION_ARGS \
      2>&1 | tee "$out_dir/flamegraph.log" || true
  fi
  {
    echo ""
    echo "## flamegraph"
    echo ""
    if [[ -f "$svg" ]]; then
      echo "SVG: \`$svg\`"
    else
      echo "not produced (see flamegraph.log)"
    fi
  } >>"$out_dir/summary.md"
}

run_valgrind_tool() {
  local tool="$1"
  local out_dir="$2"
  local bin="$3"
  local full_name="$4"
  tool_enabled "$tool" || return 0
  have_tool valgrind || {
    echo "$tool: skipped (valgrind not installed)" >>"$out_dir/summary.md"
    return 0
  }

  # Valgrind is slow — shrink Criterion budget.
  local vg_args="--warm-up-time 0.2 --measurement-time 1 --sample-size 5 --noplot"
  echo "Running valgrind --tool=$tool (short Criterion budget)..."
  case "$tool" in
    callgrind)
      # shellcheck disable=SC2086
      valgrind --tool=callgrind \
        --callgrind-out-file="$out_dir/callgrind.out" \
        --instr-atstart=yes \
        "$bin" --bench "$full_name" $vg_args \
        2>&1 | tee "$out_dir/callgrind.log" || true
      if have_tool callgrind_annotate && [[ -f "$out_dir/callgrind.out" ]]; then
        callgrind_annotate --auto=yes "$out_dir/callgrind.out" \
          2>/dev/null | head -150 >"$out_dir/callgrind-annotate.txt" || true
      fi
      ;;
    massif)
      # shellcheck disable=SC2086
      valgrind --tool=massif \
        --massif-out-file="$out_dir/massif.out" \
        "$bin" --bench "$full_name" $vg_args \
        2>&1 | tee "$out_dir/massif.log" || true
      if have_tool ms_print && [[ -f "$out_dir/massif.out" ]]; then
        ms_print "$out_dir/massif.out" 2>/dev/null \
          | head -120 >"$out_dir/massif-peak.txt" || true
      fi
      ;;
    cachegrind)
      # shellcheck disable=SC2086
      valgrind --tool=cachegrind \
        --cachegrind-out-file="$out_dir/cachegrind.out" \
        "$bin" --bench "$full_name" $vg_args \
        2>&1 | tee "$out_dir/cachegrind.log" || true
      if have_tool cg_annotate && [[ -f "$out_dir/cachegrind.out" ]]; then
        cg_annotate "$out_dir/cachegrind.out" 2>/dev/null \
          | head -120 >"$out_dir/cachegrind-annotate.txt" || true
      fi
      ;;
  esac
  {
    echo ""
    echo "## $tool"
    echo ""
    case "$tool" in
      callgrind)
        if [[ -f "$out_dir/callgrind-annotate.txt" ]]; then
          echo '```'
          head -60 "$out_dir/callgrind-annotate.txt"
          echo '```'
        else
          echo "see callgrind.log"
        fi
        ;;
      massif)
        if [[ -f "$out_dir/massif-peak.txt" ]]; then
          echo '```'
          head -40 "$out_dir/massif-peak.txt"
          echo '```'
        else
          echo "see massif.log"
        fi
        ;;
      cachegrind)
        if [[ -f "$out_dir/cachegrind-annotate.txt" ]]; then
          echo '```'
          head -40 "$out_dir/cachegrind-annotate.txt"
          echo '```'
        else
          echo "see cachegrind.log"
        fi
        ;;
    esac
  } >>"$out_dir/summary.md"
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

  probe_perf || true

  if [[ "$skip_build" != 1 ]]; then
    build_bench "$target"
  fi
  local bin
  bin=$(find_bench_binary "$target")

  write_summary_header "$out_dir" "$target" "$filter" "$full_name" "$bin"

  run_hyperfine "$out_dir" "$bin" "$full_name"
  run_heaptrack "$out_dir" "$bin" "$full_name"
  run_perf "$out_dir" "$bin" "$full_name"
  run_samply "$out_dir" "$bin" "$full_name"
  run_flamegraph "$out_dir" "$bin" "$full_name"
  run_valgrind_tool callgrind "$out_dir" "$bin" "$full_name"
  run_valgrind_tool massif "$out_dir" "$bin" "$full_name"
  run_valgrind_tool cachegrind "$out_dir" "$bin" "$full_name"

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
  profile_one index required_literal "$out_base" "$skip_build"
  profile_one candidates all_indexed_complete "$out_base" "$skip_build"
  profile_one grep full_scan_fallback "$out_base" "$skip_build"
}

run_ab() {
  local base_ref="$1"
  local head_ref="$2"
  local target="$3"
  local filter="$4"
  local out_base="${5:-.sift-profile/ab}"

  mkdir -p "$out_base"
  local report="$out_base/ab-report.md"
  {
    echo "# A/B profile: $target / $filter"
    echo ""
    echo "- base: \`$base_ref\` (\`$(git rev-parse --short "$base_ref")\`)"
    echo "- head: \`$head_ref\` (\`$(git rev-parse --short "$head_ref")\`)"
    echo ""
  } >"$report"

  local tools_saved="$PROFILE_TOOLS"
  # A/B focuses on signal: heaptrack + hyperfine + perf
  PROFILE_TOOLS=heaptrack,hyperfine,perf

  echo "Profiling base $base_ref..."
  git checkout -q "$base_ref"
  profile_one "$target" "$filter" "$out_base/base" 0
  local base_dir
  base_dir=$(find "$out_base/base" -mindepth 1 -maxdepth 1 -type d | sort | tail -1)

  echo "Profiling head $head_ref..."
  git checkout -q "$head_ref"
  profile_one "$target" "$filter" "$out_base/head" 0
  local head_dir
  head_dir=$(find "$out_base/head" -mindepth 1 -maxdepth 1 -type d | sort | tail -1)

  PROFILE_TOOLS="$tools_saved"

  {
    echo "## base ($base_ref)"
    echo ""
    rg "allocations:|temporary|Mean|time:" "$base_dir/heaptrack.log" "$base_dir/hyperfine.txt" 2>/dev/null || true
    echo ""
    echo "## head ($head_ref)"
    echo ""
    rg "allocations:|temporary|Mean|time:" "$head_dir/heaptrack.log" "$head_dir/hyperfine.txt" 2>/dev/null || true
    echo ""
    echo "## directories"
    echo "- base dir: \`$base_dir\`"
    echo "- head dir: \`$head_dir\`"
  } >>"$report"

  echo "A/B complete: $report"
}

cmd_analyze() {
  local gz="$1"
  if [[ ! -f "$gz" ]]; then
    echo "file not found: $gz" >&2
    exit 1
  fi
  local tmp
  tmp=$(mktemp)
  extract_heaptrack_hotspots "$gz" "$tmp"
  cat "$tmp"
  rm -f "$tmp"
}

main() {
  local out_base=".sift-profile"
  local skip_build=0

  if [[ $# -eq 0 ]]; then
    usage
    exit 1
  fi

  case "${1:-}" in
    doctor)
      doctor
      exit 0
      ;;
    analyze)
      shift
      cmd_analyze "${1:?heaptrack.gz path required}"
      exit 0
      ;;
    --suite)
      shift
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --out) out_base="$2"; shift 2 ;;
          --skip-build) skip_build=1; shift ;;
          -h | --help) usage; exit 0 ;;
          *) echo "unknown option: $1" >&2; usage; exit 1 ;;
        esac
      done
      run_suite "$out_base" "$skip_build"
      exit 0
      ;;
    --ab)
      shift
      local base_ref="${1:?base-ref}"
      local head_ref="${2:?head-ref}"
      local target="${3:?target}"
      local filter="${4:?filter}"
      shift 4
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --out) out_base="$2"; shift 2 ;;
          *) echo "unknown option: $1" >&2; usage; exit 1 ;;
        esac
      done
      run_ab "$base_ref" "$head_ref" "$target" "$filter" "$out_base"
      exit 0
      ;;
    -h | --help)
      usage
      exit 0
      ;;
  esac

  local target="${1:?target required}"
  local filter="${2:?bench filter required}"
  shift 2

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --out) out_base="$2"; shift 2 ;;
      --skip-build) skip_build=1; shift ;;
      -h | --help) usage; exit 0 ;;
      *) echo "unknown option: $1" >&2; usage; exit 1 ;;
    esac
  done

  profile_one "$target" "$filter" "$out_base" "$skip_build"
}

main "$@"
