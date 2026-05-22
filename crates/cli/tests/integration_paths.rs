mod common;

use common::TestProject;

#[test]
fn relative_path_scope_limits_matches() {
    let p = TestProject::new("paths-relative-scope");
    p.mkdir("a");
    p.mkdir("b");
    p.write("a/x.txt", "ONLY_IN_A\n");
    p.write("b/y.txt", "ONLY_IN_B\n");
    p.build_index();

    let out = p.index_output(["ONLY_IN_", "a"]);
    common::assert_success(&out);
    let stdout = common::normalize_stdout(&out);
    assert!(stdout.contains("a/x.txt") && stdout.contains("ONLY_IN_A"));
    assert!(!stdout.contains("b/y.txt"), "unexpected stdout: {stdout}");

    let out_both = p.index_output(["ONLY_IN_", "a", "b"]);
    common::assert_success(&out_both);
    let stdout_both = common::normalize_stdout(&out_both);
    assert!(stdout_both.contains("a/x.txt") && stdout_both.contains("b/y.txt"));
}

#[test]
fn absolute_path_scope_within_corpus_works() {
    let p = TestProject::new("paths-absolute-scope");
    p.mkdir("a");
    p.mkdir("b");
    p.write("a/x.txt", "alpha\n");
    p.write("b/y.txt", "alpha\n");
    p.build_index_at(p.root());

    let out = p
        .sift()
        .arg("--sift-dir")
        .arg(p.root().join(".sift"))
        .arg("alpha")
        .arg(p.root().join("a"))
        .output()
        .unwrap();
    common::assert_success(&out);
    let stdout = common::normalize_stdout(&out);
    assert!(stdout.contains("a/x.txt"));
    assert!(!stdout.contains("b/y.txt"), "unexpected stdout: {stdout}");
}

#[test]
fn search_path_outside_corpus_exits_2() {
    let p = TestProject::new("paths-outside-corpus");
    p.write("a.txt", "hello\n");
    let outside = TestProject::new("paths-outside-corpus-elsewhere");
    outside.write("b.txt", "hello\n");
    p.build_index_at(p.root());

    let out = p
        .sift()
        .arg("--sift-dir")
        .arg(p.root().join(".sift"))
        .arg("hello")
        .arg(outside.root())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("is not under indexed corpus root"));
}
