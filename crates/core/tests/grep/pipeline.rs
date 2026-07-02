use std::borrow::Cow;

use sift_core::candidates::{
    CandidatePlanner, CandidateRequest, CandidateScope, CandidateSelection, CandidateSource,
    CandidateSpec, CorpusMode, IndexFallback,
};
use sift_core::grep::{
    CandidateFilter, CandidateFilterConfig, CandidateOrder, Grep, GrepRequest, InputRequest,
};
use sift_core::search::{
    InputIdentity, Inputs, SearchEvent, SearchMode, SearchOptions, SearchQueryBuilder, SearchSink,
    Searcher, StatsMode,
};
use tempfile::TempDir;

use super::common::{make_parity_corpus, open_indexes};

#[test]
fn grep_finds_match_in_indexed_corpus() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let query = SearchQueryBuilder::new(vec!["beta".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");
    let searcher = Searcher::new(query.clone()).expect("searcher");

    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };
    let request = CandidateRequest {
        scope: CandidateScope::Indexed,
        corpus: CorpusMode::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: CandidateOrder::default(),
    };
    let candidates = CandidatePlanner::new(&source, CandidateSpec::from(&query), request)
        .resolve()
        .expect("candidates");
    let input_request = InputRequest::from_candidates();
    let inputs = input_request.resolve(&candidates).expect("inputs");

    let report = searcher.search(&inputs, StatsMode::Off).expect("grep run");
    assert!(report.matched());
}

#[test]
fn high_level_grep_search_resolves_candidates_and_reports_matches() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["beta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            candidates: CandidateSelection::Corpus {
                corpus: CorpusMode::Indexed,
                fallback: IndexFallback::IndexHitsOnly,
                order: CandidateOrder::default(),
            },
            inputs: InputRequest::from_candidates(),
            mode: SearchMode::Lines,
            stats: StatsMode::On,
        })
        .expect("grep search");

    assert!(report.matched);
    assert!(report.selected);
    assert!(!report.matches.is_empty());
    assert!(!report.hit_paths.is_empty());
    assert!(report.stats.is_some());
}

#[test]
fn high_level_grep_stream_emits_events_without_collecting_matches() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };
    let mut sink = EventRecorder::default();

    let report = Grep::new(source)
        .stream(
            GrepRequest {
                query: SearchQueryBuilder::new(vec!["beta".to_string()])
                    .options(SearchOptions::default())
                    .build()
                    .expect("query"),
                candidates: CandidateSelection::Corpus {
                    corpus: CorpusMode::Indexed,
                    fallback: IndexFallback::IndexHitsOnly,
                    order: CandidateOrder::default(),
                },
                inputs: InputRequest::from_candidates(),
                mode: SearchMode::Lines,
                stats: StatsMode::Off,
            },
            &mut sink,
        )
        .expect("grep stream");

    assert!(report.matched);
    assert!(!report.matches.is_empty());
    assert!(sink.matches > 0);
}

#[test]
fn high_level_grep_files_without_match_selects_nonmatching_files() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["beta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            candidates: CandidateSelection::Corpus {
                corpus: CorpusMode::Indexed,
                fallback: IndexFallback::IndexHitsOnly,
                order: CandidateOrder::default(),
            },
            inputs: InputRequest::from_candidates(),
            mode: SearchMode::FilesWithoutMatch,
            stats: StatsMode::Off,
        })
        .expect("grep search");

    assert!(report.matched);
    assert!(report.selected);
    assert!(
        report
            .files
            .iter()
            .any(|file| file.selected && !file.matched)
    );
}

#[test]
fn high_level_grep_files_without_match_is_not_selected_when_all_files_match() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource {
        indexes: &indexes,
        filter: &filter,
        store_meta: None,
    };

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["alpha|beta|gamma|delta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            candidates: CandidateSelection::Corpus {
                corpus: CorpusMode::Indexed,
                fallback: IndexFallback::IndexHitsOnly,
                order: CandidateOrder::default(),
            },
            inputs: InputRequest::from_candidates(),
            mode: SearchMode::FilesWithoutMatch,
            stats: StatsMode::Off,
        })
        .expect("grep search");

    assert!(report.matched);
    assert!(!report.selected);
    assert!(
        report
            .files
            .iter()
            .all(|file| file.matched && !file.selected)
    );
}

#[derive(Default)]
struct EventRecorder {
    matches: usize,
}

impl SearchSink for EventRecorder {
    fn event(&mut self, event: SearchEvent) -> sift_core::Result<()> {
        if let SearchEvent::Match(event) = event {
            self.matches += 1;
            assert!(!event.bytes.is_empty());
            assert!(event.absolute_byte_offset.is_some());
        }
        Ok(())
    }
}

#[test]
fn grep_finds_match_in_stdin_stream() {
    let query = SearchQueryBuilder::new(vec!["needle".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");
    let query = Searcher::new(query).expect("searcher");

    let mut inputs = Inputs::with_capacity(1);
    inputs.push_bytes(
        Cow::Borrowed("<stdin>"),
        Cow::Borrowed(b"hello needle world\n"),
        InputIdentity::from_name("<stdin>"),
    );

    let report = query.search(&inputs, StatsMode::Off).expect("grep run");
    assert!(report.matched());
    assert_eq!(report.matches.len(), 1);
    assert!(report.matches[0].text.contains("needle"));
}
