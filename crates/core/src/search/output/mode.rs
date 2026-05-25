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

/// Whether the index planner should return all files or narrowed candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateCoverage {
    /// Use index-narrowed candidates (trigram intersection).
    Narrowed,
    /// Request all indexed files (needed by `Count`, `FilesWithoutMatch`,
    /// and `CountMatches` with include-zero).
    Complete,
}
