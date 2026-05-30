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
    /// Returns [`SpawnOutcome::Spawned`] on successful launch.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition and process-spawn errors.
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

        // Check whether the daemon lock is already held (another daemon is running).
        // Release it before spawning so the child can acquire it.
        {
            let daemon_lock_path = sift_dir.join(DAEMON_LOCK);
            let mut daemon_lock = LockFile::open(&daemon_lock_path)?;
            if !daemon_lock.try_lock()? {
                return Ok(SpawnOutcome::AlreadyRunning);
            }
        }

        let request = SpawnRequest {
            exe,
            sift_dir: sift_dir.clone(),
            init_root: config.init_root.clone(),
        };
        self.spawner.spawn(&request)?;

        Ok(SpawnOutcome::Spawned)
    }
}

// ---------------------------------------------------------------------------
// Coordinator event types
// ---------------------------------------------------------------------------

/// Events flowing through the coordinator channel.
enum CoordinatorEvent {
    Fs(notify::Event),
    RefreshComplete,
}

/// State of the coordinator loop.
enum CoordinatorState {
    Idle,
    Debouncing { deadline: Instant },
    Refreshing { need_another: bool },
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

        let build_config = IndexConfig {
            corpus: CorpusSpec {
                root: &root,
                kind: corpus_kind,
                follow_links,
                include_paths: &[],
                exclude_paths: &[],
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

    fn build_initial_snapshot(
        sift_dir: &Path,
        root: &Path,
        corpus_kind: sift_core::CorpusKind,
        follow_links: bool,
        kinds: &[IndexKind],
    ) -> anyhow::Result<()> {
        let mut store =
            IndexStore::open_or_create(sift_dir, root, corpus_kind, follow_links, kinds)?;
        let exclude = sift_dir
            .strip_prefix(root)
            .unwrap_or(sift_dir)
            .to_path_buf();
        let build_config = IndexConfig {
            corpus: CorpusSpec {
                root,
                kind: corpus_kind,
                follow_links,
                include_paths: &[],
                exclude_paths: &[exclude],
            },
            visibility: VisibilityConfig::default(),
        };
        store.build(kinds, &build_config)?;
        Ok(())
    }

    /// Run the daemon forever (production use).
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, watcher, and build errors.
    pub fn run(&self) -> anyhow::Result<()> {
        self.run_until(&AtomicBool::new(false))
    }

    /// Run the daemon until `shutdown` becomes `true`.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, watcher, and build errors.
    pub fn run_until(&self, shutdown: &AtomicBool) -> anyhow::Result<()> {
        let sift_dir = &self.config.sift_dir;
        std::fs::create_dir_all(sift_dir)?;

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let DaemonMeta {
            root,
            corpus_kind,
            follow_links,
            kinds,
        } = Self::load_daemon_meta(sift_dir, self.config.init_root.as_ref())?;

        // Build initial snapshot if none exists.
        if !sift_dir.join("CURRENT").exists() {
            Self::build_initial_snapshot(sift_dir, &root, corpus_kind, follow_links, &kinds)?;
        }

        // Unified coordinator channel: receives both watcher events and
        // refresh-completion signals.
        let (tx, rx) = mpsc::channel::<CoordinatorEvent>();

        // Start the filesystem watcher.
        let watcher_tx = tx.clone();
        let watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = watcher_tx.send(CoordinatorEvent::Fs(event));
                }
            })?;
        let mut watcher = watcher;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let mut state = CoordinatorState::Idle;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }

            let timeout = match &state {
                CoordinatorState::Debouncing { deadline } => {
                    deadline.saturating_duration_since(Instant::now())
                }
                CoordinatorState::Idle | CoordinatorState::Refreshing { .. } => debounce,
            };

            match rx.recv_timeout(timeout) {
                Ok(CoordinatorEvent::Fs(event)) => {
                    state = Self::observe(state, &event, sift_dir, debounce);
                }
                Ok(CoordinatorEvent::RefreshComplete) => match state {
                    CoordinatorState::Refreshing { need_another: true } => {
                        Self::spawn_refresh(
                            tx.clone(),
                            sift_dir,
                            &kinds,
                            &root,
                            corpus_kind,
                            follow_links,
                        );
                        state = CoordinatorState::Refreshing {
                            need_another: false,
                        };
                    }
                    _ => {
                        state = CoordinatorState::Idle;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    match state {
                        CoordinatorState::Debouncing { .. } => {
                            Self::spawn_refresh(
                                tx.clone(),
                                sift_dir,
                                &kinds,
                                &root,
                                corpus_kind,
                                follow_links,
                            );
                            state = CoordinatorState::Refreshing {
                                need_another: false,
                            };
                        }
                        CoordinatorState::Idle | CoordinatorState::Refreshing { .. } => {
                            // Nothing to do.
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }

    /// Process an incoming filesystem event and transition state.
    fn observe(
        state: CoordinatorState,
        event: &notify::Event,
        sift_dir: &Path,
        debounce: Duration,
    ) -> CoordinatorState {
        // Filter out access events and .sift directory events.
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return state;
        }
        if event.paths.iter().any(|p| p.starts_with(sift_dir)) {
            return state;
        }

        match state {
            CoordinatorState::Refreshing { .. } => {
                CoordinatorState::Refreshing { need_another: true }
            }
            _ => CoordinatorState::Debouncing {
                deadline: Instant::now() + debounce,
            },
        }
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
        };
        let runner = DaemonRunner::new(config);
        runner.run_until(&AtomicBool::new(false)).unwrap();
    }
}
