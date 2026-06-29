//! Grep-style search execution, filtering, and output-mode benchmarks.
//!
//! Exercises the public `Grep::run` corpus pipeline.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::Path;

use sift_core::grep::{
    CandidateFilter, CandidateFilterConfig, ColorChoice, GrepCollection, GrepMatchFlags, GrepMode,
    GrepOptions, GrepOutput, GrepRecordStyle, GrepSeparators, OutputEmission,
};
use sift_core::grep::{CandidateIndexState, Grep, GrepCorpus, GrepQuery};
use sift_core::{Index, Indexes, NGramIndex, SnapshotValidation};

mod common;

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}

fn wrap_index(index: NGramIndex) -> Indexes {
    let root = index.root().to_path_buf();
    Indexes::from_single(Index::NGram(index), root)
}

// ─── Grep-specific helpers ───────────────────────────────────────────────────

fn quiet_output(mode: GrepMode) -> GrepOutput {
    GrepOutput {
        mode,
        emission: OutputEmission::Quiet,
        records: GrepRecordStyle {
            color: ColorChoice::Never,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn make_search(patterns: &[&str], opts: GrepOptions) -> (Vec<String>, GrepOptions) {
    let pats: Vec<String> = patterns.iter().map(ToString::to_string).collect();
    (pats, opts)
}

fn make_filter(config: &CandidateFilterConfig, root: &Path) -> CandidateFilter {
    CandidateFilter::new(config, root).unwrap()
}

fn run_grep(
    indexes: &Indexes,
    filter: &CandidateFilter,
    query: &(Vec<String>, GrepOptions),
    mode: GrepMode,
    collect: GrepCollection,
) -> bool {
    let query = GrepQuery::new(query.0.clone())
        .unwrap()
        .options(query.1.clone());
    let corpus = GrepCorpus::new(
        indexes,
        filter,
        CandidateIndexState {
            store_meta: None,
            snapshot: SnapshotValidation::Unvalidated,
        },
    );
    Grep::new(query)
        .corpus(corpus)
        .output(quiet_output(mode))
        .separators(&GrepSeparators::default())
        .collect(collect)
        .run()
        .unwrap()
        .matched()
}

fn run_standard(
    indexes: &Indexes,
    filter: &CandidateFilter,
    query: &(Vec<String>, GrepOptions),
) -> bool {
    run_grep(
        indexes,
        filter,
        query,
        GrepMode::Standard,
        GrepCollection::none(),
    )
}

// ─── Indexed search benches ──────────────────────────────────────────────────

fn bench_indexed_search(c: &mut Criterion) {
    let fixture = common::open_large_index();
    let index = fixture.1;
    let indexes = wrap_index(index);
    let filter = make_filter(&CandidateFilterConfig::default(), indexes.root());

    let mut g = c.benchmark_group("grep_indexed");

    g.bench_function("literal", |b| {
        let query = make_search(&["beta"], GrepOptions::default());
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("required_literal", |b| {
        let query = make_search(&["[A-Z]+_RESUME"], GrepOptions::default());
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("alternation", |b| {
        let query = make_search(
            &["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT"],
            GrepOptions::default(),
        );
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("case_insensitive", |b| {
        let query = make_search(
            &["beta"],
            GrepOptions {
                case_mode: sift_core::grep::CaseMode::Insensitive,
                ..Default::default()
            },
        );
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("full_scan_fallback", |b| {
        let query = make_search(
            &[r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}"],
            GrepOptions::default(),
        );
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("invert_match", |b| {
        let query = make_search(
            &["beta"],
            GrepOptions {
                flags: GrepMatchFlags::INVERT_MATCH,
                ..Default::default()
            },
        );
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("indexed_search_with_stats", |b| {
        let query = make_search(&["beta"], GrepOptions::default());
        b.iter(|| {
            black_box(run_grep(
                &indexes,
                &filter,
                &query,
                GrepMode::Standard,
                GrepCollection::stats(),
            ))
        });
    });

    g.finish();
}

// ─── Walk search benches ─────────────────────────────────────────────────────

fn bench_walk_search(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    common::make_filter_corpus(&corpus);
    let filter = make_filter(&CandidateFilterConfig::default(), &corpus);
    let indexes = Indexes::open(&tmp.path().join(".sift")).unwrap();

    let mut g = c.benchmark_group("grep_walk");

    g.bench_function("literal", |b| {
        let query = make_search(&["beta"], GrepOptions::default());
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.bench_function("full_scan", |b| {
        let query = make_search(&[".*"], GrepOptions::default());
        b.iter(|| black_box(run_standard(&indexes, &filter, &query)));
    });

    g.finish();
}

// ─── Output mode benches ─────────────────────────────────────────────────────

fn bench_output_modes(c: &mut Criterion) {
    let fixture = common::open_large_index();
    let index = fixture.1;
    let indexes = wrap_index(index);
    let filter = make_filter(&CandidateFilterConfig::default(), indexes.root());
    let query = make_search(&["beta"], GrepOptions::default());

    let mut g = c.benchmark_group("grep_output_modes");

    g.bench_function("count", |b| {
        b.iter(|| {
            black_box(run_grep(
                &indexes,
                &filter,
                &query,
                GrepMode::Count,
                GrepCollection::none(),
            ))
        });
    });

    g.bench_function("files_with_matches", |b| {
        b.iter(|| {
            black_box(run_grep(
                &indexes,
                &filter,
                &query,
                GrepMode::FilesWithMatches,
                GrepCollection::none(),
            ));
        });
    });

    g.bench_function("files_without_match", |b| {
        b.iter(|| {
            black_box(run_grep(
                &indexes,
                &filter,
                &query,
                GrepMode::FilesWithoutMatch,
                GrepCollection::none(),
            ));
        });
    });

    g.finish();
}

criterion_group! {
    name = benches;
    config = sift_criterion();
    targets = bench_indexed_search, bench_walk_search, bench_output_modes,
}
criterion_main!(benches);
