/// Whether candidate resolution needs every corpus file or only index-narrowed paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateCoverage {
    /// Every file under the filter root (or complete indexed list).
    Complete,
    /// Only paths the index marks as potential matches.
    #[default]
    PotentialMatches,
}
