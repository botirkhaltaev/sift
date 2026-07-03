//! Candidate planning and resolution benchmarks.
//!
//! Exercises `CandidatePlanner` strategy paths independently from grep execution.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::Path;

use sift_core::candidates::{
    CandidateFlags, CandidatePlanner, CandidateRequest, CandidateScope, CandidateSource,
    CandidateSpec, CorpusMode, IndexFallback,
};
use sift_core::grep::{CandidateFilter, CandidateFilterConfig, CandidateOrder, VisibilityConfig};
use sift_core::{
    CorpusKind, CorpusMeta, FilterMeta, GramWidth, IndexConfig, IndexCoverage, Indexes, StoreMeta,
    WalkMeta,
};
use sift_core::{Index, NGramIndex};

mod common;

struct PlannerFixture {
    _temp: tempfile::TempDir,
    indexes: Indexes,
    filter: CandidateFilter,
    complete_meta: StoreMeta,
    lazy_meta: StoreMeta,
}

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
    let (temp, index) = common::open_large_index();
    let root = index.root().to_path_buf();
    let indexes = wrap_index(index);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &root).unwrap();
    PlannerFixture {
        _temp: temp,
        indexes,
        filter,
        complete_meta: store_meta(&root, IndexCoverage::Complete),
        lazy_meta: store_meta(&root, IndexCoverage::Lazy),
    }
}

fn empty_index_fixture() -> (tempfile::TempDir, Indexes, CandidateFilter) {
    let temp = tempfile::tempdir().unwrap();
    let corpus = temp.path().join("corpus");
    common::make_filter_corpus(&corpus);
    let indexes = Indexes::open(&temp.path().join(".sift")).unwrap();
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).unwrap();
    (temp, indexes, filter)
}

fn resolve(
    fixture: &PlannerFixture,
    patterns: &[String],
    flags: CandidateFlags,
    scope: CandidateScope,
    fallback: IndexFallback,
    meta: Option<&StoreMeta>,
) -> usize {
    let source = CandidateSource {
        indexes: &fixture.indexes,
        filter: &fixture.filter,
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
                &fixture,
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
                &fixture,
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
                &fixture,
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

fn bench_candidate_planner_walk(c: &mut Criterion) {
    let (_temp, indexes, filter) = empty_index_fixture();
    let patterns = vec!["beta".to_string()];
    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };
    let spec = CandidateSpec {
        patterns: &patterns,
        flags: CandidateFlags::empty(),
    };
    let request = CandidateRequest {
        scope: CandidateScope::Indexed,
        corpus: CorpusMode::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: CandidateOrder::default(),
    };

    let mut g = c.benchmark_group("candidate_planner");
    g.bench_function("walk_fallback_empty_index", |b| {
        b.iter(|| {
            black_box(
                CandidatePlanner::new(&source, spec, request)
                    .resolve()
                    .unwrap()
                    .len(),
            );
        });
    });
    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_candidate_planner, bench_candidate_planner_walk,
}
criterion_main!(benches);
