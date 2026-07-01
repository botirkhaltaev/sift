use crate::grep::Error;
use crate::grep::options::RegexEngineRequest;
use crate::grep::pattern::Query;

use super::matcher::Matcher;

#[derive(Debug, Clone)]
pub struct CompiledQuery {
    matcher: Matcher,
    indexability: Indexability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegexEngine {
    Rust,
    Pcre2,
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
}

impl Query {
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
        let compiled = self.build_compiled_search()?;
        let _ = self.compiled.set(compiled);
        Ok(self.compiled.get().expect("just initialised"))
    }

    fn build_compiled_search(&self) -> Result<CompiledQuery, Error> {
        let (matcher, engine) = match self.opts.regex_engine {
            RegexEngineRequest::Rust => (self.build_rust_matcher()?, RegexEngine::Rust),
            RegexEngineRequest::Pcre2 => (self.build_pcre2_matcher()?, RegexEngine::Pcre2),
            RegexEngineRequest::Auto => match self.build_rust_matcher() {
                Ok(matcher) => (matcher, RegexEngine::Rust),
                Err(_) => (self.build_pcre2_matcher()?, RegexEngine::Pcre2),
            },
        };

        Ok(CompiledQuery {
            matcher,
            indexability: self.indexability(engine),
        })
    }

    fn indexability(&self, engine: RegexEngine) -> Indexability {
        if self.opts.invert_match() {
            Indexability::Complete(IndexabilityReason::InvertedMatch)
        } else if self.opts.input_encoding.uses_decoded_input() {
            Indexability::Complete(IndexabilityReason::DecodedInput)
        } else if engine != RegexEngine::Rust {
            Indexability::Complete(IndexabilityReason::RegexEngineUnsupportedByPlanner)
        } else {
            Indexability::Indexed
        }
    }
}
