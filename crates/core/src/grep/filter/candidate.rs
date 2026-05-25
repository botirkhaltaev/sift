use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub rel_path: PathBuf,
    pub rel_str: String,
    pub abs_path: PathBuf,
}
