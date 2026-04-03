use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use grep_matcher::Matcher;
use grep_printer::{JSON, Stats as JsonStats};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use rayon::prelude::*;

use crate::Index;
use crate::planner::TrigramPlan;

use super::{
    CandidateInfo, ColorChoice, CompiledSearch, FilenameMode, OutputEmission, SearchFilter,
    SearchMode, SearchOutput, SearchOutputFormat, SearchRecordStyle, SearchStats,
};

#[cfg(test)]
use super::{GlobConfig, HiddenMode, IgnoreConfig, Match, SearchFilterConfig, VisibilityConfig};

const ANSI_RESET: &[u8] = b"\x1b[0m";
const ANSI_PATH: &[u8] = b"\x1b[35m\x1b[1m";
const ANSI_LINE: &[u8] = b"\x1b[32m";

#[inline]
fn should_color(records: SearchRecordStyle) -> bool {
    match records.color {
        ColorChoice::Never => false,
        ColorChoice::Always => true,
        ColorChoice::Auto => std::io::stdout().is_terminal(),
    }
}

#[inline]
fn write_line_terminator(out: &mut Vec<u8>, null_data: bool) {
    if null_data {
        out.push(0);
    } else {
        out.push(b'\n');
    }
}

fn sum_candidate_file_bytes(candidates: &[CandidateInfo]) -> u64 {
    candidates.iter().fold(0u64, |acc, c| {
        acc + std::fs::metadata(&c.abs_path).map(|m| m.len()).unwrap_or(0)
    })
}

/// Optional atomics filled when collecting [`SearchStats`] (mode-dependent primary tally, files with
/// hits, bytes written to stdout).
#[derive(Clone, Copy)]
struct StatsCollection<'a> {
    primary: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    bytes_printed: Option<&'a AtomicU64>,
}

/// Discards JSON bytes (for `--json` + quiet: ripgrep emits summary only).
struct NullWriter;

impl io::Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[inline]
fn fill_json_search_stats(
    s: &mut SearchStats,
    merged: &JsonStats,
    candidates_len: usize,
    bytes_searched_sum: u64,
    elapsed: std::time::Duration,
    summary_line_bytes: u64,
) {
    s.matches = usize::try_from(merged.matches()).unwrap_or(usize::MAX);
    s.files_with_matches = usize::try_from(merged.searches_with_match()).unwrap_or(usize::MAX);
    s.files_searched = candidates_len;
    s.bytes_printed = merged.bytes_printed() + summary_line_bytes;
    s.bytes_searched = bytes_searched_sum;
    s.elapsed = elapsed;
}

