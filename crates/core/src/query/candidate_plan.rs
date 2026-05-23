/// One OR branch: every trigram here must appear in a candidate file (intersection).
pub type Arm = Vec<[u8; 3]>;

/// Trigram-specific candidate plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrigramCandidatePlan {
    pub arms: Vec<Arm>,
}

/// Index-agnostic candidate plan produced by the query planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidatePlan {
    FullScan,
    Trigram(TrigramCandidatePlan),
}
