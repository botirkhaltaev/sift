//! Upgrade the running `sift` binary via the published install script.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_REPO: &str = "botirk38/sift";

/// Resolve `PREFIX` and `BIN_DIR` from the path of the running executable.
///
/// Expects layout `PREFIX/bin/sift` (default `~/.local/bin/sift`).
///
/// # Errors
///
/// Returns an error when the executable path has no parent or `BIN_DIR` has no parent.
pub fn install_dirs_from_exe(exe: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let bin_dir = exe.parent().ok_or_else(|| {
        anyhow::anyhow!("cannot determine install directory from {}", exe.display())
    })?;
    let prefix = bin_dir.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "install path {} has no parent PREFIX; set PREFIX and BIN_DIR and re-run install.sh",
            bin_dir.display()
        )
    })?;
    Ok((prefix.to_path_buf(), bin_dir.to_path_buf()))
}

/// Download and install the latest release over the current binary.
///
/// # Errors
///
/// Returns an error when `curl` or `sh` is missing, install dirs cannot be resolved,
/// or the install script exits unsuccessfully.
pub fn run_binary_update() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let (prefix, bin_dir) = install_dirs_from_exe(&exe)?;

    if Command::new("curl").arg("--version").output().is_err() {
        anyhow::bail!(
            "sift update: curl not found on PATH; install curl or run:\n  \
             curl -fsSL https://raw.githubusercontent.com/{DEFAULT_REPO}/master/scripts/install.sh | sh"
        );
    }
    if Command::new("sh").arg("-c").arg(":").status().is_err() {
        anyhow::bail!("sift update: sh not found on PATH");
    }

    let repo = std::env::var("SIFT_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let url = format!("https://raw.githubusercontent.com/{repo}/master/scripts/install.sh");

    let mut child = Command::new("sh");
    child
        .arg("-c")
        .arg(format!(
            "curl -fsSL '{url}' | PREFIX={prefix} BIN_DIR={bin_dir} sh",
            prefix = shell_escape(&prefix),
            bin_dir = shell_escape(&bin_dir),
        ))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if let Some(version) = std::env::var_os("SIFT_VERSION") {
        child.env("SIFT_VERSION", version);
    }
    child.env("SIFT_REPO", &repo);

    let status = child.status()?;
    if !status.success() {
        anyhow::bail!("sift update: install script exited with {status}");
    }
    Ok(())
}

fn shell_escape(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_dirs_from_standard_layout() {
        let (prefix, bin_dir) =
            install_dirs_from_exe(Path::new("/home/user/.local/bin/sift")).unwrap();
        assert_eq!(bin_dir, PathBuf::from("/home/user/.local/bin"));
        assert_eq!(prefix, PathBuf::from("/home/user/.local"));
    }

    #[test]
    fn install_dirs_rejects_bin_only_path() {
        assert!(install_dirs_from_exe(Path::new("sift")).is_err());
    }
}
