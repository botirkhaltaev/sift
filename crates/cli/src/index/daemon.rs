//! Background index refresh daemon: IPC, spawn, and serve loop.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use fslock::LockFile;
use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use notify::RecursiveMode;
use notify::Watcher;
use sift_core::{CorpusKind, CorpusMeta, FilterMeta, IndexKind, IndexStore, StoreMeta, WalkMeta};
use thiserror::Error;

use crate::grep::paths::CorpusScope;

const DEBOUNCE_MS: u64 = 250;
const DAEMON_LOCK: &str = "lock";
const READY_DIR: &str = "daemon-ready";
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL: Duration = Duration::from_secs(1);
const SPAWN_LOCK: &str = "daemon-spawn.lock";

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("daemon error: {0}")]
    Message(String),
}

impl DaemonError {
    fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl From<anyhow::Error> for DaemonError {
    fn from(value: anyhow::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<sift_core::Error> for DaemonError {
    fn from(value: sift_core::Error) -> Self {
        Self::Message(value.to_string())
    }
}

/// IPC operation sent to the index daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonOp {
    /// Rel-paths to index. Empty vec = full corpus.
    Index(Vec<PathBuf>),
}

impl DaemonOp {
    const INDEX_OPCODE: u8 = 0x02;
    const STATUS_OK: u8 = 0x00;
    const STATUS_ERR: u8 = 0x01;

    /// Encode this operation for IPC.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    pub fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        match self {
            Self::Index(paths) => {
                writer.write_all(&[Self::INDEX_OPCODE])?;
                for path in paths {
                    let line = path.to_string_lossy();
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                }
                writer.write_all(b"\n")?;
            }
        }
        writer.flush()
    }

    /// Decode a daemon operation from IPC.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload is malformed.
    pub fn decode(mut reader: impl Read) -> io::Result<Self> {
        let mut opcode = [0_u8; 1];
        reader.read_exact(&mut opcode)?;
        match opcode[0] {
            Self::INDEX_OPCODE => {
                let mut paths = Vec::new();
                loop {
                    let mut buf = Vec::new();
                    loop {
                        let mut byte = [0_u8; 1];
                        let n = reader.read(&mut byte)?;
                        if n == 0 || byte[0] == b'\n' {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                    if buf.is_empty() {
                        break;
                    }
                    let line = String::from_utf8(buf).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "index path is not valid utf-8")
                    })?;
                    paths.push(PathBuf::from(line));
                }
                Ok(Self::Index(paths))
            }
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown daemon opcode: {other}"),
            )),
        }
    }
}

/// Handle to the index daemon for a `.sift` store directory.
#[derive(Debug, Clone)]
pub struct Daemon {
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
}

impl Daemon {
    #[must_use]
    pub const fn new(sift_dir: PathBuf) -> Self {
        Self {
            sift_dir,
            init_root: None,
        }
    }

    /// Send an IPC operation to the daemon, spawning it when needed.
    ///
    /// # Errors
    ///
    /// Propagates spawn and IPC failures.
    pub fn send(&self, op: &DaemonOp) -> Result<(), DaemonError> {
        self.ensure_daemon_running()?;
        let mut stream = Stream::connect(self.ipc_name()?).map_err(DaemonError::Io)?;
        op.encode(&mut stream)?;
        let mut status = [0_u8; 1];
        stream.read_exact(&mut status).map_err(DaemonError::Io)?;
        if status[0] == DaemonOp::STATUS_OK {
            Ok(())
        } else {
            Err(DaemonError::message("daemon rejected request"))
        }
    }

    /// Ensure the background daemon is running for this store.
    ///
    /// # Errors
    ///
    /// Propagates spawn and readiness failures.
    pub fn ensure_running(&self) -> Result<(), DaemonError> {
        self.ensure_daemon_running()
    }