fn format_json_summary_line(wall: std::time::Duration, agg: &JsonStats) -> crate::Result<String> {
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

impl CompiledSearch {
    /// Returns raw candidate file IDs from index (trigram or full scan).
    /// Does NOT apply `SearchFilter` - filtering happens in `prepare_candidates`.
    #[must_use]
    pub fn candidate_file_ids(&self, index: &Index, exhaustive: bool) -> Vec<usize> {
        let n = index.file_count();
        if exhaustive {
            let mut v = Vec::with_capacity(n);
            v.extend(0..n);
            return v;
        }
        match &self.plan {
            TrigramPlan::FullScan => {
                let mut v = Vec::with_capacity(n);
                v.extend(0..n);
                v
            }
            TrigramPlan::Narrow { arms } => {
                let raw = index.candidate_file_ids(arms.as_slice());
                let mut v = Vec::with_capacity(raw.len());
                v.extend(raw.into_iter().map(|id| id as usize));
                v
            }
        }
    }

    /// Execute a search over an opened index and print results to stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the matcher cannot be built or stdout cannot be written.
    pub fn run_index(
        &self,
        index: &Index,
        filter: &SearchFilter,
        output: SearchOutput,
    ) -> crate::Result<bool> {
        self.run_index_impl(index, filter, output, None)
    }

    /// Like [`Self::run_index`], but fills `stats` with search counters (ripgrep-style `--stats`).
    ///
    /// # Errors
    ///
    /// Same as [`Self::run_index`].
    pub fn run_index_with_stats(
        &self,
        index: &Index,
        filter: &SearchFilter,
        output: SearchOutput,
        stats: &mut SearchStats,
    ) -> crate::Result<bool> {
        self.run_index_impl(index, filter, output, Some(stats))
    }

    fn run_index_impl(
        &self,
        index: &Index,
        filter: &SearchFilter,
        output: SearchOutput,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        if self.opts.max_results == Some(0) {
            return Err(crate::Error::InvalidMaxCount);
        }

        // Stage 1: Get raw candidate IDs from index (trigram or full scan)
        let raw_ids = self.candidate_file_ids(index, Self::uses_exhaustive_candidates(output.mode));
        if raw_ids.is_empty() {
            if let Some(s) = stats {
                *s = SearchStats::default();
            }
            return Ok(false);
        }

        // Stage 2+3: Parallel filter + prepare CandidateInfo (single filter pass)
        let threshold = parallel_candidate_threshold();
        let candidates = Self::prepare_candidates(index, &raw_ids, filter, threshold);
        if candidates.is_empty() {
            if let Some(s) = stats {
                *s = SearchStats::default();
            }
            return Ok(false);
        }

        // Stage 4: Build matcher (once per `CompiledSearch`) and search
        let search_start = Instant::now();
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let parallel = candidates.len() >= threshold;

        if matches!(output.format, SearchOutputFormat::Json) {
            return match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => self
                    .run_json_standard_with_info(
                        &candidates,
                        matcher,
                        output,
                        parallel,
                        search_start,
                        stats,
                    ),
                _ => Err(crate::Error::JsonOutputIncompatibleMode),
            };
        }

        let match_counter = AtomicUsize::new(0);
        let counter_ref = stats.is_some().then_some(&match_counter);
        let files_with_matches = AtomicUsize::new(0);
        let files_with_ref = stats.is_some().then_some(&files_with_matches);
        let summary_counter = AtomicUsize::new(0);
        let summary_ref = stats.is_some().then_some(&summary_counter);
        let bytes_printed = AtomicU64::new(0);
        let printed_ref = stats.is_some().then_some(&bytes_printed);

        let ok = match output.mode {
            SearchMode::Standard | SearchMode::OnlyMatching => self.run_standard_with_info(
                &candidates,
                matcher,
                output,
                parallel,
                StatsCollection {
                    primary: counter_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
            SearchMode::Count
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches
            | SearchMode::FilesWithoutMatch => self.run_summary_with_info(
                &candidates,
                matcher,
                output,
                parallel,
                StatsCollection {
                    primary: summary_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
        };

        if let Some(s) = stats {
            s.matches = match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => {
                    match_counter.load(Ordering::Relaxed)
                }
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => summary_counter.load(Ordering::Relaxed),
            };
            s.files_with_matches = files_with_matches.load(Ordering::Relaxed);
            s.files_searched = candidates.len();
            s.bytes_printed = bytes_printed.load(Ordering::Relaxed);
            s.bytes_searched = sum_candidate_file_bytes(&candidates);
            s.elapsed = search_start.elapsed();
        }

        Ok(ok)
    }

    /// Search by walking the filesystem under `filter_root` (no trigram index).
    ///
    /// Candidate paths are discovered the same way as index build: all files under scoped paths,
    /// then [`SearchFilter`] is applied. Ignores [`TrigramPlan`] narrowing (full file list).
    ///
    /// # Errors
    ///
    /// Returns an error if the matcher cannot be built or stdout cannot be written.
    pub fn run_walk(
        &self,
        filter_root: &Path,
        filter: &SearchFilter,
        output: SearchOutput,
    ) -> crate::Result<bool> {
        self.run_walk_impl(filter_root, filter, output, None)
    }

    /// Like [`Self::run_walk`], but fills `stats` with search counters.
    ///
    /// # Errors
    ///
    /// Same as [`Self::run_walk`].
    pub fn run_walk_with_stats(
        &self,
        filter_root: &Path,
        filter: &SearchFilter,
        output: SearchOutput,
        stats: &mut SearchStats,
    ) -> crate::Result<bool> {
        self.run_walk_impl(filter_root, filter, output, Some(stats))
    }

    fn run_walk_impl(
        &self,
        filter_root: &Path,
        filter: &SearchFilter,
        output: SearchOutput,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        if self.opts.max_results == Some(0) {
            return Err(crate::Error::InvalidMaxCount);
        }

        let abs_paths =
            collect_abs_paths_for_scopes(filter_root, filter.scopes(), filter.follow_links())?;
        if abs_paths.is_empty() {
            if let Some(s) = stats {
                *s = SearchStats::default();
            }
            return Ok(false);
        }

        let threshold = parallel_candidate_threshold();
        let candidates = prepare_walk_candidates(filter_root, &abs_paths, filter, threshold);
        if candidates.is_empty() {
            if let Some(s) = stats {
                *s = SearchStats::default();
            }
            return Ok(false);
        }

        let search_start = Instant::now();
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let parallel = candidates.len() >= threshold;

        if matches!(output.format, SearchOutputFormat::Json) {
            return match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => self
                    .run_json_standard_with_info(
                        &candidates,
                        matcher,
                        output,
                        parallel,
                        search_start,
                        stats,
                    ),
                _ => Err(crate::Error::JsonOutputIncompatibleMode),
            };
        }

        let match_counter = AtomicUsize::new(0);
        let counter_ref = stats.is_some().then_some(&match_counter);
        let files_with_matches = AtomicUsize::new(0);
        let files_with_ref = stats.is_some().then_some(&files_with_matches);
        let summary_counter = AtomicUsize::new(0);
        let summary_ref = stats.is_some().then_some(&summary_counter);
        let bytes_printed = AtomicU64::new(0);
        let printed_ref = stats.is_some().then_some(&bytes_printed);

        let ok = match output.mode {
            SearchMode::Standard | SearchMode::OnlyMatching => self.run_standard_with_info(
                &candidates,
                matcher,
                output,
                parallel,
                StatsCollection {
                    primary: counter_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
            SearchMode::Count
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches
            | SearchMode::FilesWithoutMatch => self.run_summary_with_info(
                &candidates,
                matcher,
                output,
                parallel,
                StatsCollection {
                    primary: summary_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
        };

        if let Some(s) = stats {
            s.matches = match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => {
                    match_counter.load(Ordering::Relaxed)
                }
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => summary_counter.load(Ordering::Relaxed),
            };
            s.files_with_matches = files_with_matches.load(Ordering::Relaxed);
            s.files_searched = candidates.len();
            s.bytes_printed = bytes_printed.load(Ordering::Relaxed);
            s.bytes_searched = sum_candidate_file_bytes(&candidates);
            s.elapsed = search_start.elapsed();
        }

        Ok(ok)
    }

    /// Prepare `CandidateInfo` with parallel filter + path prep.
    #[must_use]
    pub fn prepare_candidates(
        index: &Index,
        ids: &[usize],
        filter: &SearchFilter,
        threshold: usize,
    ) -> Vec<CandidateInfo> {
        let need_rel = filter.needs_rel_str_for_matching();
        let cap = ids.len();
        if ids.len() >= threshold {
            ids.par_iter()
                .filter_map(|&id| {
                    let rel_path = index.file_path(id)?.to_path_buf();
                    let rel_str = if need_rel {
                        rel_path.to_string_lossy().replace('\\', "/")
                    } else {
                        String::new()
                    };
                    let abs_path = index.file_abs_path(id)?;
                    let info = CandidateInfo {
                        id,
                        rel_path,
                        rel_str,
                        abs_path,
                    };
                    filter.is_candidate_info(&info).then_some(info)
                })
                .collect()
        } else {
            let mut out = Vec::with_capacity(cap);
            out.extend(ids.iter().filter_map(|&id| {
                let rel_path = index.file_path(id)?.to_path_buf();
                let rel_str = if need_rel {
                    rel_path.to_string_lossy().replace('\\', "/")
                } else {
                    String::new()
                };
                let abs_path = index.file_abs_path(id)?;
                let info = CandidateInfo {
                    id,
                    rel_path,
                    rel_str,
                    abs_path,
                };
                filter.is_candidate_info(&info).then_some(info)
            }));
            out
        }
    }

    fn run_json_standard_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        parallel: bool,
        wall_start: Instant,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        let bytes_searched_sum = sum_candidate_file_bytes(candidates);
        if parallel {
            let stop = AtomicBool::new(false);
            let n = candidates.len();
            let mut files = Vec::with_capacity(n);
            candidates
                .par_iter()
                .enumerate()
                .map_init(
                    || JsonWorker::new(self, matcher, output),
                    |worker: &mut JsonWorker<'_>,
                     (result_index, candidate): (usize, &CandidateInfo)| {
                        worker.search_candidate(candidate, result_index, &stop)
                    },
                )
                .collect_into_vec(&mut files);
            files.sort_by_key(|file| file.index);
            return finish_json_run(
                files,
                wall_start,
                stats,
                candidates.len(),
                bytes_searched_sum,
            );
        }

        self.run_json_capped_with_info(candidates, matcher, output, wall_start, stats)
    }

    fn run_json_capped_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        wall_start: Instant,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        let bytes_searched_sum = sum_candidate_file_bytes(candidates);
        self.with_cached_searcher(true, self.opts.max_results, |searcher| {
            let stop = AtomicBool::new(false);
            let mut files = Vec::with_capacity(candidates.len());
            for (i, candidate) in candidates.iter().enumerate() {
                files.push(json_search_one(
                    searcher, matcher, output, candidate, i, &stop,
                ));
            }
            finish_json_run(
                files,
                wall_start,
                stats,
                candidates.len(),
                bytes_searched_sum,
            )
        })
    }

    fn run_standard_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        parallel: bool,
        stats: StatsCollection<'_>,
    ) -> crate::Result<bool> {
        if parallel {
            let stop = AtomicBool::new(false);
            let n = candidates.len();
            let mut files = Vec::with_capacity(n);
            candidates
                .par_iter()
                .enumerate()
                .map_init(
                    || {
                        StandardWorker::new(
                            self,
                            matcher,
                            output,
                            stats.primary,
                            stats.files_with_matches,
                        )
                    },
                    |worker: &mut StandardWorker<'_>,
                     (result_index, candidate): (usize, &CandidateInfo)| {
                        worker.search_candidate(candidate, result_index, &stop)
                    },
                )
                .collect_into_vec(&mut files);
            files.sort_by_key(|file| file.index);
            return flush_chunk_output(
                files.into_iter().map(|file| file.output),
                stats.bytes_printed,
            );
        }

        self.run_standard_capped_with_info(candidates, matcher, output, stats)
    }

    fn run_summary_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        parallel: bool,
        stats: StatsCollection<'_>,
    ) -> crate::Result<bool> {
        if parallel {
            let stop = AtomicBool::new(false);
            let n = candidates.len();
            let mut files = Vec::with_capacity(n);
            candidates
                .par_iter()
                .enumerate()
                .map_init(
                    || {
                        SummaryWorker::new(
                            self,
                            matcher,
                            self.opts.max_results,
                            output.mode,
                            stats.primary,
                            stats.files_with_matches,
                        )
                    },
                    |worker: &mut SummaryWorker<'_>,
                     (result_index, candidate): (usize, &CandidateInfo)| {
                        worker.search_candidate(candidate, result_index, output, &stop)
                    },
                )
                .collect_into_vec(&mut files);
            files.sort_by_key(|file| file.index);
            return flush_chunk_output(
                files.into_iter().map(|file| file.output),
                stats.bytes_printed,
            );
        }

        self.run_summary_capped_with_info(candidates, matcher, output, stats)
    }

    fn run_standard_capped_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        stats: StatsCollection<'_>,
    ) -> crate::Result<bool> {
        let show_line_numbers =
            output.lines.line_number || self.opts.before_context > 0 || self.opts.after_context > 0;
        self.with_cached_searcher(
            output.lines.line_number,
            self.opts.max_results,
            |searcher| {
                let mut any_match = false;
                let mut out = Vec::new();
                for candidate in candidates {
                    let heading =
                        output.lines.heading && output.lines.filename_mode != FilenameMode::Never;
                    let mut sink_output = output;
                    if heading {
                        sink_output.lines.filename_mode = FilenameMode::Never;
                    }
                    let mut bytes = Vec::new();
                    let mut sink = StandardSink::new(
                        matcher,
                        sink_output,
                        show_line_numbers,
                        &candidate.rel_path,
                        &mut bytes,
                    );
                    let _ = searcher.search_path(matcher, &candidate.abs_path, &mut sink);
                    if let Some(c) = stats.primary {
                        c.fetch_add(sink.match_count, Ordering::Relaxed);
                    }
                    if sink.matched
                        && let Some(c) = stats.files_with_matches
                    {
                        c.fetch_add(1, Ordering::Relaxed);
                    }
                    any_match |= sink.matched;
                    if sink.matched && heading {
                        if !out.is_empty() {
                            out.push(b'\n');
                        }
                        if should_color(output.records) {
                            out.extend_from_slice(ANSI_PATH);
                        }
                        write!(out, "{}", candidate.rel_path.display())?;
                        if should_color(output.records) {
                            out.extend_from_slice(ANSI_RESET);
                        }
                        write_line_terminator(&mut out, output.records.null_data);
                    }
                    out.extend(bytes);
                    if output.emission == OutputEmission::Quiet && any_match {
                        break;
                    }
                }

                flush_chunk_output(
                    std::iter::once(ChunkOutput {
                        bytes: out,
                        matched: any_match,
                        heading: false,
                    }),
                    stats.bytes_printed,
                )
            },
        )
    }

    fn run_summary_capped_with_info(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        output: SearchOutput,
        stats: StatsCollection<'_>,
    ) -> crate::Result<bool> {
        self.with_cached_searcher(false, self.opts.max_results, |searcher| {
            let mut any_match = false;
            let mut out = Vec::new();
            for candidate in candidates {
                let result =
                    summary_search_file(searcher, matcher, output.mode, &candidate.abs_path);
                if let Some(c) = stats.primary {
                    c.fetch_add(
                        summary_matches_tally(output.mode, result),
                        Ordering::Relaxed,
                    );
                }
                if let Some(c) = stats.files_with_matches
                    && summary_file_had_positive_hit(output.mode, result)
                {
                    c.fetch_add(1, Ordering::Relaxed);
                }
                any_match |= mode_is_success(output.mode, result);
                write_summary_record(&mut out, output, &candidate.rel_path, result)?;
                if output.emission == OutputEmission::Quiet && mode_is_success(output.mode, result)
                {
                    break;
                }
            }

            flush_chunk_output(
                std::iter::once(ChunkOutput {
                    bytes: out,
                    matched: any_match,
                    heading: false,
                }),
                stats.bytes_printed,
            )
        })
    }

    // Test-only helpers

    #[cfg(test)]
    pub(crate) fn collect_index_matches(&self, index: &Index) -> crate::Result<Vec<Match>> {
        let config = SearchFilterConfig {
            scopes: vec![],
            exclude_paths: vec![],
            glob: GlobConfig::default(),
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            follow_links: false,
        };
        let filter = SearchFilter::new(&config, &index.root)?;
        let candidate_ids = self.candidate_file_ids(index, false);
        self.collect_index_candidates(index, &filter, &candidate_ids)
    }

    #[cfg(test)]
    pub(crate) fn collect_walk_matches(&self, root: &Path) -> crate::Result<Vec<Match>> {
        let root = root.canonicalize()?;
        let mut candidates = Vec::new();
        let walker = ignore::WalkBuilder::new(&root)
            .follow_links(false)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
            .require_git(false)
            .build();
        for entry in walker {
            let entry = entry.map_err(crate::Error::Ignore)?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.path();
                if path.components().any(|c| c.as_os_str() == ".sift") {
                    continue;
                }
                candidates.push(path.to_path_buf());
            }
        }
        self.collect_walk_candidates(&candidates)
    }

    #[cfg(test)]
    fn collect_index_candidates(
        &self,
        index: &Index,
        filter: &SearchFilter,
        candidate_ids: &[usize],
    ) -> crate::Result<Vec<Match>> {
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let mut out = Vec::new();
        self.with_cached_searcher(true, None, |searcher| {
            for &id in candidate_ids {
                let Some(candidate) = index.file_path(id) else {
                    continue;
                };
                if !filter.is_candidate(candidate) {
                    continue;
                }
                let mut sink = CollectSink::new(
                    index.root.join(candidate),
                    self.opts.only_matching(),
                    matcher.clone(),
                );
                let _ = searcher.search_path(matcher, index.root.join(candidate), &mut sink);
                out.extend(sink.into_matches());
            }
        });
        Ok(out)
    }

    #[cfg(test)]
    fn collect_walk_candidates(&self, candidates: &[PathBuf]) -> crate::Result<Vec<Match>> {
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let mut out = Vec::new();
        self.with_cached_searcher(true, None, |searcher| {
            for candidate in candidates {
                let mut sink = CollectSink::new(
                    candidate.clone(),
                    self.opts.only_matching(),
                    matcher.clone(),
                );
                let _ = searcher.search_path(matcher, candidate, &mut sink);
                out.extend(sink.into_matches());
            }
        });
        Ok(out)
    }
}

