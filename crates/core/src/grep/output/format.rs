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
