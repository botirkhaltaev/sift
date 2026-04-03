//! Context lines (`-A` / `-B` / `-C`).

mod common;

use std::fs;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout};

#[test]
fn context_c_shows_surrounding_lines() {
    let root = fresh_dir("integration-context-c");
    fs::write(root.join("t.txt"), "alpha\nbeta match\ngamma\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-C", "1", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let expected = "t.txt-1-alpha\nt.txt:2:beta match\nt.txt-3-gamma\n";
    assert_eq!(normalized_stdout(&output), expected);
}

#[test]
fn context_a_shows_lines_after_match() {
    let root = fresh_dir("integration-context-a");
    fs::write(root.join("t.txt"), "alpha\nbeta match\ngamma\ndelta\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-A", "2", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let expected = "t.txt:2:beta match\nt.txt-3-gamma\nt.txt-4-delta\n";
    assert_eq!(normalized_stdout(&output), expected);
}

#[test]
fn context_b_shows_lines_before_match() {
    let root = fresh_dir("integration-context-b");
    fs::write(root.join("t.txt"), "alpha\nbeta match\ngamma\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-B", "2", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let stdout = normalized_stdout(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "expected 2 lines (1 before + match), got: {lines:?}"
    );
    assert!(
        lines[0].contains("alpha"),
        "expected 'alpha' in line 1, got: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("match"),
        "expected 'match' in line 2, got: {}",
        lines[1]
    );
}

#[test]
fn context_break_separates_match_groups() {
    let root = fresh_dir("integration-context-break");
    fs::write(
        root.join("t.txt"),
        "line1 match\nline2 not\nline3 not\nline4 not\nline5 match\nline6 not\nline7 not\nline8 match\n",
    )
    .unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-B", "1", "-A", "1", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let stdout = normalized_stdout(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 7,
        "expected at least 7 lines, got {}: {lines:?}",
        lines.len()
    );
}

#[test]
fn context_c_with_filename_uses_hyphen_separator() {
    let root = fresh_dir("integration-context-filename");
    fs::write(root.join("t.txt"), "alpha\nbeta match\ngamma\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-n", "-C", "1", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let stdout = normalized_stdout(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].starts_with("t.txt-1-"),
        "expected 't.txt-1-' prefix for context line, got: {}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("t.txt:2:"),
        "expected 't.txt:2:' prefix for match line, got: {}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("t.txt-3-"),
        "expected 't.txt-3-' prefix for context line, got: {}",
        lines[2]
    );
}
