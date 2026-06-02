//! Raw command-line arguments for ripgrep-style last-wins flag resolution.

/// Process argv slice used to resolve effective flag values after clap parsing.
pub struct Argv<'a> {
    args: &'a [String],
}

impl<'a> Argv<'a> {
    #[must_use]
    pub fn from_env() -> Vec<String> {
        std::env::args().collect()
    }

    #[must_use]
    pub const fn new(args: &'a [String]) -> Self {
        Self { args }
    }

    #[must_use]
    pub const fn as_slice(&self) -> &'a [String] {
        self.args
    }
}
