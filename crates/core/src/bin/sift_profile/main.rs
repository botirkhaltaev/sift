//! Hot-loop timings (`profile\tkey\tvalue` lines) and `cargo flamegraph` target for sift-core.
//!
//! Built only with `--features profile`. Prefer **`./scripts/profile.sh`** from the repo root.
//!
//! ## Subcommands
//!
//! - **`list`** — scenario names (one per line)
//! - **`run <scenario>`** — full pipeline timings + per-iteration search distribution
//! - **`run <scenario> --search-only`** — search-loop metrics only (cleaner for `perf` attribution)
//! - **`search-only <scenario>`** — alias for `run --search-only`
//! - **`build`** — index build benchmark
//! - **`hints`** — copy-paste `perf` / flamegraph commands
//!
//! ## Environment (`SIFT_PROFILE_*`)
//!
//! | Variable | Effect |
//! |---|---|
//! | `SIFT_PROFILE_LARGE=1` | Use large synthetic corpus instead of parity |
//! | `SIFT_PROFILE_CORPUS_FILES` | File count for large corpus (0 = parity/filter rules) |
//! | `SIFT_PROFILE_CORPUS_LINES` | Lines per file (large corpus) |
//! | `SIFT_PROFILE_CORPUS_DIRS` | Directory fan-out (large corpus) |
//! | `SIFT_PROFILE_FILTER_CORPUS` | Use 12-file filter fixture instead of parity |
//! | `SIFT_PROFILE_ITERS` | Fixed iteration count for `run` / `build` |
//! | `SIFT_PROFILE_LOOP_SECS` | Timed `run` loop (seconds); overrides `SIFT_PROFILE_ITERS` |
//! | `SIFT_PROFILE_WARMUP` | `run_index` iterations before recording per-iter samples |
//! | `SIFT_PROFILE_RSS=1` | Print resident set size before/after (Linux/macOS) |
//! | `SIFT_PROFILE_CORPUS` | External corpus root (skip materialisation) |
//! | `SIFT_PROFILE_INDEX` | External index directory (default: `<corpus>.sift`) |

mod corpus;
mod metrics;
mod run;
mod scenarios;
mod stats;

use clap::{Parser, Subcommand};
use corpus::corpus_kind_from_env;
use run::{build_iters_from_env, loop_config, run_build, run_scenario};
use scenarios::{find_scenario, list_scenario_names, scenario_names_joined};

#[derive(Parser)]
#[command(
    name = "sift-profile",
    version,
    about = "Hot-loop profiling for sift-core"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List scenario names (one per line)
    List,
    /// Benchmark `IndexBuilder::build` in a loop (see `SIFT_PROFILE_ITERS`)
    Build,
    /// Run a named scenario against the corpus index
    Run {
        /// Scenario name (from `list`)
        scenario: String,
        /// Emit only search-loop metrics (omit plan/candidate/matcher phases)
        #[arg(long)]
        search_only: bool,
    },
    /// Alias for `run <scenario> --search-only`
    SearchOnly { scenario: String },
    /// Print example `perf` and `cargo flamegraph` invocations
    Hints,
}

fn print_hints() {
    eprintln!(
        "sift-profile — metrics (TSV `profile` lines). For **CPU / call stacks**, use `./scripts/system-profile.sh`.\n"
    );
    eprintln!(
        "System profiling (flamegraph or Linux perf):\n  ./scripts/system-profile.sh literal_narrow\n  ./scripts/system-profile.sh --perf no_literal   # Linux only\n  SIFT_PROFILE_LOOP_SECS=40 ./scripts/system-profile.sh no_literal\n"
    );
    eprintln!(
        "Debug binary:\n  cargo build --profile profiling -p sift-core --features profile --bin sift-profile\n  export BIN=target/profiling/sift-profile\n"
    );
    eprintln!(
        "Flamegraph (same as script; needs `cargo install flamegraph`):\n  cargo flamegraph --profile profiling -p sift-core --features profile --bin sift-profile -- run literal_narrow\n"
    );
    eprintln!(
        "Linux perf:\n  perf record -g -F 997 -- \"$BIN\" run literal_narrow\n  perf report --stdio --no-children\n"
    );
    eprintln!(
        "Steady-state search (for profiling):\n  SIFT_PROFILE_LOOP_SECS=30 \"$BIN\" run no_literal\n"
    );
    eprintln!(
        "External corpus:\n  SIFT_PROFILE_CORPUS=/path/to/repo SIFT_PROFILE_INDEX=/path/to/index.sift \"$BIN\" run literal_narrow\n"
    );
}

fn main() {
    let cli = Cli::parse();
    let kind = corpus_kind_from_env();

    match cli.command {
        Commands::List => list_scenario_names(),
        Commands::Hints => print_hints(),
        Commands::Build => {
            if std::env::var("SIFT_PROFILE_LOOP_SECS").is_ok() {
                eprintln!(
                    "build: `SIFT_PROFILE_LOOP_SECS` is not supported; use `SIFT_PROFILE_ITERS`"
                );
                std::process::exit(2);
            }
            let iters = build_iters_from_env(&kind);
            run_build(iters, &kind);
        }
        Commands::Run {
            scenario,
            search_only,
        } => {
            let scenario = find_scenario(&scenario).unwrap_or_else(|| {
                eprintln!(
                    "unknown scenario `{scenario}` — run `sift-profile list`.\nExpected one of: {}",
                    scenario_names_joined()
                );
                std::process::exit(2);
            });
            let (_tmp, index) = corpus::open_corpus_index(&kind);
            let loop_cfg = loop_config(&kind);
            run_scenario(&index, &scenario, &loop_cfg, search_only);
        }
        Commands::SearchOnly { scenario } => {
            let scenario = find_scenario(&scenario).unwrap_or_else(|| {
                eprintln!(
                    "unknown scenario `{scenario}` — run `sift-profile list`.\nExpected one of: {}",
                    scenario_names_joined()
                );
                std::process::exit(2);
            });
            let (_tmp, index) = corpus::open_corpus_index(&kind);
            let loop_cfg = loop_config(&kind);
            run_scenario(&index, &scenario, &loop_cfg, true);
        }
    }
}
