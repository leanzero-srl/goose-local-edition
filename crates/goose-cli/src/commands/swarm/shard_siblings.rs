//! VA-106: at a SHARD's dispatch, what its module's SIBLING shards have already LANDED — the
//! folder, the piece files with their byte counts, and the README fields the sibling actually
//! wrote — so the receiver designs against what DOES exist, not only against the split's declared
//! names. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases).
//!
//! MEASURED (r6h, 2026-09-02): `viz-engine-debug-api` was dispatched at 03:39:30.266824Z, 278 µs
//! after its sibling `camera-labels-brush`'s ledger row landed (`ledger_written`
//! 03:39:30.266546Z) with a README of 11 typed PROVIDES (`viz3d.toggleBrush(id: string): void`,
//! `viz3d.brush(): string[]`, …), 10 ASSUMES and 3 WRITES beside five parse-checked pieces. Its
//! brief carried the split's DECLARED interface only; at 51k reasoning chars it wrote "the sibling
//! could define either a single viz3d object or 4 functions. I can't know exactly" and "if the
//! sibling defines viz3d differently … my code breaks at runtime"; its first calls were `ls`,
//! `node --version`, `ls web`, and it opened the sibling README at 07:05:21 — 26 minutes in —
//! because an `ls` happened to show it.
//!
//! WHY the dep_block does not cover this: shards of one module have no DAG edge between them (they
//! run in parallel by design), so `dep_sources::dependency_sources_block` — the REAL source of
//! every plan file the task depends on — has no arm for a parallel sibling, and a shard's owned
//! file is its README, not a source file. The engine's rule "workers read real dependency
//! sources" needed this arm.
//!
//! SOURCE OF TRUTH: the sibling's ledger row (`record_shard_note` → `record_task_ledger`, written
//! INSIDE the sibling's `run_task_inner` before it returns, so the row is on disk before the
//! scheduler can re-dispatch onto the freed device — a happens-before, not a race) and its folder
//! on disk. Facts only: the PROVIDES / WRITES / UNFINISHED lines verbatim, the ASSUMES lines that
//! name THIS shard's cluster, the piece paths with byte counts. The one instruction is the
//! heading's: read them before designing against the declared names. MILD: nothing here refuses
//! a dispatch or a name.
//!
//! Loud absences (gate 1): a `done` row without a `shard_note` is listed with its README named
//! absent or unparsed — never skipped; an unreadable ledger file is named in
//! `ledger_rows_unreadable`; a sibling with no row yet is `pending` with `no ledger row`; and when
//! nothing has landed the event is `shard_siblings_none{pending}` and the brief carries the
//! one-line pointer to where a landing will appear. The merger needs none of this: its dossier
//! (`shards::merger_brief`) already renders every shard's pieces, PROVIDES and ASSUMES.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use goose_swarm::ShardOf;

use super::shards::{README_FIELDS, SHARDS_DIR};
use super::LEDGER_DIR;

/// The handoff fields a sibling's row carries (`shard_note`), with where they were read from
/// (`shard_note_source`: `README.md` or `final_message`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SiblingNote {
    pub(super) source: String,
    pub(super) provides: Vec<String>,
    pub(super) assumes: Vec<String>,
    pub(super) unfinished: Vec<String>,
    pub(super) writes: Vec<String>,
}

/// A sibling shard whose ledger row says `status: done`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LandedSibling {
    pub(super) shard: String,
    pub(super) folder: String,
    pub(super) folder_on_disk: bool,
    /// `README.md`'s size when the file is in the folder now.
    pub(super) readme_bytes: Option<u64>,
    /// None when the row carries no `shard_note` — the README (or final message) had none of the
    /// five field lines; `merge_note_missing` said so at the sibling's completion.
    pub(super) note: Option<SiblingNote>,
    /// (path from the tree root, bytes): every file in the folder but the README, sorted.
    pub(super) pieces: Vec<(String, u64)>,
    /// Piece names the row recorded at completion that are not in the folder now.
    pub(super) pieces_missing_on_disk: Vec<String>,
    /// The note's ASSUMES lines that name the receiving shard's cluster (`cluster_names`).
    pub(super) assumes_naming_you: Vec<String>,
}

impl LandedSibling {
    fn readme_state(&self) -> &'static str {
        match (&self.note, self.readme_bytes) {
            (Some(_), _) => "parsed",
            (None, Some(_)) => "unparsed",
            (None, None) => "absent",
        }
    }
}

