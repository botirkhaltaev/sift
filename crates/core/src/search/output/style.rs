use std::io::IsTerminal;

use crate::search::output::format::ColumnLimit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilenameMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Never,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathDisplay {
    #[default]
    Relative,
    Absolute,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct LineStyleFlags: u8 {
        const HEADING     = 1 << 0;
        const LINE_NUMBER = 1 << 1;
        const BYTE_OFFSET = 1 << 2;
        const TRIM        = 1 << 3;
        const COLUMN      = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLineStyle {
    pub filename_mode: FilenameMode,
    pub flags: LineStyleFlags,
    pub path_display: PathDisplay,
    pub columns: Option<ColumnLimit>,
}

impl SearchLineStyle {
    #[must_use]
    pub const fn heading(self) -> bool {
        self.flags.contains(LineStyleFlags::HEADING)
    }

    #[must_use]
    pub const fn line_number(self) -> bool {
        self.flags.contains(LineStyleFlags::LINE_NUMBER)
    }

    #[must_use]
    pub const fn byte_offset(self) -> bool {
        self.flags.contains(LineStyleFlags::BYTE_OFFSET)
    }

    #[must_use]
    pub const fn trim(self) -> bool {
        self.flags.contains(LineStyleFlags::TRIM)
    }
}

impl Default for SearchLineStyle {
    fn default() -> Self {
        Self {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::default(),
            columns: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordTerminator {
    Newline,
    Nul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRecordStyle {
    pub terminator: RecordTerminator,
    pub color: ColorChoice,
    pub path_separator: Option<u8>,
}

impl SearchRecordStyle {
    #[must_use]
    pub fn should_color(&self) -> bool {
        match self.color {
            ColorChoice::Never => false,
            ColorChoice::Always => true,
            ColorChoice::Auto => std::io::stdout().is_terminal(),
        }
    }
}

impl RecordTerminator {
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self {
            Self::Nul => out.push(0),
            Self::Newline => out.push(b'\n'),
        }
    }
}

impl Default for SearchRecordStyle {
    fn default() -> Self {
        Self {
            terminator: RecordTerminator::Newline,
            color: ColorChoice::Auto,
            path_separator: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSeparators {
    pub context_separator: Option<Vec<u8>>,
    pub field_match_separator: Vec<u8>,
    pub field_context_separator: Vec<u8>,
}

impl Default for SearchSeparators {
    fn default() -> Self {
        Self {
            context_separator: Some(b"--".to_vec()),
            field_match_separator: b":".to_vec(),
            field_context_separator: b"-".to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_line_style_defaults() {
        let style = SearchLineStyle::default();
        assert!(!style.heading());
        assert!(!style.line_number());
        assert!(!style.byte_offset());
        assert!(!style.trim());
    }

    #[test]
    fn search_record_style_defaults() {
        let style = SearchRecordStyle::default();
        assert!(matches!(style.terminator, RecordTerminator::Newline));
        assert_eq!(style.color, ColorChoice::Auto);
        assert!(style.path_separator.is_none());
    }

    #[test]
    fn search_separators_defaults() {
        let sep = SearchSeparators::default();
        assert_eq!(sep.context_separator, Some(b"--".to_vec()));
        assert_eq!(sep.field_match_separator, b":".to_vec());
        assert_eq!(sep.field_context_separator, b"-".to_vec());
    }
}
