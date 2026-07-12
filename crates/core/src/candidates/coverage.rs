/// Whether candidate resolution should cover every corpus file or only
/// index-narrowed potential matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateCoverage {
    /// Index may narrow to potential matches only.
    PotentialMatches,
    /// Every corpus file must be considered (`-L`, `--include-zero`).
    Complete,
}