struct JsonWorker<'a> {
    searcher: Searcher,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
}

impl<'a> JsonWorker<'a> {
    fn new(search: &'a CompiledSearch, matcher: &'a RegexMatcher, output: SearchOutput) -> Self {
        Self {
            searcher: search.build_searcher(true, search.opts.max_results, true),
            matcher,
            output,
        }
    }

    fn search_candidate(
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

struct StandardWorker<'a> {
    /// Shared across Rayon threads; [`Matcher`] is implemented for `&RegexMatcher`.
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    output: SearchOutput,
    /// `-C` / `-A` / `-B` imply line numbers in output (ripgrep-style).
    show_line_numbers: bool,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
}

impl<'a> StandardWorker<'a> {
    fn new(
        search: &CompiledSearch,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        match_counter: Option<&'a AtomicUsize>,
        files_with_matches: Option<&'a AtomicUsize>,
    ) -> Self {
        let show_line_numbers = output.lines.line_number
            || search.opts.before_context > 0
            || search.opts.after_context > 0;
        Self {
            searcher: search.build_searcher(
                output.lines.line_number,
                search.opts.max_results,
                true,
            ),
            matcher,
            output,
            show_line_numbers,
            bytes: Vec::new(),
            match_counter,
            files_with_matches,
        }
    }

