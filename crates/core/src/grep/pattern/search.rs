use std::io;
use std::path::PathBuf;
use std::time::Instant;

use grep_matcher::{LineTerminator, Matcher as GrepMatcherTrait};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use rayon::prelude::*;

use crate::grep::input::{Input, Inputs};
use crate::grep::options::BinaryMode;
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
        match self {
            Self::Rust { matcher, .. } => match_input(matcher, input, searcher, sink),
            Self::Pcre2 { matcher, .. } => match_input(matcher, input, searcher, sink),
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

        let mut outcomes: Vec<FileOutcome> = inputs
            .as_slice()
            .par_iter()
            .map(|input| {
                let binary_detection = if query.opts.null_data() {
                    BinaryDetection::none()
                } else {
                    match query.opts.binary_mode {
                        BinaryMode::Quit
                            if matches!(
                                input,
                                Input::Path { explicit: true, .. }
                                    | Input::Bytes { explicit: true, .. }
                            ) =>
                        {
                            BinaryDetection::convert(b'\x00')
                        }
                        BinaryMode::Quit => BinaryDetection::quit(b'\x00'),
                        BinaryMode::Binary => BinaryDetection::convert(b'\x00'),
                        BinaryMode::AsText => BinaryDetection::none(),
                    }
                };
                let mut builder = SearcherBuilder::new();
                builder
                    .encoding(query.opts.input_encoding.explicit())
                    .bom_sniffing(query.opts.input_encoding.bom_sniffing())
                    .binary_detection(binary_detection)
                    .line_terminator(LineTerminator::byte(query.opts.line_terminator()))
                    .invert_match(query.opts.invert_match())
                    .line_number(true)
                    .max_matches(query.opts.max_results.map(|n| n as u64));
                if query.opts.multiline() {
                    builder.multi_line(true);
                }
                self.collect_input(&mut builder.build(), input, only_matching)
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
        match self {
            Self::Rust { matcher, .. } => collect_input(matcher, searcher, input, only_matching),
            Self::Pcre2 { matcher, .. } => collect_input(matcher, searcher, input, only_matching),
        }
    }
}

fn match_input<M: GrepMatcherTrait>(
    matcher: &M,
    input: &Input<'_>,
    searcher: &mut Searcher,
    sink: &mut impl Sink<Error = io::Error>,
) {
    match input {
        Input::Path { candidate, .. } => {
            let _ = searcher.search_path(matcher, candidate.abs_path(), sink);
        }
        Input::Bytes { bytes, .. } => {
            let _ = searcher.search_slice(matcher, bytes, sink);
        }
    }
}

fn collect_input<M: GrepMatcherTrait + Clone>(
    matcher: &M,
    searcher: &mut Searcher,
    input: &Input<'_>,
    only_matching: bool,
) -> FileOutcome {
    let (display_path, hit_path) = input.paths();
    let mut sink = MatchCollector {
        path: display_path,
        matcher: only_matching.then(|| matcher.clone()),
        matches: Vec::new(),
    };
    match_input(matcher, input, searcher, &mut sink);
    let has_matches = !sink.matches.is_empty();
    FileOutcome {
        matched: has_matches,
        matches: sink.matches,
        hit_path: has_matches.then_some(hit_path).flatten(),
    }
}

struct FileOutcome {
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
