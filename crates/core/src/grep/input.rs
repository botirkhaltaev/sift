use std::borrow::Cow;

use crate::Candidate;

#[derive(Clone)]
pub struct CandidateContent {
    pub candidate: Candidate,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct GrepStream<'a> {
    pub display_path: &'a str,
    pub bytes: &'a [u8],
}

pub(crate) enum GrepInput<'a> {
    Path {
        candidate: &'a Candidate,
    },
    Bytes {
        display_path: Cow<'a, str>,
        bytes: Cow<'a, [u8]>,
        candidate: Option<&'a Candidate>,
    },
}

impl GrepInput<'_> {
    #[must_use]
    pub(crate) fn bytes(&self) -> u64 {
        match self {
            Self::Path { candidate } => candidate
                .cached_size()
                .unwrap_or_else(|| std::fs::metadata(candidate.abs_path()).map_or(0, |m| m.len())),
            Self::Bytes { bytes, .. } => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        }
    }
}

pub(crate) struct GrepInputs<'a> {
    inputs: Vec<GrepInput<'a>>,
}

impl<'a> GrepInputs<'a> {
    #[must_use]
    pub(crate) const fn empty() -> Self {
        Self { inputs: Vec::new() }
    }

    #[must_use]
    pub(crate) fn from_candidates(candidates: &'a [Candidate]) -> Self {
        Self {
            inputs: candidates
                .iter()
                .map(|candidate| GrepInput::Path { candidate })
                .collect(),
        }
    }

    #[must_use]
    pub(crate) fn from_transformed(contents: &'a [CandidateContent]) -> Self {
        Self {
            inputs: contents
                .iter()
                .map(|content| GrepInput::Bytes {
                    display_path: Cow::Borrowed(""),
                    bytes: Cow::Borrowed(content.bytes.as_slice()),
                    candidate: Some(&content.candidate),
                })
                .collect(),
        }
    }

    pub(crate) fn push_streams(&mut self, streams: &'a [GrepStream<'a>]) {
        self.inputs
            .extend(streams.iter().map(|stream| GrepInput::Bytes {
                display_path: Cow::Borrowed(stream.display_path),
                bytes: Cow::Borrowed(stream.bytes),
                candidate: None,
            }));
    }

    #[must_use]
    pub(crate) const fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.inputs.len()
    }

    #[must_use]
    pub(crate) fn byte_count(&self) -> u64 {
        self.inputs.iter().map(GrepInput::bytes).sum()
    }

    #[must_use]
    pub(crate) fn as_slice(&self) -> &[GrepInput<'a>] {
        &self.inputs
    }
}
