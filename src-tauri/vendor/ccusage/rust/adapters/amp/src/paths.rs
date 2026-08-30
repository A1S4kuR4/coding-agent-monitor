use std::{collections::HashSet, env, path::PathBuf};

use crate::Result;

const AMP_DATA_DIR_ENV: &str = "AMP_DATA_DIR";

pub(super) fn paths() -> Result<Vec<PathBuf>> {
    // Downstream (Coding Agent Monitor) 0002 patch: explicit data roots for this load.
    if let Some(roots) = ccusage_core::load_context::root_override("amp") {
        return Ok(roots);
    }

    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    if let Ok(env_paths) = env::var(AMP_DATA_DIR_ENV) {
        for raw in env_paths
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            let path = PathBuf::from(raw);
            if path.is_dir() && seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        return Ok(paths);
    }

    let home =
        crate::home::home_dir().ok_or_else(|| crate::cli_error("home directory is not set"))?;
    let path = home.join(".local/share/amp");
    if path.is_dir() && seen.insert(path.clone()) {
        paths.push(path);
    }
    Ok(paths)
}
