use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use grep_printer::Stats as JsonStats;
use rayon::prelude::*;
use sift_core::grep::CompiledQuery;
use sift_core::grep::Inputs;
use sift_core::grep::Query;
use sift_core::grep::Report;
use sift_core::grep::Stats;

use crate::format::stats::StatsExt;

use crate::format::collection::PrintExtras;
use crate::format::output::mode::PrintMode;
use crate::format::output::style::PrintSeparators;
use crate::format::output::{PrintFormat, PrintSpec};
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use crate::format::stats::TextStatsCounters;

pub struct SearchPrinter<'a> {
    query: &'a Query,
    compiled: &'a CompiledQuery,
    print_spec: PrintSpec,
    separators: &'a PrintSeparators,
    extras: PrintExtras,
}

impl<'a> SearchPrinter<'a> {
    #[must_use]
    pub const fn new(
        query: &'a Query,
        compiled: &'a CompiledQuery,
        print_spec: PrintSpec,
        separators: &'a PrintSeparators,
        extras: PrintExtras,
    ) -> Self {
        Self {
            query,
            compiled,
            print_spec,
            separators,
            extras,
        }
    }

    /// Scan inputs and write formatted output to stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if search or output formatting fails.
    pub fn print(self, inputs: &Inputs<'_>) -> sift_core::Result<Report> {
        if inputs.is_empty() {
            return Ok(Report {
                matched: false,
                matches: Vec::new(),
                hit_paths: Vec::new(),
                stats: self.extras.collect_stats().then_some(Stats::default()),
            });
        }

        let search_start = Instant::now();
        let (matched, stats, hit_paths) = match self.print_spec.format {
            PrintFormat::Json => match self.print_spec.mode {
                PrintMode::Standard | PrintMode::OnlyMatching => {
                    let mut stats = self.extras.collect_stats().then_some(Stats::default());
                    let matched = self.run_json(inputs, search_start, stats.as_mut())?;
                    (matched, stats, Vec::new())
                }
                _ => return Err(sift_core::GrepError::JsonOutputIncompatibleMode.into()),
            },
            PrintFormat::Text => self.run_text(inputs, search_start)?,
        };

        Ok(Report {
            matched,
            matches: Vec::new(),
            hit_paths,
            stats,
        })
    }

    fn run_text(
        &self,
        inputs: &Inputs<'_>,
        search_start: Instant,
    ) -> sift_core::Result<(bool, Option<Stats>, Vec<PathBuf>)> {
        let extras = self.extras;
        let counters = TextStatsCounters::new(extras.collect_stats());

        let (matched, hit_paths) = match self.print_spec.mode {
            PrintMode::Standard | PrintMode::OnlyMatching => {
                let stop = AtomicBool::new(false);
                let mut files = Vec::with_capacity(inputs.len());
                inputs
                    .as_slice()
                    .par_iter()
                    .map_init(
                        || {
                            crate::format::sink::standard::LinePrinter::new(
                                self.query,
                                self.compiled,
                                &self.print_spec,
                                self.separators,
                                &counters,
                                extras,
                            )
                        },
                        |reporter, input| reporter.report(input, &stop),
                    )
                    .collect_into_vec(&mut files);
                Self::flush_text(
                    files,
                    extras,
                    counters.bytes_printed(),
                    self.print_spec.records.buffering,
                )?
            }
            PrintMode::Count
            | PrintMode::CountMatches
            | PrintMode::FilesWithMatches
            | PrintMode::FilesWithoutMatch => {
                let stop = AtomicBool::new(false);
                let mut files = Vec::with_capacity(inputs.len());
                inputs
                    .as_slice()
                    .par_iter()
                    .map_init(
                        || {
                            crate::format::sink::summary::AggregatePrinter::new(
                                self.query,
                                self.compiled,
                                self.print_spec.clone(),
                                &counters,
                                extras,
                            )
                        },
                        |reporter, input| reporter.report(input, &stop),
                    )
                    .collect_into_vec(&mut files);
                Self::flush_summary(
                    files,
                    extras,
                    counters.bytes_printed(),
                    self.print_spec.records.buffering,
                )?
            }
        };

        let bytes_searched = if extras.collect_stats() {
            inputs.byte_count()
        } else {
            0
        };
        let stats = counters.finish(inputs.len(), bytes_searched, search_start.elapsed());

        Ok((matched, stats, hit_paths))
    }

    fn flush_text(
        files: Vec<FileResult>,
        extras: PrintExtras,
        bytes_printed: Option<&std::sync::atomic::AtomicU64>,
        buffering: crate::format::output::style::OutputBuffering,
    ) -> sift_core::Result<(bool, Vec<PathBuf>)> {
        let mut hit_paths = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        for file in files {
            if extras.collect_hits()
                && let Some(hit) = file.hit
            {
                hit_paths.push(hit);
            }
            outputs.push(file.output);
        }
        let matched = ChunkOutput::flush_all(outputs, bytes_printed, buffering)?;
        Ok((matched, hit_paths))
    }

    fn flush_summary(
        files: Vec<FileResult>,
        extras: PrintExtras,
        bytes_printed: Option<&std::sync::atomic::AtomicU64>,
        buffering: crate::format::output::style::OutputBuffering,
    ) -> sift_core::Result<(bool, Vec<PathBuf>)> {
        let mut hit_paths = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        let mut matched = false;
        for file in files {
            if extras.collect_hits()
                && let Some(hit) = file.hit
            {
                hit_paths.push(hit);
            }
            matched |= file.output.matched;
            outputs.push(file.output);
        }
        ChunkOutput::flush_all(outputs, bytes_printed, buffering)?;
        Ok((matched, hit_paths))
    }

    fn run_json(
        &self,
        inputs: &Inputs<'_>,
        search_start: Instant,
        stats: Option<&mut Stats>,
    ) -> sift_core::Result<bool> {
        let stop = AtomicBool::new(false);
        let mut files = Vec::with_capacity(inputs.len());
        inputs
            .as_slice()
            .par_iter()
            .map_init(
                || {
                    crate::format::sink::json::JsonPrinter::new(
                        self.query,
                        self.compiled,
                        self.print_spec.clone(),
                    )
                },
                |reporter, input| reporter.report(input, &stop),
            )
            .collect_into_vec(&mut files);

        let mut merged = JsonStats::new();
        let mut outputs = Vec::with_capacity(files.len());
        for file in files {
            if let Some(stats) = file.json_stats {
                merged += &stats;
            }
            outputs.push(file.output);
        }
        let matched = ChunkOutput::flush_all(outputs, None, self.print_spec.records.buffering)?;
        let summary_line =
            crate::format::sink::json::JsonPrinter::summary_line(search_start.elapsed(), &merged)?;
        let summary_bytes = summary_line.len() as u64 + 1;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(summary_line.as_bytes())?;
        stdout.write_all(b"\n")?;
        if let Some(stats) = stats {
            stats.fill_from_json(
                &merged,
                inputs.len(),
                inputs.as_slice().iter().map(Input::byte_len).sum(),
                search_start.elapsed(),
                summary_bytes,
            );
        }
        Ok(matched)
    }
}

use sift_core::grep::Input;
