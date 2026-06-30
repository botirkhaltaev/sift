use std::sync::atomic::{AtomicBool, Ordering};

use grep_printer::{JSON, Stats as JsonStats};
use grep_searcher::Searcher;

use crate::Candidate;
use crate::grep::input::GrepInput;
use crate::grep::output::GrepOutput;
use crate::grep::output::mode::OutputEmission;
use crate::grep::query::GrepQuery;
use crate::grep::query::matcher::GrepMatcher;
use crate::grep::sink::FileReporter;
use crate::grep::sink::result::{ChunkOutput, FileResult};

struct NullWriter;

impl std::io::Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(in crate::grep) struct JsonReporter<'a> {
    searcher: Searcher,
    matcher: &'a GrepMatcher,
    output: GrepOutput,
}

impl<'a> JsonReporter<'a> {
    pub(in crate::grep) fn new(
        search: &'a GrepQuery,
        matcher: &'a GrepMatcher,
        output: GrepOutput,
    ) -> Self {
        Self {
            searcher: search.build_searcher(true, search.opts().max_results, true),
            matcher,
            output,
        }
    }

    fn search_candidate_input(
        &mut self,
        candidate: &Candidate,
        bytes: Option<&[u8]>,
        stop: &AtomicBool,
    ) -> FileResult {
        let quiet = self.output.emission == OutputEmission::Quiet;
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }
        let (bytes, file_stats) = if quiet {
            let mut json = JSON::new(NullWriter);
            let mut sink = json.sink_with_path(self.matcher, candidate.abs_path());
            if let Some(bytes) = bytes {
                let _ = self.searcher.search_slice(self.matcher, bytes, &mut sink);
            } else {
                let _ = self
                    .searcher
                    .search_path(self.matcher, candidate.abs_path(), &mut sink);
            }
            (Vec::new(), sink.stats().clone())
        } else {
            let mut json = JSON::new(Vec::new());
            let file_stats = {
                let mut sink = json.sink_with_path(self.matcher, candidate.abs_path());
                if let Some(bytes) = bytes {
                    let _ = self.searcher.search_slice(self.matcher, bytes, &mut sink);
                } else {
                    let _ =
                        self.searcher
                            .search_path(self.matcher, candidate.abs_path(), &mut sink);
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

    fn search_bytes(
        &mut self,
        display_path: &str,
        bytes: &[u8],
        candidate: Option<&Candidate>,
        stop: &AtomicBool,
    ) -> FileResult {
        if let Some(candidate) = candidate {
            return self.search_candidate_input(candidate, Some(bytes), stop);
        }

        let quiet = self.output.emission == OutputEmission::Quiet;
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }
        let path = std::path::Path::new(display_path);
        let (bytes, file_stats) = if quiet {
            let mut json = JSON::new(NullWriter);
            let mut sink = json.sink_with_path(self.matcher, path);
            let _ = self.searcher.search_slice(self.matcher, bytes, &mut sink);
            (Vec::new(), sink.stats().clone())
        } else {
            let mut json = JSON::new(Vec::new());
            let file_stats = {
                let mut sink = json.sink_with_path(self.matcher, path);
                let _ = self.searcher.search_slice(self.matcher, bytes, &mut sink);
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

impl FileReporter for JsonReporter<'_> {
    fn report(&mut self, input: &GrepInput<'_>, stop: &AtomicBool) -> FileResult {
        match input {
            GrepInput::Path { candidate } => self.search_candidate_input(candidate, None, stop),
            GrepInput::Bytes {
                display_path,
                bytes,
                candidate,
            } => self.search_bytes(display_path, bytes, *candidate, stop),
        }
    }
}

pub(in crate::grep) fn format_json_summary_line(
    wall: std::time::Duration,
    agg: &JsonStats,
) -> Result<String, crate::grep::GrepError> {
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
