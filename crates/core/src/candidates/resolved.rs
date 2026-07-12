use crate::corpus::Candidate;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};
use crate::index::Indexes;

/// Corpus files ready for search. The index arm is lazy; call [`into_vec`](Self::into_vec) to
/// materialize all.
pub enum Candidates<'a> {
    /// Walk, merge, or sorted resolve: paths already materialized.
    Materialized(Vec<Candidate>),
    /// Index-narrowed file ids; iteration materializes one file at a time.
    Index {
        indexes: &'a Indexes,
        file_ids: Vec<u32>,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    },
}

/// Index-narrowed file ids with lazy per-row materialization.
pub(crate) struct IndexFileIds<'a> {
    indexes: &'a Indexes,
    file_ids: Vec<u32>,
    filter: &'a CandidateFilter,
    admission: FilterAdmission,
}

/// Iterator over resolved candidates.
pub enum IntoIter<'a> {
    Materialized(std::vec::IntoIter<Candidate>),
    Index {
        ids: std::vec::IntoIter<u32>,
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    },
}

impl<'a> IndexFileIds<'a> {
    pub(crate) const fn new(
        indexes: &'a Indexes,
        file_ids: Vec<u32>,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> Self {
        Self {
            indexes,
            file_ids,
            filter,
            admission,
        }
    }
}

impl Candidates<'_> {
    pub(crate) const fn empty() -> Self {
        Self::Materialized(Vec::new())
    }

    /// Returns `true` when no candidates will be yielded.
    ///
    /// For index-backed rows, `false` means the id set may still filter to nothing during
    /// iteration.
    #[must_use = "candidate emptiness affects whether search runs"]
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Materialized(items) => items.is_empty(),
            Self::Index { file_ids, .. } => file_ids.is_empty(),
        }
    }

    /// Materialize every candidate. Index-backed rows materialize in parallel.
    #[must_use = "materialized candidates are consumed by search"]
    pub fn into_vec(self) -> Vec<Candidate> {
        match self {
            Self::Materialized(items) => items,
            Self::Index {
                indexes,
                file_ids,
                filter,
                admission,
            } => indexes.materialize_rows(&file_ids, filter, admission),
        }
    }
}

impl From<Vec<Candidate>> for Candidates<'_> {
    fn from(items: Vec<Candidate>) -> Self {
        Self::Materialized(items)
    }
}

impl<'a> From<IndexFileIds<'a>> for Candidates<'a> {
    fn from(ids: IndexFileIds<'a>) -> Self {
        let IndexFileIds {
            indexes,
            file_ids,
            filter,
            admission,
        } = ids;
        Self::Index {
            indexes,
            file_ids,
            filter,
            admission,
        }
    }
}

impl<'a> From<Candidates<'a>> for Vec<Candidate> {
    fn from(candidates: Candidates<'a>) -> Self {
        candidates.into_vec()
    }
}

impl Iterator for IntoIter<'_> {
    type Item = Candidate;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Materialized(iter) => iter.next(),
            Self::Index {
                ids,
                indexes,
                filter,
                admission,
            } => loop {
                let id = ids.next()?;
                if let Some(candidate) = indexes.materialize_row(id, filter, *admission) {
                    return Some(candidate);
                }
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Materialized(iter) => iter.size_hint(),
            Self::Index { ids, .. } => (0, ids.size_hint().1),
        }
    }
}

impl<'a> IntoIterator for Candidates<'a> {
    type Item = Candidate;
    type IntoIter = IntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Materialized(items) => IntoIter::Materialized(items.into_iter()),
            Self::Index {
                indexes,
                file_ids,
                filter,
                admission,
            } => IntoIter::Index {
                ids: file_ids.into_iter(),
                indexes,
                filter,
                admission,
            },
        }
    }
}
