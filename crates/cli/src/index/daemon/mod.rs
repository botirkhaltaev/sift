//! Background index refresh daemon: IPC, spawn, coordinator, and serve loop.

pub(crate) const DEBOUNCE_MS: u64 = 250;

mod coalesce;
mod coordinator;
mod error;
mod ipc;
mod serve;
mod spawn;
mod watch;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub use error::DaemonError;

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
}

/// Options for [`Daemon::serve`].
#[derive(Debug, Clone)]
pub struct Serve {
    pub ready_file: Option<PathBuf>,
    pub idle_timeout: Duration,
    pub shutdown: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Default for Serve {
    fn default() -> Self {
        Self {
            ready_file: None,
            idle_timeout: watch::DEFAULT_IDLE_TIMEOUT,
            shutdown: None,
        }
    }
}
