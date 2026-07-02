use grep_pcre2::RegexMatcher as Pcre2Matcher;
use grep_regex::RegexMatcher;

use crate::query::IndexNarrowing;

#[derive(Debug, Clone)]
pub enum CompiledQuery {
    Rust {
        matcher: RegexMatcher,
        index_narrowing: IndexNarrowing,
    },
    Pcre2 {
        matcher: Pcre2Matcher,
        index_narrowing: IndexNarrowing,
    },
}

impl CompiledQuery {
    pub(crate) const fn index_narrowing(&self) -> IndexNarrowing {
        match self {
            Self::Rust {
                index_narrowing, ..
            }
            | Self::Pcre2 {
                index_narrowing, ..
            } => *index_narrowing,
        }
    }
}
