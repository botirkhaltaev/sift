use crate::corpus::Candidate;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};
use crate::corpus::order::CandidateOrder;
use crate::index::Indexes;

enum Backend<'a> {
    Vec(Vec<Candidate>),
    IndexRows {
        indexes: &'a Indexes,
        file_ids: Vec<u32>,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    },
}

/// Resolved corpus candidates ready for search consumption.
pub struct Candidates<'a> {
    backend: Backend<'a>,
}

impl<'a> Candidates<'a> {
    pub(crate) const fn empty() -> Self {
        Self {
            backend: Backend::Vec(Vec::new()),
        }
    }

    pub(crate) const fn from_vec(candidates: Vec<Candidate>) -> Self {
        Self {
            backend: Backend::Vec(candidates),
        }
    }

    pub(crate) const fn from_index_rows(
        indexes: &'a Indexes,
        file_ids: Vec<u32>,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> Self {
        Self {
            backend: Backend::IndexRows {
                indexes,
                file_ids,
                filter,
                admission,
            },
        }
    }

    /// Returns `true` when no candidates will be yielded.
    ///
    /// For index-backed rows, `false` means the id set may still filter to
    /// nothing during iteration.
    #[must_use = "candidate emptiness affects whether search runs"]
    pub const fn is_empty(&self) -> bool {
        match &self.backend {
            Backend::Vec(items) => items.is_empty(),
            Backend::IndexRows { file_ids, .. } => file_ids.is_empty(),
        }
    }

    /// Materialize every candidate. Index-backed rows materialize in parallel.
    #[must_use = "materialized candidates are consumed by search"]
    pub fn into_vec(self) -> Vec<Candidate> {
        match self.backend {
            Backend::Vec(items) => items,
            Backend::IndexRows {
                indexes,
                file_ids,
                filter,
                admission,
            } => indexes.materialize_rows(&file_ids, filter, admission),
        }
    }

    pub(crate) fn order(self, order: CandidateOrder) -> crate::Result<Self> {
        match self.backend {
            Backend::Vec(mut items) => {
                order.order(&mut items)?;
                Ok(Self {
                    backend: Backend::Vec(items),
                })
            }
            Backend::IndexRows { .. } => {
                let mut items = self.into_vec();
                order.order(&mut items)?;
                Ok(Self {
                    backend: Backend::Vec(items),
                })
            }
        }
    }
}

pub struct IntoIter<'a> {
    backend: IntoIterBackend<'a>,
}

enum IntoIterBackend<'a> {
    Vec(std::vec::IntoIter<Candidate>),
    Index {
        ids: std::vec::IntoIter<u32>,
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    },
}

impl Iterator for IntoIter<'_> {
    type Item = Candidate;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.backend {
            IntoIterBackend::Vec(iter) => iter.next(),
            IntoIterBackend::Index {
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
        match &self.backend {
            IntoIterBackend::Vec(iter) => iter.size_hint(),
            IntoIterBackend::Index { ids, .. } => (0, Some(ids.len())),
        }
    }
}

impl<'a> IntoIterator for Candidates<'a> {
    type Item = Candidate;
    type IntoIter = IntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        match self.backend {
            Backend::Vec(items) => IntoIter {
                backend: IntoIterBackend::Vec(items.into_iter()),
            },
            Backend::IndexRows {
                indexes,
                file_ids,
                filter,
                admission,
            } => IntoIter {
                backend: IntoIterBackend::Index {
                    ids: file_ids.into_iter(),
                    indexes,
                    filter,
                    admission,
                },
            },
        }
    }
}
