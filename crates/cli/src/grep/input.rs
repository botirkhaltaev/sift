use std::io::Read;
use std::path::Path;
use std::process::Command;

use globset::{Glob, GlobSet, GlobSetBuilder};
use grep_cli::DecompressionReaderBuilder;
use sift_core::Candidate;
use sift_core::grep::CandidateContentSource;
use sift_core::search::CandidateContent;

#[derive(Debug, Clone, Default)]
pub struct ContentConfig {
    pub search_zip: bool,
    pub pre: Option<String>,
    pub pre_globs: Vec<String>,
}

impl ContentConfig {
    /// # Errors
    ///
    /// Returns an error when a `--pre-glob` pattern is not a valid glob.
    pub fn source(&self) -> sift_core::Result<Option<TransformedContent>> {
        if !self.enabled() {
            return Ok(None);
        }
        Ok(Some(TransformedContent {
            search_zip: self.search_zip,
            pre: if let Some(command) = &self.pre {
                Some(Preprocessor {
                    command: command.clone(),
                    globs: PreprocessorGlobs::new(&self.pre_globs)?,
                })
            } else {
                None
            },
            decompressor: DecompressionReaderBuilder::new(),
        }))
    }

    const fn enabled(&self) -> bool {
        self.search_zip || self.pre.is_some()
    }
}

pub struct TransformedContent {
    search_zip: bool,
    pre: Option<Preprocessor>,
    decompressor: DecompressionReaderBuilder,
}

impl CandidateContentSource for TransformedContent {
    fn read(&self, candidates: &[Candidate]) -> sift_core::Result<Vec<CandidateContent>> {
        candidates
            .iter()
            .map(|candidate| {
                let bytes = self.read_candidate(candidate)?;
                Ok(CandidateContent {
                    candidate: candidate.clone(),
                    bytes,
                })
            })
            .collect()
    }
}

impl TransformedContent {
    fn read_candidate(&self, candidate: &Candidate) -> sift_core::Result<Vec<u8>> {
        if let Some(pre) = &self.pre
            && pre.matches(candidate)
        {
            return pre.read(candidate.abs_path());
        }
        if self.search_zip {
            return self.read_decompressed(candidate.abs_path());
        }
        Ok(std::fs::read(candidate.abs_path())?)
    }

    fn read_decompressed(&self, path: &Path) -> sift_core::Result<Vec<u8>> {
        let mut reader = self
            .decompressor
            .build(path)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

struct Preprocessor {
    command: String,
    globs: PreprocessorGlobs,
}

impl Preprocessor {
    fn matches(&self, candidate: &Candidate) -> bool {
        self.globs.matches(candidate.rel_path())
    }

    fn read(&self, path: &Path) -> sift_core::Result<Vec<u8>> {
        let mut parts = self.command.split_whitespace();
        let Some(program) = parts.next() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--pre command is empty",
            )
            .into());
        };
        let output = Command::new(program).args(parts).arg(path).output()?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(std::io::Error::other(format!(
                "preprocessor `{}` failed for {}: {}",
                self.command,
                path.display(),
                stderr.trim()
            ))
            .into())
        }
    }
}

struct PreprocessorGlobs {
    globs: Option<GlobSet>,
}

impl PreprocessorGlobs {
    fn new(patterns: &[String]) -> sift_core::Result<Self> {
        if patterns.is_empty() {
            return Ok(Self { globs: None });
        }
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = Glob::new(pattern).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid --pre-glob `{pattern}`: {err}"),
                )
            })?;
            builder.add(glob);
        }
        Ok(Self {
            globs: Some(builder.build().map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid --pre-glob set: {err}"),
                )
            })?),
        })
    }

    fn matches(&self, path: &Path) -> bool {
        self.globs.as_ref().is_none_or(|globs| {
            let rel = path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            globs.is_match(&rel)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_globs_match_forward_slash_paths() {
        let globs = PreprocessorGlobs::new(&["src/*.txt".to_string()]).unwrap();
        assert!(globs.matches(Path::new("src/a.txt")));
        assert!(!globs.matches(Path::new("src/a.rs")));
    }

    #[test]
    fn empty_pre_globs_match_everything() {
        let globs = PreprocessorGlobs::new(&[]).unwrap();
        assert!(globs.matches(Path::new("a.rs")));
    }

    #[test]
    fn invalid_pre_glob_errors() {
        let Err(err) = PreprocessorGlobs::new(&["[".to_string()]) else {
            panic!("invalid glob unexpectedly succeeded");
        };
        assert!(err.to_string().contains("invalid --pre-glob"));
    }
}
