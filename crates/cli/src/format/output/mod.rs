pub mod error;
pub mod format;
pub mod mode;
pub mod passthru;
pub mod style;

use mode::{PrintMode, OutputEmission, ZeroCountMode};
use passthru::PassthruMode;
use sift_core::grep::CandidateScope;
use style::{PrintLineStyle, PrintRecordStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintSpec {
    pub format: PrintFormat,
    pub mode: PrintMode,
    pub emission: OutputEmission,
    pub lines: PrintLineStyle,
    pub records: PrintRecordStyle,
    pub passthru: PassthruMode,
    pub include_zero: ZeroCountMode,
}

impl PrintSpec {
    /// Scan scope implied by this output configuration.
    #[must_use]
    pub(crate) const fn candidate_scope(&self) -> CandidateScope {
        match self.mode {
            PrintMode::Count | PrintMode::FilesWithoutMatch => CandidateScope::All,
            PrintMode::CountMatches if matches!(self.include_zero, ZeroCountMode::Include) => {
                CandidateScope::All
            }
            PrintMode::Standard
            | PrintMode::OnlyMatching
            | PrintMode::CountMatches
            | PrintMode::FilesWithMatches => CandidateScope::Indexed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_output_defaults() {
        let output = PrintSpec::default();
        assert_eq!(output.format, PrintFormat::Text);
        assert_eq!(output.mode, PrintMode::Standard);
        assert_eq!(output.emission, OutputEmission::Normal);
        assert!(matches!(output.passthru, PassthruMode::Disabled));
        assert!(matches!(output.include_zero, ZeroCountMode::Omit));
    }
}
