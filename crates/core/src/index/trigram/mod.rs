pub mod builder;
pub mod file_table;
pub mod key;
pub mod storage;

mod planner;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

use crate::index::{CorpusKind, FileId, IndexConfig, PlanMode, QueryPlanOutput};
use crate::query::QuerySpec;

use self::builder::{IndexTables, build_tables};
use self::file_table::FileFingerprint;
use self::planner::TrigramPlanner;
pub use key::Trigram;

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
    trigram_sets: storage::trigram_sets::TrigramSets,
    lexicon: storage::lexicon::Lexicon,
    postings: storage::postings::Postings,
    corpus_kind: CorpusKind,
}

impl TrigramIndex {
    /// Write tables to `dir` as persistence files and return an mmap-backed index.
    pub(crate) fn create_in_dir(
        tables: &IndexTables,
        root: &Path,
        corpus_kind: CorpusKind,
        dir: &Path,
    ) -> crate::Result<Self> {
        std::fs::create_dir_all(dir)?;

        let files_path = dir.join(crate::FILES_BIN);
        let lexicon_path = dir.join(crate::LEXICON_BIN);
        let postings_path = dir.join(crate::POSTINGS_BIN);
        let trigrams_path = dir.join(crate::TRIGRAMS_BIN);

        let ((fr, lr), (pr, tr)) = rayon::join(
            || {
                rayon::join(
                    || file_table::FileTable::create(&files_path, &tables.fingerprints),
                    || storage::lexicon::Lexicon::create(&lexicon_path, &tables.lexicon),
                )
            },
            || {
                rayon::join(
                    || storage::postings::Postings::create(&postings_path, &tables.postings),
                    || {
                        storage::trigram_sets::TrigramSets::create(
                            &trigrams_path,
                            &tables.file_trigrams,
                        )
                    },
                )
            },
        );

        let files = fr.map_err(crate::Error::Io)?;
        let lexicon = lr.map_err(crate::Error::Io)?;
        let postings = pr.map_err(crate::Error::Io)?;
        let trigram_sets = tr.map_err(crate::Error::Io)?;

        let root = root.to_path_buf();
        let fingerprints = files.to_fingerprints().map_err(crate::Error::Io)?;
        Self::validate_file_paths(&fingerprints, &files_path)?;

        Self::validate_lexicon_postings(&lexicon, &postings)?;

        Ok(Self {
            root,
            fingerprints,
            trigram_sets,
            lexicon,
            postings,
            corpus_kind,
        })
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
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Option<Vec<crate::Candidate>> {
        let plan = TrigramPlanner::build(query)?;
        Some(
            self.candidate_file_ids(&plan.arms)
                .into_iter()
                .filter_map(|id| {
                    let fid = FileId::new(usize::try_from(id).ok()?);
                    let fp = self.fingerprints.get(fid.get())?;
                    Some(crate::Candidate::new(fp.path.clone(), self.root.join(&fp.path)))
                })
                .collect(),
        )
    }

    /// Build a new trigram index from the corpus described in `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or file I/O fails.
    pub fn build(config: &IndexConfig<'_>, output_dir: &Path) -> crate::Result<Self> {
        let tables = build_tables(config)?;
        let root = config.corpus.root.canonicalize()?;
        Self::create_in_dir(&tables, &root, config.corpus.kind, output_dir)
    }

    /// Open a previously persisted trigram index from `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    pub fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<Self> {
        Ok(Self::open_tables(index_dir, root, corpus_kind)?)
    }

