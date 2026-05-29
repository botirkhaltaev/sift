//! Integration tests for the sift daemon.
//!
//! These tests use the real `sift-daemon` binary but avoid long-running
//! processes by passing `--once`.  The `daemon_reindexes_on_file_changes`
//! test runs a real long-lived daemon and verifies index updates.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::assert_exit_code;
use common::normalize;
use sift_core::{CorpusKind, IndexKind, StoreMeta};

fn sift_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sift"))
}

fn daemon_bin() -> PathBuf {
    // CARGO_BIN_EXE_sift is set by cargo to the path of the sift binary
    // (e.g. target/debug/deps/sift-<hash>).
    // When built via `cargo build`, the daemon binary is at
    // target/debug/sift-daemon.  Try the deps/ directory first (it is set
    // when cargo builds both binaries for test), then fall back to
    // target/debug/ (when `cargo build --bin sift-daemon` was run first).
    let sift = sift_bin();
    let deps_path = sift.with_file_name("sift-daemon");
    if deps_path.exists() {
        return deps_path;
    }
    // Walk up from target/debug/deps/ to target/debug/
    sift.parent()
        .and_then(Path::parent)
        .map(|p| p.join("sift-daemon"))
        .filter(|p| p.exists())
        .unwrap_or(deps_path)
}

fn fresh_dir(name: &str) -> tempfile::TempDir {
    tempfile::TempDir::with_prefix(format!("sift-cli-daemon-{name}")).expect("create temp dir")
}

fn search(sift_dir: &Path, pattern: &str) -> Output {
    Command::new(sift_bin())
        .arg("--sift-dir")
        .arg(sift_dir)
        .arg(pattern)
        .env("SIFT_NO_DAEMON", "1")
        .output()
        .unwrap()
}

