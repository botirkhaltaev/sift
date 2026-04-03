use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TMP_ID: AtomicUsize = AtomicUsize::new(0);

fn sift_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sift"))
}

pub fn fresh_dir(name: &str) -> PathBuf {
    let id = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "sift-cli-integration-{name}-{}-{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub fn command(cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new(sift_exe());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

/// Build-time flags for `sift build` in integration tests. Add fields as the CLI grows.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuildIndexOptions {
    /// Pass `--follow` / `-L` to `sift build`.
    pub follow_symlinks: bool,
}

impl BuildIndexOptions {
    pub fn run(self, cwd: Option<&Path>, sift_dir: &Path, corpus: &Path) {
        let mut cmd = command(cwd);
        cmd.arg("--sift-dir").arg(sift_dir);
        if self.follow_symlinks {
            cmd.arg("--follow");
        }
        let status = cmd.arg("build").arg(corpus).status().unwrap();
        assert!(status.success(), "build index failed with status {status}");
    }
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(dead_code)]
pub fn assert_index_and_walk_output(cwd: &Path, args: &[OsString], expected_stdout: &str) {
    let idx = cwd.join(".sift");
    let missing_idx = fresh_dir("missing-index").join(".sift");
    BuildIndexOptions::default().run(Some(cwd), &idx, Path::new("."));

    let index = run_search(Some(cwd), &idx, args);
    let walk = run_search(Some(cwd), &missing_idx, args);

    for (name, output) in [("index", &index), ("walk", &walk)] {
        assert_success(output);
        assert_eq!(
            normalized_stdout(output),
            expected_stdout,
            "{name}: stdout mismatch"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "",
            "{name}: stderr mismatch"
        );
    }
}

#[allow(dead_code)]
fn run_search(cwd: Option<&Path>, sift_dir: &Path, args: &[OsString]) -> Output {
    let mut cmd = command(cwd);
    cmd.arg("--sift-dir").arg(sift_dir);
    cmd.args(args);
    cmd.output().unwrap()
}

fn normalize_path_str(path: &str) -> String {
    let mut normalized = path.replace("\r\n", "\n").replace('\\', "/");
    normalized = normalized.replace("//?/", "");
    normalized
}

pub fn normalized_stdout(output: &Output) -> String {
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    normalize_path_str(&raw)
}

#[allow(dead_code)]
pub fn abs(root: &Path, rel: &str) -> String {
    let joined = root.join(rel);
    let canonical = joined.canonicalize().unwrap_or(joined);
    normalize_path_str(&canonical.display().to_string())
}

#[allow(dead_code)]
pub fn abs_match(root: &Path, rel: &str, text: &str) -> String {
    format!("{}:{text}", abs(root, rel))
}

/// `path:rest` where `path` is printed relative to the corpus root (like `grep`).
#[allow(dead_code)]
pub fn rel_match(rel: &str, rest: &str) -> String {
    format!("{}:{rest}", normalize_path_str(rel))
}

#[allow(dead_code)]
pub fn line_path<'a>(line: &'a str, candidates: &[String]) -> &'a str {
    candidates
        .iter()
        .find_map(|candidate| {
            if line == candidate || line.starts_with(&format!("{candidate}:")) {
                Some(&line[..candidate.len()])
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("could not match output line to any candidate path: {line}"))
}
