use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use fslock::LockFile;
use notify::RecursiveMode;
use sift_core::{
    CorpusKind, CorpusMeta, DaemonOp, FilterMeta, IndexKind, IndexStore, StoreMeta, WalkMeta,
};

use crate::grep::paths::CorpusScope;

use super::coalesce::IndexCoalesce;
use super::coordinator::{CoordinatorAction, CoordinatorInput, CoordinatorState};
use super::error::DaemonError;
use super::watch::{DAEMON_LOCK, DEFAULT_IDLE_TIMEOUT, FileWatcher, SHUTDOWN_POLL};
use super::{Daemon, Serve};

enum ServeMessage {
    FsChange(notify::Event),
    RefreshComplete,
    Client(DaemonOp),
}

struct ServeRuntime {
    _lock: LockFile,
    _watcher: FileWatcher,
    sift_dir: PathBuf,
    sift_dir_raw: PathBuf,
    meta: StoreMeta,
    coalesce: Arc<Mutex<IndexCoalesce>>,
    tx: mpsc::Sender<ServeMessage>,
    rx: mpsc::Receiver<ServeMessage>,
}

impl ServeRuntime {
    fn run(
        &self,
        idle_timeout: Duration,
        debounce: Duration,
        shutdown: &AtomicBool,
    ) -> Result<(), DaemonError> {
        let mut state = CoordinatorState::new_idle(idle_timeout);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                self.drain_active_refresh(&mut state);
                return Ok(());
            }

            let timeout = state.deadline().map_or(SHUTDOWN_POLL, |d| {
                d.saturating_duration_since(Instant::now())
                    .min(SHUTDOWN_POLL)
            });

