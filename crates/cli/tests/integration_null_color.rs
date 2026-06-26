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

#[test]
fn color_ansi_emits_ansi_on_captured_stdout() {
    let p = TestProject::new("integration-color-ansi");
    p.write("t.txt", "needle\n");

    let output = p.walk_output(["--color=ansi", "needle", "t.txt"]);
    common::assert_success(&output);

    let s = common::normalize_stdout(&output);
    assert!(
        s.contains('\x1b'),
        "expected ANSI escapes with --color=ansi, got {s:?}"
    );
}

#[test]
fn colors_customize_match_style() {
    let p = TestProject::new("integration-colors-match");
    p.write("t.txt", "needle\n");

    let output = p.walk_output([
        "--color=always",
        "--colors",
        "match:fg:blue",
        "--colors",
        "match:style:nobold",
        "needle",
        "t.txt",
    ]);
    common::assert_success(&output);

    let s = common::normalize_stdout(&output);
    assert!(
        s.contains("\x1b[0m\x1b[22;34mneedle\x1b[0m"),
        "expected custom blue non-bold match style, got {s:?}"
    );
}

#[test]
fn colors_customize_path_style() {
    let p = TestProject::new("integration-colors-path");
    p.write("t.txt", "needle\n");

    let output = p.walk_output([
        "--color=always",
        "--colors",
        "path:fg:blue",
        "--with-filename",
        "needle",
        "t.txt",
    ]);
    common::assert_success(&output);

    let s = common::normalize_stdout(&output);
    assert!(
        s.contains("\x1b[0m\x1b[34mt.txt\x1b[0m:"),
        "expected custom blue path style, got {s:?}"
    );
}

#[test]
fn invalid_colors_spec_exits_with_error() {
    let p = TestProject::new("integration-colors-invalid");
    p.write("t.txt", "needle\n");

    let output = p.walk_output(["--colors", "bogus:fg:red", "needle", "t.txt"]);

    common::assert_exit_code(&output, 2);
    let stderr = common::normalize_stderr(&output);
    assert!(
        stderr.contains("unrecognized output type 'bogus'"),
        "expected invalid color spec error, got {stderr:?}"
    );
}

#[test]
fn hyperlink_format_vscode_wraps_path() {
    let p = TestProject::new("integration-hyperlink-vscode");
    p.write("t.txt", "needle\n");

    let output = p.walk_output([
        "--hyperlink-format=vscode",
        "--with-filename",
        "--line-number",
        "--column",
        "needle",
        "t.txt",
    ]);
    common::assert_success(&output);

    let s = String::from_utf8_lossy(&output.stdout);
    assert!(
        s.contains("\x1b]8;;vscode://file") && s.contains("t.txt\x1b]8;;\x1b\\:1:1:"),
        "expected OSC-8 vscode hyperlink around path, got {s:?}"
    );
}
