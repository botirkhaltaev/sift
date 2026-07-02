use std::path::{Path, PathBuf};

use std::borrow::Cow;

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
        match self {
            Self::Path { identity, .. } | Self::Bytes { identity, .. } => {
                (identity.display_path.clone(), identity.hit_path.clone())
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
