use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use grep_printer::{JSON, Stats as JsonStats};
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use rayon::prelude::*;

use crate::Candidate;
use crate::search::emit::result::{ChunkOutput, FileResult};
use crate::search::emit::stats::SearchStats;
use crate::search::output::SearchOutput;
use crate::search::output::mode::OutputEmission;
use crate::search::query::SearchQuery;

struct NullWriter;

impl std::io::Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct JsonWorker<'a> {
    searcher: Searcher,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
}

impl<'a> JsonWorker<'a> {
    fn new(scan: &'a JsonScan<'a>) -> Self {
        Self {
            searcher: scan
                .search
                .build_searcher(true, scan.search.opts().max_results, true),
            matcher: scan.matcher,
            output: scan.output.clone(),
        }
    }

    fn search_candidate(&mut self, candidate: &Candidate, stop: &AtomicBool) -> FileResult {
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
            let _ = self
                .searcher
                .search_path(self.matcher, candidate.abs_path(), &mut sink);
            (Vec::new(), sink.stats().clone())
        } else {
            let mut json = JSON::new(Vec::new());
            let file_stats = {
                let mut sink = json.sink_with_path(self.matcher, candidate.abs_path());
                let _ = self
                    .searcher
                    .search_path(self.matcher, candidate.abs_path(), &mut sink);
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

fn format_json_summary_line(
    wall: std::time::Duration,
    agg: &JsonStats,
) -> Result<String, crate::search::SearchError> {
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

pub struct JsonScan<'a> {
    search: &'a SearchQuery,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    wall_start: Instant,
}

impl<'a> JsonScan<'a> {
    pub const fn new(
        search: &'a SearchQuery,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        wall_start: Instant,
    ) -> Self {
        Self {
            search,
            matcher,
            output,
            wall_start,
        }
    }

    /// # Errors
    ///
    /// Returns an error if scanning or writing output fails.
    pub fn run(
        &self,
        candidates: &[Candidate],
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        let stop = AtomicBool::new(false);
        let n = candidates.len();
        let mut files = Vec::with_capacity(n);
        candidates
            .par_iter()
            .map_init(
                || JsonWorker::new(self),
                |worker: &mut JsonWorker<'_>, candidate: &Candidate| {
                    worker.search_candidate(candidate, &stop)
                },
            )
            .collect_into_vec(&mut files);

        let mut merged = JsonStats::new();
        let mut outputs = Vec::with_capacity(files.len());
        for f in files {
            if let Some(st) = f.json_stats {
                merged += &st;
            }
            outputs.push(f.output);
        }
        let any_match = ChunkOutput::flush_all(
            outputs,
            None,
            crate::search::output::style::OutputBuffering::Auto,
        )?;
        let summary_line = format_json_summary_line(self.wall_start.elapsed(), &merged)?;
        let summary_bytes = summary_line.len() as u64 + 1;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(summary_line.as_bytes())?;
        stdout.write_all(b"\n")?;
        if let Some(s) = stats {
            s.fill_from_json(
                &merged,
                candidates.len(),
                crate::Candidate::total_file_bytes(candidates),
                self.wall_start.elapsed(),
                summary_bytes,
            );
        }
        Ok(any_match)
    }
}