    fn search_candidate(
        &mut self,
        candidate: &CandidateInfo,
        result_index: usize,
        stop: &AtomicBool,
    ) -> FileResult {
        self.bytes.clear();
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                index: result_index,
                output: ChunkOutput::empty(),
                json_stats: None,
            };
        }

        let matched = {
            let heading =
                self.output.lines.heading && self.output.lines.filename_mode != FilenameMode::Never;
            let mut sink_output = self.output;
            if heading {
                sink_output.lines.filename_mode = FilenameMode::Never;
            }
            let mut sink = StandardSink::new(
                self.matcher,
                sink_output,
                self.show_line_numbers,
                &candidate.rel_path,
                &mut self.bytes,
            );
            let _ = self
                .searcher
                .search_path(self.matcher, &candidate.abs_path, &mut sink);
            let n = sink.match_count;
            if let Some(c) = self.match_counter {
                c.fetch_add(n, Ordering::Relaxed);
            }
            if sink.matched
                && let Some(c) = self.files_with_matches
            {
                c.fetch_add(1, Ordering::Relaxed);
            }
            sink.matched
        };

        if self.output.emission == OutputEmission::Quiet && matched {
            stop.store(true, Ordering::SeqCst);
        }

        // P0 fix: use mem::take instead of clone - avoids allocation when bytes is empty (quiet mode)
        FileResult {
            index: result_index,
            output: ChunkOutput {
                bytes: if matched
                    && self.output.lines.heading
                    && self.output.lines.filename_mode != FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet
                {
                    let mut out = Vec::new();
                    if should_color(self.output.records) {
                        out.extend_from_slice(ANSI_PATH);
                    }
                    let _ = write!(out, "{}", candidate.rel_path.display());
                    if should_color(self.output.records) {
                        out.extend_from_slice(ANSI_RESET);
                    }
                    write_line_terminator(&mut out, self.output.records.null_data);
                    out.extend(std::mem::take(&mut self.bytes));
                    out
                } else {
                    std::mem::take(&mut self.bytes)
                },
                matched,
                heading: matched
                    && self.output.lines.heading
                    && self.output.lines.filename_mode != FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet,
            },
            json_stats: None,
        }
    }
}

