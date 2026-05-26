use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use fslock::LockFile;
use notify::{RecursiveMode, Watcher};
use sift_core::{IndexBuildConfig, IndexKind, IndexStore};

const DEBOUNCE_MS: u64 = 250;

pub struct DaemonConfig {
    pub sift_dir: std::path::PathBuf,
    pub init_root: Option<std::path::PathBuf>,
}

impl DaemonConfig {
    /// Best-effort spawn `sift-daemon` in the background.
    ///
    /// Respects `SIFT_NO_DAEMON=1` to suppress spawning (used in tests).
    pub fn spawn(sift_dir: &Path, init_root: Option<&Path>) {
        if std::env::var_os("SIFT_NO_DAEMON").is_some_and(|v| v == "1") {
            return;
        }

        let exe = match std::env::current_exe() {
            Ok(p) => p.with_file_name("sift-daemon"),
            Err(_) => return,
        };

        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--sift-dir").arg(sift_dir);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        cmd.stdin(std::process::Stdio::null());

        if let Some(root) = init_root {
            cmd.arg("--init-root").arg(root);
        }

        let _ = cmd.spawn();
    }

    /// Run the daemon: acquire lock, build initial snapshot if needed, watch
    /// root, debounce file events, and refresh the index.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock cannot be acquired (other than contention),
    /// metadata is malformed, or the watcher fails to start.
    pub fn run(&self) -> anyhow::Result<()> {
        let sift_dir = &self.sift_dir;
        std::fs::create_dir_all(sift_dir)?;

        let lock_path = sift_dir.join("lock");
        let mut lock_file = LockFile::open(&lock_path)?;
        if !lock_file.try_lock()? {
            return Ok(());
        }

        let (root, corpus_kind, follow_links, stored_kinds) =
            match (IndexStore::read_meta(sift_dir), &self.init_root) {
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

        if store.current_id().is_none() {
            let exclude = sift_dir
                .strip_prefix(&root)
                .unwrap_or(sift_dir)
                .to_path_buf();
            let build_config = IndexBuildConfig {
                root: &root,
                follow_links,
                exclude_paths: &[exclude],
                include_paths: &[],
                corpus_kind,
            };
            store.build(kinds, &build_config)?;
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let mut changed = false;
        loop {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(event) => {
                    if Self::is_relevant_event(&event, sift_dir) {
                        changed = true;
                    }
                    while let Ok(event) = rx.try_recv() {
                        if Self::is_relevant_event(&event, sift_dir) {
                            changed = true;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if changed {
                        changed = false;
                        Self::refresh(
                            &mut store,
                            sift_dir,
                            kinds,
                            &root,
                            corpus_kind,
                            follow_links,
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }

    fn is_relevant_event(event: &notify::Event, sift_dir: &Path) -> bool {
        use notify::EventKind;
        match event.kind {
            EventKind::Access(_) => false,
            _ => !event.paths.iter().any(|p| p.starts_with(sift_dir)),
        }
    }

    fn refresh(
        store: &mut IndexStore,
        sift_dir: &Path,
        kinds: &[IndexKind],
        root: &Path,
        corpus_kind: sift_core::CorpusKind,
        follow_links: bool,
    ) {
        let exclude = sift_dir
            .strip_prefix(root)
            .unwrap_or(sift_dir)
            .to_path_buf();
        let build_config = IndexBuildConfig {
            root,
            follow_links,
            exclude_paths: &[exclude],
            include_paths: &[],
            corpus_kind,
        };

        if let Err(e) = store.update(kinds, &build_config) {
            eprintln!("sift-daemon: refresh failed: {e}");
        }
    }
}
