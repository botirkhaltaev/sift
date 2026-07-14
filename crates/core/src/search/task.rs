use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::{Captures, LineTerminator, Matcher as GrepMatcherTrait};
use grep_searcher::{
    BinaryDetection, Searcher as RegexSearcher, SearcherBuilder as RegexSearcherBuilder, Sink,
    SinkContext, SinkContextKind, SinkFinish, SinkMatch,
};

use crate::search::event::{
    BinaryEvent, ContextEvent, ContextKind, FileEvent, MatchEvent, SearchEvent,
};
use crate::search::hit::Match;
use crate::search::input::Input;
use crate::search::matcher::Matcher;
use crate::search::mode::SearchMode;
use crate::search::options::{BinaryMode, SearchOptions};
use crate::search::searcher::EventCollection;

pub(in crate::search) struct SearchTask<'searcher, 'input> {
    matcher: &'searcher Matcher,
    options: &'searcher SearchOptions,
    mode: SearchMode,
    events: EventCollection,
    input: &'input Input<'input>,
}

pub struct SearchOutcome {
    pub(crate) path: PathBuf,
    pub(crate) matched: bool,
    pub(crate) matches: Vec<Match>,
    events: Vec<SearchEvent>,
    pub(crate) line_matches: usize,
    pub(crate) match_spans: usize,
    pub(crate) bytes_searched: u64,
    pub(crate) binary_byte_offset: Option<u64>,
    pub(crate) hit_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchEmission {
    /// File presence only; no line text or span scan.
    Presence,
    /// Count matching lines without scanning match spans.
    LineCount,
    Lines,
    Spans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOrigin {
    Explicit,
    Discovered,
}

impl<'searcher, 'input> SearchTask<'searcher, 'input> {
    pub(in crate::search) const fn new(
        matcher: &'searcher Matcher,
        options: &'searcher SearchOptions,
        mode: SearchMode,
        events: EventCollection,
        input: &'input Input<'input>,
    ) -> Self {
        Self {
            matcher,
            options,
            mode,
            events,
            input,
        }
    }

    pub(in crate::search) fn execute(self, grep: &mut RegexSearcher) -> SearchOutcome {
        let origin = InputOrigin::from(self.input);
        match origin {
            InputOrigin::Discovered => match self.matcher {
                Matcher::Rust(matcher) => self.search_with_matcher(grep, matcher, origin),
                Matcher::Pcre2(matcher) => self.search_with_matcher(grep, matcher, origin),
            },
            InputOrigin::Explicit => {
                let mut explicit = Self::searcher(self.options, self.mode, origin);
                match self.matcher {
                    Matcher::Rust(matcher) => {
                        self.search_with_matcher(&mut explicit, matcher, origin)
                    }
                    Matcher::Pcre2(matcher) => {
                        self.search_with_matcher(&mut explicit, matcher, origin)
                    }
                }
            }
        }
    }

    pub(in crate::search) fn discovered_searcher(
        options: &SearchOptions,
        mode: SearchMode,
    ) -> RegexSearcher {
        Self::searcher(options, mode, InputOrigin::Discovered)
    }

    fn searcher(options: &SearchOptions, mode: SearchMode, origin: InputOrigin) -> RegexSearcher {
        let mut builder = RegexSearcherBuilder::new();
        builder
            .encoding(options.input_encoding.explicit())
            .bom_sniffing(options.input_encoding.bom_sniffing())
            .binary_detection(Self::binary_detection_for(options, origin))
            .line_terminator(LineTerminator::byte(options.line_terminator()))
            .invert_match(options.invert_match())
            .line_number(!matches!(
                mode,
                SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch
            ))
            .max_matches(Self::match_limit_for(mode, options));
        builder.before_context(options.before_context);
        builder.after_context(options.after_context);
        if options.multiline() {
            builder.multi_line(true);
        }
        builder.build()
    }

    fn search_with_matcher<M: GrepMatcherTrait>(
        &self,
        grep: &mut RegexSearcher,
        grep_matcher: &M,
        origin: InputOrigin,
    ) -> SearchOutcome {
        let match_emission = MatchEmission::from(self.mode, self.options);
        // Discard never emits path-bearing MatchEvents; line hits nest under FileReport.
        let sink_path = match self.events {
            EventCollection::Discard => PathBuf::new(),
            EventCollection::Collect => self.input.display_path().to_path_buf(),
        };
        let mut sink = MatchSink {
            path: sink_path,
            origin,
            matcher: grep_matcher,
            replacement: self
                .options
                .replace
                .as_deref()
                .map(str::as_bytes)
                .map(<[u8]>::to_vec),
            match_emission,
            event_collection: self.events,
            line_matches: 0,
            match_spans: 0,
            bytes_searched: 0,
            binary_byte_offset: None,
            matches: Vec::new(),
            events: Vec::new(),
        };
        match self.input {
            Input::Path { path, .. } => {
                let _ = grep.search_path(grep_matcher, path, &mut sink);
            }
            Input::Bytes { bytes, .. } => {
                let _ = grep.search_slice(grep_matcher, bytes, &mut sink);
            }
        }
        let has_match = sink.line_matches > 0;
        SearchOutcome {
            path: self.input.display_path().to_path_buf(),
            matched: has_match,
            matches: sink.matches,
            events: sink.events,
            line_matches: sink.line_matches,
            match_spans: sink.match_spans,
            bytes_searched: sink.bytes_searched,
            binary_byte_offset: sink.binary_byte_offset,
            hit_path: has_match
                .then(|| self.input.hit_path().map(Path::to_path_buf))
                .flatten(),
        }
    }

    fn match_limit_for(mode: SearchMode, options: &SearchOptions) -> Option<u64> {
        match mode {
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch => Some(1),
            SearchMode::Lines
            | SearchMode::Matches
            | SearchMode::CountLines { .. }
            | SearchMode::CountMatches { .. } => options.max_results.map(|n| n as u64),
        }
    }

    fn binary_detection_for(options: &SearchOptions, origin: InputOrigin) -> BinaryDetection {
        if options.null_data() {
            return BinaryDetection::none();
        }
        match (options.binary_mode, origin) {
            (BinaryMode::Quit, InputOrigin::Explicit) | (BinaryMode::Binary, _) => {
                BinaryDetection::convert(b'\x00')
            }
            (BinaryMode::Quit, InputOrigin::Discovered) => BinaryDetection::quit(b'\x00'),
            (BinaryMode::AsText, _) => BinaryDetection::none(),
        }
    }
}

impl SearchOutcome {
    pub(in crate::search) fn drain_events(&mut self) -> impl Iterator<Item = SearchEvent> + '_ {
        self.events.drain(..)
    }
}

impl MatchEmission {
    const fn from(mode: SearchMode, options: &SearchOptions) -> Self {
        if options.replace.is_some() {
            return if matches!(mode, SearchMode::Matches) || options.only_matching() {
                Self::Spans
            } else {
                Self::Lines
            };
        }
        match mode {
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch => Self::Presence,
            SearchMode::CountLines { .. } => Self::LineCount,
            SearchMode::CountMatches { .. } | SearchMode::Matches => Self::Spans,
            SearchMode::Lines if options.only_matching() => Self::Spans,
            SearchMode::Lines => Self::Lines,
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

struct MatchSink<'a, M> {
    path: PathBuf,
    origin: InputOrigin,
    matcher: &'a M,
    replacement: Option<Vec<u8>>,
    match_emission: MatchEmission,
    event_collection: EventCollection,
    line_matches: usize,
    match_spans: usize,
    bytes_searched: u64,
    binary_byte_offset: Option<u64>,
    matches: Vec<Match>,
    events: Vec<SearchEvent>,
}

impl<M: GrepMatcherTrait> MatchSink<'_, M> {
    fn count_spans(&mut self, line_bytes: &[u8]) {
        let _ = self
            .matcher
            .find_iter(line_bytes, |_m: grep_matcher::Match| {
                self.match_spans += 1;
                true
            });
    }

    fn collect_spans(&mut self, mat: &SinkMatch<'_>, line: usize, line_bytes: &[u8]) {
        let replacement = self.replacement.as_deref().and_then(|replacement| {
            Replacement::expand(self.matcher, line_bytes, replacement).ok()
        });
        let mut ranges = Vec::new();
        let _ = self
            .matcher
            .find_iter(line_bytes, |m: grep_matcher::Match| {
                ranges.push(m.start()..m.end());
                self.match_spans += 1;
                self.matches.push(Match {
                    line,
                    text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                });
                true
            });
        if matches!(self.event_collection, EventCollection::Collect) {
            self.events.push(SearchEvent::Match(MatchEvent {
                path: self.path.clone(),
                line_number: mat.line_number(),
                absolute_byte_offset: Some(mat.absolute_byte_offset()),
                bytes: line_bytes.to_vec(),
                ranges,
                replacement: replacement
                    .as_ref()
                    .map(|replacement| replacement.line.clone()),
                replacement_matches: replacement.map_or_else(Vec::new, |r| r.matches),
            }));
        }
    }

    fn record_match(&mut self, mat: &SinkMatch<'_>, line: usize, line_bytes: &[u8]) -> bool {
        match self.match_emission {
            MatchEmission::Presence | MatchEmission::LineCount => {
                if matches!(self.event_collection, EventCollection::Collect) {
                    self.events.push(SearchEvent::Match(MatchEvent {
                        path: self.path.clone(),
                        line_number: mat.line_number(),
                        absolute_byte_offset: Some(mat.absolute_byte_offset()),
                        bytes: Vec::new(),
                        ranges: Vec::new(),
                        replacement: None,
                        replacement_matches: Vec::new(),
                    }));
                }
            }
            MatchEmission::Lines
                if self.replacement.is_none()
                    && matches!(self.event_collection, EventCollection::Discard) =>
            {
                self.matches.push(Match {
                    line,
                    text: String::from_utf8_lossy(line_bytes).into_owned(),
                });
            }
            MatchEmission::Lines => {
                let replacement = self.replacement.as_deref().and_then(|replacement| {
                    Replacement::expand(self.matcher, line_bytes, replacement).ok()
                });
                let mut ranges = Vec::new();
                let _ = self
                    .matcher
                    .find_iter(line_bytes, |m: grep_matcher::Match| {
                        ranges.push(m.start()..m.end());
                        self.match_spans += 1;
                        true
                    });
                self.matches.push(Match {
                    line,
                    text: String::from_utf8_lossy(line_bytes).into_owned(),
                });
                if matches!(self.event_collection, EventCollection::Collect) {
                    self.events.push(SearchEvent::Match(MatchEvent {
                        path: self.path.clone(),
                        line_number: mat.line_number(),
                        absolute_byte_offset: Some(mat.absolute_byte_offset()),
                        bytes: line_bytes.to_vec(),
                        ranges,
                        replacement: replacement
                            .as_ref()
                            .map(|replacement| replacement.line.clone()),
                        replacement_matches: replacement.map_or_else(Vec::new, |r| r.matches),
                    }));
                }
            }
            MatchEmission::Spans if matches!(self.event_collection, EventCollection::Discard) => {
                self.count_spans(line_bytes);
            }
            MatchEmission::Spans => self.collect_spans(mat, line, line_bytes),
        }
        true
    }
}

impl<M: GrepMatcherTrait> Sink for MatchSink<'_, M> {
    type Error = io::Error;

