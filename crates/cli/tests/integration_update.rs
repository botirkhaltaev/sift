//! `sift update` (binary upgrade via install script).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// Spawn a command, retrying on transient `ETXTBSY` (errno 26) which can
/// occur when the OS hasn't fully released the binary after a recent copy.
fn spawn_retry_on_busy(mut build: impl FnMut() -> Command) -> Output {
    for attempt in 0..5_u32 {
        match build().output() {
            Ok(out) => return out,
            Err(e) if e.raw_os_error() == Some(26) && attempt < 4 => {
                std::thread::sleep(std::time::Duration::from_millis(
                    100 * u64::from(attempt + 1),
                ));
            }
            Err(e) => panic!("failed to spawn command: {e}"),
        }
    }
    unreachable!()
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

/// On Windows, `CreateProcessW` searches System32 regardless of PATH, so
/// `curl.exe` (shipped with Windows 10+) is always reachable — the
/// "curl not found" scenario cannot be isolated via PATH alone.
#[cfg(not(windows))]
#[test]
fn binary_update_without_curl_exits_2() {
    let (tmpdir, sift) = install_layout(Path::new(env!("CARGO_BIN_EXE_sift")));

    let path_bin = tmpdir.path().join("path-bin");
    fs::create_dir_all(&path_bin).unwrap();
    write_stub_sh(&path_bin);

    let sift_clone = sift.clone();
    let path_bin_clone = path_bin.clone();
    let out = spawn_retry_on_busy(move || {
        let mut cmd = Command::new(&sift_clone);
        cmd.env("PATH", &path_bin_clone).arg("update");
        cmd
    });

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

    let sift_clone = sift.clone();
    let out = spawn_retry_on_busy(move || {
        let mut cmd = Command::new(&sift_clone);
        cmd.env("SIFT_VERSION", "0.3.0")
            .env("SIFT_REPO", "botirk38/sift")
            .arg("update");
        cmd
    });

    assert_exit_code(&out, 0);
    assert_stdout_contains(&out, "Installed sift");
}
