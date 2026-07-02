use std::io;
use std::path::PathBuf;
use std::time::Instant;

use grep_matcher::{LineTerminator, Matcher as GrepMatcherTrait};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use rayon::prelude::*;

use crate::grep::compiled::CompiledQuery;
use crate::grep::input::{Input, Inputs};
use crate::grep::matched::Match;
use crate::grep::options::{BinaryMode, MatchOptions};
use crate::grep::query::Query;
use crate::grep::report::Report;
use crate::grep::stats::{Stats, StatsMode};

pub struct ReportCollector<'query, 'input> {
    pub query: &'query Query,
    pub compiled: &'query CompiledQuery,
    pub inputs: &'query Inputs<'input>,
    pub stats: StatsMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOrigin {
    Explicit,
    Discovered,
}

struct SearchExecution<'a> {
    opts: &'a MatchOptions,
    origin: InputOrigin,
}

impl ReportCollector<'_, '_> {
    #[must_use]
    pub fn collect(&self) -> Report {
        if self.inputs.is_empty() {
            return Report {
                matched: false,
                matches: Vec::new(),
                hit_paths: Vec::new(),
                stats: self.stats.collect().then(Stats::default),
            };
        }

        let search_start = Instant::now();
        let only_matching = self.query.opts().only_matching();

        let mut outcomes: Vec<InputOutcome> = self
            .inputs
            .as_slice()
            .par_iter()
            .map(|input| {
                let execution = SearchExecution {
                    opts: self.query.opts(),
                    origin: InputOrigin::from(input),
                };
                self.collect_input(&mut execution.searcher(), input, only_matching)
            })
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

        let report_stats = self.stats.collect().then(|| Stats {
            matches: match_count,
            files_with_matches,
            files_searched: self.inputs.len(),
            bytes_printed: 0,
            bytes_searched: self.inputs.byte_count(),
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
    ) -> InputOutcome {
        match self.compiled {
            CompiledQuery::Rust { matcher, .. } => {
                InputCollector::new(matcher, input, only_matching).collect(searcher)
            }
            CompiledQuery::Pcre2 { matcher, .. } => {
                InputCollector::new(matcher, input, only_matching).collect(searcher)
            }
        }
    }
}

impl SearchExecution<'_> {
    fn searcher(&self) -> Searcher {
        let mut builder = SearcherBuilder::new();
        builder
            .encoding(self.opts.input_encoding.explicit())
            .bom_sniffing(self.opts.input_encoding.bom_sniffing())
            .binary_detection(self.binary_detection())
            .line_terminator(LineTerminator::byte(self.opts.line_terminator()))
            .invert_match(self.opts.invert_match())
            .line_number(true)
            .max_matches(self.opts.max_results.map(|n| n as u64));
        if self.opts.multiline() {
            builder.multi_line(true);
        }
        builder.build()
    }

    fn binary_detection(&self) -> BinaryDetection {
        if self.opts.null_data() {
            return BinaryDetection::none();
        }
        match (self.opts.binary_mode, self.origin) {
            (BinaryMode::Quit, InputOrigin::Explicit) | (BinaryMode::Binary, _) => {
                BinaryDetection::convert(b'\x00')
            }
            (BinaryMode::Quit, InputOrigin::Discovered) => BinaryDetection::quit(b'\x00'),
            (BinaryMode::AsText, _) => BinaryDetection::none(),
        }
    }
}

impl From<&Input<'_>> for InputOrigin {
    fn from(input: &Input<'_>) -> Self {
        match input {
            Input::Path { explicit: true, .. } | Input::Bytes { explicit: true, .. } => {
                Self::Explicit
            }
            Input::Path {
                explicit: false, ..
            }
            | Input::Bytes {
                explicit: false, ..
            } => Self::Discovered,
        }
    }
}

struct InputCollector<'a, M> {
    matcher: &'a M,
    input: &'a Input<'a>,
    only_matching: bool,
}

impl<'a, M: GrepMatcherTrait + Clone> InputCollector<'a, M> {
    const fn new(matcher: &'a M, input: &'a Input<'a>, only_matching: bool) -> Self {
        Self {
            matcher,
            input,
            only_matching,
        }
    }

    fn collect(self, searcher: &mut Searcher) -> InputOutcome {
        let (display_path, hit_path) = self.input.paths();
        let mut sink = MatchCollector {
            path: display_path,
            matcher: self.only_matching.then(|| self.matcher.clone()),
            matches: Vec::new(),
        };
        self.search(searcher, &mut sink);
        let has_matches = !sink.matches.is_empty();
        InputOutcome {
            matched: has_matches,
            matches: sink.matches,
            hit_path: has_matches.then_some(hit_path).flatten(),
        }
    }

    fn search(&self, searcher: &mut Searcher, sink: &mut impl Sink<Error = io::Error>) {
        match self.input {
            Input::Path { candidate, .. } => {
                let _ = searcher.search_path(self.matcher, candidate.abs_path(), sink);
            }
            Input::Bytes { bytes, .. } => {
                let _ = searcher.search_slice(self.matcher, bytes, sink);
            }
        }
    }
}

struct InputOutcome {
    matched: bool,
    matches: Vec<Match>,
    hit_path: Option<PathBuf>,
}

struct MatchCollector<M> {
    path: PathBuf,
    matcher: Option<M>,
    matches: Vec<Match>,
}

impl<M: GrepMatcherTrait> Sink for MatchCollector<M> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line = usize::try_from(mat.line_number().unwrap_or(0)).unwrap_or(0);
        let line_bytes = mat.bytes();
        if let Some(matcher) = self.matcher.as_ref() {
            let _ = matcher.find_iter(line_bytes, |m: grep_matcher::Match| {
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
