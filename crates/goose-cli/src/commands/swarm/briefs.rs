//! Worker-brief text fragments measured against the tree at dispatch time. Sibling module under
//! the incremental-split law (development_gates::swarm_rs_line_count_only_decreases).

use std::path::Path;

/// The multi-file note a worker owning >1 file reads. Two honest framings, branched on a
/// MEASURED predicate, never a guess:
///
/// - AUTHORING (any owned file missing): multi-file tasks fail by writing the first owned file,
///   forgetting the rest, then claiming done — the completion guard retries but the worker
///   repeats it, so the note demands every path exist and be non-empty.
/// - REPAIR where every owned file already EXISTS NON-EMPTY (the 6585f0845 winner+runner-up shard shape:
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
    // Non-empty, not merely present: an existing-but-EMPTY owned file still needs its one
    // write, and "already exists — targeted fix" would suppress exactly that. An unreadable
    // stat counts as missing, which keeps the DEMANDING arm — the honest degradation.
    let exists_non_empty = |f: &String| {
        std::fs::metadata(root.join(f))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    };
    if repairing && owned_files.iter().all(exists_non_empty) {
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

/// Non-entry MULTI-FILE modules are the other over-read failure class (verified UNIQ13 plan-shopping, which owns
/// plan.py + shopping.py and needs 4 sibling modules: across 3 attempts it ran ls/tree/find/cat exploring the
/// layout + reading deps but NEVER wrote an owned file, so the no-write over-read timeout killed each attempt and
/// cascade-failed the run — 2nd instance after the UNIQ9 tests-writer). The entry gets skeleton_note; give non-entry
/// multi-file owners the same MECHANICAL fix: write a COMPILING STUB of each owned file FIRST (which flips
/// any_owned_written true and exempts the over-read timeout), then read deps + fill. Scoped to multi-file only —
/// single-file skeleton-first was a same-spec-A/B WASH. Empty when an owned file is the entry (skeleton_note covers
/// it). Gated on GOOSE_SWARM_SKELETON_FIRST (passed in as `enabled`). Pure + unit-tested.
///
/// DISARMED for a REPAIR shard (the 0dc8c297f tracer's addendum): a repairing multi-file shard
/// — exactly the two-file winner+runner-up shape 6585f0845 creates (route table + handler
/// body) — owns LIVE files, and "your FIRST actions must be a `write` for EACH owned file
/// emitting a COMPILING STUB… with a `pass` body" orders it to gut both before fixing one.
/// Same `repairing` predicate `multi_file_note` branches on, never re-derived.
pub(super) fn multifile_stub_note(
    owned_files: &[String],
    enabled: bool,
    repairing: bool,
) -> String {
    let is_entry = |f: &str| {
        f.ends_with("cli.py")
            || f.ends_with("__main__.py")
            || f.ends_with("main.rs")
            || f.ends_with("index.ts")
            || f.ends_with("cli.ts")
            || f.ends_with("main.go")
    };
    if !enabled
        || repairing
        || owned_files.len() <= 1
        || owned_files.iter().any(|f| is_entry(f.as_str()))
    {
        return String::new();
    }
    "\nSTUB-FIRST (you own MULTIPLE non-entry files): do NOT run ls/tree/find or read every dependency before \
     producing — a weak worker that explores first burns its budget and is KILLED for over-reading before it \
     writes anything (a whole task lost). Your FIRST actions must be a `write` for EACH owned file emitting a \
     COMPILING STUB: the imports it needs plus every public function/class with its real signature and a `pass` \
     body. Once the files EXIST you are exempt from the over-read timeout — THEN read only the specific dependency \
     APIs you need (injected below under 'API of …') and fill each body with a focused `edit`. Never finish with a \
     `pass`/stub body still in place."
        .to_string()
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
        // Repair shard but one file EMPTY: the demand stands — "already exists" must not
        // suppress the one write an empty file needs (the 0dc8c297f tracer's addendum).
        std::fs::write(dir.join("app/b.py"), "").unwrap();
        assert!(multi_file_note(&owned, true, &dir).contains("MUST write EVERY one"));
        // Repair shard but one file missing: the demand stands (the missing file must appear).
        std::fs::remove_file(dir.join("app/b.py")).unwrap();
        assert!(multi_file_note(&owned, true, &dir).contains("MUST write EVERY one"));
        assert_eq!(multi_file_note(&owned[..1], true, &dir), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multifile_stub_note_fires_only_for_multifile_non_entry() {
        // Multi-file non-entry module (the plan-shopping case) -> stub-first note; entry,
        // single-file, disabled, and REPAIRING -> empty.
        let note = multifile_stub_note(
            &["recipes/plan.py".into(), "recipes/shopping.py".into()],
            true,
            false,
        );
        assert!(note.contains("STUB-FIRST") && note.contains("COMPILING STUB"));
        // A REPAIR shard's owned files are LIVE: no stub order may reach it — the r5 round-2
        // two-file shape (route table + handler body) must not be told to gut both.
        assert!(
            multifile_stub_note(
                &["app/ledgerd/__init__.py".into(), "app/httpapi.py".into()],
                true,
                true,
            )
            .is_empty(),
            "a repairing multi-file shard reads no stub-first order"
        );
        // A file set that includes the entry is covered by skeleton_note -> empty here.
        assert!(
            multifile_stub_note(&["pkg/cli.py".into(), "pkg/util.py".into()], true, false)
                .is_empty()
        );
        assert!(
            multifile_stub_note(&["pkg/__main__.py".into(), "pkg/x.py".into()], true, false)
                .is_empty()
        );
        // Single-file -> empty (skeleton-first was a wash on simple single-file tasks).
        assert!(multifile_stub_note(&["pkg/only.py".into()], true, false).is_empty());
        // Disabled -> empty.
        assert!(multifile_stub_note(&["a.py".into(), "b.py".into()], false, false).is_empty());
    }
}
