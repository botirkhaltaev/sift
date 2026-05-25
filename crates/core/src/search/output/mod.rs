pub mod error;
pub mod format;
pub mod mode;
pub mod passthru;
pub mod style;

use mode::{CandidateCoverage, OutputEmission, SearchMode, ZeroCountMode};
use passthru::PassthruMode;
use style::{SearchLineStyle, SearchRecordStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOutput {
    pub format: SearchOutputFormat,
    pub mode: SearchMode,
    pub emission: OutputEmission,
    pub lines: SearchLineStyle,
    pub records: SearchRecordStyle,
    pub passthru: PassthruMode,
    pub include_zero: ZeroCountMode,
}

impl Default for SearchOutput {
    fn default() -> Self {
        Self {
            format: SearchOutputFormat::Text,
            mode: SearchMode::Standard,
            emission: OutputEmission::Normal,
            lines: SearchLineStyle::default(),
            records: SearchRecordStyle::default(),
            passthru: PassthruMode::Disabled,
            include_zero: ZeroCountMode::Omit,
        }
    }
}

impl SearchOutput {
    /// Whether the query planner should request all indexed files or narrowed candidates.
    #[must_use]
    pub(crate) const fn candidate_coverage(self) -> CandidateCoverage {
        match self.mode {
            SearchMode::Count | SearchMode::FilesWithoutMatch => CandidateCoverage::Complete,
            SearchMode::CountMatches if matches!(self.include_zero, ZeroCountMode::Include) => {
                CandidateCoverage::Complete
            }
            SearchMode::Standard
            | SearchMode::OnlyMatching
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches => CandidateCoverage::Narrowed,
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
