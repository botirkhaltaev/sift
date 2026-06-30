use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};

pub(in crate::grep) struct FileResult {
    pub(in crate::grep) output: ChunkOutput,
    pub(in crate::grep) json_stats: Option<grep_printer::Stats>,
    pub(in crate::grep) hit: Option<std::path::PathBuf>,
}

pub(in crate::grep) struct ChunkOutput {
    pub(in crate::grep) bytes: Vec<u8>,
    pub(in crate::grep) matched: bool,
    pub(in crate::grep) heading: bool,
}

impl ChunkOutput {
    #[must_use]
    pub(in crate::grep) const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            matched: false,
            heading: false,
        }
    }

    /// # Errors
    ///
    /// Returns an error if writing to stdout fails.
    pub(in crate::grep) fn flush_all(
        outputs: impl IntoIterator<Item = Self>,
        bytes_printed: Option<&AtomicU64>,
    ) -> crate::Result<bool> {
        let mut stdout = io::stdout().lock();
        let mut any_match = false;
        let mut emitted = false;
        for output in outputs {
            any_match |= output.matched;
            if output.bytes.is_empty() {
                continue;
            }
            if output.heading && emitted {
                stdout.write_all(b"\n")?;
                if let Some(p) = bytes_printed {
                    p.fetch_add(1, Ordering::Relaxed);
                }
            }
            let n = output.bytes.len() as u64;
            if let Some(p) = bytes_printed {
                p.fetch_add(n, Ordering::Relaxed);
            }
            stdout.write_all(&output.bytes)?;
            emitted = true;
        }
        Ok(any_match)
    }
}
