/// Optional artifacts gathered during search printing beyond primary output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintExtras {
    #[default]
    None,
    Stats,
    Hits,
    Both,
}

impl PrintExtras {
    #[must_use]
    pub const fn none() -> Self {
        Self::None
    }

    #[must_use]
    pub const fn stats() -> Self {
        Self::Stats
    }

    #[must_use]
    pub const fn hits() -> Self {
        Self::Hits
    }

    #[must_use]
    pub const fn both() -> Self {
        Self::Both
    }

    #[must_use]
    pub const fn with_stats(self, stats: bool) -> Self {
        if stats {
            match self {
                Self::None => Self::Stats,
                Self::Hits => Self::Both,
                other => other,
            }
        } else {
            match self {
                Self::Stats => Self::None,
                Self::Both => Self::Hits,
                other => other,
            }
        }
    }

    #[must_use]
    pub const fn with_hits(self, hits: bool) -> Self {
        if hits {
            match self {
                Self::None => Self::Hits,
                Self::Stats => Self::Both,
                other => other,
            }
        } else {
            match self {
                Self::Hits => Self::None,
                Self::Both => Self::Stats,
                other => other,
            }
        }
    }

    #[must_use]
    pub const fn collect_stats(self) -> bool {
        matches!(self, Self::Stats | Self::Both)
    }

    #[must_use]
    pub const fn collect_hits(self) -> bool {
        matches!(self, Self::Hits | Self::Both)
    }
}
