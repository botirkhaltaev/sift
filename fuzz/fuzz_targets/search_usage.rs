#![no_main]

use libfuzzer_sys::fuzz_target;
use sift_core::grep::{CandidateIndexState, Grep, GrepCorpus, GrepQuery};
use sift_core::grep::{
    CandidateFilter, CandidateFilterConfig, OutputEmission, PatternCompiler, GrepCollection,
    GrepMatchFlags, GrepOptions, GrepOutput, GrepOutputFormat, GrepSeparators,
    VisibilityConfig,
};
use sift_core::{
    CorpusKind, CorpusSpec, GramWidth, IndexBuildConfig, IndexWalkConfig, Indexes, NGramConfig,
    SnapshotValidation,
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

fn opts_from_bytes(data: &[u8]) -> GrepOptions {
    let flags = data
        .first()
        .map(|b| GrepMatchFlags::from_bits_truncate(u16::from(*b)))
        .unwrap_or_default();
    let max_results = data.get(1).map(|b| (*b as usize).min(10_000));
    GrepOptions {
        flags,
        max_results,
        ..GrepOptions::default()
    }
}

fn run_search(indexes: &Indexes, patterns: &[String], opts: &GrepOptions) {
    let Ok(query) = GrepQuery::new(patterns.to_vec()) else {
        return;
    };
    let query = query.options(opts.clone());
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), indexes.root()).unwrap();
    let corpus = GrepCorpus::new(
        indexes,
        &filter,
        CandidateIndexState {
            store_meta: None,
            snapshot: SnapshotValidation::Unvalidated,
        },
    );
    let _ = Grep::new(query)
        .corpus(corpus)
        .output(GrepOutput {
            format: GrepOutputFormat::Text,
            emission: OutputEmission::Quiet,
            ..GrepOutput::default()
        })
        .separators(&GrepSeparators::default())
        .collect(GrepCollection::none())
        .run();
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

fn compile_with_flags(patterns: &[&str], opts: &GrepOptions) -> Result<(), ()> {
    PatternCompiler::new()
        .fixed_strings(opts.fixed_strings())
        .word_regexp(opts.word_regexp())
        .line_regexp(opts.line_regexp())
        .case_insensitive(opts.case_insensitive())
        .compile(patterns)
        .map(|_| ())
        .map_err(|_| ())
}
