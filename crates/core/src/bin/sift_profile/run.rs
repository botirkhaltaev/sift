//! Execute scenarios and emit `profile\t` metrics.

use std::hint::black_box;
use std::time::{Duration, Instant};

use sift_core::{CompiledSearch, Index, SearchFilter, SearchSeparators, TrigramPlan};

use crate::corpus::CorpusKind;
use crate::metrics::{print_profile, resident_set_bytes, rss_enabled};
use crate::scenarios::Scenario;
use crate::stats::duration_stats;

pub enum Loop {
    Timed(Duration),
    Iters(usize),
}

pub fn ns_per_iter(elapsed: Duration, iters: usize) -> u128 {
    if iters == 0 {
        return 0;
    }
    let iters_u128 = u128::try_from(iters).unwrap_or(u128::MAX);
    elapsed.as_nanos() / iters_u128
}

const fn default_search_iters(kind: &CorpusKind) -> usize {
    match kind {
        CorpusKind::Parity | CorpusKind::Filter => 2_000_000,
        CorpusKind::Large { .. } => 5_000,
    }
}

pub fn loop_config(kind: &CorpusKind) -> Loop {
    if let Ok(s) = std::env::var("SIFT_PROFILE_LOOP_SECS") {
        let secs: u64 = s.parse().unwrap_or(15);
        return Loop::Timed(Duration::from_secs(secs));
    }
    let iters: usize = std::env::var("SIFT_PROFILE_ITERS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or_else(|| default_search_iters(kind));
    Loop::Iters(iters)
}

fn warmup_iters() -> usize {
    std::env::var("SIFT_PROFILE_WARMUP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn run_scenario(index: &Index, scenario: &Scenario, loop_cfg: &Loop, search_only: bool) {
    if rss_enabled()
        && let Some(b) = resident_set_bytes()
    {
        print_profile("rss_bytes_before", &b.to_string());
    }

    let t_plan = Instant::now();
    let query = CompiledSearch::new(&scenario.patterns, scenario.opts.clone()).unwrap();
    let plan_us = t_plan.elapsed().as_micros();

    let plan_kind = match &query.plan {
        TrigramPlan::Narrow { .. } => "narrow",
        TrigramPlan::FullScan => "full_scan",
    };

    let total_files = index.file_count();

    let t_candidates = Instant::now();
    let filter = SearchFilter::new(&scenario.filter_config, &index.root).unwrap();
    let raw_ids = query.candidate_file_ids(index, false);
    let threshold = raw_ids
        .len()
        .min(8 * std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get));
    let candidates = CompiledSearch::prepare_candidates(index, &raw_ids, &filter, threshold);
    let candidates_us = t_candidates.elapsed().as_micros();
    let candidate_count = candidates.len();

    let t_matcher = Instant::now();
    query
        .matcher
        .get_or_try_init(|| query.build_matcher())
        .unwrap();
    let matcher_us = t_matcher.elapsed().as_micros();

    let (samples, search_wall) = run_search_loop(&query, index, &filter, scenario, loop_cfg);

    let measured_count = samples.len();
    let search_us: u128 = samples.iter().map(Duration::as_micros).sum();

    print_profile("scenario", scenario.name);
    print_profile("command", if search_only { "search_only" } else { "run" });
    print_profile("search_only", if search_only { "true" } else { "false" });
    print_profile("warmup_iters", &warmup_iters().to_string());
    print_profile("measured_iters", &measured_count.to_string());
    print_profile("plan_kind", plan_kind);
    print_profile("search_mode", &format!("{:?}", scenario.output.mode));
    print_profile("total_files", &total_files.to_string());
    print_profile("candidate_files", &candidate_count.to_string());

    if !search_only {
        print_profile("phase_plan_us", &plan_us.to_string());
        print_profile("phase_candidate_us", &candidates_us.to_string());
        print_profile("phase_matcher_us", &matcher_us.to_string());
    }

    print_profile("phase_search_wall_us", &search_wall.as_micros().to_string());
    print_profile("phase_search_sum_us", &search_us.to_string());

    if measured_count == 0 {
        print_profile("search_ns_per_iter", "0");
        print_profile("error", "no_measured_iterations");
    } else {
        let mut samples = samples;
        let st = duration_stats(&mut samples);
        print_profile("search_ns_mean", &st.mean_ns.to_string());
        print_profile("search_ns_p50", &st.p50_ns.to_string());
        print_profile("search_ns_p95", &st.p95_ns.to_string());
        print_profile("search_ns_p99", &st.p99_ns.to_string());
        print_profile("search_ns_min", &st.min.as_nanos().to_string());
        print_profile("search_ns_max", &st.max.as_nanos().to_string());
        print_profile(
            "search_ns_per_iter",
            &(st.sum.as_nanos() / u128::try_from(measured_count).unwrap_or(1)).to_string(),
        );
    }

    if rss_enabled()
        && let Some(b) = resident_set_bytes()
    {
        print_profile("rss_bytes_after", &b.to_string());
    }
}

const fn default_build_iters(kind: &CorpusKind) -> usize {
    match kind {
        CorpusKind::Parity | CorpusKind::Filter => 500,
        CorpusKind::Large { .. } => 2,
    }
}

pub fn run_build(iters: usize, kind: &CorpusKind) {
    let max_cap = match kind {
        CorpusKind::Parity | CorpusKind::Filter => 500,
        CorpusKind::Large { .. } => 20,
    };
    let iters = iters.clamp(1, max_cap);
    let t0 = Instant::now();
    for _ in 0..iters {
        let tmp = tempfile::tempdir().unwrap();
        let corpus = tmp.path().join("corpus");
        crate::corpus::materialize_build_corpus(&corpus, kind);
        let idx = tmp.path().join(".sift");
        let _ = sift_core::IndexBuilder::new(&corpus)
            .with_dir(&idx)
            .build()
            .unwrap();
    }
    let elapsed = t0.elapsed();
    let ns = ns_per_iter(elapsed, iters);
    match kind {
        CorpusKind::Parity => {
            print_profile("corpus_kind", "parity");
            print_profile("corpus_files", "32");
            print_profile("mode", "build_small_32files");
        }
        CorpusKind::Filter => {
            print_profile("corpus_kind", "filter");
            print_profile("corpus_files", "32");
            print_profile("mode", "build_small_32files");
        }
        CorpusKind::Large {
            files,
            lines_per_file,
            dir_fanout,
        } => {
            print_profile("corpus_kind", "large");
            print_profile("corpus_files", &files.to_string());
            print_profile("corpus_lines_per_file", &lines_per_file.to_string());
            print_profile("corpus_dir_fanout", &dir_fanout.to_string());
            print_profile("mode", "build_large");
        }
    }
    print_profile("scenario", "build");
    print_profile("command", "build");
    print_profile("iters", &iters.to_string());
    print_profile("total_ms", &format!("{:.3}", elapsed.as_secs_f64() * 1e3));
    print_profile("ns_per_iter", &ns.to_string());
}

pub fn build_iters_from_env(kind: &CorpusKind) -> usize {
    std::env::var("SIFT_PROFILE_ITERS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or_else(|| default_build_iters(kind))
}

fn run_search_loop(
    query: &CompiledSearch,
    index: &Index,
    filter: &SearchFilter,
    scenario: &Scenario,
    loop_cfg: &Loop,
) -> (Vec<Duration>, Duration) {
    let seps = SearchSeparators::default();
    let warm = warmup_iters();
    for _ in 0..warm {
        black_box(
            query
                .run_index(index, filter, scenario.output, &seps)
                .unwrap(),
        );
    }

    let mut samples: Vec<Duration> = Vec::new();
    let t_search = Instant::now();
    match loop_cfg {
        Loop::Timed(d) => {
            let deadline = Instant::now() + *d;
            while Instant::now() < deadline {
                let t0 = Instant::now();
                black_box(
                    query
                        .run_index(index, filter, scenario.output, &seps)
                        .unwrap(),
                );
                samples.push(t0.elapsed());
            }
        }
        Loop::Iters(n) => {
            for _ in 0..*n {
                let t0 = Instant::now();
                black_box(
                    query
                        .run_index(index, filter, scenario.output, &seps)
                        .unwrap(),
                );
                samples.push(t0.elapsed());
            }
        }
    }
    (samples, t_search.elapsed())
}