    fn begin(&mut self, _searcher: &RegexSearcher) -> Result<bool, Self::Error> {
        match self.event_collection {
            EventCollection::Discard => {}
            EventCollection::Collect => self.events.push(SearchEvent::Begin(FileEvent {
                path: self.path.clone(),
            })),
        }
        Ok(true)
    }

    fn matched(
        &mut self,
        _searcher: &RegexSearcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let line = usize::try_from(mat.line_number().unwrap_or(0)).unwrap_or(0);
        let line_bytes = mat.bytes();
        self.line_matches += 1;
        Ok(self.record_match(mat, line, line_bytes))
    }

    fn context(
        &mut self,
        _searcher: &RegexSearcher,
        context: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        match self.event_collection {
            EventCollection::Discard => {}
            EventCollection::Collect => self.events.push(SearchEvent::Context(ContextEvent {
                path: self.path.clone(),
                kind: ContextKind::from(context.kind()),
                line_number: context.line_number(),
                absolute_byte_offset: context.absolute_byte_offset(),
                bytes: context.bytes().to_vec(),
            })),
        }
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &RegexSearcher) -> Result<bool, Self::Error> {
        match self.event_collection {
            EventCollection::Discard => {}
            EventCollection::Collect => self.events.push(SearchEvent::ContextBreak),
        }
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _searcher: &RegexSearcher,
        binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary_byte_offset.get_or_insert(binary_byte_offset);
        match self.event_collection {
            EventCollection::Discard => {}
            EventCollection::Collect => self.events.push(SearchEvent::Binary(BinaryEvent {
                path: self.path.clone(),
                absolute_byte_offset: binary_byte_offset,
                explicit: matches!(self.origin, InputOrigin::Explicit),
            })),
        }
        Ok(true)
    }

