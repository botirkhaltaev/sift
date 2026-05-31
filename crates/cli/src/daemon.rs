use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fslock::LockFile;
use notify::{RecursiveMode, Watcher};
use sift_core::{CorpusSpec, IndexConfig, IndexKind, IndexStore, StoreMeta, VisibilityConfig};

use crate::config::DaemonSpawnConfig;

const DEBOUNCE_MS: u64 = 250;
const SPAWN_LOCK: &str = "daemon-spawn.lock";
const DAEMON_LOCK: &str = "lock";
const READY_DIR: &str = "daemon-ready";
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Platform-specific watcher chosen by cfg.
///
/// On Linux and macOS the native `notify` backend is used (inotify, FSEvent).
/// On Windows the polling backend is used as a workaround for `ReadDirectoryChangesWatcher`
/// not reliably delivering file-creation events in recursive temp-directory watches
/// under CI (GitHub Actions `windows-latest`).
#[cfg(windows)]
type PlatformWatcher = notify::PollWatcher;

#[cfg(not(windows))]
type PlatformWatcher = notify::RecommendedWatcher;

#[cfg(windows)]
fn watcher_config() -> notify::Config {
    notify::Config::default().with_poll_interval(Duration::from_millis(250))
}

#[cfg(not(windows))]
fn watcher_config() -> notify::Config {
    notify::Config::default()
}

/// A cross-platform filesystem watcher.
///
/// Wraps the platform-specific `notify` backend and provides the same
/// [`notify::Watcher`] interface.  Linux and macOS use native inotify /
/// `FSEvent`; Windows uses `PollWatcher` to work around
/// [`ReadDirectoryChangesWatcher` failing to deliver creation events in
/// recursive temp-directory watches on CI](https://github.com/notify-rs/notify/issues/935).
struct FileWatcher {
    inner: PlatformWatcher,
}

impl FileWatcher {
    fn new<F>(event_handler: F) -> notify::Result<Self>
    where
        F: notify::EventHandler,
    {
        Ok(Self {
            inner: PlatformWatcher::new(event_handler, watcher_config())?,
        })
    }

    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.inner.watch(path, mode)
    }
}

// ---------------------------------------------------------------------------
// Spawn types
// ---------------------------------------------------------------------------

/// Outcome of a daemon spawn attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnOutcome {
    Disabled,
    AlreadyRunning,
    Spawned,
}

/// Parameters for launching a daemon child process.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub exe: PathBuf,
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
    /// Startup handshake file the child creates after the watcher is active.
    pub ready_file: Option<PathBuf>,
}

/// Abstraction over process spawning for testability.
pub trait DaemonSpawner {
    /// Spawn a daemon process.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be launched.
    fn spawn(&self, request: &SpawnRequest) -> anyhow::Result<()>;
}

/// Real spawner using `std::process::Command`.
#[derive(Debug, Default)]
pub struct ProcessDaemonSpawner;

impl DaemonSpawner for ProcessDaemonSpawner {
    fn spawn(&self, request: &SpawnRequest) -> anyhow::Result<()> {
        let mut cmd = std::process::Command::new(&request.exe);
        cmd.arg("--sift-dir")
            .arg(&request.sift_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null());
        if let Some(root) = &request.init_root {
            cmd.arg("--init-root").arg(root);
        }
        if let Some(path) = &request.ready_file {
            cmd.arg("--ready-file").arg(path);
        }
        cmd.spawn()?;
        Ok(())
    }
}

/// Lock-based gate for daemon spawn decisions.
///
/// Uses a spawn coordination lock to serialise concurrent parent processes and
/// checks the daemon lock to detect an already-running daemon.
pub struct DaemonSupervisor<S = ProcessDaemonSpawner> {
    spawner: S,
}

impl DaemonSupervisor {
    /// Convenience constructor using the real process spawner.
    #[must_use]
    pub const fn new_process() -> Self {
        Self {
            spawner: ProcessDaemonSpawner,
        }
    }
}

impl<S> DaemonSupervisor<S> {
    #[must_use]
    pub const fn new(spawner: S) -> Self {
        Self { spawner }
    }
}

