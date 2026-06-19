#![no_main]

use libfuzzer_sys::fuzz_target;
use sift_core::grep::{GrepRequest, run as grep_run};
use sift_core::{
    CandidateFilter, CandidateFilterConfig, CorpusKind, CorpusSpec, IndexConfig, Indexes,
    IndexWalkOptions, PatternCompiler, SearchCollection, SearchOutput, SearchOutputFormat,
    SearchQuery, SearchSeparators, TrigramIndex, VisibilityConfig,
};
use std::fs;
use std::sync::OnceLock;

const MAX_PATTERN_LEN: usize = 512;

struct IndexHolder {
    temp: tempfile::TempDir,
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
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkOptions::new(false),
            visibility: VisibilityConfig::default(),
        };
        TrigramIndex::build(&config, &trigram_dir, &[]).expect("build_index");
        let indexes = Indexes::open(&sift_dir).expect("open_index");
        IndexHolder {
            temp: tmp,
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

fn opts_from_bytes(data: &[u8]) -> sift_core::SearchOptions {
    let flags = data
        .first()
        .map(|b| sift_core::SearchMatchFlags::from_bits_truncate(u16::from(*b)))
        .unwrap_or_default();
    let max_results = data.get(1).map(|b| (*b as usize).min(10_000));
    sift_core::SearchOptions {
        flags,
        max_results,
        ..sift_core::SearchOptions::default()
    }
}

fn run_search(
    indexes: &Indexes,
    patterns: &[String],
    opts: &sift_core::SearchOptions,
) {
    let Ok(q) = SearchQuery::new(patterns, opts.clone()) else {
        return;
    };
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), indexes.root()).unwrap();
    let _ = grep_run(&q, &GrepRequest {
        indexes,
        filter: &filter,
        output: SearchOutput {
            format: SearchOutputFormat::Text,
            emission: sift_core::OutputEmission::Quiet,
            ..SearchOutput::default()
        },
        separators: &SearchSeparators::default(),
        collect: SearchCollection::none(),
        store_meta: None,
        walk_unindexed: false,
    });
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

fn compile_with_flags(patterns: &[&str], opts: &sift_core::SearchOptions) -> Result<(), ()> {
    PatternCompiler::new()
        .fixed_strings(opts.fixed_strings())
        .word_regexp(opts.word_regexp())
        .line_regexp(opts.line_regexp())
        .case_insensitive(opts.case_insensitive())
        .compile(patterns)
        .map(|_| ())
        .map_err(|_| ())
}
