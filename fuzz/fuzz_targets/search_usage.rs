#![no_main]

use libfuzzer_sys::fuzz_target;
use sift_core::candidates::{
    CandidatePlanner, CandidateRequest, CandidateScope, CandidateSource, CandidateSpec, CorpusMode,
    IndexFallback,
};
use sift_core::grep::{CandidateFilter, CandidateFilterConfig, InputRequest, VisibilityConfig};
use sift_core::search::{
    InputExtent, SearchFlags, SearchOptions, SearchQueryBuilder, Searcher, StatsMode,
};
use sift_core::{
    CorpusKind, CorpusSpec, GramWidth, IndexBuildConfig, IndexWalkConfig, Indexes, NGramConfig,
};
use std::fs;
use std::sync::OnceLock;

const MAX_PATTERN_LEN: usize = 512;

struct IndexHolder {
    _temp: tempfile::TempDir,
    indexes: Indexes,
}

static INDEXES: OnceLock<IndexHolder> = OnceLock::new();

fn indexed() -> &'static Indexes {
    let holder = INDEXES.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("tempdir");
        let corpus = tmp.path().join("c");
        fs::create_dir_all(&corpus).expect("mkdir");
        fs::write(corpus.join("a.txt"), b"hello world\nfoo bar\n").expect("a.txt");
        fs::write(corpus.join("b.txt"), b"baz\nquux line\n").expect("b.txt");
        let sift_dir = tmp.path().join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        NGramConfig::new(GramWidth::TRIGRAM)
            .build(&config, &trigram_dir, &[])
            .expect("build_index");
        let indexes = Indexes::open(&sift_dir).expect("open_index");
        IndexHolder {
            _temp: tmp,
            indexes,
        }
    });
    &holder.indexes
}

fn lossy_pattern(data: &[u8]) -> String {
    String::from_utf8_lossy(data)
        .chars()
        .take(MAX_PATTERN_LEN)
        .collect()
}

fn opts_from_bytes(data: &[u8]) -> SearchOptions {
    let flags = data
        .first()
        .map(|b| SearchFlags::from_bits_truncate(u16::from(*b)))
        .unwrap_or_default();
    let max_results = data.get(1).map(|b| (*b as usize).min(10_000));
    SearchOptions {
        flags,
        max_results,
        ..SearchOptions::default()
    }
}

fn run_search(indexes: &Indexes, patterns: &[String], opts: &SearchOptions) {
    let Ok(query) = SearchQueryBuilder::new(patterns.to_vec())
        .options(opts.clone())
        .build()
    else {
        return;
    };
    let Ok(searcher) = Searcher::new(query.clone()) else {
        return;
    };
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), indexes.root()).unwrap();
    let source = CandidateSource {
        indexes,
        filter: &filter,
        store_meta: None,
    };
    let request = CandidateRequest {
        scope: CandidateScope::Indexed,
        corpus: CorpusMode::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: Default::default(),
    };
    let Ok(candidates) =
        CandidatePlanner::new(&source, CandidateSpec::from(&query), request).resolve()
    else {
        return;
    };
    let input_request = InputRequest::from_candidates();
    let Ok(inputs) = input_request.resolve(&candidates, InputExtent::Complete) else {
        return;
    };
    let _ = searcher.search(inputs, StatsMode::Off);
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let opts = opts_from_bytes(data);
    let indexes = indexed();

    let pat1 = lossy_pattern(&data[2..]);
    run_search(indexes, &[pat1], &opts);

    if data.len() > 4 {
        let mid = 2 + (data.len() - 2) / 2;
        let p_a = lossy_pattern(&data[2..mid]);
        let p_b = lossy_pattern(&data[mid..]);
        run_search(indexes, &[p_a, p_b], &opts);
    }

    let p = lossy_pattern(&data[2..]);
    let _ = compile_with_flags(&[&p], &opts);
    if data.len() > 4 {
        let mid = 2 + (data.len() - 2) / 2;
        let p_a = lossy_pattern(&data[2..mid]);
        let p_b = lossy_pattern(&data[mid..]);
        let _ = compile_with_flags(&[&p_a, &p_b], &opts);
    }
});

fn compile_with_flags(patterns: &[&str], opts: &SearchOptions) -> Result<(), ()> {
    let query = SearchQueryBuilder::new(
        patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
    )
    .options(opts.clone())
    .build()
    .map_err(|_| ())?;
    Searcher::new(query).map(|_| ()).map_err(|_| ())
}
