//! Search query compilation benchmarks.
//!
//! Exercises the public `Query` compilation API.
//! All benches operate on small inputs and measure only the compilation cost.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use sift_core::Query;
use sift_core::grep::{CaseMode, MatchFlags, MatchOptions, RegexEngineRequest};

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}

fn bench_query_compile(c: &mut Criterion) {
    let mut g = c.benchmark_group("query_compile");

    g.bench_function("one_pattern", |b| {
        let pats = vec!["hello".to_string()];
        b.iter(|| {
            let query = Query::new(pats.clone())
                .unwrap()
                .options(MatchOptions::default());
            black_box(query.compile().unwrap());
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
            let query = Query::new(pats.clone())
                .unwrap()
                .options(MatchOptions::default());
            black_box(query.compile().unwrap());
        });
    });

    g.bench_function("fixed_strings", |b| {
        let pats = vec!["a.c*+?".to_string()];
        let opts = MatchOptions {
            flags: MatchFlags::FIXED_STRINGS,
            ..Default::default()
        };
        b.iter(|| {
            let query = Query::new(pats.clone()).unwrap().options(opts.clone());
            black_box(query.compile().unwrap());
        });
    });

    g.bench_function("word_regexp", |b| {
        let pats = vec!["hello".to_string()];
        let opts = MatchOptions {
            flags: MatchFlags::WORD_REGEXP,
            ..Default::default()
        };
        b.iter(|| {
            let query = Query::new(pats.clone()).unwrap().options(opts.clone());
            black_box(query.compile().unwrap());
        });
    });

    g.bench_function("line_regexp", |b| {
        let pats = vec!["hello".to_string()];
        let opts = MatchOptions {
            flags: MatchFlags::LINE_REGEXP,
            ..Default::default()
        };
        b.iter(|| {
            let query = Query::new(pats.clone()).unwrap().options(opts.clone());
            black_box(query.compile().unwrap());
        });
    });

    g.bench_function("case_insensitive", |b| {
        let pats = vec!["hello".to_string()];
        let opts = MatchOptions {
            case_mode: CaseMode::Insensitive,
            ..Default::default()
        };
        b.iter(|| {
            let query = Query::new(pats.clone()).unwrap().options(opts.clone());
            black_box(query.compile().unwrap());
        });
    });

    g.bench_function("pcre2_auto_fallback", |b| {
        let pats = vec!["(?<=hello) world".to_string()];
        let opts = MatchOptions {
            regex_engine: RegexEngineRequest::Auto,
            ..Default::default()
        };
        b.iter(|| {
            let query = Query::new(pats.clone()).unwrap().options(opts.clone());
            black_box(query.compile().unwrap());
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
