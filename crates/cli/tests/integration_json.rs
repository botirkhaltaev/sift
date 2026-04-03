//! `--json` JSON Lines output (ripgrep-compatible wire format via `grep-printer`).

mod common;

use std::ffi::OsString;
use std::fs;

use common::{BuildIndexOptions, assert_success, fresh_dir};

#[test]
fn json_emits_begin_match_end_and_summary() {
    let root = fresh_dir("json-basic");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let mut cmd = common::command(Some(&root));
    cmd.arg("--sift-dir").arg(&idx);
    cmd.args([OsString::from("hello"), OsString::from("--json")]);
    let output = cmd.output().unwrap();
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 4,
        "expected begin, match, end, summary lines; got {}: {stdout:?}",
        lines.len()
    );
    assert!(
        lines[0].contains("\"type\":\"begin\""),
        "begin: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("\"type\":\"match\""),
        "match: {}",
        lines[1]
    );
    assert!(lines[2].contains("\"type\":\"end\""), "end: {}", lines[2]);
    assert!(
        lines[lines.len() - 1].contains("\"type\":\"summary\""),
        "summary: {}",
        lines[lines.len() - 1]
    );
}

#[test]
fn json_implies_stats_on_stderr() {
    let root = fresh_dir("json-stats-stderr");
    fs::write(root.join("a.txt"), "x\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let mut cmd = common::command(Some(&root));
    cmd.arg("--sift-dir").arg(&idx);
    cmd.args([OsString::from("x"), OsString::from("--json")]);
    let output = cmd.output().unwrap();
    assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matches") && stderr.contains("bytes searched"),
        "expected --stats-style lines on stderr: {stderr:?}"
    );
}

#[test]
fn json_with_count_exits_error() {
    let root = fresh_dir("json-bad-count");
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let mut cmd = common::command(Some(&root));
    cmd.arg("--sift-dir").arg(&idx);
    cmd.args([
        OsString::from("a"),
        OsString::from("--json"),
        OsString::from("--count"),
    ]);
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--json") && stderr.contains("count"),
        "expected conflict message: {stderr:?}"
    );
}
