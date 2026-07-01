use std::io;
use std::path::PathBuf;
use std::time::Instant;

use grep_matcher::Matcher as GrepMatcherTrait;
use grep_searcher::{Searcher, Sink, SinkMatch};
use rayon::prelude::*;

use crate::grep::engine::matcher::{Matcher, SearcherConfig};
use crate::grep::input::{Input, Inputs};
use crate::grep::pattern::{Match, Query};
use crate::grep::report::Report;
use crate::grep::stats::{Stats, StatsMode};

use super::compile::CompiledQuery;

impl CompiledQuery {
    /// Match one input, forwarding hits to a ripgrep sink.
    ///
    /// Used by custom output handlers (CLI printers). Library callers collecting
    /// matches into a [`Report`] should prefer [`Self::report`] or [`Query::search`].
    pub fn match_input(
        &self,
        input: &Input<'_>,
        searcher: &mut Searcher,
        sink: &mut impl Sink<Error = io::Error>,
    ) {
        let matcher = self.matcher();
        match input {
            Input::Path { candidate, .. } => {
                let _ = searcher.search_path(matcher, candidate.abs_path(), sink);
            }
            Input::Bytes { bytes, .. } => {
                let _ = searcher.search_slice(matcher, bytes, sink);
            }
        }
    }

    /// Collect matches for all inputs into a [`Report`].
    ///
    /// `query` supplies execution options (match limits, line collection mode).
    #[must_use]
    pub fn report(&self, query: &Query, inputs: &Inputs<'_>, stats: StatsMode) -> Report {
        if inputs.is_empty() {
            return Report {
                matched: false,
                matches: Vec::new(),
                hit_paths: Vec::new(),
                stats: stats.collect().then(Stats::default),
            };
        }

        let search_start = Instant::now();
        let only_matching = query.opts().only_matching();
        let searcher_config = SearcherConfig::match_collection(query.opts().max_results);

        let mut outcomes: Vec<FileOutcome> = inputs
            .as_slice()
            .par_iter()
            .map_init(
                || searcher_config.searcher(query),
                |searcher, input| self.collect_input(searcher, input, only_matching),
            )
            .collect();

        let mut line_matches = Vec::new();
        let mut hit_paths = Vec::new();
        let mut any_match = false;
        let mut match_count = 0usize;
        let mut files_with_matches = 0usize;

        for outcome in &mut outcomes {
            any_match |= outcome.matched;
            if outcome.matched {
                files_with_matches += 1;
                if let Some(path) = outcome.hit_path.take() {
                    hit_paths.push(path);
                }
            }
            match_count += outcome.matches.len();
            line_matches.append(&mut outcome.matches);
        }

        let report_stats = stats.collect().then(|| Stats {
            matches: match_count,
            files_with_matches,
            files_searched: inputs.len(),
            bytes_printed: 0,
            bytes_searched: inputs.byte_count(),
            elapsed: search_start.elapsed(),
        });

        Report {
            matched: any_match,
            matches: line_matches,
            hit_paths,
            stats: report_stats,
        }
    }

    fn collect_input(
        &self,
        searcher: &mut Searcher,
        input: &Input<'_>,
        only_matching: bool,
    ) -> FileOutcome {
        let (display_path, hit_path) = input.paths();
        let mut sink = MatchCollector {
            path: display_path,
            only_matching,
            matcher: self.matcher().clone(),
            matches: Vec::new(),
            matched: false,
        };
        self.match_input(input, searcher, &mut sink);
        FileOutcome {
            matched: sink.matched,
            matches: sink.matches,
            hit_path: sink.matched.then_some(hit_path).flatten(),
        }
    }
}

struct FileOutcome {
    matched: bool,
    matches: Vec<Match>,
    hit_path: Option<PathBuf>,
}

struct MatchCollector {
    path: PathBuf,
    only_matching: bool,
    matcher: Matcher,
    matches: Vec<Match>,
    matched: bool,
}

impl Sink for MatchCollector {
    type Error = io::Error;

    fn matched(&mut self, searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        self.matched = true;
        let line = usize::try_from(mat.line_number().unwrap_or(0)).unwrap_or(0);
        let line_bytes = mat.bytes();
        if self.only_matching {
            let _ = self
                .matcher
                .find_iter(line_bytes, |m: grep_matcher::Match| {
                    self.matches.push(Match {
                        file: self.path.clone(),
                        line,
                        text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                    });
                    true
                });
        } else {
            self.matches.push(Match {
                file: self.path.clone(),
                line,
                text: String::from_utf8_lossy(line_bytes).into_owned(),
            });
        }
        Ok(true)
    }
}
