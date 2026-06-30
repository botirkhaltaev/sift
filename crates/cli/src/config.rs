use std::path::{Path, PathBuf};

const RIPGREP_CONFIG_PATH: &str = "RIPGREP_CONFIG_PATH";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigArgs {
    args: Vec<String>,
}

impl ConfigArgs {
    #[must_use]
    pub const fn empty() -> Self {
        Self { args: Vec::new() }
    }

    /// Loads config arguments from `RIPGREP_CONFIG_PATH` unless disabled by raw CLI args.
    ///
    /// # Errors
    ///
    /// Returns an error when `RIPGREP_CONFIG_PATH` points to a file that cannot be read.
    pub fn from_env(raw_args: &[String]) -> Result<Self, ConfigError> {
        if Self::raw_args_disable_config(raw_args) {
            return Ok(Self::empty());
        }

        let Some(path) = std::env::var_os(RIPGREP_CONFIG_PATH) else {
            return Ok(Self::empty());
        };
        if path.is_empty() {
            return Ok(Self::empty());
        }

        Self::read(PathBuf::from(path))
    }

    /// Reads and parses a ripgrep-compatible config file.
    ///
    /// # Errors
    ///
    /// Returns an error when the config file cannot be read as UTF-8 text.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self {
            args: Self::parse(&contents),
        })
    }

    #[must_use]
    pub fn apply(&self, raw_args: &[String]) -> Vec<String> {
        let mut args = Vec::with_capacity(raw_args.len() + self.args.len());
        match raw_args.split_first() {
            Some((program, rest)) => {
                args.push(program.clone());
                args.extend(self.args.iter().cloned());
                args.extend(rest.iter().cloned());
            }
            None => args.extend(self.args.iter().cloned()),
        }
        args
    }

    fn parse(contents: &str) -> Vec<String> {
        contents
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ToOwned::to_owned)
            .collect()
    }

    fn raw_args_disable_config(raw_args: &[String]) -> bool {
        raw_args
            .iter()
            .skip(1)
            .take_while(|arg| arg.as_str() != "--")
            .any(|arg| arg == "--no-config")
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                f,
                "failed to read the file specified in {RIPGREP_CONFIG_PATH}: {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigArgs;

    #[test]
    fn parses_non_comment_lines_as_arguments() {
        let args = ConfigArgs::parse(
            "\n\
             # comment\n\
             --smart-case\n\
               --glob=*.rs\n\
             \n\
             -n\n",
        );
        assert_eq!(args, ["--smart-case", "--glob=*.rs", "-n"]);
    }

    #[test]
    fn applies_config_after_program_name() {
        let config = ConfigArgs {
            args: vec!["--smart-case".into(), "-n".into()],
        };
        let args = config.apply(&["sift".into(), "needle".into()]);
        assert_eq!(args, ["sift", "--smart-case", "-n", "needle"]);
    }

    #[test]
    fn no_config_detection_stops_at_double_dash() {
        assert!(ConfigArgs::raw_args_disable_config(&[
            "sift".into(),
            "--no-config".into(),
            "needle".into(),
        ]));
        assert!(!ConfigArgs::raw_args_disable_config(&[
            "sift".into(),
            "--".into(),
            "--no-config".into(),
        ]));
    }
}
