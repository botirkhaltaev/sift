// Test helpers — not every file uses every helper.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

// ── Test Project ──────────────────────────────────────────────────────────────

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sift"))
}

/// Path to the `sift-daemon` binary for integration tests and subprocess spawns.
pub fn daemon_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_sift-daemon") {
        return PathBuf::from(path);
    }
    let sift = exe();
    #[cfg(windows)]
    {
        let deps_exe = sift.with_file_name("sift-daemon.exe");
        if deps_exe.exists() {
            return deps_exe;
        }
        if let Some(debug_bin) = sift
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("sift-daemon.exe"))
            .filter(|p| p.exists())
        {
            return debug_bin;
        }
    }
    let deps_path = sift.with_file_name("sift-daemon");
    if deps_path.exists() {
        return deps_path;
    }
    sift.parent()
        .and_then(Path::parent)
        .map(|p| p.join("sift-daemon"))
        .filter(|p| p.exists())
        .unwrap_or(deps_path)
}

/// A temporary directory with helpers to write files, build indexes, and run
/// `sift` in index or walk mode.
///
/// # Example
/// ```
/// use common::TestProject;
///
/// let p = TestProject::new("example");
/// p.write("a.txt", "hello world\n");
/// p.build_index();
/// let out = p.index_output(&["world"]);
/// assert_success(&out);
/// ```
pub struct TestProject {
    root: PathBuf,
    sift_dir: PathBuf,
    walk_sift_dir: PathBuf,
}

impl TestProject {
    pub fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sift-grep-integration-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap_or(root);
        Self {
            sift_dir: root.join(".sift"),
            walk_sift_dir: root.join(".sift-not-found"),
            root,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sift_dir(&self) -> &Path {
        &self.sift_dir
    }

    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> &Self {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content.as_ref()).unwrap();
        self
    }

    pub fn mkdir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.root.join(rel)).unwrap();
        self
    }

    pub fn build_index(&self) -> &Self {
        self.build_index_opts(Path::new("."), false)
    }

    pub fn build_index_follow(&self) -> &Self {
        self.build_index_opts(Path::new("."), true)
    }

    pub fn build_index_at(&self, corpus: &Path) -> &Self {
        self.build_index_opts(corpus, false)
    }

    fn build_index_opts(&self, corpus: &Path, follow: bool) -> &Self {
        self.build_index_with(corpus, follow, std::iter::empty::<&str>())
    }

    pub fn build_index_with<I, S>(&self, corpus: &Path, follow: bool, extra_args: I) -> &Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.sift();
        cmd.arg("--sift-dir").arg(&self.sift_dir);
        if follow {
            cmd.arg("--follow");
        }
        cmd.args(extra_args);
        let status = cmd
            .args(["index", "build", "--wait"])
            .arg(corpus)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "build index over {} failed with status {status}",
            corpus.display()
        );
        self
    }

    /// Create a `Command` with the project root as the working directory.
    pub fn sift(&self) -> Command {
        let mut cmd = Command::new(exe());
        cmd.current_dir(&self.root);
        cmd.env("SIFT_NO_DAEMON", "1");
        cmd
    }

    /// Like [`Self::sift`], but leaves the watch daemon enabled.
    pub fn sift_with_daemon(&self) -> Command {
        let mut cmd = Command::new(exe());
        cmd.current_dir(&self.root);
        cmd.env_remove("SIFT_NO_DAEMON");
        cmd.env("CARGO_BIN_EXE_sift-daemon", daemon_bin());
        cmd
    }

    /// Run `sift` in index mode, return the full `Output`.
    pub fn index_output<I, S>(&self, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.sift();
        cmd.arg("--sift-dir").arg(&self.sift_dir);
        cmd.args(args);
        cmd.output().unwrap()
    }

    /// Run `sift` in index mode, return only the exit status.
    pub fn index_status<I, S>(&self, args: I) -> ExitStatus
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.sift();
        cmd.arg("--sift-dir").arg(&self.sift_dir);
        cmd.args(args);
        cmd.status().unwrap()
    }

    /// Run `sift` in walk mode (no index), return the full `Output`.
    pub fn walk_output<I, S>(&self, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.sift();
        cmd.arg("--sift-dir").arg(&self.walk_sift_dir);
        cmd.args(args);
        cmd.output().unwrap()
    }

    /// Run `sift` in walk mode (no index), return only the exit status.
    pub fn walk_status<I, S>(&self, args: I) -> ExitStatus
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.sift();
        cmd.arg("--sift-dir").arg(&self.walk_sift_dir);
        cmd.args(args);
        cmd.status().unwrap()
    }

    /// Build the index, run both index and walk mode with the same args, and
    /// assert they produce identical exit code 0, stdout, and empty stderr.
    #[track_caller]
    pub fn assert_index_walk_same<I, S>(&self, args: I, expected: &str)
    where
        I: IntoIterator<Item = S> + Clone,
        S: AsRef<OsStr>,
    {
        self.build_index();
        let idx = self.index_output(args.clone());
        let walk = self.walk_output(args);
        for (mode, output) in [("index", &idx), ("walk", &walk)] {
            assert_success(output);
            let n = normalize_stdout(output);
            assert_eq!(n, expected, "{mode}: stdout mismatch");
            assert!(
                normalize_stderr(output).is_empty(),
                "{mode}: unexpected stderr"
            );
        }
    }
}

