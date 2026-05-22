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