    /// Run the daemon event loop until idle timeout or shutdown.
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, watcher, and IPC setup errors.
    pub fn serve(
        &self,
        ready_file: Option<PathBuf>,
        idle_timeout: Duration,
        shutdown: &AtomicBool,
    ) -> Result<(), DaemonError> {
        std::fs::create_dir_all(&self.sift_dir)?;

        let sift_dir_raw = self.sift_dir.clone();
        let sift_dir = sift_dir_raw
            .canonicalize()
            .unwrap_or_else(|_| sift_dir_raw.clone());

        let lock_path = sift_dir.join(DAEMON_LOCK);
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let meta = self.load_meta(&sift_dir)?;
        let result = IndexStore::open_or_create(&sift_dir, &meta)
            .and_then(|mut store| store.reconcile(&meta, &[]));
        if let Err(e) = result {
            eprintln!("sift-daemon: refresh failed: {e}");
        }

        let pending = Arc::new(Mutex::new(PendingIndex::None));
        let (tx, rx) = mpsc::channel::<Event>();

        let ipc_tx = tx.clone();
        let ipc_daemon = Self {
            sift_dir: sift_dir.clone(),
            init_root: None,
        };
        std::thread::spawn(move || {
            if let Err(e) = ipc_daemon.listen(move |op| ipc_tx.send(Event::Client(op)).is_ok()) {
                eprintln!("sift-daemon: ipc listener stopped: {e}");
            }
        });

        if let Some(ready) = ready_file {
            let tmp = ready.with_extension("tmp");
            std::fs::write(&tmp, "")?;
            std::fs::rename(&tmp, &ready)?;
        }

        let watcher_tx = tx.clone();
        #[cfg(windows)]
        let watcher_config =
            notify::Config::default().with_poll_interval(Duration::from_millis(DEBOUNCE_MS));
        #[cfg(not(windows))]
        let watcher_config = notify::Config::default();
        let mut watcher = PlatformWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = watcher_tx.send(Event::FsChange(event));
                }
            },
            watcher_config,
        )
        .map_err(|e| DaemonError::message(e.to_string()))?;
        watcher
            .watch(&meta.corpus.root, RecursiveMode::Recursive)
            .map_err(|e| DaemonError::message(e.to_string()))?;

        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let mut phase = Phase::idle(idle_timeout);
        ServeLoop {
            _lock: lock_file,
            _watcher: watcher,
            rx: &rx,
            tx: &tx,
            sift_dir: &sift_dir,
            sift_dir_raw: &sift_dir_raw,
            pending: &pending,
            idle_timeout,
            shutdown,
            phase: &mut phase,
            debounce,
        }
        .run()
    }

    fn daemon_reachable(&self) -> Result<bool, DaemonError> {
        let name = self.ipc_name()?;
        Ok(Stream::connect(name).is_ok())
    }

    fn wait_for_daemon(&self, deadline: Instant) -> Result<(), DaemonError> {
        loop {
            if self.daemon_reachable()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(DaemonError::message(format!(
                    "daemon did not become reachable within {READY_TIMEOUT:?}"
                )));
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }
    }

    fn ipc_name(&self) -> Result<interprocess::local_socket::Name<'static>, DaemonError> {
        let canonical = self
            .sift_dir
            .canonicalize()
            .unwrap_or_else(|_| self.sift_dir.clone());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        format!("sift-{:016x}", hasher.finish())
            .to_ns_name::<GenericNamespaced>()
            .map_err(DaemonError::Io)
    }

    fn listen(
        &self,
        mut handler: impl FnMut(DaemonOp) -> bool + Send + 'static,
    ) -> Result<(), DaemonError> {
        let listener = ListenerOptions::new()
            .name(self.ipc_name()?)
            .try_overwrite(true)
            .create_sync()
            .map_err(DaemonError::Io)?;
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let op = match DaemonOp::decode(&mut stream) {
                Ok(op) => op,
                Err(e) => {
                    let _ = stream.write_all(&[DaemonOp::STATUS_ERR]);
                    eprintln!("sift-daemon: ipc decode failed: {e}");
                    continue;
                }
            };
            let status = if handler(op) {
                DaemonOp::STATUS_OK
            } else {
                DaemonOp::STATUS_ERR
            };
            let _ = stream.write_all(&[status]);
        }
        Ok(())
    }

    fn ensure_daemon_running(&self) -> Result<(), DaemonError> {
        if self.daemon_reachable()? {
            return Ok(());
        }

        let sift_dir = &self.sift_dir;
        let init_root = self.init_root.as_deref();
        let exe = {
            if let Some(path) = std::env::var_os("CARGO_BIN_EXE_sift-daemon") {
                PathBuf::from(path)
            } else {
                let sift = std::env::current_exe().map_err(DaemonError::Io)?;
                let sibling = sift.with_file_name("sift-daemon");
                if sibling.exists() {
                    sibling
                } else if let Some(debug_bin) = sift
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("sift-daemon"))
                    .filter(|p| p.exists())
                {
                    debug_bin
                } else {
                    sibling
                }
            }
        };

        std::fs::create_dir_all(sift_dir)?;

        let spawn_lock_path = sift_dir.join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path)?;
        if !spawn_lock.try_lock()? {
            return self.wait_for_daemon(Instant::now() + READY_TIMEOUT);
        }

        {
            let daemon_lock_path = sift_dir.join(DAEMON_LOCK);
            let mut daemon_lock = LockFile::open(&daemon_lock_path)?;
            if !daemon_lock.try_lock()? {
                return self.wait_for_daemon(Instant::now() + READY_TIMEOUT);
            }
        }

        if self.daemon_reachable()? {
            return Ok(());
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

        let log_path = sift_dir.join("daemon-spawn.log");
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(DaemonError::Io)?;

        let mut cmd = Command::new(&exe);
        cmd.arg("--sift-dir")
            .arg(sift_dir)
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
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
            if self.daemon_reachable()? {
                let _ = std::fs::remove_file(&ready_path);
                return Ok(());
            }
            if Instant::now() >= deadline {
                let log_tail = std::fs::read_to_string(&log_path)
                    .ok()
                    .map(|s| {
                        s.lines()
                            .rev()
                            .take(5)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|s| !s.is_empty());
                let detail = log_tail
                    .map(|tail| format!("\nlast daemon log lines:\n{tail}"))
                    .unwrap_or_default();
                return Err(DaemonError::message(format!(
                    "daemon did not signal readiness within {READY_TIMEOUT:?}{detail}"
                )));
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }
    }

    fn load_meta(&self, sift_dir: &Path) -> Result<StoreMeta, DaemonError> {
        match (read_store_meta(sift_dir), self.init_root.as_deref()) {
            (Ok(meta), _) => Ok(meta),
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
            (Err(e), None) => Err(e),
        }
    }
}

