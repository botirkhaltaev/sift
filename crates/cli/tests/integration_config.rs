mod common;

use common::{
    TestProject, assert_exit_code, assert_stderr_empty, assert_stdout_contains,
    assert_stdout_not_contains, normalize_stderr,
};

#[test]
fn ripgrep_config_path_applies_arguments_before_cli() {
    let p = TestProject::new("config-precedence");
    p.write("a.txt", "FOO\n");
    p.write("sift.rc", "--ignore-case\n");

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("sift.rc"));
    let out = cmd.args(["foo"]).output().unwrap();
    assert_stdout_contains(&out, "a.txt:FOO");

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("sift.rc"));
    let out = cmd.args(["--case-sensitive", "foo"]).output().unwrap();
    assert_exit_code(&out, 1);
}

#[test]
fn config_parser_ignores_comments_and_blank_lines() {
    let p = TestProject::new("config-comments");
    p.write("a.rs", "needle\n");
    p.write("a.txt", "needle\n");
    p.write(
        "sift.rc",
        "\n\
         # only Rust files\n\
         --glob=*.rs\n\
         \n",
    );

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("sift.rc"));
    let out = cmd.args(["needle"]).output().unwrap();
    assert_stdout_contains(&out, "a.rs:needle");
    common::assert_stdout_not_contains(&out, "a.txt:needle");
}

#[test]
fn no_config_disables_ripgrep_config_path() {
    let p = TestProject::new("config-disabled");
    p.write("a.txt", "FOO\n");
    p.write("sift.rc", "--ignore-case\n");

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("sift.rc"));
    let out = cmd.args(["--no-config", "foo"]).output().unwrap();
    assert_exit_code(&out, 1);
    assert_stderr_empty(&out);
}

#[test]
fn missing_config_path_is_fatal_for_search() {
    let p = TestProject::new("config-missing");
    p.write("a.txt", "needle\n");

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("missing.rc"));
    let out = cmd.args(["needle"]).output().unwrap();
    assert_exit_code(&out, 1);
    assert_stdout_not_contains(&out, "a.txt:needle");
    let stderr = normalize_stderr(&out);
    assert!(
        stderr.contains("failed to read the file specified in RIPGREP_CONFIG_PATH"),
        "expected missing config warning, got: {stderr}"
    );
}

#[test]
fn ripgrep_config_path_does_not_apply_to_index_subcommands() {
    let p = TestProject::new("config-index-subcommand");
    p.write("a.txt", "needle\n");

    let mut cmd = p.sift();
    cmd.env("RIPGREP_CONFIG_PATH", p.root().join("missing.rc"));
    let out = cmd.args(["index", "build", "--wait"]).output().unwrap();
    assert_exit_code(&out, 0);
}
