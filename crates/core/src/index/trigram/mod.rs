pub mod builder;
pub mod file_table;
pub mod maintenance;
pub mod storage;
pub mod types;

mod planner;

use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

use crate::index::{CorpusKind, FileId, PlanMode, QueryPlanOutput, SearchIndex};
use crate::query::QuerySpec;

use self::planner::{TrigramCandidatePlan, TrigramPlanner};
pub use builder::TrigramIndexBuilder;
pub use maintenance::TrigramMaintenance;
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
    file_paths: Vec<PathBuf>,
    abs_paths: Vec<PathBuf>,
    lexicon: storage::lexicon::MappedLexicon,
    postings: storage::postings::MappedPostings,
    corpus_kind: CorpusKind,
}

impl TrigramIndex {
    /// Open an index written to `dir` by `save_to_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`TrigramIndexError::MissingComponent`] if a trigram table file
    /// is missing, or [`TrigramIndexError::Io`] on read/mmap failure.
    pub fn open(
        dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> Result<Self, TrigramIndexError> {
        let paths = [
            dir.join(crate::FILES_BIN),
            dir.join(crate::LEXICON_BIN),
            dir.join(crate::POSTINGS_BIN),
        ];
        for p in &paths {
            if !p.is_file() {
                return Err(TrigramIndexError::MissingComponent(p.clone()));
            }
        }

        let files = file_table::MappedFilesView::open(&paths[0]).map_err(TrigramIndexError::Io)?;
        let file_paths = files.to_path_bufs().map_err(TrigramIndexError::Io)?;
        validate_file_paths(&file_paths, &paths[0])?;
        let abs_paths = compute_abs_paths(root, &file_paths);
        let lexicon =
            storage::lexicon::MappedLexicon::open(&paths[1]).map_err(TrigramIndexError::Io)?;
        let postings =
            storage::postings::MappedPostings::open(&paths[2]).map_err(TrigramIndexError::Io)?;

        Ok(Self {
            root: root.to_path_buf(),
            file_paths,
            abs_paths,
            lexicon,
            postings,
            corpus_kind,
        })
    }

    /// Persist the in-memory index to `dir`.
    ///
    /// # Errors
    ///
    /// Propagates IO errors from creating directories or writing files.
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), TrigramIndexError> {
        std::fs::create_dir_all(dir)?;

        let files = file_table::MappedFilesView::from_paths(&self.file_paths);
        std::fs::write(dir.join(crate::FILES_BIN), files.backing_slice())
            .map_err(TrigramIndexError::Io)?;
        std::fs::write(dir.join(crate::LEXICON_BIN), self.lexicon.backing_slice())
            .map_err(TrigramIndexError::Io)?;
        std::fs::write(dir.join(crate::POSTINGS_BIN), self.postings.backing_slice())
            .map_err(TrigramIndexError::Io)?;
        Ok(())
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
    pub fn file_path(&self, id: FileId) -> Option<&Path> {
        self.file_paths.get(id.get()).map(PathBuf::as_path)
    }

    #[must_use]
    pub fn file_abs_path(&self, id: FileId) -> Option<PathBuf> {
        self.abs_paths.get(id.get()).cloned()
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

    fn posting_bytes_slice(&self, tri: Trigram) -> &[u8] {
        let Some(entry) = self.lexicon.get(tri.to_bytes()) else {
            return &[];
        };
        let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
        let n = usize::try_from(entry.len).unwrap_or(usize::MAX);
        let nbytes = n.saturating_mul(4);
        self.postings.slice(start, nbytes)
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
        merge_sorted_runs(id_lists)
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
        let ids = intersect_sorted_posting_byte_slices(&slices);
        if ids.is_empty() { None } else { Some(ids) }
    }

    fn trigram_candidate_ids(&self, plan: &TrigramCandidatePlan) -> Vec<FileId> {
        let raw = self.candidate_file_ids(&plan.arms);
        raw.into_iter()
            .filter_map(|id| usize::try_from(id).ok().map(FileId::new))
            .collect()
    }

    fn all_file_ids(&self) -> Vec<FileId> {
        (0..self.file_paths.len()).map(FileId::new).collect()
    }

    fn resolve_candidates(&self, ids: impl IntoIterator<Item = FileId>) -> Vec<crate::Candidate> {
        ids.into_iter()
            .filter_map(|id| {
                let rel_path = self.file_paths.get(id.get())?.clone();
                let abs_path = self.abs_paths.get(id.get())?.clone();
                Some(crate::Candidate::new(rel_path, abs_path))
            })
            .collect()
    }
}

impl SearchIndex for TrigramIndex {
    fn root(&self) -> &Path {
        &self.root
    }

    fn corpus_kind(&self) -> CorpusKind {
        self.corpus_kind
    }

    fn candidates(&self, query: &QuerySpec<'_>) -> Vec<crate::Candidate> {
        let ids = TrigramPlanner::build(query).map_or_else(
            || self.all_file_ids(),
            |plan| self.trigram_candidate_ids(&plan),
        );
        self.resolve_candidates(ids)
    }

    fn all_files(&self) -> Vec<crate::Candidate> {
        self.resolve_candidates(self.all_file_ids())
    }
}

fn compute_abs_paths(root: &Path, file_paths: &[PathBuf]) -> Vec<PathBuf> {
    file_paths.iter().map(|p| root.join(p)).collect()
}

fn validate_file_paths(file_paths: &[PathBuf], _meta_path: &Path) -> Result<(), TrigramIndexError> {
    for path in file_paths {
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(TrigramIndexError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid file path in index: {}", path.display()),
            )));
        }
    }
    Ok(())
}

