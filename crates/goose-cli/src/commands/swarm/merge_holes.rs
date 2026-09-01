//! MERGE HOLES — what THE SPLIT leaves empty, said aloud at the seams that decide over it.
//!
//! The works-prover (VA-079, 2026-09-01) read the shard completion gate: a shard is Done when
//! its README exists (`.swarm/shards/<module>/<shard>/README.md` is its one owned file); its
//! PIECES are never required, and a README-only shard went Done with `shard_note{pieces: []}`
//! and no absence event — the merger's brief then listed it beside the shards that built
//! something. The gate-auditor (VA-065) read the merger's dispatch seam: "when every shard is
//! Done" was a COMMENT, r6e dispatched its merger over `merge_dossier{pieces: 0, readmes_missing:
//! all 8}`, and nothing consumed that fact.
//!
//! This module names the holes — CODE's stat of the shard folders, never a shard's claim — as
//! events tick.py can print and as the GAP paragraph the merger reads. MILD throughout: nothing
//! here gates, retries, aborts or bounds; the merger is told what is missing and told to fill or
//! send out, exactly as its brief already says for an UNFINISHED item.
//!
//! The folder derivation mirrors `shards::build_merge_dossier` (`merger.folders[i]`, falling
//! back to `<SHARDS_DIR>/<module>/<shard>`), and a README "counts" by the same rule — it parses
//! to a `ShardNote` (`parse_shard_note`); a README with none of the four fields is a missing
//! handoff, as the dossier's `readmes_missing` already says.

use std::path::Path;

use goose_swarm::{MergerOf, ShardOf};

use super::shards::{parse_shard_note, SHARDS_DIR};

/// What CODE finds in one shard's folder at a merger seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ShardFolderState {
    pub(super) id: String,
    pub(super) folder: String,
    /// The README parses to a handoff (the dossier's rule; a field-less README is no handoff).
    pub(super) note_present: bool,
    /// Every file in the folder that is not the README, sorted.
    pub(super) pieces: Vec<String>,
}

pub(super) fn shard_folder_states(root: &Path, merger: &MergerOf) -> Vec<ShardFolderState> {
    merger
        .shards
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let folder = merger
                .folders
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("{SHARDS_DIR}/{}/{id}", merger.module));
            let dir = root.join(&folder);
            let note_present = std::fs::read_to_string(dir.join("README.md"))
                .ok()
                .as_deref()
                .and_then(parse_shard_note)
                .is_some();
            let mut pieces: Vec<String> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n != "README.md")
                .collect();
            pieces.sort();
            ShardFolderState {
                id: id.clone(),
                folder,
                note_present,
                pieces,
            }
        })
        .collect()
}

/// The two hole classes at a merger's dispatch: a shard with no handoff (its pieces, if any, are
/// the only record), and a shard whose handoff stands over an EMPTY folder (r6e's shape: Done on
/// the README alone). A shard with neither is in `readmes_missing` only — the README is the
/// louder absence and its pieces list is empty on its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DispatchGaps {
    pub(super) readmes_missing: Vec<String>,
    pub(super) pieces_absent: Vec<String>,
}

impl DispatchGaps {
    pub(super) fn is_empty(&self) -> bool {
        self.readmes_missing.is_empty() && self.pieces_absent.is_empty()
    }
}

pub(super) fn dispatch_gaps(states: &[ShardFolderState]) -> DispatchGaps {
    DispatchGaps {
        readmes_missing: states
            .iter()
            .filter(|s| !s.note_present)
            .map(|s| s.id.clone())
            .collect(),
        pieces_absent: states
            .iter()
            .filter(|s| s.note_present && s.pieces.is_empty())
            .map(|s| s.id.clone())
            .collect(),
    }
}

/// The paragraph appended to the merger's brief when the folders have holes: each hole named by
/// shard id and folder with what CODE found, then the one instruction the brief already gives
/// for an UNFINISHED item — fill it or send it out, and say which under FILLED / SENT_OUT. None
/// when there is nothing missing, so a clean split's brief is byte-identical.
pub(super) fn gap_paragraph(module: &str, states: &[ShardFolderState]) -> Option<String> {
    let gaps = dispatch_gaps(states);
    if gaps.is_empty() {
        return None;
    }
    let mut s = format!(
        "\n\nGAPS AT YOUR DISPATCH — CODE read `{module}`'s shard folders a moment ago; these are \
         FACTS about what is on disk, not the shards' claims:\n"
    );
    for st in states {
        if gaps.readmes_missing.iter().any(|id| id == &st.id) {
            s.push_str(&format!(
                "  - shard `{}` (folder `{}`): its README.md handoff is MISSING — {}\n",
                st.id,
                st.folder,
                if st.pieces.is_empty() {
                    "and the folder holds NO piece files; nothing of its part exists.".to_string()
                } else {
                    format!(
                        "its {} piece file(s) ({}) are the only record of what it built; read them \
                         as such.",
                        st.pieces.len(),
                        st.pieces.join(", ")
                    )
                }
            ));
        } else if gaps.pieces_absent.iter().any(|id| id == &st.id) {
            s.push_str(&format!(
                "  - shard `{}` (folder `{}`): README present but NO piece files — its part was \
                 never built; its README's PROVIDES is a promise, not a delivery.\n",
                st.id, st.folder
            ));
        }
    }
    s.push_str(
        "Each line above is a GAP in this module, the same as an UNFINISHED item: fill it yourself \
         if it is small, else send it out with one `MERGE_GAP:` line — and name it under FILLED or \
         SENT_OUT in MERGE.md. Never list a gap as merged.",
    );
    Some(s)
}

