use std::path::Path;

use crate::index::snapshot::ArtifactData;
use crate::index::{CorpusKind, IndexConfig, IndexDestination, IndexSource};

use super::TrigramIndex;
use super::TrigramIndexError;
use super::builder::IndexTables;
use super::file_table::{FileFingerprint, FileTable};
use super::storage;
use super::storage::lexicon::Lexicon;
use super::storage::postings::Postings;
use super::storage::trigram_sets::TrigramSets;

impl TrigramIndex {
    /// Write tables to `dir` as persistence files and return an mmap-backed index.
    fn create_in_dir(
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
                    || FileTable::create(&files_path, &tables.fingerprints),
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

    /// Build a new trigram index from the corpus described in `config`,
    /// writing artifact files into `output_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or encoding fails.
    pub fn build(config: &IndexConfig<'_>, output_dir: &Path) -> crate::Result<Self> {
        let tables = IndexTables::build(config)?;
        let root = config.corpus.root.canonicalize()?;
        Self::persist_tables(
            &tables,
            &root,
            config.corpus.kind,
            IndexDestination::Directory(output_dir),
        )
    }

    /// Encode and store tables at the given destination, returning a live index.
    pub(crate) fn persist_tables(
        tables: &IndexTables,
        root: &Path,
        corpus_kind: CorpusKind,
        dest: IndexDestination,
    ) -> crate::Result<Self> {
        match dest {
            IndexDestination::Directory(dir) => Self::create_in_dir(tables, root, corpus_kind, dir),
            IndexDestination::Snapshot { writer, namespace } => {
                let ((fr, lr), (pr, tr)) = rayon::join(
                    || {
                        rayon::join(
                            || FileTable::encode(&tables.fingerprints),
                            || Lexicon::encode(&tables.lexicon),
                        )
                    },
                    || {
                        rayon::join(
                            || Postings::encode(&tables.postings),
                            || TrigramSets::encode(&tables.file_trigrams),
                        )
                    },
                );

                let files_bytes = fr.map_err(crate::Error::Io)?;
                let lexicon_bytes = lr.map_err(crate::Error::Io)?;
                let postings_bytes = pr.map_err(crate::Error::Io)?;
                let trigram_sets_bytes = tr.map_err(crate::Error::Io)?;

                let files =
                    FileTable::from_artifact(ArtifactData::Memory(files_bytes.clone().into()))?;
                let lexicon =
                    Lexicon::from_artifact(ArtifactData::Memory(lexicon_bytes.clone().into()))?;
                let postings =
                    Postings::from_artifact(ArtifactData::Memory(postings_bytes.clone().into()))?;
                let trigram_sets = TrigramSets::from_artifact(ArtifactData::Memory(
                    trigram_sets_bytes.clone().into(),
                ))?;

                writer.put_artifact(namespace, crate::FILES_BIN, files_bytes)?;
                writer.put_artifact(namespace, crate::LEXICON_BIN, lexicon_bytes)?;
                writer.put_artifact(namespace, crate::POSTINGS_BIN, postings_bytes)?;
                writer.put_artifact(namespace, crate::TRIGRAMS_BIN, trigram_sets_bytes)?;

                let fingerprints = files.to_fingerprints().map_err(crate::Error::Io)?;
                Self::validate_file_paths(&fingerprints, Path::new(""))?;
                Self::validate_lexicon_postings(&lexicon, &postings)?;

                Ok(Self {
                    root: root.to_path_buf(),
                    fingerprints,
                    trigram_sets,
                    lexicon,
                    postings,
                    corpus_kind,
                })
            }
        }
    }

    /// Open a previously persisted trigram index from `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    pub fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<Self> {
        Self::open_tables(IndexSource::Directory(index_dir), root, corpus_kind)
    }

    /// Update the index from the current corpus, writing artifact files
    /// into `output_dir`.
    ///
    /// Returns `Ok(Some(index))` if a new index was written, or `Ok(None)`
    /// if no files changed.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or encoding fails.
    pub fn update(
        &self,
        config: &IndexConfig<'_>,
        output_dir: &Path,
    ) -> crate::Result<Option<Self>> {
        self.rebuild(config, IndexDestination::Directory(output_dir))
    }

    /// Rebuild index tables for changed files and persist to `dest`.
    pub(crate) fn rebuild(
        &self,
        config: &IndexConfig<'_>,
        dest: IndexDestination,
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
        let index = Self::persist_tables(&tables, &root, config.corpus.kind, dest)?;
        Ok(Some(index))
    }

    /// Open index tables from a storage source (directory or snapshot).
    pub(crate) fn open_tables(
        source: IndexSource,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Self> {
        match source {
            IndexSource::Directory(dir) => {
                let files_path = dir.join(crate::FILES_BIN);
                let lexicon_path = dir.join(crate::LEXICON_BIN);
                let postings_path = dir.join(crate::POSTINGS_BIN);
                let trigrams_path = dir.join(crate::TRIGRAMS_BIN);

                for p in [&files_path, &lexicon_path, &postings_path, &trigrams_path] {
                    if !p.is_file() {
                        return Err(TrigramIndexError::MissingComponent(p.clone()).into());
                    }
                }

                let files = FileTable::open(&files_path).map_err(TrigramIndexError::Io)?;
                let fingerprints = files.to_fingerprints().map_err(TrigramIndexError::Io)?;
                Self::validate_file_paths(&fingerprints, &files_path)?;

                let lexicon = storage::lexicon::Lexicon::open(&lexicon_path)
                    .map_err(TrigramIndexError::Io)?;
                let postings = storage::postings::Postings::open(&postings_path)
                    .map_err(TrigramIndexError::Io)?;

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
            IndexSource::Snapshot { reader, namespace } => {
                let files_data = reader.artifact(namespace, crate::FILES_BIN)?;
                let files = FileTable::from_artifact(files_data).map_err(TrigramIndexError::Io)?;
                let fingerprints = files.to_fingerprints().map_err(TrigramIndexError::Io)?;
                Self::validate_file_paths(&fingerprints, Path::new(""))?;

                let lexicon_data = reader.artifact(namespace, crate::LEXICON_BIN)?;
                let lexicon =
                    Lexicon::from_artifact(lexicon_data).map_err(TrigramIndexError::Io)?;

                let postings_data = reader.artifact(namespace, crate::POSTINGS_BIN)?;
                let postings =
                    Postings::from_artifact(postings_data).map_err(TrigramIndexError::Io)?;

                Self::validate_lexicon_postings(&lexicon, &postings)?;

                let trigram_sets_data = reader.artifact(namespace, crate::TRIGRAMS_BIN)?;
                let trigram_sets =
                    TrigramSets::from_artifact(trigram_sets_data).map_err(TrigramIndexError::Io)?;

                Ok(Self {
                    root: root.to_path_buf(),
                    fingerprints,
                    trigram_sets,
                    lexicon,
                    postings,
                    corpus_kind,
                })
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
