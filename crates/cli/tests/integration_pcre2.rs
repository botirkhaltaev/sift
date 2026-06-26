mod common;

use common::{
    TestProject, assert_exit_code, assert_stderr_empty, assert_stdout_contains, assert_stdout_eq,
    assert_success, normalize_stderr, normalize_stdout,
};

#[test]
fn pcre2_enables_look_around() {
    let p = TestProject::new("pcre2-look-around");
    p.write("hay.txt", "foo\nbar\nbaz\n");

    let out = p.walk_output(["--pcre2", "(?<=ba)r"]);

    assert_success(&out);
    assert_stdout_eq(&out, "hay.txt:bar\n");
    assert_stderr_empty(&out);
}

#[test]
fn default_engine_rejects_look_around() {
    let p = TestProject::new("pcre2-default-rejects-look-around");
    p.write("hay.txt", "bar\n");

    let out = p.walk_output(["(?<=ba)r"]);

    assert_exit_code(&out, 2);
    assert_stdout_eq(&out, "");
    assert!(normalize_stderr(&out).contains("regex"));
}

#[test]
fn pcre2_enables_backreferences() {
    let p = TestProject::new("pcre2-backreferences");
    p.write("hay.txt", "abab\nabcd\n");

    let out = p.walk_output(["--engine", "pcre2", r"^(ab)\1$"]);

    assert_success(&out);
    assert_stdout_eq(&out, "hay.txt:abab\n");
    assert_stderr_empty(&out);
}

#[test]
fn auto_engine_falls_back_to_pcre2() {
    let p = TestProject::new("pcre2-auto-fallback");
    p.write("hay.txt", "bar\n");

    let out = p.walk_output(["--engine", "auto", "(?<=ba)r"]);

    assert_success(&out);
    assert_stdout_eq(&out, "hay.txt:bar\n");
    assert_stderr_empty(&out);
}

#[test]
fn last_engine_flag_wins() {
    let p = TestProject::new("pcre2-last-wins");
    p.write("hay.txt", "bar\n");

    let pcre2 = p.walk_output(["--no-pcre2", "--pcre2", "(?<=ba)r"]);
    assert_success(&pcre2);
    assert_stdout_eq(&pcre2, "hay.txt:bar\n");

    let default = p.walk_output(["--pcre2", "--no-pcre2", "(?<=ba)r"]);
    assert_exit_code(&default, 2);
}

#[test]
fn pcre2_version_reports_version() {
    let p = TestProject::new("pcre2-version");

    let out = p.walk_output(["--pcre2-version"]);

    assert_success(&out);
    assert_stdout_contains(&out, "PCRE2 ");
    assert_stderr_empty(&out);
}

#[test]
fn engine_equals_form_is_supported() {
    let p = TestProject::new("pcre2-engine-equals");
    p.write("hay.txt", "bar\n");

    let out = p.walk_output(["--engine=pcre2", "(?<=ba)r"]);

    assert_success(&out);
    assert_eq!(normalize_stdout(&out), "hay.txt:bar\n");
    assert_stderr_empty(&out);
}