fn intersect_sorted_posting_byte_slices(slices: &[&[u8]]) -> Vec<u32> {
    if slices.is_empty() {
        return Vec::new();
    }
    if slices.len() == 1 {
        return u32_vec_from_le_bytes(slices[0]);
    }
    let mut cur = intersect_two_posting_bytes(slices[0], slices[1]);
    for s in &slices[2..] {
        cur = intersect_vec_with_posting_bytes(&cur, s);
        if cur.is_empty() {
            break;
        }
    }
    cur
}

fn intersect_two_posting_bytes(a: &[u8], b: &[u8]) -> Vec<u32> {
    if !a.len().is_multiple_of(4) || !b.len().is_multiple_of(4) {
        return Vec::new();
    }
    let an = a.len() / 4;
    let bn = b.len() / 4;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut out = Vec::new();
    while i < an && j < bn {
        let ai = u32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
        let bj = u32::from_le_bytes(b[j * 4..j * 4 + 4].try_into().unwrap());
        match ai.cmp(&bj) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                out.push(ai);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

fn intersect_vec_with_posting_bytes(cur: &[u32], b: &[u8]) -> Vec<u32> {
    if !b.len().is_multiple_of(4) {
        return Vec::new();
    }
    let bn = b.len() / 4;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut out = Vec::new();
    while i < cur.len() && j < bn {
        let bj = u32::from_le_bytes(b[j * 4..j * 4 + 4].try_into().unwrap());
        match cur[i].cmp(&bj) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                out.push(cur[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

fn u32_vec_from_le_bytes(slice: &[u8]) -> Vec<u32> {
    if !slice.len().is_multiple_of(4) {
        return Vec::new();
    }
    slice
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{
        compute_abs_paths, intersect_sorted_posting_byte_slices, merge_sorted_runs,
        validate_file_paths,
    };
    use std::path::PathBuf;

    fn bytes(ids: &[u32]) -> Vec<u8> {
        ids.iter().flat_map(|id| id.to_le_bytes()).collect()
    }

    #[test]
    fn merge_sorted_runs_preserves_order_and_uniqueness() {
        let merged = merge_sorted_runs(vec![vec![1, 3, 7], vec![1, 2, 7, 9], vec![4, 7, 8]]);
        assert_eq!(merged, vec![1, 2, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_handles_smallest_first_order() {
        let a = bytes(&[1, 3, 5, 7, 9]);
        let b = bytes(&[3, 7]);
        let c = bytes(&[0, 3, 4, 7, 8]);
        let slices = vec![a.as_slice(), b.as_slice(), c.as_slice()];
        let ids = intersect_sorted_posting_byte_slices(&slices);
        assert_eq!(ids, vec![3, 7]);
    }

    #[test]
    fn merge_sorted_runs_empty_input_returns_empty() {
        let merged = merge_sorted_runs(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_sorted_runs_single_list_returns_as_is() {
        let merged = merge_sorted_runs(vec![vec![1, 2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn merge_sorted_runs_with_empty_lists_mixed_in() {
        let merged = merge_sorted_runs(vec![vec![1, 3], vec![], vec![2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_empty_input_returns_empty() {
        let ids = intersect_sorted_posting_byte_slices(&[]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_single_slice_returns_decoded_ids() {
        let a = bytes(&[1, 3, 5]);
        let ids = intersect_sorted_posting_byte_slices(&[a.as_slice()]);
        assert_eq!(ids, vec![1, 3, 5]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_non_multiple_of_four_returns_empty() {
        let a = &[1, 2, 3];
        let ids = intersect_sorted_posting_byte_slices(&[a]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_no_overlap_returns_empty() {
        let a = bytes(&[1, 2, 3]);
        let b = bytes(&[4, 5, 6]);
        let ids = intersect_sorted_posting_byte_slices(&[a.as_slice(), b.as_slice()]);
        assert!(ids.is_empty());
    }

    #[test]
    fn validate_file_paths_accepts_normal_relative_paths() {
        let paths = vec![PathBuf::from("a.txt"), PathBuf::from("sub/b.txt")];
        let result = validate_file_paths(&paths, std::path::Path::new("/meta.json"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_file_paths_rejects_absolute_paths() {
        let abs = std::env::current_dir().unwrap().join("a.txt");
        let paths = vec![abs];
        let result = validate_file_paths(&paths, std::path::Path::new("/meta.json"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_empty_paths() {
        let paths = vec![PathBuf::from("")];
        let result = validate_file_paths(&paths, std::path::Path::new("/meta.json"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_parent_dir_paths() {
        let paths = vec![PathBuf::from("../escape.txt")];
        let result = validate_file_paths(&paths, std::path::Path::new("/meta.json"));
        assert!(result.is_err());
    }

    #[test]
    fn compute_abs_paths_joins_root_with_relative_paths() {
        let root = std::path::Path::new("/corpus");
        let rel = vec![PathBuf::from("a.txt"), PathBuf::from("sub/b.txt")];
        let abs = compute_abs_paths(root, &rel);
        assert_eq!(abs[0], PathBuf::from("/corpus/a.txt"));
        assert_eq!(abs[1], PathBuf::from("/corpus/sub/b.txt"));
    }
}
