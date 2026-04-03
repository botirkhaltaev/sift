//! `--stats` prints match and file counts to stderr (ripgrep-style).

mod common;

use std::ffi::OsString;
use std::fs;

use common::{BuildIndexOptions, assert_success, fresh_dir};

#[test]
fn stats_reports_matches_and_files_searched() {
    let root = fresh_dir("stats-basic");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();
    fs::write(root.join("b.txt"), "goodbye\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let mut cmd = common::command(Some(&root));
    cmd.arg("--sift-dir").arg(&idx);
    cmd.args([OsString::from("hello"), OsString::from("--stats")]);
    let output = cmd.output().unwrap();
    assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 matches"),
        "expected match count in stderr, got: {stderr:?}"
    );
    // Trigram narrowing only opens candidate files that can match; `b.txt` is not searched.
    assert!(
        stderr.contains("1 files searched"),
        "expected files searched in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("bytes searched"),
        "expected bytes searched line in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("s elapsed"),
        "expected elapsed line in stderr, got: {stderr:?}"
    );
}
