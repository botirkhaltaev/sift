use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use grep_printer::{JSON, Stats as JsonStats};
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use rayon::prelude::*;

use crate::grep::emit::result::{ChunkOutput, FileResult, flush_chunk_output};
use crate::grep::emit::stats::{SearchStats, fill_json_search_stats};
use crate::grep::filter::CandidateInfo;
use crate::grep::output::SearchOutput;
use crate::grep::output::mode::OutputEmission;
use crate::grep::query::SearchQuery;

struct NullWriter;

impl std::io::Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct JsonWorker<'a> {
    searcher: Searcher,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
}

impl<'a> JsonWorker<'a> {
    pub fn new(search: &'a SearchQuery, matcher: &'a RegexMatcher, output: SearchOutput) -> Self {
        Self {
            searcher: search.build_searcher(true, search.opts.max_results, true),
            matcher,
            output,
        }
    }

    pub fn search_candidate(
        &mut self,
        candidate: &CandidateInfo,
        result_index: usize,
        stop: &AtomicBool,
    ) -> FileResult {
        json_search_one(
            &mut self.searcher,
            self.matcher,
            self.output,
            candidate,
            result_index,
            stop,
        )
    }
}

pub fn json_search_one(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    output: SearchOutput,
    candidate: &CandidateInfo,
    result_index: usize,
    stop: &AtomicBool,
) -> FileResult {
    if stop.load(Ordering::SeqCst) {
        return FileResult {
            index: result_index,
            output: ChunkOutput::empty(),
            json_stats: None,
        };
    }
    let quiet = output.emission == OutputEmission::Quiet;
    let (bytes, file_stats) = if quiet {
        let mut json = JSON::new(NullWriter);
        let mut sink = json.sink_with_path(matcher, &candidate.abs_path);
        let _ = searcher.search_path(matcher, &candidate.abs_path, &mut sink);
        (Vec::new(), sink.stats().clone())
    } else {
        let mut json = JSON::new(Vec::new());
        let file_stats = {
            let mut sink = json.sink_with_path(matcher, &candidate.abs_path);
            let _ = searcher.search_path(matcher, &candidate.abs_path, &mut sink);
            sink.stats().clone()
        };
        (json.into_inner(), file_stats)
    };
    let had_match = file_stats.matches() > 0;
    if output.emission == OutputEmission::Quiet && had_match {
        stop.store(true, Ordering::SeqCst);
    }
    FileResult {
        index: result_index,
        output: ChunkOutput {
            bytes,
            matched: had_match,
            heading: false,
        },
        json_stats: Some(file_stats),
    }
}

pub fn format_json_summary_line(
    wall: std::time::Duration,
    agg: &JsonStats,
) -> Result<String, crate::grep::SearchError> {
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

pub fn finish_json_run(
    files: Vec<FileResult>,
    wall_start: Instant,
    stats: Option<&mut SearchStats>,
    candidates_len: usize,
    bytes_searched_sum: u64,
) -> crate::Result<bool> {
    let mut merged = JsonStats::new();
    let mut outputs = Vec::with_capacity(files.len());
    for f in files {
        if let Some(st) = f.json_stats {
            merged += &st;
        }
        outputs.push(f.output);
    }
    let any_match = flush_chunk_output(outputs, None)?;
    let summary_line = format_json_summary_line(wall_start.elapsed(), &merged)?;
    let summary_bytes = summary_line.len() as u64 + 1;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(summary_line.as_bytes())?;
    stdout.write_all(b"\n")?;
    if let Some(s) = stats {
        fill_json_search_stats(
            s,
            &merged,
            candidates_len,
            bytes_searched_sum,
            wall_start.elapsed(),
            summary_bytes,
        );
    }
    Ok(any_match)
}

pub fn run_json_standard_with_info(
    search: &SearchQuery,
    candidates: &[CandidateInfo],
    matcher: &RegexMatcher,
    output: SearchOutput,
    wall_start: Instant,
    stats: Option<&mut SearchStats>,
) -> crate::Result<bool> {
    let stop = AtomicBool::new(false);
    let n = candidates.len();
    let mut files = Vec::with_capacity(n);
    candidates
        .par_iter()
        .enumerate()
        .map_init(
            || JsonWorker::new(search, matcher, output),
            |worker: &mut JsonWorker<'_>, (result_index, candidate): (usize, &CandidateInfo)| {
                worker.search_candidate(candidate, result_index, &stop)
            },
        )
        .collect_into_vec(&mut files);
    files.sort_by_key(|file| file.index);
    finish_json_run(
        files,
        wall_start,
        stats,
        candidates.len(),
        crate::grep::emit::format::sum_candidate_file_bytes(candidates),
    )
}
