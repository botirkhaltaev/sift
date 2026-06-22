pub mod error;
pub mod format;
pub mod mode;
pub mod passthru;
pub mod style;

use crate::query::CandidateRequirement;
use mode::{OutputEmission, SearchMode, ZeroCountMode};
use passthru::PassthruMode;
use style::{SearchLineStyle, SearchRecordStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOutput {
    pub format: SearchOutputFormat,
    pub mode: SearchMode,
    pub emission: OutputEmission,
    pub lines: SearchLineStyle,
    pub records: SearchRecordStyle,
    pub passthru: PassthruMode,
    pub include_zero: ZeroCountMode,
}

impl SearchOutput {
    /// Whether search needs complete candidate coverage or only potential matches.
    #[must_use]
    pub(crate) const fn candidate_requirement(self) -> CandidateRequirement {
        match self.mode {
            SearchMode::Count | SearchMode::FilesWithoutMatch => CandidateRequirement::Complete,
            SearchMode::CountMatches if matches!(self.include_zero, ZeroCountMode::Include) => {
                CandidateRequirement::Complete
            }
            SearchMode::Standard
            | SearchMode::OnlyMatching
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches => CandidateRequirement::PotentialMatches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_output_defaults() {
        let output = SearchOutput::default();
        assert_eq!(output.format, SearchOutputFormat::Text);
        assert_eq!(output.mode, SearchMode::Standard);
        assert_eq!(output.emission, OutputEmission::Normal);
        assert!(matches!(output.passthru, PassthruMode::Disabled));
        assert!(matches!(output.include_zero, ZeroCountMode::Omit));
    }
}
