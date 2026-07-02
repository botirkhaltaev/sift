use crate::grep::Error;

use super::SearchOptions;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub(crate) patterns: Vec<String>,
    pub(crate) options: SearchOptions,
}

pub struct SearchQueryBuilder {
    patterns: Vec<String>,
    options: SearchOptions,
}

impl SearchQuery {
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub const fn options(&self) -> &SearchOptions {
        &self.options
    }
}

impl SearchQueryBuilder {
    #[must_use]
    pub fn new(patterns: Vec<String>) -> Self {
        Self {
            patterns,
            options: SearchOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: SearchOptions) -> Self {
        self.options = options;
        self
    }

    /// Build inert search query data.
    ///
    /// # Errors
    ///
    /// Returns `Error::EmptyPatterns` if no patterns were provided.
    pub fn build(self) -> Result<SearchQuery, Error> {
        if self.patterns.is_empty() {
            return Err(Error::EmptyPatterns);
        }
        Ok(SearchQuery {
            patterns: self.patterns,
            options: self.options,
        })
    }
}
