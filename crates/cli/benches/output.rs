use criterion::{BenchmarkId, Criterion, measurement::WallTime};
use std::hint::black_box;

use sift_cli::output::{
    build_line_style_flags, parse_color_when, parse_usize_token, resolve_color_from_args,
    resolve_context_from_args, resolve_glob_case_insensitive_from_args, resolve_heading_from_args,
    resolve_json_from_args, resolve_line_number_from_args, resolve_null_from_args,
    resolve_stats_from_args, resolve_with_filename_from_args, unescape_separator,
};
use sift_core::SearchMode;

use crate::support::{args, parse_cli};

fn bench_resolve_null(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let null_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("null", &["sift", "-0", "pattern"]),
    ];
    for (name, argv) in &null_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_null", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_null_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_color(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let color_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("never", &["sift", "--color=never", "pattern"]),
        (
            "last_wins",
            &["sift", "--color=never", "--color=always", "pattern"],
        ),
    ];
    for (name, argv) in &color_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_color", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_color_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_json(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let json_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("json", &["sift", "--json", "pattern"]),
        (
            "last_wins",
            &["sift", "--json", "--no-json", "--json", "pattern"],
        ),
    ];
    for (name, argv) in &json_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_json", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_json_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_stats(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let stats_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("stats", &["sift", "--stats", "pattern"]),
    ];
    for (name, argv) in &stats_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_stats", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_stats_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_heading(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let heading_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        (
            "last_wins",
            &["sift", "--heading", "--no-heading", "--heading", "pattern"],
        ),
    ];
    for (name, argv) in &heading_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_heading", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_heading_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_line_number(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let ln_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("last_wins", &["sift", "-n", "-N", "-n", "pattern"]),
    ];
    for (name, argv) in &ln_cases {
        let v = args(argv);
        g.bench_with_input(
            BenchmarkId::new("resolve_line_number", *name),
            &v,
            |b, a| {
                b.iter(|| black_box(resolve_line_number_from_args(black_box(a))));
            },
        );
    }
}

fn bench_resolve_with_filename(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let fn_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("last_wins", &["sift", "-H", "-I", "-H", "pattern"]),
    ];
    for (name, argv) in &fn_cases {
        let v = args(argv);
        g.bench_with_input(
            BenchmarkId::new("resolve_with_filename", *name),
            &v,
            |b, a| {
                b.iter(|| black_box(resolve_with_filename_from_args(black_box(a))));
            },
        );
    }
}

fn bench_resolve_context(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let ctx_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        (
            "all_flags",
            &["sift", "-A", "5", "-B", "3", "-C", "10", "pattern"],
        ),
    ];
    for (name, argv) in &ctx_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("resolve_context", *name), &v, |b, a| {
            b.iter(|| black_box(resolve_context_from_args(black_box(a))));
        });
    }
}

fn bench_resolve_glob_case_insensitive(g: &mut criterion::BenchmarkGroup<'_, WallTime>) {
    let gci_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        (
            "last_wins",
            &[
                "sift",
                "--glob-case-insensitive",
                "--no-glob-case-insensitive",
                "--glob-case-insensitive",
                "pattern",
            ],
        ),
    ];
    for (name, argv) in &gci_cases {
        let v = args(argv);
        g.bench_with_input(
            BenchmarkId::new("resolve_glob_case_insensitive", *name),
            &v,
            |b, a| {
                b.iter(|| black_box(resolve_glob_case_insensitive_from_args(black_box(a))));
            },
        );
    }
}

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("output");

    bench_resolve_null(&mut g);
    bench_resolve_color(&mut g);
    bench_resolve_json(&mut g);
    bench_resolve_stats(&mut g);
    bench_resolve_heading(&mut g);
    bench_resolve_line_number(&mut g);
    bench_resolve_with_filename(&mut g);
    bench_resolve_context(&mut g);
    bench_resolve_glob_case_insensitive(&mut g);

    g.bench_function("parse_usize_token", |b| {
        b.iter(|| black_box(parse_usize_token("42")));
    });

    g.bench_function("parse_color_when", |b| {
        b.iter(|| {
            let _ = black_box(parse_color_when("never"));
            let _ = black_box(parse_color_when("always"));
            let _ = black_box(parse_color_when("auto"));
        });
    });

    g.bench_function("unescape_separator/plain", |b| {
        b.iter(|| black_box(unescape_separator("::")));
    });
    g.bench_function("unescape_separator/escapes", |b| {
        b.iter(|| black_box(unescape_separator(r"\n\t\\\0")));
    });

    // build_line_style_flags
    let cli_line = parse_cli(&["-n", "-H", "--column", "pattern"]);
    let argv_line = args(&["sift", "-n", "-H", "--column", "pattern"]);
    let line_number = resolve_line_number_from_args(&argv_line).unwrap_or(false);
    let (out, _filter) = cli_line.build_output_and_filter(
        &argv_line,
        SearchMode::Standard,
        false,
        Some(line_number),
    );
    g.bench_function("build_line_style_flags", |b| {
        b.iter(|| black_box(build_line_style_flags(black_box(&out), line_number)));
    });

    // Cli::build_output_and_filter
    let cli_default = parse_cli(&["pattern"]);
    let argv_default = args(&["sift", "pattern"]);

    let cli_json = parse_cli(&["--json", "pattern"]);
    let argv_json = args(&["sift", "--json", "pattern"]);

    let cli_verbose_strs = &[
        "-n",
        "-H",
        "--heading",
        "--color=always",
        "--stats",
        "--null",
        "--context",
        "3",
        "pattern",
    ];
    let cli_verbose = parse_cli(cli_verbose_strs);
    let argv_verbose = args(&[
        "sift",
        "-n",
        "-H",
        "--heading",
        "--color=always",
        "--stats",
        "--null",
        "--context",
        "3",
        "pattern",
    ]);

    g.bench_function("build_output_and_filter/default", |b| {
        b.iter(|| {
            cli_default.build_output_and_filter(
                black_box(&argv_default),
                SearchMode::Standard,
                false,
                None,
            )
        });
    });

    g.bench_function("build_output_and_filter/json", |b| {
        b.iter(|| {
            cli_json.build_output_and_filter(
                black_box(&argv_json),
                SearchMode::Standard,
                false,
                None,
            )
        });
    });

    g.bench_function("build_output_and_filter/verbose", |b| {
        b.iter(|| {
            cli_verbose.build_output_and_filter(
                black_box(&argv_verbose),
                SearchMode::Standard,
                false,
                None,
            )
        });
    });

    g.finish();
}
