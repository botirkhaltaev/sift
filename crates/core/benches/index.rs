//! Index build, open, candidate, and persistence benchmarks.
//!
//! Exercises public `TrigramIndexBuilder`, `TrigramIndex`, `Indexes`, and `Index` APIs.
//! Storage effects are measured indirectly through build/open/save/reopen paths.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::{
    CorpusKind, Index, IndexBuildConfig, IndexStore, Indexes, QueryFlags, QuerySpec, TrigramIndex,
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

// ─── Build benchmarks ────────────────────────────────────────────────────────

fn bench_index_build(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_build");

    g.bench_function("single_file", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_single_file_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            common::build_index(&corpus, &idx);
        });
    });

    g.bench_function("small_corpus", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_parity_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            common::build_index(&corpus, &idx);
        });
    });

    g.bench_function("many_tiny_files", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_many_files_corpus(&corpus, 1_000);
            let idx = tmp.path().join(".sift");
            common::build_index(&corpus, &idx);
        });
    });

    g.bench_function("monorepo", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::materialize_monorepo_corpus(&corpus, 8_000, 100, 256);
            let idx = tmp.path().join(".sift");
            common::build_index(&corpus, &idx);
        });
    });

    g.finish();
}

// ─── Open benchmarks ─────────────────────────────────────────────────────────

fn bench_index_open(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_open");

    g.bench_function("small", |b| {
        let (_tmp, idx_dir, root) = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_parity_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            let built = common::build_index(&corpus, &idx);
            let root = built.root().to_path_buf();
            drop(built);
            (tmp, idx, root)
        };
        b.iter(|| {
            black_box(common::open_index(&idx_dir, &root, CorpusKind::Directory));
        });
    });

    g.bench_function("large", |b| {
        let (_tmp, idx_dir, root) = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::materialize_large_corpus(&corpus, 8_000, 100, 256);
            let idx = tmp.path().join(".sift");
            let built = common::build_index(&corpus, &idx);
            let root = built.root().to_path_buf();
            drop(built);
            (tmp, idx, root)
        };
        b.iter(|| {
            black_box(common::open_index(&idx_dir, &root, CorpusKind::Directory));
        });
    });

    g.finish();
}

// ─── Indexes::open benchmarks ────────────────────────────────────────────────

fn bench_indexes_open(c: &mut Criterion) {
    let mut g = c.benchmark_group("indexes_open");

    g.bench_function("empty_registry", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let sift_dir = tmp.path().join(".sift");
            std::fs::create_dir_all(&sift_dir).unwrap();
            black_box(Indexes::open(&sift_dir).unwrap());
        });
    });

    g.bench_function("one_trigram_index", |b| {
        let (_tmp, sift_dir) = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_parity_corpus(&corpus);
            let sift = tmp.path().join(".sift");
            let mut store =
                IndexStore::open_or_create(&sift, &corpus, CorpusKind::Directory, false)
                    .expect("open store");
            store
                .build::<TrigramIndex>(&IndexBuildConfig {
                    root: &corpus,
                    follow_links: false,
                    exclude_paths: &[],
                    include_paths: &[],
                    corpus_kind: CorpusKind::Directory,
                })
                .expect("build");
            drop(store);
            (tmp, sift)
        };
        b.iter(|| {
            black_box(Indexes::open(&sift_dir).unwrap());
        });
    });

    g.finish();
}

// ─── Save/reopen benchmarks ──────────────────────────────────────────────────

fn bench_index_save_reopen(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_save_reopen");

    g.bench_function("save_and_reopen", |b| {
        let (_tmp, index) = common::open_parity_index();
        let root = index.root().to_path_buf();
        let corpus_kind = index.corpus_kind();
        b.iter(|| {
            let tmp2 = tempfile::tempdir().unwrap();
            let save_dir = tmp2.path().join("saved_index");
            index.save_to_dir(&save_dir).unwrap();
            black_box(common::open_index(&save_dir, &root, corpus_kind));
        });
    });

    g.finish();
}

// ─── TrigramIndex inherent method benches ────────────────────────────────────

fn bench_trigram_index_methods(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();

    let mut g = c.benchmark_group("trigram_index");

    g.bench_function("file_path", |b| {
        b.iter(|| black_box(index.file_path(sift_core::FileId::new(0))));
    });

    g.bench_function("file_abs_path", |b| {
        b.iter(|| black_box(index.file_abs_path(sift_core::FileId::new(0))));
    });

    g.finish();
}

// ─── Candidate benches ───────────────────────────────────────────────────────

fn bench_candidates(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();

    let mut g = c.benchmark_group("index_candidates");

    g.bench_function("literal", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.candidates(&spec)));
    });

    g.bench_function("required_literal", |b| {
        let spec = QuerySpec {
            patterns: &["[A-Z]+_RESUME".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.candidates(&spec)));
    });

    g.bench_function("full_scan_fallback", |b| {
        let spec = QuerySpec {
            patterns: &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.candidates(&spec)));
    });

    g.bench_function("alternation", |b| {
        let spec = QuerySpec {
            patterns: &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.candidates(&spec)));
    });

    g.bench_function("case_insensitive", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::CASE_INSENSITIVE,
        };
        b.iter(|| black_box(index.candidates(&spec)));
    });

    g.finish();
}

// ─── Explain benches ─────────────────────────────────────────────────────────

fn bench_explain(c: &mut Criterion) {
    let (_tmp, index) = common::open_large_index();

    let mut g = c.benchmark_group("index_explain");

    g.bench_function("indexed_mode", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.explain(&spec)));
    });

    g.bench_function("full_scan_mode", |b| {
        let spec = QuerySpec {
            patterns: &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(index.explain(&spec)));
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_index_build, bench_index_open, bench_indexes_open, bench_index_save_reopen, bench_trigram_index_methods, bench_candidates, bench_explain,
}
criterion_main!(benches);
