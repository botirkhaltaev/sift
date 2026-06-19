use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};

use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use sift_core::DaemonOp;

use super::Daemon;
use super::error::DaemonError;

/// Namespaced IPC endpoint derived from a `.sift` store directory.
struct IpcEndpoint(interprocess::local_socket::Name<'static>);

impl IpcEndpoint {
    fn open(daemon: &Daemon) -> Result<Self, DaemonError> {
        let canonical = daemon
            .sift_dir
            .canonicalize()
            .unwrap_or_else(|_| daemon.sift_dir.clone());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        let name = format!("sift-{:016x}", hasher.finish())
            .to_ns_name::<GenericNamespaced>()
            .map_err(DaemonError::Io)?;
        Ok(Self(name))
    }

    fn connect(&self) -> Result<Stream, DaemonError> {
        Stream::connect(self.0.clone()).map_err(DaemonError::Io)
    }

    fn listen(
        &self,
        mut handler: impl FnMut(DaemonOp) -> bool + Send + 'static,
    ) -> Result<(), DaemonError> {
        let listener = ListenerOptions::new()
            .name(self.0.clone())
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

    fn transmit(&self, op: &DaemonOp) -> Result<(), DaemonError> {
        let mut stream = self.connect()?;
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

impl Daemon {
    /// Send an IPC operation to the daemon, spawning it when needed.
    ///
    /// # Errors
    ///
    /// Propagates spawn and IPC failures.
    pub fn send(&self, op: &DaemonOp) -> Result<(), DaemonError> {
        self.ensure_running()?;
        IpcEndpoint::open(self)?.transmit(op)
    }

    /// Listen for IPC requests and dispatch them to `handler`.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be created.
    pub fn listen(
        &self,
        handler: impl FnMut(DaemonOp) -> bool + Send + 'static,
    ) -> Result<(), DaemonError> {
        IpcEndpoint::open(self)?.listen(handler)
    }
}
