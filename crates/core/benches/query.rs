//! Pattern-compilation and search-compilation benchmarks.
//!
//! Exercises public `PatternCompiler` and `SearchQuery` APIs.
//! All benches operate on small inputs and measure only the compilation cost.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::search::{CaseMode, PatternCompiler, SearchOptions};
use sift_core::SearchQuery;

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}

// ─── PatternCompiler benches ─────────────────────────────────────────────────

fn bench_pattern_compiler(c: &mut Criterion) {
    let mut g = c.benchmark_group("pattern_compiler");

    g.bench_function("shape_fixed_string", |b| {
        let compiler = PatternCompiler::new().fixed_strings(true);
        b.iter(|| black_box(compiler.shape("a.c*+?")));
    });

    g.bench_function("shape_word_regexp", |b| {
        let compiler = PatternCompiler::new().word_regexp(true);
        b.iter(|| black_box(compiler.shape("hello")));
    });

    g.bench_function("shape_line_regexp", |b| {
        let compiler = PatternCompiler::new().line_regexp(true);
        b.iter(|| black_box(compiler.shape("hello")));
    });

    g.bench_function("compile_one", |b| {
        let compiler = PatternCompiler::new();
        b.iter(|| black_box(compiler.compile_one("hello\\s+world").unwrap()));
    });

    g.bench_function("compile_many", |b| {
        let compiler = PatternCompiler::new();
        let patterns = &["foo", "bar", "baz", "qux", "quux"];
        b.iter(|| black_box(compiler.compile(patterns).unwrap()));
    });

    g.bench_function("compile_case_insensitive", |b| {
        let compiler = PatternCompiler::new().case_insensitive(true);
        let patterns = &["Hello", "World"];
        b.iter(|| black_box(compiler.compile(patterns).unwrap()));
    });

    g.finish();
}

// ─── SearchQuery::new benches ───────────────────────────────────────────────

fn bench_compiled_search_new(c: &mut Criterion) {
    let mut g = c.benchmark_group("compiled_search_new");

    g.bench_function("one_pattern", |b| {
        let pats = vec!["hello".to_string()];
        b.iter(|| {
            black_box(SearchQuery::new(&pats, SearchOptions::default()).unwrap());
        });
    });

    g.bench_function("many_patterns", |b| {
        let pats = vec![
            "foo".to_string(),
            "bar".to_string(),
            "baz".to_string(),
            "qux".to_string(),
        ];
        b.iter(|| {
            black_box(SearchQuery::new(&pats, SearchOptions::default()).unwrap());
        });
    });

    g.bench_function("case_insensitive", |b| {
        let pats = vec!["hello".to_string()];
        let opts = SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..Default::default()
        };
        b.iter(|| {
            black_box(SearchQuery::new(&pats, opts.clone()).unwrap());
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_pattern_compiler, bench_compiled_search_new,
}
criterion_main!(benches);
