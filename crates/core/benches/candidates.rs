//! Candidate planning and resolution benchmarks.
//!
//! `iter` measures `CandidatePlanner::resolve` only. Index open, filter, and
//! `StoreMeta` stay outside the hot loop.
//!
//! Groups:
//! - `candidate_planner/*` — large-index strategies
//! - `candidate_planner_tiny/*` — walk fallback on the tiny filter corpus

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::Path;

use sift_core::candidates::{
    CandidateFlags, CandidatePlanner, CandidateRequest, CandidateScope, CandidateSource,
    CandidateSpec, CorpusMode, IndexFallback,
};
use sift_core::grep::{CandidateFilter, CandidateFilterConfig, CandidateOrder, VisibilityConfig};
use sift_core::{
    CorpusKind, CorpusMeta, FilterMeta, GramWidth, Index, IndexConfig, IndexCoverage, Indexes,
    NGramIndex, StoreMeta, WalkMeta,
};

mod common;

use common::criterion_config::sift_criterion;
use common::fixtures::{make_filter_corpus, open_large_index};

struct PlannerFixture {
    indexes: Indexes,
    filter: CandidateFilter,
    complete_meta: StoreMeta,
    lazy_meta: StoreMeta,
}

fn wrap_indexes(index: NGramIndex) -> Indexes {
    let root = index.root().to_path_buf();
    Indexes::from_single(Index::NGram(index), root)
}

fn store_meta(root: &Path, coverage: IndexCoverage) -> StoreMeta {
    StoreMeta::new(
        CorpusMeta {
            root: root.to_path_buf(),
            kind: CorpusKind::Directory,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
        },
        coverage,
        WalkMeta {
            follow_links: false,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        },
        FilterMeta {
            visibility: VisibilityConfig::default(),
        },
        vec![IndexConfig::ngram(GramWidth::TRIGRAM)],
    )
}

fn planner_fixture() -> PlannerFixture {
    let (_corpus, index) = open_large_index();
    let root = index.root().to_path_buf();
    let indexes = wrap_indexes(index);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &root).unwrap();
    PlannerFixture {
        indexes,
        filter,
        complete_meta: store_meta(&root, IndexCoverage::Complete),
        lazy_meta: store_meta(&root, IndexCoverage::Lazy),
    }
}

fn empty_index_fixture() -> (tempfile::TempDir, Indexes, CandidateFilter) {
    let temp = tempfile::tempdir().unwrap();
    let corpus = temp.path().join("corpus");
    make_filter_corpus(&corpus);
    let indexes = Indexes::open(&temp.path().join(".sift")).unwrap();
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).unwrap();
    (temp, indexes, filter)
}

fn resolve(
    indexes: &Indexes,
    filter: &CandidateFilter,
    patterns: &[String],
    flags: CandidateFlags,
    scope: CandidateScope,
    fallback: IndexFallback,
    meta: Option<&StoreMeta>,
) -> usize {
    let source = CandidateSource {
        indexes,
        filter,
        store_meta: meta,
    };
    let spec = CandidateSpec { patterns, flags };
    let request = CandidateRequest {
        scope,
        corpus: CorpusMode::Indexed,
        fallback,
        order: CandidateOrder::default(),
    };
    CandidatePlanner::new(&source, spec, request)
        .resolve()
        .unwrap()
        .len()
}

fn bench_candidate_planner(c: &mut Criterion) {
    let fixture = planner_fixture();
    let literal = vec!["[A-Z]+_RESUME".to_string()];
    let no_literal = vec![r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}".to_string()];

    let mut g = c.benchmark_group("candidate_planner");

    g.bench_function("use_index_literal", |b| {
        b.iter(|| {
            black_box(resolve(
                &fixture.indexes,
                &fixture.filter,
                &literal,
                CandidateFlags::empty(),
                CandidateScope::Indexed,
                IndexFallback::WalkOnStaleSnapshot,
                Some(&fixture.complete_meta),
            ));
        });
    });

    g.bench_function("all_indexed_complete", |b| {
        b.iter(|| {
            black_box(resolve(
                &fixture.indexes,
                &fixture.filter,
                &no_literal,
                CandidateFlags::empty(),
                CandidateScope::All,
                IndexFallback::IndexHitsOnly,
                Some(&fixture.complete_meta),
            ));
        });
    });

    g.bench_function("lazy_merge_index_and_walk", |b| {
        b.iter(|| {
            black_box(resolve(
                &fixture.indexes,
                &fixture.filter,
                &literal,
                CandidateFlags::empty(),
                CandidateScope::Indexed,
                IndexFallback::WalkOnStaleSnapshot,
                Some(&fixture.lazy_meta),
            ));
        });
    });

    g.finish();
}

fn bench_candidate_planner_tiny(c: &mut Criterion) {
    let (_temp, indexes, filter) = empty_index_fixture();
    let patterns = vec!["beta".to_string()];

    let mut g = c.benchmark_group("candidate_planner_tiny");
    g.bench_function("walk_fallback_empty_index", |b| {
        b.iter(|| {
            black_box(resolve(
                &indexes,
                &filter,
                &patterns,
                CandidateFlags::empty(),
                CandidateScope::Indexed,
                IndexFallback::WalkOnStaleSnapshot,
                None,
            ));
        });
    });
    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_candidate_planner, bench_candidate_planner_tiny,
}
criterion_main!(benches);
