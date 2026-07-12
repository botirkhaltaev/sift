use crate::corpus::Candidate;
use crate::index::{Indexes, MaterializeRequest};

/// Indexed file ids that can be turned into [`Candidate`] values on demand.
pub struct IndexedCandidates<'a> {
    selection: IndexedSelection,
    indexes: &'a Indexes,
    matching: MaterializeRequest<'a>,
}

enum IndexedSelection {
    Ids(Vec<u32>),
    All { count: u32 },
}

impl<'a> IndexedCandidates<'a> {
    #[must_use]
    pub const fn from_ids(
        file_ids: Vec<u32>,
        indexes: &'a Indexes,
        matching: MaterializeRequest<'a>,
    ) -> Self {
        Self {
            selection: IndexedSelection::Ids(file_ids),
            indexes,
            matching,
        }
    }

    #[must_use]
    pub const fn all(count: u32, indexes: &'a Indexes, matching: MaterializeRequest<'a>) -> Self {
        Self {
            selection: IndexedSelection::All { count },
            indexes,
            matching,
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        match &self.selection {
            IndexedSelection::Ids(ids) => ids.is_empty(),
            IndexedSelection::All { count } => *count == 0,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        match &self.selection {
            IndexedSelection::Ids(ids) => ids.len(),
            IndexedSelection::All { count } => *count as usize,
        }
    }

    /// Materialize one indexed id, skipping filtered-out rows.
    #[must_use]
    pub fn candidate_at(&self, index: usize) -> Option<Candidate> {
        let id = match &self.selection {
            IndexedSelection::Ids(ids) => *ids.get(index)?,
            IndexedSelection::All { count } => {
                if index >= *count as usize {
                    return None;
                }
                u32::try_from(index).ok()?
            }
        };
        self.indexes.candidate(id, self.matching)
    }

    /// Materialize every surviving candidate for eager search paths.
    #[must_use]
    pub fn materialize_all(&self) -> Vec<Candidate> {
        let mut out = Vec::with_capacity(self.len());
        for index in 0..self.len() {
            if let Some(candidate) = self.candidate_at(index) {
                out.push(candidate);
            }
        }
        out
    }
}

/// Candidates ready for input resolution / search.
pub enum ResolvedCandidates<'a> {
    /// Fully materialized candidate paths.
    Ready(Vec<Candidate>),
    /// Indexed ids materialized during `FirstMatch` search.
    Indexed(IndexedCandidates<'a>),
}

impl ResolvedCandidates<'_> {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Ready(candidates) => candidates.is_empty(),
            Self::Indexed(indexed) => indexed.is_empty(),
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Ready(candidates) => candidates.len(),
            Self::Indexed(indexed) => indexed.len(),
        }
    }

    #[must_use]
    pub fn into_ready(self) -> Vec<Candidate> {
        match self {
            Self::Ready(candidates) => candidates,
            Self::Indexed(indexed) => indexed.materialize_all(),
        }
    }
}

/// Whether the planner may defer indexed materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateMaterialization {
    /// Build every [`Candidate`] before search.
    Eager,
    /// Keep indexed ids when ordering does not require paths up front.
    Deferred,
}