impl<S: DaemonSpawner> DaemonSupervisor<S> {
    /// Attempt to spawn the daemon according to `config`.
    ///
    /// Returns [`SpawnOutcome::Disabled`] when config has spawning disabled.
    /// Returns [`SpawnOutcome::AlreadyRunning`] when the spawn coordination
    /// lock or the daemon lock is already held.
    /// Returns [`SpawnOutcome::Spawned`] on successful launch after the child
    /// signals startup readiness.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition and process-spawn errors. Returns an error
    /// if the child does not signal readiness within [`READY_TIMEOUT`].
    pub fn spawn(&self, config: &DaemonSpawnConfig) -> anyhow::Result<SpawnOutcome> {
        if !config.enabled {
            return Ok(SpawnOutcome::Disabled);
        }

        let exe = std::env::current_exe()
            .map(|p| p.with_file_name("sift-daemon"))
            .map_err(|e| anyhow::anyhow!("cannot resolve current executable path: {e}"))?;

        let sift_dir = &config.sift_dir;
        std::fs::create_dir_all(sift_dir)?;

        // Acquire spawn coordination lock to prevent concurrent spawn attempts.
        let spawn_lock_path = sift_dir.join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path)?;
        if !spawn_lock.try_lock()? {
            return Ok(SpawnOutcome::AlreadyRunning);
        }

        // Check whether the daemon lock is already held (another daemon is
        // running).  Keep the probe lock released — the child will acquire the
        // daemon lock after starting.
        {
            let daemon_lock_path = sift_dir.join(DAEMON_LOCK);
            let mut daemon_lock = LockFile::open(&daemon_lock_path)?;
            if !daemon_lock.try_lock()? {
                return Ok(SpawnOutcome::AlreadyRunning);
            }
        }

        // Generate a unique ready-file path for this spawn attempt.
        let ready_dir = sift_dir.join(READY_DIR);
        std::fs::create_dir_all(&ready_dir)?;
        let pid = std::process::id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let ready_path = ready_dir.join(format!("{pid}-{now}"));

        // Remove any stale ready file at this path.
        let _ = std::fs::remove_file(&ready_path);

        let request = SpawnRequest {
            exe,
            sift_dir: sift_dir.clone(),
            init_root: config.init_root.clone(),
            ready_file: Some(ready_path.clone()),
        };
        self.spawner.spawn(&request)?;

        // Poll for readiness while holding the spawn lock.
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if ready_path.exists() {
                // Child signalled readiness — clean up and confirm.
                let _ = std::fs::remove_file(&ready_path);
                return Ok(SpawnOutcome::Spawned);
            }
            if Instant::now() >= deadline {
                anyhow::bail!("daemon did not signal readiness within {READY_TIMEOUT:?}");
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }
    }
}

// ---------------------------------------------------------------------------
// Coordinator types
// ---------------------------------------------------------------------------

/// Events flowing through the coordinator channel.
enum CoordinatorEvent {
    Fs(notify::Event),
    RefreshComplete,
}

/// Input to the coordinator state machine.
enum CoordinatorInput {
    ChangeObserved { debounce: Duration },
    DebounceElapsed,
    RefreshFinished,
}

/// Action produced by a state transition.
#[derive(Debug, PartialEq)]
enum CoordinatorAction {
    None,
    StartRefresh,
}

/// Whether a follow-up refresh was requested while one was already running.
#[derive(Clone, Copy)]
enum FollowUpRefresh {
    None,
    Requested,
}

/// Time remaining in a debounce window.
struct DebounceState {
    deadline: Instant,
}

/// Refresh currently running, with an optional follow-up request.
struct RefreshState {
    follow_up: FollowUpRefresh,
}

/// Phase of the coordinator loop.
enum CoordinatorState {
    Idle,
    Debouncing(DebounceState),
    Refreshing(RefreshState),
}

impl CoordinatorState {
    fn timeout(&self, fallback: Duration) -> Duration {
        match self {
            Self::Debouncing(state) => state.deadline.saturating_duration_since(Instant::now()),
            Self::Idle | Self::Refreshing(_) => fallback,
        }
    }

