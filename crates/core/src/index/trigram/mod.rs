pub mod builder;
pub mod file_table;
pub mod storage;
pub mod types;

mod planner;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

use crate::index::{CorpusKind, FileId, IndexBuildConfig, PlanMode, QueryPlanOutput};
use crate::query::QuerySpec;

use self::builder::{IndexTableBuilder, IndexTables};
use self::file_table::FileFingerprint;
use self::planner::{TrigramCandidatePlan, TrigramPlanner};
pub use builder::TrigramIndexBuilder;
pub use types::Trigram;

/// Errors specific to opening or persisting a trigram index.
#[derive(Debug, thiserror::Error)]
pub enum TrigramIndexError {
    #[error("index component missing: {0}")]
    MissingComponent(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct TrigramIndex {
    root: PathBuf,
    pub(crate) fingerprints: Vec<FileFingerprint>,
    trigram_sets: storage::trigram_sets::MappedTrigramSets,
    lexicon: storage::lexicon::MappedLexicon,
    postings: storage::postings::MappedPostings,
    corpus_kind: CorpusKind,
}

impl TrigramIndex {
    /// Construct from in-memory index tables.
    pub(crate) fn from_tables(tables: IndexTables, root: PathBuf, corpus_kind: CorpusKind) -> Self {
        let lexicon = storage::lexicon::MappedLexicon::from_entries(&tables.lexicon);
        let postings = storage::postings::MappedPostings::from_bytes(&tables.postings);
        let trigram_sets =
            storage::trigram_sets::MappedTrigramSets::from_sets(&tables.file_trigrams);

        Self {
            root,
            fingerprints: tables.fingerprints,
            trigram_sets,
            lexicon,
            postings,
            corpus_kind,
        }
    }

    /// Persist the in-memory index to `dir`.
    ///
    /// # Errors
    ///
    /// Propagates IO errors from creating directories or writing files.
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), TrigramIndexError> {
        std::fs::create_dir_all(dir)?;

        let files = file_table::MappedFilesView::from_fingerprints(&self.fingerprints);
        let files_path = dir.join(crate::FILES_BIN);
        let lexicon_path = dir.join(crate::LEXICON_BIN);
        let postings_path = dir.join(crate::POSTINGS_BIN);
        let trigrams_path = dir.join(crate::TRIGRAMS_BIN);

        let files_bytes = files.backing_slice();
        let lexicon_bytes = self.lexicon.backing_slice();
        let postings_bytes = self.postings.backing_slice();
        let trigrams_bytes = self.trigram_sets.backing_slice();

        let ((files_res, lexicon_res), (postings_res, trigrams_res)) = rayon::join(
            || {
                rayon::join(
                    || std::fs::write(&files_path, files_bytes),
                    || std::fs::write(&lexicon_path, lexicon_bytes),
                )
            },
            || {
                rayon::join(
                    || std::fs::write(&postings_path, postings_bytes),
                    || std::fs::write(&trigrams_path, trigrams_bytes),
                )
            },
        );

        files_res.map_err(TrigramIndexError::Io)?;
        lexicon_res.map_err(TrigramIndexError::Io)?;
        postings_res.map_err(TrigramIndexError::Io)?;
        trigrams_res.map_err(TrigramIndexError::Io)?;
        Ok(())
    }

    #[must_use]
    pub fn file_path(&self, id: FileId) -> Option<&Path> {
        self.fingerprints.get(id.get()).map(|fp| fp.path.as_path())
    }

    #[must_use]
    pub fn file_abs_path(&self, id: FileId) -> Option<PathBuf> {
        self.fingerprints
            .get(id.get())
            .map(|fp| self.root.join(&fp.path))
    }

    /// Returns an explanation of how a query would be handled.
    #[must_use]
    pub fn explain(&self, query: &QuerySpec<'_>) -> QueryPlanOutput {
        let mode = match TrigramPlanner::build(query) {
            Some(_) => PlanMode::IndexedCandidates,
            None => PlanMode::FullScan,
        };
        QueryPlanOutput {
            pattern: query.patterns.to_vec().join("|"),
            mode,
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        self.corpus_kind
    }

    #[must_use]
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Vec<crate::Candidate> {
        TrigramPlanner::build(query).map_or_else(
            || self.resolve_all_candidates(),
            |plan| self.resolve_candidates(self.trigram_candidate_ids(&plan)),
        )
    }

    #[must_use]
    pub fn all_files(&self) -> Vec<crate::Candidate> {
        self.resolve_all_candidates()
    }

    /// Build a new trigram index from the corpus described in `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or file I/O fails.
    pub fn build(config: &IndexBuildConfig<'_>, output_dir: &Path) -> crate::Result<Self> {
        std::fs::create_dir_all(output_dir)?;

        let tables = IndexTableBuilder::new(config).build()?;
        let root = config.root.canonicalize()?;
        let index = Self::from_tables(tables, root, config.corpus_kind);
        index.save_to_dir(output_dir).map_err(|e| match e {
            TrigramIndexError::Io(io) => crate::Error::Io(io),
            TrigramIndexError::MissingComponent(p) => crate::Error::Io(std::io::Error::other(
                format!("missing component: {}", p.display()),
            )),
        })?;
        Ok(index)
    }

    /// Open a previously persisted trigram index from `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    pub fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<Self> {
        Ok(Self::open_tables(index_dir, root, corpus_kind)?)
    }

    /// Incrementally update the index, rebuilding only if the corpus changed.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking or file I/O fails.
    pub fn update(
        &self,
        config: &IndexBuildConfig<'_>,
        output_dir: &Path,
    ) -> crate::Result<Option<Self>> {
        let file_trigrams = self.trigram_sets.to_sets().map_err(crate::Error::Io)?;

        let tables = IndexTableBuilder::new(config)
            .with_previous(&self.fingerprints, &file_trigrams)
            .build()?;

        if tables.fingerprints == self.fingerprints {
            return Ok(None);
        }

        std::fs::create_dir_all(output_dir)?;
        let root = config.root.canonicalize()?;
        let index = Self::from_tables(tables, root, config.corpus_kind);
        index.save_to_dir(output_dir).map_err(|e| match e {
            TrigramIndexError::Io(io) => crate::Error::Io(io),
            TrigramIndexError::MissingComponent(p) => crate::Error::Io(std::io::Error::other(
                format!("missing component: {}", p.display()),
            )),
        })?;
        Ok(Some(index))
    }

    fn posting_bytes_slice(&self, tri: Trigram) -> &[u8] {
        let Some(entry) = self.lexicon.get(tri.to_bytes()) else {
            return &[];
        };
        let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
        let payload_len = self.postings.payload_len();
        let end = self.lexicon.posting_byte_end(entry.offset, payload_len);
        self.postings.slice(start, end.saturating_sub(start))
    }

    fn candidate_file_ids(&self, arms: &[Vec<u8>]) -> Vec<u32> {
        if arms.is_empty() {
            return Vec::new();
        }
        if arms.len() == 1 {
            return self.posting_ids_for_literal(&arms[0]).unwrap_or_default();
        }
        let mut id_lists: Vec<Vec<u32>> = Vec::with_capacity(arms.len());
        for arm in arms {
            if let Some(ids) = self.posting_ids_for_literal(arm) {
                id_lists.push(ids);
            }
        }
        PostingOps::merge_sorted_runs(id_lists)
    }

    fn posting_ids_for_literal(&self, lit: &[u8]) -> Option<Vec<u32>> {
        if lit.len() < 3 {
            return None;
        }
        let trigrams: Vec<Trigram> = Trigram::windows(lit).collect();
        if trigrams.is_empty() {
            return None;
        }
        let mut slices: Vec<&[u8]> = Vec::with_capacity(trigrams.len());
        for tri in &trigrams {
            let s = self.posting_bytes_slice(*tri);
            if s.is_empty() {
                return None;
            }
            slices.push(s);
        }
        slices.sort_unstable_by_key(|slice| slice.len());
        let ids = PostingOps::intersect_sorted_slices(&slices);
        if ids.is_empty() { None } else { Some(ids) }
    }

    fn trigram_candidate_ids(&self, plan: &TrigramCandidatePlan) -> Vec<FileId> {
        let raw = self.candidate_file_ids(&plan.arms);
        raw.into_iter()
            .filter_map(|id| usize::try_from(id).ok().map(FileId::new))
            .collect()
    }

    fn candidate_from_fingerprint(&self, fp: &FileFingerprint) -> crate::Candidate {
        let rel_path = fp.path.clone();
        let abs_path = self.root.join(&fp.path);
        crate::Candidate::new(rel_path, abs_path)
    }

    fn resolve_candidates(&self, ids: impl IntoIterator<Item = FileId>) -> Vec<crate::Candidate> {
        ids.into_iter()
            .filter_map(|id| {
                self.fingerprints
                    .get(id.get())
                    .map(|fp| self.candidate_from_fingerprint(fp))
            })
            .collect()
    }

    fn resolve_all_candidates(&self) -> Vec<crate::Candidate> {
        self.fingerprints
            .iter()
            .map(|fp| self.candidate_from_fingerprint(fp))
            .collect()
    }

    fn open_tables(
        dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> Result<Self, TrigramIndexError> {
        let files_path = dir.join(crate::FILES_BIN);
        let lexicon_path = dir.join(crate::LEXICON_BIN);
        let postings_path = dir.join(crate::POSTINGS_BIN);
        let trigrams_path = dir.join(crate::TRIGRAMS_BIN);

        for p in [&files_path, &lexicon_path, &postings_path, &trigrams_path] {
            if !p.is_file() {
                return Err(TrigramIndexError::MissingComponent(p.clone()));
            }
        }

        let files =
            file_table::MappedFilesView::open(&files_path).map_err(TrigramIndexError::Io)?;
        let fingerprints = files.to_fingerprints().map_err(TrigramIndexError::Io)?;
        Self::validate_file_paths(&fingerprints, &files_path)?;

        let lexicon =
            storage::lexicon::MappedLexicon::open(&lexicon_path).map_err(TrigramIndexError::Io)?;
        let postings = storage::postings::MappedPostings::open(&postings_path)
            .map_err(TrigramIndexError::Io)?;

        let trigram_sets = storage::trigram_sets::MappedTrigramSets::open(&trigrams_path)
            .map_err(TrigramIndexError::Io)?;

        Ok(Self {
            root: root.to_path_buf(),
            fingerprints,
            trigram_sets,
            lexicon,
            postings,
            corpus_kind,
        })
    }

    fn validate_file_paths(
        fingerprints: &[FileFingerprint],
        _meta_path: &Path,
    ) -> Result<(), TrigramIndexError> {
        for fp in fingerprints {
            if fp.path.as_os_str().is_empty()
                || fp.path.is_absolute()
                || fp
                    .path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(TrigramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid file path in index: {}", fp.path.display()),
                )));
            }
        }
        Ok(())
    }
}

