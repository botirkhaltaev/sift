#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Lines,
    Matches,
    CountLines {
        zeros: ZeroCounts,
    },
    CountMatches {
        zeros: ZeroCounts,
    },
    FilesWithMatches,
    FilesWithoutMatch,
}

impl SearchMode {
    pub(crate) const fn selects(self, matched: bool) -> bool {
        match self {
            Self::FilesWithoutMatch => !matched,
            Self::Lines
            | Self::Matches
            | Self::CountLines { .. }
            | Self::CountMatches { .. }
            | Self::FilesWithMatches => matched,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZeroCounts {
    #[default]
    Omit,
    Include,
}
