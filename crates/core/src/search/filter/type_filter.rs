use std::path::Path;

use ignore::overrides::{Override, OverrideBuilder};

use super::config::{CandidateFilterConfig, TypeDef};
use super::error::FilterError;

fn globs_for_type(defs: &[TypeDef], name: &str) -> Result<Vec<String>, FilterError> {
    for def in defs {
        if def.name == name {
            return Ok(def.globs.clone());
        }
    }
    Err(FilterError::RegexBuild(format!(
        "unknown file type: '{name}'"
    )))
}

impl CandidateFilterConfig {
    /// Build a type-filter override matcher when `-t` / `-T` flags are present.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::RegexBuild`] if type names or globs are invalid.
    pub fn type_override(&self, root: &Path) -> Result<Option<Override>, FilterError> {
        if self.type_include.is_empty() && self.type_exclude.is_empty() {
            return Ok(None);
        }
        let mut builder = OverrideBuilder::new(root);
        for name in &self.type_include {
            let patterns = globs_for_type(&self.type_definitions, name)?;
            for g in patterns {
                builder
                    .add(&g)
                    .map_err(|e| FilterError::RegexBuild(format!("type glob for '{name}': {e}")))?;
            }
        }
        for name in &self.type_exclude {
            let patterns = globs_for_type(&self.type_definitions, name)?;
            for g in patterns {
                let negated = format!("!{g}");
                builder
                    .add(&negated)
                    .map_err(|e| FilterError::RegexBuild(format!("type glob for '{name}': {e}")))?;
            }
        }
        Ok(Some(
            builder
                .build()
                .map_err(|e| FilterError::RegexBuild(e.to_string()))?,
        ))
    }
}
