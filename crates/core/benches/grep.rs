//! Grep-style search execution, filtering, and output-mode benchmarks.
//!
//! Exercises the public `grep::run` pipeline.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::grep::{GrepRequest, run as grep_run};
use sift_core::{
    Index, Indexes, SearchMatchFlags, SearchMode, SearchOptions, SearchQuery, TrigramIndex,
};

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

fn wrap_index(index: TrigramIndex) -> Indexes {
    let root = index.root().to_path_buf();
    Indexes::from_single(index, root)
}

// ─── Indexed search benches ──────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn bench_indexed_search(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();
    let indexes = wrap_index(index);
    let filter = common::make_filter(&common::default_filter(), indexes.root());

    let mut g = c.benchmark_group("grep_indexed");

    g.bench_function("literal", |b| {
        let query: SearchQuery = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("required_literal", |b| {
        let query: SearchQuery = common::make_search(&["[A-Z]+_RESUME"], SearchOptions::default());
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("alternation", |b| {
        let query: SearchQuery = common::make_search(
            &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
            SearchOptions::default(),
        );
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("case_insensitive", |b| {
        let query: SearchQuery = common::make_search(
            &["beta"],
            SearchOptions {
                case_mode: sift_core::CaseMode::Insensitive,
                ..Default::default()
            },
        );
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("full_scan_fallback", |b| {
        let query: SearchQuery = common::make_search(
            &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
            SearchOptions::default(),
        );
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("invert_match", |b| {
        let query: SearchQuery = common::make_search(
            &["beta"],
            SearchOptions {
                flags: SearchMatchFlags::INVERT_MATCH,
                ..Default::default()
            },
        );
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("indexed_search_with_stats", |b| {
        let query: SearchQuery = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: true,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.finish();
}

// ─── Walk search benches ─────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn bench_walk_search(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    common::make_filter_corpus(&corpus);
    let filter = common::make_filter(&common::default_filter(), &corpus);
    let indexes = Indexes::open(&tmp.path().join(".sift")).unwrap();

    let mut g = c.benchmark_group("grep_walk");

    g.bench_function("literal", |b| {
        let query: SearchQuery = common::make_search(&["beta"], SearchOptions::default());
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("full_scan", |b| {
        let query: SearchQuery = common::make_search(&[".*"], SearchOptions::default());
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Standard),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.finish();
}

// ─── Output mode benches ─────────────────────────────────────────────────────

fn bench_output_modes(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();
    let indexes = wrap_index(index);
    let filter = common::make_filter(&common::default_filter(), indexes.root());
    let query: SearchQuery = common::make_search(&["beta"], SearchOptions::default());

    let mut g = c.benchmark_group("grep_output_modes");

    g.bench_function("count", |b| {
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::Count),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("files_with_matches", |b| {
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::FilesWithMatches),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.bench_function("files_without_match", |b| {
        b.iter(|| {
            black_box(
                grep_run(
                    &query,
                    &GrepRequest {
                        indexes: &indexes,
                        filter: &filter,
                        output: common::output_quiet(SearchMode::FilesWithoutMatch),
                        separators: &common::default_seps(),
                        collect_stats: false,
                    },
                )
                .unwrap()
                .matched,
            );
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_indexed_search, bench_walk_search, bench_output_modes,
}
criterion_main!(benches);
