use criterion::{Criterion, Throughput};

use crate::support::{
    build_index, large_indexed_fixture, make_filter_corpus, make_small_corpus, run_sift, search_scale,
};

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

fn bench_indexed_small(c: &mut Criterion, small: &std::path::Path, small_sift: &std::path::Path) {
    let mut g = c.benchmark_group("e2e/subprocess/indexed_tiny");
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
    g.finish();
}

/// Monorepo-scale indexed CLI search (same `SIFT_BENCH_SCALE` as sift-core).
fn bench_indexed_large(c: &mut Criterion) {
    let (corpus, sift_dir) = large_indexed_fixture();
    let scale = search_scale();
    let mut g = c.benchmark_group("e2e/subprocess/indexed_large");
    g.throughput(Throughput::Elements(scale.files as u64));
    g.sample_size(20);
    g.measurement_time(std::time::Duration::from_secs(15));

    let sift = sift_dir.to_string_lossy().into_owned();
    g.bench_function("literal", |b| {
        b.iter(|| {
            run_sift(&["--sift-dir", &sift, "-n", "beta"], &corpus);
        });
    });
    g.bench_function("required_literal", |b| {
        b.iter(|| {
            run_sift(&["--sift-dir", &sift, "-n", "[A-Z]+_RESUME"], &corpus);
        });
    });
    g.bench_function("full_scan", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &sift,
                    "-n",
                    r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}",
                ],
                &corpus,
            );
        });
    });
    g.bench_function("alternation", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &sift,
                    "-n",
                    "ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT",
                ],
                &corpus,
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
    bench_indexed_small(c, &small, &small_sift);
    bench_indexed_large(c);

    let tmp_filt = tempfile::tempdir().unwrap();
    let filt = tmp_filt.path().join("corpus");
    make_filter_corpus(&filt);
    let filt_no_index = tmp_filt.path().join(".sift-absent");
    bench_filter(c, &filt, &filt_no_index);
}