/// A sibling with no `done` row: not dispatched, still running, or finished otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingSibling {
    pub(super) shard: String,
    /// `no ledger row`, or the row's `status` when a row exists that is not `done`.
    pub(super) ledger_status: String,
}

#[derive(Debug, Default)]
pub(super) struct SiblingsBlock {
    pub(super) module: String,
    pub(super) shard: String,
    /// The names an ASSUMES line is matched against (rendered so the receiver sees the filter).
    pub(super) cluster_names: Vec<String>,
    pub(super) landed: Vec<LandedSibling>,
    pub(super) pending: Vec<PendingSibling>,
    /// `<path>: <error>` per ledger file that could not be read or parsed.
    pub(super) ledger_rows_unreadable: Vec<String>,
    pub(super) text: String,
}

impl SiblingsBlock {
    /// `shard_siblings_delivered{task_id, module, shard, siblings, pending, …}` when at least one
    /// sibling landed; `shard_siblings_none{task_id, module, shard, pending, …}` otherwise. The
    /// vigil reads both.
    pub(super) fn event(&self, task_id: &str) -> serde_json::Value {
        let pending: Vec<serde_json::Value> = self
            .pending
            .iter()
            .map(|p| serde_json::json!({"shard": p.shard, "ledger_status": p.ledger_status}))
            .collect();
        if self.landed.is_empty() {
            return serde_json::json!({
                "event": "shard_siblings_none",
                "task_id": task_id,
                "module": self.module,
                "shard": self.shard,
                "pending": pending,
                "ledger_rows_unreadable": self.ledger_rows_unreadable,
            });
        }
        let siblings: Vec<serde_json::Value> = self
            .landed
            .iter()
            .map(|l| {
                let (provides, writes, unfinished, assumes_total) = match &l.note {
                    Some(n) => (
                        n.provides.clone(),
                        n.writes.clone(),
                        n.unfinished.clone(),
                        n.assumes.len(),
                    ),
                    None => (Vec::new(), Vec::new(), Vec::new(), 0),
                };
                serde_json::json!({
                    "shard": l.shard,
                    "folder": l.folder,
                    "folder_on_disk": l.folder_on_disk,
                    "readme": l.readme_state(),
                    "pieces": l.pieces.iter()
                        .map(|(p, b)| serde_json::json!({"path": p, "bytes": b}))
                        .collect::<Vec<_>>(),
                    "pieces_missing_on_disk": l.pieces_missing_on_disk,
                    "provides": provides,
                    "writes": writes,
                    "unfinished": unfinished,
                    "assumes_total": assumes_total,
                    "assumes_naming_you": l.assumes_naming_you,
                })
            })
            .collect();
        serde_json::json!({
            "event": "shard_siblings_delivered",
            "task_id": task_id,
            "module": self.module,
            "shard": self.shard,
            "cluster_names": self.cluster_names,
            "chars": self.text.len(),
            "siblings": siblings,
            "pending": pending,
            "ledger_rows_unreadable": self.ledger_rows_unreadable,
        })
    }
}

