#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnOverflow {
    Omit,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnLimit {
    pub max: u64,
    pub overflow: ColumnOverflow,
}

pub enum ColumnAction {
    Normal,
    Omit,
    Preview,
}

impl ColumnLimit {
    #[must_use]
    pub fn classify(&self, line: &[u8]) -> ColumnAction {
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
        if trimmed.len() as u64 > self.max {
            match self.overflow {
                ColumnOverflow::Preview => ColumnAction::Preview,
                ColumnOverflow::Omit => ColumnAction::Omit,
            }
        } else {
            ColumnAction::Normal
        }
    }

    #[must_use]
    pub fn truncate(&self, line: &[u8]) -> Vec<u8> {
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
        let limit = usize::try_from(self.max).unwrap_or(usize::MAX);
        let mut out = Vec::with_capacity(limit.saturating_add(30));
        out.extend_from_slice(&trimmed[..limit.min(trimmed.len())]);
        out.extend_from_slice(b" [... omitted end ...]");
        out.push(b'\n');
        out
    }
}
