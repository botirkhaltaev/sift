//! Background index refresh daemon: CLI spawn/IPC and server event loop.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

use fslock::LockFile;
use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use notify::RecursiveMode;
use notify::Watcher;
use sift_core::{CorpusKind, CorpusMeta, FilterMeta, IndexKind, IndexStore, StoreMeta, WalkMeta};
use thiserror::Error;

use crate::grep::paths::CorpusScope;

pub(crate) const DAEMON_LOCK: &str = "lock";
const READY_DIR: &str = "daemon-ready";
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const SPAWN_LOCK: &str = "daemon-spawn.lock";
const DEBOUNCE_MS: u64 = 250;
const SHUTDOWN_POLL: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("daemon error: {0}")]
    Message(String),
}

impl DaemonError {
    pub(crate) fn message(msg: impl Into<String>) -> Self {
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

/// Options for the background daemon server loop.
pub struct ServeConfig {
    /// Internal spawn handshake file, written once the watcher is active.
    pub ready: Option<PathBuf>,
    pub idle_timeout: Duration,
}

/// IPC operation sent to the index daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonOp {
    /// Rel-paths to index. Empty vec = full corpus.
    Index(Vec<PathBuf>),
}

impl DaemonOp {
    const INDEX_OPCODE: u8 = 0x02;
    pub(crate) const STATUS_OK: u8 = 0x00;
    pub(crate) const STATUS_ERR: u8 = 0x01;

    #[must_use]
    pub const fn index(paths: Vec<PathBuf>) -> Self {
        Self::Index(paths)
    }

    #[must_use]
    pub const fn full_corpus() -> Self {
        Self::Index(Vec::new())
    }

    pub(crate) fn into_index_paths(self) -> Vec<PathBuf> {
        match self {
            Self::Index(paths) => paths,
        }
    }

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
    pub(crate) sift_dir: PathBuf,
    pub(crate) init_root: Option<PathBuf>,
}

impl Daemon {
    const EXE: &'static str = "sift-daemon";
    #[cfg(windows)]
    const EXE_PLATFORM: &'static str = "sift-daemon.exe";

    /// Client handle for the sift CLI (search / index commands).
    #[must_use]
    pub const fn new(sift_dir: PathBuf) -> Self {
        Self {
            sift_dir,
            init_root: None,
        }
    }

    /// Server process (`sift-daemon` binary); optional corpus bootstrap when meta is missing.
    #[must_use]
    pub const fn bootstrap(sift_dir: PathBuf, init_root: Option<PathBuf>) -> Self {
        Self {
            sift_dir,
            init_root,
        }
    }

    /// Start the background daemon if IPC is not already reachable.
    ///
    /// # Errors
    ///
    /// Propagates spawn and readiness failures.
    pub fn ensure_running(&self) -> Result<(), DaemonError> {
        if self.reachable()? {
            return Ok(());
        }
        self.spawn()
    }

    /// Queue index work. Empty `paths` = full corpus reconcile.
    ///
    /// # Errors
    ///
    /// Propagates spawn and IPC failures.
    pub fn index(&self, paths: Vec<PathBuf>) -> Result<(), DaemonError> {
        self.invoke(&DaemonOp::index(paths))
    }

    /// Run the daemon event loop (called from `sift-daemon` and integration tests).
    ///
    /// # Errors
    ///
    /// Propagates lock-acquisition, metadata, watcher, and IPC setup errors.
    pub fn serve(&self, config: ServeConfig, shutdown: &AtomicBool) -> Result<(), DaemonError> {
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

        let meta = self.store_meta(&sift_dir)?;
        IndexStore::open_or_create(&sift_dir, &meta)
            .and_then(|mut store| store.reconcile(&meta, &[]))?;

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

        if let Some(ready) = config.ready {
            let tmp = ready.with_extension("tmp");
            std::fs::write(&tmp, "")?;
            std::fs::rename(&tmp, &ready)?;
        }

        let watcher_tx = tx.clone();
        let mut watcher = CorpusWatcher::new(&watcher_tx, &meta.corpus.root)?;

        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let idle_timeout = config.idle_timeout;
        let mut phase = Phase::idle(idle_timeout);

        let store = StorePaths {
            canonical: &sift_dir,
            raw: &sift_dir_raw,
        };
        let events = EventChannel { rx: &rx, tx: &tx };
        let refresh = IndexRefresh {
            tx: events.tx,
            store: &sift_dir,
            pending: &pending,
        };
        let _lock = lock_file;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                phase.drain_until_idle(&rx, idle_timeout);
                return Ok(());
            }

