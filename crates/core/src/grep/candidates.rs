use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rayon::prelude::*;

use crate::Candidate;
use crate::grep::corpus::GrepCorpus;
use crate::grep::output::GrepOutput;
use crate::grep::query::{CandidateStrategy, CompleteCandidateReason, GrepQuery};
use crate::query::{CandidatePlan, CandidateRequirement, QueryPlanner};
use crate::walk::FileWalk;

pub(crate) struct CandidateResolver<'a> {
    query: &'a GrepQuery,
    corpus: &'a GrepCorpus<'a>,
    output: GrepOutput,
    candidate_strategy: CandidateStrategy,
}

impl<'a> CandidateResolver<'a> {
    pub(crate) const fn new(
        query: &'a GrepQuery,
        corpus: &'a GrepCorpus<'a>,
        output: GrepOutput,
        candidate_strategy: CandidateStrategy,
    ) -> Self {
        Self {
            query,
            corpus,
            output,
            candidate_strategy,
        }
    }

    pub(crate) fn resolve(self) -> crate::Result<CandidateSet> {
        let coverage = self.coverage();
        let raw = QueryPlanner::new(self.query.build_query_spec()).candidates(
            CandidatePlan {
                indexes: self.corpus.indexes,
                requirement: coverage.requirement(),
                filter: self.corpus.filter,
                source: self.corpus.index_state.candidate_source(),
            },
            || FileWalk::from_filter(self.corpus.filter).collect(),
        )?;

        CandidateSet::new(raw, coverage)
            .retain_matches(self.corpus.filter)
            .order(self.corpus.order)
    }

    fn coverage(&self) -> CandidateCoverage {
        let mut reasons = CandidateCoverageReasons::default();

        if self.corpus.content_source.is_some() {
            reasons.push(CandidateCoverageReason::TransformedContent);
        }

        if self.corpus.indexes.is_empty() {
            reasons.push(CandidateCoverageReason::MissingIndex);
        }

        if self.corpus.index_state.snapshot == crate::query::SnapshotValidation::Stale {
            reasons.push(CandidateCoverageReason::StaleSnapshot);
        }

        match self.candidate_strategy {
            CandidateStrategy::Indexed => {
                if self.output.candidate_requirement() == CandidateRequirement::Complete {
                    reasons.push(CandidateCoverageReason::OutputRequiresCompleteCorpus);
                }
            }
            CandidateStrategy::Complete(reason) => reasons.push(reason.into()),
        }

        if reasons.is_empty() {
            CandidateCoverage::PotentialMatches
        } else {
            CandidateCoverage::Complete(reasons)
        }
    }
}

pub(crate) struct CandidateSet {
    candidates: Vec<Candidate>,
    coverage: CandidateCoverage,
}

impl CandidateSet {
    #[must_use]
    pub(crate) const fn new(candidates: Vec<Candidate>, coverage: CandidateCoverage) -> Self {
        Self {
            candidates,
            coverage,
        }
    }

    pub(crate) fn retain_matches(mut self, filter: &crate::grep::CandidateFilter) -> Self {
        self.candidates = self
            .candidates
            .into_par_iter()
            .filter(|candidate| candidate.matches(filter))
            .collect();
        self
    }

    pub(crate) fn order(mut self, order: CandidateOrder) -> crate::Result<Self> {
        order.order(&mut self.candidates)?;
        Ok(self)
    }

    #[must_use]
    pub(crate) fn as_slice(&self) -> &[Candidate] {
        &self.candidates
    }

