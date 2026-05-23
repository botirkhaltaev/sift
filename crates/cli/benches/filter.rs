use std::path::PathBuf;

use criterion::{BenchmarkGroup, BenchmarkId, Criterion, measurement::WallTime};
use std::hint::black_box;

use sift_cli::filter::{
    FilterDecl, SearchFilterCtx, build_search_filter_config, builtin_type_defs, parse_size_suffix,
    resolve_type_defs,
};
use sift_cli::ignore::MessageFlags;
use sift_core::IgnoreSources;

use crate::support::parse_cli;

fn bench_filter_type_defs_variants(g: &mut BenchmarkGroup<'_, WallTime>) {
    let decl_clear = FilterDecl {
        max_depth: None,
        max_filesize: None,
        iglob: vec![],
        ignore_file: vec![],
        files: false,
        type_include: vec![],
        type_exclude: vec![],
        type_list: false,
        type_add: vec![],
        type_clear: vec!["rust".into(), "py".into(), "js".into()],
        sort: None,
        sortr: None,
    };
    let decl_add = FilterDecl {
        max_depth: None,
        max_filesize: None,
        iglob: vec![],
        ignore_file: vec![],
        files: false,
        type_include: vec![],
        type_exclude: vec![],
        type_list: false,
        type_add: vec!["mytype:*.my".into(), "rust:*.rsx".into()],
        type_clear: vec![],
        sort: None,
        sortr: None,
    };

    let type_cases = [("with_clear", &decl_clear), ("with_add", &decl_add)];
    for (name, decl) in &type_cases {
        g.bench_with_input(
            BenchmarkId::new("resolve_type_defs", *name),
            decl,
            |b, d| {
                b.iter(|| black_box(resolve_type_defs(black_box(d))));
            },
        );
    }
}

pub fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("filter");

    g.bench_function("parse_size_suffix", |b| {
        b.iter(|| {
            let _ = black_box(parse_size_suffix("42"));
            let _ = black_box(parse_size_suffix("100K"));
            let _ = black_box(parse_size_suffix("2MB"));
            let _ = black_box(parse_size_suffix("1G"));
        });
    });

    g.bench_function("builtin_type_defs", |b| {
        b.iter(|| black_box(builtin_type_defs()));
    });

    let decl_default = FilterDecl {
        max_depth: None,
        max_filesize: None,
        iglob: vec![],
        ignore_file: vec![],
        files: false,
        type_include: vec![],
        type_exclude: vec![],
        type_list: false,
        type_add: vec![],
        type_clear: vec![],
        sort: None,
        sortr: None,
    };
    g.bench_with_input(
        BenchmarkId::new("resolve_type_defs", "default"),
        &decl_default,
        |b, d| {
            b.iter(|| black_box(resolve_type_defs(black_box(d))));
        },
    );

    bench_filter_type_defs_variants(&mut g);

    // build_filter_config
    let cli_plain = parse_cli(&["pattern"]);
    let filter_ctx_default = SearchFilterCtx {
        hidden: false,
        ignore_sources: IgnoreSources::all(),
        require_git: false,
        glob_case_insensitive: false,
        msg_flags: MessageFlags::empty(),
    };

    g.bench_function("build_filter_config/default", |b| {
        b.iter(|| black_box(cli_plain.build_filter_config(filter_ctx_default, vec![], vec![])));
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
    let filter_ctx_glob = SearchFilterCtx {
        hidden: false,
        ignore_sources: IgnoreSources::all(),
        require_git: false,
        glob_case_insensitive: true,
        msg_flags: MessageFlags::empty(),
    };
    g.bench_function("build_filter_config/with_glob_and_type", |b| {
        b.iter(|| {
            black_box(cli_glob.build_filter_config(
                filter_ctx_glob,
                vec![PathBuf::from("")],
                vec![],
            ))
        });
    });

    // build_search_filter_config
    g.bench_function("build_search_filter_config/default", |b| {
        b.iter(|| {
            black_box(build_search_filter_config(
                &cli_plain,
                filter_ctx_default,
                vec![],
                vec![],
            ))
        });
    });

    g.finish();
}
