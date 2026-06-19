use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};

use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use sift_core::DaemonOp;

use super::Daemon;
use super::error::DaemonError;

impl Daemon {
    fn ipc_name(&self) -> Result<interprocess::local_socket::Name<'_>, DaemonError> {
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

    /// Listen for IPC requests and dispatch them to `handler`.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be created.
    pub fn listen(
        &self,
        mut handler: impl FnMut(DaemonOp) -> bool + Send + 'static,
    ) -> Result<(), DaemonError> {
        let name = self.ipc_name()?;
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
        let name = self.ipc_name()?;
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
