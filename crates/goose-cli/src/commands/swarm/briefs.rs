//! Worker-brief text fragments measured against the tree at dispatch time. Sibling module under
//! the incremental-split law (development_gates::swarm_rs_line_count_only_decreases).

use std::path::Path;

/// The multi-file note a worker owning >1 file reads. Two honest framings, branched on a
/// MEASURED predicate, never a guess:
///
/// - AUTHORING (any owned file missing): multi-file tasks fail by writing the first owned file,
///   forgetting the rest, then claiming done — the completion guard retries but the worker
///   repeats it, so the note demands every path exist and be non-empty.
/// - REPAIR where every owned file already EXISTS (the 6585f0845 winner+runner-up shard shape:
///   route table + handler body): "you MUST write EVERY one" was a lie-shaped pressure to
///   rewrite two live files whose defect lives in ONE of them. The softened note states the
///   measured fact (all files exist) and asks for a targeted edit wherever the defect actually
///   lives — either side can land, per the promote's owned-files surface.
///
/// Empty for a single-file task, exactly as before.
pub(super) fn multi_file_note(owned_files: &[String], repairing: bool, root: &Path) -> String {
    if owned_files.len() <= 1 {
        return String::new();
    }
    let n = owned_files.len();
    if repairing && owned_files.iter().all(|f| root.join(f).is_file()) {
        return format!(
            "\nYOU OWN {n} FILES — every one already exists on disk. Your job is a targeted \
             fix, not a rewrite: the defect may live in EITHER file (route table vs handler \
             body — whichever side you fix can land), so read them, edit the one(s) that \
             actually carry the defect, and leave the rest as they are. Do NOT rewrite a file \
             just to have written it."
        );
    }
    format!(
        "\nYOU OWN {n} FILES — you MUST write EVERY one. The classic multi-file failure is \
         writing the first and forgetting the rest, then claiming done: this task is NOT \
         complete until ALL {n} paths above exist and are non-empty. Write them one after \
         another and verify each is on disk before you finish."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The predicate is measured on disk: a repair shard whose owned files ALL exist gets the
    /// targeted-edit framing; a missing file (or a non-repair task) keeps the write-every-one
    /// demand; a single file gets nothing.
    #[test]
    fn the_multi_file_note_softens_only_for_a_repair_shard_whose_files_all_exist() {
        let dir = std::env::temp_dir().join(format!("briefs-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("app")).unwrap();
        let owned = vec!["app/a.py".to_string(), "app/b.py".to_string()];
        std::fs::write(dir.join("app/a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("app/b.py"), "y = 2\n").unwrap();
        let soft = multi_file_note(&owned, true, &dir);
        assert!(soft.contains("targeted"), "softened framing: {soft}");
        assert!(!soft.contains("MUST write EVERY one"));
        // Same files, not a repair shard: the authoring demand stands.
        assert!(multi_file_note(&owned, false, &dir).contains("MUST write EVERY one"));
        // Repair shard but one file missing: the demand stands (the missing file must appear).
        std::fs::remove_file(dir.join("app/b.py")).unwrap();
        assert!(multi_file_note(&owned, true, &dir).contains("MUST write EVERY one"));
        assert_eq!(multi_file_note(&owned[..1], true, &dir), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
