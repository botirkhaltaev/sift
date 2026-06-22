//! Index build, open, candidate, and persistence benchmarks.
//!
//! Exercises public `TrigramIndexBuilder`, `TrigramIndex`, `Indexes`, and `Index` APIs.
//! Storage effects are measured indirectly through build/open/save/reopen paths.

use criterion::{Criterion, criterion_group, criterion_main};
use std::fs;
use std::hint::black_box;
use std::path::Path;

use sift_core::{
    CorpusKind, CorpusMeta, CorpusSpec, FilterMeta, IndexConfig, IndexKind, IndexStore,
    IndexWalkConfig, Indexes, QueryFlags, QuerySpec, StoreMeta, VisibilityConfig, WalkMeta,
};

mod common;

struct IndexOpenFixture {
    temp: tempfile::TempDir,
    idx_dir: std::path::PathBuf,
    root: std::path::PathBuf,
}

struct SiftDirFixture {
    temp: tempfile::TempDir,
    sift_dir: std::path::PathBuf,
}

impl Drop for IndexOpenFixture {
    fn drop(&mut self) {
        let _ = &mut self.temp;
    }
}

impl Drop for SiftDirFixture {
    fn drop(&mut self) {
        let _ = &mut self.temp;
    }
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

// ─── Index-only corpus helpers ───────────────────────────────────────────────

fn make_parity_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/x.txt"), "alpha beta\n").unwrap();
    fs::write(root.join("b/y.txt"), "gamma delta\n").unwrap();
}

fn make_single_file_corpus(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("single.rs"),
        "fn main() {\n    let x = 42;\n    println!(\"beta: {}\", x);\n}\n",
    )
    .unwrap();
}

fn make_many_files_corpus(root: &Path, n: usize) {
    for i in 0..n {
        let dir = root.join(format!("d{}", i % 10));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("f{i}.txt")),
            format!("line one line two content {i}\n"),
        )
        .unwrap();
    }
}

fn materialize_monorepo_corpus(
    root: &Path,
    files: usize,
    lines_per_file: usize,
    dir_fanout: usize,
) {
    common::materialize_large_corpus(root, files, lines_per_file, dir_fanout);
}

// ─── Index-only build helpers ────────────────────────────────────────────────

fn standard_build_config<'a>(
    root: &'a Path,
    exclude_paths: &'a [std::path::PathBuf],
) -> IndexConfig<'a> {
    IndexConfig {
        corpus: CorpusSpec {
            root,
            kind: CorpusKind::Directory,
            follow_links: false,
            include_paths: &[],
            exclude_paths,
        },
        walk: IndexWalkConfig::new(false),
        visibility: VisibilityConfig::default(),
    }
}

/// Full `sift build` path via [`IndexStore`] (production defaults).
fn build_index_via_store(corpus: &Path, sift_dir: &Path) {
    let corpus_path = corpus.to_path_buf();
    let root = corpus.canonicalize().unwrap_or(corpus_path);
    let meta = sift_core::StoreMeta::new(
        CorpusMeta {
            root,
            kind: CorpusKind::Directory,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
        },
        WalkMeta {
            follow_links: false,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        },
        FilterMeta {
            visibility: VisibilityConfig::default(),
        },
        vec![IndexKind::Trigram],
    );
    let mut store = IndexStore::open_or_create(sift_dir, &meta).unwrap();
    let config = standard_build_config(corpus, &[]);
    store.build(&[IndexKind::Trigram], &config, &[]).unwrap();
}

// ─── Build benchmarks ────────────────────────────────────────────────────────

fn bench_index_build(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_build");

    g.bench_function("single_file", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            make_single_file_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            build_index_via_store(&corpus, &idx);
        });
    });

    g.bench_function("small_corpus", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            make_parity_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            build_index_via_store(&corpus, &idx);
        });
    });

    g.bench_function("filter_corpus", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::make_filter_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            build_index_via_store(&corpus, &idx);
        });
    });

    g.bench_function("many_tiny_files", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            make_many_files_corpus(&corpus, 1_000);
            let idx = tmp.path().join(".sift");
            build_index_via_store(&corpus, &idx);
        });
    });

    g.bench_function("monorepo", |b| {
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            materialize_monorepo_corpus(&corpus, 8_000, 100, 256);
            let idx = tmp.path().join(".sift");
            build_index_via_store(&corpus, &idx);
        });
    });

    g.finish();
}

