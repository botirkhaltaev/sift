//! Integration tests for the sift daemon.
//!
//! These tests use the real `sift-daemon` binary but avoid long-running
//! processes by passing `--once`.  They exercise observable behavior:
//! exit codes, lock contention, and missing metadata.

mod common;

use std::fs;
use std::process::{Command, Output};

use common::assert_exit_code;
use common::normalize;
use sift_core::{CorpusKind, IndexKind, StoreMeta};

fn daemon_bin() -> Command {
    let sift_bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_sift"));
    let daemon_exe = sift_bin.with_file_name("sift-daemon");
    Command::new(daemon_exe)
}

fn fresh_dir(name: &str) -> tempfile::TempDir {
    tempfile::TempDir::with_prefix(format!("sift-cli-daemon-{name}")).expect("create temp dir")
}

#[test]
fn daemon_errors_without_meta_or_init_root() {
    let dir = fresh_dir("no-meta");
    let sift_dir = dir.path().join(".sift");
    fs::create_dir_all(&sift_dir).unwrap();

    let out: Output = daemon_bin()
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

    let out: Output = daemon_bin()
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

    let out: Output = daemon_bin()
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
    let out = daemon_bin()
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();
    assert_exit_code(&out, 0);

    // Add a new file and update.
    fs::write(dir.path().join("b.txt"), "goodbye world\n").unwrap();
    let out = daemon_bin()
        .arg("--once")
        .arg("--sift-dir")
        .arg(&sift_dir)
        .output()
        .unwrap();
    assert_exit_code(&out, 0);
}
