//! Search query compilation benchmarks.
//!
//! Exercises the public searcher compilation API.
//! All benches operate on small inputs and measure only the compilation cost.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::search::{
    CaseMode, RegexEngine, SearchFlags, SearchOptions, SearchQueryBuilder, Searcher,
};

#[path = "common/criterion_config.rs"]
mod criterion_config;

use criterion_config::sift_criterion;

fn searcher(patterns: Vec<String>, options: SearchOptions) -> Searcher {
    let query = SearchQueryBuilder::new(patterns)
        .options(options)
        .build()
        .unwrap();
    Searcher::new(query).unwrap()
}

fn bench_query_compile(c: &mut Criterion) {
    let mut g = c.benchmark_group("query_compile");

    g.bench_function("one_pattern", |b| {
        let pats = vec!["hello".to_string()];
        b.iter(|| {
            let query = searcher(pats.clone(), SearchOptions::default());
            black_box(query);
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
            let query = searcher(pats.clone(), SearchOptions::default());
            black_box(query);
        });
    });

    g.bench_function("fixed_strings", |b| {
        let pats = vec!["a.c*+?".to_string()];
        let opts = SearchOptions {
            flags: SearchFlags::FIXED_STRINGS,
            ..Default::default()
        };
        b.iter(|| {
            let query = searcher(pats.clone(), opts.clone());
            black_box(query);
        });
    });

    g.bench_function("word_regexp", |b| {
        let pats = vec!["hello".to_string()];
        let opts = SearchOptions {
            flags: SearchFlags::WORD_REGEXP,
            ..Default::default()
        };
        b.iter(|| {
            let query = searcher(pats.clone(), opts.clone());
            black_box(query);
        });
    });

    g.bench_function("line_regexp", |b| {
        let pats = vec!["hello".to_string()];
        let opts = SearchOptions {
            flags: SearchFlags::LINE_REGEXP,
            ..Default::default()
        };
        b.iter(|| {
            let query = searcher(pats.clone(), opts.clone());
            black_box(query);
        });
    });

    g.bench_function("case_insensitive", |b| {
        let pats = vec!["hello".to_string()];
        let opts = SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..Default::default()
        };
        b.iter(|| {
            let query = searcher(pats.clone(), opts.clone());
            black_box(query);
        });
    });

    g.bench_function("pcre2_auto_fallback", |b| {
        let pats = vec!["(?<=hello) world".to_string()];
        let opts = SearchOptions {
            regex_engine: RegexEngine::Auto,
            ..Default::default()
        };
        b.iter(|| {
            let query = searcher(pats.clone(), opts.clone());
            black_box(query);
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_query_compile,
}
criterion_main!(benches);
