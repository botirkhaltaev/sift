//! Query-planning and pattern-compilation benchmarks.
//!
//! Exercises public `QueryPlanner` and `PatternCompiler` APIs.
//! All benches operate on small inputs and measure only the planning/compilation cost.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::{CaseMode, CompiledSearch, PatternCompiler, QueryFlags, QueryPlanner, QuerySpec, SearchOptions};

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}

// ─── QueryPlanner benches ────────────────────────────────────────────────────

fn bench_query_planner(c: &mut Criterion) {
    let mut g = c.benchmark_group("query_planner");

    g.bench_function("literal", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("fixed_string", |b| {
        let spec = QuerySpec {
            patterns: &["beta.gamma".to_string()],
            flags: QueryFlags::FIXED_STRINGS,
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("word_regexp", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::WORD_REGEXP,
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("line_regexp", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::LINE_REGEXP,
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("case_insensitive", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::CASE_INSENSITIVE,
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("invert_match", |b| {
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::INVERT_MATCH,
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("alternation", |b| {
        let spec = QuerySpec {
            patterns: &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("required_literal", |b| {
        let spec = QuerySpec {
            patterns: &["[A-Z]+_RESUME".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("no_literal", |b| {
        let spec = QuerySpec {
            patterns: &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("unicode_class", |b| {
        let spec = QuerySpec {
            patterns: &[r"\p{Greek}".to_string()],
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.bench_function("multi_pattern", |b| {
        let pats = vec!["hello".to_string(), "world".to_string(), "foo".to_string()];
        let spec = QuerySpec {
            patterns: &pats,
            flags: QueryFlags::empty(),
        };
        b.iter(|| black_box(QueryPlanner::should_use_indexes(&spec)));
    });

    g.finish();
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

// ─── CompiledSearch::new benches ─────────────────────────────────────────────

fn bench_compiled_search_new(c: &mut Criterion) {
    let mut g = c.benchmark_group("compiled_search_new");

    g.bench_function("one_pattern", |b| {
        let pats = vec!["hello".to_string()];
        b.iter(|| {
            black_box(CompiledSearch::new(&pats, SearchOptions::default()).unwrap());
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
            black_box(CompiledSearch::new(&pats, SearchOptions::default()).unwrap());
        });
    });

    g.bench_function("case_insensitive", |b| {
        let pats = vec!["hello".to_string()];
        let opts = sift_core::SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..Default::default()
        };
        b.iter(|| {
            black_box(CompiledSearch::new(&pats, opts.clone()).unwrap());
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_query_planner, bench_pattern_compiler, bench_compiled_search_new,
}
criterion_main!(benches);