enum Event {
    FsChange(notify::Event),
    RefreshComplete,
    Client(DaemonOp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseInput {
    FsChange,
    RefreshComplete,
    DeadlineReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopAction {
    None,
    StartRefresh,
    Exit,
}

enum Phase {
    Idle { deadline: Instant },
    Debouncing { deadline: Instant },
    Refreshing { follow_up: bool },
}

impl Phase {
    fn idle(idle_timeout: Duration) -> Self {
        Self::Idle {
            deadline: Instant::now() + idle_timeout,
        }
    }

    const fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle { deadline } | Self::Debouncing { deadline } => Some(*deadline),
            Self::Refreshing { .. } => None,
        }
    }

    const fn is_refreshing(&self) -> bool {
        matches!(self, Self::Refreshing { .. })
    }

    fn advance(
        &mut self,
        input: PhaseInput,
        debounce: Duration,
        idle_timeout: Duration,
    ) -> LoopAction {
        let (next, action) = match (std::mem::replace(self, Self::idle(idle_timeout)), input) {
            (Self::Idle { .. } | Self::Debouncing { .. }, PhaseInput::FsChange) => (
                Self::Debouncing {
                    deadline: Instant::now() + debounce,
                },
                LoopAction::None,
            ),
            (Self::Debouncing { .. }, PhaseInput::DeadlineReached)
            | (Self::Refreshing { follow_up: true }, PhaseInput::RefreshComplete) => (
                Self::Refreshing { follow_up: false },
                LoopAction::StartRefresh,
            ),
            (Self::Idle { .. }, PhaseInput::DeadlineReached) => {
                (Self::idle(idle_timeout), LoopAction::Exit)
            }
            (Self::Refreshing { .. }, PhaseInput::FsChange) => {
                (Self::Refreshing { follow_up: true }, LoopAction::None)
            }
            (
                Self::Refreshing {
                    follow_up: false, ..
                },
                PhaseInput::RefreshComplete,
            ) => (Self::idle(idle_timeout), LoopAction::None),
            (state, _) => (state, LoopAction::None),
        };
        *self = next;
        action
    }
}

enum PendingIndex {
    None,
    Full,
    Paths(Vec<PathBuf>),
}

impl PendingIndex {
    fn push(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            *self = Self::Full;
            return;
        }
        match self {
            Self::Full => {}
            Self::None => *self = Self::Paths(paths),
            Self::Paths(existing) => {
                for path in paths {
                    if !existing.contains(&path) {
                        existing.push(path);
                    }
                }
            }
        }
    }

