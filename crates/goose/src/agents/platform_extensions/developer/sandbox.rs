use std::path::{Path, PathBuf};

pub(super) const ROOT_ENV: &str = "GOOSE_TOOL_SANDBOX_ROOT";
pub(super) const HOME_ENV: &str = "GOOSE_TOOL_SANDBOX_HOME";
pub(super) const DENY_ROOT_ENV: &str = "GOOSE_TOOL_SANDBOX_DENY_ROOT";

pub(super) fn root() -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os(ROOT_ENV) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("tool sandbox root is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("tool sandbox root is not a directory".to_string());
    }
    Ok(Some(canonical))
}

pub(super) fn checked_path(path: PathBuf) -> Result<PathBuf, String> {
    let Some(root) = root()? else {
        return Ok(path);
    };
    let comparable = canonical_target(&path)?;
    if !comparable.starts_with(&root) {
        return Err(format!(
            "benchmark tool sandbox denied path outside {}",
            root.display()
        ));
    }
    Ok(path)
}

fn canonical_target(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| format!("cannot resolve tool path: {error}"));
    }

    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| "cannot resolve tool path ancestor".to_string())?;
        suffix.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "cannot resolve tool path ancestor".to_string())?;
    }
    let mut canonical = ancestor
        .canonicalize()
        .map_err(|error| format!("cannot resolve tool path ancestor: {error}"))?;
    for component in suffix.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(target_os = "macos")]
pub(super) fn macos_profile(root: &Path, home: &Path, temp: &Path) -> Result<String, String> {
    let deny_root = std::env::var_os(DENY_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{DENY_ROOT_ENV} is required when {ROOT_ENV} is set"))?;
    let deny_root = deny_root
        .canonicalize()
        .map_err(|error| format!("tool sandbox deny root is unavailable: {error}"))?;
    for allowed in [root, home, temp] {
        if !allowed.starts_with(&deny_root) {
            return Err("tool sandbox paths must be contained by its deny root".to_string());
        }
    }
    Ok(format!(
        "(version 1) (allow default) (deny process-info*) \
         (deny file-read* file-write* (subpath \"{}\")) \
         (deny file-read* file-write* (subpath \"/private/tmp\")) \
         (allow file-read* file-write* (subpath \"{}\")) \
         (allow file-read* file-write* (subpath \"{}\")) \
         (allow file-read* file-write* (subpath \"{}\"))",
        profile_escape(&deny_root),
        profile_escape(root),
        profile_escape(home),
        profile_escape(temp),
    ))
}

#[cfg(target_os = "macos")]
fn profile_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_target_detects_existing_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
            assert!(canonical_target(&root.join("escape/file"))
                .unwrap()
                .starts_with(&outside));
        }
    }
}
