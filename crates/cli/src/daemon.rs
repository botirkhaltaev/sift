use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use fslock::LockFile;
use notify::{RecursiveMode, Watcher};
use sift_core::{IndexBuildConfig, IndexStore, TrigramIndex};

const DEBOUNCE_MS: u64 = 250;

pub struct DaemonConfig {
    pub sift_dir: std::path::PathBuf,
    pub init_root: Option<std::path::PathBuf>,
}

/// Run the daemon: acquire lock, build initial snapshot if needed, watch
/// root, debounce file events, and refresh the index.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired (other than contention),
/// metadata is malformed, or the watcher fails to start.
pub fn run(config: &DaemonConfig) -> anyhow::Result<()> {
    let sift_dir = &config.sift_dir;
    std::fs::create_dir_all(sift_dir)?;

    // ── acquire daemon lock ──────────────────────────────────────────────
    let lock_path = sift_dir.join("lock");
    let mut lock_file = LockFile::open(&lock_path)?;
    if !lock_file.try_lock()? {
        return Ok(());
    }

    // ── read or initialize metadata ──────────────────────────────────────
    let (root, corpus_kind, follow_links) =
        match (IndexStore::read_meta(sift_dir), &config.init_root) {
            (Ok(meta), _) => (meta.root, meta.corpus_kind, meta.follow_links),
            (Err(_), Some(init_root)) => {
                let root = init_root.canonicalize()?;
                (root, sift_core::CorpusKind::Directory, false)
            }
            (Err(e), None) => {
                anyhow::bail!("no store metadata: {e}");
            }
        };

    // ── build initial snapshot if this is a fresh store ──────────────────
    let mut store = IndexStore::open_or_create(sift_dir, &root, corpus_kind, follow_links)?;

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
        store.build::<TrigramIndex>(&build_config)?;
    }

    // ── file watcher ─────────────────────────────────────────────────────
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    // ── watch loop with debounce ─────────────────────────────────────────
    let mut changed = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
            Ok(event) => {
                if is_relevant_event(&event, sift_dir) {
                    changed = true;
                }
                while let Ok(event) = rx.try_recv() {
                    if is_relevant_event(&event, sift_dir) {
                        changed = true;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if changed {
                    changed = false;
                    refresh(sift_dir);
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

fn refresh(sift_dir: &Path) {
    let meta = match IndexStore::read_meta(sift_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("sift-daemon: read_meta: {e}");
            return;
        }
    };

    let mut store = match IndexStore::open(sift_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sift-daemon: open store: {e}");
            return;
        }
    };

    let exclude = sift_dir
        .strip_prefix(&meta.root)
        .unwrap_or(sift_dir)
        .to_path_buf();
    let build_config = IndexBuildConfig {
        root: &meta.root,
        follow_links: meta.follow_links,
        exclude_paths: &[exclude],
        include_paths: &[],
        corpus_kind: meta.corpus_kind,
    };

    if let Err(e) = store.build::<TrigramIndex>(&build_config) {
        eprintln!("sift-daemon: refresh failed: {e}");
    }
}
