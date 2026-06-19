use notify::Watcher;

pub(super) const DAEMON_LOCK: &str = "lock";
pub(super) const DAEMON_SOCKET: &str = "daemon.sock";
pub(super) const DEFAULT_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(2);
pub(super) const READY_DIR: &str = "daemon-ready";
pub(super) const READY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
pub(super) const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(super) const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_secs(1);
pub(super) const SPAWN_LOCK: &str = "daemon-spawn.lock";

#[cfg(windows)]
type PlatformWatcher = notify::PollWatcher;

#[cfg(not(windows))]
type PlatformWatcher = notify::RecommendedWatcher;

#[cfg(windows)]
fn watcher_config() -> notify::Config {
    notify::Config::default()
        .with_poll_interval(std::time::Duration::from_millis(super::DEBOUNCE_MS))
}

#[cfg(not(windows))]
fn watcher_config() -> notify::Config {
    notify::Config::default()
}

pub(super) struct FileWatcher {
    inner: PlatformWatcher,
}

impl FileWatcher {
    pub fn new<F>(event_handler: F) -> notify::Result<Self>
    where
        F: notify::EventHandler,
    {
        Ok(Self {
            inner: PlatformWatcher::new(event_handler, watcher_config())?,
        })
    }

    pub fn watch(
        &mut self,
        path: &std::path::Path,
        mode: notify::RecursiveMode,
    ) -> notify::Result<()> {
        self.inner.watch(path, mode)
    }
}
