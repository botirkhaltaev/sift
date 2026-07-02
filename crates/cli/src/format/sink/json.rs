use std::sync::atomic::{AtomicBool, Ordering};

use grep_matcher::LineTerminator;
use grep_matcher::Matcher as GrepMatcherTrait;
use grep_printer::{JSON, Stats as JsonStats};
use grep_searcher::{BinaryDetection, SearcherBuilder};

use crate::format::output::PrintSpec;
use crate::format::output::mode::OutputEmission;
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use sift_core::grep::{BinaryMode, CompiledQuery, Input, Query};

struct NullWriter;

impl std::io::Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(in crate::format) struct JsonPrinter<'a> {
    compiled: &'a CompiledQuery,
    search: &'a Query,
    output: PrintSpec,
}

impl<'a> JsonPrinter<'a> {
    pub(in crate::format) const fn new(
        search: &'a Query,
        compiled: &'a CompiledQuery,
        output: PrintSpec,
    ) -> Self {
        Self {
            compiled,
            search,
            output,
        }
    }

    fn search_input(&self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        let quiet = self.output.emission == OutputEmission::Quiet;
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let path = match input {
            Input::Path { candidate, .. } => candidate.abs_path(),
            Input::Bytes { path, .. } => std::path::Path::new(path.as_ref()),
        };
        let before_context = self.search.opts().before_context;
        let after_context = self.search.opts().after_context;
        let binary_detection = if self.search.opts().null_data() {
            BinaryDetection::none()
        } else {
            match self.search.opts().binary_mode {
                BinaryMode::Quit
                    if matches!(
                        input,
                        Input::Path { explicit: true, .. } | Input::Bytes { explicit: true, .. }
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
            .encoding(self.search.opts().input_encoding.explicit())
            .bom_sniffing(self.search.opts().input_encoding.bom_sniffing())
            .binary_detection(binary_detection)
            .line_terminator(LineTerminator::byte(self.search.opts().line_terminator()))
            .invert_match(self.search.opts().invert_match())
            .line_number(true)
            .before_context(before_context)
            .after_context(after_context)
            .max_matches(self.search.opts().max_results.map(|n| n as u64));
        if self.search.opts().multiline() {
            builder.multi_line(true);
        }
        let searcher = builder.build();

        match self.compiled {
            CompiledQuery::Rust { matcher, .. } => {
                Self::search_with_matcher(matcher, input, quiet, path, searcher, stop)
            }
            CompiledQuery::Pcre2 { matcher, .. } => {
                Self::search_with_matcher(matcher, input, quiet, path, searcher, stop)
            }
        }
    }

    fn search_with_matcher<M: GrepMatcherTrait>(
        matcher: &M,
        input: &Input<'_>,
        quiet: bool,
        path: &std::path::Path,
        mut searcher: grep_searcher::Searcher,
        stop: &AtomicBool,
    ) -> FileResult {
        let (bytes, file_stats) = if quiet {
            let mut json = JSON::new(NullWriter);
            let mut sink = json.sink_with_path(matcher, path);
            match input {
                Input::Path { candidate, .. } => {
                    let _ = searcher.search_path(matcher, candidate.abs_path(), &mut sink);
                }
                Input::Bytes { bytes, .. } => {
                    let _ = searcher.search_slice(matcher, bytes, &mut sink);
                }
            }
            (Vec::new(), sink.stats().clone())
        } else {
            let mut json = JSON::new(Vec::new());
            let file_stats = {
                let mut sink = json.sink_with_path(matcher, path);
                match input {
                    Input::Path { candidate, .. } => {
                        let _ = searcher.search_path(matcher, candidate.abs_path(), &mut sink);
                    }
                    Input::Bytes { bytes, .. } => {
                        let _ = searcher.search_slice(matcher, bytes, &mut sink);
                    }
                }
                sink.stats().clone()
            };
            (json.into_inner(), file_stats)
        };

        let had_match = file_stats.matches() > 0;
        if quiet && had_match {
            stop.store(true, Ordering::SeqCst);
        }
        FileResult {
            output: ChunkOutput {
                bytes,
                matched: had_match,
                heading: false,
            },
            json_stats: Some(file_stats),
            hit: None,
        }
    }
}

impl InputPrinter for JsonPrinter<'_> {
    fn report(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        self.search_input(input, stop)
    }
}

impl JsonPrinter<'_> {
    pub(in crate::format) fn summary_line(
        wall: std::time::Duration,
        agg: &JsonStats,
    ) -> Result<String, sift_core::GrepError> {
        let stats_val = serde_json::to_value(agg)?;
        let wall_secs = f64::from(wall.subsec_nanos()).mul_add(1e-9, wall.as_secs_f64());
        let v = serde_json::json!({
            "type": "summary",
            "data": {
                "elapsed_total": {
                    "secs": wall.as_secs(),
                    "nanos": wall.subsec_nanos(),
                    "human": format!("{wall_secs:0.6}s"),
                },
                "stats": stats_val,
            }
        });
        Ok(serde_json::to_string(&v)?)
    }
}
