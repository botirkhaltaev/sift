use super::gram::GramWidth;

/// Configured runtime-width N-gram index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    pub(crate) width: GramWidth,
}

impl Config {
    #[must_use]
    pub const fn new(width: GramWidth) -> Self {
        Self { width }
    }

    pub const DEFAULT: Self = Self {
        width: GramWidth::TRIGRAM,
    };

    #[must_use]
    pub const fn width(self) -> GramWidth {
        self.width
    }

    #[must_use]
    pub fn name(self) -> String {
        format!("ngram-{}", self.width.get())
    }

    /// Parse an N-gram index configuration name.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is not `ngram-N` or `ngram:N`, or if `N` is not a valid width.
    pub fn parse_name(value: &str) -> Result<Self, String> {
        let width = value
            .strip_prefix("ngram-")
            .or_else(|| value.strip_prefix("ngram:"))
            .ok_or_else(|| format!("unknown index: {value}"))?;
        let width = width
            .parse::<u8>()
            .map_err(|_| format!("invalid ngram width: {width}"))?;
        Ok(Self::new(GramWidth::new(width)))
    }

    #[must_use]
    pub const fn artifact_names(self) -> &'static [&'static str] {
        &[
            crate::FILES_BIN,
            crate::LEXICON_BIN,
            crate::POSTINGS_BIN,
            crate::GRAMS_BIN,
        ]
    }
}
