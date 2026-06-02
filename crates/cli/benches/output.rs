use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

use sift_core::SearchMode;
use sift_grep::Argv;
use sift_grep::output::{OutputArgv, SearchOutputCtx};
use sift_grep::pattern::PatternArgv;

use crate::support::{args, parse_cli};

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("output");

    let null_cases = [
        ("none", &["sift", "pattern"] as &[&str]),
        ("null", &["sift", "-0", "pattern"]),
    ];
    for (name, argv) in &null_cases {
        let v = args(argv);
        g.bench_with_input(BenchmarkId::new("null_data", *name), &v, |b, a| {
            b.iter(|| black_box(OutputArgv::resolve(&Argv::new(black_box(a))).path.null_data));
        });
    }

    let ctx_argv = args(&["sift", "-A", "5", "-B", "3", "-C", "10", "pattern"]);
    g.bench_function("context_lines", |b| {
        b.iter(|| black_box(PatternArgv::context(&Argv::new(&ctx_argv))));
    });

    let cli_default = parse_cli(&["pattern"]);
    let argv_default = args(&["sift", "pattern"]);
    g.bench_function("SearchOutputCtx_resolve_default", |b| {
        b.iter(|| {
            black_box(SearchOutputCtx::resolve(
                &cli_default.grep_config().output,
                &Argv::new(black_box(&argv_default)),
                SearchMode::Standard,
                false,
                None,
            ))
        });
    });

    g.finish();
}
