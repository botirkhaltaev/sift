use std::path::{Path, PathBuf};

use crate::candidates::CandidateSpec;
use crate::index::{CorpusKind, FileId};

use super::config::Config;
use super::files::FileFingerprint;
use super::gram::GramWidth;
use super::storage::grams::GramSets;
use super::storage::lexicon::Lexicon;
use super::storage::postings::Postings;

/// Errors specific to opening or persisting an N-gram index.
#[derive(Debug, thiserror::Error)]
pub enum NGramIndexError {
    #[error("index component missing: {0}")]
    MissingComponent(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Opened runtime-width N-gram index.
#[derive(Debug)]
pub struct Index {
    pub(crate) width: GramWidth,
    pub(crate) storage: Storage,
}

#[derive(Debug)]
pub struct Storage {
    pub(crate) root: PathBuf,
    pub(crate) fingerprints: Vec<FileFingerprint>,
    pub(crate) gram_sets: GramSets,
    pub(crate) lexicon: Lexicon,
    pub(crate) postings: Postings,
    pub(crate) corpus_kind: CorpusKind,
}

impl Index {
    #[must_use]
    pub const fn width(&self) -> GramWidth {
        self.width
    }

    #[must_use]
    pub fn file_path(&self, id: FileId) -> Option<&Path> {
        self.storage
            .fingerprints
            .get(id.get())
            .map(|fp| fp.path.as_path())
    }

    #[must_use]
    pub fn file_abs_path(&self, id: FileId) -> Option<PathBuf> {
        self.storage
            .fingerprints
            .get(id.get())
            .map(|fp| self.storage.root.join(&fp.path))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.storage.root
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        self.storage.corpus_kind
    }

    /// Produce narrowed candidate files for the query.
    /// Returns `None` if the query can't be narrowed (full scan required).
    #[must_use]
    pub fn candidates(&self, query: &CandidateSpec<'_>) -> Option<Vec<crate::Candidate>> {
        let arms = Config::new(self.width).extract_literal_arms(query)?;
        Some(
            self.candidate_file_ids(&arms)
                .into_iter()
                .filter_map(|id| {
                    let fid = FileId::new(usize::try_from(id).ok()?);
                    let fp = self.storage.fingerprints.get(fid.get())?;
                    Some(crate::Candidate::with_metadata(
                        fp.path.clone(),
                        self.storage.root.join(&fp.path),
                        Some(fp.size),
                        None,
                    ))
                })
                .collect(),
        )
    }

    /// Returns an explanation of how a query would be handled.
    #[must_use]
    pub fn explain(&self, query: &CandidateSpec<'_>) -> crate::index::QueryPlanOutput {
        let mode = match Config::new(self.width).extract_literal_arms(query) {
            Some(_) => crate::index::PlanMode::IndexedCandidates,
            None => crate::index::PlanMode::FullScan,
        };
        crate::index::QueryPlanOutput {
            pattern: query.patterns.to_vec().join("|"),
            mode,
        }
    }

    #[must_use]
    pub(crate) fn all_files(&self) -> Vec<crate::Candidate> {
        self.storage
            .fingerprints
            .iter()
            .map(|fp| {
                crate::Candidate::with_metadata(
                    fp.path.clone(),
                    self.storage.root.join(&fp.path),
                    Some(fp.size),
                    None,
                )
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn indexed_rel_paths(&self) -> std::collections::HashSet<PathBuf> {
        self.storage
            .fingerprints
            .iter()
            .map(|fp| fp.path.clone())
            .collect()
    }

    pub(crate) fn merge_partial_fingerprints(
        existing: &[FileFingerprint],
        root: &Path,
        paths: &[PathBuf],
    ) -> crate::Result<Vec<FileFingerprint>> {
        use std::collections::HashMap;

        let mut by_path: HashMap<PathBuf, FileFingerprint> = existing
            .iter()
            .map(|fp| (fp.path.clone(), fp.clone()))
            .collect();
        for rel in paths {
            let abs = root.join(rel);
            let meta = std::fs::metadata(&abs).map_err(crate::Error::Io)?;
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            let fp = FileFingerprint {
                path: rel.clone(),
                mtime_secs,
                size: meta.len(),
            };
            by_path.insert(rel.clone(), fp);
        }
        let mut merged: Vec<_> = by_path.into_values().collect();
        merged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(merged)
    }

    pub(crate) fn validate_lexicon_postings(
        lexicon: &Lexicon,
        postings: &Postings,
    ) -> Result<(), NGramIndexError> {
        let payload_len = postings.payload_len();
        for entry in lexicon {
            let start = usize::try_from(entry.offset).map_err(|_| {
                NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} offset {} exceeds usize",
                        entry.gram, entry.offset
                    ),
                ))
            })?;
            let end = lexicon.posting_byte_end(entry.offset, payload_len);
            if start > end || end > payload_len {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} posting range [{start},{end}) exceeds payload_len {payload_len}",
                        entry.gram,
                    ),
                )));
            }
            let slice = postings.slice(start, end.saturating_sub(start));
            let decoded_count = Postings::validate_list(slice).map_err(|e| {
                NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("posting list for gram {:?}: {e}", entry.gram),
                ))
            })?;
            if decoded_count != entry.len as usize {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} claims len {} but posting list has {decoded_count} entries",
                        entry.gram, entry.len,
                    ),
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_file_paths(
        fingerprints: &[FileFingerprint],
    ) -> Result<(), NGramIndexError> {
        for fp in fingerprints {
            if fp.path.as_os_str().is_empty()
                || fp.path.is_absolute()
                || fp
                    .path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid file path in index: {}", fp.path.display()),
                )));
            }
        }
        Ok(())
    }
}
