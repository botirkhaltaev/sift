#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintMode {
    #[default]
    Standard,
    OnlyMatching,
    Count,
    CountMatches,
    FilesWithMatches,
    FilesWithoutMatch,
}

impl PrintMode {
    /// Summary modes render from the search report and do not need streamed
    /// match events.
    #[must_use]
    pub const fn is_summary(self) -> bool {
        matches!(
            self,
            Self::Count | Self::CountMatches | Self::FilesWithMatches | Self::FilesWithoutMatch
        )
    }
}

/// How search results reach stdout.
///
/// - [`Normal`](Self::Normal) — stream begin/match/end events through the printer.
/// - [`Summary`](Self::Summary) — discard events; print counts/paths from the report.
/// - [`Quiet`](Self::Quiet) — discard events; write nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputEmission {
    #[default]
    Normal,
    Summary,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZeroCountMode {
    #[default]
    Omit,
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEmissionMode {
    Lines,
    OnlyMatching,
}