/// The block for `shard`'s dispatch: its module's siblings (every plan file under
/// `<SHARDS_DIR>/<module>/` in `all_files` — a shard's owned file is `<folder>/README.md`,
/// `shards::apply_module_split`) split into landed (a `done` ledger row) and pending, the landed
/// ones read from their row and folder, rendered.
pub(super) fn landed_siblings(root: &Path, shard: &ShardOf, all_files: &[String]) -> SiblingsBlock {
    let names = cluster_names(shard);
    let prefix = format!("{SHARDS_DIR}/{}/", shard.module);
    let universe: BTreeSet<String> = all_files
        .iter()
        .filter_map(|f| f.strip_prefix(prefix.as_str()))
        .filter_map(|rest| rest.split('/').next())
        .filter(|s| !s.is_empty() && *s != shard.shard)
        .map(str::to_string)
        .collect();

    let mut rows: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut unreadable: Vec<String> = Vec::new();
    let dir = root.join(LEDGER_DIR);
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries {
                let path = match entry {
                    Ok(e) => e.path(),
                    Err(e) => {
                        unreadable.push(format!("{}: {e}", dir.display()));
                        continue;
                    }
                };
                if path.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                let row: serde_json::Value = match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
                {
                    Ok(v) => v,
                    Err(e) => {
                        unreadable.push(format!("{}: {e}", path.display()));
                        continue;
                    }
                };
                let of = &row["shard_of"];
                let same_module = of["module"].as_str() == Some(shard.module.as_str());
                let Some(sib) = of["shard"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                else {
                    continue;
                };
                if !same_module || sib == shard.shard {
                    continue;
                }
                // Two rows for one shard name (the split's id-collision suffix): a `done` row is
                // the landing; never let a later non-done row hide it.
                let keep = match rows.get(&sib) {
                    Some(prev) => prev["status"].as_str() != Some("done"),
                    None => true,
                };
                if keep {
                    rows.insert(sib, row);
                }
            }
        }
        // No task has finished yet: the ledger directory does not exist. Honest-empty — the
        // block below says every sibling has no row.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => unreadable.push(format!("{}: {e}", dir.display())),
    }

    let mut landed: Vec<LandedSibling> = Vec::new();
    let mut pending: Vec<PendingSibling> = Vec::new();
    for sib in &universe {
        match rows.get(sib) {
            Some(row) if row["status"].as_str() == Some("done") => {
                landed.push(read_landed(root, shard, sib, row, &names));
            }
            Some(row) => pending.push(PendingSibling {
                shard: sib.clone(),
                ledger_status: row["status"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| "row without a status".to_string()),
            }),
            None => pending.push(PendingSibling {
                shard: sib.clone(),
                ledger_status: "no ledger row".to_string(),
            }),
        }
    }
    // A `done` row for a shard the manifest does not list (an id the plan renamed after the row
    // was written) is still a landing on disk; the disk outranks the manifest.
    for (sib, row) in &rows {
        if !universe.contains(sib) && row["status"].as_str() == Some("done") {
            landed.push(read_landed(root, shard, sib, row, &names));
        }
    }
    landed.sort_by(|a, b| a.shard.cmp(&b.shard));

    let text = render(shard, &names, &landed, &pending);
    SiblingsBlock {
        module: shard.module.clone(),
        shard: shard.shard.clone(),
        cluster_names: names,
        landed,
        pending,
        ledger_rows_unreadable: unreadable,
        text,
    }
}

