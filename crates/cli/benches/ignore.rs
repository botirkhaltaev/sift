use criterion::Criterion;
use std::hint::black_box;

use sift_grep::Argv;
use sift_grep::ignore::IgnoreResolution;

use crate::support::args;

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("ignore");

    let argv_default = args(&["sift", "pattern"]);
    let argv_hidden = args(&["sift", "--hidden", "pattern"]);
    let argv_hidden_lw = args(&["sift", "--hidden", "--no-hidden", "--hidden", "pattern"]);
    let argv_uuu = args(&["sift", "-uuu", "pattern"]);

    g.bench_function("default", |b| {
        b.iter(|| {
            black_box(IgnoreResolution::resolve(&Argv::new(black_box(
                &argv_default,
            ))))
        });
    });

    g.bench_function("hidden_enabled", |b| {
        b.iter(|| {
            black_box(IgnoreResolution::resolve(&Argv::new(black_box(
                &argv_hidden,
            ))))
        });
    });

    g.bench_function("hidden_last_wins", |b| {
        b.iter(|| {
            black_box(IgnoreResolution::resolve(&Argv::new(black_box(
                &argv_hidden_lw,
            ))))
        });
    });

    g.bench_function("unrestricted_x3", |b| {
        b.iter(|| black_box(IgnoreResolution::resolve(&Argv::new(black_box(&argv_uuu)))));
    });

    let argv_all_no = args(&[
        "sift",
        "--no-ignore",
        "--no-ignore-vcs",
        "--no-ignore-dot",
        "--no-ignore-global",
        "--no-ignore-exclude",
        "--no-ignore-parent",
        "--no-require-git",
        "--no-messages",
        "--no-ignore-messages",
        "--no-ignore-files",
        "pattern",
    ]);
    g.bench_function("all_no_ignore", |b| {
        b.iter(|| {
            black_box(IgnoreResolution::resolve(&Argv::new(black_box(
                &argv_all_no,
            ))))
        });
    });

    let argv_all_toggle = args(&[
        "sift",
        "--no-ignore",
        "--ignore",
        "--no-ignore-vcs",
        "--ignore-vcs",
        "--no-ignore-dot",
        "--ignore-dot",
        "--no-ignore-global",
        "--ignore-global",
        "--no-ignore-exclude",
        "--ignore-exclude",
        "--no-ignore-parent",
        "--ignore-parent",
        "--no-require-git",
        "--require-git",
        "pattern",
    ]);
    g.bench_function("all_ignore_toggles", |b| {
        b.iter(|| {
            black_box(IgnoreResolution::resolve(&Argv::new(black_box(
                &argv_all_toggle,
            ))))
        });
    });

    g.finish();
}
