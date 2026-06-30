mod common;

use common::TestProject;

#[test]
fn null_terminates_paths_with_files_with_matches() {
    let p = TestProject::new("integration-null-l");
    p.write("a.txt", "needle\n");
    p.write("b.txt", "other\n");
    p.build_index();

    let output = p.index_output(["-l", "--null", "needle"]);
    common::assert_success(&output);

    assert!(
        output.stdout.contains(&b'\0'),
        "expected NUL between path records, got {:?}",
        output.stdout
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains('\n'),
        "with --null, path list should not use newlines"
    );
}

#[test]
fn null_data_treats_nul_as_line_terminator() {
    let p = TestProject::new("integration-null-data");
    p.write("a.txt", b"alpha\0needle\0omega\0");
    p.build_index();

    let output = p.index_output(["--null-data", "needle"]);
    common::assert_success(&output);

    assert!(
        output.stdout.ends_with(b"\0"),
        "expected NUL-terminated match record, got {:?}",
        output.stdout
    );
    assert!(
        !output.stdout.contains(&b'\n'),
        "--null-data output should use NUL record terminators"
    );
}

#[test]
fn null_data_terminates_count_records_with_nul() {
    let p = TestProject::new("integration-null-data-count");
    p.write("a.txt", b"alpha\0needle\0omega\0");
    p.build_index();

    let output = p.index_output(["--null-data", "--count", "needle"]);
    common::assert_success(&output);

    assert_eq!(output.stdout, b"a.txt:1\0");
}

#[test]
fn null_data_terminates_files_with_matches_with_nul() {
    let p = TestProject::new("integration-null-data-files-with-matches");
    p.write("a.txt", b"alpha\0needle\0");
    p.write("b.txt", b"alpha\0omega\0");
    p.build_index();

    let output = p.index_output(["--null-data", "--files-with-matches", "needle"]);
    common::assert_success(&output);

    assert_eq!(output.stdout, b"a.txt\0");
}

#[test]
fn null_data_terminates_files_without_match_with_nul() {
    let p = TestProject::new("integration-null-data-files-without-match");
    p.write("a.txt", b"alpha\0needle\0");
    p.write("b.txt", b"alpha\0omega\0");
    p.build_index();

    let output = p.index_output(["--null-data", "--files-without-match", "needle"]);
    common::assert_success(&output);

    assert_eq!(output.stdout, b"b.txt\0");
}

#[test]
fn null_data_terminates_context_separator_with_nul() {
    let p = TestProject::new("integration-null-data-context-separator");
    p.write("a.txt", b"before\0needle\0gap\0gap\0gap\0needle\0after\0");
    p.build_index();

    let output = p.index_output(["--null-data", "--context", "1", "needle"]);
    common::assert_success(&output);

    assert!(
        output.stdout.windows(3).any(|window| window == b"--\0"),
        "expected NUL-terminated context separator, got {:?}",
        output.stdout
    );
    assert!(
        !output.stdout.contains(&b'\n'),
        "--null-data context output should not contain newline terminators"
    );
}

#[test]
fn null_data_is_separate_from_null_path_terminator() {
    let p = TestProject::new("integration-null-data-separate");
    p.write("a.txt", b"alpha\0needle\0");
    p.build_index();

    let output = p.index_output(["--null", "needle"]);
    common::assert_exit_code(&output, 1);

    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("needle"),
        "--null alone should not switch input records to NUL data"
    );
}

#[test]
fn color_always_emits_ansi_on_stdout() {
    let p = TestProject::new("integration-color");
    p.write("t.txt", "needle\n");
    p.build_index();

    let output = p.index_output(["--color=always", "needle", "t.txt"]);
    common::assert_success(&output);

    let s = common::normalize_stdout(&output);
    assert!(
        s.contains('\x1b'),
        "expected ANSI escapes with --color=always, got {s:?}"
    );
}
