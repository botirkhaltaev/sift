use std::path::{Path, PathBuf};

use std::borrow::Cow;

use crate::corpus::Candidate;
use crate::corpus::candidate::PathDisplay;

#[derive(Debug, Clone)]
pub struct InputIdentity {
    pub display_path: PathBuf,
    pub hit_path: Option<PathBuf>,
    pub byte_len: Option<u64>,
}

pub enum Input<'a> {
    Path {
        path: Cow<'a, Path>,
        identity: InputIdentity,
        explicit: bool,
    },
    Bytes {
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        identity: InputIdentity,
        explicit: bool,
    },
}

impl InputIdentity {
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        Self {
            display_path: PathBuf::from(name),
            hit_path: None,
            byte_len: None,
        }
    }
}

impl Input<'_> {
    /// Display path for matching and optional corpus rel-path for hit tracking.
    #[must_use]
    pub fn paths(&self) -> (PathBuf, Option<PathBuf>) {
        (
            self.display_path().to_path_buf(),
            self.hit_path().map(Path::to_path_buf),
        )
    }

    #[must_use]
    pub fn display_path(&self) -> &Path {
        match self {
            Self::Path { identity, .. } | Self::Bytes { identity, .. } => &identity.display_path,
        }
    }

    #[must_use]
    pub fn hit_path(&self) -> Option<&Path> {
        match self {
            Self::Path { identity, .. } | Self::Bytes { identity, .. } => {
                identity.hit_path.as_deref()
            }
        }
    }

    #[must_use]
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Path { path, identity, .. } => identity
                .byte_len
                .unwrap_or_else(|| std::fs::metadata(path).map_or(0, |m| m.len())),
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

    pub fn push_path(&mut self, path: Cow<'a, Path>, identity: InputIdentity, explicit: bool) {
        self.items.push(Input::Path {
            path,
            identity,
            explicit,
        });
    }

    pub fn push_bytes(
        &mut self,
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        identity: InputIdentity,
    ) {
        self.push_bytes_input(path, bytes, identity, false);
    }

    pub fn push_explicit_bytes(
        &mut self,
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        identity: InputIdentity,
    ) {
        self.push_bytes_input(path, bytes, identity, true);
    }

    fn push_bytes_input(
        &mut self,
        path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        identity: InputIdentity,
        explicit: bool,
    ) {
        self.items.push(Input::Bytes {
            path,
            bytes,
            identity,
            explicit,
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

/// How candidate-backed inputs are materialized before search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputExtent {
    /// Materialize every candidate-backed input before search starts.
    Complete,
    /// Materialize candidate-backed inputs one-at-a-time during search.
    Progressive,
}

/// Read transformed bytes that should be searched for one candidate.
pub trait CandidateTransform {
    /// # Errors
    ///
    /// Returns an error if transformed content cannot be read.
    fn read_candidate(&self, candidate: &Candidate) -> crate::Result<Vec<u8>>;
}

/// Plan for turning corpus candidates into [`Input`] values.
pub struct CandidateInputPlan<'a> {
    explicit_paths: &'a [PathBuf],
    path_display: PathDisplay,
    transform: Option<&'a dyn CandidateTransform>,
}

impl<'a> CandidateInputPlan<'a> {
    #[must_use]
    pub const fn new(
        explicit_paths: &'a [PathBuf],
        path_display: PathDisplay,
        transform: Option<&'a dyn CandidateTransform>,
    ) -> Self {
        Self {
            explicit_paths,
            path_display,
            transform,
        }
    }

    /// Materialize one candidate into a searchable input.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured candidate transform cannot read input bytes.
    pub fn materialize<'c>(&self, candidate: &'c Candidate) -> crate::Result<Input<'c>> {
        let explicit = self
            .explicit_paths
            .iter()
            .any(|path| path == candidate.rel_path() || path == candidate.abs_path());
        let identity = self.identity(candidate);
        if let Some(transform) = self.transform {
            let bytes = transform.read_candidate(candidate)?;
            let path = Cow::Owned(candidate.abs_path().display().to_string());
            Ok(if explicit {
                Input::Bytes {
                    path,
                    bytes: Cow::Owned(bytes),
                    identity,
                    explicit: true,
                }
            } else {
                Input::Bytes {
                    path,
                    bytes: Cow::Owned(bytes),
                    identity,
                    explicit: false,
                }
            })
        } else {
            Ok(Input::Path {
                path: Cow::Borrowed(candidate.abs_path()),
                identity,
                explicit,
            })
        }
    }

    pub(crate) fn identity(&self, candidate: &Candidate) -> InputIdentity {
        let display_path = match self.path_display {
            PathDisplay::Relative => candidate.rel_path(),
            PathDisplay::Absolute => candidate.abs_path(),
        };
        InputIdentity {
            display_path: display_path.to_path_buf(),
            hit_path: Some(candidate.rel_path().to_path_buf()),
            byte_len: candidate.cached_size(),
        }
    }
}

/// Inputs ready for [`crate::search::Searcher`] execution.
pub enum SearchInputs<'a> {
    /// Fully materialized inputs.
    Complete(Inputs<'a>),
    /// Candidate-backed inputs resolved on demand; byte streams are eager.
    Progressive {
        candidates: ProgressiveCandidates<'a>,
        streams: Inputs<'a>,
        plan: CandidateInputPlan<'a>,
    },
}

/// Candidate-backed inputs for progressive search.
pub enum ProgressiveCandidates<'a> {
    /// Already materialized candidates.
    Ready(Vec<Candidate>),
    /// Indexed ids materialized during `FirstMatch` search.
    Indexed(crate::candidates::IndexedCandidates<'a>),
}

impl ProgressiveCandidates<'_> {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Ready(candidates) => candidates.is_empty(),
            Self::Indexed(indexed) => indexed.is_empty(),
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Ready(candidates) => candidates.len(),
            Self::Indexed(indexed) => indexed.len(),
        }
    }
}

impl SearchInputs<'_> {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Complete(inputs) => inputs.is_empty(),
            Self::Progressive {
                candidates,
                streams,
                ..
            } => candidates.is_empty() && streams.is_empty(),
        }
    }
}
