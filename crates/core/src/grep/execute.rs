use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use grep_matcher::{Captures, Matcher};
use grep_printer::{JSON, Stats as JsonStats};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use rayon::prelude::*;

use crate::index::{FileId, IndexId, SearchIndex};
use crate::query::{QueryFlags, QueryPlanner, QuerySpec};

use super::{
    CandidateInfo, ColorChoice, CompiledSearch, FilenameMode, LineStyleFlags, OutputEmission,
    PathDisplay, SearchFilter, SearchMode, SearchOutput, SearchOutputFormat, SearchRecordStyle,
    SearchSeparators, SearchStats, error::SearchError,
};

#[cfg(test)]
use super::{HiddenMode, IgnoreConfig, Match, SearchFilterConfig, VisibilityConfig};

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
fn display_path_for_candidate(
    candidate: &CandidateInfo,
    display: PathDisplay,
    path_separator: Option<u8>,
) -> String {
    let raw = match display {
        PathDisplay::Absolute => candidate.abs_path.display().to_string(),
        PathDisplay::Relative => candidate.rel_path.display().to_string(),
    };
    if let Some(sep) = path_separator {
        let sep_char = sep as char;
        raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())
    } else {
        raw
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

enum ColumnAction {
    /// Line is within the column limit (or no limit set).
    Normal,
    /// Line exceeds the limit and should be silently omitted.
    Omit,
    /// Line exceeds the limit; print a truncated preview.
    Preview,
}

fn check_max_columns(line: &[u8], max_columns: Option<u64>, preview: bool) -> ColumnAction {
    let Some(limit) = max_columns else {
        return ColumnAction::Normal;
    };
    let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    if trimmed.len() as u64 > limit {
        if preview {
            ColumnAction::Preview
        } else {
            ColumnAction::Omit
        }
    } else {
        ColumnAction::Normal
    }
}

/// Truncate a line to `max_columns` bytes, appending " [... omitted end ...]".
fn truncate_line(line: &[u8], max_columns: u64) -> Vec<u8> {
    let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    let limit = usize::try_from(max_columns).unwrap_or(usize::MAX);
    let mut out = Vec::with_capacity(limit.saturating_add(30));
    out.extend_from_slice(&trimmed[..limit.min(trimmed.len())]);
    out.extend_from_slice(b" [... omitted end ...]");
    out.push(b'\n');
    out
}

fn sum_candidate_file_bytes(candidates: &[CandidateInfo]) -> u64 {
    candidates.iter().fold(0u64, |acc, c| {
        acc + std::fs::metadata(&c.abs_path).map_or(0, |m| m.len())
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

fn format_json_summary_line(
    wall: std::time::Duration,
    agg: &JsonStats,
) -> Result<String, SearchError> {
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
    fn build_query_spec(&self) -> QuerySpec<'_> {
        let mut flags = QueryFlags::empty();
        if self.opts.fixed_strings() {
            flags |= QueryFlags::FIXED_STRINGS;
        }
        if self.opts.case_insensitive() {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if self.opts.word_regexp() {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if self.opts.line_regexp() {
            flags |= QueryFlags::LINE_REGEXP;
        }
        if self.opts.invert_match() {
            flags |= QueryFlags::INVERT_MATCH;
        }
        QuerySpec {
            patterns: &self.patterns,
            flags,
        }
    }

    /// Execute a search over one or more indexes and print results to stdout.
    ///
    /// All indexes must share the same corpus root; scope paths, exclude paths,
    /// and relative-path display are resolved against a single [`SearchFilter`].
    ///
    /// # Errors
    ///
    /// Returns an error if the matcher cannot be built or stdout cannot be written.
    pub fn run_indexes(
        &self,
        indexes: &[&dyn SearchIndex],
        filter: &SearchFilter,
        output: SearchOutput,
        separators: &SearchSeparators,
    ) -> crate::Result<bool> {
        self.run_indexes_impl(indexes, filter, output, separators, None)
    }

    /// Like [`Self::run_indexes`], but fills `stats` with search counters.
    ///
    /// # Errors
    ///
    /// Same as [`Self::run_indexes`].
    pub fn run_indexes_with_stats(
        &self,
        indexes: &[&dyn SearchIndex],
        filter: &SearchFilter,
        output: SearchOutput,
        separators: &SearchSeparators,
        stats: &mut SearchStats,
    ) -> crate::Result<bool> {
        self.run_indexes_impl(indexes, filter, output, separators, Some(stats))
    }

    fn run_indexes_impl(
        &self,
        indexes: &[&dyn SearchIndex],
        filter: &SearchFilter,
        output: SearchOutput,
        separators: &SearchSeparators,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }
        if indexes.is_empty() {
            if let Some(s) = stats {
                *s = SearchStats::default();
            }
            return Ok(false);
        }

        let spec = self.build_query_spec();
        let needs_all_files = matches!(
            output.mode,
            SearchMode::Count | SearchMode::FilesWithoutMatch
        );
        let use_indexes = !needs_all_files && QueryPlanner::should_use_indexes(&spec);

        let threshold = parallel_candidate_threshold();
        let candidates = if use_indexes {
            Self::prepare_index_candidates(indexes, filter, threshold, &spec)
        } else {
            Self::prepare_all_files(indexes, filter, threshold, &spec)
        };
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
                _ => Err(SearchError::JsonOutputIncompatibleMode.into()),
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
                separators,
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

    fn prepare_all_files(
        indexes: &[&dyn SearchIndex],
        filter: &SearchFilter,
        threshold: usize,
        _spec: &QuerySpec<'_>,
    ) -> Vec<CandidateInfo> {
        let need_rel = filter.needs_rel_str_for_matching();
        let max_fs = filter.max_filesize();
        let max_d = filter.max_depth();

        let exceeds_depth = |rel: &Path| -> bool {
            max_d.is_some_and(|d| rel.components().count().saturating_sub(1) > d)
        };

        let all_ids: Vec<(IndexId, FileId)> = indexes
            .iter()
            .enumerate()
            .flat_map(|(i, idx)| {
                let index_id = IndexId::new(i);
                (0..idx.file_count()).map(move |fid| (index_id, FileId::new(fid)))
            })
            .collect();

        if all_ids.len() >= threshold {
            all_ids
                .into_par_iter()
                .filter_map(|(index_id, file_id)| {
                    let idx = *indexes.get(index_id.get())?;
                    let rel_path = idx.file_path(file_id)?.to_path_buf();
                    if exceeds_depth(&rel_path) {
                        return None;
                    }
                    let rel_str = if need_rel {
                        rel_path.to_string_lossy().replace('\\', "/")
                    } else {
                        String::new()
                    };
                    let abs_path = idx.file_abs_path(file_id)?;
                    if max_fs.is_some_and(|limit| {
                        std::fs::metadata(&abs_path).is_ok_and(|m| m.len() > limit)
                    }) {
                        return None;
                    }
                    let info = CandidateInfo {
                        rel_path,
                        rel_str,
                        abs_path,
                    };
                    filter.is_candidate_info(&info).then_some(info)
                })
                .collect()
        } else {
            let mut out = Vec::with_capacity(all_ids.len());
            out.extend(all_ids.into_iter().filter_map(|(index_id, file_id)| {
                let idx = *indexes.get(index_id.get())?;
                let rel_path = idx.file_path(file_id)?.to_path_buf();
                if exceeds_depth(&rel_path) {
                    return None;
                }
                let rel_str = if need_rel {
                    rel_path.to_string_lossy().replace('\\', "/")
                } else {
                    String::new()
                };
                let abs_path = idx.file_abs_path(file_id)?;
                if max_fs.is_some_and(|limit| {
                    std::fs::metadata(&abs_path).is_ok_and(|m| m.len() > limit)
                }) {
                    return None;
                }
                let info = CandidateInfo {
                    rel_path,
                    rel_str,
                    abs_path,
                };
                filter.is_candidate_info(&info).then_some(info)
            }));
            out
        }
    }

    fn prepare_index_candidates(
        indexes: &[&dyn SearchIndex],
        filter: &SearchFilter,
        threshold: usize,
        spec: &QuerySpec<'_>,
    ) -> Vec<CandidateInfo> {
        let need_rel = filter.needs_rel_str_for_matching();
        let max_fs = filter.max_filesize();
        let max_d = filter.max_depth();

        let exceeds_depth = |rel: &Path| -> bool {
            max_d.is_some_and(|d| rel.components().count().saturating_sub(1) > d)
        };

        let candidate_ids: Vec<(IndexId, FileId)> = indexes
            .iter()
            .enumerate()
            .flat_map(|(i, idx)| {
                let index_id = IndexId::new(i);
                idx.candidates(spec)
                    .into_iter()
                    .map(move |fid| (index_id, fid))
            })
            .collect();

        if candidate_ids.len() >= threshold {
            candidate_ids
                .into_par_iter()
                .filter_map(|(index_id, file_id)| {
                    let idx = *indexes.get(index_id.get())?;
                    let rel_path = idx.file_path(file_id)?.to_path_buf();
                    if exceeds_depth(&rel_path) {
                        return None;
                    }
                    let rel_str = if need_rel {
                        rel_path.to_string_lossy().replace('\\', "/")
                    } else {
                        String::new()
                    };
                    let abs_path = idx.file_abs_path(file_id)?;
                    if max_fs.is_some_and(|limit| {
                        std::fs::metadata(&abs_path).is_ok_and(|m| m.len() > limit)
                    }) {
                        return None;
                    }
                    let info = CandidateInfo {
                        rel_path,
                        rel_str,
                        abs_path,
                    };
                    filter.is_candidate_info(&info).then_some(info)
                })
                .collect()
        } else {
            let mut out = Vec::with_capacity(candidate_ids.len());
            out.extend(candidate_ids.into_iter().filter_map(|(index_id, file_id)| {
                let idx = *indexes.get(index_id.get())?;
                let rel_path = idx.file_path(file_id)?.to_path_buf();
                if exceeds_depth(&rel_path) {
                    return None;
                }
                let rel_str = if need_rel {
                    rel_path.to_string_lossy().replace('\\', "/")
                } else {
                    String::new()
                };
                let abs_path = idx.file_abs_path(file_id)?;
                if max_fs.is_some_and(|limit| {
                    std::fs::metadata(&abs_path).is_ok_and(|m| m.len() > limit)
                }) {
                    return None;
                }
                let info = CandidateInfo {
                    rel_path,
                    rel_str,
                    abs_path,
                };
                filter.is_candidate_info(&info).then_some(info)
            }));
            out
        }
    }

    /// Search by walking the filesystem under `filter_root` (no trigram index).
    ///
    /// Candidate paths are discovered the same way as index build: all files under scoped paths,
    /// then [`SearchFilter`] is applied. Ignores [`CandidatePlan`] narrowing (full file list).
    ///
    /// # Errors
    ///
    /// Returns an error if the matcher cannot be built or stdout cannot be written.
    pub fn run_walk(
        &self,
        filter_root: &Path,
        filter: &SearchFilter,
        output: SearchOutput,
        separators: &SearchSeparators,
    ) -> crate::Result<bool> {
        self.run_walk_impl(filter_root, filter, output, separators, None)
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
        separators: &SearchSeparators,
        stats: &mut SearchStats,
    ) -> crate::Result<bool> {
        self.run_walk_impl(filter_root, filter, output, separators, Some(stats))
    }

    fn run_walk_impl(
        &self,
        filter_root: &Path,
        filter: &SearchFilter,
        output: SearchOutput,
        separators: &SearchSeparators,
        stats: Option<&mut SearchStats>,
    ) -> crate::Result<bool> {
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }

        let abs_paths = collect_abs_paths_for_scopes(
            filter_root,
            filter.scopes(),
            filter.follow_links(),
            filter.one_file_system(),
            filter.max_depth(),
            filter.max_filesize(),
        )?;
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
                _ => Err(SearchError::JsonOutputIncompatibleMode.into()),
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
                separators,
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
        separators: &SearchSeparators,
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
                            separators,
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

        self.run_standard_capped_with_info(candidates, matcher, output, separators, stats)
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
        separators: &SearchSeparators,
        stats: StatsCollection<'_>,
    ) -> crate::Result<bool> {
        self.with_cached_searcher(
            output.lines.line_number(),
            self.opts.max_results,
            |searcher| {
                let mut any_match = false;
                let mut out = Vec::new();
                for candidate in candidates {
                    let heading =
                        output.lines.heading() && output.lines.filename_mode != FilenameMode::Never;
                    let mut sink_output = output;
                    if heading {
                        sink_output.lines.filename_mode = FilenameMode::Never;
                    }
                    let mut bytes = Vec::new();
                    let display = display_path_for_candidate(
                        candidate,
                        output.lines.path_display,
                        output.records.path_separator,
                    );
                    let mut sink = StandardSink::new(
                        matcher,
                        sink_output,
                        display,
                        &mut bytes,
                        separators,
                        self.opts.replace.as_deref(),
                        SinkConfig {
                            before_context: self.opts.before_context,
                            after_context: self.opts.after_context,
                        },
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
                        let display = display_path_for_candidate(
                            candidate,
                            output.lines.path_display,
                            output.records.path_separator,
                        );
                        write!(out, "{display}")?;
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
                let display = display_path_for_candidate(
                    candidate,
                    output.lines.path_display,
                    output.records.path_separator,
                );
                write_summary_record(&mut out, output, &display, result)?;
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
    pub(crate) fn collect_index_matches(
        &self,
        index: &dyn SearchIndex,
    ) -> crate::Result<Vec<Match>> {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            ..SearchFilterConfig::default()
        };
        let filter = SearchFilter::new(&config, index.root())?;
        let spec = self.build_query_spec();
        let candidate_ids = index.candidates(&spec);
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
        index: &dyn SearchIndex,
        filter: &SearchFilter,
        candidate_ids: &[FileId],
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
                    index.root().join(candidate),
                    self.opts.only_matching(),
                    matcher.clone(),
                );
                let _ = searcher.search_path(matcher, index.root().join(candidate), &mut sink);
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
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    output: SearchOutput,
    separators: &'a SearchSeparators,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    replace: Option<String>,
    sink_config: SinkConfig,
}

impl<'a> StandardWorker<'a> {
    fn new(
        search: &CompiledSearch,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        separators: &'a SearchSeparators,
        match_counter: Option<&'a AtomicUsize>,
        files_with_matches: Option<&'a AtomicUsize>,
    ) -> Self {
        Self {
            searcher: search.build_searcher(
                output.lines.line_number(),
                search.opts.max_results,
                true,
            ),
            matcher,
            output,
            separators,
            bytes: Vec::new(),
            match_counter,
            files_with_matches,
            replace: search.opts.replace.clone(),
            sink_config: SinkConfig {
                before_context: search.opts.before_context,
                after_context: search.opts.after_context,
            },
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
            let heading = self.output.lines.heading()
                && self.output.lines.filename_mode != FilenameMode::Never;
            let mut sink_output = self.output;
            if heading {
                sink_output.lines.filename_mode = FilenameMode::Never;
            }
            let display = display_path_for_candidate(
                candidate,
                self.output.lines.path_display,
                self.output.records.path_separator,
            );
            let mut sink = StandardSink::new(
                self.matcher,
                sink_output,
                display,
                &mut self.bytes,
                self.separators,
                self.replace.as_deref(),
                self.sink_config,
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
                    && self.output.lines.heading()
                    && self.output.lines.filename_mode != FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet
                {
                    let mut out = Vec::new();
                    if should_color(self.output.records) {
                        out.extend_from_slice(ANSI_PATH);
                    }
                    let display = display_path_for_candidate(
                        candidate,
                        self.output.lines.path_display,
                        self.output.records.path_separator,
                    );
                    let _ = write!(out, "{display}");
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
                    && self.output.lines.heading()
                    && self.output.lines.filename_mode != FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet,
            },
            json_stats: None,
        }
    }
}

fn apply_replace(matcher: &RegexMatcher, line: &[u8], replacement: &str) -> String {
    let Ok(mut caps) = matcher.new_captures() else {
        return String::from_utf8_lossy(line).into_owned();
    };
    let mut dst = Vec::new();
    let _ = matcher.replace_with_captures(line, &mut caps, &mut dst, |caps, dst| {
        caps.interpolate(
            |name| matcher.capture_index(name),
            line,
            replacement.as_bytes(),
            dst,
        );
        true
    });
    String::from_utf8_lossy(&dst).into_owned()
}

#[derive(Clone, Copy)]
struct SinkConfig {
    before_context: usize,
    after_context: usize,
}

struct StandardSink<'a> {
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    show_line_numbers: bool,
    display_path: String,
    bytes: &'a mut Vec<u8>,
    separators: &'a SearchSeparators,
    matched: bool,
    match_count: usize,
    replace: Option<&'a str>,
    trim: bool,
}

impl<'a> StandardSink<'a> {
    const fn new(
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        display_path: String,
        bytes: &'a mut Vec<u8>,
        separators: &'a SearchSeparators,
        replace: Option<&'a str>,
        config: SinkConfig,
    ) -> Self {
        Self {
            matcher,
            output,
            show_line_numbers: output.lines.line_number()
                || config.before_context > 0
                || config.after_context > 0,
            display_path,
            bytes,
            separators,
            matched: false,
            match_count: 0,
            replace,
            trim: output.lines.trim(),
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
            return Ok(self.handle_only_matching(mat));
        }

        let line_bytes = mat.bytes();
        if self.handle_max_columns(line_bytes, mat)? {
            return Ok(true);
        }

        let col = self.compute_first_column(mat);
        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path.as_str(),
            mat.line_number(),
            self.show_line_numbers,
            &PrefixCtx {
                is_context_line: false,
                column: col,
                separators: self.separators,
            },
            if self.output.lines.byte_offset() {
                Some(mat.absolute_byte_offset())
            } else {
                None
            },
        )?;
        self.write_line_content(mat.bytes())
    }

    fn context(&mut self, _: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(self.output.mode, SearchMode::OnlyMatching) {
            return Ok(true);
        }
        let ctx_bytes = ctx.bytes();
        let max_cols = self.output.lines.max_columns;
        let preview = self.output.lines.max_columns_preview;
        match check_max_columns(ctx_bytes, max_cols, preview) {
            ColumnAction::Omit => return Ok(true),
            ColumnAction::Preview => {
                write_standard_prefix(
                    self.bytes,
                    self.output,
                    self.display_path.as_str(),
                    ctx.line_number(),
                    self.show_line_numbers,
                    &PrefixCtx {
                        is_context_line: true,
                        column: None,
                        separators: self.separators,
                    },
                    if self.output.lines.byte_offset() {
                        Some(ctx.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = truncate_line(ctx_bytes, max_cols.unwrap_or(0));
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            ColumnAction::Normal => {}
        }
        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path.as_str(),
            ctx.line_number(),
            self.show_line_numbers,
            &PrefixCtx {
                is_context_line: true,
                column: None,
                separators: self.separators,
            },
            if self.output.lines.byte_offset() {
                Some(ctx.absolute_byte_offset())
            } else {
                None
            },
        )?;
        let line_bytes = ctx.bytes();
        if self.trim {
            let s = String::from_utf8_lossy(line_bytes);
            let trimmed = s.trim_start();
            self.bytes.write_all(trimmed.as_bytes())?;
            if !trimmed.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else {
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(b"\n") {
                self.bytes.write_all(b"\n")?;
            }
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
        if let Some(ref sep) = self.separators.context_separator {
            self.bytes.write_all(sep)?;
            self.bytes.write_all(b"\n")?;
        }
        Ok(true)
    }
}

impl StandardSink<'_> {
    fn handle_only_matching(&mut self, mat: &SinkMatch<'_>) -> bool {
        let show_column = self.output.lines.flags.contains(LineStyleFlags::COLUMN);
        let line_number = mat.line_number();
        let line = mat.bytes();
        let byte_offset = mat.absolute_byte_offset();
        let _ = self.matcher.find_iter(line, |m: grep_matcher::Match| {
            let col = if show_column {
                Some(m.start() + 1)
            } else {
                None
            };
            let matched_slice = &line[m.start()..m.end()];
            let text = if let Some(rep) = self.replace {
                apply_replace(self.matcher, matched_slice, rep)
            } else {
                String::from_utf8_lossy(matched_slice).into_owned()
            };
            let _ = write_standard_prefix(
                self.bytes,
                self.output,
                self.display_path.as_str(),
                line_number,
                self.show_line_numbers,
                &PrefixCtx {
                    is_context_line: false,
                    column: col,
                    separators: self.separators,
                },
                if self.output.lines.byte_offset() {
                    Some(byte_offset)
                } else {
                    None
                },
            );
            let _ = self.bytes.write_all(text.as_bytes());
            let _ = self.bytes.write_all(b"\n");
            true
        });
        true
    }

    fn handle_max_columns(
        &mut self,
        line_bytes: &[u8],
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let max_cols = self.output.lines.max_columns;
        let preview = self.output.lines.max_columns_preview;
        match check_max_columns(line_bytes, max_cols, preview) {
            ColumnAction::Omit => return Ok(true),
            ColumnAction::Preview => {
                write_standard_prefix(
                    self.bytes,
                    self.output,
                    self.display_path.as_str(),
                    mat.line_number(),
                    self.show_line_numbers,
                    &PrefixCtx {
                        is_context_line: false,
                        column: None,
                        separators: self.separators,
                    },
                    if self.output.lines.byte_offset() {
                        Some(mat.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = truncate_line(line_bytes, max_cols.unwrap_or(0));
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            ColumnAction::Normal => {}
        }
        Ok(false)
    }

    fn compute_first_column(&self, mat: &SinkMatch<'_>) -> Option<usize> {
        if !self.output.lines.flags.contains(LineStyleFlags::COLUMN) {
            return None;
        }
        let line = mat.bytes();
        let mut first_col = None;
        let _ = self.matcher.find_iter(line, |m: grep_matcher::Match| {
            first_col = Some(m.start() + 1);
            false
        });
        first_col
    }

    fn write_line_content(&mut self, line_bytes: &[u8]) -> Result<bool, io::Error> {
        if let Some(rep) = self.replace {
            let text = apply_replace(self.matcher, line_bytes, rep);
            self.bytes.write_all(text.as_bytes())?;
            if !text.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else if self.trim {
            let trimmed = String::from_utf8_lossy(line_bytes);
            let trimmed = trimmed.trim_start();
            self.bytes.write_all(trimmed.as_bytes())?;
            if !trimmed.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else {
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(b"\n") {
                self.bytes.write_all(b"\n")?;
            }
        }
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
        let display = display_path_for_candidate(
            candidate,
            output.lines.path_display,
            output.records.path_separator,
        );
        let _ = write_summary_record(&mut bytes, output, &display, result);
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
    display_path: &str,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    match output.mode {
        SearchMode::Count | SearchMode::CountMatches => {
            if result.count == 0 && !output.include_zero {
                return Ok(());
            }
            let print_filename = output.lines.filename_mode != FilenameMode::Never;
            if print_filename {
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{display_path}")?;
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
                write!(out, "{display_path}")?;
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
            write!(out, "{display_path}")?;
            if should_color(output.records) {
                out.extend_from_slice(ANSI_RESET);
            }
            write_line_terminator(out, output.records.null_data);
            Ok(())
        }
        SearchMode::Standard | SearchMode::OnlyMatching => unreachable!(),
    }
}

struct PrefixCtx<'a> {
    is_context_line: bool,
    column: Option<usize>,
    separators: &'a SearchSeparators,
}

fn write_standard_prefix(
    out: &mut Vec<u8>,
    output: SearchOutput,
    path: &str,
    line_number: Option<u64>,
    show_line_numbers: bool,
    prefix: &PrefixCtx<'_>,
    byte_offset: Option<u64>,
) -> io::Result<()> {
    let color = should_color(output.records);
    let print_filename = output.lines.filename_mode != FilenameMode::Never;
    let field_sep = if prefix.is_context_line {
        &prefix.separators.field_context_separator
    } else {
        &prefix.separators.field_match_separator
    };
    if print_filename {
        if color {
            out.extend_from_slice(ANSI_PATH);
        }
        write!(out, "{path}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if show_line_numbers {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        write!(out, "{}", line_number.unwrap_or(0))?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if let Some(col) = prefix.column {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        write!(out, "{col}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if let Some(offset) = byte_offset {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        let sep = if prefix.is_context_line { '-' } else { ':' };
        write!(out, "{offset}{sep}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
    }
    Ok(())
}

const fn mode_is_success(mode: SearchMode, result: FileSummary) -> bool {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => result.count > 0,
        SearchMode::FilesWithMatches | SearchMode::Standard | SearchMode::OnlyMatching => {
            result.matched
        }
        SearchMode::FilesWithoutMatch => !result.matched,
    }
}

fn walk_directory_files(
    root: &Path,
    follow_links: bool,
    one_file_system: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
) -> crate::Result<Vec<PathBuf>> {
    let root = root.canonicalize()?;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .follow_links(follow_links)
        .same_file_system(one_file_system)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(false)
        .git_exclude(false)
        .require_git(false);
    if let Some(d) = max_depth {
        // User-facing semantics: 0 = root files only, 1 = root + one subdir level.
        // WalkBuilder counts depth from the root dir entry (depth 0), so files at
        // the root are depth 1. Shift by +1 to match ripgrep's convention.
        builder.max_depth(Some(d + 1));
    }
    let mut out = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(crate::Error::Ignore)?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if let Some(limit) = max_filesize {
            let skip = entry.metadata().is_ok_and(|m| m.len() > limit);
            if skip {
                continue;
            }
        }
        out.push(entry.path().to_path_buf());
    }
    Ok(out)
}

/// Collect absolute file paths for each scope under `filter_root` (same walk policy as index build).
fn collect_abs_paths_for_scopes(
    filter_root: &Path,
    scopes: &[PathBuf],
    follow_links: bool,
    one_file_system: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
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
            out.extend(walk_directory_files(
                &path,
                follow_links,
                one_file_system,
                max_depth,
                max_filesize,
            )?);
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
            .filter_map(|abs_path| {
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
                    rel_path,
                    rel_str,
                    abs_path: abs_path.clone(),
                };
                filter.is_candidate_info(&info).then_some(info)
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(cap);
        for abs_path in abs_paths {
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
                rel_path,
                rel_str,
                abs_path: abs_path.clone(),
            };
            if filter.is_candidate_info(&info) {
                out.push(info);
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
        let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
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

#[cfg(test)]
mod execute_helper_tests {
    use super::*;

    #[test]
    fn should_color_never_returns_false() {
        let records = SearchRecordStyle::default();
        let records_never = SearchRecordStyle {
            color: ColorChoice::Never,
            ..records
        };
        assert!(!should_color(records_never));
    }

    #[test]
    fn should_color_always_returns_true() {
        let records = SearchRecordStyle::default();
        let records_always = SearchRecordStyle {
            color: ColorChoice::Always,
            ..records
        };
        assert!(should_color(records_always));
    }

    #[test]
    fn display_path_returns_relative_by_default() {
        let candidate = CandidateInfo {
            rel_path: PathBuf::from("src/lib.rs"),
            rel_str: "src/lib.rs".to_string(),
            abs_path: PathBuf::from("/root/src/lib.rs"),
        };
        let display = display_path_for_candidate(&candidate, PathDisplay::Relative, None);
        assert_eq!(display, "src/lib.rs");
    }

    #[test]
    fn display_path_returns_absolute_when_requested() {
        let candidate = CandidateInfo {
            rel_path: PathBuf::from("src/lib.rs"),
            rel_str: "src/lib.rs".to_string(),
            abs_path: PathBuf::from("/root/src/lib.rs"),
        };
        let display = display_path_for_candidate(&candidate, PathDisplay::Absolute, None);
        assert_eq!(display, "/root/src/lib.rs");
    }

    #[test]
    fn display_path_applies_custom_separator() {
        let candidate = CandidateInfo {
            rel_path: PathBuf::from("src/lib.rs"),
            rel_str: "src/lib.rs".to_string(),
            abs_path: PathBuf::from("/root/src/lib.rs"),
        };
        let display = display_path_for_candidate(&candidate, PathDisplay::Relative, Some(b'/'));
        assert_eq!(display, "src/lib.rs");
    }

    #[test]
    fn write_line_terminator_writes_newline() {
        let mut out = Vec::new();
        write_line_terminator(&mut out, false);
        assert_eq!(out, b"\n");
    }

    #[test]
    fn write_line_terminator_writes_nul() {
        let mut out = Vec::new();
        write_line_terminator(&mut out, true);
        assert_eq!(out, b"\0");
    }

    #[test]
    fn check_max_columns_returns_normal_when_within_limit() {
        let action = check_max_columns(b"short line\n", Some(100), false);
        assert!(matches!(action, ColumnAction::Normal));
    }

    #[test]
    fn check_max_columns_returns_omit_when_over_limit_no_preview() {
        let action = check_max_columns(b"this is a very long line\n", Some(5), false);
        assert!(matches!(action, ColumnAction::Omit));
    }

    #[test]
    fn check_max_columns_returns_preview_when_over_limit_with_preview() {
        let action = check_max_columns(b"this is a very long line\n", Some(5), true);
        assert!(matches!(action, ColumnAction::Preview));
    }

    #[test]
    fn check_max_columns_returns_normal_when_no_limit_set() {
        let action = check_max_columns(b"any length\n", None, false);
        assert!(matches!(action, ColumnAction::Normal));
    }

    #[test]
    fn truncate_line_trims_line_ending_and_appends_omission() {
        let result = truncate_line(b"this is a very long line\n", 10);
        assert!(result.ends_with(b" [... omitted end ...]\n"));
        let content = String::from_utf8_lossy(&result);
        assert!(content.starts_with("this is a "));
    }

    #[test]
    fn truncate_line_handles_line_without_ending() {
        let result = truncate_line(b"short", 3);
        assert!(result.ends_with(b" [... omitted end ...]\n"));
    }

    #[test]
    fn sum_candidate_file_bytes_sums_existing_files() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let path1 = tmp.path().join("a.txt");
        let path2 = tmp.path().join("b.txt");
        std::fs::write(&path1, "hello").expect("write a");
        std::fs::write(&path2, "world!!").expect("write b");

        let candidates = vec![
            CandidateInfo {
                rel_path: PathBuf::from("a.txt"),
                rel_str: "a.txt".to_string(),
                abs_path: path1,
            },
            CandidateInfo {
                rel_path: PathBuf::from("b.txt"),
                rel_str: "b.txt".to_string(),
                abs_path: path2,
            },
        ];
        let total = sum_candidate_file_bytes(&candidates);
        assert_eq!(total, 12);
    }

    #[test]
    fn sum_candidate_file_bytes_treats_missing_files_as_zero() {
        let candidates = vec![CandidateInfo {
            rel_path: PathBuf::from("nonexistent.txt"),
            rel_str: "nonexistent.txt".to_string(),
            abs_path: PathBuf::from("/nonexistent.txt"),
        }];
        let total = sum_candidate_file_bytes(&candidates);
        assert_eq!(total, 0);
    }

    #[test]
    fn null_writer_returns_buf_len() {
        let mut w = NullWriter;
        let n = w.write(b"hello").expect("write");
        assert_eq!(n, 5);
    }

    #[test]
    fn null_writer_flush_returns_ok() {
        let mut w = NullWriter;
        w.flush().expect("flush");
    }
}
