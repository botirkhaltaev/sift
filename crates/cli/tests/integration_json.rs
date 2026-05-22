mod common;

use common::TestProject;

#[test]
fn json_emits_begin_match_end_and_summary() {
    let p = TestProject::new("json-basic");
    p.write("a.txt", "hello world\n");
    p.build_index();

    let output = p.index_output(["hello", "--json"]);
    common::assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 4,
        "expected begin, match, end, summary lines; got {}: {stdout:?}",
        lines.len()
    );
    assert!(
        lines[0].contains("\"type\":\"begin\""),
        "begin: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("\"type\":\"match\""),
        "match: {}",
        lines[1]
    );
    assert!(lines[2].contains("\"type\":\"end\""), "end: {}", lines[2]);
    assert!(
        lines[lines.len() - 1].contains("\"type\":\"summary\""),
        "summary: {}",
        lines[lines.len() - 1]
    );
}

#[test]
fn json_implies_stats_on_stderr() {
    let p = TestProject::new("json-stats-stderr");
    p.write("a.txt", "x\n");
    p.build_index();

    let output = p.index_output(["x", "--json"]);
    common::assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matches") && stderr.contains("bytes searched"),
        "expected --stats-style lines on stderr: {stderr:?}"
    );
}

#[test]
fn json_with_count_exits_error() {
    let p = TestProject::new("json-bad-count");
    p.write("a.txt", "a\n");
    p.build_index();

    let output = p.index_output(["a", "--json", "--count"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--json") && stderr.contains("count"),
        "expected conflict message: {stderr:?}"
    );
}
