use crate::grep::Error;
use crate::grep::options::MatchOptions;

use super::matcher::{Matcher, MatcherCompiler, RegexEngine};

#[derive(Debug, Clone)]
pub struct CompiledQuery {
    matcher: Matcher,
    indexability: Indexability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indexability {
    Indexed,
    Complete(IndexabilityReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexabilityReason {
    InvertedMatch,
    DecodedInput,
    RegexEngineUnsupportedByPlanner,
}

impl CompiledQuery {
    pub(crate) fn compile(patterns: &[String], opts: &MatchOptions) -> Result<Self, Error> {
        let (matcher, engine) = MatcherCompiler::new(patterns, opts).compile()?;
        Ok(Self {
            matcher,
            indexability: Self::compute_indexability(opts, engine),
        })
    }

    #[must_use]
    pub const fn matcher(&self) -> &Matcher {
        &self.matcher
    }

    #[must_use]
    pub const fn indexability(&self) -> Indexability {
        self.indexability
    }

    #[must_use]
    pub const fn index_capable(&self) -> bool {
        matches!(self.indexability, Indexability::Indexed)
    }

    fn compute_indexability(opts: &MatchOptions, engine: RegexEngine) -> Indexability {
        if opts.invert_match() {
            Indexability::Complete(IndexabilityReason::InvertedMatch)
        } else if opts.input_encoding.uses_decoded_input() {
            Indexability::Complete(IndexabilityReason::DecodedInput)
        } else if engine != RegexEngine::Rust {
            Indexability::Complete(IndexabilityReason::RegexEngineUnsupportedByPlanner)
        } else {
            Indexability::Indexed
        }
    }
}