    fn transition(self, input: CoordinatorInput) -> (Self, CoordinatorAction) {
        match (self, input) {
            (Self::Idle | Self::Debouncing(_), CoordinatorInput::ChangeObserved { debounce }) => (
                Self::Debouncing(DebounceState {
                    deadline: Instant::now() + debounce,
                }),
                CoordinatorAction::None,
            ),
            (Self::Debouncing(_), CoordinatorInput::DebounceElapsed) => (
                Self::Refreshing(RefreshState {
                    follow_up: FollowUpRefresh::None,
                }),
                CoordinatorAction::StartRefresh,
            ),
            (Self::Refreshing(_), CoordinatorInput::ChangeObserved { .. }) => (
                Self::Refreshing(RefreshState {
                    follow_up: FollowUpRefresh::Requested,
                }),
                CoordinatorAction::None,
            ),
            (Self::Refreshing(state), CoordinatorInput::RefreshFinished) => match state.follow_up {
                FollowUpRefresh::Requested => (
                    Self::Refreshing(RefreshState {
                        follow_up: FollowUpRefresh::None,
                    }),
                    CoordinatorAction::StartRefresh,
                ),
                FollowUpRefresh::None => (Self::Idle, CoordinatorAction::None),
            },
            (state, _) => (state, CoordinatorAction::None),
        }
    }

    const fn is_refreshing(&self) -> bool {
        matches!(self, Self::Refreshing(_))
    }
}

// ---------------------------------------------------------------------------
// Daemon runner
// ---------------------------------------------------------------------------

/// Loaded daemon metadata extracted from the store.
struct DaemonMeta {
    root: PathBuf,
    corpus_kind: sift_core::CorpusKind,
    follow_links: bool,
    kinds: Vec<IndexKind>,
}

/// Configuration for the long-running daemon process.
#[derive(Debug, Clone)]
pub struct DaemonRunConfig {
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
    /// Internal startup handshake: after the watcher is active, the child
    /// atomically creates this file to signal readiness to the parent.
    pub ready_file: Option<PathBuf>,
}

/// The long-running daemon event loop with stoppable support for testing.
///
/// Uses a coordinator loop that receives filesystem events and manages a
/// background refresh worker. The coordinator can keep processing events
/// while the worker is running, and schedules a follow-up refresh if events
/// arrive during an active refresh.
pub struct DaemonRunner {
    config: DaemonRunConfig,
}

impl DaemonRunner {
    #[must_use]
    pub const fn new(config: DaemonRunConfig) -> Self {
        Self { config }
    }

    /// Run a single build or update, then exit.
    ///
    /// Acquires the daemon lock, loads metadata, and builds the initial
    /// snapshot if none exists.  Returns without entering the watch loop.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, and build errors.
    pub fn run_once(&self) -> anyhow::Result<()> {
        let sift_dir = &self.config.sift_dir;
        std::fs::create_dir_all(sift_dir)?;

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let (root, corpus_kind, follow_links, stored_kinds) =
            match (StoreMeta::read(sift_dir), &self.config.init_root) {
                (Ok(meta), _) => (meta.root, meta.corpus_kind, meta.follow_links, meta.indexes),
                (Err(_), Some(init_root)) => {
                    let root = init_root.canonicalize()?;
                    (root, sift_core::CorpusKind::Directory, false, Vec::new())
                }
                (Err(e), None) => {
                    anyhow::bail!("no store metadata: {e}");
                }
            };
        let kinds: &[IndexKind] = if stored_kinds.is_empty() {
            IndexKind::ALL
        } else {
            &stored_kinds
        };

        let mut store =
            IndexStore::open_or_create(sift_dir, &root, corpus_kind, follow_links, kinds)?;

        let exclude = sift_dir
            .strip_prefix(&root)
            .unwrap_or(sift_dir)
            .to_path_buf();
        let build_config = IndexConfig {
            corpus: CorpusSpec {
                root: &root,
                kind: corpus_kind,
                follow_links,
                include_paths: &[],
                exclude_paths: &[exclude],
            },
            visibility: VisibilityConfig::default(),
        };

        if store.current_id().is_none() {
            store.build(kinds, &build_config)?;
        } else {
            store.update(kinds, &build_config)?;
        }

        Ok(())
    }

