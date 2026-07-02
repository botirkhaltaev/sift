pub mod error;
pub mod format;
pub mod mode;
pub mod passthru;
pub mod style;

use mode::{OutputEmission, PrintMode, ZeroCountMode};
use passthru::PassthruMode;
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
