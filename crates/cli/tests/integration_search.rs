mod common;

use std::fs;
use std::path::Path;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout, rel_match};

#[test]
fn build_then_search_finds_line() {
    let root = fresh_dir("search-line");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "fn f() {\n  let y = 2;\n}\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg(r"let\s+y")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains(&rel_match("src/lib.rs", "")) && stdout.contains("let y = 2;"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn search_no_match_exits_1() {
    let root = fresh_dir("search-no-match");
    fs::write(root.join("a.txt"), "nope\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let status = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("ZZZ_NOT_THERE")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn fixed_string_ignore_case_finds_match() {
    let root = fresh_dir("search-fixed-ignore-case");
    fs::write(root.join("t.txt"), "hello world\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-i")
        .arg("-F")
        .arg("HELLO")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("t.txt", "")) && stdout.contains("hello world"));
}

#[test]
fn invert_match_returns_non_matching_lines() {
    let root = fresh_dir("search-invert-match");
    fs::write(root.join("t.txt"), "keep\nskip\nkeep too\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-v")
        .arg("skip")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("keep"));
    assert!(stdout.contains("keep too"));
    assert!(
        !stdout.contains("t.txt:skip"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn word_regexp_matches_whole_words_only() {
    let root = fresh_dir("search-word-regexp");
    fs::write(root.join("t.txt"), "cat\nscatter\ncatnip\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-w")
        .arg("cat")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("t.txt", "cat")));
    assert!(!stdout.contains("scatter"), "unexpected stdout: {stdout}");
    assert!(!stdout.contains("catnip"), "unexpected stdout: {stdout}");
}

#[test]
fn line_regexp_matches_whole_lines_only() {
    let root = fresh_dir("search-line-regexp");
    fs::write(root.join("t.txt"), "cat\ncat dog\ndog cat\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-x")
        .arg("cat")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("t.txt", "cat")));
    assert!(!stdout.contains("cat dog"), "unexpected stdout: {stdout}");
    assert!(!stdout.contains("dog cat"), "unexpected stdout: {stdout}");
}

#[test]
fn missing_pattern_exits_2() {
    let root = fresh_dir("search-missing-pattern");
    fs::write(root.join("t.txt"), "hello\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None).arg("--sift-dir").arg(&idx).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no pattern"));
}

#[test]
fn search_literal_index_without_subcommand() {
    let root = fresh_dir("search-literal-index");
    fs::write(root.join("t.txt"), "word index here\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(Some(&root), &idx, Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("index")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("t.txt", "")) && stdout.contains("index"));
}

#[test]
fn build_single_file_then_search_finds_match() {
    let root = fresh_dir("search-single-file");
    let file = root.join("one.txt");
    fs::write(&file, "alpha\nbeta needle\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &file);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("beta needle"),
        "single-file index search should not print filename, got: {stdout}"
    );
    assert!(!stdout.contains("one.txt"));
}

#[test]
fn build_single_file_then_search_path_scope_accepts_that_file() {
    let root = fresh_dir("search-single-file-scope");
    let file = root.join("one.txt");
    fs::write(&file, "needle here\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &file);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("needle")
        .arg(&file)
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("needle here"),
        "single-file explicit target should not print filename, got: {stdout}"
    );
    assert!(!stdout.contains("one.txt"));
}

#[test]
fn binary_files_are_skipped_by_default() {
    let root = fresh_dir("search-binary-skip");
    fs::write(root.join("text.txt"), "alpha βeta\n").unwrap();
    fs::write(
        root.join("bin.dat"),
        b"prefix\0\xce\xb2\xce\xb5\xcf\x84\xce\xb1\n",
    )
    .unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg(r"\p{Greek}+")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains(&rel_match("text.txt", "alpha βeta")),
        "unexpected stdout: {stdout}"
    );
    assert!(
        !stdout.contains("bin.dat"),
        "binary file should be skipped: {stdout}"
    );
}

#[cfg(not(windows))]
#[test]
fn symlinked_files_are_not_searched_by_default() {
    use std::os::unix::fs::symlink;

    let root = fresh_dir("search-symlink-skip");
    fs::create_dir_all(root.join("real")).unwrap();
    fs::create_dir_all(root.join("link")).unwrap();
    fs::write(root.join("real/target.txt"), "needle here\n").unwrap();
    symlink(root.join("real/target.txt"), root.join("link/target.txt")).unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, [rel_match("real/target.txt", "needle here")]);
}

#[cfg(not(windows))]
#[test]
fn follow_symlink_searches_linked_path() {
    use std::os::unix::fs::symlink;

    let root = fresh_dir("search-follow-symlink");
    fs::create_dir_all(root.join("real")).unwrap();
    fs::create_dir_all(root.join("link")).unwrap();
    fs::write(root.join("real/target.txt"), "needle here\n").unwrap();
    symlink(root.join("real/target.txt"), root.join("link/target.txt")).unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions {
        follow_symlinks: true,
    }
    .run(Some(&root), &idx, Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--follow")
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);

    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("needle here"),
        "expected match through symlink: {stdout}"
    );
    assert!(
        stdout.contains("link/target.txt"),
        "expected symlink path in output: {stdout}"
    );
}

#[test]
fn partial_index_missing_component_falls_back_to_walk() {
    let root = fresh_dir("search-partial-index-walk");
    fs::write(root.join("found.txt"), "unique_marker_partial_index\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, Path::new("."));

    let postings = idx.join(".index").join("postings.bin");
    fs::remove_file(&postings).unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("unique_marker_partial_index")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("unique_marker_partial_index"),
        "expected walk fallback to find line: {stdout}"
    );
}

#[test]
fn invalid_meta_falls_back_to_walk() {
    let root = fresh_dir("search-invalid-meta-walk");
    fs::write(root.join("found.txt"), "unique_marker_bad_meta\n").unwrap();
    let idx = root.join(".sift");
    fs::create_dir_all(&idx).unwrap();
    fs::write(idx.join("sift.meta"), "not valid json\n").unwrap();

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("unique_marker_bad_meta")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("unique_marker_bad_meta"),
        "expected walk fallback to find line: {stdout}"
    );
}
