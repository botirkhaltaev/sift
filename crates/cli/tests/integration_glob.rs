mod common;

use std::ffi::OsString;

use common::{TestProject, assert_success, line_path, normalize_stdout};

#[test]
fn glob_include_only_matching_files() {
    let p = TestProject::new("glob-include");
    p.write("a.txt", "hello\n");
    p.write("b.log", "hello\n");
    p.write("c.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.txt", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    let candidates = vec![
        "a.txt".to_string(),
        "b.log".to_string(),
        "c.txt".to_string(),
    ];
    let lines: Vec<_> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();

    assert!(
        lines.iter().all(|l| std::path::Path::new(l)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))),
        "expected only .txt files: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| std::path::Path::new(l)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))),
        "should not contain .log files: {lines:?}"
    );
}

#[test]
fn glob_exclude_pattern_excludes_matched_files() {
    let p = TestProject::new("glob-exclude");
    p.write("a.txt", "hello\n");
    p.write("b.log", "hello\n");
    p.write("c.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "!*.log", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    let candidates = vec![
        "a.txt".to_string(),
        "b.log".to_string(),
        "c.txt".to_string(),
    ];
    let lines: Vec<_> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        !lines.iter().any(|l| std::path::Path::new(l)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))),
        "should not contain .log files: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| std::path::Path::new(l)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))),
        "should contain .txt files: {lines:?}"
    );
}

