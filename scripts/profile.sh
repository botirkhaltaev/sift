#!/usr/bin/env bash
# Profile sift-core via the `sift-profile` binary (workspace `profiling` profile: optimized + line tables).
#
# Subcommands (passed through to `sift-profile`):
#   ./scripts/profile.sh list
#   ./scripts/profile.sh hints
#   ./scripts/profile.sh build
#   ./scripts/profile.sh run <scenario>
#   ./scripts/profile.sh search-only <scenario>
#
# Query-planning scenarios (use: ./scripts/profile.sh run <name>):
#   literal_narrow word_literal line_literal fixed_string casei_literal smart_case_lower
#   smart_case_upper required_literal no_literal alternation alternation_casei unicode_class
#
# Filter + query: glob_include glob_exclude glob_casei hidden_default hidden_include
#   ignore_default ignore_custom scoped_search
#
# Output-mode: only_matching count count_matches files_with_matches files_without_match max_count_1
#
# Shorthand: `./scripts/profile.sh literal_narrow` runs `sift-profile run literal_narrow`.
#
# Large corpus:
#   SIFT_PROFILE_LARGE=1 ./scripts/profile.sh run literal_narrow
#   SIFT_PROFILE_CORPUS_FILES=20000 ./scripts/profile.sh run no_literal
#
# Timing:
#   SIFT_PROFILE_LOOP_SECS=30 ./scripts/profile.sh run required_literal
#
# Flamegraph (wrapper around cargo flamegraph):
#   ./scripts/profile.sh flamegraph literal_narrow
#   ./scripts/profile.sh flamegraph build
#
# System profiling (CPU call stacks — prefer this to find hot functions):
#   ./scripts/system-profile.sh literal_narrow
#   ./scripts/system-profile.sh --perf no_literal   # Linux: perf report
#
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

cargo_prof=(--profile profiling -p sift-core --features profile --bin sift-profile)

first="${1:-list}"
second="${2:-}"

case "$first" in
list|hints|build)
    exec cargo run "${cargo_prof[@]}" -- "$first"
    ;;
flamegraph)
    scenario="${second:-literal_narrow}"
    if [[ "$scenario" == build ]]; then
        unset SIFT_PROFILE_LOOP_SECS || true
        exec cargo flamegraph "${cargo_prof[@]}" -- build
    else
        export SIFT_PROFILE_LOOP_SECS="${SIFT_PROFILE_LOOP_SECS:-30}"
        exec cargo flamegraph "${cargo_prof[@]}" -- run "$scenario"
    fi
    ;;
run|search-only)
    exec cargo run "${cargo_prof[@]}" -- "$first" "$second"
    ;;
*)
    exec cargo run "${cargo_prof[@]}" -- run "$first"
    ;;
esac
