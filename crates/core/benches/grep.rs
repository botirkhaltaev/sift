//! Grep-style search execution and filtering benchmarks.
//!
//! Exercises the public `Searcher::search` corpus pipeline.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::Path;

use sift_core::candidates::{
    CandidatePlanner, CandidateRequest, CandidateScope, CandidateSource, CandidateSpec, CorpusMode,
    IndexFallback,
};
use sift_core::grep::InputRequest;
use sift_core::grep::{CandidateFilter, CandidateFilterConfig, CandidateOrder};
use sift_core::search::{SearchFlags, SearchOptions, SearchQueryBuilder, Searcher, StatsMode};
use sift_core::{Index, Indexes, NGramIndex};

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

fn wrap_index(index: NGramIndex) -> Indexes {
    let root = index.root().to_path_buf();
    Indexes::from_single(Index::NGram(index), root)
}

fn make_search(patterns: &[&str], opts: SearchOptions) -> (Vec<String>, SearchOptions) {
    let pats: Vec<String> = patterns.iter().map(ToString::to_string).collect();
    (pats, opts)
}

fn make_filter(config: &CandidateFilterConfig, root: &Path) -> CandidateFilter {
    CandidateFilter::new(config, root).unwrap()
}

fn run_grep(
    indexes: &Indexes,
    filter: &CandidateFilter,
    query: &(Vec<String>, SearchOptions),
) -> bool {
    let source = CandidateSource {
        indexes,
        filter,
        store_meta: None,
    };
    let query = SearchQueryBuilder::new(query.0.clone())
        .options(query.1.clone())
        .build()
        .unwrap();
    let searcher = Searcher::new(query.clone()).unwrap();
    let request = CandidateRequest {
        scope: CandidateScope::Indexed,
        corpus: CorpusMode::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: CandidateOrder::default(),
    };
    let candidates = CandidatePlanner::new(&source, CandidateSpec::from(&query), request)
        .resolve()
        .unwrap();
    let input_request = InputRequest::from_candidates();
    let inputs = input_request.resolve(&candidates).unwrap();
    searcher.search(&inputs, StatsMode::Off).unwrap().matched()
}

fn bench_indexed_search(c: &mut Criterion) {
    let fixture = common::open_large_index();
    let index = fixture.1;
    let indexes = wrap_index(index);
    let filter = make_filter(&CandidateFilterConfig::default(), indexes.root());

    let mut g = c.benchmark_group("grep_indexed");

    g.bench_function("literal", |b| {
        let query = make_search(&["beta"], SearchOptions::default());
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("required_literal", |b| {
        let query = make_search(&["[A-Z]+_RESUME"], SearchOptions::default());
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("alternation", |b| {
        let query = make_search(
            &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
            SearchOptions::default(),
        );
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("case_insensitive", |b| {
        let query = make_search(
            &["beta"],
            SearchOptions {
                case_mode: sift_core::search::CaseMode::Insensitive,
                ..Default::default()
            },
        );
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("case_insensitive_alternation", |b| {
        let query = make_search(
            &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
            SearchOptions {
                case_mode: sift_core::search::CaseMode::Insensitive,
                ..Default::default()
            },
        );
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("full_scan_fallback", |b| {
        let query = make_search(
            &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
            SearchOptions::default(),
        );
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("invert_match", |b| {
        let query = make_search(
            &["beta"],
            SearchOptions {
                flags: SearchFlags::INVERT_MATCH,
                ..Default::default()
            },
        );
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.finish();
}

fn bench_walk_search(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    common::make_filter_corpus(&corpus);
    let filter = make_filter(&CandidateFilterConfig::default(), &corpus);
    let indexes = Indexes::open(&tmp.path().join(".sift")).unwrap();

    let mut g = c.benchmark_group("grep_walk");

    g.bench_function("literal", |b| {
        let query = make_search(&["beta"], SearchOptions::default());
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.bench_function("full_scan", |b| {
        let query = make_search(&[".*"], SearchOptions::default());
        b.iter(|| black_box(run_grep(&indexes, &filter, &query)));
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_indexed_search, bench_walk_search,
}
criterion_main!(benches);
