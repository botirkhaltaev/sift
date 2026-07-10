use std::io;
use std::path::PathBuf;

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

pub(in crate::search) struct SearchTask<'a> {
    matcher: &'a Matcher,
    options: &'a SearchOptions,
    mode: SearchMode,
    events: EventCollection,
    input: &'a Input<'a>,
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
    /// `-l` / `-L`: file presence only.
    Presence,
    /// `-c` line counts: count matching lines, no span scan.
    LineCount,
    Lines,
    Spans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOrigin {
    Explicit,
    Discovered,
}

impl<'a> SearchTask<'a> {
    pub(in crate::search) const fn new(
        matcher: &'a Matcher,
        options: &'a SearchOptions,
        mode: SearchMode,
        events: EventCollection,
        input: &'a Input<'a>,
    ) -> Self {
        Self {
            matcher,
            options,
            mode,
            events,
            input,
        }
    }

    pub(in crate::search) fn execute(self) -> SearchOutcome {
        let origin = InputOrigin::from(self.input);
        let mut grep = self.grep_searcher(origin);
        match self.matcher {
            Matcher::Rust(matcher) => self.search_with_matcher(&mut grep, matcher, origin),
            Matcher::Pcre2(matcher) => self.search_with_matcher(&mut grep, matcher, origin),
        }
    }

    fn search_with_matcher<M: GrepMatcherTrait>(
        &self,
        grep: &mut RegexSearcher,
        grep_matcher: &M,
        origin: InputOrigin,
    ) -> SearchOutcome {
        let (display_path, hit_path) = self.input.paths();
        let mut sink = MatchSink {
            path: display_path.clone(),
            origin,
            matcher: grep_matcher,
            replacement: self
                .options
                .replace
                .as_deref()
                .map(str::as_bytes)
                .map(<[u8]>::to_vec),
            match_emission: MatchEmission::from(self.mode, self.options),
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
            path: display_path,
            matched: has_match,
            matches: sink.matches,
            events: sink.events,
            line_matches: sink.line_matches,
            match_spans: sink.match_spans,
            bytes_searched: sink.bytes_searched,
            binary_byte_offset: sink.binary_byte_offset,
            hit_path: has_match.then_some(hit_path).flatten(),
        }
    }

    fn grep_searcher(&self, origin: InputOrigin) -> RegexSearcher {
        let mut builder = RegexSearcherBuilder::new();
        builder
            .encoding(self.options.input_encoding.explicit())
            .bom_sniffing(self.options.input_encoding.bom_sniffing())
            .binary_detection(self.binary_detection(origin))
            .line_terminator(LineTerminator::byte(self.options.line_terminator()))
            .invert_match(self.options.invert_match())
            .line_number(!matches!(
                self.mode,
                SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch
            ))
            .max_matches(self.match_limit());
        builder.before_context(self.options.before_context);
        builder.after_context(self.options.after_context);
        if self.options.multiline() {
            builder.multi_line(true);
        }
        builder.build()
    }

    fn match_limit(&self) -> Option<u64> {
        match self.mode {
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch => Some(1),
            SearchMode::Lines
            | SearchMode::Matches
            | SearchMode::CountLines { .. }
            | SearchMode::CountMatches { .. } => self.options.max_results.map(|n| n as u64),
        }
    }

    fn binary_detection(&self, origin: InputOrigin) -> BinaryDetection {
        if self.options.null_data() {
            return BinaryDetection::none();
        }
        match (self.options.binary_mode, origin) {
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
            SearchMode::Lines => {
                if options.only_matching() {
                    Self::Spans
                } else {
                    Self::Lines
                }
            }
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

impl<M: GrepMatcherTrait> Sink for MatchSink<'_, M> {
    type Error = io::Error;

    fn begin(&mut self, _searcher: &RegexSearcher) -> Result<bool, Self::Error> {
        self.event_collection.push(
            &mut self.events,
            SearchEvent::Begin(FileEvent {
                path: self.path.clone(),
            }),
        );
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

        if matches!(
            self.match_emission,
            MatchEmission::Presence | MatchEmission::LineCount
        ) {
            self.event_collection.push(
                &mut self.events,
                SearchEvent::Match(MatchEvent {
                    path: self.path.clone(),
                    line_number: mat.line_number(),
                    absolute_byte_offset: Some(mat.absolute_byte_offset()),
                    bytes: Vec::new(),
                    ranges: Vec::new(),
                    replacement: None,
                    replacement_matches: Vec::new(),
                }),
            );
            return Ok(true);
        }

        let scan_spans = self.replacement.is_some()
            || matches!(self.match_emission, MatchEmission::Spans)
            || matches!(self.event_collection, EventCollection::Collect);

        if !scan_spans {
            if matches!(self.match_emission, MatchEmission::Lines) {
                self.matches.push(Match {
                    file: self.path.clone(),
                    line,
                    text: String::from_utf8_lossy(line_bytes).into_owned(),
                });
            }
            return Ok(true);
        }

        let replacement = self.replacement.as_deref().and_then(|replacement| {
            Replacement::expand(self.matcher, line_bytes, replacement).ok()
        });
        let mut ranges = Vec::new();
        let _ = self
            .matcher
            .find_iter(line_bytes, |m: grep_matcher::Match| {
                ranges.push(m.start()..m.end());
                self.match_spans += 1;
                if matches!(self.match_emission, MatchEmission::Spans) {
                    self.matches.push(Match {
                        file: self.path.clone(),
                        line,
                        text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                    });
                }
                true
            });
        if matches!(self.match_emission, MatchEmission::Lines) {
            self.matches.push(Match {
                file: self.path.clone(),
                line,
                text: String::from_utf8_lossy(line_bytes).into_owned(),
            });
        }
        self.event_collection.push(
            &mut self.events,
            SearchEvent::Match(MatchEvent {
                path: self.path.clone(),
                line_number: mat.line_number(),
                absolute_byte_offset: Some(mat.absolute_byte_offset()),
                bytes: line_bytes.to_vec(),
                ranges,
                replacement: replacement
                    .as_ref()
                    .map(|replacement| replacement.line.clone()),
                replacement_matches: replacement
                    .map_or_else(Vec::new, |replacement| replacement.matches),
            }),
        );
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &RegexSearcher,
        context: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        self.event_collection.push(
            &mut self.events,
            SearchEvent::Context(ContextEvent {
                path: self.path.clone(),
                kind: ContextKind::from(context.kind()),
                line_number: context.line_number(),
                absolute_byte_offset: context.absolute_byte_offset(),
                bytes: context.bytes().to_vec(),
            }),
        );
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &RegexSearcher) -> Result<bool, Self::Error> {
        self.event_collection
            .push(&mut self.events, SearchEvent::ContextBreak);
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _searcher: &RegexSearcher,
        binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary_byte_offset.get_or_insert(binary_byte_offset);
        self.event_collection.push(
            &mut self.events,
            SearchEvent::Binary(BinaryEvent {
                path: self.path.clone(),
                absolute_byte_offset: binary_byte_offset,
                explicit: matches!(self.origin, InputOrigin::Explicit),
            }),
        );
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
        self.event_collection.push(
            &mut self.events,
            SearchEvent::End(FileEvent {
                path: self.path.clone(),
            }),
        );
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
