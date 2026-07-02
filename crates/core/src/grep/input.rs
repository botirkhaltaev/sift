use std::borrow::Cow;
use std::path::PathBuf;

use crate::corpus::Candidate;
use crate::corpus::candidate::PathDisplay;
use crate::search::{InputIdentity, Inputs};

pub trait CandidateTransform {
    /// Read the transformed bytes that should be searched for one candidate.
    ///
    /// # Errors
    ///
    /// Returns an error if transformed content cannot be read.
    fn read_candidate(&self, candidate: &Candidate) -> crate::Result<Vec<u8>>;
}

pub struct ByteInput<'a> {
    pub path: Cow<'a, str>,
    pub bytes: Cow<'a, [u8]>,
    pub explicit: bool,
}

#[derive(Default)]
pub struct InputRequest<'a> {
    streams: Vec<ByteInput<'a>>,
    explicit_paths: Vec<PathBuf>,
    candidate_transform: Option<&'a dyn CandidateTransform>,
    path_display: PathDisplay,
}

impl<'a> InputRequest<'a> {
    #[must_use]
    pub const fn from_candidates() -> Self {
        Self {
            streams: Vec::new(),
            explicit_paths: Vec::new(),
            candidate_transform: None,
            path_display: PathDisplay::Relative,
        }
    }

    #[must_use]
    pub fn with_stream(mut self, stream: ByteInput<'a>) -> Self {
        self.streams.push(stream);
        self
    }

    #[must_use]
    pub fn with_explicit_path(mut self, path: PathBuf) -> Self {
        self.explicit_paths.push(path);
        self
    }

    #[must_use]
    pub fn with_candidate_transform(mut self, transform: &'a dyn CandidateTransform) -> Self {
        self.candidate_transform = Some(transform);
        self
    }

    #[must_use]
    pub const fn with_path_display(mut self, path_display: PathDisplay) -> Self {
        self.path_display = path_display;
        self
    }

    /// Resolve candidate paths and byte streams into executable grep inputs.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured candidate transform cannot read input bytes.
    pub fn resolve<'c>(&'a self, candidates: &'c [Candidate]) -> crate::Result<Inputs<'c>>
    where
        'a: 'c,
    {
        let mut inputs = Inputs::with_capacity(candidates.len() + self.streams.len());
        for candidate in candidates {
            let explicit = self
                .explicit_paths
                .iter()
                .any(|path| path == candidate.rel_path() || path == candidate.abs_path());
            if let Some(transform) = self.candidate_transform {
                let bytes = transform.read_candidate(candidate)?;
                let path = Cow::Owned(candidate.abs_path().display().to_string());
                let identity = candidate_identity(candidate, self.path_display);
                if explicit {
                    inputs.push_explicit_bytes(path, Cow::Owned(bytes), identity);
                } else {
                    inputs.push_bytes(path, Cow::Owned(bytes), identity);
                }
            } else {
                inputs.push_path(
                    Cow::Borrowed(candidate.abs_path()),
                    candidate_identity(candidate, self.path_display),
                    explicit,
                );
            }
        }
        for stream in &self.streams {
            if stream.explicit {
                inputs.push_explicit_bytes(
                    stream.path.clone(),
                    stream.bytes.clone(),
                    InputIdentity::from_name(stream.path.as_ref()),
                );
            } else {
                inputs.push_bytes(
                    stream.path.clone(),
                    stream.bytes.clone(),
                    InputIdentity::from_name(stream.path.as_ref()),
                );
            }
        }
        Ok(inputs)
    }
}

fn candidate_identity(candidate: &Candidate, path_display: PathDisplay) -> InputIdentity {
    let display_path = match path_display {
        PathDisplay::Relative => candidate.rel_path(),
        PathDisplay::Absolute => candidate.abs_path(),
    };
    InputIdentity {
        display_path: display_path.to_path_buf(),
        hit_path: Some(candidate.rel_path().to_path_buf()),
        byte_len: candidate.cached_size(),
    }
}