    fn load_daemon_meta(
        sift_dir: &Path,
        init_root: Option<&PathBuf>,
    ) -> anyhow::Result<DaemonMeta> {
        match (StoreMeta::read(sift_dir), init_root) {
            (Ok(meta), _) => Ok(DaemonMeta {
                root: meta.root,
                corpus_kind: meta.corpus_kind,
                follow_links: meta.follow_links,
                kinds: if meta.indexes.is_empty() {
                    IndexKind::ALL.to_vec()
                } else {
                    meta.indexes
                },
            }),
            (Err(_), Some(init_root)) => {
                let root = init_root.canonicalize()?;
                Ok(DaemonMeta {
                    root,
                    corpus_kind: sift_core::CorpusKind::Directory,
                    follow_links: false,
                    kinds: IndexKind::ALL.to_vec(),
                })
            }
            (Err(e), None) => {
                anyhow::bail!("no store metadata: {e}");
            }
        }
    }

    /// Run the daemon forever (production use).
    ///
    /// Lock-acquisition, metadata, and watcher errors are propagated
    /// immediately.  Index build/update failures during background
    /// refreshes are logged to stderr and do not propagate — the
    /// daemon stays running and retries on the next file change.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, and watcher setup errors.
    pub fn run(&self) -> anyhow::Result<()> {
        self.run_until(&AtomicBool::new(false))
    }

