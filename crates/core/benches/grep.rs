//! Grep-style search execution, filtering, and output-mode benchmarks.
//!
//! Exercises public `CompiledSearch::run_indexes`, `CompiledSearch::run_walk`,
//! `SearchFilter`, and output-mode paths.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::{SearchIndex, SearchMode, SearchOptions, SearchOutput, SearchStats};

mod common;

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}

// ─── Indexed search benches ──────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn bench_indexed_search(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();
    let filter = common::make_filter(&common::default_filter(), index.root());

    let mut g = c.benchmark_group("grep_indexed");

    g.bench_function("literal", |b| {
        let query = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("required_literal", |b| {
        let query = common::make_search(&["[A-Z]+_RESUME"], SearchOptions::default());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("alternation", |b| {
        let query = common::make_search(
            &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
            SearchOptions::default(),
        );
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("full_scan_fallback", |b| {
        let query = common::make_search(
            &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
            SearchOptions::default(),
        );
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("max_count", |b| {
        let opts = SearchOptions {
            max_results: Some(1),
            ..SearchOptions::default()
        };
        let query = common::make_search(&["beta"], opts);
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_std(),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("quiet", |b| {
        let query = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("stats", |b| {
        let query = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            let mut stats = SearchStats::default();
            black_box(
                query
                    .run_indexes_with_stats(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                        &mut stats,
                    )
                    .unwrap(),
            );
        });
    });

    g.finish();
}

// ─── Walk search benches ─────────────────────────────────────────────────────

fn bench_walk_search(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();
    let corpus = index.root().to_path_buf();
    let filter = common::make_filter(&common::default_filter(), &corpus);

    let mut g = c.benchmark_group("grep_walk");

    g.bench_function("literal", |b| {
        let query = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                query
                    .run_walk(
                        &corpus,
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("no_literal", |b| {
        let query = common::make_search(
            &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
            SearchOptions::default(),
        );
        b.iter(|| {
            black_box(
                query
                    .run_walk(
                        &corpus,
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("filtered_walk", |b| {
        let filter_cfg = common::glob_include_filter();
        let filter = common::make_filter(&filter_cfg, &corpus);
        let query = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                query
                    .run_walk(
                        &corpus,
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.finish();
}

// ─── Filter benches ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn bench_filters(c: &mut Criterion) {
    let (_tmp, index) = common::open_filter_index();
    let query = common::make_search(&["beta"], SearchOptions::default());

    let mut g = c.benchmark_group("grep_filters");

    g.bench_function("no_filter", |b| {
        let filter = common::make_filter(&common::default_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("glob_include", |b| {
        let filter = common::make_filter(&common::glob_include_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("glob_exclude", |b| {
        let filter = common::make_filter(&common::glob_exclude_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("glob_casei", |b| {
        let filter = common::make_filter(&common::glob_casei_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("hidden_default", |b| {
        let filter = common::make_filter(&common::default_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("hidden_include", |b| {
        let filter = common::make_filter(&common::hidden_include_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("ignore_default", |b| {
        let filter = common::make_filter(&common::default_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("ignore_custom", |b| {
        let filter = common::make_filter(&common::ignore_custom_filter(), index.root());
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_quiet(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("scoped", |b| {
        let filter = common::make_filter(&common::scoped_filter("subdir"), index.root());
        let output = SearchOutput {
            mode: SearchMode::FilesWithMatches,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.finish();
}

// ─── Output-mode benches ─────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn bench_output_modes(c: &mut Criterion) {
    let (_tmp, index) = common::open_parity_index();
    let query = common::make_search(&["beta"], SearchOptions::default());
    let filter = common::make_filter(&common::default_filter(), index.root());

    let mut g = c.benchmark_group("grep_output_modes");

    g.bench_function("standard", |b| {
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_std(),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.bench_function("only_matching", |b| {
        let output = SearchOutput {
            mode: SearchMode::OnlyMatching,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.bench_function("count", |b| {
        let output = SearchOutput {
            mode: SearchMode::Count,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.bench_function("count_matches", |b| {
        let output = SearchOutput {
            mode: SearchMode::CountMatches,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.bench_function("files_with_matches", |b| {
        let output = SearchOutput {
            mode: SearchMode::FilesWithMatches,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.bench_function("files_without_match", |b| {
        let output = SearchOutput {
            mode: SearchMode::FilesWithoutMatch,
            emission: common::output_std().emission,
            ..SearchOutput::default()
        };
        b.iter(|| {
            black_box(
                query
                    .run_indexes(&[&index], &filter, output, &common::default_seps())
                    .unwrap(),
            );
        });
    });

    g.bench_function("json", |b| {
        b.iter(|| {
            black_box(
                query
                    .run_indexes(
                        &[&index],
                        &filter,
                        common::output_json(SearchMode::Standard),
                        &common::default_seps(),
                    )
                    .unwrap(),
            );
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_indexed_search, bench_walk_search, bench_filters, bench_output_modes,
}
criterion_main!(benches);
