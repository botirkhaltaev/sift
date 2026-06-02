//! `sift update` (binary upgrade via install script).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{assert_exit_code, assert_stdout_contains};
use tempfile::TempDir;

fn install_layout(exe_src: &Path) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let bin = tmp.path().join(".local").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let sift = bin.join("sift");
    fs::copy(exe_src, &sift).unwrap();
    let mut perms = fs::metadata(&sift).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&sift, perms).unwrap();
    (tmp, sift)
}

#[test]
fn help_lists_binary_update_subcommand() {
    let out = Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("--help")
        .output()
        .unwrap();
    assert_exit_code(&out, 0);
    assert_stdout_contains(&out, "update");
    assert_stdout_contains(&out, "install the latest release");
}

#[test]
fn binary_update_without_curl_exits_2() {
    let (tmpdir, sift) = install_layout(Path::new(env!("CARGO_BIN_EXE_sift")));

    let path_bin = tmpdir.path().join("path-bin");
    fs::create_dir_all(&path_bin).unwrap();
    let sh = path_bin.join("sh");
    fs::write(&sh, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&sh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&sh, perms).unwrap();

    let out = Command::new(&sift)
        .env("PATH", &path_bin)
        .arg("update")
        .output()
        .unwrap();

    assert_exit_code(&out, 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("curl not found"),
        "expected curl error, got stderr: {stderr}"
    );
}

#[test]
fn binary_update_runs_install_script() {
    let (_tmpdir, sift) = install_layout(Path::new(env!("CARGO_BIN_EXE_sift")));

    let out = Command::new(&sift)
        .env("SIFT_VERSION", "0.3.0")
        .env("SIFT_REPO", "botirk38/sift")
        .arg("update")
        .output()
        .unwrap();

    assert_exit_code(&out, 0);
    assert_stdout_contains(&out, "Installed sift");
}
