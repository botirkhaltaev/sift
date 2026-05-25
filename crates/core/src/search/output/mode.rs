#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Standard,
    OnlyMatching,
    Count,
    CountMatches,
    FilesWithMatches,
    FilesWithoutMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputEmission {
    #[default]
    Normal,
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

/// Whether the query planner should request all files or narrowed candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidatePlan {
    /// Use index-narrowed candidates (trigram intersection).
    Narrowed,
    /// Request all indexed files (needed by Count and `FilesWithoutMatch` modes).
    AllFiles,
}

impl SearchMode {
    #[must_use]
    pub(crate) const fn candidate_plan(self) -> CandidatePlan {
        match self {
            Self::Standard | Self::OnlyMatching | Self::CountMatches | Self::FilesWithMatches => {
                CandidatePlan::Narrowed
            }
            Self::Count | Self::FilesWithoutMatch => CandidatePlan::AllFiles,
        }
    }
}
