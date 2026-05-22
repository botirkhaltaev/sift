mod common;

use common::TestProject;

#[test]
fn stats_reports_matches_and_files_searched() {
    let p = TestProject::new("stats-basic");
    p.write("a.txt", "hello world\n");
    p.write("b.txt", "goodbye\n");
    p.build_index();

    let output = p.index_output(["hello", "--stats"]);
    common::assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 matches"),
        "expected match count in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("1 files contained matches"),
        "expected files-with-matches count in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("1 files searched"),
        "expected files searched in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("bytes printed"),
        "expected bytes printed line in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("bytes searched"),
        "expected bytes searched line in stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("s elapsed"),
        "expected elapsed line in stderr, got: {stderr:?}"
    );
}
