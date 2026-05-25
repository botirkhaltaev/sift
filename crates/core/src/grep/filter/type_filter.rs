use std::path::Path;

use ignore::overrides::{Override, OverrideBuilder};

use super::config::TypeDef;
use super::error::FilterError;

pub fn build_type_glob(
    root: &Path,
    defs: &[TypeDef],
    include: &[String],
    exclude: &[String],
) -> Result<Option<Override>, FilterError> {
    if include.is_empty() && exclude.is_empty() {
        return Ok(None);
    }
    let mut builder = OverrideBuilder::new(root);
    for name in include {
        let patterns = globs_for_type(defs, name)?;
        for g in patterns {
            builder
                .add(&g)
                .map_err(|e| FilterError::RegexBuild(format!("type glob for '{name}': {e}")))?;
        }
    }
    for name in exclude {
        let patterns = globs_for_type(defs, name)?;
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
