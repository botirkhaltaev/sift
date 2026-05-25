use std::path::PathBuf;

use crate::search::output::style::PathDisplay;

#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub rel_path: PathBuf,
    pub rel_str: String,
    pub abs_path: PathBuf,
}

impl CandidateInfo {
    #[must_use]
    pub fn display_path(&self, display: PathDisplay, path_separator: Option<u8>) -> String {
        let raw = match display {
            PathDisplay::Absolute => self.abs_path.display().to_string(),
            PathDisplay::Relative => self.rel_path.display().to_string(),
        };
        if let Some(sep) = path_separator {
            let sep_char = sep as char;
            raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())
        } else {
            raw
        }
    }
}
