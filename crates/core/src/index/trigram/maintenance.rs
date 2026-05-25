use std::path::{Path, PathBuf};

use super::TrigramIndex;
use super::builder::build_index_tables;
use super::file_table::MappedFilesView;
use super::storage::lexicon::MappedLexicon;
use super::storage::postings::MappedPostings;
use crate::index::CorpusKind;
use crate::index::maintenance::{IndexBuildConfig, IndexMaintenance};

pub struct TrigramMaintenance;

impl IndexMaintenance for TrigramMaintenance {
    type Index = TrigramIndex;
    const NAME: &'static str = "trigram";

    fn build(config: &IndexBuildConfig<'_>, output_dir: &Path) -> crate::Result<TrigramIndex> {
        std::fs::create_dir_all(output_dir)?;

        let tables = build_index_tables(&super::builder::IndexBuildConfig {
            root: config.root,
            follow_links: config.follow_links,
            exclude_paths: config.exclude_paths,
            include_paths: config.include_paths,
        })?;

        let files = MappedFilesView::from_paths(&tables.files);
        std::fs::write(output_dir.join(crate::FILES_BIN), files.backing_slice())?;

        let lex = MappedLexicon::from_entries(&tables.lexicon);
        std::fs::write(output_dir.join(crate::LEXICON_BIN), lex.backing_slice())?;

        let post = MappedPostings::from_bytes(&tables.postings);
        std::fs::write(output_dir.join(crate::POSTINGS_BIN), post.backing_slice())?;

        let root = config.root.canonicalize()?;
        let abs_paths = tables.files.iter().map(|p| root.join(p)).collect();

        let lexicon = MappedLexicon::from_entries(&tables.lexicon);
        let postings = MappedPostings::from_bytes(&tables.postings);

        Ok(TrigramIndex {
            root,
            file_paths: tables.files,
            abs_paths,
            lexicon,
            postings,
            corpus_kind: config.corpus_kind,
        })
    }

    fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<TrigramIndex> {
        let files_path = index_dir.join(crate::FILES_BIN);
        let lexicon_path = index_dir.join(crate::LEXICON_BIN);
        let postings_path = index_dir.join(crate::POSTINGS_BIN);

        if !files_path.exists() || !lexicon_path.exists() || !postings_path.exists() {
            return Err(crate::Error::Index(crate::index::IndexError::Trigram(
                super::TrigramIndexError::MissingComponent(index_dir.to_path_buf()),
            )));
        }

        let files = MappedFilesView::open(&files_path)?;
        let file_paths = files.to_path_bufs()?;
        let lexicon = MappedLexicon::open(&lexicon_path)?;
        let postings = MappedPostings::open(&postings_path)?;

        super::validate_file_paths(&file_paths, &files_path)?;

        let root = root.to_path_buf();
        let abs_paths: Vec<PathBuf> = file_paths.iter().map(|p| root.join(p)).collect();

        Ok(TrigramIndex {
            root,
            file_paths,
            abs_paths,
            lexicon,
            postings,
            corpus_kind,
        })
    }
}