    const fn is_pending(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn take(&mut self) -> Option<Vec<PathBuf>> {
        match std::mem::replace(self, Self::None) {
            Self::None => None,
            Self::Full => Some(Vec::new()),
            Self::Paths(paths) => Some(paths),
        }
    }

    fn reconcile(&mut self, sift_dir: &Path, meta: &StoreMeta) {
        let Some(paths) = self.take() else {
            return;
        };
        let result = IndexStore::open_or_create(sift_dir, meta)
            .and_then(|mut store| store.reconcile(meta, &paths));
        if let Err(e) = result {
            eprintln!("sift-daemon: refresh failed: {e}");
            self.push(paths);
        }
    }
}

fn read_store_meta(sift_dir: &Path) -> Result<StoreMeta, DaemonError> {
    let mut meta = StoreMeta::read(sift_dir)
        .map_err(|e| DaemonError::message(format!("no store metadata: {e}")))?;
    if meta.indexes.is_empty() {
        meta.indexes = IndexKind::ALL.to_vec();
    }
    Ok(meta)
}

enum RefreshScope {
    CorpusAndPending,
    PendingOnly,
}

fn pending_lock(
    pending: &Mutex<PendingIndex>,
) -> Result<std::sync::MutexGuard<'_, PendingIndex>, DaemonError> {
    pending
        .lock()
        .map_err(|_| DaemonError::message("daemon pending queue lock poisoned"))
}

struct ServeLoop<'a> {
    _lock: LockFile,
    _watcher: PlatformWatcher,
    rx: &'a mpsc::Receiver<Event>,
    tx: &'a mpsc::Sender<Event>,
    sift_dir: &'a Path,
    sift_dir_raw: &'a Path,
    pending: &'a Arc<Mutex<PendingIndex>>,
    idle_timeout: Duration,
    shutdown: &'a AtomicBool,
    phase: &'a mut Phase,
    debounce: Duration,
}

