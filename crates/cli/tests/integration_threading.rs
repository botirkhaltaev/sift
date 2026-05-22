mod common;

use std::fs;

use common::{assert_success, command, fresh_dir, normalized_stdout};

// ─── --threads / -j ────────────────────────────────────────────────────────────

#[test]
fn threads_flag_runs_search_walk() {
    let root = fresh_dir("threads-walk");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-j", "1", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello world"),
        "expected match with -j 1, got: {stdout}"
    );
}

#[test]
fn threads_flag_accepts_larger_value_walk() {
    let root = fresh_dir("threads-large-walk");
    fs::write(root.join("a.txt"), "line\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--threads", "4", "line"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("line"),
        "expected match with --threads 4, got: {stdout}"
    );
}

// ─── --line-buffered / --block-buffered ────────────────────────────────────────

#[test]
fn line_buffered_accepted_walk() {
    let root = fresh_dir("line-buffered-walk");
    fs::write(root.join("a.txt"), "hello\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--line-buffered", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello"),
        "expected output with --line-buffered, got: {stdout}"
    );
}

#[test]
fn block_buffered_accepted_walk() {
    let root = fresh_dir("block-buffered-walk");
    fs::write(root.join("a.txt"), "hello\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--block-buffered", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello"),
        "expected output with --block-buffered, got: {stdout}"
    );
}

// ─── --path-separator ──────────────────────────────────────────────────────────

#[test]
fn path_separator_replaces_separator_walk() {
    let root = fresh_dir("path-sep-walk");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("a.txt"), "match\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--path-separator", ":", "match"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("sub:a.txt"),
        "expected path with ':' separator, got: {stdout}"
    );
}

// ─── --one-file-system ─────────────────────────────────────────────────────────

#[test]
fn one_file_system_accepted_walk() {
    let root = fresh_dir("one-fs-walk");
    fs::write(root.join("a.txt"), "data\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--one-file-system", "data"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("data"),
        "expected match with --one-file-system, got: {stdout}"
    );
}

// ─── --mmap / --no-mmap (advisory) ────────────────────────────────────────────

#[test]
fn mmap_flag_accepted_walk() {
    let root = fresh_dir("mmap-walk");
    fs::write(root.join("a.txt"), "hello\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--mmap", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello"),
        "expected output with --mmap, got: {stdout}"
    );
}

#[test]
fn no_mmap_flag_accepted_walk() {
    let root = fresh_dir("no-mmap-walk");
    fs::write(root.join("a.txt"), "hello\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--no-mmap", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello"),
        "expected output with --no-mmap, got: {stdout}"
    );
}

// ─── -U/--multiline ────────────────────────────────────────────────────────────

#[test]
fn multiline_matches_across_lines_walk() {
    let root = fresh_dir("multiline-walk");
    fs::write(root.join("a.txt"), "foo\nbar\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-U", r"foo\nbar"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("foo") && stdout.contains("bar"),
        "expected multiline match, got: {stdout}"
    );
}

#[test]
fn multiline_long_flag_walk() {
    let root = fresh_dir("multiline-long-walk");
    fs::write(root.join("a.txt"), "alpha\nbeta\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--multiline", r"alpha\nbeta"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("alpha") && stdout.contains("beta"),
        "expected multiline match, got: {stdout}"
    );
}

// ─── --multiline-dotall ────────────────────────────────────────────────────────

#[test]
fn multiline_dotall_dot_matches_newline_walk() {
    let root = fresh_dir("dotall-walk");
    fs::write(root.join("a.txt"), "start\nend\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-U", "--multiline-dotall", "start.end"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("start") && stdout.contains("end"),
        "expected dotall match, got: {stdout}"
    );
}

// ─── --crlf ────────────────────────────────────────────────────────────────────

#[test]
fn crlf_flag_matches_with_carriage_return_walk() {
    let root = fresh_dir("crlf-walk");
    fs::write(root.join("a.txt"), "hello world\r\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--crlf", "hello"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("hello"),
        "expected match with --crlf, got: {stdout}"
    );
}

#[test]
fn crlf_word_boundary_respects_cr_walk() {
    let root = fresh_dir("crlf-word-walk");
    // With CRLF, `world$` should match `world\r\n`
    fs::write(root.join("a.txt"), "hello world\r\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["--crlf", "world$"])
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("world"),
        "expected 'world' with --crlf and $, got: {stdout}"
    );
}

// ─── Index + walk consistency ──────────────────────────────────────────────────

#[test]
fn multiline_consistent_index_and_walk() {
    let root = fresh_dir("multiline-consistency");
    fs::write(root.join("a.txt"), "foo\nbar\n").unwrap();

    let idx_dir = root.join(".sift");
    common::BuildIndexOptions::default().run(Some(&root), &idx_dir, std::path::Path::new("."));

    let index_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx_dir)
        .args(["-U", r"foo\nbar"])
        .output()
        .unwrap();
    let walk_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-U", r"foo\nbar"])
        .output()
        .unwrap();
    assert_success(&index_out);
    assert_success(&walk_out);
    let index_stdout = normalized_stdout(&index_out);
    let walk_stdout = normalized_stdout(&walk_out);
    assert_eq!(
        index_stdout, walk_stdout,
        "index and walk multiline results differ"
    );
}

#[test]
fn threads_consistent_index_and_walk() {
    let root = fresh_dir("threads-consistency");
    fs::write(root.join("a.txt"), "hello\n").unwrap();

    let idx_dir = root.join(".sift");
    common::BuildIndexOptions::default().run(Some(&root), &idx_dir, std::path::Path::new("."));

    let index_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx_dir)
        .args(["-j", "1", "hello"])
        .output()
        .unwrap();
    let walk_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(root.join("missing-index"))
        .args(["-j", "1", "hello"])
        .output()
        .unwrap();
    assert_success(&index_out);
    assert_success(&walk_out);
    let index_stdout = normalized_stdout(&index_out);
    let walk_stdout = normalized_stdout(&walk_out);
    assert_eq!(
        index_stdout, walk_stdout,
        "index and walk results differ with -j 1"
    );
}
