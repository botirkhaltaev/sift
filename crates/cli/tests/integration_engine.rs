mod common;

use std::ffi::OsString;
use std::fs;

use common::{assert_index_and_walk_output, assert_success, command, fresh_dir, normalized_stdout};

// ─── --no-config ──────────────────────────────────────────────────────────────

#[test]
fn no_config_flag_accepted_walk() {
    let root = fresh_dir("engine-no-config");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--no-config")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("hello world"));
}

// ─── --unicode / --no-unicode ────────────────────────────────────────────────

#[test]
fn unicode_flag_accepted_walk() {
    let root = fresh_dir("engine-unicode");
    fs::write(root.join("a.txt"), "café\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--unicode")
        .arg("café")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("café"));
}

#[test]
fn no_unicode_flag_accepted_walk() {
    let root = fresh_dir("engine-no-unicode");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--no-unicode")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("hello"));
}

#[test]
fn unicode_consistent_index_and_walk() {
    let root = fresh_dir("engine-unicode-idx");
    fs::write(root.join("a.txt"), "café\n").unwrap();
    assert_index_and_walk_output(
        &root,
        &[OsString::from("--unicode"), OsString::from("café")],
        "a.txt:café\n",
    );
}

// ─── --colors ────────────────────────────────────────────────────────────────

#[test]
fn colors_flag_accepted_walk() {
    let root = fresh_dir("engine-colors");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--colors")
        .arg("match:fg:red")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("hello"));
}

// ─── --regex-size-limit ──────────────────────────────────────────────────────

#[test]
fn regex_size_limit_accepted_walk() {
    let root = fresh_dir("engine-regex-limit");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--regex-size-limit")
        .arg("10M")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("hello"));
}

#[test]
fn regex_size_limit_suffix_parsing_walk() {
    let root = fresh_dir("engine-regex-limit-suffix");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    for suffix in ["1K", "1M", "1G"] {
        let out = command(Some(&root))
            .arg("--sift-dir")
            .arg(&sift_dir)
            .arg("--regex-size-limit")
            .arg(suffix)
            .arg("hello")
            .output()
            .unwrap();
        assert_success(&out);
        assert!(
            normalized_stdout(&out).contains("hello"),
            "failed with suffix {suffix}"
        );
    }
}

// ─── --dfa-size-limit ────────────────────────────────────────────────────────

#[test]
fn dfa_size_limit_accepted_walk() {
    let root = fresh_dir("engine-dfa-limit");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("--dfa-size-limit")
        .arg("10M")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(normalized_stdout(&out).contains("hello"));
}

// ─── -M / --max-columns ─────────────────────────────────────────────────────

#[test]
fn max_columns_omits_long_lines_walk() {
    let root = fresh_dir("engine-maxcol-omit");
    fs::write(
        root.join("a.txt"),
        "short\nthis line is very long and exceeds the limit\n",
    )
    .unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("-M")
        .arg("10")
        .arg(".")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("short"), "short line should appear");
    assert!(
        !stdout.contains("this line is very long"),
        "long line should be omitted"
    );
}

#[test]
fn max_columns_preview_truncates_long_lines_walk() {
    let root = fresh_dir("engine-maxcol-preview");
    fs::write(
        root.join("a.txt"),
        "short\nthis line is very long and exceeds the limit\n",
    )
    .unwrap();
    let sift_dir = root.join(".sift-missing");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&sift_dir)
        .arg("-M")
        .arg("10")
        .arg("--max-columns-preview")
        .arg(".")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("short"), "short line should appear");
    assert!(
        stdout.contains("[... omitted end ...]"),
        "truncated preview should appear"
    );
}

#[test]
fn max_columns_consistent_index_and_walk() {
    let root = fresh_dir("engine-maxcol-idx");
    fs::write(
        root.join("a.txt"),
        "short\nthis line is very long and exceeds the limit\n",
    )
    .unwrap();
    assert_index_and_walk_output(
        &root,
        &[
            OsString::from("-M"),
            OsString::from("10"),
            OsString::from("--max-columns-preview"),
            OsString::from("."),
        ],
        "a.txt:short\na.txt:this line  [... omitted end ...]\n",
    );
}

#[test]
fn max_columns_without_preview_omits_consistently() {
    let root = fresh_dir("engine-maxcol-nopreview-idx");
    fs::write(
        root.join("a.txt"),
        "short\nthis line is very long and exceeds the limit\n",
    )
    .unwrap();
    assert_index_and_walk_output(
        &root,
        &[
            OsString::from("-M"),
            OsString::from("10"),
            OsString::from("."),
        ],
        "a.txt:short\n",
    );
}

// ─── --regex-size-limit / --dfa-size-limit consistent index & walk ──────────

#[test]
fn regex_size_limit_consistent_index_and_walk() {
    let root = fresh_dir("engine-regex-limit-idx");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();
    assert_index_and_walk_output(
        &root,
        &[
            OsString::from("--regex-size-limit"),
            OsString::from("10M"),
            OsString::from("hello"),
        ],
        "a.txt:hello world\n",
    );
}

#[test]
fn dfa_size_limit_consistent_index_and_walk() {
    let root = fresh_dir("engine-dfa-limit-idx");
    fs::write(root.join("a.txt"), "hello world\n").unwrap();
    assert_index_and_walk_output(
        &root,
        &[
            OsString::from("--dfa-size-limit"),
            OsString::from("10M"),
            OsString::from("hello"),
        ],
        "a.txt:hello world\n",
    );
}
