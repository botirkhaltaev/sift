use std::path::PathBuf;

use criterion::{BenchmarkGroup, BenchmarkId, Criterion, measurement::WallTime};
use std::hint::black_box;

use std::str::FromStr;

use sift_grep::filter::{ByteSize, FilterDecl, GrepFilterCtx, TypeCatalog};

use crate::support::parse_cli;

fn bench_filter_type_defs_variants(g: &mut BenchmarkGroup<'_, WallTime>) {
    let decl_clear = FilterDecl {
        type_clear: vec!["rust".into(), "py".into(), "js".into()],
        ..Default::default()
    };
    let decl_add = FilterDecl {
        type_add: vec!["mytype:*.my".into(), "rust:*.rsx".into()],
        ..Default::default()
    };

    let type_cases = [("with_clear", &decl_clear), ("with_add", &decl_add)];
    for (name, decl) in &type_cases {
        g.bench_with_input(
            BenchmarkId::new("TypeCatalog::from_decl", *name),
            decl,
            |b, d| {
                b.iter(|| black_box(TypeCatalog::from_decl(black_box(d)).into_definitions()));
            },
        );
    }
}

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("filter");

    g.bench_function("ByteSize/from_str", |b| {
        b.iter(|| {
            let _ = black_box(ByteSize::from_str("42"));
            let _ = black_box(ByteSize::from_str("100K"));
            let _ = black_box(ByteSize::from_str("2MB"));
            let _ = black_box(ByteSize::from_str("1G"));
        });
    });

    let decl_default = FilterDecl::default();
    g.bench_with_input(
        BenchmarkId::new("TypeCatalog::from_decl", "default"),
        &decl_default,
        |b, d| {
            b.iter(|| black_box(TypeCatalog::from_decl(black_box(d)).into_definitions()));
        },
    );

    bench_filter_type_defs_variants(&mut g);

    let cli_plain = parse_cli(&["pattern"]);
    let filter_ctx_default = GrepFilterCtx::default();

    g.bench_function("candidate_config/default", |b| {
        b.iter(|| {
            black_box(cli_plain.filter_config().candidate_config(
                filter_ctx_default,
                vec![],
                vec![],
            ))
        });
    });

    let cli_glob = parse_cli(&[
        "-g",
        "*.rs",
        "-g",
        "*.toml",
        "-t",
        "rust",
        "--max-depth",
        "10",
        "--max-filesize",
        "1MB",
        "pattern",
    ]);
    let filter_ctx_glob = GrepFilterCtx {
        glob_case_insensitive: true,
        ..Default::default()
    };
    g.bench_function("candidate_config/with_glob_and_type", |b| {
        b.iter(|| {
            black_box(cli_glob.filter_config().candidate_config(
                filter_ctx_glob,
                vec![PathBuf::from("")],
                vec![],
            ))
        });
    });

    g.finish();
}
