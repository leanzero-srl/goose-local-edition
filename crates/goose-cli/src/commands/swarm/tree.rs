//! The working tree as DATA: what is on disk, when, and how big — read by the fs_delta
//! attribution and (from VA-027) the pre-fix snapshot. Sibling module under the incremental-split
//! law (development_gates::swarm_rs_line_count_only_decreases). `snapshot_tree_files` moved
//! verbatim from swarm.rs, paying for the worker prompt's REQUEST_FILE line (VA-008 adjunct).

use std::path::Path;

/// II-1 fs_delta, half 1: a cheap (path → mtime secs, size) map of the tree, taken at attempt
/// start. Excludes the engine's own bookkeeping and cache dirs so the delta is the APP's files.
pub(super) fn snapshot_tree_files(root: &Path) -> std::collections::BTreeMap<String, (i64, u64)> {
    const SKIP_DIRS: [&str; 6] = [
        ".swarm",
        ".git",
        "__pycache__",
        "node_modules",
        ".venv",
        "target",
    ];
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Ok(rel) = path.strip_prefix(root) {
                out.insert(rel.to_string_lossy().into_owned(), (mtime, meta.len()));
            }
        }
    }
    out
}
