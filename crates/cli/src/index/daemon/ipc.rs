use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use sift_core::DaemonOp;

use super::Daemon;
use super::error::DaemonError;

fn socket_id(sift_dir: &Path) -> u64 {
    let canonical = sift_dir
        .canonicalize()
        .unwrap_or_else(|_| sift_dir.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    hasher.finish()
}

fn socket_name_for(sift_dir: &Path) -> Result<interprocess::local_socket::Name<'_>, DaemonError> {
    format!("sift-{:016x}", socket_id(sift_dir))
        .to_ns_name::<GenericNamespaced>()
        .map_err(DaemonError::Io)
}

impl Daemon {
    /// Namespaced IPC endpoint derived from the `.sift` directory.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        PathBuf::from(format!("sift-{:016x}", socket_id(&self.sift_dir)))
    }

    fn socket_name(&self) -> Result<interprocess::local_socket::Name<'_>, DaemonError> {
        socket_name_for(&self.sift_dir)
    }

    /// Listen for IPC requests and dispatch them to `handler`.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be created.
    pub fn listen(
        &self,
        mut handler: impl FnMut(DaemonOp) -> bool + Send + 'static,
    ) -> Result<(), DaemonError> {
        let name = self.socket_name()?;
        let listener = ListenerOptions::new()
            .name(name)
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

    pub(super) fn transmit(&self, op: &DaemonOp) -> Result<(), DaemonError> {
        let name = self.socket_name()?;
        let mut stream = Stream::connect(name).map_err(DaemonError::Io)?;
        op.encode(&mut stream)?;
        let mut status = [0_u8; 1];
        stream.read_exact(&mut status)?;
        if status[0] == DaemonOp::STATUS_OK {
            Ok(())
        } else {
            Err(DaemonError::message("daemon rejected request"))
        }
    }
}