impl ServeLoop<'_> {
    fn run(self) -> Result<(), DaemonError> {
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                drain_refresh(self.rx, self.phase, self.idle_timeout);
                return Ok(());
            }

            let timeout = self.phase.deadline().map_or(SHUTDOWN_POLL, |d| {
                d.saturating_duration_since(Instant::now())
                    .min(SHUTDOWN_POLL)
            });

            let input = match self.rx.recv_timeout(timeout) {
                Ok(Event::FsChange(event)) => {
                    let internal = matches!(event.kind, notify::EventKind::Access(_))
                        || event.paths.iter().any(|p| {
                            p.starts_with(self.sift_dir) || p.starts_with(self.sift_dir_raw)
                        });
                    if internal {
                        continue;
                    }
                    Some(PhaseInput::FsChange)
                }
                Ok(Event::RefreshComplete) => Some(PhaseInput::RefreshComplete),
                Ok(Event::Client(DaemonOp::Index(paths))) => {
                    pending_lock(self.pending)?.push(paths);
                    if self.phase.is_refreshing() {
                        *self.phase = Phase::Refreshing { follow_up: true };
                    } else {
                        spawn_refresh(
                            RefreshScope::CorpusAndPending,
                            self.tx,
                            self.sift_dir,
                            Arc::clone(self.pending),
                        );
                        *self.phase = Phase::Refreshing { follow_up: false };
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.phase.deadline().is_some_and(|d| Instant::now() >= d) {
                        Some(PhaseInput::DeadlineReached)
                    } else {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };

            let idle_deadline = matches!(self.phase, Phase::Idle { .. });

            if idle_deadline
                && matches!(input, Some(PhaseInput::DeadlineReached))
                && pending_lock(self.pending)?.is_pending()
            {
                spawn_refresh(
                    RefreshScope::PendingOnly,
                    self.tx,
                    self.sift_dir,
                    Arc::clone(self.pending),
                );
                *self.phase = Phase::Refreshing { follow_up: false };
                continue;
            }

            let Some(input) = input else {
                continue;
            };

            let action = self.phase.advance(input, self.debounce, self.idle_timeout);

            match action {
                LoopAction::None => {}
                LoopAction::StartRefresh => {
                    spawn_refresh(
                        RefreshScope::CorpusAndPending,
                        self.tx,
                        self.sift_dir,
                        Arc::clone(self.pending),
                    );
                    *self.phase = Phase::Refreshing { follow_up: false };
                }
                LoopAction::Exit => return Ok(()),
            }
        }
    }
}

#[cfg(windows)]
type PlatformWatcher = notify::PollWatcher;

#[cfg(not(windows))]
type PlatformWatcher = notify::RecommendedWatcher;

fn spawn_refresh(
    scope: RefreshScope,
    tx: &mpsc::Sender<Event>,
    sift_dir: &Path,
    pending: Arc<Mutex<PendingIndex>>,
) {
    let tx = tx.clone();
    let sift_dir = sift_dir.to_path_buf();
    std::thread::spawn(move || {
        let meta = match read_store_meta(&sift_dir) {
            Ok(meta) => meta,
            Err(e) => {
                eprintln!("sift-daemon: {e}");
                let _ = tx.send(Event::RefreshComplete);
                return;
            }
        };
        if matches!(scope, RefreshScope::CorpusAndPending) {
            let result = IndexStore::open_or_create(&sift_dir, &meta)
                .and_then(|mut store| store.reconcile(&meta, &[]));
            if let Err(e) = result {
                eprintln!("sift-daemon: refresh failed: {e}");
            }
        }
        match pending.lock() {
            Ok(mut queue) => queue.reconcile(&sift_dir, &meta),
            Err(_) => eprintln!("sift-daemon: pending queue lock poisoned"),
        }
        let _ = tx.send(Event::RefreshComplete);
    });
}

