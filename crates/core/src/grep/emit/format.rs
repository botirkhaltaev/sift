use std::io::IsTerminal;

use crate::grep::filter::CandidateInfo;
use crate::grep::output::format::ColumnLimit;
use crate::grep::output::style::{ColorChoice, PathDisplay, RecordTerminator, SearchRecordStyle};

pub const ANSI_RESET: &[u8] = b"\x1b[0m";
pub const ANSI_PATH: &[u8] = b"\x1b[35m\x1b[1m";
pub const ANSI_LINE: &[u8] = b"\x1b[32m";

pub enum ColumnAction {
    Normal,
    Omit,
    Preview,
}

#[inline]
pub fn should_color(records: SearchRecordStyle) -> bool {
    match records.color {
        ColorChoice::Never => false,
        ColorChoice::Always => true,
        ColorChoice::Auto => std::io::stdout().is_terminal(),
    }
}

#[inline]
pub fn display_path_for_candidate(
    candidate: &CandidateInfo,
    display: PathDisplay,
    path_separator: Option<u8>,
) -> String {
    let raw = match display {
        PathDisplay::Absolute => candidate.abs_path.display().to_string(),
        PathDisplay::Relative => candidate.rel_path.display().to_string(),
    };
    if let Some(sep) = path_separator {
        let sep_char = sep as char;
        raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())
    } else {
        raw
    }
}

#[inline]
pub fn write_line_terminator(out: &mut Vec<u8>, terminator: RecordTerminator) {
    match terminator {
        RecordTerminator::Nul => out.push(0),
        RecordTerminator::Newline => out.push(b'\n'),
    }
}

pub fn check_max_columns(line: &[u8], columns: Option<ColumnLimit>) -> ColumnAction {
    let Some(limit) = columns else {
        return ColumnAction::Normal;
    };
    let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    if trimmed.len() as u64 > limit.max {
        match limit.overflow {
            crate::grep::output::format::ColumnOverflow::Preview => ColumnAction::Preview,
            crate::grep::output::format::ColumnOverflow::Omit => ColumnAction::Omit,
        }
    } else {
        ColumnAction::Normal
    }
}

pub fn truncate_line(line: &[u8], max_columns: u64) -> Vec<u8> {
    let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    let limit = usize::try_from(max_columns).unwrap_or(usize::MAX);
    let mut out = Vec::with_capacity(limit.saturating_add(30));
    out.extend_from_slice(&trimmed[..limit.min(trimmed.len())]);
    out.extend_from_slice(b" [... omitted end ...]");
    out.push(b'\n');
    out
}

pub fn sum_candidate_file_bytes(candidates: &[CandidateInfo]) -> u64 {
    candidates.iter().fold(0u64, |acc, c| {
        acc + std::fs::metadata(&c.abs_path).map_or(0, |m| m.len())
    })
}