    fn finish(
        &mut self,
        _searcher: &RegexSearcher,
        finish: &SinkFinish,
    ) -> Result<(), Self::Error> {
        self.bytes_searched = finish.byte_count();
        if self.binary_byte_offset.is_none() {
            self.binary_byte_offset = finish.binary_byte_offset();
        }
        match self.event_collection {
            EventCollection::Discard => {}
            EventCollection::Collect => self.events.push(SearchEvent::End(FileEvent {
                path: self.path.clone(),
            })),
        }
        Ok(())
    }
}

struct Replacement {
    line: Vec<u8>,
    matches: Vec<Vec<u8>>,
}

impl Replacement {
    fn expand<M: GrepMatcherTrait>(
        matcher: &M,
        bytes: &[u8],
        replacement: &[u8],
    ) -> Result<Self, M::Error> {
        let mut caps = matcher.new_captures()?;
        let mut line = Vec::new();
        let mut spans = Vec::new();
        matcher.replace_with_captures(bytes, &mut caps, &mut line, |captures, dst| {
            let start = dst.len();
            captures.interpolate(|name| matcher.capture_index(name), bytes, replacement, dst);
            spans.push(dst[start..].to_vec());
            true
        })?;
        Ok(Self {
            line,
            matches: spans,
        })
    }
}

impl From<&SinkContextKind> for ContextKind {
    fn from(kind: &SinkContextKind) -> Self {
        match kind {
            SinkContextKind::Before => Self::Before,
            SinkContextKind::After => Self::After,
            SinkContextKind::Other => Self::Other,
        }
    }
}
