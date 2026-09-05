//! The working tree as DATA: what is on disk, when, and how big — read by the fs_delta
//! attribution — and the APP-TREE snapshots: the best-tree mirror and (VA-027) the write-once
//! pre-fix tree. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). `snapshot_tree_files` moved verbatim
//! from swarm.rs, paying for the worker prompt's REQUEST_FILE line (VA-008 adjunct); the two
//! rsync call sites' argument lists collapsed into `rsync_app_tree`, paying for the handover's
//! prefix snapshot.

use std::path::Path;

/// A cheap stable content hash (FNV-1a) — provenance, not cryptography (the `desc_sha` a brief is
/// stamped with).
pub(super) fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

/// The engine's snapshot exclusions (F886): what is NOT the app tree — the engine's own state,
/// the run log, the screenshots, the heartbeat and the scorer's db all live beside the app in the
/// same directory. ONE list for the best-tree mirror, its restore and the pre-fix snapshot; the
/// harness's `ENGINE_EXCLUDED_NAMES` (fix_waves_delta.py) mirrors it name for name.
pub(super) const SNAPSHOT_EXCLUDES: [&str; 5] = [
    ".swarm",
    "run.jsonl",
    "bench-shots",
    "heartbeat",
    "graded.db",
];

/// rsync `src/` into `dest/` under `SNAPSHOT_EXCLUDES`. `mirror` = `--delete` (the best tree is
/// REPLACED by a strictly-better verify, and the restore replaces the final tree); without it the
/// copy only adds — the write-once shape. `Ok(true)` = rsync exited 0; `Err` = it could not be
/// spawned or `dest` could not be created. Never silent: the callers put the result in an event.
pub(super) async fn rsync_app_tree(src: &Path, dest: &Path, mirror: bool) -> std::io::Result<bool> {
    std::fs::create_dir_all(dest)?;
    let mut cmd = tokio::process::Command::new("rsync");
    cmd.arg("-a");
    if mirror {
        cmd.arg("--delete");
    }
    for ex in SNAPSHOT_EXCLUDES {
        cmd.arg("--exclude").arg(ex);
    }
    let status = cmd
        .arg(format!("{}/", src.display()))
        .arg(format!("{}/", dest.display()))
        .status()
        .await?;
    Ok(status.success())
}