struct StandardSink<'a> {
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    show_line_numbers: bool,
    /// Path printed in prefixes (relative to search root, `grep`-style).
    display_path: &'a Path,
    bytes: &'a mut Vec<u8>,
    matched: bool,
    match_count: usize,
}

impl<'a> StandardSink<'a> {
    const fn new(
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        show_line_numbers: bool,
        display_path: &'a Path,
        bytes: &'a mut Vec<u8>,
    ) -> Self {
        Self {
            matcher,
            output,
            show_line_numbers,
            display_path,
            bytes,
            matched: false,
            match_count: 0,
        }
    }
}

impl Sink for StandardSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matched = true;
        self.match_count += 1;

        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }

        if matches!(self.output.mode, SearchMode::OnlyMatching) {
            let line_number = mat.line_number();
            let line = mat.bytes();
            let _ = self.matcher.find_iter(line, |m: grep_matcher::Match| {
                let _ = write_standard_prefix(
                    self.bytes,
                    self.output,
                    self.display_path,
                    line_number,
                    self.show_line_numbers,
                    false,
                );
                let _ = self.bytes.write_all(&line[m.start()..m.end()]);
                let _ = self.bytes.write_all(b"\n");
                true
            });
            return Ok(true);
        }

        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path,
            mat.line_number(),
            self.show_line_numbers,
            false,
        )?;
        self.bytes.write_all(mat.bytes())?;
        if !mat.bytes().ends_with(b"\n") {
            self.bytes.write_all(b"\n")?;
        }
        Ok(true)
    }

    fn context(&mut self, _: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(self.output.mode, SearchMode::OnlyMatching) {
            return Ok(true);
        }
        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path,
            ctx.line_number(),
            self.show_line_numbers,
            true,
        )?;
        self.bytes.write_all(ctx.bytes())?;
        if !ctx.bytes().ends_with(b"\n") {
            self.bytes.write_all(b"\n")?;
        }
        Ok(true)
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(self.output.mode, SearchMode::OnlyMatching) {
            return Ok(true);
        }
        self.bytes.write_all(b"--\n")?;
        Ok(true)
    }
}