    /// Update the index from the current corpus, reusing per-file trigram sets
    /// for unchanged files.
    ///
    /// Returns `Ok(Some(index))` if a new snapshot was written, or `Ok(None)`
    /// if no files changed.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or file I/O fails.
    pub fn update(
        &self,
        config: &IndexConfig<'_>,
        output_dir: &Path,
    ) -> crate::Result<Option<Self>> {
        use rayon::prelude::*;
        use std::collections::HashMap;

        let paths = crate::index::trigram::builder::CorpusWalker::new(config).collect()?;
        let fingerprints =
            crate::index::trigram::builder::FingerprintCollector::new(config.corpus.root, &paths)
                .collect()?;

        if fingerprints == self.fingerprints {
            return Ok(None);
        }

        let previous_sets = self.trigram_sets.to_vec().map_err(crate::Error::Io)?;

        let lookup: HashMap<(&Path, i64, u64), &storage::trigram_sets::TrigramSet> = self
            .fingerprints
            .iter()
            .zip(previous_sets.iter())
            .map(|(fp, set)| ((fp.path.as_path(), fp.mtime_secs, fp.size), set))
            .collect();

        let file_trigrams: Vec<storage::trigram_sets::TrigramSet> = fingerprints
            .par_iter()
            .map(|fp| {
                if let Some(set) = lookup.get(&(fp.path.as_path(), fp.mtime_secs, fp.size)) {
                    return Ok((*set).clone());
                }
                let abs = config.corpus.root.join(&fp.path);
                storage::trigram_sets::TrigramSet::from_file(&abs).map_err(crate::Error::Io)
            })
            .collect::<crate::Result<_>>()?;

        let (lexicon, postings) =
            crate::index::trigram::builder::PostingAssembler::new(&file_trigrams).assemble()?;

        let tables = IndexTables {
            fingerprints,
            file_trigrams,
            lexicon,
            postings,
        };

        let root = config.corpus.root.canonicalize()?;
        let index = Self::create_in_dir(&tables, &root, config.corpus.kind, output_dir)?;
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

        let files = file_table::FileTable::open(&files_path).map_err(TrigramIndexError::Io)?;
        let fingerprints = files.to_fingerprints().map_err(TrigramIndexError::Io)?;
        Self::validate_file_paths(&fingerprints, &files_path)?;

        let lexicon =
            storage::lexicon::Lexicon::open(&lexicon_path).map_err(TrigramIndexError::Io)?;
        let postings =
            storage::postings::Postings::open(&postings_path).map_err(TrigramIndexError::Io)?;

        Self::validate_lexicon_postings(&lexicon, &postings)?;

        let trigram_sets = storage::trigram_sets::TrigramSets::open(&trigrams_path)
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

    fn validate_lexicon_postings(
        lexicon: &storage::lexicon::Lexicon,
        postings: &storage::postings::Postings,
    ) -> Result<(), TrigramIndexError> {
        let payload_len = postings.payload_len();
        for entry in lexicon {
            let start = usize::try_from(entry.offset).map_err(|_| {
                TrigramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} offset {} exceeds usize",
                        entry.trigram, entry.offset,
                    ),
                ))
            })?;
            let end = lexicon.posting_byte_end(entry.offset, payload_len);
            if start > end || end > payload_len {
                return Err(TrigramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} posting range [{start},{end}) exceeds payload_len {payload_len}",
                        entry.trigram,
                    ),
                )));
            }
            let slice = postings.slice(start, end.saturating_sub(start));
            let decoded_count = storage::postings::Postings::validate_list(slice).map_err(|e| {
                TrigramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("posting list for trigram {:?}: {e}", entry.trigram),
                ))
            })?;
            if decoded_count != entry.len as usize {
                return Err(TrigramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} claims len {} but posting list has {decoded_count} entries",
                        entry.trigram, entry.len,
                    ),
                )));
            }
        }
        Ok(())
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
            return storage::postings::Postings::decode_sorted(slices[0])
                .expect("postings validated at open");
        }
        let mut ordered: Vec<&[u8]> = slices.to_vec();
        ordered.sort_unstable_by_key(|slice| slice.len());
        let mut cur = storage::postings::Postings::decode_sorted(ordered[0])
            .expect("postings validated at open");
        for s in &ordered[1..] {
            cur = storage::postings::Postings::intersect_sorted(&cur, s)
                .expect("postings validated at open");
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
        let mut prev = 0u64;
        for (i, &value) in ids.iter().enumerate() {
            let raw = if i == 0 {
                u64::from(value)
            } else {
                u64::from(value) - prev
            };
            let mut varint_buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut varint_buf);
            buf.extend_from_slice(encoded);
            prev = u64::from(value);
        }
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
    #[should_panic(expected = "postings validated at open")]
    fn intersect_sorted_slices_invalid_varint_panics() {
        let a = &[0xff];
        let _ids = PostingOps::intersect_sorted_slices(&[a]);
    }

    #[test]
    fn intersect_sorted_slices_no_overlap_returns_empty() {
        let a = encode(&[1, 2, 3]);
        let b = encode(&[4, 5, 6]);
        let ids = PostingOps::intersect_sorted_slices(&[a.as_slice(), b.as_slice()]);
        assert!(ids.is_empty());
    }

    #[test]
    fn open_tables_rejects_count_mismatch() {
        use crate::index::trigram::storage::format::{
            FILES_MAGIC, LEXICON_MAGIC, POSTINGS_MAGIC, TRIGRAMS_MAGIC,
        };
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("create temp dir");
        let dir = tmp.path().join("index");
        std::fs::create_dir(&dir).expect("create index dir");

        // files.bin: empty
        let mut files = FILES_MAGIC.to_vec();
        files.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join("files.bin"), &files).expect("write files");

        // lexicon.bin: one entry, trigram "abc", offset=0, len=3
        let mut lex = LEXICON_MAGIC.to_vec();
        lex.extend_from_slice(&1u32.to_le_bytes()); // count=1
        lex.extend_from_slice(b"abc");
        lex.extend_from_slice(&0u64.to_le_bytes()); // offset=0
        lex.extend_from_slice(&3u32.to_le_bytes()); // len=3 — claims 3 entries
        std::fs::write(dir.join("lexicon.bin"), &lex).expect("write lexicon");

        // postings.bin: only 2 encoded entries (lexicon claims 3)
        let mut posting_payload = Vec::new();
        let mut buf = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(0, &mut buf));
        let mut buf2 = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(1, &mut buf2));
        let mut pb = POSTINGS_MAGIC.to_vec();
        pb.extend_from_slice(&u32::try_from(posting_payload.len()).unwrap().to_le_bytes());
        pb.extend_from_slice(&posting_payload);
        std::fs::write(dir.join("postings.bin"), &pb).expect("write postings");

        // trigrams.bin: empty
        let mut tri = TRIGRAMS_MAGIC.to_vec();
        tri.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join("trigrams.bin"), &tri).expect("write trigrams");

        let result = TrigramIndex::open_tables(
            &dir,
            Path::new("/root"),
            crate::index::CorpusKind::Directory,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("claims len") || err.contains("entries"),
            "expected count mismatch error, got: {err}",
        );
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