// ─── Open benchmarks ─────────────────────────────────────────────────────────

fn bench_index_open(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_open");

    g.bench_function("small", |b| {
        let fixture = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            make_parity_corpus(&corpus);
            let idx = tmp.path().join(".sift");
            let built = common::build_index(&corpus, &idx);
            let root = built.root().to_path_buf();
            drop(built);
            IndexOpenFixture {
                temp: tmp,
                idx_dir: idx,
                root,
            }
        };
        b.iter(|| {
            black_box(common::open_index(
                &fixture.idx_dir,
                &fixture.root,
                CorpusKind::Directory,
            ));
        });
    });

    g.bench_function("large", |b| {
        let fixture = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            common::materialize_large_corpus(&corpus, 8_000, 100, 256);
            let idx = tmp.path().join(".sift");
            let built = common::build_index(&corpus, &idx);
            let root = built.root().to_path_buf();
            drop(built);
            IndexOpenFixture {
                temp: tmp,
                idx_dir: idx,
                root,
            }
        };
        b.iter(|| {
            black_box(common::open_index(
                &fixture.idx_dir,
                &fixture.root,
                CorpusKind::Directory,
            ));
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
        let fixture = {
            let tmp = tempfile::tempdir().unwrap();
            let corpus = tmp.path().join("corpus");
            make_parity_corpus(&corpus);
            let sift = tmp.path().join(".sift");
            let corpus_path = corpus.clone();
            let root = corpus.canonicalize().unwrap_or(corpus_path);
            let meta = StoreMeta::new(
                CorpusMeta {
                    root,
                    kind: CorpusKind::Directory,
                    include_paths: Vec::new(),
                    exclude_paths: Vec::new(),
                },
                WalkMeta {
                    follow_links: false,
                    one_file_system: false,
                    max_depth: None,
                    max_filesize: None,
                },
                FilterMeta {
                    visibility: VisibilityConfig::default(),
                },
                vec![IndexKind::Trigram],
            );
            let mut store = IndexStore::open_or_create(&sift, &meta).expect("open store");
            store
                .build(
                    &[IndexKind::Trigram],
                    &IndexConfig {
                        corpus: CorpusSpec {
                            root: &corpus,
                            kind: CorpusKind::Directory,
                            follow_links: false,
                            include_paths: &[],
                            exclude_paths: &[],
                        },
                        walk: IndexWalkConfig::new(false),
                        visibility: VisibilityConfig::default(),
                    },
                    &[],
                )
                .expect("build");
            drop(store);
            SiftDirFixture {
                temp: tmp,
                sift_dir: sift,
            }
        };
        b.iter(|| {
            black_box(Indexes::open(&fixture.sift_dir).unwrap());
        });
    });

    g.finish();
}

// ─── Save/reopen benchmarks ──────────────────────────────────────────────────

fn bench_index_save_reopen(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_save_reopen");

    g.bench_function("reopen", |b| {
        let tmp = tempfile::tempdir().unwrap();
        let corpus = tmp.path().join("corpus");
        make_parity_corpus(&corpus);
        let idx_dir = tmp.path().join(".sift");
        let index = common::build_index(&corpus, &idx_dir);
        let root = index.root().to_path_buf();
        let kind = index.corpus_kind();
        drop(index);
        b.iter(|| {
            black_box(common::open_index(&idx_dir, &root, kind));
        });
    });

    g.finish();
}

// ─── TrigramIndex inherent method benches ────────────────────────────────────

fn bench_trigram_index_methods(c: &mut Criterion) {
    let fixture = common::open_large_index();
    let index = fixture.1;

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
    let fixture = common::open_large_index();
    let index = fixture.1;

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
    let fixture = common::open_large_index();
    let index = fixture.1;

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
