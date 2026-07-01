use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Candidate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderKey {
    #[default]
    None,
    Path,
    Modified,
    Accessed,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateOrderDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidateOrder {
    pub key: CandidateOrderKey,
    pub direction: CandidateOrderDirection,
}

impl CandidateOrder {
    #[must_use]
    pub const fn new(key: CandidateOrderKey, direction: CandidateOrderDirection) -> Self {
        Self { key, direction }
    }

    #[must_use]
    pub const fn is_sorted(self) -> bool {
        !matches!(self.key, CandidateOrderKey::None)
    }

    /// Order candidates in place according to the configured key.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when filesystem metadata required by a timestamp
    /// ordering key cannot be read.
    pub fn order(self, candidates: &mut [Candidate]) -> crate::Result<()> {
        if !self.is_sorted() {
            return Ok(());
        }

        let mut keyed = Vec::with_capacity(candidates.len());
        for candidate in candidates.iter().cloned() {
            keyed.push(CandidateOrderEntry::new(candidate, self.key)?);
        }

        keyed.sort_by(|a, b| {
            a.value
                .cmp(&b.value)
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });
        if matches!(self.direction, CandidateOrderDirection::Descending) {
            keyed.reverse();
        }

        for (slot, entry) in candidates.iter_mut().zip(keyed) {
            *slot = entry.candidate;
        }

        Ok(())
    }
}

struct CandidateOrderEntry {
    value: CandidateOrderValue,
    rel_path: PathBuf,
    candidate: Candidate,
}

impl CandidateOrderEntry {
    fn new(candidate: Candidate, key: CandidateOrderKey) -> crate::Result<Self> {
        let rel_path = candidate.rel_path().to_path_buf();
        let value = match key {
            CandidateOrderKey::None | CandidateOrderKey::Path => {
                CandidateOrderValue::Path(rel_path.clone())
            }
            CandidateOrderKey::Modified => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::modified,
            )?),
            CandidateOrderKey::Accessed => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::accessed,
            )?),
            CandidateOrderKey::Created => CandidateOrderValue::Time(candidate_time(
                candidate.abs_path(),
                std::fs::Metadata::created,
            )?),
        };

        Ok(Self {
            value,
            rel_path,
            candidate,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CandidateOrderValue {
    Path(PathBuf),
    Time(SystemTime),
}

fn candidate_time(
    path: &Path,
    timestamp: impl FnOnce(&std::fs::Metadata) -> std::io::Result<SystemTime>,
) -> crate::Result<SystemTime> {
    Ok(timestamp(&std::fs::metadata(path)?)?)
}
