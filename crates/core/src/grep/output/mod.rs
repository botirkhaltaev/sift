pub mod error;
pub mod format;
pub mod mode;
pub mod passthru;
pub mod style;

use crate::query::CandidateRequirement;
use mode::{GrepMode, OutputEmission, ZeroCountMode};
use passthru::PassthruMode;
use style::{GrepLineStyle, GrepRecordStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrepOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrepOutput {
    pub format: GrepOutputFormat,
    pub mode: GrepMode,
    pub emission: OutputEmission,
    pub lines: GrepLineStyle,
    pub records: GrepRecordStyle,
    pub passthru: PassthruMode,
    pub include_zero: ZeroCountMode,
}

impl GrepOutput {
    /// Whether search needs complete candidate coverage or only potential matches.
    #[must_use]
    pub(crate) const fn candidate_requirement(self) -> CandidateRequirement {
        match self.mode {
            GrepMode::Count | GrepMode::FilesWithoutMatch => CandidateRequirement::Complete,
            GrepMode::CountMatches if matches!(self.include_zero, ZeroCountMode::Include) => {
                CandidateRequirement::Complete
            }
            GrepMode::Standard
            | GrepMode::OnlyMatching
            | GrepMode::CountMatches
            | GrepMode::FilesWithMatches => CandidateRequirement::PotentialMatches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_output_defaults() {
        let output = GrepOutput::default();
        assert_eq!(output.format, GrepOutputFormat::Text);
        assert_eq!(output.mode, GrepMode::Standard);
        assert_eq!(output.emission, OutputEmission::Normal);
        assert!(matches!(output.passthru, PassthruMode::Disabled));
        assert!(matches!(output.include_zero, ZeroCountMode::Omit));
    }
}