struct PostingOps;

impl PostingOps {
    fn intersect_sorted_slices(slices: &[&[u8]]) -> Vec<u32> {
        if slices.is_empty() {
            return Vec::new();
        }
        if slices.len() == 1 {
            return storage::varint::decode_sorted_deltas::<u32>(slices[0]);
        }
        let mut ordered: Vec<&[u8]> = slices.to_vec();
        ordered.sort_unstable_by_key(|slice| slice.len());
        let mut cur = storage::varint::decode_sorted_deltas::<u32>(ordered[0]);
        for s in &ordered[1..] {
            cur = storage::varint::intersect_sorted(&cur, s);
            if cur.is_empty() {
                break;
            }
        }
        cur
    }

    fn merge_sorted_runs(lists: Vec<Vec<u32>>) -> Vec<u32> {
        if lists.is_empty() {
            return Vec::new();
        }
        if lists.len() == 1 {
            return lists.into_iter().next().unwrap_or_default();
        }

        let total: usize = lists.iter().map(Vec::len).sum();
        let mut heap: BinaryHeap<Reverse<(u32, usize)>> = BinaryHeap::with_capacity(lists.len());
        let mut positions = vec![0usize; lists.len()];

        for (list_idx, list) in lists.iter().enumerate() {
            if let Some(&first) = list.first() {
                heap.push(Reverse((first, list_idx)));
            }
        }

        let mut out = Vec::with_capacity(total);
        let mut last = None;
        while let Some(Reverse((value, list_idx))) = heap.pop() {
            if last != Some(value) {
                out.push(value);
                last = Some(value);
            }

            positions[list_idx] += 1;
            if let Some(&next) = lists[list_idx].get(positions[list_idx]) {
                heap.push(Reverse((next, list_idx)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn encode(ids: &[u32]) -> Vec<u8> {
        let mut buf = Vec::new();
        storage::varint::encode_sorted_deltas(&mut buf, ids);
        buf
    }

    #[test]
    fn merge_sorted_runs_preserves_order_and_uniqueness() {
        let merged =
            PostingOps::merge_sorted_runs(vec![vec![1, 3, 7], vec![1, 2, 7, 9], vec![4, 7, 8]]);
        assert_eq!(merged, vec![1, 2, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_handles_smallest_first_order() {
        let a = encode(&[1, 3, 5, 7, 9]);
        let b = encode(&[3, 7]);
        let c = encode(&[0, 3, 4, 7, 8]);
        let slices = vec![a.as_slice(), b.as_slice(), c.as_slice()];
        let ids = PostingOps::intersect_sorted_slices(&slices);
        assert_eq!(ids, vec![3, 7]);
    }

    #[test]
    fn merge_sorted_runs_empty_input_returns_empty() {
        let merged = PostingOps::merge_sorted_runs(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_sorted_runs_single_list_returns_as_is() {
        let merged = PostingOps::merge_sorted_runs(vec![vec![1, 2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn merge_sorted_runs_with_empty_lists_mixed_in() {
        let merged = PostingOps::merge_sorted_runs(vec![vec![1, 3], vec![], vec![2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_empty_input_returns_empty() {
        let ids = PostingOps::intersect_sorted_slices(&[]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_slices_single_returns_decoded_ids() {
        let a = encode(&[1, 3, 5]);
        let ids = PostingOps::intersect_sorted_slices(&[a.as_slice()]);
        assert_eq!(ids, vec![1, 3, 5]);
    }

    #[test]
    fn intersect_sorted_slices_invalid_varint_returns_empty() {
        let a = &[0xff];
        let ids = PostingOps::intersect_sorted_slices(&[a]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_slices_no_overlap_returns_empty() {
        let a = encode(&[1, 2, 3]);
        let b = encode(&[4, 5, 6]);
        let ids = PostingOps::intersect_sorted_slices(&[a.as_slice(), b.as_slice()]);
        assert!(ids.is_empty());
    }

    #[test]
    fn validate_file_paths_accepts_normal_relative_paths() {
        let fps = vec![
            FileFingerprint {
                path: PathBuf::from("a.txt"),
                mtime_secs: 0,
                size: 0,
            },
            FileFingerprint {
                path: PathBuf::from("sub/b.txt"),
                mtime_secs: 0,
                size: 0,
            },
        ];
        let result = TrigramIndex::validate_file_paths(&fps, Path::new("/meta.json"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_file_paths_rejects_absolute_paths() {
        let abs = std::env::current_dir().unwrap().join("a.txt");
        let fps = vec![FileFingerprint {
            path: abs,
            mtime_secs: 0,
            size: 0,
        }];
        let result = TrigramIndex::validate_file_paths(&fps, Path::new("/meta.json"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_empty_paths() {
        let fps = vec![FileFingerprint {
            path: PathBuf::from(""),
            mtime_secs: 0,
            size: 0,
        }];
        let result = TrigramIndex::validate_file_paths(&fps, Path::new("/meta.json"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_parent_dir_paths() {
        let fps = vec![FileFingerprint {
            path: PathBuf::from("../escape.txt"),
            mtime_secs: 0,
            size: 0,
        }];
        let result = TrigramIndex::validate_file_paths(&fps, Path::new("/meta.json"));
        assert!(result.is_err());
    }
}
