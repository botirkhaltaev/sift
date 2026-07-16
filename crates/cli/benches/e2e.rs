use criterion::{Criterion, Throughput};

use crate::support::{build_index, make_filter_corpus, make_small_corpus, run_sift};

fn bench_tiny(c: &mut Criterion) {
    let tmp_tiny = tempfile::tempdir().unwrap();
    let tiny = tmp_tiny.path().join("corpus");
    std::fs::create_dir_all(&tiny).unwrap();
    std::fs::write(tiny.join("main.rs"), "fn beta() {}\n").unwrap();
    let tiny_no_index = tmp_tiny.path().join(".sift-absent");
    let tiny_sift = tmp_tiny.path().join(".sift-tiny");
    build_index(&tiny, &tiny_sift);

    let mut g = c.benchmark_group("e2e/subprocess/tiny");
    g.throughput(Throughput::Elements(1));
    g.bench_function("startup", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &tiny_no_index.to_string_lossy(), "beta"],
                &tiny,
            );
        });
    });
    g.bench_function("startup_with_flags", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &tiny_no_index.to_string_lossy(),
                    "-i",
                    "-n",
                    "-H",
                    "--color=never",
                    "beta",
                ],
                &tiny,
            );
        });
    });
    g.finish();
}

fn bench_walk(c: &mut Criterion, small: &std::path::Path, small_no_index: &std::path::Path) {
    let mut g = c.benchmark_group("e2e/subprocess/walk");
    g.throughput(Throughput::Elements(20));

    g.bench_function("literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &small_no_index.to_string_lossy(), "beta"],
                small,
            );
        });
    });
    g.bench_function("no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &small_no_index.to_string_lossy(), r"\w{10}"],
                small,
            );
        });
    });
    g.bench_function("json", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &small_no_index.to_string_lossy(),
                    "--json",
                    "beta",
                ],
                small,
            );
        });
    });
    g.bench_function("count", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &small_no_index.to_string_lossy(),
                    "-c",
                    "beta",
                ],
                small,
            );
        });
    });
    g.finish();
}

fn bench_indexed(c: &mut Criterion, small: &std::path::Path, small_sift: &std::path::Path) {
    let mut g = c.benchmark_group("e2e/subprocess/indexed");
    g.throughput(Throughput::Elements(20));

    g.bench_function("literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &small_sift.to_string_lossy(), "beta"],
                small,
            );
        });
    });
    g.bench_function("no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &small_sift.to_string_lossy(), r"\w{10}"],
                small,
            );
        });
    });
    g.bench_function("json", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &small_sift.to_string_lossy(),
                    "--json",
                    "beta",
                ],
                small,
            );
        });
    });
    g.bench_function("count", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &small_sift.to_string_lossy(), "-c", "beta"],
                small,
            );
        });
    });
    g.finish();
}

fn bench_filter(c: &mut Criterion, filt: &std::path::Path, filt_no_index: &std::path::Path) {
    let mut g = c.benchmark_group("e2e/subprocess/filter");

    g.bench_function("default", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &filt_no_index.to_string_lossy(), "beta"],
                filt,
            );
        });
    });
    g.bench_function("hidden", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &filt_no_index.to_string_lossy(),
                    "--hidden",
                    "beta",
                ],
                filt,
            );
        });
    });
    g.bench_function("glob_rust", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &filt_no_index.to_string_lossy(),
                    "-g",
                    "*.rs",
                    "beta",
                ],
                filt,
            );
        });
    });
    g.finish();
}

pub fn bench(c: &mut Criterion) {
    bench_tiny(c);

    let tmp_small = tempfile::tempdir().unwrap();
    let small = tmp_small.path().join("corpus");
    make_small_corpus(&small, 20);
    let small_sift = tmp_small.path().join(".sift");
    build_index(&small, &small_sift);
    let small_no_index = tmp_small.path().join(".sift-absent");
    bench_walk(c, &small, &small_no_index);
    bench_indexed(c, &small, &small_sift);

    let tmp_filt = tempfile::tempdir().unwrap();
    let filt = tmp_filt.path().join("corpus");
    make_filter_corpus(&filt);
    let filt_no_index = tmp_filt.path().join(".sift-absent");
    bench_filter(c, &filt, &filt_no_index);
}
