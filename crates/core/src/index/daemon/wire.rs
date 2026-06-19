use std::io::{self, Read, Write};
use std::path::PathBuf;

use super::op::DaemonOp;

impl DaemonOp {
    /// Encode this operation for IPC.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    pub fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        match self {
            Self::Watch => {
                writer.write_all(&[Self::WATCH_OPCODE])?;
            }
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
            Self::WATCH_OPCODE => Ok(Self::Watch),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_watch() {
        let mut buf = Vec::new();
        DaemonOp::Watch.encode(&mut buf).unwrap();
        let op = DaemonOp::decode(buf.as_slice()).unwrap();
        assert_eq!(op, DaemonOp::Watch);
    }

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
}
