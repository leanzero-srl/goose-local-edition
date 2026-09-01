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

use super::shards::{
    parse_fields, parse_merge_gaps, parse_shard_note, MERGE_FIELDS, MERGE_README, SHARDS_DIR,
};

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

/// At the merger's dispatch (VA-065): the dossier's `readmes_missing` had no consumer — r6e's
/// merger dispatched over `merge_dossier{pieces: 0, readmes_missing: all 8}` and the fact sat in
/// run.jsonl unread while the merger was told to merge. `merge_dossier_incomplete{module, task_id,
/// missing, pieces_absent}` is the loud form, for tick.py to print; the GAP paragraph beside it is
/// what the merger reads. MILD: no gate — the scheduler's relax rule (VA-064) and the gap door own
/// what becomes of a hole. None when the split is clean.
pub(super) fn dispatch_incomplete_event(
    module: &str,
    task_id: &str,
    gaps: &DispatchGaps,
) -> Option<serde_json::Value> {
    (!gaps.is_empty()).then(|| {
        serde_json::json!({
            "event": "merge_dossier_incomplete",
            "module": module,
            "task_id": task_id,
            "missing": gaps.readmes_missing,
            "pieces_absent": gaps.pieces_absent,
        })
    })
}

/// Does `text` name `name` as a whole token (no identifier character on either side)? The
/// same boundary rule `check_merge` uses for a referenced symbol; case-insensitive because a
/// merger writes "Pick" for `pick` as often as not.
fn mentions(text: &str, name: &str) -> bool {
    let (t, n) = (text.to_lowercase(), name.to_lowercase());
    if n.is_empty() {
        return false;
    }
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    t.match_indices(&n).any(|(i, _)| {
        let (before_text, rest) = t.split_at(i);
        let before = before_text.chars().next_back();
        let after = rest.split_at(n.len()).1.chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// At the merger's COMPLETION (VA-065, the refuter on 5f45e8ea0): `check_merge` reconciles every
/// README's UNFINISHED item against MERGE.md's FILLED / SENT_OUT and the `MERGE_GAP:` lines —
/// but a shard that built NOTHING has no README item to reconcile, so a merger that neither
/// filled nor sent out that part went Done (on a prior hint alone, if need be) and the sink
/// integrated a module with a hole, silently. This is that reconciliation for the EMPTY shards:
/// a shard whose folder holds no piece file is a hole unless FILLED, SENT_OUT or a `MERGE_GAP:`
/// line names it (its id or its folder's last segment, as a whole token). `readmes_missing` rides
/// along as the handoff-less list. None when no shard is empty or every empty one is explained;
/// the merger's Done is never touched.
pub(super) fn merge_hole_event(
    root: &Path,
    merger: &MergerOf,
    task_id: &str,
    final_text: &str,
) -> Option<serde_json::Value> {
    let states = shard_folder_states(root, merger);
    if states.iter().all(|s| !s.pieces.is_empty()) {
        return None;
    }
    let merge_md = std::fs::read_to_string(
        root.join(SHARDS_DIR)
            .join(&merger.module)
            .join(MERGE_README),
    )
    .ok();
    let mut explained: Vec<String> = merge_md
        .as_deref()
        .and_then(|t| parse_fields(t, &MERGE_FIELDS))
        .into_iter()
        .flat_map(|fields| {
            // 2 = FILLED, 3 = SENT_OUT (MERGE_FIELDS' order).
            fields
                .into_iter()
                .skip(2)
                .take(2)
                .flatten()
                .collect::<Vec<_>>()
        })
        .collect();
    explained.extend(parse_merge_gaps(final_text));
    let shards_missing: Vec<String> = states
        .iter()
        .filter(|s| s.pieces.is_empty())
        .filter(|s| {
            let tail = s.folder.rsplit('/').next().unwrap_or(s.folder.as_str());
            !explained
                .iter()
                .any(|e| mentions(e, &s.id) || mentions(e, tail))
        })
        .map(|s| s.id.clone())
        .collect();
    (!shards_missing.is_empty()).then(|| {
        serde_json::json!({
            "event": "merge_hole",
            "module": merger.module,
            "task_id": task_id,
            "shards_missing": shards_missing,
            "readmes_missing": states.iter().filter(|s| !s.note_present).map(|s| s.id.clone()).collect::<Vec<_>>(),
            "merge_readme_present": merge_md.is_some(),
        })
    })
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

    /// VA-065 at dispatch: the holes are said as one event, none when clean.
    #[test]
    fn the_dispatch_event_carries_both_hole_classes_and_is_absent_when_clean() {
        let gaps = DispatchGaps {
            readmes_missing: vec!["labels".into()],
            pieces_absent: vec!["pick".into()],
        };
        let ev = dispatch_incomplete_event("web-viz", "web-viz", &gaps).expect("holes → event");
        assert_eq!(ev["event"], "merge_dossier_incomplete");
        assert_eq!(ev["missing"], serde_json::json!(["labels"]));
        assert_eq!(ev["pieces_absent"], serde_json::json!(["pick"]));
        assert!(
            dispatch_incomplete_event("web-viz", "web-viz", &DispatchGaps::default()).is_none()
        );
    }

    /// VA-065 at completion (the refuter's hole): `pick` built nothing and MERGE.md explains
    /// nothing → `merge_hole{shards_missing: [pick]}`; `render` built its piece and is never a
    /// hole. Naming `pick` under SENT_OUT, or a `MERGE_GAP:` line in the final message, closes it.
    #[test]
    fn an_empty_shard_neither_filled_nor_sent_out_is_a_merge_hole() {
        let root = tmp("hole");
        for s in ["render", "pick"] {
            std::fs::create_dir_all(root.join(format!(".swarm/shards/web-viz/{s}"))).unwrap();
        }
        std::fs::write(root.join(".swarm/shards/web-viz/render/README.md"), NOTE).unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/render/render.js"), "x\n").unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/pick/README.md"), NOTE).unwrap();
        let m = merger(&["render", "pick"]);
        let merge_md = root.join(".swarm/shards/web-viz/MERGE.md");
        std::fs::write(
            &merge_md,
            "KEPT: render\nDROPPED: none\nFILLED: none\nSENT_OUT: none\n",
        )
        .unwrap();

        let ev = merge_hole_event(&root, &m, "web-viz", "merged.").expect("pick is a hole");
        assert_eq!(ev["event"], "merge_hole");
        assert_eq!(ev["shards_missing"], serde_json::json!(["pick"]));
        assert_eq!(ev["readmes_missing"], serde_json::json!([]));
        assert_eq!(ev["merge_readme_present"], true);

        std::fs::write(&merge_md, "KEPT: render\nDROPPED: none\nFILLED: none\nSENT_OUT: Pick — readPickAt still to build\n").unwrap();
        assert!(
            merge_hole_event(&root, &m, "web-viz", "merged.").is_none(),
            "SENT_OUT names it"
        );

        std::fs::write(
            &merge_md,
            "KEPT: render\nDROPPED: none\nFILLED: none\nSENT_OUT: none\n",
        )
        .unwrap();
        assert!(
            merge_hole_event(
                &root,
                &m,
                "web-viz",
                "done\nMERGE_GAP: pick's readPickAt(sx, sy)\n"
            )
            .is_none(),
            "a MERGE_GAP line names it"
        );
        assert!(
            merge_hole_event(&root, &m, "web-viz", "MERGE_GAP: the picker\n").is_some(),
            "`picker` is not the token `pick`"
        );
    }

    /// No MERGE.md at all: an empty shard is still a hole (and the event says the README is absent);
    /// a split where every shard built something has no hole to name.
    #[test]
    fn a_missing_merge_readme_does_not_explain_a_hole_and_a_built_split_has_none() {
        let root = tmp("hole-no-md");
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/labels")).unwrap();
        let m = merger(&["labels"]);
        let ev = merge_hole_event(&root, &m, "web-viz", "").expect("bare folder, nothing said");
        assert_eq!(ev["shards_missing"], serde_json::json!(["labels"]));
        assert_eq!(ev["readmes_missing"], serde_json::json!(["labels"]));
        assert_eq!(ev["merge_readme_present"], false);
        std::fs::write(root.join(".swarm/shards/web-viz/labels/labels.js"), "x\n").unwrap();
        assert!(merge_hole_event(&root, &m, "web-viz", "").is_none());
    }
}
