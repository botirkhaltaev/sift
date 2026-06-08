use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fslock::LockFile;
use notify::{RecursiveMode, Watcher};
use sift_core::{CorpusSpec, IndexConfig, IndexKind, IndexStore, StoreMeta, VisibilityConfig};

const DEBOUNCE_MS: u64 = 250;
/// Default idle timeout for the daemon (2 minutes).
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_mins(2);
/// Maximum `recv_timeout` — keeps the event loop responsive to the
/// `shutdown` flag even when the next state deadline is far away.
const SHUTDOWN_POLL: Duration = Duration::from_secs(1);
const SPAWN_LOCK: &str = "daemon-spawn.lock";
const DAEMON_LOCK: &str = "lock";
const READY_DIR: &str = "daemon-ready";
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Platform-specific watcher chosen by cfg.
///
/// On Linux and macOS the native `notify` backend is used (inotify, `FSEvent`).
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

/// Whether the daemon should watch continuously or perform a single operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DaemonMode {
    /// Long-running watch loop with filesystem monitoring.
    #[default]
    Watch,
    /// Build/update once and exit. Fire-and-forget — no readiness handshake.
    Once,
}

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
    pub mode: DaemonMode,
}

/// Spawn policy for the daemon background process.
#[derive(Debug, Clone)]
pub struct DaemonSpawnConfig {
    pub enabled: bool,
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
    pub mode: DaemonMode,
}

impl Default for DaemonSpawnConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sift_dir: PathBuf::new(),
            init_root: None,
            mode: DaemonMode::Watch,
        }
    }
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
        if request.mode == DaemonMode::Once {
            cmd.arg("--once");
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
    /// In [`DaemonMode::Watch`] mode, acquires the spawn coordination lock,
    /// checks the daemon lock, spawns the child, and polls for startup
    /// readiness.
    ///
    /// In [`DaemonMode::Once`] mode, spawns a fire-and-forget daemon that
    /// builds/updates once and exits. No lock coordination or readiness
    /// handshake — the child acquires the daemon lock internally.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition and process-spawn errors. In Watch mode,
    /// returns an error if the child does not signal readiness within
    /// [`READY_TIMEOUT`].
    pub fn spawn(&self, config: &DaemonSpawnConfig) -> anyhow::Result<SpawnOutcome> {
        if !config.enabled {
            return Ok(SpawnOutcome::Disabled);
        }

        let exe = std::env::current_exe()
            .map(|p| p.with_file_name("sift-daemon"))
            .map_err(|e| anyhow::anyhow!("cannot resolve current executable path: {e}"))?;

        let sift_dir = &config.sift_dir;
        std::fs::create_dir_all(sift_dir)?;

        match config.mode {
            DaemonMode::Once => {
                let request = SpawnRequest {
                    exe,
                    sift_dir: sift_dir.clone(),
                    init_root: config.init_root.clone(),
                    ready_file: None,
                    mode: DaemonMode::Once,
                };
                self.spawner.spawn(&request)?;
                Ok(SpawnOutcome::Spawned)
            }
            DaemonMode::Watch => self.coordinate_launch(exe, config),
        }
    }

    /// Acquire coordination locks, spawn the daemon, and poll for readiness.
    fn coordinate_launch(
        &self,
        exe: PathBuf,
        config: &DaemonSpawnConfig,
    ) -> anyhow::Result<SpawnOutcome> {
        let sift_dir = &config.sift_dir;

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
            mode: DaemonMode::Watch,
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

/// Messages sent through the coordinator channel.
enum CoordinatorMessage {
    FsChange(notify::Event),
    RefreshComplete,
}

/// Input to the coordinator state machine.
enum CoordinatorInput {
    FsChange,
    RefreshComplete,
    DeadlineReached,
}

/// Action produced by a state transition.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CoordinatorAction {
    None,
    StartRefresh,
    Exit,
}

/// Phase of the coordinator loop.
enum CoordinatorState {
    Idle { deadline: Instant },
    Debouncing { deadline: Instant },
    Refreshing { follow_up: bool },
}

impl CoordinatorState {
    fn new_idle(idle_timeout: Duration) -> Self {
        Self::Idle {
            deadline: Instant::now() + idle_timeout,
        }
    }

    /// The deadline at which a [`CoordinatorInput::DeadlineReached`] input
    /// should be generated.  Returns `None` when the state has no deadline
    /// (i.e. `Refreshing`, which waits for `RefreshComplete`).
    const fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle { deadline } | Self::Debouncing { deadline } => Some(*deadline),
            Self::Refreshing { .. } => None,
        }
    }

    /// Pure state transition.  `debounce` and `idle_timeout` are loop config.
    fn transition(
        self,
        input: CoordinatorInput,
        debounce: Duration,
        idle_timeout: Duration,
    ) -> (Self, CoordinatorAction) {
        match (self, input) {
            (Self::Idle { .. } | Self::Debouncing { .. }, CoordinatorInput::FsChange) => (
                Self::Debouncing {
                    deadline: Instant::now() + debounce,
                },
                CoordinatorAction::None,
            ),
            (Self::Debouncing { .. }, CoordinatorInput::DeadlineReached)
            | (Self::Refreshing { follow_up: true }, CoordinatorInput::RefreshComplete) => (
                Self::Refreshing { follow_up: false },
                CoordinatorAction::StartRefresh,
            ),
            (Self::Idle { .. }, CoordinatorInput::DeadlineReached) => {
                (Self::new_idle(idle_timeout), CoordinatorAction::Exit)
            }
            (Self::Refreshing { .. }, CoordinatorInput::FsChange) => (
                Self::Refreshing { follow_up: true },
                CoordinatorAction::None,
            ),
            (Self::Refreshing { follow_up: false }, CoordinatorInput::RefreshComplete) => {
                (Self::new_idle(idle_timeout), CoordinatorAction::None)
            }
            (state, _) => (state, CoordinatorAction::None),
        }
    }

    const fn is_refreshing(&self) -> bool {
        matches!(self, Self::Refreshing { .. })
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
    /// How long the daemon stays alive with no filesystem activity before
    /// exiting gracefully.  Defaults to 2 minutes.
    pub idle_timeout: Duration,
}

