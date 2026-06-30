use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

use sift_core::grep::{GrepMode, GrepOutputFormat};
use sift_grep::Argv;
use sift_grep::output::{FilenameContext, OutputArgv, OutputConfig};
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
        g.bench_with_input(
            BenchmarkId::new("nul_terminated_paths", *name),
            &v,
            |b, a| {
                b.iter(|| {
                    black_box(
                        OutputArgv::resolve(&Argv::new(black_box(a)))
                            .path
                            .nul_terminated,
                    )
                });
            },
        );
    }

    let ctx_argv = args(&["sift", "-A", "5", "-B", "3", "-C", "10", "pattern"]);
    g.bench_function("context_lines", |b| {
        b.iter(|| black_box(PatternArgv::context(&Argv::new(&ctx_argv))));
    });

    let cli_default = parse_cli(&["pattern"]);
    let argv_default = args(&["sift", "pattern"]);
    g.bench_function("GrepOutput/default", |b| {
        b.iter(|| {
            let output = cli_default.grep_config().output;
            let output_argv = OutputArgv::resolve(&Argv::new(black_box(&argv_default)));
            black_box(output.grep_output(
                &output_argv,
                GrepMode::Standard,
                false,
                None,
                FilenameContext::DirectoryCorpus,
            ))
        });
    });

    g.bench_function("GrepOutput/format_json", |b| {
        let argv = args(&["sift", "--json", "pattern"]);
        b.iter(|| {
            let output_argv = OutputArgv::resolve(&Argv::new(black_box(&argv)));
            black_box(
                OutputConfig::format(&output_argv, GrepMode::Standard) == GrepOutputFormat::Json,
            )
        });
    });

    g.finish();
}
