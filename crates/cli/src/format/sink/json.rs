use std::sync::atomic::{AtomicBool, Ordering};

use grep_printer::{JSON, Stats as JsonStats};
use grep_searcher::Searcher;

use crate::format::output::PrintSpec;
use crate::format::output::mode::OutputEmission;
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use sift_core::grep::{CompiledQuery, Input, Matcher, Query, SearcherConfig};

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
    searcher: Searcher,
    matcher: &'a Matcher,
    output: PrintSpec,
}

impl<'a> JsonPrinter<'a> {
    pub(in crate::format) fn new(
        search: &'a Query,
        compiled: &'a CompiledQuery,
        output: PrintSpec,
    ) -> Self {
        Self {
            compiled,
            searcher: SearcherConfig {
                line_numbers: true,
                max_matches: search.opts().max_results,
                include_context: true,
            }
            .searcher(search),
            matcher: compiled.matcher(),
            output,
        }
    }

    fn search_input(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        let quiet = self.output.emission == OutputEmission::Quiet;
        if stop.load(Ordering::SeqCst) {
            return FileResult::text_empty();
        }

        let path = match input {
            Input::Path { candidate } => candidate.abs_path(),
            Input::Bytes { path, .. } => std::path::Path::new(path.as_ref()),
        };

        let (bytes, file_stats) = if quiet {
            let mut json = JSON::new(NullWriter);
            let mut sink = json.sink_with_path(self.matcher, path);
            self.compiled
                .match_input(input, &mut self.searcher, &mut sink);
            (Vec::new(), sink.stats().clone())
        } else {
            let mut json = JSON::new(Vec::new());
            let file_stats = {
                let mut sink = json.sink_with_path(self.matcher, path);
                self.compiled
                    .match_input(input, &mut self.searcher, &mut sink);
                sink.stats().clone()
            };
            (json.into_inner(), file_stats)
        };

        let had_match = file_stats.matches() > 0;
        if quiet && had_match {
            stop.store(true, Ordering::SeqCst);
        }
        FileResult::Json {
            output: ChunkOutput {
                bytes,
                matched: had_match,
                heading: false,
            },
            stats: file_stats,
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
