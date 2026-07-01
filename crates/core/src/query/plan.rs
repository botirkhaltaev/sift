/// How candidate resolution should proceed for a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionStrategy {
    /// Narrow via index lookup, merging walk results for lazy stores when needed.
    UseIndex,
    /// Walk the filesystem under the filter root.
    WalkAll,
    /// Use the complete indexed file list without narrowing.
    AllIndexed,
}

/// Pure planning decision for a grep run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionPlan {
    pub strategy: ResolutionStrategy,
}