fn poll_until<F>(sift_dir: &Path, pattern: &str, timeout: Duration, mut predicate: F)
where
    F: FnMut(&Output) -> bool,
{
    let start = Instant::now();
    while start.elapsed() < timeout {
        let out = search(sift_dir, pattern);
        if predicate(&out) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let last = search(sift_dir, pattern);
    panic!(
        "timed out after {timeout:?} waiting for pattern {pattern:?}\n\
         last exit: {:?}\nlast stdout:\n{}\nlast stderr:\n{}",
        last.status.code(),
        normalize(&String::from_utf8_lossy(&last.stdout)),
        normalize(&String::from_utf8_lossy(&last.stderr)),
    );
}

#[test]
fn daemon_errors_without_meta_or_init_root() {
    let dir = fresh_dir("no-meta");
    let sift_dir = dir.path().join(".sift");
    fs::create_dir_all(&sift_dir).unwrap();

    let out: Output = Command::new(daemon_bin())
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();

    assert_exit_code(&out, 1);
    let stderr = normalize(&String::from_utf8_lossy(&out.stderr));
    assert!(
        stderr.contains("no store metadata"),
        "expected stderr to contain 'no store metadata', got: {stderr}"
    );
}

#[test]
fn daemon_exits_zero_when_lock_held() {
    let dir = fresh_dir("lock-held");
    let sift_dir = dir.path().join(".sift");
    fs::create_dir_all(&sift_dir).unwrap();

    // Hold the daemon lock before starting the daemon.
    let lock_path = sift_dir.join("lock");
    let mut lock = fslock::LockFile::open(&lock_path).unwrap();
    lock.try_lock().unwrap();

    let out: Output = Command::new(daemon_bin())
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();

    assert_exit_code(&out, 0);
}

#[test]
fn daemon_once_builds_initial_index() {
    let dir = fresh_dir("once-build");
    let sift_dir = dir.path().join(".sift");

    let meta = StoreMeta::new(
        dir.path().to_path_buf(),
        CorpusKind::Directory,
        false,
        vec![IndexKind::Trigram],
    );
    std::fs::create_dir_all(&sift_dir).unwrap();
    StoreMeta::write(&meta, &sift_dir).unwrap();

    // Write a file so there's something to index.
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();

    let out: Output = Command::new(daemon_bin())
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();

    assert_exit_code(&out, 0);
}

#[test]
fn daemon_once_updates_existing_index() {
    let dir = fresh_dir("once-update");
    let sift_dir = dir.path().join(".sift");

    let meta = StoreMeta::new(
        dir.path().to_path_buf(),
        CorpusKind::Directory,
        false,
        vec![IndexKind::Trigram],
    );
    std::fs::create_dir_all(&sift_dir).unwrap();
    StoreMeta::write(&meta, &sift_dir).unwrap();

    // Write a file and build the initial index.
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    let out = Command::new(daemon_bin())
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();
    assert_exit_code(&out, 0);

    // Add a new file and update.
    fs::write(dir.path().join("b.txt"), "goodbye world\n").unwrap();
    let out = Command::new(daemon_bin())
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();
    assert_exit_code(&out, 0);
}

#[test]
fn daemon_reindexes_on_file_changes() {
    let dir = fresh_dir("daemon-live");
    let root = dir.path().to_path_buf();
    let sift_dir = root.join(".sift");

    // Set up project with initial file and metadata.
    let meta = StoreMeta::new(
        root.clone(),
        CorpusKind::Directory,
        false,
        vec![IndexKind::Trigram],
    );
    fs::create_dir_all(&sift_dir).unwrap();
    StoreMeta::write(&meta, &sift_dir).unwrap();
    fs::write(root.join("a.txt"), "initial_token\n").unwrap();

    // Build index without spawning daemon.
    let status = Command::new(sift_bin())
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("build")
        .arg(&root)
        .env("SIFT_NO_DAEMON", "1")
        .status()
        .unwrap();
    assert!(status.success(), "initial build failed");

    // Verify baseline search works.
    let out = search(&sift_dir, "initial_token");
    assert_exit_code(&out, 0);
    assert!(
        normalize(&String::from_utf8_lossy(&out.stdout)).contains("a.txt"),
        "expected a.txt in baseline search"
    );

    // Run the daemon event loop in a thread.
    let daemon_sift_dir = sift_dir.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let s = Arc::clone(&shutdown);

    let handle = std::thread::spawn(move || {
        let config = sift_cli::daemon::DaemonRunConfig {
            sift_dir: daemon_sift_dir,
            init_root: None,
        };
        let runner = sift_cli::daemon::DaemonRunner::new(config);
        runner.run_until(&s).unwrap();
    });

    std::thread::sleep(Duration::from_millis(500));

    let lock_path = sift_dir.join("lock");
    let mut lock = fslock::LockFile::open(&lock_path).unwrap();
    assert!(!lock.try_lock().unwrap(), "daemon did not acquire lock");

    // --- Add a file ---
    fs::write(root.join("b.txt"), "added_by_daemon\n").unwrap();
    poll_until(
        &sift_dir,
        "added_by_daemon",
        Duration::from_secs(5),
        |out| {
            out.status.success()
                && normalize(&String::from_utf8_lossy(&out.stdout)).contains("b.txt")
        },
    );

    // --- Modify a file ---
    fs::write(root.join("a.txt"), "initial_token modified_by_daemon\n").unwrap();
    poll_until(
        &sift_dir,
        "modified_by_daemon",
        Duration::from_secs(5),
        |out| {
            out.status.success()
                && normalize(&String::from_utf8_lossy(&out.stdout)).contains("a.txt")
        },
    );

    // --- Delete a file ---
    fs::remove_file(root.join("b.txt")).unwrap();
    poll_until(
        &sift_dir,
        "added_by_daemon",
        Duration::from_secs(5),
        |out| !out.status.success(),
    );

    shutdown.store(true, Ordering::Relaxed);
    handle.join().unwrap();
}
