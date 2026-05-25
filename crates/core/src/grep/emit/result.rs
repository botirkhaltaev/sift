use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct FileResult {
    pub index: usize,
    pub output: ChunkOutput,
    pub json_stats: Option<grep_printer::Stats>,
}

pub struct ChunkOutput {
    pub bytes: Vec<u8>,
    pub matched: bool,
    pub heading: bool,
}

impl ChunkOutput {
    pub const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            matched: false,
            heading: false,
        }
    }
}

pub fn flush_chunk_output(
    outputs: impl IntoIterator<Item = ChunkOutput>,
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
