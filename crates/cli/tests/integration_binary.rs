mod common;

use std::fs;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout};

fn make_binary_file(root: &std::path::Path, name: &str, content: &[u8]) {
    fs::write(root.join(name), content).unwrap();
}

// ─── default (quit on NUL) ───────────────────────────────────────────────────

#[test]
fn default_skips_binary_file_walk() {
    let root = fresh_dir("binary-default-walk");
    // A file with NUL byte before the match
    make_binary_file(&root, "binary.txt", b"abc\x00match_here\n");
    fs::write(root.join("text.txt"), "match_here\n").unwrap();
    let missing_idx = fresh_dir("binary-default-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("match_here")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("text.txt"),
        "should find match in text file"
    );
    assert!(
        !stdout.contains("binary.txt:") || !stdout.contains("match_here"),
        "should not show match from binary file (NUL before match)"
    );
}

#[test]
fn default_skips_binary_file_index() {
    let root = fresh_dir("binary-default-index");
    make_binary_file(&root, "binary.txt", b"abc\x00match_here\n");
    fs::write(root.join("text.txt"), "match_here\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("match_here")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("text.txt"),
        "should find match in text file"
    );
}

// ─── -a / --text ─────────────────────────────────────────────────────────────

#[test]
fn text_flag_searches_binary_walk() {
    let root = fresh_dir("text-flag-walk");
    // Put match before NUL so it's detectable
    make_binary_file(&root, "binary.txt", b"findme\n\x00other\n");
    let missing_idx = fresh_dir("text-flag-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("-a")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("binary.txt") && stdout.contains("findme"),
        "with -a, should find match in binary file"
    );
}

#[test]
fn text_flag_searches_binary_index() {
    let root = fresh_dir("text-flag-index");
    make_binary_file(&root, "binary.txt", b"findme\n\x00other\n");
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--text")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("binary.txt") && stdout.contains("findme"),
        "with --text, should find match in binary file"
    );
}

#[test]
fn text_flag_finds_match_after_nul_walk() {
    let root = fresh_dir("text-after-nul-walk");
    // NUL before the match — without -a, search would quit before finding it
    make_binary_file(&root, "binary.txt", b"\x00findme\n");
    let missing_idx = fresh_dir("text-after-nul-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("-a")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("findme"),
        "with -a, should find match after NUL byte"
    );
}

// ─── --binary ────────────────────────────────────────────────────────────────

#[test]
fn binary_flag_continues_after_nul_walk() {
    let root = fresh_dir("binary-flag-walk");
    // Match before NUL, NUL in the middle
    make_binary_file(&root, "mixed.txt", b"findme\nmore\x00stuff\n");
    let missing_idx = fresh_dir("binary-flag-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--binary")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("findme"),
        "with --binary, should find match in file with NUL bytes"
    );
}

#[test]
fn binary_flag_continues_after_nul_index() {
    let root = fresh_dir("binary-flag-index");
    make_binary_file(&root, "mixed.txt", b"findme\nmore\x00stuff\n");
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--binary")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("findme"),
        "with --binary, should find match in file with NUL bytes"
    );
}

// ─── text overrides binary ───────────────────────────────────────────────────

#[test]
fn text_overrides_binary() {
    let root = fresh_dir("text-overrides-binary");
    make_binary_file(&root, "binary.txt", b"\x00findme\n");
    let missing_idx = fresh_dir("text-overrides-binary-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--binary")
        .arg("-a")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("findme"),
        "-a should override --binary and find match after NUL"
    );
}

// ─── consistency: index vs walk ──────────────────────────────────────────────

#[test]
fn text_mode_consistent_index_and_walk() {
    let root = fresh_dir("text-consistent");
    make_binary_file(&root, "binary.txt", b"findme\n\x00other\n");
    fs::write(root.join("text.txt"), "findme\n").unwrap();

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));
    let missing_idx = fresh_dir("text-consistent-noidx").join(".sift");

    let index_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-a")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&index_out);

    let walk_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("-a")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&walk_out);

    let mut index_lines: Vec<String> = normalized_stdout(&index_out)
        .lines()
        .map(str::to_string)
        .collect();
    let mut walk_lines: Vec<String> = normalized_stdout(&walk_out)
        .lines()
        .map(str::to_string)
        .collect();
    index_lines.sort();
    walk_lines.sort();
    assert_eq!(
        index_lines, walk_lines,
        "index and walk should produce same results with -a"
    );
}