// ── Normalization ─────────────────────────────────────────────────────────────

pub fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\\', "/")
        .replace("//?/", "")
}

pub fn normalize_stdout(out: &Output) -> String {
    normalize(&String::from_utf8_lossy(&out.stdout))
}

pub fn normalize_stderr(out: &Output) -> String {
    normalize(&String::from_utf8_lossy(&out.stderr))
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// `path:rest` where `path` is printed relative to the corpus root (like `grep`).
pub fn rel_match(rel: &str, rest: &str) -> String {
    format!("{}:{rest}", normalize(rel))
}

pub fn abs_match(root: &Path, rel: &str, rest: &str) -> String {
    let abs = root.join(rel);
    format!("{}:{rest}", normalize(&abs.display().to_string()))
}

pub fn abs_path(root: &Path, rel: &str) -> String {
    let joined = root.join(rel);
    let canonical = joined.canonicalize().unwrap_or(joined);
    normalize(&canonical.display().to_string())
}

// ── Assertion helpers ─────────────────────────────────────────────────────────

#[track_caller]
pub fn assert_success(out: &Output) {
    assert!(
        out.status.success(),
        "expected exit 0, got {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status.code(),
        normalize_stdout(out),
        normalize_stderr(out),
    );
}

#[track_caller]
pub fn assert_exit_code(out: &Output, expected: i32) {
    let got = out.status.code().unwrap_or(-1);
    assert_eq!(
        got,
        expected,
        "exit code mismatch\n--- stdout ---\n{}\n--- stderr ---\n{}",
        normalize_stdout(out),
        normalize_stderr(out),
    );
}

#[track_caller]
pub fn assert_stdout_eq(out: &Output, expected: &str) {
    let n = normalize_stdout(out);
    assert_eq!(
        n,
        expected,
        "stdout mismatch\n--- stderr ---\n{}",
        normalize_stderr(out),
    );
}

#[track_caller]
pub fn assert_stdout_contains(out: &Output, substr: &str) {
    let n = normalize_stdout(out);
    assert!(
        n.contains(substr),
        "expected stdout to contain {substr:?}\n--- stdout ---\n{n}\n--- stderr ---\n{}",
        normalize_stderr(out),
    );
}

#[track_caller]
pub fn assert_stdout_not_contains(out: &Output, substr: &str) {
    let n = normalize_stdout(out);
    assert!(
        !n.contains(substr),
        "expected stdout to not contain {substr:?}\n--- stdout ---\n{n}",
    );
}

#[track_caller]
pub fn assert_stderr_empty(out: &Output) {
    let n = normalize_stderr(out);
    assert!(
        n.is_empty(),
        "expected empty stderr, got:\n--- stderr ---\n{n}"
    );
}

pub fn fresh_dir(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "sift-cli-integration-{name}-{}-{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub fn command(cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new(exe());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuildIndexOptions {
    pub follow_symlinks: bool,
}

impl BuildIndexOptions {
    pub fn run(self, cwd: Option<&Path>, sift_dir: &Path, corpus: &Path) {
        let mut cmd = command(cwd);
        cmd.arg("--sift-dir").arg(sift_dir);
        if self.follow_symlinks {
            cmd.arg("--follow");
        }
        let status = cmd
            .args(["index", "build", "--wait"])
            .arg(corpus)
            .status()
            .unwrap();
        assert!(status.success(), "build index failed with status {status}");
    }
}

pub fn assert_index_and_walk_output(cwd: &Path, args: &[OsString], expected_stdout: &str) {
    let idx = cwd.join(".sift");
    let missing_idx = fresh_dir("missing-index").join(".sift");
    BuildIndexOptions::default().run(Some(cwd), &idx, Path::new("."));

    let index = run_search(Some(cwd), &idx, args);
    let walk = run_search(Some(cwd), &missing_idx, args);

    for (name, output) in [("index", &index), ("walk", &walk)] {
        assert_success(output);
        assert_eq!(
            normalize_stdout(output),
            expected_stdout,
            "{name}: stdout mismatch"
        );
        assert_eq!(normalize_stderr(output), "", "{name}: stderr mismatch");
    }
}

fn run_search(cwd: Option<&Path>, sift_dir: &Path, args: &[OsString]) -> Output {
    let mut cmd = command(cwd);
    cmd.arg("--sift-dir").arg(sift_dir);
    cmd.args(args);
    cmd.output().unwrap()
}

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