            let input = match self.rx.recv_timeout(timeout) {
                Ok(ServeMessage::FsChange(event)) => {
                    let internal = matches!(event.kind, notify::EventKind::Access(_))
                        || event.paths.iter().any(|p| {
                            p.starts_with(&self.sift_dir) || p.starts_with(&self.sift_dir_raw)
                        });
                    if internal {
                        continue;
                    }
                    CoordinatorInput::FsChange
                }
                Ok(ServeMessage::RefreshComplete) => CoordinatorInput::RefreshComplete,
                Ok(ServeMessage::Client(DaemonOp::Watch)) => continue,
                Ok(ServeMessage::Client(DaemonOp::Index(paths))) => {
                    {
                        let mut pending = self.coalesce.lock().expect("coalesce lock");
                        pending.push(paths);
                    }
                    if state.is_refreshing() {
                        state = CoordinatorState::Refreshing { follow_up: true };
                    } else {
                        self.spawn_refresh(true);
                        state = CoordinatorState::Refreshing { follow_up: false };
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if state.deadline().is_some_and(|d| Instant::now() >= d) {
                        CoordinatorInput::DeadlineReached
                    } else {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };

            let idle_deadline = matches!(state, CoordinatorState::Idle { .. });
            let debouncing_deadline = matches!(state, CoordinatorState::Debouncing { .. });

            let (next, action) = state.transition(input, debounce, idle_timeout);

            if idle_deadline
                && matches!(input, CoordinatorInput::DeadlineReached)
                && self.coalesce.lock().expect("coalesce lock").is_pending()
            {
                // Drain queued IPC index work before exiting; idle timeout is not
                // a hard guarantee while coalesce still has pending paths.
                self.spawn_refresh(true);
                state = CoordinatorState::Refreshing { follow_up: false };
                continue;
            }

            match action {
                CoordinatorAction::None => {}
                CoordinatorAction::StartRefresh => {
                    self.spawn_refresh(false);
                    state = CoordinatorState::Refreshing { follow_up: false };
                    continue;
                }
                CoordinatorAction::Exit => return Ok(()),
            }
            if debouncing_deadline && matches!(input, CoordinatorInput::DeadlineReached) {
                self.coalesce
                    .lock()
                    .expect("coalesce lock")
                    .reconcile(&self.sift_dir, &self.meta);
            }
            state = next;
        }
    }

    fn spawn_refresh(&self, index_only: bool) {
        let tx = self.tx.clone();
        let sift_dir = self.sift_dir.clone();
        let meta = self.meta.clone();
        let coalesce = Arc::clone(&self.coalesce);
        std::thread::spawn(move || {
            if !index_only {
                let result = IndexStore::open_or_create(&sift_dir, &meta)
                    .and_then(|mut store| store.reconcile(&meta, &[]));
                if let Err(e) = result {
                    eprintln!("sift-daemon: refresh failed: {e}");
                }
            }
            coalesce
                .lock()
                .expect("coalesce lock")
                .reconcile(&sift_dir, &meta);
            let _ = tx.send(ServeMessage::RefreshComplete);
        });
    }

    fn drain_active_refresh(&self, state: &mut CoordinatorState) {
        while state.is_refreshing() {
            match self.rx.recv_timeout(Duration::from_secs(1)) {
                Ok(ServeMessage::RefreshComplete) => {
                    *state = CoordinatorState::new_idle(DEFAULT_IDLE_TIMEOUT);
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

impl Daemon {
    /// Run the daemon event loop until idle timeout or shutdown.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, watcher, and IPC setup errors.
    pub fn serve(&self, options: Serve) -> Result<(), DaemonError> {
        let shutdown = options
            .shutdown
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let Some(runtime) = self.start_runtime(options.ready_file)? else {
            return Ok(());
        };
        runtime.run(
            options.idle_timeout,
            Duration::from_millis(super::DEBOUNCE_MS),
            &shutdown,
        )
    }

    fn start_runtime(
        &self,
        ready_file: Option<PathBuf>,
    ) -> Result<Option<ServeRuntime>, DaemonError> {
        std::fs::create_dir_all(&self.sift_dir)?;

        let sift_dir_raw = self.sift_dir.clone();
        let sift_dir = sift_dir_raw
            .canonicalize()
            .unwrap_or_else(|_| sift_dir_raw.clone());

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(None);
        }

        let meta = self.load_meta(&sift_dir)?;
        let result = IndexStore::open_or_create(&sift_dir, &meta)
            .and_then(|mut store| store.reconcile(&meta, &[]));
        if let Err(e) = result {
            eprintln!("sift-daemon: refresh failed: {e}");
        }

        let coalesce = Arc::new(Mutex::new(IndexCoalesce::default()));
        let (tx, rx) = mpsc::channel::<ServeMessage>();

        let ipc_tx = tx.clone();
        let ipc_daemon = Self {
            sift_dir: sift_dir.clone(),
            init_root: None,
        };
        std::thread::spawn(move || {
            if let Err(e) =
                ipc_daemon.listen(move |op| ipc_tx.send(ServeMessage::Client(op)).is_ok())
            {
                eprintln!("sift-daemon: ipc listener stopped: {e}");
            }
        });

        if let Some(ready) = ready_file {
            let tmp = ready.with_extension("tmp");
            std::fs::write(&tmp, "")?;
            std::fs::rename(&tmp, &ready)?;
        }

        let watcher_tx = tx.clone();
        let mut watcher = FileWatcher::new(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = watcher_tx.send(ServeMessage::FsChange(event));
            }
        })
        .map_err(|e| DaemonError::message(e.to_string()))?;
        watcher
            .watch(&meta.corpus.root, RecursiveMode::Recursive)
            .map_err(|e| DaemonError::message(e.to_string()))?;

        Ok(Some(ServeRuntime {
            _lock: lock_file,
            _watcher: watcher,
            sift_dir,
            sift_dir_raw,
            meta,
            coalesce,
            tx,
            rx,
        }))
    }

    fn load_meta(&self, sift_dir: &Path) -> Result<StoreMeta, DaemonError> {
        match (StoreMeta::read(sift_dir), self.init_root.as_deref()) {
            (Ok(mut meta), _) => {
                if meta.indexes.is_empty() {
                    meta.indexes = IndexKind::ALL.to_vec();
                }
                Ok(meta)
            }
            (Err(_), Some(init_root)) => {
                let root = init_root.canonicalize()?;
                Ok(StoreMeta::new(
                    CorpusMeta {
                        root: root.clone(),
                        kind: CorpusKind::Directory,
                        include_paths: Vec::new(),
                        exclude_paths: CorpusScope::excluded_paths(&root, sift_dir),
                    },
                    WalkMeta {
                        follow_links: false,
                        one_file_system: false,
                        max_depth: None,
                        max_filesize: None,
                    },
                    FilterMeta {
                        visibility: sift_core::VisibilityConfig::default(),
                    },
                    IndexKind::ALL.to_vec(),
                ))
            }
            (Err(e), None) => Err(DaemonError::message(format!("no store metadata: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fslock::LockFile;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    #[test]
    fn serve_stops_on_shutdown() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");

        let meta = StoreMeta::new(
            CorpusMeta {
                root: dir.path().to_path_buf(),
                kind: CorpusKind::Directory,
                include_paths: Vec::new(),
                exclude_paths: Vec::new(),
            },
            WalkMeta {
                follow_links: false,
                one_file_system: false,
                max_depth: None,
                max_filesize: None,
            },
            FilterMeta {
                visibility: sift_core::VisibilityConfig::default(),
            },
            vec![IndexKind::Trigram],
        );
        std::fs::create_dir_all(&sift_dir).unwrap();
        StoreMeta::write(&meta, &sift_dir).unwrap();

        let daemon = Daemon::new(sift_dir);
        let shutdown = Arc::new(AtomicBool::new(false));
        let s = Arc::clone(&shutdown);

        let handle = std::thread::spawn(move || {
            daemon
                .serve(Serve {
                    ready_file: None,
                    idle_timeout: DEFAULT_IDLE_TIMEOUT,
                    shutdown: Some(s),
                })
                .unwrap();
        });

        std::thread::sleep(Duration::from_millis(100));
        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn serve_returns_early_when_lock_held() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).unwrap();
        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock = LockFile::open(&lock_path).unwrap();
        lock.try_lock().unwrap();

        let daemon = Daemon::new(sift_dir);
        daemon
            .serve(Serve {
                ready_file: None,
                idle_timeout: DEFAULT_IDLE_TIMEOUT,
                shutdown: None,
            })
            .unwrap();
    }
}
