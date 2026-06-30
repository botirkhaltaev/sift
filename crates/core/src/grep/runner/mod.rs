use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use grep_printer::Stats as JsonStats;
use rayon::prelude::*;

use crate::grep::input::{GrepInput, GrepInputs};
use crate::grep::output::GrepOutputFormat;
use crate::grep::output::mode::GrepMode;
use crate::grep::output::style::GrepSeparators;
use crate::grep::query::{CompiledGrepQuery, GrepQuery};
use crate::grep::sink::FileReporter;
use crate::grep::sink::result::{ChunkOutput, FileResult};
use crate::grep::stats::{GrepStats, TextStatsCounters};
use crate::grep::{GrepCollection, GrepError, GrepOutcome, GrepOutput, GrepReport};

pub(super) struct GrepRunner<'a> {
    query: &'a GrepQuery,
    compiled: &'a CompiledGrepQuery,
    output: GrepOutput,
    separators: &'a GrepSeparators,
    collect: GrepCollection,
}

impl<'a> GrepRunner<'a> {
    pub(crate) const fn new(
        query: &'a GrepQuery,
        compiled: &'a CompiledGrepQuery,
        output: GrepOutput,
        separators: &'a GrepSeparators,
        collect: GrepCollection,
    ) -> Self {
        Self {
            query,
            compiled,
            output,
            separators,
            collect,
        }
    }

    pub(crate) fn run(self, inputs: &GrepInputs<'_>) -> crate::Result<GrepReport> {
        if inputs.is_empty() {
            return Ok(GrepReport {
                outcome: GrepOutcome {
                    matched: false,
                    stats: self.collect.stats.then_some(GrepStats::default()),
                },
                hits: Vec::new(),
            });
        }

        let search_start = Instant::now();
        let (matched, stats, hits) = match self.output.format {
            GrepOutputFormat::Json => match self.output.mode {
                GrepMode::Standard | GrepMode::OnlyMatching => {
                    let mut stats = self.collect.stats.then_some(GrepStats::default());
                    let matched = self.run_json(inputs, search_start, stats.as_mut())?;
                    (matched, stats, Vec::new())
                }
                _ => return Err(GrepError::JsonOutputIncompatibleMode.into()),
            },
            GrepOutputFormat::Text => self.run_text(inputs, search_start)?,
        };

        Ok(GrepReport {
            outcome: GrepOutcome { matched, stats },
            hits,
        })
    }

    fn run_text(
        &self,
        inputs: &GrepInputs<'_>,
        search_start: Instant,
    ) -> crate::Result<(bool, Option<GrepStats>, Vec<PathBuf>)> {
        let collect = self.collect;
        let counters = TextStatsCounters::new(collect.stats);

        let (matched, hits) = match self.output.mode {
            GrepMode::Standard | GrepMode::OnlyMatching => {
                let stop = AtomicBool::new(false);
                let mut files = Vec::with_capacity(inputs.len());
                inputs
                    .as_slice()
                    .par_iter()
                    .map_init(
                        || {
                            crate::grep::sink::standard::StandardReporter::new(
                                self.query,
                                self.compiled.matcher(),
                                &self.output,
                                self.separators,
                                &counters,
                                collect,
                            )
                        },
                        |reporter, input| reporter.report(input, &stop),
                    )
                    .collect_into_vec(&mut files);
                Self::flush_text(
                    files,
                    collect,
                    counters.bytes_printed(),
                    self.output.records.buffering,
                )?
            }
            GrepMode::Count
            | GrepMode::CountMatches
            | GrepMode::FilesWithMatches
            | GrepMode::FilesWithoutMatch => {
                let stop = AtomicBool::new(false);
                let mut files = Vec::with_capacity(inputs.len());
                inputs
                    .as_slice()
                    .par_iter()
                    .map_init(
                        || {
                            crate::grep::sink::summary::SummaryReporter::new(
                                self.query,
                                self.compiled.matcher(),
                                self.output.clone(),
                                &counters,
                                collect,
                            )
                        },
                        |reporter, input| reporter.report(input, &stop),
                    )
                    .collect_into_vec(&mut files);
                Self::flush_summary(
                    files,
                    collect,
                    counters.bytes_printed(),
                    self.output.records.buffering,
                )?
            }
        };

        let bytes_searched = if collect.stats {
            inputs.byte_count()
        } else {
            0
        };
        let stats = counters.finish(inputs.len(), bytes_searched, search_start.elapsed());

        Ok((matched, stats, hits))
    }

    fn flush_text(
        files: Vec<FileResult>,
        collect: GrepCollection,
        bytes_printed: Option<&std::sync::atomic::AtomicU64>,
        buffering: crate::grep::output::style::OutputBuffering,
    ) -> crate::Result<(bool, Vec<PathBuf>)> {
        let mut hits = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        for file in files {
            if collect.hits
                && let Some(hit) = file.hit
            {
                hits.push(hit);
            }
            outputs.push(file.output);
        }
        let matched = ChunkOutput::flush_all(outputs, bytes_printed, buffering)?;
        Ok((matched, hits))
    }

    fn flush_summary(
        files: Vec<FileResult>,
        collect: GrepCollection,
        bytes_printed: Option<&std::sync::atomic::AtomicU64>,
        buffering: crate::grep::output::style::OutputBuffering,
    ) -> crate::Result<(bool, Vec<PathBuf>)> {
        let mut hits = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        let mut matched = false;
        for file in files {
            if collect.hits
                && let Some(hit) = file.hit
            {
                hits.push(hit);
            }
            matched |= file.output.matched;
            outputs.push(file.output);
        }
        ChunkOutput::flush_all(outputs, bytes_printed, buffering)?;
        Ok((matched, hits))
    }

    fn run_json(
        &self,
        inputs: &GrepInputs<'_>,
        search_start: Instant,
        stats: Option<&mut GrepStats>,
    ) -> crate::Result<bool> {
        let stop = AtomicBool::new(false);
        let mut files = Vec::with_capacity(inputs.len());
        inputs
            .as_slice()
            .par_iter()
            .map_init(
                || {
                    crate::grep::sink::json::JsonReporter::new(
                        self.query,
                        self.compiled.matcher(),
                        self.output.clone(),
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
        let matched = ChunkOutput::flush_all(outputs, None, self.output.records.buffering)?;
        let summary_line =
            crate::grep::sink::json::format_json_summary_line(search_start.elapsed(), &merged)?;
        let summary_bytes = summary_line.len() as u64 + 1;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(summary_line.as_bytes())?;
        stdout.write_all(b"\n")?;
        if let Some(stats) = stats {
            stats.fill_from_json(
                &merged,
                inputs.len(),
                inputs.as_slice().iter().map(GrepInput::bytes).sum(),
                search_start.elapsed(),
                summary_bytes,
            );
        }
        Ok(matched)
    }
}