            let timeout = phase.deadline().map_or(SHUTDOWN_POLL, |d| {
                d.saturating_duration_since(Instant::now())
                    .min(SHUTDOWN_POLL)
            });

            let poll = events.poll(&store, &phase, timeout);
            match poll {
                ChannelPoll::Done => return Ok(()),
                ChannelPoll::Client(op) => {
                    refresh.apply_client(op, &mut watcher, store.canonical, &mut phase)?;
                }
                ChannelPoll::Input(input) => {
                    if matches!(phase, Phase::Idle { .. })
                        && matches!(input, PhaseInput::DeadlineReached)
                        && PendingIndex::lock(&pending)?.is_pending()
                    {
                        refresh.flush_pending(&mut phase);
                    } else {
                        let action = phase.advance(input, debounce, idle_timeout);
                        match action {
                            LoopAction::None => {}
                            LoopAction::StartRefresh => refresh.start(&mut phase),
                            LoopAction::Exit => return Ok(()),
                        }
                    }
                }
                ChannelPoll::Continue => {}
            }
        }
    }

    pub(crate) fn reachable(&self) -> Result<bool, DaemonError> {
        let name = self.ipc_name()?;
        Ok(Stream::connect(name).is_ok())
    }

    pub(crate) fn ipc_name(
        &self,
    ) -> Result<interprocess::local_socket::Name<'static>, DaemonError> {
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

    fn store_meta(&self, sift_dir: &Path) -> Result<StoreMeta, DaemonError> {
        match (StorePaths::read_meta(sift_dir), self.init_root.as_deref()) {
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

    fn invoke(&self, op: &DaemonOp) -> Result<(), DaemonError> {
        self.ensure_running()?;
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

    fn spawn(&self) -> Result<(), DaemonError> {
        let sift_dir = &self.sift_dir;
        let init_root = self.init_root.as_deref();
        let exe = Self::executable()?;

        std::fs::create_dir_all(sift_dir)?;

        let spawn_lock_path = sift_dir.join(SPAWN_LOCK);
        let mut spawn_lock = LockFile::open(&spawn_lock_path)?;
        if !spawn_lock.try_lock()? {
            return self.wait_until_reachable(Instant::now() + READY_TIMEOUT);
        }

        {
            let daemon_lock_path = sift_dir.join(DAEMON_LOCK);
            let mut daemon_lock = LockFile::open(&daemon_lock_path)?;
            if !daemon_lock.try_lock()? {
                return self.wait_until_reachable(Instant::now() + READY_TIMEOUT);
            }
        }

        if self.reachable()? {
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
            if self.reachable()? {
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

    fn wait_until_reachable(&self, deadline: Instant) -> Result<(), DaemonError> {
        loop {
            if self.reachable()? {
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

    fn executable() -> Result<PathBuf, DaemonError> {
        if let Some(path) = std::env::var_os("CARGO_BIN_EXE_sift-daemon") {
            return Ok(PathBuf::from(path));
        }

        let sift = std::env::current_exe().map_err(DaemonError::Io)?;
        let sibling = sift.with_file_name(Self::EXE);
        if sibling.exists() {
            return Ok(sibling);
        }
        #[cfg(windows)]
        {
            let sibling_exe = sift.with_file_name(Self::EXE_PLATFORM);
            if sibling_exe.exists() {
                return Ok(sibling_exe);
            }
        }

        if let Some(debug_bin) = sift
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(Self::EXE))
            .filter(|p| p.exists())
        {
            return Ok(debug_bin);
        }
        #[cfg(windows)]
        if let Some(debug_bin) = sift
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(Self::EXE_PLATFORM))
            .filter(|p| p.exists())
        {
            return Ok(debug_bin);
        }

        Ok(sibling)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshFollowUp {
    None,
    Queued,
}

enum Phase {
    Idle { deadline: Instant },
    Debouncing { deadline: Instant },
    Refreshing { follow_up: RefreshFollowUp },
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
            | (
                Self::Refreshing {
                    follow_up: RefreshFollowUp::Queued,
                },
                PhaseInput::RefreshComplete,
            ) => (
                Self::Refreshing {
                    follow_up: RefreshFollowUp::None,
                },
                LoopAction::StartRefresh,
            ),
            (Self::Idle { .. }, PhaseInput::DeadlineReached) => {
                (Self::idle(idle_timeout), LoopAction::Exit)
            }
            (Self::Refreshing { .. }, PhaseInput::FsChange) => (
                Self::Refreshing {
                    follow_up: RefreshFollowUp::Queued,
                },
                LoopAction::None,
            ),
            (
                Self::Refreshing {
                    follow_up: RefreshFollowUp::None,
                },
                PhaseInput::RefreshComplete,
            ) => (Self::idle(idle_timeout), LoopAction::None),
            (state, _) => (state, LoopAction::None),
        };
        *self = next;
        action
    }

    fn drain_until_idle(&mut self, rx: &mpsc::Receiver<Event>, idle_timeout: Duration) {
        while self.is_refreshing() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(Event::RefreshComplete) => *self = Self::idle(idle_timeout),
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

enum PendingIndex {
    None,
    Full,
    Paths(Vec<PathBuf>),
}

impl PendingIndex {
    fn lock(pending: &Arc<Mutex<Self>>) -> Result<MutexGuard<'_, Self>, DaemonError> {
        pending
            .lock()
            .map_err(|_| DaemonError::message("daemon pending queue lock poisoned"))
    }

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

enum RefreshScope {
    CorpusAndPending,
    PendingOnly,
}

struct StorePaths<'a> {
    canonical: &'a Path,
    raw: &'a Path,
}

impl StorePaths<'_> {
    fn read_meta(sift_dir: &Path) -> Result<StoreMeta, DaemonError> {
        let mut meta = StoreMeta::read(sift_dir)
            .map_err(|e| DaemonError::message(format!("no store metadata: {e}")))?;
        if meta.indexes.is_empty() {
            meta.indexes = IndexKind::ALL.to_vec();
        }
        Ok(meta)
    }

    fn is_internal(&self, path: &Path) -> bool {
        path.starts_with(self.canonical) || path.starts_with(self.raw)
    }
}

struct EventChannel<'a> {
    rx: &'a mpsc::Receiver<Event>,
    tx: &'a mpsc::Sender<Event>,
}

enum ChannelPoll {
    Continue,
    Done,
    Input(PhaseInput),
    Client(DaemonOp),
}

impl EventChannel<'_> {
    fn poll(&self, store: &StorePaths<'_>, phase: &Phase, timeout: Duration) -> ChannelPoll {
        match self.rx.recv_timeout(timeout) {
            Ok(Event::FsChange(event)) => {
                let internal = matches!(event.kind, notify::EventKind::Access(_))
                    || event.paths.iter().any(|path| store.is_internal(path));
                if internal {
                    ChannelPoll::Continue
                } else {
                    ChannelPoll::Input(PhaseInput::FsChange)
                }
            }
            Ok(Event::RefreshComplete) => ChannelPoll::Input(PhaseInput::RefreshComplete),
            Ok(Event::Client(op)) => ChannelPoll::Client(op),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if phase.deadline().is_some_and(|d| Instant::now() >= d) {
                    ChannelPoll::Input(PhaseInput::DeadlineReached)
                } else {
                    ChannelPoll::Continue
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => ChannelPoll::Done,
        }
    }
}

#[cfg(windows)]
type PlatformWatcher = notify::PollWatcher;

#[cfg(not(windows))]
type PlatformWatcher = notify::RecommendedWatcher;

struct CorpusWatcher {
    platform: PlatformWatcher,
    root: PathBuf,
}

impl CorpusWatcher {
    fn new(events: &mpsc::Sender<Event>, root: &Path) -> Result<Self, DaemonError> {
        #[cfg(windows)]
        let config =
            notify::Config::default().with_poll_interval(Duration::from_millis(DEBOUNCE_MS));
        #[cfg(not(windows))]
        let config = notify::Config::default();
        let platform = PlatformWatcher::new(
            {
                let events = events.clone();
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        let _ = events.send(Event::FsChange(event));
                    }
                }
            },
            config,
        )
        .map_err(|e| DaemonError::message(e.to_string()))?;
        let mut watcher = Self {
            platform,
            root: root.to_path_buf(),
        };
        watcher.watch(root)?;
        Ok(watcher)
    }

    fn watch(&mut self, root: &Path) -> Result<(), DaemonError> {
        self.platform
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| DaemonError::message(e.to_string()))
    }

    fn rebind(&mut self, store: &Path) -> Result<(), DaemonError> {
        let meta = StorePaths::read_meta(store)?;
        let root = meta.corpus.root;
        if root == self.root {
            return Ok(());
        }
        let _ = self.platform.unwatch(self.root.as_path());
        self.watch(&root)?;
        self.root = root;
        Ok(())
    }
}

struct IndexRefresh<'a> {
    tx: &'a mpsc::Sender<Event>,
    store: &'a Path,
    pending: &'a Arc<Mutex<PendingIndex>>,
}

impl IndexRefresh<'_> {
    fn apply_client(
        &self,
        op: DaemonOp,
        watcher: &mut CorpusWatcher,
        store: &Path,
        phase: &mut Phase,
    ) -> Result<(), DaemonError> {
        let paths = op.into_index_paths();
        watcher.rebind(store)?;
        PendingIndex::lock(self.pending)?.push(paths);
        if phase.is_refreshing() {
            *phase = Phase::Refreshing {
                follow_up: RefreshFollowUp::Queued,
            };
        } else {
            self.spawn(RefreshScope::CorpusAndPending);
            *phase = Phase::Refreshing {
                follow_up: RefreshFollowUp::None,
            };
        }
        Ok(())
    }

    fn flush_pending(&self, phase: &mut Phase) {
        self.spawn(RefreshScope::PendingOnly);
        *phase = Phase::Refreshing {
            follow_up: RefreshFollowUp::None,
        };
    }

    fn start(&self, phase: &mut Phase) {
        self.spawn(RefreshScope::CorpusAndPending);
        *phase = Phase::Refreshing {
            follow_up: RefreshFollowUp::None,
        };
    }

    fn spawn(&self, scope: RefreshScope) {
        let tx = self.tx.clone();
        let sift_dir = self.store.to_path_buf();
        let pending = Arc::clone(self.pending);
        std::thread::spawn(move || {
            let meta = match StorePaths::read_meta(&sift_dir) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use sift_core::VisibilityConfig;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    const DEBOUNCE: Duration = Duration::from_mins(1);
    const IDLE: Duration = Duration::from_mins(10);

    #[test]
    fn round_trip_index_paths() {
        let paths = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let mut buf = Vec::new();
        DaemonOp::index(paths.clone()).encode(&mut buf).unwrap();
        let op = DaemonOp::decode(buf.as_slice()).unwrap();
        assert_eq!(op, DaemonOp::Index(paths));
    }

    #[test]
    fn round_trip_index_full() {
        let mut buf = Vec::new();
        DaemonOp::full_corpus().encode(&mut buf).unwrap();
        let op = DaemonOp::decode(buf.as_slice()).unwrap();
        assert_eq!(op, DaemonOp::full_corpus());
    }

    #[cfg(unix)]
    #[test]
    fn index_round_trip_over_unix_stream() {
        use std::os::unix::net::UnixStream;
        use std::thread;

        let (mut client, server) = UnixStream::pair().unwrap();
        let paths = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let expected = DaemonOp::index(paths.clone());

        let handle = thread::spawn(move || {
            let mut server = server;
            let op = DaemonOp::decode(&mut server).unwrap();
            assert_eq!(op, DaemonOp::index(paths));
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
        assert!(matches!(
            phase,
            Phase::Refreshing {
                follow_up: RefreshFollowUp::None
            }
        ));
        assert_eq!(action, LoopAction::StartRefresh);
    }

    #[test]
    fn phase_refreshing_complete_with_follow_up_restarts_refresh() {
        let mut phase = Phase::Refreshing {
            follow_up: RefreshFollowUp::Queued,
        };
        let action = phase.advance(PhaseInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(
            phase,
            Phase::Refreshing {
                follow_up: RefreshFollowUp::None
            }
        ));
        assert_eq!(action, LoopAction::StartRefresh);
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
                visibility: VisibilityConfig::default(),
            },
            vec![IndexKind::Trigram],
        );
        StoreMeta::write(&meta, &sift_dir).unwrap();

        pending.reconcile(&sift_dir, &meta);
        assert!(pending.is_pending());
    }

    #[test]
    fn rebind_watcher_skips_when_root_unchanged() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");
        std::fs::create_dir_all(&sift_dir).unwrap();
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
                visibility: VisibilityConfig::default(),
            },
            vec![IndexKind::Trigram],
        );
        StoreMeta::write(&meta, &sift_dir).unwrap();

        let (tx, _rx) = mpsc::channel();
        let mut watcher = CorpusWatcher::new(&tx, dir.path()).unwrap();

        watcher.rebind(&sift_dir).unwrap();
        assert_eq!(watcher.root.as_path(), dir.path());
    }

    #[test]
    fn rebind_watcher_switches_to_new_corpus_root() {
        let dir = TempDir::new().unwrap();
        let sift_dir = dir.path().join(".sift");
        let old_root = dir.path().join("old");
        let new_root = dir.path().join("new");
        std::fs::create_dir_all(&old_root).unwrap();
        std::fs::create_dir_all(&new_root).unwrap();
        std::fs::create_dir_all(&sift_dir).unwrap();

        StoreMeta::write(
            &StoreMeta::new(
                CorpusMeta {
                    root: old_root.clone(),
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
                    visibility: VisibilityConfig::default(),
                },
                vec![IndexKind::Trigram],
            ),
            &sift_dir,
        )
        .unwrap();

        let (tx, _rx) = mpsc::channel();
        let mut watcher = CorpusWatcher::new(&tx, &old_root).unwrap();

        StoreMeta::write(
            &StoreMeta::new(
                CorpusMeta {
                    root: new_root.clone(),
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
                    visibility: VisibilityConfig::default(),
                },
                vec![IndexKind::Trigram],
            ),
            &sift_dir,
        )
        .unwrap();

        watcher.rebind(&sift_dir).unwrap();
        assert_eq!(watcher.root.as_path(), new_root.as_path());
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
                visibility: VisibilityConfig::default(),
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
                    .serve(
                        ServeConfig {
                            ready: None,
                            idle_timeout: Duration::from_mins(2),
                        },
                        shutdown.as_ref(),
                    )
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
            .serve(
                ServeConfig {
                    ready: None,
                    idle_timeout: Duration::from_mins(2),
                },
                &shutdown,
            )
            .unwrap();
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
                visibility: VisibilityConfig::default(),
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
                    .serve(
                        ServeConfig {
                            ready: None,
                            idle_timeout: Duration::from_mins(2),
                        },
                        shutdown.as_ref(),
                    )
                    .unwrap();
            }
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        while !Daemon::new(sift_dir.clone()).reachable().unwrap() {
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
}