fn str_list(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

fn read_landed(
    root: &Path,
    receiver: &ShardOf,
    sib: &str,
    row: &serde_json::Value,
    names: &[String],
) -> LandedSibling {
    // The row's folder, or the split's own layout rule (`apply_module_split`:
    // `<SHARDS_DIR>/<module>/<shard>`) when the row predates the field.
    let folder = row["shard_of"]["folder"]
        .as_str()
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{SHARDS_DIR}/{}/{sib}", receiver.module));
    let dir = root.join(&folder);
    let folder_on_disk = dir.is_dir();
    let readme_bytes = std::fs::metadata(dir.join("README.md"))
        .ok()
        .map(|m| m.len());
    let mut pieces: Vec<(String, u64)> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                if name == "README.md" {
                    return None;
                }
                let bytes = e.metadata().ok()?.len();
                Some((format!("{folder}/{name}"), bytes))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    pieces.sort();
    let pieces_missing_on_disk: Vec<String> = str_list(&row["pieces"])
        .into_iter()
        .filter(|n| !pieces.iter().any(|(p, _)| p == &format!("{folder}/{n}")))
        .collect();
    let note = row
        .get("shard_note")
        .filter(|n| n.is_object())
        .map(|n| SiblingNote {
            source: row["shard_note_source"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| "unrecorded".to_string()),
            provides: str_list(&n["provides"]),
            assumes: str_list(&n["assumes"]),
            unfinished: str_list(&n["unfinished"]),
            writes: str_list(&n["writes"]),
        });
    let assumes_naming_you = match &note {
        Some(n) => n
            .assumes
            .iter()
            .filter(|a| names.iter().any(|name| word_occurs(a, name)))
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    LandedSibling {
        shard: sib.to_string(),
        folder,
        folder_on_disk,
        readme_bytes,
        note,
        pieces,
        pieces_missing_on_disk,
        assumes_naming_you,
    }
}

fn render(
    shard: &ShardOf,
    names: &[String],
    landed: &[LandedSibling],
    pending: &[PendingSibling],
) -> String {
    let module = &shard.module;
    let pending_list = || {
        pending
            .iter()
            .map(|p| format!("`{}` ({})", p.shard, p.ledger_status))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if landed.is_empty() {
        if pending.is_empty() {
            return String::new();
        }
        return format!(
            "## SIBLING SHARDS ALREADY LANDED — none yet\n\
             None of the {n} sibling shard(s) of module `{module}` has a `done` ledger row yet: \
             {list}. When one lands, its folder is `{SHARDS_DIR}/{module}/<shard>/` — its README.md \
             carries the fields it actually implemented, its piece files beside it.\n\n",
            n = pending.len(),
            list = pending_list(),
        );
    }
    let pending_part = if pending.is_empty() {
        String::new()
    } else {
        format!("; still building: {}", pending_list())
    };
    let mut s = format!(
        "## SIBLING SHARDS ALREADY LANDED — read these before designing against the declared names\n\
         The declared interface says what MUST exist; these say what DOES. {landed} of {total} \
         sibling shard(s) of module `{module}` landed{pending_part}.\n",
        landed = landed.len(),
        total = landed.len() + pending.len(),
    );
    let fields = README_FIELDS.join("/");
    let names_list = names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ");
    for l in landed {
        let readme_part = match (&l.note, l.readme_bytes) {
            (Some(_), Some(b)) => format!(" (README.md, {b} bytes)"),
            (Some(n), None) => format!(
                " (README.md absent on disk — the fields below are from its {})",
                n.source
            ),
            (None, Some(b)) => format!(
                " (README.md present, {b} bytes, but none of {fields} parsed — read it whole)"
            ),
            (None, None) => " (README.md absent — no handoff fields exist for it)".to_string(),
        };
        s.push_str(&format!(
            "\n### `{}` — `{}/`{readme_part}\n",
            l.shard, l.folder
        ));
        if !l.folder_on_disk {
            s.push_str("The folder is NOT on disk now — its row says it landed; nothing is there to read.\n");
        }
        if l.pieces.is_empty() {
            s.push_str("Pieces: none in the folder\n");
        } else {
            s.push_str(&format!(
                "Pieces: {}\n",
                l.pieces
                    .iter()
                    .map(|(p, b)| format!("`{p}` ({b} bytes)"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !l.pieces_missing_on_disk.is_empty() {
            s.push_str(&format!(
                "Pieces its row recorded that are no longer in the folder: {}\n",
                l.pieces_missing_on_disk.join(", ")
            ));
        }
        let Some(n) = &l.note else {
            continue;
        };
        if n.provides.is_empty() {
            s.push_str(&format!("{}: none declared\n", README_FIELDS[0]));
        }
        for p in &n.provides {
            s.push_str(&format!("{}: {p}\n", README_FIELDS[0]));
        }
        if n.writes.is_empty() {
            s.push_str(&format!("{}: none\n", README_FIELDS[4]));
        }
        for w in &n.writes {
            s.push_str(&format!("{}: {w}\n", README_FIELDS[4]));
        }
        for u in &n.unfinished {
            s.push_str(&format!("{}: {u}\n", README_FIELDS[2]));
        }
        if l.assumes_naming_you.is_empty() {
            s.push_str(&format!(
                "{a} naming your cluster ({names_list}): none of its {n}\n",
                a = README_FIELDS[1],
                n = n.assumes.len(),
            ));
        } else {
            s.push_str(&format!(
                "{a} it makes about YOUR cluster ({names_list}) — {k} of its {n}:\n",
                a = README_FIELDS[1],
                k = l.assumes_naming_you.len(),
                n = n.assumes.len(),
            ));
            for a in &l.assumes_naming_you {
                s.push_str(&format!("{}: {a}\n", README_FIELDS[1]));
            }
        }
    }
    s.push('\n');
    s
}

/// The names by which a sibling's README refers to THIS shard's cluster: its id and every run of
/// two or more of its hyphen segments (r6h's camera README called `data-stream-render-pick`
/// "render-pick" and "data-stream"; a single segment like `pick` would match prose), plus every
/// declared export — its head (`vs7dbg` of `vs7dbg.setCamera`) or last segment — that this
/// shard's split text names (debug-api's "Wire window.vs7dbg …", "own boot-time assembly" →
/// `vs7dbg`, `boot`). Over-inclusion hands the receiver one more sibling fact; under-inclusion
/// hides one, so the rule leans wide.
pub(super) fn cluster_names(shard: &ShardOf) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let segs: Vec<&str> = shard.shard.split('-').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        names.push(shard.shard.clone());
    }
    for i in 0..segs.len() {
        for j in i + 2..=segs.len() {
            names.push(segs[i..j].join("-"));
        }
    }
    let split_words: BTreeSet<&str> = shard
        .responsibility
        .split(|c: char| !is_ident_char(c))
        .filter(|w| !w.is_empty())
        .collect();
    for e in &shard.interface.exports {
        let head = e.name.split('.').find(|p| !p.is_empty());
        let last = e.name.rsplit('.').find(|p| !p.is_empty());
        for cand in [head, last].into_iter().flatten() {
            if split_words.contains(cand) && !names.iter().any(|n| n == cand) {
                names.push(cand.to_string());
            }
        }
    }
    names
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A word character for the ASSUMES match: identifier characters AND `-`, so `render-pick` is not
/// found inside `data-stream-render-pick` and `render` would not be found inside `render-pick`.
fn is_word_char(c: char) -> bool {
    is_ident_char(c) || c == '-'
}

fn word_occurs(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = text[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let bounded_before = !text[..start].chars().next_back().is_some_and(is_word_char);
        let bounded_after = !text[end..].chars().next().is_some_and(is_word_char);
        if bounded_before && bounded_after {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::{DeclaredExport, ModuleInterface};
    use std::path::PathBuf;

    /// r6h's camera shard as it landed (2026-09-02 03:39:30Z): its README and its ledger row,
    /// copied from the run tree byte for byte; the five pieces by name and size.
    const R6H_CAMERA_README: &str = include_str!("testdata/va106/camera-labels-brush-README.md");
    const R6H_CAMERA_ROW: &str = include_str!("testdata/va106/viz-engine-camera-labels-brush.json");
    const R6H_CAMERA_PIECES: [(&str, usize); 5] = [
        ("brush.js", 5339),
        ("camera-input.js", 3531),
        ("camera-state.js", 5043),
        ("labels.js", 5461),
        ("project.js", 2963),
    ];
    const CAMERA_FOLDER: &str = ".swarm/shards/viz-engine/camera-labels-brush";
    const RENDER_PICK_FOLDER: &str = ".swarm/shards/viz-engine/data-stream-render-pick";
    /// debug-api's `shard_of.responsibility` from r6h's plan, verbatim.
    const R6H_DEBUG_API_RESPONSIBILITY: &str = "Wire window.vs7dbg as the synchronous, truthful \
        graded facade over all other shards and own boot-time assembly, carrying the cross-cutting \
        contracts (section 8 overview, performance budgets, rules) that the whole module must satisfy.";
    /// The module's declared export names from r6h's plan, verbatim and in order.
    const R6H_EXPORTS: [&str; 30] = [
        "viz3d.toggleBrush",
        "viz3d.clearBrush",
        "viz3d.brush",
        "viz3d.onBrush",
        "vs7dbg.layout",
        "vs7dbg.sceneDigest",
        "vs7dbg.camera",
        "vs7dbg.setCamera",
        "vs7dbg.pick",
        "vs7dbg.pickPixel",
        "vs7dbg.brush",
        "vs7dbg.frames",
        "loadRecords",
        "applyBatch",
        "heightFor",
        "topColorRGB",
        "onStreamMessage",
        "initViz",
        "renderFrame",
        "requestRender",
        "pickCore",
        "pickPixelCore",
        "setPanelState",
        "bindClickInput",
        "project",
        "getCamera",
        "setCameraCore",
        "bindCameraInput",
        "updateLabels",
        "boot",
    ];

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shard-siblings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn r6h_shard(shard: &str, responsibility: &str) -> ShardOf {
        ShardOf {
            module: "viz-engine".into(),
            shard: shard.into(),
            folder: format!("{SHARDS_DIR}/viz-engine/{shard}"),
            responsibility: responsibility.into(),
            interface: ModuleInterface {
                exports: R6H_EXPORTS
                    .iter()
                    .map(|n| DeclaredExport {
                        name: n.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            module_files: vec!["web/viz.js".into()],
        }
    }

    /// The scheduler's `all_files`: every task's owned files, sorted and deduped.
    fn r6h_all_files() -> Vec<String> {
        [
            format!("{CAMERA_FOLDER}/README.md"),
            format!("{RENDER_PICK_FOLDER}/README.md"),
            format!("{SHARDS_DIR}/viz-engine/debug-api/README.md"),
            "app/db.py".to_string(),
            "web/index.html".to_string(),
            "web/viz.js".to_string(),
        ]
        .to_vec()
    }

    fn land_camera(root: &Path) {
        let dir = root.join(CAMERA_FOLDER);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), R6H_CAMERA_README).unwrap();
        for (name, bytes) in R6H_CAMERA_PIECES {
            std::fs::write(dir.join(name), "x".repeat(bytes)).unwrap();
        }
        std::fs::create_dir_all(root.join(LEDGER_DIR)).unwrap();
        std::fs::write(
            root.join(LEDGER_DIR)
                .join("viz-engine-camera-labels-brush.json"),
            R6H_CAMERA_ROW,
        )
        .unwrap();
    }

    /// THE r6h CASE: debug-api dispatched after camera landed. Its brief now carries the heading,
    /// camera's eleven PROVIDES verbatim (`viz3d.toggleBrush(id: string): void` — the thing it
    /// spent 26 minutes unable to know), the five piece paths with their sizes, the WRITES, and
    /// exactly the two ASSUMES that address debug-api's cluster (`debug-api`, `vs7dbg`, `boot`):
    /// the vs7dbg.setCamera line and the "initBrushFlagBuffer() is wired once by boot" line —
    /// never the render-pick lines. render-pick, still building, is named pending.
    #[test]
    fn r6h_debug_api_dispatch_carries_the_landed_camera_shard() {
        let root = scratch("debug-api");
        land_camera(&root);
        let shard = r6h_shard("debug-api", R6H_DEBUG_API_RESPONSIBILITY);
        assert_eq!(cluster_names(&shard), ["debug-api", "vs7dbg", "boot"]);

        let b = landed_siblings(&root, &shard, &r6h_all_files());
        assert_eq!(b.landed.len(), 1, "pending: {:?}", b.pending);
        assert!(
            b.ledger_rows_unreadable.is_empty(),
            "{:?}",
            b.ledger_rows_unreadable
        );
        assert!(b.text.starts_with(
            "## SIBLING SHARDS ALREADY LANDED — read these before designing against the declared names\n\
             The declared interface says what MUST exist; these say what DOES. 1 of 2 sibling \
             shard(s) of module `viz-engine` landed; still building: `data-stream-render-pick` (no \
             ledger row).\n\n### `camera-labels-brush` — `.swarm/shards/viz-engine/camera-labels-brush/` \
             (README.md, 5546 bytes)\nPieces: "
        ), "{}", b.text);
        assert!(b.text.contains(
            "PROVIDES: viz3d.toggleBrush(id: string): void — toggle in ONE shared set, changed flag \
             bytes uploaded (≤ stride+4096, no realloc), fires onBrush with ascending ids, requestRender\n"
        ));
        assert!(b
            .text
            .contains("PROVIDES: viz3d.brush(): string[] — shared brush set"));
        for (name, bytes) in R6H_CAMERA_PIECES {
            let want = format!("`{CAMERA_FOLDER}/{name}` ({bytes} bytes)");
            assert!(b.text.contains(&want), "missing {want} in:\n{}", b.text);
        }
        assert!(b
            .text
            .contains("WRITES: brushSet — Set<string> of brushed record ids; the ONE shared set"));
        assert!(b.text.contains(
            "ASSUMES it makes about YOUR cluster (`debug-api`, `vs7dbg`, `boot`) — 2 of its 10:\n\
             ASSUMES: render-pick's renderFrame() calls updateLabels() exactly once"
        ));
        let l = &b.landed[0];
        assert_eq!(l.assumes_naming_you.len(), 2, "{:#?}", l.assumes_naming_you);
        assert!(l.assumes_naming_you[0].contains("debug-api's vs7dbg.setCamera = setCameraCore()"));
        assert!(l.assumes_naming_you[1].contains("wired once by boot or first-render"));
        assert!(!b
            .text
            .contains("ASSUMES: render-pick declares requestRender()"));
        assert!(
            !b.text.contains("UNFINISHED:"),
            "camera's UNFINISHED is `none`"
        );

        let ev = b.event("viz-engine-debug-api");
        assert_eq!(ev["event"], "shard_siblings_delivered");
        assert_eq!(ev["module"], "viz-engine");
        assert_eq!(ev["shard"], "debug-api");
        assert_eq!(ev["siblings"].as_array().unwrap().len(), 1);
        assert_eq!(ev["siblings"][0]["shard"], "camera-labels-brush");
        assert_eq!(ev["siblings"][0]["readme"], "parsed");
        assert_eq!(ev["siblings"][0]["folder_on_disk"], true);
        assert_eq!(ev["siblings"][0]["pieces"].as_array().unwrap().len(), 5);
        assert_eq!(
            ev["siblings"][0]["pieces"][0]["path"],
            format!("{CAMERA_FOLDER}/brush.js")
        );
        assert_eq!(ev["siblings"][0]["pieces"][0]["bytes"], 5339);
        assert_eq!(ev["siblings"][0]["provides"].as_array().unwrap().len(), 11);
        assert_eq!(ev["siblings"][0]["assumes_total"], 10);
        assert_eq!(
            ev["siblings"][0]["assumes_naming_you"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(ev["pending"][0]["shard"], "data-stream-render-pick");
        assert_eq!(ev["pending"][0]["ledger_status"], "no ledger row");
        assert_eq!(ev["chars"], b.text.len());
    }

    /// The same camera README received by render-pick: the filter picks the eight ASSUMES that
    /// name `render-pick`, `data-stream`, or an export its split names (requestRender,
    /// invalidatePick via "render-pick marks…", initViz, pickCore, applyBatch) and leaves the two
    /// about shared-state shapes and index.html.
    #[test]
    fn render_pick_receives_the_assumes_that_name_its_cluster() {
        let root = scratch("render-pick");
        land_camera(&root);
        let shard = r6h_shard(
            "data-stream-render-pick",
            "Own the GL scene: initViz, renderFrame/requestRender demand rendering, the offscreen \
             picking FBO with pickCore/pickPixelCore, bindClickInput, and the data path \
             loadRecords/applyBatch/onStreamMessage.",
        );
        let names = cluster_names(&shard);
        for n in [
            "data-stream-render-pick",
            "render-pick",
            "data-stream",
            "initViz",
            "applyBatch",
        ] {
            assert!(names.iter().any(|x| x == n), "{n} missing from {names:?}");
        }
        assert!(
            !names.iter().any(|x| x == "pick"),
            "single segments are not names: {names:?}"
        );

        let b = landed_siblings(&root, &shard, &r6h_all_files());
        let l = &b.landed[0];
        assert_eq!(l.assumes_naming_you.len(), 8, "{:#?}", l.assumes_naming_you);
        assert!(l.assumes_naming_you[0].starts_with("render-pick declares requestRender()"));
        assert!(l
            .assumes_naming_you
            .iter()
            .any(|a| a.starts_with("GL context variable is named gl")));
        assert!(l
            .assumes_naming_you
            .iter()
            .any(|a| a.starts_with("data-stream's applyBatch")));
        assert!(!l
            .assumes_naming_you
            .iter()
            .any(|a| a.starts_with("records {count")));
        assert!(!l
            .assumes_naming_you
            .iter()
            .any(|a| a.starts_with("index.html provides #viz3d")));
        assert_eq!(b.pending[0].shard, "debug-api");
    }

    /// Nothing landed yet (the first shard of a module to be dispatched): no ledger directory
    /// exists, the event is `shard_siblings_none` naming every sibling as `no ledger row`, and
    /// the brief carries the one-line pointer to where a landing will appear.
    #[test]
    fn no_landed_sibling_is_a_named_absence() {
        let root = scratch("none");
        let shard = r6h_shard("camera-labels-brush", "Camera, labels and brush.");
        let b = landed_siblings(&root, &shard, &r6h_all_files());
        assert!(b.landed.is_empty());
        assert_eq!(b.pending.len(), 2);
        assert!(b
            .text
            .starts_with("## SIBLING SHARDS ALREADY LANDED — none yet\n"));
        assert!(b.text.contains(
            "None of the 2 sibling shard(s) of module `viz-engine` has a `done` ledger row yet: \
             `data-stream-render-pick` (no ledger row), `debug-api` (no ledger row)."
        ));
        assert!(b
            .text
            .contains("its folder is `.swarm/shards/viz-engine/<shard>/`"));
        let ev = b.event("viz-engine-camera-labels-brush");
        assert_eq!(ev["event"], "shard_siblings_none");
        assert_eq!(ev["pending"].as_array().unwrap().len(), 2);
        assert!(ev["ledger_rows_unreadable"].as_array().unwrap().is_empty());
    }

    /// Absences are said, never skipped: a `done` row with no `shard_note` and no README on disk
    /// is listed as README absent with its pieces (and the piece its row recorded that is gone);
    /// a sibling whose row is `failed` is pending with that status; an unparseable ledger file is
    /// named in `ledger_rows_unreadable`.
    #[test]
    fn a_row_without_a_shard_note_is_listed_with_its_readme_absent() {
        let root = scratch("absent");
        land_camera(&root);
        let ledger = root.join(LEDGER_DIR);
        std::fs::write(
            ledger.join("viz-engine-camera-labels-brush.json"),
            R6H_CAMERA_ROW.replacen("\"status\": \"done\"", "\"status\": \"failed\"", 1),
        )
        .unwrap();
        let rp = root.join(RENDER_PICK_FOLDER);
        std::fs::create_dir_all(&rp).unwrap();
        let render_js = "export function requestRender() {}\n";
        std::fs::write(rp.join("render.js"), render_js).unwrap();
        std::fs::write(
            ledger.join("viz-engine-data-stream-render-pick.json"),
            serde_json::json!({
                "kind": "task",
                "task_id": "viz-engine-data-stream-render-pick",
                "status": "done",
                "shard_of": {"module": "viz-engine", "shard": "data-stream-render-pick", "folder": RENDER_PICK_FOLDER},
                "pieces": ["render.js", "gone.js"],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(ledger.join("broken.json"), "{not json").unwrap();

        let shard = r6h_shard("debug-api", R6H_DEBUG_API_RESPONSIBILITY);
        let b = landed_siblings(&root, &shard, &r6h_all_files());
        assert_eq!(b.landed.len(), 1);
        assert_eq!(b.landed[0].shard, "data-stream-render-pick");
        assert_eq!(b.landed[0].readme_state(), "absent");
        let want = format!(
            "### `data-stream-render-pick` — `{RENDER_PICK_FOLDER}/` (README.md absent — no handoff \
             fields exist for it)\nPieces: `{RENDER_PICK_FOLDER}/render.js` ({} bytes)\nPieces its row \
             recorded that are no longer in the folder: gone.js\n",
            render_js.len()
        );
        assert!(b.text.contains(&want), "want:\n{want}\nin:\n{}", b.text);
        assert!(
            !b.text.contains("PROVIDES:"),
            "no note → no field lines invented"
        );
        assert!(b
            .text
            .contains("still building: `camera-labels-brush` (failed)"));
        assert_eq!(
            b.ledger_rows_unreadable.len(),
            1,
            "{:?}",
            b.ledger_rows_unreadable
        );
        assert!(b.ledger_rows_unreadable[0].contains("broken.json"));
        let ev = b.event("viz-engine-debug-api");
        assert_eq!(ev["event"], "shard_siblings_delivered");
        assert_eq!(ev["siblings"][0]["readme"], "absent");
        assert_eq!(ev["siblings"][0]["pieces_missing_on_disk"][0], "gone.js");
        assert_eq!(ev["pending"][0]["ledger_status"], "failed");
        assert_eq!(ev["ledger_rows_unreadable"].as_array().unwrap().len(), 1);
    }

    /// A README on disk whose row has no note (none of the five field lines parsed) is
    /// "unparsed", named with its size and the fields it lacks — distinct from absent.
    #[test]
    fn a_readme_that_parsed_to_nothing_is_named_unparsed() {
        let root = scratch("unparsed");
        let rp = root.join(RENDER_PICK_FOLDER);
        std::fs::create_dir_all(&rp).unwrap();
        let prose = "# render-pick\nprose, no fields\n";
        std::fs::write(rp.join("README.md"), prose).unwrap();
        std::fs::create_dir_all(root.join(LEDGER_DIR)).unwrap();
        std::fs::write(
            root.join(LEDGER_DIR).join("viz-engine-data-stream-render-pick.json"),
            serde_json::json!({
                "task_id": "viz-engine-data-stream-render-pick",
                "status": "done",
                "shard_of": {"module": "viz-engine", "shard": "data-stream-render-pick", "folder": RENDER_PICK_FOLDER},
            })
            .to_string(),
        )
        .unwrap();
        let shard = r6h_shard("debug-api", R6H_DEBUG_API_RESPONSIBILITY);
        let b = landed_siblings(&root, &shard, &r6h_all_files());
        assert_eq!(b.landed[0].readme_state(), "unparsed");
        let want = format!(
            "(README.md present, {} bytes, but none of PROVIDES/ASSUMES/UNFINISHED/CHECKED_WITH/WRITES \
             parsed — read it whole)\nPieces: none in the folder\n",
            prose.len()
        );
        assert!(b.text.contains(&want), "want:\n{want}\nin:\n{}", b.text);
    }

    #[test]
    fn word_match_is_bounded_by_identifier_and_hyphen_characters() {
        assert!(word_occurs("render-pick's renderFrame()", "render-pick"));
        assert!(!word_occurs("data-stream-render-pick", "render-pick"));
        assert!(!word_occurs("render-pick", "render"));
        assert!(word_occurs("debug-api's vs7dbg.setCamera", "vs7dbg"));
        assert!(!word_occurs("written by initViz2", "initViz"));
        assert!(word_occurs("wired once by boot or first-render", "boot"));
        assert!(!word_occurs("anything", ""));
    }
}
