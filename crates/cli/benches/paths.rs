use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

use sift_grep::paths::{
    corpus_path_prefixes, effective_path_display, excluded_search_paths, walk_path_prefixes,
};

use crate::support::make_small_corpus;

fn bench_effective_path_display(c: &mut Criterion) {
    let mut g = c.benchmark_group("paths");

    let empty: &[PathBuf] = &[];
    let rel: &[PathBuf] = &[PathBuf::from("src")];
    let abs: &[PathBuf] = &[PathBuf::from("/home/user")];

    g.bench_with_input(
        BenchmarkId::new("effective_path_display", "empty"),
        empty,
        |b, s| {
            b.iter(|| black_box(effective_path_display(black_box(s))));
        },
    );
    g.bench_with_input(
        BenchmarkId::new("effective_path_display", "relative"),
        rel,
        |b, s| {
            b.iter(|| black_box(effective_path_display(black_box(s))));
        },
    );
    g.bench_with_input(
        BenchmarkId::new("effective_path_display", "absolute"),
        abs,
        |b, s| {
            b.iter(|| black_box(effective_path_display(black_box(s))));
        },
    );

    g.finish();
}

fn bench_path_prefixes(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_small_corpus(&corpus, 20);

    let empty_scope: &[PathBuf] = &[];
    let one_scope: &[PathBuf] = &[PathBuf::from("pkg0000")];
    let many_scope: Vec<PathBuf> = (0..20)
        .map(|i| PathBuf::from(format!("pkg{i:04}")))
        .collect();

    let mut g = c.benchmark_group("paths");

    g.bench_with_input(
        BenchmarkId::new("corpus_prefixes", "empty"),
        &(&corpus, empty_scope),
        |b, (root, scopes)| {
            b.iter(|| {
                black_box(
                    corpus_path_prefixes(black_box(root), black_box(root), black_box(scopes))
                        .unwrap(),
                )
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("corpus_prefixes", "one_scope"),
        &(&corpus, one_scope),
        |b, (root, scopes)| {
            b.iter(|| {
                black_box(
                    corpus_path_prefixes(black_box(root), black_box(root), black_box(scopes))
                        .unwrap(),
                )
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("corpus_prefixes", "many_scopes"),
        &(&corpus, &many_scope),
        |b, (root, scopes)| {
            b.iter(|| {
                black_box(
                    corpus_path_prefixes(black_box(root), black_box(root), black_box(scopes))
                        .unwrap(),
                )
            });
        },
    );

    g.bench_with_input(
        BenchmarkId::new("walk_prefixes", "empty"),
        &(&corpus, empty_scope),
        |b, (root, scopes)| {
            b.iter(|| black_box(walk_path_prefixes(black_box(root), black_box(scopes)).unwrap()));
        },
    );
    g.bench_with_input(
        BenchmarkId::new("walk_prefixes", "one_scope"),
        &(&corpus, one_scope),
        |b, (root, scopes)| {
            b.iter(|| black_box(walk_path_prefixes(black_box(root), black_box(scopes)).unwrap()));
        },
    );
    g.bench_with_input(
        BenchmarkId::new("walk_prefixes", "many_scopes"),
        &(&corpus, &many_scope),
        |b, (root, scopes)| {
            b.iter(|| black_box(walk_path_prefixes(black_box(root), black_box(scopes)).unwrap()));
        },
    );

    g.finish();
}

fn bench_excluded_paths(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let index_root = tmp.path().join("index_root");
    std::fs::create_dir_all(&index_root).unwrap();
    let sift_dir = index_root.join(".sift");
    std::fs::create_dir_all(&sift_dir).unwrap();
    let outside = tmp.path().join("outside");

    let mut g = c.benchmark_group("paths");

    g.bench_function("excluded_paths/inside", |b| {
        b.iter(|| {
            black_box(excluded_search_paths(
                black_box(&index_root),
                black_box(&sift_dir),
            ))
        });
    });
    g.bench_function("excluded_paths/outside", |b| {
        b.iter(|| {
            black_box(excluded_search_paths(
                black_box(&index_root),
                black_box(&outside),
            ))
        });
    });

    g.finish();
}

pub fn bench(c: &mut Criterion) {
    bench_effective_path_display(c);
    bench_path_prefixes(c);
    bench_excluded_paths(c);
}
