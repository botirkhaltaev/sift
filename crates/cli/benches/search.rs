use criterion::Criterion;
use std::hint::black_box;

use crate::support::{build_index, make_small_corpus, parse_cli, run_sift};

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("search_dispatch");

    // run_type_list
    g.bench_function("type_list", |b| {
        let tmp = tempfile::tempdir().unwrap();
        b.iter(|| run_sift(&["--type-list"], tmp.path()));
    });

    // Cli::resolve_binary_mode
    let cli_default = parse_cli(&["pattern"]);
    let cli_text = parse_cli(&["-a", "pattern"]);

    g.bench_function("resolve_binary_mode/default", |b| {
        b.iter(|| black_box(cli_default.resolve_binary_mode()));
    });
    g.bench_function("resolve_binary_mode/text", |b| {
        b.iter(|| black_box(cli_text.resolve_binary_mode()));
    });

    // run_files_mode — subprocess
    let tmp_files = tempfile::tempdir().unwrap();
    let f_corpus = tmp_files.path().join("corpus");
    make_small_corpus(&f_corpus, 20);
    let no_index = tmp_files.path().join(".sift-absent");

    g.bench_function("files_mode/default", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "--files"],
                &f_corpus,
            );
        });
    });

    // Walk search dispatch
    g.bench_function("walk_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "beta"],
                &f_corpus,
            );
        });
    });
    g.bench_function("walk_no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), r"\w{10}"],
                &f_corpus,
            );
        });
    });
    g.bench_function("walk_json", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "--json", "beta"],
                &f_corpus,
            );
        });
    });
    g.bench_function("walk_count", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "-c", "beta"],
                &f_corpus,
            );
        });
    });

    // Indexed search dispatch
    let tmp_idx = tempfile::tempdir().unwrap();
    let i_corpus = tmp_idx.path().join("corpus");
    make_small_corpus(&i_corpus, 20);
    let sift_dir = tmp_idx.path().join(".sift");
    build_index(&i_corpus, &sift_dir);

    g.bench_function("indexed_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "beta"],
                &i_corpus,
            );
        });
    });
    g.bench_function("indexed_no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), r"\w{10}"],
                &i_corpus,
            );
        });
    });
    g.bench_function("indexed_json", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "--json", "beta"],
                &i_corpus,
            );
        });
    });
    g.bench_function("indexed_count", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "-c", "beta"],
                &i_corpus,
            );
        });
    });

    g.finish();
}
