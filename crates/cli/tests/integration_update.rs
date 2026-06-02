//! `sift update` (binary upgrade via install script).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{assert_exit_code, assert_stdout_contains};
use tempfile::TempDir;

const fn installed_sift_name() -> &'static str {
    if cfg!(windows) { "sift.exe" } else { "sift" }
}

/// Executable used to run `sift update` in tests. Must differ from [`installed_sift_name`]
/// so the install script can replace `bin/sift` while this process is still running (ETXTBSY).
const fn update_runner_name() -> &'static str {
    if cfg!(windows) {
        "sift-update-test.exe"
    } else {
        "sift-update-test"
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(windows)]
const fn make_executable(path: &Path) {
    let _ = path;
}

fn write_stub_sh(path_bin: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let sh = path_bin.join("sh.cmd");
        fs::write(&sh, "@echo off\r\nexit /b 0\r\n").unwrap();
        sh
    }
    #[cfg(not(windows))]
    {
        let sh = path_bin.join("sh");
        fs::write(&sh, "#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&sh);
        sh
    }
}

fn install_layout(exe_src: &Path) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let bin = tmp.path().join(".local").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let install_target = bin.join(installed_sift_name());
    let runner = bin.join(update_runner_name());
    fs::copy(exe_src, &install_target).unwrap();
    fs::copy(exe_src, &runner).unwrap();
    make_executable(&install_target);
    make_executable(&runner);
    (tmp, runner)
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
    write_stub_sh(&path_bin);

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