fn summary_search_file(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    mode: SearchMode,
    path: &Path,
) -> FileSummary {
    let sink_matcher = if mode == SearchMode::CountMatches {
        Some(matcher.clone())
    } else {
        None
    };
    let mut sink = SummarySink::new(mode, sink_matcher);
    let _ = searcher.search_path(matcher, path, &mut sink);
    sink.finish()
}

struct SummaryWorker<'a> {
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    mode: SearchMode,
    summary_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
}

impl<'a> SummaryWorker<'a> {
    fn new(
        search: &CompiledSearch,
        matcher: &'a RegexMatcher,
        max_results: Option<usize>,
        mode: SearchMode,
        summary_counter: Option<&'a AtomicUsize>,
        files_with_matches: Option<&'a AtomicUsize>,
    ) -> Self {
        Self {
            searcher: search.build_searcher(false, max_results, false),
            matcher,
            mode,
            summary_counter,
            files_with_matches,
        }
    }

    fn search_file(&mut self, path: &Path) -> FileSummary {
        summary_search_file(&mut self.searcher, self.matcher, self.mode, path)
    }

    fn search_candidate(
        &mut self,
        candidate: &CandidateInfo,
        result_index: usize,
        output: SearchOutput,
        stop: &AtomicBool,
    ) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                index: result_index,
                output: ChunkOutput::empty(),
                json_stats: None,
            };
        }

        let result = self.search_file(&candidate.abs_path);
        if let Some(c) = self.summary_counter {
            c.fetch_add(summary_matches_tally(self.mode, result), Ordering::Relaxed);
        }
        if let Some(c) = self.files_with_matches
            && summary_file_had_positive_hit(self.mode, result)
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        let matched = mode_is_success(output.mode, result);
        let mut bytes = Vec::new();
        let _ = write_summary_record(&mut bytes, output, &candidate.rel_path, result);
        if output.emission == OutputEmission::Quiet && mode_is_success(output.mode, result) {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            index: result_index,
            output: ChunkOutput {
                bytes,
                matched,
                heading: false,
            },
            json_stats: None,
        }
    }
}

struct FileResult {
    index: usize,
    output: ChunkOutput,
    /// Per-file [`JsonStats`] when running JSON output mode; unused for text printers.
    json_stats: Option<JsonStats>,
}

struct ChunkOutput {
    bytes: Vec<u8>,
    matched: bool,
    heading: bool,
}

impl ChunkOutput {
    const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            matched: false,
            heading: false,
        }
    }
}

