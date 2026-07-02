use std::sync::OnceLock;

use crate::corpus::Candidate;
use crate::grep::Error;
use crate::grep::collection::ReportCollector;
use crate::grep::compiled::{CompiledQuery, IndexUse, QueryCompiler};
use crate::grep::input::Inputs;
use crate::grep::options::MatchOptions;
use crate::grep::policy::CandidatePolicy;
use crate::grep::report::Report;
use crate::grep::session::Session;
use crate::grep::stats::StatsMode;
use crate::query::{
    IndexNarrowing, PlanContext, QueryFlags, QueryPlanner, QuerySpec, ResolutionConfig,
};

#[derive(Debug)]
pub struct Query {
    pub(crate) patterns: Vec<String>,
    pub(crate) opts: MatchOptions,
    compiled: OnceLock<CompiledQuery>,
}

impl Clone for Query {
    fn clone(&self) -> Self {
        Self {
            patterns: self.patterns.clone(),
            opts: self.opts.clone(),
            compiled: OnceLock::new(),
        }
    }
}

impl Query {
    /// Creates a new search query from patterns.
    ///
    /// # Errors
    ///
    /// Returns `Error::EmptyPatterns` if the pattern list is empty.
    pub fn new(patterns: Vec<String>) -> Result<Self, Error> {
        if patterns.is_empty() {
            return Err(Error::EmptyPatterns);
        }
        Ok(Self {
            patterns,
            opts: MatchOptions::default(),
            compiled: OnceLock::new(),
        })
    }

    #[must_use]
    pub fn options(mut self, opts: MatchOptions) -> Self {
        self.opts = opts;
        self.compiled = OnceLock::new();
        self
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub const fn opts(&self) -> &MatchOptions {
        &self.opts
    }

    /// Compiles patterns into a matcher and records planner-facing query capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if pattern compilation fails.
    ///
    /// # Panics
    ///
    /// Panics if the compiled query cache is empty immediately after initialization.
    pub fn compile(&self) -> Result<&CompiledQuery, Error> {
        if let Some(compiled) = self.compiled.get() {
            return Ok(compiled);
        }
        let compiled = QueryCompiler::new(&self.patterns, &self.opts).compile()?;
        let _ = self.compiled.set(compiled);
        Ok(self.compiled.get().expect("just initialised"))
    }

    #[must_use]
    pub fn query_spec(&self) -> QuerySpec<'_> {
        let mut flags = QueryFlags::empty();
        if self.opts().fixed_strings() {
            flags |= QueryFlags::FIXED_STRINGS;
        }
        if self.opts().case_insensitive() {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if self.opts().word_regexp() {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if self.opts().line_regexp() {
            flags |= QueryFlags::LINE_REGEXP;
        }
        if self.opts().invert_match() {
            flags |= QueryFlags::INVERT_MATCH;
        }
        QuerySpec {
            patterns: self.patterns(),
            flags,
        }
    }

    fn validate_max_results(&self) -> crate::Result<()> {
        if self.opts().max_results == Some(0) {
            return Err(crate::Error::Search(Error::InvalidMaxCount));
        }
        Ok(())
    }

    /// Resolve candidate files for this query under the given session and policy.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution or regex compilation fails.
    pub fn candidates(
        &self,
        session: &Session,
        policy: CandidatePolicy,
    ) -> crate::Result<Vec<Candidate>> {
        self.validate_max_results()?;
        let spec = self.query_spec();
        let compiled = self.compile()?;
        QueryPlanner::new(spec).resolve(
            PlanContext::new(
                session.indexes,
                session.filter,
                session.store_meta,
                match compiled.index_use() {
                    IndexUse::Narrow => IndexNarrowing::Enabled,
                    IndexUse::CompleteScan => IndexNarrowing::Disabled,
                },
            ),
            ResolutionConfig {
                coverage: policy.coverage(),
                fallback: policy.fallback,
                order: policy.order,
            },
        )
    }

    /// Search the given inputs and return a report.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation or search execution fails.
    pub fn search(&self, inputs: &Inputs, stats: StatsMode) -> crate::Result<Report> {
        self.validate_max_results()?;
        let compiled = self.compile()?;
        Ok(ReportCollector {
            query: self,
            compiled,
            inputs,
            stats,
        }
        .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_new_rejects_empty_patterns() {
        let result = Query::new(Vec::new());

        assert!(result.is_err());
    }
}