fn drain_refresh(rx: &mpsc::Receiver<Event>, phase: &mut Phase, idle_timeout: Duration) {
    while phase.is_refreshing() {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Event::RefreshComplete) => *phase = Phase::idle(idle_timeout),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fslock::LockFile;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    const DEBOUNCE: Duration = Duration::from_mins(1);
    const IDLE: Duration = Duration::from_mins(10);

    #[test]
    fn round_trip_index_paths() {
        let paths = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let mut buf = Vec::new();
        DaemonOp::Index(paths.clone()).encode(&mut buf).unwrap();
        let op = DaemonOp::decode(buf.as_slice()).unwrap();
        assert_eq!(op, DaemonOp::Index(paths));
    }

    #[test]
    fn round_trip_index_full() {
        let mut buf = Vec::new();
        DaemonOp::Index(Vec::new()).encode(&mut buf).unwrap();
        let op = DaemonOp::decode(buf.as_slice()).unwrap();
        assert_eq!(op, DaemonOp::Index(Vec::new()));
    }

    #[cfg(unix)]
    #[test]
    fn index_round_trip_over_unix_stream() {
        use std::os::unix::net::UnixStream;
        use std::thread;

        let (mut client, server) = UnixStream::pair().unwrap();
        let paths = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let expected = DaemonOp::Index(paths.clone());

        let handle = thread::spawn(move || {
            let mut server = server;
            let op = DaemonOp::decode(&mut server).unwrap();
            assert_eq!(op, DaemonOp::Index(paths));
            server.write_all(&[DaemonOp::STATUS_OK]).unwrap();
        });

        expected.encode(&mut client).unwrap();
        let mut status = [0_u8; 1];
        client.read_exact(&mut status).unwrap();
        assert_eq!(status[0], DaemonOp::STATUS_OK);
        handle.join().unwrap();
    }

    #[test]
    fn pending_index_merge_partial_paths() {
        let mut pending = PendingIndex::None;
        pending.push(vec![PathBuf::from("a.rs")]);
        pending.push(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        assert_eq!(
            pending.take(),
            Some(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")])
        );
    }

    #[test]
    fn pending_index_full_promotes_and_clears_partials() {
        let mut pending = PendingIndex::None;
        pending.push(vec![PathBuf::from("a.rs")]);
        pending.push(Vec::new());
        assert_eq!(pending.take(), Some(Vec::new()));
        assert!(!pending.is_pending());
    }

    #[test]
    fn phase_idle_fs_change_transitions_to_debouncing() {
        let mut phase = Phase::idle(IDLE);
        let action = phase.advance(PhaseInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(phase, Phase::Debouncing { .. }));
        assert_eq!(action, LoopAction::None);
    }

    #[test]
    fn phase_idle_deadline_reached_exits() {
        let mut phase = Phase::idle(IDLE);
        let action = phase.advance(PhaseInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(phase, Phase::Idle { .. }));
        assert_eq!(action, LoopAction::Exit);
    }

    #[test]
    fn phase_debouncing_deadline_reached_starts_refresh() {
        let mut phase = Phase::Debouncing {
            deadline: Instant::now(),
        };
        let action = phase.advance(PhaseInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(phase, Phase::Refreshing { follow_up: false }));
        assert_eq!(action, LoopAction::StartRefresh);
    }

    #[test]
    fn phase_refreshing_complete_with_follow_up_restarts_refresh() {
        let mut phase = Phase::Refreshing { follow_up: true };
        let action = phase.advance(PhaseInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(phase, Phase::Refreshing { follow_up: false }));
        assert_eq!(action, LoopAction::StartRefresh);
    }

    #[test]
    fn ensure_running_skips_spawn_when_ipc_reachable() {
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

        let daemon = Daemon::new(sift_dir.clone());
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = std::thread::spawn({
            let shutdown = Arc::clone(&shutdown);
            move || {
                daemon
                    .serve(None, Duration::from_mins(2), shutdown.as_ref())
                    .unwrap();
            }
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        while !Daemon::new(sift_dir.clone()).daemon_reachable().unwrap() {
            assert!(
                Instant::now() < deadline,
                "daemon ipc did not become reachable"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        Daemon::new(sift_dir).ensure_running().unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn pending_paths_restored_on_reconcile_failure() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).unwrap();

        let mut pending = PendingIndex::Paths(vec![PathBuf::from("missing.rs")]);
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
        StoreMeta::write(&meta, &sift_dir).unwrap();

        pending.reconcile(&sift_dir, &meta);
        assert!(pending.is_pending());
    }

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

        let handle = std::thread::spawn({
            let shutdown = Arc::clone(&shutdown);
            move || {
                daemon
                    .serve(None, Duration::from_mins(2), shutdown.as_ref())
                    .unwrap();
            }
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
        let shutdown = AtomicBool::new(false);
        daemon
            .serve(None, Duration::from_mins(2), &shutdown)
            .unwrap();
    }
}