fn flush_chunk_output(
    outputs: impl IntoIterator<Item = ChunkOutput>,
    bytes_printed: Option<&AtomicU64>,
) -> crate::Result<bool> {
    let mut stdout = io::stdout().lock();
    let mut any_match = false;
    let mut emitted = false;
    for output in outputs {
        any_match |= output.matched;
        if output.bytes.is_empty() {
            continue;
        }
        if output.heading && emitted {
            stdout.write_all(b"\n")?;
            if let Some(p) = bytes_printed {
                p.fetch_add(1, Ordering::Relaxed);
            }
        }
        let n = output.bytes.len() as u64;
        if let Some(p) = bytes_printed {
            p.fetch_add(n, Ordering::Relaxed);
        }
        stdout.write_all(&output.bytes)?;
        emitted = true;
    }
    Ok(any_match)
}

fn finish_json_run(
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
    let mut stdout = io::stdout().lock();
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

fn json_search_one(
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

#[derive(Clone, Copy)]
struct FileSummary {
    matched: bool,
    count: usize,
}

#[inline]
const fn summary_file_had_positive_hit(mode: SearchMode, r: FileSummary) -> bool {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => r.count > 0,
        SearchMode::FilesWithMatches => r.matched,
        SearchMode::FilesWithoutMatch | SearchMode::Standard | SearchMode::OnlyMatching => false,
    }
}

#[inline]
fn summary_matches_tally(mode: SearchMode, result: FileSummary) -> usize {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => result.count,
        SearchMode::FilesWithMatches => usize::from(result.matched),
        SearchMode::FilesWithoutMatch => usize::from(!result.matched),
        SearchMode::Standard | SearchMode::OnlyMatching => 0,
    }
}

struct SummarySink {
    mode: SearchMode,
    matcher: Option<RegexMatcher>,
    matched: bool,
    count: usize,
}

impl SummarySink {
    const fn new(mode: SearchMode, matcher: Option<RegexMatcher>) -> Self {
        Self {
            mode,
            matcher,
            matched: false,
            count: 0,
        }
    }

    fn finish(self) -> FileSummary {
        FileSummary {
            matched: self.matched,
            count: self.count,
        }
    }
}

impl Sink for SummarySink {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matched = true;
        if self.mode == SearchMode::CountMatches {
            if let Some(ref matcher) = self.matcher {
                let line = mat.bytes();
                let mut n = 0;
                let _ = matcher.find_iter(line, |_| {
                    n += 1;
                    true
                });
                self.count += n;
            }
        } else {
            self.count += 1;
        }
        Ok(matches!(
            self.mode,
            SearchMode::Count | SearchMode::CountMatches
        ))
    }
}

fn write_summary_record(
    out: &mut Vec<u8>,
    output: SearchOutput,
    path: &Path,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    match output.mode {
        SearchMode::Count | SearchMode::CountMatches => {
            if result.count == 0 {
                return Ok(());
            }
            let print_filename = output.lines.filename_mode != FilenameMode::Never;
            if print_filename {
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{}", path.display())?;
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_RESET);
                }
                writeln!(out, ":{}", result.count)?;
            } else {
                writeln!(out, "{}", result.count)?;
            }
            Ok(())
        }
        SearchMode::FilesWithMatches => {
            if result.matched {
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{}", path.display())?;
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_RESET);
                }
                write_line_terminator(out, output.records.null_data);
            }
            Ok(())
        }
        SearchMode::FilesWithoutMatch => {
            if result.matched {
                return Ok(());
            }
            if should_color(output.records) {
                out.extend_from_slice(ANSI_PATH);
            }
            write!(out, "{}", path.display())?;
            if should_color(output.records) {
                out.extend_from_slice(ANSI_RESET);
            }
            write_line_terminator(out, output.records.null_data);
            Ok(())
        }
        SearchMode::Standard | SearchMode::OnlyMatching => unreachable!(),
    }
}

fn write_standard_prefix(
    out: &mut Vec<u8>,
    output: SearchOutput,
    path: &Path,
    line_number: Option<u64>,
    show_line_numbers: bool,
    is_context_line: bool,
) -> io::Result<()> {
    let color = should_color(output.records);
    let print_filename = output.lines.filename_mode != FilenameMode::Never;
    if print_filename {
        if color {
            out.extend_from_slice(ANSI_PATH);
        }
        write!(out, "{}", path.display())?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        let sep = if is_context_line { '-' } else { ':' };
        write!(out, "{sep}")?;
    }
    if show_line_numbers {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        let sep = if is_context_line { '-' } else { ':' };
        write!(out, "{}{}", line_number.unwrap_or(0), sep)?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
    }
    Ok(())
}

#[allow(clippy::match_same_arms)]
const fn mode_is_success(mode: SearchMode, result: FileSummary) -> bool {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => result.count > 0,
        SearchMode::FilesWithMatches => result.matched,
        SearchMode::FilesWithoutMatch => !result.matched,
        SearchMode::Standard | SearchMode::OnlyMatching => result.matched,
    }
}

