use std::borrow::Cow;
use std::path::PathBuf;

use crate::candidates::ResolvedCandidates;
use crate::corpus::candidate::PathDisplay;
use crate::search::{
    CandidateInputPlan, CandidateTransform, InputExtent, InputIdentity, Inputs,
    ProgressiveCandidates, SearchInputs,
};

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

    /// Resolve candidate paths and byte streams into executable search inputs.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured candidate transform cannot read input bytes.
    pub fn resolve(
        &'a self,
        candidates: ResolvedCandidates<'a>,
        extent: InputExtent,
    ) -> crate::Result<SearchInputs<'a>> {
        match extent {
            InputExtent::Complete => self.resolve_complete(candidates),
            InputExtent::Progressive => Ok(self.resolve_progressive(candidates)),
        }
    }

    fn resolve_complete(
        &'a self,
        candidates: ResolvedCandidates<'a>,
    ) -> crate::Result<SearchInputs<'a>> {
        let ready = match candidates {
            ResolvedCandidates::Ready(candidates) => candidates,
            ResolvedCandidates::Indexed(indexed) => indexed.materialize_all(),
        };
        let plan = self.plan();
        let mut inputs = Inputs::with_capacity(ready.len() + self.streams.len());
        for candidate in ready {
            let explicit = self
                .explicit_paths
                .iter()
                .any(|path| path == candidate.rel_path() || path == candidate.abs_path());
            if let Some(transform) = self.candidate_transform {
                let bytes = transform.read_candidate(&candidate)?;
                let path = Cow::Owned(candidate.abs_path().display().to_string());
                let identity = plan.identity(&candidate);
                if explicit {
                    inputs.push_explicit_bytes(path, Cow::Owned(bytes), identity);
                } else {
                    inputs.push_bytes(path, Cow::Owned(bytes), identity);
                }
            } else {
                inputs.push_path(
                    Cow::Owned(candidate.abs_path().to_path_buf()),
                    plan.identity(&candidate),
                    explicit,
                );
            }
        }
        self.push_streams(&mut inputs);
        Ok(SearchInputs::Complete(inputs))
    }

    fn resolve_progressive(&'a self, candidates: ResolvedCandidates<'a>) -> SearchInputs<'a> {
        let mut streams = Inputs::with_capacity(self.streams.len());
        self.push_streams(&mut streams);
        let candidates = match candidates {
            ResolvedCandidates::Ready(candidates) => ProgressiveCandidates::Ready(candidates),
            ResolvedCandidates::Indexed(indexed) => ProgressiveCandidates::Indexed(indexed),
        };
        SearchInputs::Progressive {
            candidates,
            streams,
            plan: self.plan(),
        }
    }

    fn plan(&'a self) -> CandidateInputPlan<'a> {
        CandidateInputPlan::new(
            &self.explicit_paths,
            self.path_display,
            self.candidate_transform,
        )
    }

    fn push_streams(&self, inputs: &mut Inputs<'a>) {
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
    }
}
