use std::borrow::Cow;
use std::fs;

use sift_core::candidates::{CandidateSource, IndexNarrowing, ScanScope, SnapshotFreshness};
use sift_core::grep::{
    ByteInput, CandidateFilter, CandidateFilterConfig, CandidateOrder, Grep, GrepRequest,
    PathDisplay,
};
use sift_core::search::{
    InputConversion, Inputs, SearchEvent, SearchMode, SearchOptions, SearchQueryBuilder,
    SearchSink, Searcher, StatsMode,
};
use tempfile::TempDir;

use super::common::{make_parity_corpus, open_indexes};

const fn index_scope(order: CandidateOrder) -> ScanScope {
    ScanScope::Index {
        order,
        freshness: SnapshotFreshness::Current,
    }
}

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
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );
    let request = GrepRequest {
        query: query.clone(),
        streams: Inputs::empty(),
        conversion: InputConversion::new(&[], PathDisplay::Relative, None),
        mode: SearchMode::Lines,
        stats: StatsMode::Off,
    };
    let grep = Grep::new(source);
    let candidates = grep.resolve_candidates(&request).expect("candidates");
    let searcher = Searcher::new(query).expect("searcher");
    let inputs = sift_core::search::SearchInputs {
        candidates,
        streams: Inputs::empty(),
        conversion: InputConversion::new(&[], PathDisplay::Relative, None),
    };

    let report = searcher.search(inputs, StatsMode::Off).expect("grep run");
    assert!(report.matched());
}

#[test]
fn candidate_planner_all_indexed_uses_index_when_metadata_missing() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let query = SearchQueryBuilder::new(vec!["alpha|beta|gamma|delta".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );
    let request = GrepRequest {
        query,
        streams: Inputs::empty(),
        conversion: InputConversion::new(&[], PathDisplay::Relative, None),
        mode: SearchMode::Lines,
        stats: StatsMode::Off,
    };

    let grep = Grep::new(source);
    let candidates = grep.resolve_candidates(&request).expect("candidates");

    assert_eq!(candidates.into_vec().len(), 2);
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
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["beta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            streams: Inputs::empty(),
            conversion: InputConversion::new(&[], PathDisplay::Relative, None),
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
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );
    let mut sink = EventRecorder::default();

    let report = Grep::new(source)
        .stream(
            GrepRequest {
                query: SearchQueryBuilder::new(vec!["beta".to_string()])
                    .options(SearchOptions::default())
                    .build()
                    .expect("query"),
                streams: Inputs::empty(),
                conversion: InputConversion::new(&[], PathDisplay::Relative, None),
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
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        ScanScope::Walk {
            order: CandidateOrder::default(),
        },
        IndexNarrowing::Allowed,
    );

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["beta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            streams: Inputs::empty(),
            conversion: InputConversion::new(&[], PathDisplay::Relative, None),
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
fn high_level_grep_files_without_match_uses_full_corpus_with_index() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.txt"), "hello\n").expect("write a");
    fs::write(corpus.join("b.txt"), "goodbye\n").expect("write b");

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["hello".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            streams: Inputs::empty(),
            conversion: InputConversion::new(&[], PathDisplay::Relative, None),
            mode: SearchMode::FilesWithoutMatch,
            stats: StatsMode::Off,
        })
        .expect("grep search");

    assert!(report.matched);
    assert!(report.selected);
    assert_eq!(report.files.iter().filter(|file| file.selected).count(), 1);
    assert!(
        report
            .files
            .iter()
            .any(|file| file.selected && file.path.ends_with("b.txt"))
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
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        index_scope(CandidateOrder::default()),
        IndexNarrowing::Allowed,
    );

    let report = Grep::new(source)
        .search(GrepRequest {
            query: SearchQueryBuilder::new(vec!["alpha|beta|gamma|delta".to_string()])
                .options(SearchOptions::default())
                .build()
                .expect("query"),
            streams: Inputs::empty(),
            conversion: InputConversion::new(&[], PathDisplay::Relative, None),
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
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");

    let query = SearchQueryBuilder::new(vec!["needle".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");

    let indexes = open_indexes(&tmp.path().join(".sift"));
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let source = CandidateSource::new(
        &indexes,
        &filter,
        None,
        ScanScope::StreamsOnly,
        IndexNarrowing::Allowed,
    );
    let request = GrepRequest {
        query: query.clone(),
        streams: Inputs::empty(),
        conversion: InputConversion::new(&[], PathDisplay::Relative, None),
        mode: SearchMode::Lines,
        stats: StatsMode::Off,
    };
    let grep = Grep::new(source);
    let candidates = grep.resolve_candidates(&request).expect("candidates");
    let searcher = Searcher::new(query).expect("searcher");

    let streams = Inputs::empty().with_stream(ByteInput {
        path: Cow::Borrowed("<stdin>"),
        bytes: Cow::Borrowed(b"hello needle world\n"),
        explicit: false,
    });
    let inputs = sift_core::search::SearchInputs {
        candidates,
        streams,
        conversion: InputConversion::new(&[], PathDisplay::Relative, None),
    };

    let report = searcher.search(inputs, StatsMode::Off).expect("grep run");
    assert!(report.matched());
    assert_eq!(report.matches.len(), 1);
    assert!(report.matches[0].text.contains("needle"));
}
