use criterion::Criterion;
use std::hint::black_box;

use crate::support::parse_cli;

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("cli_parse");

    g.bench_function("plain_pattern", |b| {
        b.iter(|| black_box(parse_cli(&["pattern"])));
    });

    g.bench_function("many_flags", |b| {
        b.iter(|| {
            black_box(parse_cli(&[
                "-i",
                "-n",
                "-H",
                "-c",
                "-e",
                "foo",
                "-e",
                "bar",
                "--hidden",
                "--no-ignore",
                "--color=never",
                "--json",
                "--stats",
                "pattern",
            ]))
        });
    });

    g.bench_function("path_scope", |b| {
        b.iter(|| black_box(parse_cli(&["pattern", "src/", "tests/", "docs/"])));
    });

    g.bench_function("build_subcommand", |b| {
        b.iter(|| black_box(parse_cli(&["build", "/tmp"])));
    });

    g.bench_function("last_wins_heavy", |b| {
        b.iter(|| {
            black_box(parse_cli(&[
                "-i",
                "-s",
                "-S",
                "-i",
                "-e",
                "foo",
                "-e",
                "bar",
                "-e",
                "baz",
                "-g",
                "*.rs",
                "-g",
                "*.toml",
                "--color=never",
                "--json",
                "--stats",
                "pattern",
            ]))
        });
    });

    g.bench_function("with_glob_and_type", |b| {
        b.iter(|| {
            black_box(parse_cli(&[
                "-g",
                "*.rs",
                "-g",
                "*.toml",
                "-t",
                "rust",
                "-T",
                "py",
                "--type-add",
                "mytype:*.my",
                "pattern",
            ]))
        });
    });

    g.bench_function("ignore_all_flags", |b| {
        b.iter(|| {
            black_box(parse_cli(&[
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
                "--hidden",
                "pattern",
            ]))
        });
    });

    g.finish();
}