    #[must_use]
    pub(crate) const fn coverage(&self) -> CandidateCoverage {
        self.coverage
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateCoverage {
    PotentialMatches,
    Complete(CandidateCoverageReasons),
}

impl CandidateCoverage {
    const fn requirement(self) -> CandidateRequirement {
        match self {
            Self::PotentialMatches => CandidateRequirement::PotentialMatches,
            Self::Complete(_) => CandidateRequirement::Complete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CandidateCoverageReasons {
    bits: u16,
}

impl CandidateCoverageReasons {
    const OUTPUT_REQUIRES_COMPLETE_CORPUS: u16 = 1 << 0;
    const INVERTED_MATCH: u16 = 1 << 1;
    const DECODED_INPUT: u16 = 1 << 2;
    const REGEX_ENGINE_UNSUPPORTED_BY_PLANNER: u16 = 1 << 3;
    const TRANSFORMED_CONTENT: u16 = 1 << 4;
    const MISSING_INDEX: u16 = 1 << 5;
    const STALE_SNAPSHOT: u16 = 1 << 6;

    const fn is_empty(self) -> bool {
        self.bits == 0
    }

    const fn push(&mut self, reason: CandidateCoverageReason) {
        self.bits |= reason.bit();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateCoverageReason {
    OutputRequiresCompleteCorpus,
    InvertedMatch,
    DecodedInput,
    RegexEngineUnsupportedByPlanner,
    TransformedContent,
    MissingIndex,
    StaleSnapshot,
}

impl CandidateCoverageReason {
    const fn bit(self) -> u16 {
        match self {
            Self::OutputRequiresCompleteCorpus => {
                CandidateCoverageReasons::OUTPUT_REQUIRES_COMPLETE_CORPUS
            }
            Self::InvertedMatch => CandidateCoverageReasons::INVERTED_MATCH,
            Self::DecodedInput => CandidateCoverageReasons::DECODED_INPUT,
            Self::RegexEngineUnsupportedByPlanner => {
                CandidateCoverageReasons::REGEX_ENGINE_UNSUPPORTED_BY_PLANNER
            }
            Self::TransformedContent => CandidateCoverageReasons::TRANSFORMED_CONTENT,
            Self::MissingIndex => CandidateCoverageReasons::MISSING_INDEX,
            Self::StaleSnapshot => CandidateCoverageReasons::STALE_SNAPSHOT,
        }
    }
}

impl From<CompleteCandidateReason> for CandidateCoverageReason {
    fn from(reason: CompleteCandidateReason) -> Self {
        match reason {
            CompleteCandidateReason::InvertedMatch => Self::InvertedMatch,
            CompleteCandidateReason::DecodedInput => Self::DecodedInput,
            CompleteCandidateReason::RegexEngineUnsupportedByPlanner => {
                Self::RegexEngineUnsupportedByPlanner
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderKey {
    #[default]
    None,
    Path,
    Modified,
    Accessed,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidateOrder {
    pub key: CandidateOrderKey,
    pub direction: CandidateOrderDirection,
}

impl CandidateOrder {
    #[must_use]
    pub const fn new(key: CandidateOrderKey, direction: CandidateOrderDirection) -> Self {
        Self { key, direction }
    }

    #[must_use]
    pub const fn is_sorted(self) -> bool {
        !matches!(self.key, CandidateOrderKey::None)
    }

    /// Order candidates in place according to the configured key.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when filesystem metadata required by a timestamp
    /// ordering key cannot be read.
    pub fn order(self, candidates: &mut [Candidate]) -> crate::Result<()> {
        if !self.is_sorted() {
            return Ok(());
        }

        let mut keyed = Vec::with_capacity(candidates.len());
        for candidate in candidates.iter().cloned() {
            keyed.push(CandidateOrderEntry::new(candidate, self.key)?);
        }

        keyed.sort_by(|a, b| {
            a.value
                .cmp(&b.value)
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });
        if matches!(self.direction, CandidateOrderDirection::Descending) {
            keyed.reverse();
        }

        for (slot, entry) in candidates.iter_mut().zip(keyed) {
            *slot = entry.candidate;
        }

        Ok(())
    }
}

struct CandidateOrderEntry {
    value: CandidateOrderValue,
    rel_path: PathBuf,
    candidate: Candidate,
}

impl CandidateOrderEntry {
    fn new(candidate: Candidate, key: CandidateOrderKey) -> crate::Result<Self> {
        let rel_path = candidate.rel_path().to_path_buf();
        let value = match key {
            CandidateOrderKey::None | CandidateOrderKey::Path => {
                CandidateOrderValue::Path(rel_path.clone())
            }
            CandidateOrderKey::Modified => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::modified,
            )?),
            CandidateOrderKey::Accessed => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::accessed,
            )?),
            CandidateOrderKey::Created => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::created,
            )?),
        };

        Ok(Self {
            value,
            rel_path,
            candidate,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CandidateOrderValue {
    Path(PathBuf),
    Time(SystemTime),
}

fn candidate_time(
    path: &Path,
    timestamp: impl FnOnce(&std::fs::Metadata) -> std::io::Result<SystemTime>,
) -> crate::Result<SystemTime> {
    Ok(timestamp(&std::fs::metadata(path)?)?)
}
