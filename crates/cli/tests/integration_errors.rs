//! Edge-case error handling and flag usage tests.

mod common;

use std::process::Command;

use common::{TestProject, assert_exit_code, assert_stdout_contains, assert_success};

#[test]
fn help_flag_prints_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("--help")
        .output()
        .unwrap();
    assert_exit_code(&out, 0);
    assert_stdout_contains(&out, "Usage:");
    assert_stdout_contains(&out, "sift");
}

#[test]
fn help_short_flag() {
    let out = Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("-h")
        .output()
        .unwrap();
    assert_exit_code(&out, 0);
    assert_stdout_contains(&out, "Usage:");
}

#[test]
fn unknown_flag_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("--this-flag-does-not-exist")
        .output()
        .unwrap();
    assert_exit_code(&out, 2);
}

#[test]
fn build_on_nonexistent_path_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("--sift-dir")
        .arg("/tmp/nonexistent-sift-test-xyzzy")
        .arg("build")
        .arg("/tmp/nonexistent-corpus-test-xyzzy")
        .output()
        .unwrap();
    assert_exit_code(&out, 2);
}

#[test]
fn empty_index_search_exits_1() {
    let p = TestProject::new("errors-empty-index");

    let out = p.index_output(["something"]);
    assert_exit_code(&out, 1);
}

#[test]
fn walk_finds_match_without_index() {
    let p = TestProject::new("errors-walk-no-idx");
    p.write("a.txt", "hello\n");

    let out = p.walk_output(["hello"]);
    assert_success(&out);
}