impl Default for DaemonRunConfig {
    fn default() -> Self {
        Self {
            sift_dir: PathBuf::new(),
            init_root: None,
            ready_file: None,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
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
    /// Lifecycle: lock → metadata → reconcile → ready signal → watcher → loop.
    /// Reconciliation runs synchronously **before** the watcher starts, so
    /// index writes never generate spurious watcher events.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, and watcher setup errors.
    pub fn run_until(&self, shutdown: &AtomicBool) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config.sift_dir)?;

        // Keep both the raw and canonical forms of `sift_dir`.  On macOS
        // temp directories are symlinked (`/var/folders` →
        // `/private/var/folders`).  FSEvent returns raw (symlink) paths,
        // while `IndexStore` needs the canonical path.  Checking both in
        // `should_ignore` avoids calling `canonicalize()` on each event
        // path (which fails for already-deleted temp files).
        let sift_dir_raw = self.config.sift_dir.clone();
        let sift_dir = sift_dir_raw
            .canonicalize()
            .unwrap_or_else(|_| sift_dir_raw.clone());

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let meta = Self::load_daemon_meta(&sift_dir, self.config.init_root.as_ref())?;

        // Reconcile synchronously before setting up the watcher.  This
        // guarantees the index is up-to-date and avoids the watcher seeing
        // its own index-write events (the root cause of macOS CI hangs).
        Self::refresh_index(&sift_dir, &meta);

        // Signal readiness after lock + reconciliation.
        if let Some(ready) = &self.config.ready_file {
            let tmp = ready.with_extension("tmp");
            std::fs::write(&tmp, "")?;
            std::fs::rename(&tmp, ready)?;
        }

        // Start the watcher AFTER reconciliation so it never sees the
        // index writes from the initial build/update.
        let (tx, rx) = mpsc::channel::<CoordinatorMessage>();
        let watcher_tx = tx.clone();
        let mut watcher = FileWatcher::new(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = watcher_tx.send(CoordinatorMessage::FsChange(event));
            }
        })?;
        watcher.watch(&meta.root, RecursiveMode::Recursive)?;

        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let idle_timeout = self.config.idle_timeout;
        let mut state = CoordinatorState::new_idle(idle_timeout);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                Self::drain_active_refresh(&rx, &mut state);
                return Ok(());
            }

            let timeout = state.deadline().map_or(SHUTDOWN_POLL, |d| {
                d.saturating_duration_since(Instant::now())
                    .min(SHUTDOWN_POLL)
            });

            let input = match rx.recv_timeout(timeout) {
                Ok(CoordinatorMessage::FsChange(event)) => {
                    if Self::should_ignore(&event, &sift_dir, &sift_dir_raw) {
                        continue;
                    }
                    CoordinatorInput::FsChange
                }
                Ok(CoordinatorMessage::RefreshComplete) => CoordinatorInput::RefreshComplete,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if state.deadline().is_some_and(|d| Instant::now() >= d) {
                        CoordinatorInput::DeadlineReached
                    } else {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };

            let (next, action) = state.transition(input, debounce, idle_timeout);
            if !Self::execute(action, &tx, &sift_dir, &meta) {
                return Ok(());
            }
            state = next;
        }
    }

    /// Drain any active refresh worker before releasing the daemon lock.
    fn drain_active_refresh(rx: &mpsc::Receiver<CoordinatorMessage>, state: &mut CoordinatorState) {
        while state.is_refreshing() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(CoordinatorMessage::RefreshComplete) => {
                    *state = CoordinatorState::new_idle(DEFAULT_IDLE_TIMEOUT);
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Execute a coordinator action.  Returns `true` when the loop should
    /// continue, `false` when the daemon should exit.
    fn execute(
        action: CoordinatorAction,
        tx: &mpsc::Sender<CoordinatorMessage>,
        sift_dir: &Path,
        meta: &DaemonMeta,
    ) -> bool {
        match action {
            CoordinatorAction::None => true,
            CoordinatorAction::StartRefresh => {
                Self::spawn_refresh(tx.clone(), sift_dir, meta);
                true
            }
            CoordinatorAction::Exit => false,
        }
    }

    /// Returns `true` when a filesystem event should be ignored.
    ///
    /// Checks event paths against both the canonical and raw `sift_dir`
    /// paths.  On macOS, `FSEvent` delivers symlink-based paths that won't
    /// match the canonical form; checking the raw form avoids calling
    /// `canonicalize()` on each event path (which fails for temp files
    /// deleted during index builds).
    fn should_ignore(event: &notify::Event, canonical: &Path, raw: &Path) -> bool {
        matches!(event.kind, notify::EventKind::Access(_))
            || event
                .paths
                .iter()
                .any(|p| p.starts_with(canonical) || p.starts_with(raw))
    }

    /// Build or update the index for the watched corpus.
    fn refresh_index(sift_dir: &Path, meta: &DaemonMeta) {
        let exclude = sift_dir
            .strip_prefix(&meta.root)
            .unwrap_or(sift_dir)
            .to_path_buf();
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: &meta.root,
                kind: meta.corpus_kind,
                follow_links: meta.follow_links,
                include_paths: &[],
                exclude_paths: &[exclude],
            },
            visibility: VisibilityConfig::default(),
        };

        let result = (|| -> anyhow::Result<()> {
            let mut store = IndexStore::open_or_create(
                sift_dir,
                &meta.root,
                meta.corpus_kind,
                meta.follow_links,
                &meta.kinds,
            )?;
            store.update(&meta.kinds, &config)?;
            Ok(())
        })();

        if let Err(e) = result {
            eprintln!("sift-daemon: refresh failed: {e}");
        }
    }

    /// Spawn a background refresh worker thread.
    fn spawn_refresh(tx: mpsc::Sender<CoordinatorMessage>, sift_dir: &Path, meta: &DaemonMeta) {
        let sift_dir = sift_dir.to_path_buf();
        let root = meta.root.clone();
        let corpus_kind = meta.corpus_kind;
        let follow_links = meta.follow_links;
        let kinds = meta.kinds.clone();

        std::thread::spawn(move || {
            let meta = DaemonMeta {
                root,
                corpus_kind,
                follow_links,
                kinds,
            };
            Self::refresh_index(&sift_dir, &meta);
            let _ = tx.send(CoordinatorMessage::RefreshComplete);
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

    #[test]
    fn spawn_returns_disabled_from_config() {
        let dir = TempDir::new().unwrap();
        let sup = supervisor(fake_spawner());
        let config = DaemonSpawnConfig {
            enabled: false,
            sift_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::Disabled);
    }

    #[test]
    fn spawn_invokes_spawner_when_lock_free() {
        let dir = TempDir::new().unwrap();
        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
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
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
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
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
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
            ..Default::default()
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

        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::AlreadyRunning);
        assert_eq!(spawner.call_count(), 0);
    }

    #[test]
    fn spawn_once_mode_skips_lock_and_readiness() {
        let dir = TempDir::new().unwrap();
        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            mode: DaemonMode::Once,
            ..Default::default()
        };
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::Spawned);
        assert_eq!(spawner.call_count(), 1);

        let req = spawner.last_request().unwrap();
        assert_eq!(req.mode, DaemonMode::Once);
        assert!(req.ready_file.is_none());
    }

    #[test]
    fn spawn_once_mode_succeeds_even_when_daemon_lock_held() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        lock.try_lock().unwrap();

        let spawner = fake_spawner();
        let sup = supervisor(spawner.clone());
        let config = DaemonSpawnConfig {
            enabled: true,
            sift_dir: dir.path().to_path_buf(),
            mode: DaemonMode::Once,
            ..Default::default()
        };
        let outcome = sup.spawn(&config).unwrap();
        assert_eq!(outcome, SpawnOutcome::Spawned);
        assert_eq!(spawner.call_count(), 1);
    }

    // -----------------------------------------------------------------------
    // CoordinatorState::transition tests
    // -----------------------------------------------------------------------

    const DEBOUNCE: Duration = Duration::from_mins(1);
    const IDLE: Duration = Duration::from_mins(10);

    fn idle_state() -> CoordinatorState {
        CoordinatorState::new_idle(IDLE)
    }

    #[test]
    fn idle_fs_change_transitions_to_debouncing() {
        let (next, action) = idle_state().transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn idle_deadline_reached_exits() {
        let (next, action) =
            idle_state().transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::Exit);
    }

    #[test]
    fn idle_refresh_complete_stays_idle() {
        let (next, action) =
            idle_state().transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_fs_change_resets_debounce() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        let (next, action) = state.transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_deadline_reached_starts_refresh() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn debouncing_refresh_complete_stays_debouncing() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now() + DEBOUNCE,
        };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_deadline_reached_stays_debouncing_on_catch_all() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now() + DEBOUNCE,
        };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn refreshing_fs_change_requests_follow_up() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if follow_up));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_deadline_reached_stays_refreshing() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_complete_without_follow_up_returns_to_idle() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_complete_with_follow_up_restarts_refresh() {
        let state = CoordinatorState::Refreshing { follow_up: true };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn refreshing_complete_with_follow_up_clears_flag() {
        let state = CoordinatorState::Refreshing { follow_up: true };
        let (next, _action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        match next {
            CoordinatorState::Refreshing { follow_up } => {
                assert!(!follow_up, "follow_up should be false after restart");
            }
            _ => panic!("expected Refreshing state"),
        }
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_true_for_refreshing() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        assert!(state.is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_idle() {
        assert!(!idle_state().is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_debouncing() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        assert!(!state.is_refreshing());
    }

    #[test]
    fn refreshing_complete_resets_idle_deadline() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let before = Instant::now();
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert_eq!(action, CoordinatorAction::None);
        match next {
            CoordinatorState::Idle { deadline } => {
                assert!(deadline >= before + IDLE);
            }
            _ => panic!("expected Idle state"),
        }
    }

    // -----------------------------------------------------------------------
    // DaemonRunner integration tests
    // -----------------------------------------------------------------------

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
            ..Default::default()
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
            ..Default::default()
        };
        let runner = DaemonRunner::new(config);
        runner.run_until(&AtomicBool::new(false)).unwrap();
    }

    #[test]
    fn run_until_exits_on_idle_timeout() {
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
            sift_dir: sift_dir.clone(),
            idle_timeout: Duration::from_secs(2),
            ..Default::default()
        };
        let runner = DaemonRunner::new(config);

        let start = Instant::now();
        runner.run_until(&AtomicBool::new(false)).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_secs(1),
            "daemon exited too early: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "daemon took too long to exit: {elapsed:?}"
        );

        // Verify the daemon lock is released.
        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        assert!(lock.try_lock().unwrap(), "daemon lock should be released");
    }
}
