#!/usr/bin/env bash
# System-profile Criterion benches (or a CLI argv).
#
# Primary mode profiles sift-core Criterion binaries via --profile-time so
# stacks reflect one domain operation. Uses [profile.bench] (debug = 1).
#
# Prefers samply; falls back to xctrace Time Profiler on macOS when samply
# cannot attach (common without debugger entitlement).
#
# Usage:
#   ./scripts/profile.sh --bench <name> [--profile-time SECS] [--frame-pointers] [-- <filter>]
#   ./scripts/profile.sh --cli [--frame-pointers] [--no-build] -- <command...>
#
# Examples:
#   ./scripts/profile.sh --bench grep --profile-time 30 -- grep_search/full_scan
#   ./scripts/profile.sh --bench candidates --profile-time 30 -- candidate_planner/use_index_literal
#   ./scripts/profile.sh --cli -- target/release/sift --sift-dir /tmp/x.sift -n beta
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

mode=""
bench_name=""
profile_time=30
frame_pointers=0
no_build=0

usage() {
  sed -n '2,17p' "$0" | sed 's/^# \?//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage 0 ;;
    --bench)
      mode=bench
      bench_name="${2:?--bench requires a name}"
      shift 2
      ;;
    --cli)
      mode=cli
      shift
      ;;
    --profile-time)
      profile_time="${2:?--profile-time requires seconds}"
      shift 2
      ;;
    --frame-pointers) frame_pointers=1; shift ;;
    --no-build) no_build=1; shift ;;
    --) shift; break ;;
    *)
      echo "unknown option: $1" >&2
      usage 1
      ;;
  esac
done

if [[ -z "$mode" ]]; then
  echo "error: pass --bench <name> or --cli" >&2
  usage 1
fi

if [[ "$frame_pointers" -eq 1 ]]; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C force-frame-pointers=yes"
fi

cargo_target_dir() {
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    printf '%s\n' "$CARGO_TARGET_DIR"
    return
  fi
  cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' \
    2>/dev/null \
    || printf '%s\n' "$repo_root/target"
}

resolve_bench_bin() {
  local name="$1"
  local target_dir bin=""
  target_dir="$(cargo_target_dir)"
  while IFS= read -r p; do
    if [[ -x "$p" && "$p" != *.d && "$p" != *.rlib && "$p" != *.o ]]; then
      bin="$p"
      break
    fi
  done < <(ls -t "$target_dir"/release/deps/"${name}"-* 2>/dev/null || true)
  if [[ -z "$bin" ]]; then
    while IFS= read -r p; do
      if [[ -x "$p" && "$p" != *.d && "$p" != *.rlib && "$p" != *.o ]]; then
        bin="$p"
        break
      fi
    done < <(ls -t "$target_dir"/*/deps/"${name}"-* 2>/dev/null || true)
  fi
  if [[ -z "$bin" ]]; then
    echo "error: could not find bench binary for '$name' under $target_dir/*/deps/" >&2
    exit 1
  fi
  printf '%s\n' "$bin"
}

record_argv() {
  local -a argv=("$@")
  if command -v samply >/dev/null 2>&1; then
    if samply record -- "${argv[@]}"; then
      return 0
    fi
    echo "warning: samply failed; trying xctrace Time Profiler" >&2
  fi
  if command -v xctrace >/dev/null 2>&1; then
    local out="${PROFILE_TRACE_OUT:-$repo_root/target/profile-$(date +%Y%m%d-%H%M%S).trace}"
    mkdir -p "$(dirname "$out")"
    rm -rf "$out"
    xctrace record \
      --template "Time Profiler" \
      --output "$out" \
      --time-limit "${profile_time}s" \
      --no-prompt \
      --launch -- "${argv[@]}"
    echo "trace written to $out" >&2
    return 0
  fi
  cat >&2 <<'EOF'
error: no working system profiler.

Install samply (`cargo install samply`, then `samply setup` on macOS), or use
Xcode's xctrace / Instruments Time Profiler on the same argv.
EOF
  exit 1
}

case "$mode" in
  bench)
    case "$bench_name" in
      query|index|grep|candidates) ;;
      *)
        echo "error: unknown bench '$bench_name' (query|index|grep|candidates)" >&2
        exit 1
        ;;
    esac
    if [[ "$no_build" -eq 0 ]]; then
      cargo bench -p sift-core --bench "$bench_name" --no-run
    fi
    bin="$(resolve_bench_bin "$bench_name")"
    filter_args=()
    if [[ $# -gt 0 ]]; then
      filter_args=("$@")
    fi
    record_argv "$bin" --bench --profile-time "$profile_time" "${filter_args[@]}"
    ;;
  cli)
    if [[ $# -eq 0 ]]; then
      echo "error: pass a command after --" >&2
      usage 1
    fi
    if [[ "$no_build" -eq 0 ]]; then
      export CARGO_PROFILE_RELEASE_DEBUG=1
      cargo build --release -p sift-grep
    fi
    record_argv "$@"
    ;;
esac