    /// Run the daemon until `shutdown` becomes `true`.
    ///
    /// Lock-acquisition, metadata, and watcher errors are propagated
    /// immediately.  Index build/update failures during background
    /// refreshes are logged to stderr and do not propagate — the
    /// daemon stays running and retries on the next file change.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, and watcher setup errors.
    pub fn run_until(&self, shutdown: &AtomicBool) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config.sift_dir)?;
        let sift_dir = self
            .config
            .sift_dir
            .canonicalize()
            .unwrap_or_else(|_| self.config.sift_dir.clone());

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let meta = Self::load_daemon_meta(&sift_dir, self.config.init_root.as_ref())?;

        // Unified coordinator channel: receives both watcher events and
        // refresh-completion signals.
        let (tx, rx) = mpsc::channel::<CoordinatorEvent>();

        // Start the filesystem watcher.
        let watcher_tx = tx.clone();
        let mut watcher = FileWatcher::new(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = watcher_tx.send(CoordinatorEvent::Fs(event));
            }
        })?;
        watcher.watch(&meta.root, RecursiveMode::Recursive)?;

        // Signal readiness after acquiring the daemon lock AND setting up the
        // filesystem watcher, so the parent knows the daemon is fully ready to
        // receive events.  Write atomically so the parent never observes a
        // partial file.
        if let Some(ready) = &self.config.ready_file {
            let tmp = ready.with_extension("tmp");
            std::fs::write(&tmp, "")?;
            std::fs::rename(&tmp, ready)?;
        }

        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let mut state = CoordinatorState::Idle;

        // If there is no current snapshot, schedule an initial refresh through
        // the normal refresh path instead of blocking before readiness.
        if !sift_dir.join("CURRENT").exists() {
            state = CoordinatorState::Refreshing(RefreshState {
                follow_up: FollowUpRefresh::None,
            });
            Self::spawn_refresh(
                tx.clone(),
                &sift_dir,
                &meta.kinds,
                &meta.root,
                meta.corpus_kind,
                meta.follow_links,
            );
        }

        loop {
            if shutdown.load(Ordering::Relaxed) {
                Self::drain_active_refresh(&rx, &mut state);
                return Ok(());
            }

            let (next, continue_running) =
                Self::handle_one_event(&rx, tx.clone(), state, &sift_dir, &meta, debounce);

            state = next;
            if !continue_running {
                return Ok(());
            }
        }
    }

    /// Drain any active refresh worker before releasing the daemon lock.
    fn drain_active_refresh(rx: &mpsc::Receiver<CoordinatorEvent>, state: &mut CoordinatorState) {
        while state.is_refreshing() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(CoordinatorEvent::RefreshComplete) => {
                    *state = CoordinatorState::Idle;
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Receive and handle one coordinator event.  Returns the next state and
    /// whether the loop should continue.
    fn handle_one_event(
        rx: &mpsc::Receiver<CoordinatorEvent>,
        tx: mpsc::Sender<CoordinatorEvent>,
        state: CoordinatorState,
        sift_dir: &Path,
        meta: &DaemonMeta,
        debounce: Duration,
    ) -> (CoordinatorState, bool) {
        let timeout = state.timeout(debounce);

        match rx.recv_timeout(timeout) {
            Ok(CoordinatorEvent::Fs(event)) => {
                if Self::should_ignore_event(&event, sift_dir) {
                    return (state, true);
                }
                let (next, action) =
                    state.transition(CoordinatorInput::ChangeObserved { debounce });
                if matches!(action, CoordinatorAction::StartRefresh) {
                    Self::spawn_refresh(
                        tx,
                        sift_dir,
                        &meta.kinds,
                        &meta.root,
                        meta.corpus_kind,
                        meta.follow_links,
                    );
                }
                (next, true)
            }
            Ok(CoordinatorEvent::RefreshComplete) => {
                let (next, action) = state.transition(CoordinatorInput::RefreshFinished);
                if matches!(action, CoordinatorAction::StartRefresh) {
                    Self::spawn_refresh(
                        tx,
                        sift_dir,
                        &meta.kinds,
                        &meta.root,
                        meta.corpus_kind,
                        meta.follow_links,
                    );
                }
                (next, true)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let (next, action) = state.transition(CoordinatorInput::DebounceElapsed);
                if matches!(action, CoordinatorAction::StartRefresh) {
                    Self::spawn_refresh(
                        tx,
                        sift_dir,
                        &meta.kinds,
                        &meta.root,
                        meta.corpus_kind,
                        meta.follow_links,
                    );
                }
                (next, true)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => (state, false),
        }
    }

    /// Returns `true` when a filesystem event should be ignored.
    fn should_ignore_event(event: &notify::Event, sift_dir: &Path) -> bool {
        matches!(event.kind, notify::EventKind::Access(_))
            || event.paths.iter().any(|p| p.starts_with(sift_dir))
    }

    /// Spawn a background refresh worker thread.
    fn spawn_refresh(
        tx: mpsc::Sender<CoordinatorEvent>,
        sift_dir: &Path,
        kinds: &[IndexKind],
        root: &Path,
        corpus_kind: sift_core::CorpusKind,
        follow_links: bool,
    ) {
        let sift_dir = sift_dir.to_path_buf();
        let kinds = kinds.to_vec();
        let root = root.to_path_buf();

        std::thread::spawn(move || {
            let exclude = sift_dir
                .strip_prefix(&root)
                .unwrap_or(&sift_dir)
                .to_path_buf();
            let config = IndexConfig {
                corpus: CorpusSpec {
                    root: &root,
                    kind: corpus_kind,
                    follow_links,
                    include_paths: &[],
                    exclude_paths: &[exclude],
                },
                visibility: VisibilityConfig::default(),
            };

            let result = (|| -> anyhow::Result<()> {
                let mut store = IndexStore::open_or_create(
                    &sift_dir,
                    &root,
                    corpus_kind,
                    follow_links,
                    &kinds,
                )?;
                store.update(&kinds, &config)?;
                Ok(())
            })();

            if let Err(e) = result {
                eprintln!("sift-daemon: refresh failed: {e}");
            }

            let _ = tx.send(CoordinatorEvent::RefreshComplete);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sift_core::CorpusKind;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use tempfile::TempDir;

    /// A fake spawner that records calls.
    #[derive(Clone, Default)]
    struct FakeSpawner {
        calls: Arc<AtomicUsize>,
        last_request: Arc<std::sync::Mutex<Option<SpawnRequest>>>,
    }

    impl FakeSpawner {
        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_request(&self) -> Option<SpawnRequest> {
            self.last_request.lock().unwrap().clone()
        }
    }

    impl DaemonSpawner for FakeSpawner {
        fn spawn(&self, request: &SpawnRequest) -> anyhow::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_request.lock().unwrap() = Some(request.clone());
            // Simulate daemon startup: create the ready file so the parent
            // doesn't time out waiting for the real daemon.
            if let Some(path) = &request.ready_file {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(path, "")?;
            }
            Ok(())
        }
    }

    fn fake_spawner() -> FakeSpawner {
        FakeSpawner::default()
    }

    fn supervisor(s: FakeSpawner) -> DaemonSupervisor<FakeSpawner> {
        DaemonSupervisor::new(s)
    }

    fn spawn_config(enabled: bool, sift_dir: PathBuf) -> DaemonSpawnConfig {
        DaemonSpawnConfig {
            enabled,
            sift_dir,
            init_root: None,
        }
    }

    #[test]
    fn spawn_returns_disabled_from_config() {
        let dir = TempDir::new().unwrap();
        let sup = supervisor(fake_spawner());
        let config = spawn_config(false, dir.path().to_path_buf());
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::Disabled);
    }

    #[test]
    fn spawn_invokes_spawner_when_lock_free() {
        let dir = TempDir::new().unwrap();
        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = spawn_config(true, dir.path().to_path_buf());
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::Spawned);
        assert_eq!(spawner.call_count(), 1);
    }

    #[test]
    fn spawn_returns_already_running_when_daemon_lock_held() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        lock.try_lock().unwrap();

        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = spawn_config(true, dir.path().to_path_buf());
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::AlreadyRunning);
        assert_eq!(spawner.call_count(), 0);
    }

    #[test]
    fn spawn_returns_already_running_when_spawn_lock_held() {
        let dir = TempDir::new().unwrap();
        let spawn_lock_path = dir.path().join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path).unwrap();
        spawn_lock.try_lock().unwrap();

        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = spawn_config(true, dir.path().to_path_buf());
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::AlreadyRunning);
        assert_eq!(spawner.call_count(), 0);
    }

    #[test]
    fn spawn_passes_sift_dir_and_init_root() {
        let dir = TempDir::new().unwrap();
        let init_root = Some(PathBuf::from("/tmp/init"));
        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            init_root,
        };
        sup.spawn(&config).unwrap();

        let req = spawner.last_request().unwrap();
        assert_eq!(req.sift_dir, config.sift_dir);
        assert_eq!(req.init_root, config.init_root);
    }

    #[test]
    fn spawn_lock_prevents_concurrent_attempts() {
        let dir = TempDir::new().unwrap();
        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());

        let spawn_lock_path = dir.path().join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path).unwrap();
        spawn_lock.try_lock().unwrap();

        let config = spawn_config(true, dir.path().to_path_buf());
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::AlreadyRunning);
        assert_eq!(spawner.call_count(), 0);
    }

    // -----------------------------------------------------------------------
    // CoordinatorState::transition tests
    // -----------------------------------------------------------------------

    const DEBOUNCE: Duration = Duration::from_mins(1);

    #[test]
    fn idle_change_observed_transitions_to_debouncing() {
        let (next, action) = CoordinatorState::Idle
            .transition(CoordinatorInput::ChangeObserved { debounce: DEBOUNCE });
        assert!(matches!(next, CoordinatorState::Debouncing(_)));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn idle_debounce_elapsed_stays_idle() {
        let (next, action) = CoordinatorState::Idle.transition(CoordinatorInput::DebounceElapsed);
        assert!(matches!(next, CoordinatorState::Idle));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn idle_refresh_finished_stays_idle() {
        let (next, action) = CoordinatorState::Idle.transition(CoordinatorInput::RefreshFinished);
        assert!(matches!(next, CoordinatorState::Idle));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_change_observed_resets_debounce() {
        let state = CoordinatorState::Debouncing(DebounceState {
            deadline: Instant::now(),
        });
        let (next, action) =
            state.transition(CoordinatorInput::ChangeObserved { debounce: DEBOUNCE });
        assert!(matches!(next, CoordinatorState::Debouncing(_)));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_debounce_elapsed_starts_refresh() {
        let state = CoordinatorState::Debouncing(DebounceState {
            deadline: Instant::now(),
        });
        let (next, action) = state.transition(CoordinatorInput::DebounceElapsed);
        assert!(
            matches!(next, CoordinatorState::Refreshing(s) if matches!(s.follow_up, FollowUpRefresh::None))
        );
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn debouncing_refresh_finished_stays_debouncing() {
        let state = CoordinatorState::Debouncing(DebounceState {
            deadline: Instant::now() + DEBOUNCE,
        });
        let (next, action) = state.transition(CoordinatorInput::RefreshFinished);
        assert!(matches!(next, CoordinatorState::Debouncing(_)));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_change_observed_requests_follow_up() {
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::None,
        });
        let (next, action) =
            state.transition(CoordinatorInput::ChangeObserved { debounce: DEBOUNCE });
        assert!(
            matches!(next, CoordinatorState::Refreshing(s) if matches!(s.follow_up, FollowUpRefresh::Requested))
        );
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_debounce_elapsed_stays_refreshing() {
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::None,
        });
        let (next, action) = state.transition(CoordinatorInput::DebounceElapsed);
        assert!(
            matches!(next, CoordinatorState::Refreshing(s) if matches!(s.follow_up, FollowUpRefresh::None))
        );
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_finished_without_follow_up_returns_to_idle() {
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::None,
        });
        let (next, action) = state.transition(CoordinatorInput::RefreshFinished);
        assert!(matches!(next, CoordinatorState::Idle));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_finished_with_follow_up_restarts_refresh() {
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::Requested,
        });
        let (next, action) = state.transition(CoordinatorInput::RefreshFinished);
        assert!(
            matches!(next, CoordinatorState::Refreshing(s) if matches!(s.follow_up, FollowUpRefresh::None))
        );
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn refreshing_finished_with_follow_up_clears_request_flag() {
        // Regression: follow-up flag must be reset to None when restarting.
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::Requested,
        });
        let (next, _action) = state.transition(CoordinatorInput::RefreshFinished);
        match next {
            CoordinatorState::Refreshing(s) => assert!(
                matches!(s.follow_up, FollowUpRefresh::None),
                "follow_up should be None after restart"
            ),
            _ => panic!("expected Refreshing state"),
        }
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_true_for_refreshing() {
        let state = CoordinatorState::Refreshing(RefreshState {
            follow_up: FollowUpRefresh::None,
        });
        assert!(state.is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_idle() {
        assert!(!CoordinatorState::Idle.is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_debouncing() {
        let state = CoordinatorState::Debouncing(DebounceState {
            deadline: Instant::now(),
        });
        assert!(!state.is_refreshing());
    }

    #[test]
    fn run_until_stops_on_shutdown() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");

        let meta = sift_core::StoreMeta::new(
            dir.path().to_path_buf(),
            CorpusKind::Directory,
            false,
            vec![IndexKind::Trigram],
        );
        std::fs::create_dir_all(&sift_dir).unwrap();
        sift_core::StoreMeta::write(&meta, &sift_dir).unwrap();

        let config = DaemonRunConfig {
            sift_dir,
            init_root: None,
            ready_file: None,
        };
        let runner = DaemonRunner::new(config);
        let shutdown = Arc::new(AtomicBool::new(false));
        let s = Arc::clone(&shutdown);

        let handle = std::thread::spawn(move || {
            runner.run_until(&s).unwrap();
        });

        std::thread::sleep(Duration::from_millis(100));
        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn run_until_returns_early_when_lock_held() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).unwrap();
        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        lock.try_lock().unwrap();

        let config = DaemonRunConfig {
            sift_dir,
            init_root: None,
            ready_file: None,
        };
        let runner = DaemonRunner::new(config);
        runner.run_until(&AtomicBool::new(false)).unwrap();
    }
}
