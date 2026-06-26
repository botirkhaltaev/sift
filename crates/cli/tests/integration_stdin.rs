mod common;

use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use common::{TestProject, assert_exit_code, assert_stdout_eq, assert_success, normalize_stdout};

fn output_with_stdin<I, S>(mut cmd: Command, args: I, stdin: &[u8]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn sift");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait sift")
}

#[test]
fn piped_stdin_is_searched_when_no_paths_are_given() {
    let p = TestProject::new("stdin-implicit");
    let out = output_with_stdin(p.sift(), ["needle"], b"alpha\nneedle\nomega\n");

    assert_success(&out);
    assert_stdout_eq(&out, "needle\n");
}

#[test]
fn explicit_dash_searches_stdin() {
    let p = TestProject::new("stdin-explicit");
    let out = output_with_stdin(p.sift(), ["needle", "-"], b"alpha\nneedle\nomega\n");

    assert_success(&out);
    assert_stdout_eq(&out, "needle\n");
}

#[test]
fn explicit_dash_searches_stdin_with_other_paths() {
    let p = TestProject::new("stdin-explicit-with-path");
    p.write("hay.txt", "needle\n");

    let out = output_with_stdin(p.sift(), ["needle", "hay.txt", "-"], b"needle\n");

    assert_success(&out);
    assert_eq!(normalize_stdout(&out), "hay.txt:needle\n<stdin>:needle\n");
}

#[test]
fn stdin_with_filename_uses_ripgrep_stdin_name() {
    let p = TestProject::new("stdin-filename");
    let out = output_with_stdin(p.sift(), ["-H", "needle"], b"needle\n");

    assert_success(&out);
    assert_stdout_eq(&out, "<stdin>:needle\n");
}

#[test]
fn pattern_file_dash_reads_patterns_from_stdin() {
    let p = TestProject::new("stdin-pattern-file");
    p.write("hay.txt", "alpha\nneedle\nomega\n");

    let out = output_with_stdin(p.sift(), ["-f", "-", "hay.txt"], b"needle\n");

    assert_success(&out);
    assert_eq!(normalize_stdout(&out), "hay.txt:needle\n");
}

#[test]
fn missing_stdin_match_exits_one() {
    let p = TestProject::new("stdin-no-match");
    let out = output_with_stdin(p.sift(), ["needle"], b"alpha\nomega\n");

    assert_exit_code(&out, 1);
    assert_stdout_eq(&out, "");
}
