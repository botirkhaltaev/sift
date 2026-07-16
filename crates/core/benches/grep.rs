//! Search execution benchmarks.
//!
//! Groups:
//! - `grep_search/*` — `Searcher::search` on pre-resolved inputs (Searcher + candidates outside iter)
//! - `grep_pipeline/*` — plan candidates + search (Searcher outside iter; resolve inside)
//! - `grep_walk_tiny/*` — walk on the tiny filter corpus (not full-scan signal)

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::candidates::{
    CandidatePlanner, CandidateRequest, CandidateScope, CandidateSource, CandidateSpec, CorpusMode,
    IndexFallback,
};
use sift_core::Candidate;
use sift_core::grep::InputRequest;
use sift_core::grep::{CandidateFilter, CandidateFilterConfig, CandidateOrder};
use sift_core::search::{
    CaseMode, Inputs, SearchFlags, SearchOptions, SearchQuery, SearchQueryBuilder, Searcher,
    StatsMode,
};
use sift_core::{Index, Indexes};

mod common;

use common::criterion_config::sift_criterion;
use common::fixtures::{make_filter_corpus, open_large_index};

struct IndexedFixture {
    indexes: Indexes,
    filter: CandidateFilter,
}

fn wrap_indexes(index: sift_core::NGramIndex) -> Indexes {
    let root = index.root().to_path_buf();
    Indexes::from_single(Index::NGram(index), root)
}

fn indexed_fixture() -> IndexedFixture {
    let (_corpus, index) = open_large_index();
    let indexes = wrap_indexes(index);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), indexes.root()).unwrap();
    IndexedFixture { indexes, filter }
}

fn build_query(patterns: &[&str], options: SearchOptions) -> SearchQuery {
    let pats: Vec<String> = patterns.iter().map(ToString::to_string).collect();
    SearchQueryBuilder::new(pats).options(options).build().unwrap()
}

fn build_searcher(query: &SearchQuery) -> Searcher {
    Searcher::new(query.clone()).unwrap()
}

fn indexed_request() -> CandidateRequest {
    CandidateRequest {
        scope: CandidateScope::Indexed,
        corpus: CorpusMode::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: CandidateOrder::default(),
    }
}

fn resolve_candidates(
    indexes: &Indexes,
    filter: &CandidateFilter,
    query: &SearchQuery,
) -> Vec<Candidate> {
    let source = CandidateSource {
        indexes,
        filter,
        store_meta: None,
    };
    CandidatePlanner::new(&source, CandidateSpec::from(query), indexed_request())
        .resolve()
        .unwrap()
}

fn search_only(searcher: &Searcher, inputs: &Inputs<'_>) -> bool {
    searcher.search(inputs, StatsMode::Off).unwrap().matched()
}

fn pipeline(
    indexes: &Indexes,
    filter: &CandidateFilter,
    searcher: &Searcher,
    query: &SearchQuery,
) -> bool {
    let candidates = resolve_candidates(indexes, filter, query);
    let request = InputRequest::from_candidates();
    let inputs = request.resolve(&candidates).unwrap();
    search_only(searcher, &inputs)
}

struct SearchCase {
    name: &'static str,
    searcher: Searcher,
    candidates: Vec<Candidate>,
}

fn bench_grep_search(c: &mut Criterion) {
    let fx = indexed_fixture();
    let mut g = c.benchmark_group("grep_search");

    let specs: &[(&str, SearchQuery)] = &[
        ("literal", build_query(&["beta"], SearchOptions::default())),
        (
            "required_literal",
            build_query(&["[A-Z]+_RESUME"], SearchOptions::default()),
        ),
        (
            "alternation",
            build_query(
                &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
                SearchOptions::default(),
            ),
        ),
        (
            "case_insensitive",
            build_query(
                &["beta"],
                SearchOptions {
                    case_mode: CaseMode::Insensitive,
                    ..Default::default()
                },
            ),
        ),
        (
            "full_scan",
            build_query(
                &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
                SearchOptions::default(),
            ),
        ),
        (
            "invert_match",
            build_query(
                &["beta"],
                SearchOptions {
                    flags: SearchFlags::INVERT_MATCH,
                    ..Default::default()
                },
            ),
        ),
    ];

    let cases: Vec<SearchCase> = specs
        .iter()
        .map(|(name, query)| SearchCase {
            name,
            searcher: build_searcher(query),
            candidates: resolve_candidates(&fx.indexes, &fx.filter, query),
        })
        .collect();

    for case in &cases {
        let request = InputRequest::from_candidates();
        let inputs = request.resolve(&case.candidates).unwrap();
        g.bench_function(case.name, |b| {
            b.iter(|| black_box(search_only(&case.searcher, &inputs)));
        });
    }

    g.finish();
}

fn bench_grep_pipeline(c: &mut Criterion) {
    let fx = indexed_fixture();
    let mut g = c.benchmark_group("grep_pipeline");

    let cases: &[(&str, SearchQuery)] = &[
        ("literal", build_query(&["beta"], SearchOptions::default())),
        (
            "full_scan",
            build_query(
                &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
                SearchOptions::default(),
            ),
        ),
    ];

    for (name, query) in cases {
        let searcher = build_searcher(query);
        g.bench_function(*name, |b| {
            b.iter(|| black_box(pipeline(&fx.indexes, &fx.filter, &searcher, query)));
        });
    }

    g.finish();
}

fn bench_grep_walk_tiny(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_filter_corpus(&corpus);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).unwrap();
    let indexes = Indexes::open(&tmp.path().join(".sift")).unwrap();

    let mut g = c.benchmark_group("grep_walk_tiny");

    let query = build_query(&["beta"], SearchOptions::default());
    let searcher = build_searcher(&query);
    g.bench_function("literal", |b| {
        b.iter(|| black_box(pipeline(&indexes, &filter, &searcher, &query)));
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_grep_search, bench_grep_pipeline, bench_grep_walk_tiny,
}
criterion_main!(benches);
