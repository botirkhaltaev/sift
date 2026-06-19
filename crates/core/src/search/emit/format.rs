use crate::Candidate;

pub const ANSI_RESET: &[u8] = b"\x1b[0m";
pub const ANSI_PATH: &[u8] = b"\x1b[35m\x1b[1m";
pub const ANSI_LINE: &[u8] = b"\x1b[32m";

impl Candidate {
    /// Sum on-disk byte sizes for all candidates (used for search stats).
    #[must_use]
    pub fn total_file_bytes(candidates: &[Self]) -> u64 {
        candidates.iter().fold(0u64, |acc, c| {
            acc + std::fs::metadata(c.abs_path()).map_or(0, |m| m.len())
        })
    }
}
