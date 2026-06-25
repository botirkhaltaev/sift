use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::search::filter::{CandidateFilter, HiddenMode, IgnoreConfig, VisibilityConfig};
use crate::search::request::{LinkTraversal, WalkOptions};
use crate::walk::FileWalk;

/// Collect candidate files across all scopes by walking the filesystem.
impl CandidateFilter {
    /// # Errors
    ///
    /// Returns an error if walking the corpus fails.
    pub fn collect(&self) -> crate::Result<Vec<crate::Candidate>> {
        FileWalk::new(self.root())
            .scopes(self.scopes())
            .excludes(self.exclude_paths())
            .visibility(self.visibility().clone())
            .links(if self.follow_links() {
                LinkTraversal::Follow
            } else {
                LinkTraversal::DoNotFollow
            })
            .one_file_system(self.one_file_system())
            .max_depth(self.max_depth())
            .max_filesize(self.max_filesize())
            .collect()
    }
}

impl WalkOptions {
    /// Discovers files under the given root matching these walk options.
    ///
    /// # Errors
    ///
    /// Returns an error if the root path cannot be canonicalized or
    /// the walk encounters an inaccessible directory.
    pub fn discover_files(&self, root: &Path) -> crate::Result<HashSet<PathBuf>> {
        Ok(FileWalk::new(root)
            .visibility(VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::disabled(),
            })
            .links(self.links)
            .one_file_system(self.one_file_system)
            .max_depth(self.max_depth)
            .max_filesize(self.max_filesize)
            .collect_paths()?
            .into_iter()
            .collect())
    }
}