/// At a shard's completion: the `shard_note` row `record_shard_note` returned carries the
/// folder's `pieces`; an empty list is the README-only shard — Done by the deliverable gate
/// (the README is its owned file), delivered nothing. Named here, once, at the moment it
/// completes; `None` when pieces exist or the row never measured them (no key → nothing to say).
pub(super) fn shard_pieces_absent_event(
    shard: &ShardOf,
    task_id: &str,
    note_row: &serde_json::Value,
) -> Option<serde_json::Value> {
    let empty = note_row
        .get("pieces")
        .and_then(|p| p.as_array())
        .is_some_and(|p| p.is_empty());
    empty.then(|| {
        serde_json::json!({
            "event": "shard_pieces_absent",
            "module": shard.module,
            "shard": shard.shard,
            "task_id": task_id,
            "folder": shard.folder,
            "readme_present": note_row.get("shard_note").is_some_and(|n| !n.is_null()),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-merge-holes-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn merger(shards: &[&str]) -> MergerOf {
        MergerOf {
            module: "web-viz".into(),
            shards: shards.iter().map(|s| s.to_string()).collect(),
            folders: shards
                .iter()
                .map(|s| format!(".swarm/shards/web-viz/{s}"))
                .collect(),
            interface: Default::default(),
        }
    }

    const NOTE: &str =
        "PROVIDES: buildScene(data)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check\n";

    /// r6e's shape beside a real shard: `render` built a piece and wrote its handoff; `pick`
    /// wrote ONLY its README (Done by the deliverable gate); `labels` has a folder and nothing
    /// in it. The dispatch reads one delivered part, one pieces-absent gap, one README-missing
    /// gap — and the paragraph names the two gaps, never `render`.
    #[test]
    fn a_readme_only_shard_and_a_bare_folder_are_gaps_and_a_built_shard_is_not() {
        let root = tmp("gaps");
        for s in ["render", "pick", "labels"] {
            std::fs::create_dir_all(root.join(format!(".swarm/shards/web-viz/{s}"))).unwrap();
        }
        std::fs::write(root.join(".swarm/shards/web-viz/render/README.md"), NOTE).unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/render/render.js"),
            "export function buildScene(data) { return data; }\n",
        )
        .unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/pick/README.md"), NOTE).unwrap();

        let states = shard_folder_states(&root, &merger(&["render", "pick", "labels"]));
        assert_eq!(states[0].pieces, vec!["render.js".to_string()]);
        assert!(states[0].note_present);
        let gaps = dispatch_gaps(&states);
        assert_eq!(gaps.pieces_absent, vec!["pick".to_string()]);
        assert_eq!(gaps.readmes_missing, vec!["labels".to_string()]);

        let p = gap_paragraph("web-viz", &states).expect("two holes → a paragraph");
        assert!(
            p.contains("shard `pick`") && p.contains("NO piece files"),
            "{p}"
        );
        assert!(p.contains("shard `labels`") && p.contains("MISSING"), "{p}");
        assert!(
            !p.contains("shard `render`"),
            "a built shard is not a gap: {p}"
        );
        assert!(p.contains("FILLED or") && p.contains("MERGE_GAP"), "{p}");
    }

    /// A clean split appends nothing — the brief stays byte-identical.
    #[test]
    fn a_clean_split_has_no_paragraph() {
        let root = tmp("clean");
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/render")).unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/render/README.md"), NOTE).unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/render/render.js"), "x\n").unwrap();
        let states = shard_folder_states(&root, &merger(&["render"]));
        assert!(dispatch_gaps(&states).is_empty());
        assert_eq!(gap_paragraph("web-viz", &states), None);
    }

    /// A field-less README is no handoff (the dossier's own `readmes_missing` rule).
    #[test]
    fn a_fieldless_readme_reads_as_missing() {
        let root = tmp("fieldless");
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/render")).unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/render/README.md"),
            "# render\nsome prose, no fields\n",
        )
        .unwrap();
        let states = shard_folder_states(&root, &merger(&["render"]));
        assert!(!states[0].note_present);
        assert_eq!(
            dispatch_gaps(&states).readmes_missing,
            vec!["render".to_string()]
        );
    }

    /// The completion event fires on `pieces: []` only — pieces present or unmeasured say nothing.
    #[test]
    fn the_pieces_absent_event_fires_on_an_empty_measured_list_only() {
        let shard = ShardOf {
            module: "web-viz".into(),
            shard: "pick".into(),
            folder: ".swarm/shards/web-viz/pick".into(),
            ..Default::default()
        };
        let ev = shard_pieces_absent_event(
            &shard,
            "web-viz-pick",
            &serde_json::json!({"pieces": [], "shard_note": {"provides": ["readPickAt"]}}),
        )
        .expect("an empty measured list is the absence");
        assert_eq!(ev["event"], "shard_pieces_absent");
        assert_eq!(ev["module"], "web-viz");
        assert_eq!(ev["shard"], "pick");
        assert_eq!(ev["task_id"], "web-viz-pick");
        assert_eq!(ev["readme_present"], true);
        assert!(shard_pieces_absent_event(
            &shard,
            "web-viz-pick",
            &serde_json::json!({"pieces": ["pick.js"]})
        )
        .is_none());
        assert!(
            shard_pieces_absent_event(&shard, "web-viz-pick", &serde_json::json!({})).is_none(),
            "an unmeasured list is not an absence"
        );
    }
}
