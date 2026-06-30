mod common;

use common::{
    TestProject, assert_stderr_empty, assert_stdout_eq, assert_success, normalize_stdout,
};

// ─── --no-config ──────────────────────────────────────────────────────────────

#[test]
fn no_config_flag_accepted_walk() {
    let p = TestProject::new("engine-no-config");
    p.write("a.txt", "hello world\n");
    let out = p.walk_output(["--no-config", "hello"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("hello world"));
}

// ─── --unicode / --no-unicode ────────────────────────────────────────────────

#[test]
fn unicode_flag_accepted_walk() {
    let p = TestProject::new("engine-unicode");
    p.write("a.txt", "café\n");
    let out = p.walk_output(["--unicode", "café"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("café"));
}

#[test]
fn no_unicode_flag_accepted_walk() {
    let p = TestProject::new("engine-no-unicode");
    p.write("a.txt", "hello\n");
    let out = p.walk_output(["--no-unicode", "hello"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("hello"));
}

#[test]
fn unicode_consistent_index_and_walk() {
    let p = TestProject::new("engine-unicode-idx");
    p.write("a.txt", "café\n");
    p.build_index();
    let args = ["--unicode", "café"];

    let index = p.index_output(args);
    assert_success(&index);
    assert_stdout_eq(&index, "a.txt:café\n");
    assert_stderr_empty(&index);

    let walk = p.walk_output(args);
    assert_success(&walk);
    assert_stdout_eq(&walk, "a.txt:café\n");
    assert_stderr_empty(&walk);
}

// ─── --colors ────────────────────────────────────────────────────────────────

#[test]
fn colors_flag_accepted_walk() {
    let p = TestProject::new("engine-colors");
    p.write("a.txt", "hello\n");
    let out = p.walk_output(["--colors", "match:fg:red", "hello"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("hello"));
}

// ─── --regex-size-limit ──────────────────────────────────────────────────────

#[test]
fn regex_size_limit_accepted_walk() {
    let p = TestProject::new("engine-regex-limit");
    p.write("a.txt", "hello\n");
    let out = p.walk_output(["--regex-size-limit", "10M", "hello"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("hello"));
}

#[test]
fn regex_size_limit_suffix_parsing_walk() {
    let p = TestProject::new("engine-regex-limit-suffix");
    p.write("a.txt", "hello\n");
    for suffix in ["1K", "1M", "1G"] {
        let out = p.walk_output(["--regex-size-limit", suffix, "hello"]);
        assert_success(&out);
        assert!(
            normalize_stdout(&out).contains("hello"),
            "failed with suffix {suffix}"
        );
    }
}

// ─── --dfa-size-limit ────────────────────────────────────────────────────────

#[test]
fn dfa_size_limit_accepted_walk() {
    let p = TestProject::new("engine-dfa-limit");
    p.write("a.txt", "hello\n");
    let out = p.walk_output(["--dfa-size-limit", "10M", "hello"]);
    assert_success(&out);
    assert!(normalize_stdout(&out).contains("hello"));
}

// ─── -M / --max-columns ─────────────────────────────────────────────────────

#[test]
fn max_columns_omits_long_lines_walk() {
    let p = TestProject::new("engine-maxcol-omit");
    p.write(
        "a.txt",
        "short\nthis line is very long and exceeds the limit\n",
    );
    let out = p.walk_output(["-M", "10", "."]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    assert!(stdout.contains("short"), "short line should appear");
    assert!(
        !stdout.contains("this line is very long"),
        "long line should be omitted"
    );
}

#[test]
fn max_columns_preview_truncates_long_lines_walk() {
    let p = TestProject::new("engine-maxcol-preview");
    p.write(
        "a.txt",
        "short\nthis line is very long and exceeds the limit\n",
    );
    let out = p.walk_output(["-M", "10", "--max-columns-preview", "."]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    assert!(stdout.contains("short"), "short line should appear");
    assert!(
        stdout.contains("[... omitted end ...]"),
        "truncated preview should appear"
    );
}

#[test]
fn max_columns_consistent_index_and_walk() {
    let p = TestProject::new("engine-maxcol-idx");
    p.write(
        "a.txt",
        "short\nthis line is very long and exceeds the limit\n",
    );
    p.build_index();
    let args = ["-M", "10", "--max-columns-preview", "."];
    let expected = "a.txt:short\na.txt:this line  [... omitted end ...]\n";

    let index = p.index_output(args);
    assert_success(&index);
    assert_stdout_eq(&index, expected);
    assert_stderr_empty(&index);

    let walk = p.walk_output(args);
    assert_success(&walk);
    assert_stdout_eq(&walk, expected);
    assert_stderr_empty(&walk);
}

#[test]
fn max_columns_without_preview_omits_consistently() {
    let p = TestProject::new("engine-maxcol-nopreview-idx");
    p.write(
        "a.txt",
        "short\nthis line is very long and exceeds the limit\n",
    );
    p.build_index();
    let args = ["-M", "10", "."];

    let index = p.index_output(args);
    assert_success(&index);
    assert_stdout_eq(&index, "a.txt:short\n");
    assert_stderr_empty(&index);

    let walk = p.walk_output(args);
    assert_success(&walk);
    assert_stdout_eq(&walk, "a.txt:short\n");
    assert_stderr_empty(&walk);
}

// ─── --regex-size-limit / --dfa-size-limit consistent index & walk ──────────

#[test]
fn regex_size_limit_consistent_index_and_walk() {
    let p = TestProject::new("engine-regex-limit-idx");
    p.write("a.txt", "hello world\n");
    p.build_index();
    let args = ["--regex-size-limit", "10M", "hello"];

    let index = p.index_output(args);
    assert_success(&index);
    assert_stdout_eq(&index, "a.txt:hello world\n");
    assert_stderr_empty(&index);

    let walk = p.walk_output(args);
    assert_success(&walk);
    assert_stdout_eq(&walk, "a.txt:hello world\n");
    assert_stderr_empty(&walk);
}

#[test]
fn dfa_size_limit_consistent_index_and_walk() {
    let p = TestProject::new("engine-dfa-limit-idx");
    p.write("a.txt", "hello world\n");
    p.build_index();
    let args = ["--dfa-size-limit", "10M", "hello"];

    let index = p.index_output(args);
    assert_success(&index);
    assert_stdout_eq(&index, "a.txt:hello world\n");
    assert_stderr_empty(&index);

    let walk = p.walk_output(args);
    assert_success(&walk);
    assert_stdout_eq(&walk, "a.txt:hello world\n");
    assert_stderr_empty(&walk);
}
