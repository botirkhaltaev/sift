use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use fslock::LockFile;

use super::Daemon;
use super::error::DaemonError;
use super::watch::{DAEMON_LOCK, READY_DIR, READY_POLL_INTERVAL, READY_TIMEOUT, SPAWN_LOCK};

fn daemon_exe() -> Result<PathBuf, DaemonError> {
    let sift = std::env::current_exe().map_err(DaemonError::Io)?;
    let sibling = sift.with_file_name("sift-daemon");
    if sibling.exists() {
        return Ok(sibling);
    }
    if let Some(debug_bin) = sift
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("sift-daemon"))
        .filter(|p| p.exists())
    {
        return Ok(debug_bin);
    }
    Ok(sibling)
}

impl Daemon {
    pub(super) fn ensure_running(&self) -> Result<(), DaemonError> {
        let sift_dir = &self.sift_dir;
        let init_root = self.init_root.as_deref();
        let exe = daemon_exe()?;

        std::fs::create_dir_all(sift_dir)?;

        let spawn_lock_path = sift_dir.join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path)?;
        if !spawn_lock.try_lock()? {
            return Ok(());
        }

        {
            let daemon_lock_path = sift_dir.join(DAEMON_LOCK);
            let mut daemon_lock = LockFile::open(&daemon_lock_path)?;
            if !daemon_lock.try_lock()? {
                return Ok(());
            }
        }

        let ready_dir = sift_dir.join(READY_DIR);
        std::fs::create_dir_all(&ready_dir)?;
        let pid = std::process::id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let ready_path = ready_dir.join(format!("{pid}-{now}"));
        let _ = std::fs::remove_file(&ready_path);

        let mut cmd = Command::new(&exe);
        cmd.arg("--sift-dir")
            .arg(sift_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null());
        if let Some(root) = init_root {
            cmd.arg("--init-root").arg(root);
        }
        cmd.arg("--ready-file").arg(&ready_path);
        cmd.spawn()?;

        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if ready_path.exists() {
                let _ = std::fs::remove_file(&ready_path);
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(DaemonError::message(format!(
                    "daemon did not signal readiness within {READY_TIMEOUT:?}"
                )));
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_running_returns_when_daemon_lock_held() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        lock.try_lock().unwrap();

        Daemon::new(dir.path().to_path_buf())
            .ensure_running()
            .unwrap();
    }

    #[test]
    fn ensure_running_returns_when_spawn_lock_held() {
        let dir = TempDir::new().unwrap();
        let spawn_lock_path = dir.path().join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path).unwrap();
        spawn_lock.try_lock().unwrap();

        Daemon::new(dir.path().to_path_buf())
            .ensure_running()
            .unwrap();
    }
}
