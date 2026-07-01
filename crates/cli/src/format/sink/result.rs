use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::format::output::style::OutputBuffering;

pub(in crate::format) struct FileResult {
    pub(in crate::format) output: ChunkOutput,
    pub(in crate::format) json_stats: Option<grep_printer::Stats>,
    pub(in crate::format) hit: Option<std::path::PathBuf>,
}

pub(in crate::format) struct ChunkOutput {
    pub(in crate::format) bytes: Vec<u8>,
    pub(in crate::format) matched: bool,
    pub(in crate::format) heading: bool,
}

impl ChunkOutput {
    #[must_use]
    pub(in crate::format) const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            matched: false,
            heading: false,
        }
    }

    /// # Errors
    ///
    /// Returns an error if writing to stdout fails.
    pub(in crate::format) fn flush_all(
        outputs: impl IntoIterator<Item = Self>,
        bytes_printed: Option<&AtomicU64>,
        buffering: OutputBuffering,
    ) -> sift_core::Result<bool> {
        let stdout = io::stdout();
        let mut locked = stdout.lock();
        let mut block;
        let writer: &mut dyn Write = match buffering {
            OutputBuffering::Block => {
                block = io::BufWriter::new(locked);
                &mut block
            }
            OutputBuffering::Auto | OutputBuffering::Line => &mut locked,
        };
        let mut any_match = false;
        let mut emitted = false;
        for output in outputs {
            any_match |= output.matched;
            if output.bytes.is_empty() {
                continue;
            }
            if output.heading && emitted {
                writer.write_all(b"\n")?;
                if let Some(p) = bytes_printed {
                    p.fetch_add(1, Ordering::Relaxed);
                }
                if matches!(buffering, OutputBuffering::Line) {
                    writer.flush()?;
                }
            }
            let n = output.bytes.len() as u64;
            if let Some(p) = bytes_printed {
                p.fetch_add(n, Ordering::Relaxed);
            }
            writer.write_all(&output.bytes)?;
            if matches!(buffering, OutputBuffering::Line) {
                writer.flush()?;
            }
            emitted = true;
        }
        writer.flush()?;
        Ok(any_match)
    }
}
