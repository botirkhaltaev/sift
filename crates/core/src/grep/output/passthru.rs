#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassthruMode {
    #[default]
    Disabled,
    Enabled,
}
