#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintMode {
    #[default]
    Standard,
    OnlyMatching,
    Count,
    CountMatches,
    FilesWithMatches,
    FilesWithoutMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputEmission {
    #[default]
    Normal,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZeroCountMode {
    #[default]
    Omit,
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEmissionMode {
    Lines,
    OnlyMatching,
}