fn walk_directory_files(root: &Path, follow_links: bool) -> crate::Result<Vec<PathBuf>> {
    let root = root.canonicalize()?;
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(&root)
        .follow_links(follow_links)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(false)
        .git_exclude(false)
        .require_git(false)
        .build();
    for entry in walker {
        let entry = entry.map_err(crate::Error::Ignore)?;
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

/// Collect absolute file paths for each scope under `filter_root` (same walk policy as index build).
fn collect_abs_paths_for_scopes(
    filter_root: &Path,
    scopes: &[PathBuf],
    follow_links: bool,
) -> crate::Result<Vec<PathBuf>> {
    let filter_root = filter_root.canonicalize()?;
    let mut out = Vec::new();
    for scope in scopes {
        let path = if scope.as_os_str().is_empty() {
            filter_root.clone()
        } else {
            filter_root.join(scope)
        };
        if !path.exists() {
            continue;
        }
        let path = path.canonicalize().unwrap_or(path);
        if path.is_file() {
            out.push(path);
        } else if path.is_dir() {
            out.extend(walk_directory_files(&path, follow_links)?);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn prepare_walk_candidates(
    filter_root: &Path,
    abs_paths: &[PathBuf],
    filter: &SearchFilter,
    threshold: usize,
) -> Vec<CandidateInfo> {
    let filter_root = filter_root
        .canonicalize()
        .unwrap_or_else(|_| filter_root.to_path_buf());
    let cap = abs_paths.len();
    let need_rel = filter.needs_rel_str_for_matching();

    if abs_paths.len() >= threshold {
        abs_paths
            .par_iter()
            .enumerate()
            .filter_map(|(id, abs_path)| {
                let rel_path = abs_path
                    .strip_prefix(&filter_root)
                    .unwrap_or(abs_path.as_path())
                    .to_path_buf();
                let rel_str = if need_rel {
                    rel_path.to_string_lossy().replace('\\', "/")
                } else {
                    String::new()
                };
                let info = CandidateInfo {
                    id,
                    rel_path,
                    rel_str,
                    abs_path: abs_path.clone(),
                };
                filter.is_candidate_info(&info).then_some(info)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(i, mut c)| {
                c.id = i;
                c
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(cap);
        for (id, abs_path) in abs_paths.iter().enumerate() {
            let rel_path = abs_path
                .strip_prefix(&filter_root)
                .unwrap_or(abs_path.as_path())
                .to_path_buf();
            let rel_str = if need_rel {
                rel_path.to_string_lossy().replace('\\', "/")
            } else {
                String::new()
            };
            let info = CandidateInfo {
                id,
                rel_path,
                rel_str,
                abs_path: abs_path.clone(),
            };
            if filter.is_candidate_info(&info) {
                let mut c = info;
                c.id = out.len();
                out.push(c);
            }
        }
        out
    }
}

/// # Errors
///
/// Returns an error when canonicalizing `root` or while walking the tree.
pub fn walk_file_paths(root: &Path, follow_links: bool) -> crate::Result<HashSet<PathBuf>> {
    let root = root.canonicalize()?;
    let mut set = HashSet::new();
    let walker = ignore::WalkBuilder::new(&root)
        .follow_links(follow_links)
        .build();
    for entry in walker {
        let entry = entry.map_err(crate::Error::Ignore)?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let display = path.strip_prefix(&root).unwrap_or(path).to_path_buf();
        set.insert(display);
    }
    Ok(set)
}

static PARALLEL_CANDIDATE_THRESHOLD: OnceLock<usize> = OnceLock::new();

/// Minimum candidate file count before `run_index` uses Rayon for filtering and search.
///
/// Value is `8 * effective_threads`, where `effective_threads` is
/// `min(RAYON_NUM_THREADS, available_parallelism)` when `RAYON_NUM_THREADS` is set and valid,
/// else `available_parallelism`. If there is only one effective thread, returns [`usize::MAX`]
/// so the sequential path is always used.
///
/// The result is computed **once per process** on first call (including `RAYON_NUM_THREADS` read).
/// Changing the env var after that has no effect until restart.
#[must_use]
pub fn parallel_candidate_threshold() -> usize {
    *PARALLEL_CANDIDATE_THRESHOLD.get_or_init(|| {
        let cpus = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1);
        let rayon_threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok());
        let effective = rayon_threads
            .filter(|&n| n > 0)
            .map_or(cpus, |rt| rt.min(cpus))
            .max(1);
        if effective <= 1 {
            usize::MAX
        } else {
            effective.saturating_mul(8)
        }
    })
}

#[cfg(test)]
struct CollectSink {
    path: PathBuf,
    only_matching: bool,
    matcher: RegexMatcher,
    matches: Vec<Match>,
}

#[cfg(test)]
impl CollectSink {
    fn new(path: PathBuf, only_matching: bool, matcher: RegexMatcher) -> Self {
        Self {
            path,
            only_matching,
            matcher,
            matches: Vec::new(),
        }
    }

    fn into_matches(self) -> Vec<Match> {
        self.matches
    }
}

#[cfg(test)]
impl grep_searcher::Sink for CollectSink {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
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
