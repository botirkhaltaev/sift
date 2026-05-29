use criterion::{BatchSize, Criterion};
use std::hint::black_box;

use sift_grep::pattern::{
    resolve_case_mode_from_args, resolve_invert_match_from_args, resolve_output_mode,
    resolve_patterns,
};

use crate::support::{args, parse_cli};

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("pattern");

    // resolve_case_mode_from_args
    let argv_none = args(&["sift", "pattern"]);
    let argv_single_i = args(&["sift", "-i", "pattern"]);
    let argv_last_wins = args(&["sift", "-i", "-s", "-S", "-i", "pattern"]);

    g.bench_function("case_mode/default", |b| {
        b.iter(|| black_box(resolve_case_mode_from_args(black_box(&argv_none))));
    });
    g.bench_function("case_mode/single_i", |b| {
        b.iter(|| black_box(resolve_case_mode_from_args(black_box(&argv_single_i))));
    });
    g.bench_function("case_mode/last_wins", |b| {
        b.iter(|| black_box(resolve_case_mode_from_args(black_box(&argv_last_wins))));
    });

    // resolve_invert_match_from_args
    let argv_v = args(&["sift", "-v", "pattern"]);
    let argv_long = args(&["sift", "--invert-match", "pattern"]);
    let argv_dash = args(&["sift", "-v", "--", "-v"]);

    g.bench_function("invert_match/none", |b| {
        b.iter(|| black_box(resolve_invert_match_from_args(black_box(&argv_none))));
    });
    g.bench_function("invert_match/short_v", |b| {
        b.iter(|| black_box(resolve_invert_match_from_args(black_box(&argv_v))));
    });
    g.bench_function("invert_match/long", |b| {
        b.iter(|| black_box(resolve_invert_match_from_args(black_box(&argv_long))));
    });
    g.bench_function("invert_match/dash_dash_terminates", |b| {
        b.iter(|| black_box(resolve_invert_match_from_args(black_box(&argv_dash))));
    });

    // resolve_output_mode
    let argv_output_lw = args(&["sift", "-c", "-l", "-o", "-q", "pattern"]);

    g.bench_function("output_mode/default", |b| {
        b.iter(|| black_box(resolve_output_mode(black_box(&argv_none), false)));
    });
    g.bench_function("output_mode/last_wins", |b| {
        b.iter(|| black_box(resolve_output_mode(black_box(&argv_output_lw), false)));
    });
    g.bench_function("output_mode/invert_and_only", |b| {
        b.iter(|| black_box(resolve_output_mode(black_box(&argv_none), true)));
    });

    // resolve_patterns
    g.bench_function("patterns/positional", |b| {
        b.iter(|| black_box(resolve_patterns(&[], None, Some("beta")).unwrap()));
    });

    g.bench_function("patterns/repeated_e", |b| {
        let regexp = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
        b.iter(|| black_box(resolve_patterns(&regexp, None, None).unwrap()));
    });

    g.bench_function("patterns/from_file", |b| {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "foo\nbar\nbaz\n# comment\n\nqux\n").unwrap();
        b.iter_batched(
            || tmp.path().to_path_buf(),
            |p| black_box(resolve_patterns(&[], Some(&p), None).unwrap()),
            BatchSize::SmallInput,
        );
    });

    g.bench_function("patterns/e_and_positional", |b| {
        let regexp = vec!["foo".to_string()];
        b.iter(|| black_box(resolve_patterns(&regexp, None, Some("bar")).unwrap()));
    });

    // Cli::build_search_opts
    let cli_default = parse_cli(&["pattern"]);
    let argv_default = args(&["sift", "pattern"]);

    let cli_context = parse_cli(&["-A", "5", "-B", "3", "pattern"]);
    let argv_context = args(&["sift", "-A", "5", "-B", "3", "pattern"]);

    let cli_flags = parse_cli(&[
        "-i",
        "-w",
        "-x",
        "-F",
        "-U",
        "--crlf",
        "--no-unicode",
        "--regex-size-limit",
        "10MB",
        "pattern",
    ]);
    let argv_flags = args(&[
        "sift",
        "-i",
        "-w",
        "-x",
        "-F",
        "-U",
        "--crlf",
        "--no-unicode",
        "--regex-size-limit",
        "10MB",
        "pattern",
    ]);

    g.bench_function("build_search_opts/default", |b| {
        b.iter(|| black_box(cli_default.build_search_opts(black_box(&argv_default), false)));
    });
    g.bench_function("build_search_opts/context", |b| {
        b.iter(|| black_box(cli_context.build_search_opts(black_box(&argv_context), false)));
    });
    g.bench_function("build_search_opts/with_flags", |b| {
        b.iter(|| black_box(cli_flags.build_search_opts(black_box(&argv_flags), false)));
    });

    g.finish();
}
