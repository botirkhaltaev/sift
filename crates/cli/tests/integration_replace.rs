mod common;

use std::fs;

use common::{
    BuildIndexOptions, assert_index_and_walk_output, assert_success, command, fresh_dir,
    normalized_stdout, rel_match,
};

// ─── --replace / -r ──────────────────────────────────────────────────────────

#[test]
fn replace_literal_walk() {
    let root = fresh_dir("replace-literal-walk");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-r", "planet", "world"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello planet"),
        "expected replacement output, got: {stdout}"
    );
}

#[test]
fn replace_literal_consistent_index_and_walk() {
    let root = fresh_dir("replace-literal-both");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();

    assert_index_and_walk_output(
        &root,
        &["-r".into(), "planet".into(), "world".into()],
        &format!("{}\n", rel_match("a.txt", "hello planet")),
    );
}

#[test]
fn replace_with_capture_groups_walk() {
    let root = fresh_dir("replace-capture-walk");
    fs::write(root.join("a.txt"), "foo123bar\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-r", "${1}_${2}", "(foo)(\\d+)", "a.txt"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("foo_123bar"),
        "expected capture-group replacement, got: {stdout}"
    );
}

#[test]
fn replace_with_capture_groups_index() {
    let root = fresh_dir("replace-capture-index");
    fs::write(root.join("a.txt"), "foo123bar\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .args(["-r", "${1}_${2}", "(foo)(\\d+)"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("foo_123bar"),
        "expected capture-group replacement, got: {stdout}"
    );
}

// ─── --trim ──────────────────────────────────────────────────────────────────

#[test]
fn trim_removes_leading_whitespace_walk() {
    let root = fresh_dir("trim-walk");
    fs::write(root.join("a.txt"), "    indented line\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--trim", "indented"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("indented line"),
        "expected trimmed output, got: {stdout}"
    );
    assert!(
        !stdout.contains("    indented"),
        "leading whitespace should be removed, got: {stdout}"
    );
}

#[test]
fn trim_consistent_index_and_walk() {
    let root = fresh_dir("trim-both");
    fs::write(root.join("a.txt"), "    hello world\n").unwrap();

    assert_index_and_walk_output(
        &root,
        &["--trim".into(), "hello".into()],
        &format!("{}\n", rel_match("a.txt", "hello world")),
    );
}

// ─── --byte-offset / -b ─────────────────────────────────────────────────────

#[test]
fn byte_offset_shows_position_walk() {
    let root = fresh_dir("byte-offset-walk");
    // "first line\n" = 11 bytes, then "hello world\n"
    fs::write(root.join("a.txt"), "first line\nhello world\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-b", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    // byte offset of "hello world" is 11
    assert!(
        stdout.contains("11:"),
        "expected byte offset 11, got: {stdout}"
    );
}

#[test]
fn byte_offset_consistent_index_and_walk() {
    let root = fresh_dir("byte-offset-both");
    fs::write(root.join("a.txt"), "abc\nhello\n").unwrap();

    assert_index_and_walk_output(
        &root,
        &["-b".into(), "hello".into()],
        &format!("{}\n", rel_match("a.txt", "4:hello")),
    );
}

// ─── --passthru ─────────────────────────────────────────────────────────────

#[test]
fn passthru_shows_all_lines_walk() {
    let root = fresh_dir("passthru-walk");
    fs::write(root.join("a.txt"), "first\nhello\nthird\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--passthru", "hello", "a.txt"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("first"),
        "passthru should show non-matching lines, got: {stdout}"
    );
    assert!(
        stdout.contains("hello"),
        "passthru should show matching lines, got: {stdout}"
    );
    assert!(
        stdout.contains("third"),
        "passthru should show non-matching lines, got: {stdout}"
    );
}

#[test]
fn passthru_alias_passthrough_walk() {
    let root = fresh_dir("passthrough-alias-walk");
    fs::write(root.join("a.txt"), "first\nhello\nthird\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--passthrough", "hello", "a.txt"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("first") && stdout.contains("third"),
        "passthrough alias should show all lines, got: {stdout}"
    );
}

// ─── --include-zero ─────────────────────────────────────────────────────────

#[test]
fn include_zero_in_count_mode_walk() {
    let root = fresh_dir("include-zero-walk");
    fs::write(root.join("a.txt"), "match\n").unwrap();
    fs::write(root.join("b.txt"), "nothing here\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--count", "--include-zero", "match"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("a.txt:1"),
        "should show a.txt with count 1, got: {stdout}"
    );
    assert!(
        stdout.contains("b.txt:0"),
        "should show b.txt with count 0 when --include-zero, got: {stdout}"
    );
}

#[test]
fn include_zero_in_count_mode_index() {
    let root = fresh_dir("include-zero-index");
    fs::write(root.join("a.txt"), "match\n").unwrap();
    fs::write(root.join("b.txt"), "nothing here\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .args(["--count", "--include-zero", "match"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("a.txt:1"),
        "should show a.txt with count 1, got: {stdout}"
    );
    assert!(
        stdout.contains("b.txt:0"),
        "should show b.txt with count 0 when --include-zero, got: {stdout}"
    );
}

#[test]
fn count_without_include_zero_omits_zero_files_walk() {
    let root = fresh_dir("no-include-zero-walk");
    fs::write(root.join("a.txt"), "match\n").unwrap();
    fs::write(root.join("b.txt"), "nothing here\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--count", "match"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("a.txt:1"),
        "should show a.txt with count 1, got: {stdout}"
    );
    assert!(
        !stdout.contains("b.txt"),
        "should NOT show b.txt without --include-zero, got: {stdout}"
    );
}
