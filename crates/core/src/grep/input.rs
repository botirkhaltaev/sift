use std::borrow::Cow;
use std::path::PathBuf;

use crate::corpus::Candidate;

pub enum Input<'a> {
    Path {
        candidate: &'a Candidate,
        explicit: bool,
    },
    Bytes {
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        candidate: Option<&'a Candidate>,
    },
}

impl Input<'_> {
    /// Display path for matching and optional corpus rel-path for hit tracking.
    #[must_use]
    pub fn paths(&self) -> (PathBuf, Option<PathBuf>) {
        match self {
            Self::Path { candidate, .. } => (
                candidate.abs_path().to_path_buf(),
                Some(candidate.rel_path().to_path_buf()),
            ),
            Self::Bytes {
                path, candidate, ..
            } => {
                let display = candidate.map_or_else(
                    || PathBuf::from(path.as_ref()),
                    |c| c.abs_path().to_path_buf(),
                );
                let hit = candidate.map(|c| c.rel_path().to_path_buf());
                (display, hit)
            }
        }
    }

    #[must_use]
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Path { candidate, .. } => candidate
                .cached_size()
                .unwrap_or_else(|| std::fs::metadata(candidate.abs_path()).map_or(0, |m| m.len())),
            Self::Bytes { bytes, .. } => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        }
    }
}

pub struct Inputs<'a> {
    items: Vec<Input<'a>>,
}

impl<'a> Inputs<'a> {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    pub fn push_path(&mut self, candidate: &'a Candidate) {
        self.push_candidate(candidate, false);
    }

    pub fn push_explicit_path(&mut self, candidate: &'a Candidate) {
        self.push_candidate(candidate, true);
    }

    fn push_candidate(&mut self, candidate: &'a Candidate, explicit: bool) {
        self.items.push(Input::Path {
            candidate,
            explicit,
        });
    }

    pub fn push_bytes(
        &mut self,
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        candidate: Option<&'a Candidate>,
    ) {
        self.items.push(Input::Bytes {
            path,
            bytes,
            candidate,
        });
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn byte_count(&self) -> u64 {
        self.items.iter().map(Input::byte_len).sum()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Input<'_>] {
        &self.items
    }
}