#[test]
fn glob_multiple_patterns_later_wins() {
    let p = TestProject::new("glob-multiple");
    p.write("a.txt", "hello\n");
    p.write("b.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.txt", "-g", "!a*.txt", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    assert!(
        stdout.contains("b.txt"),
        "b.txt should be included: {stdout}"
    );
    assert!(
        !stdout.contains("a.txt"),
        "a.txt should be excluded by later ignore pattern: {stdout}"
    );
}

#[test]
fn glob_directory_matches_subtree() {
    let p = TestProject::new("glob-dir");
    p.mkdir("foo/bar");
    p.write("foo/bar/baz.txt", "hello\n");
    p.write("foo/qux.log", "hello\n");
    p.write("other.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "foo/**", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    assert!(
        stdout.contains("foo/bar/baz.txt"),
        "foo/** should match subdirectory: {stdout}"
    );
    assert!(
        stdout.contains("foo/qux.log"),
        "foo/** should match file in foo/: {stdout}"
    );
    assert!(
        !stdout.contains("other.txt"),
        "foo/** should not match outside: {stdout}"
    );
}

#[test]
fn glob_whitelist_then_exclude() {
    let p = TestProject::new("glob-whitelist-exclude");
    p.write("a.txt", "hello\n");
    p.write("b.txt", "hello\n");
    p.write("c.log", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.txt", "-g", "!a*.txt", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    assert!(
        stdout.contains("b.txt"),
        "b.txt should be included: {stdout}"
    );
    assert!(
        !stdout.contains("a.txt"),
        "a.txt should be excluded: {stdout}"
    );
}

#[test]
fn glob_only_whitelist_none_match_excludes_all() {
    let p = TestProject::new("glob-no-match");
    p.write("a.txt", "hello\n");
    p.write("b.log", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.xyz", "hello"]);

    let stdout = normalize_stdout(&out);
    assert!(
        stdout.is_empty(),
        "no matching whitelist should exclude all: {stdout}"
    );
}

#[test]
fn glob_invalid_pattern_returns_error() {
    let p = TestProject::new("glob-invalid");
    p.write("a.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "[", "hello"]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid glob pattern"),
        "should report invalid glob: {stderr}"
    );
}

#[test]
fn glob_files_with_matches_includes_only_glob_matched() {
    let p = TestProject::new("glob-files-with");
    p.write("a.txt", "hello\n");
    p.write("b.log", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.txt", "-l", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    assert!(stdout.contains("a.txt"), "should contain a.txt: {stdout}");
    assert!(
        !stdout.contains("b.log"),
        "should not contain b.log: {stdout}"
    );
}

#[test]
fn glob_combined_with_path_scope() {
    let p = TestProject::new("glob-path-scope");
    p.mkdir("foo");
    p.mkdir("bar");
    p.write("foo/a.txt", "hello\n");
    p.write("bar/b.txt", "hello\n");
    p.write("bar/c.log", "hello\n");
    p.build_index_at(p.root());

    let foo_arg: OsString = p.root().join("foo").into();
    let out = p.index_output(vec![
        OsString::from("-g"),
        OsString::from("*.txt"),
        OsString::from("hello"),
        foo_arg,
    ]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    assert!(
        stdout.contains("foo/a.txt"),
        "should find foo/a.txt: {stdout}"
    );
    assert!(
        !stdout.contains("bar"),
        "path scope should limit search: {stdout}"
    );
}

#[test]
fn glob_case_sensitive_by_default() {
    let p = TestProject::new("glob-case-sensitive-default");
    p.write("lower.txt", "hello\n");
    p.write("upper.TXT", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["-g", "*.txt", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    let candidates = vec!["lower.txt".to_string(), "upper.TXT".to_string()];
    let lines: Vec<_> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        lines.iter().any(|l| l.ends_with("lower.txt")),
        "should match lower.txt: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.ends_with("upper.TXT")),
        "upper.TXT should not match *.txt when case-sensitive: {lines:?}"
    );
}

#[test]
fn glob_case_insensitive_flag() {
    let p = TestProject::new("glob-case-insensitive");
    p.write("lower.txt", "hello\n");
    p.write("upper.TXT", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["--glob-case-insensitive", "-g", "*.txt", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    let candidates = vec!["lower.txt".to_string(), "upper.TXT".to_string()];
    let lines: Vec<_> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        lines.iter().any(|l| l.ends_with("lower.txt")),
        "should match lower.txt: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.ends_with("upper.TXT")),
        "should match upper.TXT when case-insensitive: {lines:?}"
    );
}

#[test]
fn glob_case_insensitive_with_negation() {
    let p = TestProject::new("glob-case-insensitive-neg");
    p.write("skip.log", "hello\n");
    p.write("skip.LOG", "hello\n");
    p.write("keep.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p.index_output(["--glob-case-insensitive", "-g", "!*.log", "hello"]);
    assert_success(&out);

    let stdout = normalize_stdout(&out);
    let candidates = vec![
        "skip.log".to_string(),
        "skip.LOG".to_string(),
        "keep.txt".to_string(),
    ];
    let lines: Vec<_> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        !lines.iter().any(|l| std::path::Path::new(l)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))),
        "should not contain any .log files: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.ends_with("keep.txt")),
        "should contain keep.txt: {lines:?}"
    );
}

#[test]
fn glob_case_insensitive_precedence_last_wins() {
    let p = TestProject::new("glob-case-insensitive-precedence");
    p.write("lower.txt", "hello\n");
    p.write("upper.TXT", "hello\n");
    p.build_index_at(p.root());

    let out_on = p.index_output([
        "--glob-case-insensitive",
        "--no-glob-case-insensitive",
        "-g",
        "*.txt",
        "hello",
    ]);
    assert_success(&out_on);
    let stdout_on = normalize_stdout(&out_on);
    let candidates = vec!["lower.txt".to_string(), "upper.TXT".to_string()];
    let lines_on: Vec<_> = stdout_on
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        lines_on.iter().any(|l| l.ends_with("lower.txt")),
        "should match lower.txt: {lines_on:?}"
    );
    assert!(
        !lines_on.iter().any(|l| l.ends_with("upper.TXT")),
        "upper.TXT should not match when off: {lines_on:?}"
    );

    let out_off = p.index_output([
        "--no-glob-case-insensitive",
        "--glob-case-insensitive",
        "-g",
        "*.txt",
        "hello",
    ]);
    assert_success(&out_off);
    let stdout_off = normalize_stdout(&out_off);
    let lines_off: Vec<_> = stdout_off
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| line_path(l, &candidates))
        .collect();
    assert!(
        lines_off.iter().any(|l| l.ends_with("lower.txt")),
        "should match lower.txt: {lines_off:?}"
    );
    assert!(
        lines_off.iter().any(|l| l.ends_with("upper.TXT")),
        "upper.TXT should match when on: {lines_off:?}"
    );
}