/// VA-027: the PRE-FIX tree — the app as INTEGRATE left it, copied ONCE at the INTEGRATE -> REPAIR
/// handover into `.swarm/prefix-tree/` and never written again (no --delete; a present, non-empty
/// dir is left exactly as it is, so a resume that re-enters REPAIR cannot overwrite the original).
/// WHY: the engine's `.swarm/best-tree` is mirrored on every strictly-better verify — r6c's round 1
/// overwrote round 0, the survivor was byte-identical to the final tree, and whether 458 node-
/// minutes of fix waves moved the score was UNMEASURABLE BY CONSTRUCTION (VA-019/VA-027). The harness
/// (`fix_waves_delta.py`, `score_run.sh --prefix`) prefers this dir the moment it exists. Returns the
/// `prefix_tree_snapshot` event row — `{ok:true, files, path}`, `{ok:true, skipped, files}` when
/// already present, or `{ok:false, error}` — so the failure is as loud as the success (gate 1).
pub(super) async fn write_once_prefix_tree(cwd: &Path) -> serde_json::Value {
    let dest = cwd.join(".swarm").join("prefix-tree");
    let present = std::fs::read_dir(&dest).is_ok_and(|mut d| d.next().is_some());
    if present {
        return serde_json::json!({
            "event": "prefix_tree_snapshot",
            "ok": true,
            "skipped": "already present — write-once, left as it was",
            "files": snapshot_tree_files(&dest).len(),
            "path": dest.display().to_string(),
        });
    }
    match rsync_app_tree(cwd, &dest, false).await {
        Ok(true) => serde_json::json!({
            "event": "prefix_tree_snapshot",
            "ok": true,
            "files": snapshot_tree_files(&dest).len(),
            "path": dest.display().to_string(),
        }),
        Ok(false) => serde_json::json!({
            "event": "prefix_tree_snapshot",
            "ok": false,
            "error": "rsync exited non-zero",
            "path": dest.display().to_string(),
        }),
        Err(e) => serde_json::json!({
            "event": "prefix_tree_snapshot",
            "ok": false,
            "error": e.to_string(),
            "path": dest.display().to_string(),
        }),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_and_separates_bytes() {
        let v1 = content_hash(b"def f(): return 41");
        let v2 = content_hash(b"def f(): return 42");
        assert_ne!(v1, v2, "different bytes must hash apart");
        assert_eq!(
            v1,
            content_hash(b"def f(): return 41"),
            "same bytes, same hash"
        );
    }

    fn rsync_available() -> bool {
        std::process::Command::new("rsync")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// The r6c shape: the app tree beside the run's evidence. The pre-fix copy carries the app and
    /// none of the excluded names; a second handover (a resume re-entering REPAIR, or the harness
    /// calling twice) leaves the first copy untouched and says so.
    #[tokio::test]
    async fn the_prefix_tree_is_the_app_only_and_written_once() {
        if !rsync_available() {
            eprintln!("rsync not on PATH — skipping (the engine shells out to it at runtime)");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::create_dir_all(root.join(".swarm/best-tree")).unwrap();
        std::fs::create_dir_all(root.join("bench-shots")).unwrap();
        std::fs::write(root.join("app/api.py"), "print(1)\n").unwrap();
        std::fs::write(root.join("run.jsonl"), "{}\n").unwrap();
        std::fs::write(root.join("heartbeat"), "1\n").unwrap();
        std::fs::write(root.join("graded.db"), "x").unwrap();
        std::fs::write(root.join("bench-shots/a.png"), "x").unwrap();
        std::fs::write(root.join(".swarm/best-tree/api.py"), "old").unwrap();

        let first = write_once_prefix_tree(root).await;
        assert_eq!(first["event"], "prefix_tree_snapshot");
        assert_eq!(first["ok"], true, "{first}");
        assert!(first.get("skipped").is_none(), "{first}");
        assert_eq!(first["files"], 1, "the app file only: {first}");
        let dest = root.join(".swarm/prefix-tree");
        assert_eq!(
            std::fs::read_to_string(dest.join("app/api.py")).unwrap(),
            "print(1)\n"
        );
        for excluded in [
            "run.jsonl",
            "heartbeat",
            "graded.db",
            "bench-shots",
            ".swarm",
        ] {
            assert!(
                !dest.join(excluded).exists(),
                "{excluded} must not be snapshotted"
            );
        }

        // The app changes after the handover (a fix wave promoted); the pre-fix copy does not move.
        std::fs::write(root.join("app/api.py"), "print(2)\n").unwrap();
        std::fs::write(root.join("app/new.py"), "").unwrap();
        let second = write_once_prefix_tree(root).await;
        assert_eq!(second["ok"], true, "{second}");
        assert!(second["skipped"].as_str().is_some(), "{second}");
        assert_eq!(
            std::fs::read_to_string(dest.join("app/api.py")).unwrap(),
            "print(1)\n",
            "write-once: the original bytes survive the wave"
        );
        assert!(!dest.join("app/new.py").exists());
    }

    /// A destination that cannot be created is a loud `ok:false` with the error, never a silent
    /// skip — here the run root is a FILE, so `.swarm/prefix-tree` cannot exist under it.
    #[tokio::test]
    async fn a_failed_snapshot_is_loud() {
        if !rsync_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let not_a_dir = dir.path().join("a-file");
        std::fs::write(&not_a_dir, "x").unwrap();
        let row = write_once_prefix_tree(&not_a_dir).await;
        assert_eq!(row["ok"], false, "{row}");
        assert!(
            row["error"].as_str().is_some_and(|e| !e.is_empty()),
            "{row}"
        );
    }
}
