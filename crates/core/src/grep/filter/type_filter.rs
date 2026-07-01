use std::path::Path;

use ignore::overrides::{Override, OverrideBuilder};

use super::config::{CandidateFilterConfig, TypeDef, TypeSelection};
use super::error::FilterError;

fn globs_for_type(defs: &[TypeDef], name: &str) -> Result<Vec<String>, FilterError> {
    if name == "all" {
        let globs = defs
            .iter()
            .flat_map(|def| def.globs.iter().cloned())
            .collect::<Vec<_>>();
        return Ok(globs);
    }
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
        if self.type_selections.is_empty()
            && self.type_include.is_empty()
            && self.type_exclude.is_empty()
        {
            return Ok(None);
        }
        let mut builder = OverrideBuilder::new(root);
        if self.type_selections.is_empty() {
            for name in &self.type_include {
                add_type_patterns(&mut builder, &self.type_definitions, name, false)?;
            }
            for name in &self.type_exclude {
                add_type_patterns(&mut builder, &self.type_definitions, name, true)?;
            }
        } else {
            for selection in &self.type_selections {
                match selection {
                    TypeSelection::Include(name) => {
                        add_type_patterns(&mut builder, &self.type_definitions, name, false)?;
                    }
                    TypeSelection::Exclude(name) => {
                        add_type_patterns(&mut builder, &self.type_definitions, name, true)?;
                    }
                }
            }
        }
        Ok(Some(
            builder
                .build()
                .map_err(|e| FilterError::RegexBuild(e.to_string()))?,
        ))
    }
}

fn add_type_patterns(
    builder: &mut OverrideBuilder,
    definitions: &[TypeDef],
    name: &str,
    exclude: bool,
) -> Result<(), FilterError> {
    let patterns = globs_for_type(definitions, name)?;
    for glob in patterns {
        let pattern;
        let glob = if exclude {
            pattern = format!("!{glob}");
            pattern.as_str()
        } else {
            glob.as_str()
        };
        builder
            .add(glob)
            .map_err(|e| FilterError::RegexBuild(format!("type glob for '{name}': {e}")))?;
    }
    Ok(())
}
