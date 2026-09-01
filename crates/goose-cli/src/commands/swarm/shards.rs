//! THE SPLIT (VA-021 / VA-024; Mihai 2026-09-01: "You have a series of 10 tasks and all of them
//! fall over to one node. WHY? Why can't another node pick it up? Why must it be done
//! sequentially? Why can't we find a way to edit a file in parallel?").
//!
//! MEASURED: r6c `web-viz` was ONE 39 KB file written in ONE model session — 519 minutes, the run's
//! long pole — because the opener routed SEVEN spec sections to a task owning ONE file; ledgerd-core
//! had the biggest brief (33.6k chars, 6 files) and finished first of the two, so brief chars is
//! the WRONG fatness proxy (tick.py's VA-024 replay). r5's `viz-field`: 11 sections, 1 file. The
//! synthesis prompt's "DEPENDENCIES ARE EXPENSIVE — every one of them idles a machine" biased
//! towards few fat tasks, and nothing could help a task once it was in progress.
//!
//! WHAT THIS DOES, in the one door every task enters the DAG through (`plan_slices_to_dag`, right
//! after the first `finalize_plan_before_dag`):
//!
//! 1. MEASURE — per task, spec sections claimed per owned file (and claimed chars per file). The
//!    threshold is derived from the plan's own distribution — mean + one standard deviation of
//!    sections-per-file across the measured tasks — never a literal; the median rides the event so
//!    the reader sees the multiple. A task above it is a loud `plan_flag{kind: fat_task, …}`.
//! 2. REQUEST — ONE split request to synthesis per fat task (a PATCH, invariant 3, never a
//!    re-emission): the planner DECLARES the module's interface as plan text — exported names,
//!    kinds, signatures, the shared-state shape, the assembly order — and partitions the claimed
//!    sections into SHARDS. Declining (unparseable, fewer than two shards) is allowed and loud
//!    (`split_declined`); the flag stays.
//! 3. PATCH — CODE builds the `PlanPatch`: N SHARD tasks, each owning only its temp folder's
//!    `README.md` under `.swarm/shards/<module>/<shard>/`, depending on nothing (shards of one
//!    module are independent by construction), whose brief carries the SAME declaration and the
//!    module's whole brief; the module task becomes the MERGER — it keeps the module's final
//!    file(s) and its planner deps and now depends on every shard. NOBODY writes the final file
//!    until the merger: that is what makes parallel work on one module safe (Mihai 11:3x). The
//!    declaration is written to NO stub file — the measured CONTRACTS harm was stub FILES on disk
//!    (2/3 and 3/6 unparseable), never the declaration. The patched plan walks
//!    `finalize_plan_before_dag` again (source `split`) so the shard tasks meet every repair.
//!
//! Sibling module under the incremental-split law; the retired test-only `split_fat_modules`
//! (files-per-role, frozen contract stubs) is deleted with this commit — the new split is measured
//! from spec sections, requested from the planner, and merged by a model with a code-built dossier.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use goose_swarm::{
    apply_patch, DeclaredExport, EventSink, MergerOf, ModuleInterface, PlanPatch, ShardOf, TaskAdd,
    TaskEdit,
};

use super::opener::OpenOutput;
use super::orientation::{heading_key, spec_sections};
use super::plan_shape::decomposition_of;
use super::skeleton::SKELETON_ID;
use super::{finalize_plan_before_dag, string_list, GooseAgentDispatcher};

/// Where a module's shards work. Under `.swarm/` on purpose: every tree lister, snapshot and
/// manifest already excludes it (`tree::SNAPSHOT_EXCLUDES`), so pieces never reach the scored tree
/// and never read as stray files — the merger reads them by path from its dossier.
pub(super) const SHARDS_DIR: &str = ".swarm/shards";

/// The opener's claim per slice: how many spec sections, and how many characters of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SectionClaim {
    pub(super) sections: usize,
    pub(super) chars: usize,
}

/// Slice id → its claimed sections, measured against the request's OWN sections (the same
/// `heading_key` match `briefs_from_slices` splices with). A claimed heading the request does not
/// carry counts as claimed-but-empty: it still names work the opener routed here.
pub(super) fn section_claims(opened: &OpenOutput, spec: &str) -> HashMap<String, SectionClaim> {
    let sections = spec_sections(spec);
    let chars_of: HashMap<String, usize> = sections
        .iter()
        .map(|s| (heading_key(&s.heading), s.body.chars().count()))
        .collect();
    opened
        .slices
        .iter()
        .map(|sl| {
            let chars = sl
                .sections
                .iter()
                .map(|h| chars_of.get(&heading_key(h)).copied().unwrap_or(0))
                .sum();
            (
                sl.id.clone(),
                SectionClaim {
                    sections: sl.sections.len(),
                    chars,
                },
            )
        })
        .collect()
}

/// One measured task: the plan's facts about how much spec lands on each file it owns.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TaskDensity {
    pub(super) id: String,
    pub(super) files: Vec<String>,
    pub(super) sections: usize,
    pub(super) section_chars: usize,
    pub(super) brief_chars: usize,
    pub(super) sections_per_file: f64,
    pub(super) chars_per_file: f64,
}

impl TaskDensity {
    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task": self.id,
            "files": self.files,
            "sections": self.sections,
            "section_chars": self.section_chars,
            "brief_chars": self.brief_chars,
            "sections_per_file": self.sections_per_file,
            "chars_per_file": self.chars_per_file,
        })
    }
}

/// The plan's density distribution and the tasks above its own threshold.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FatMeasure {
    pub(super) rows: Vec<TaskDensity>,
    pub(super) median: f64,
    pub(super) mean: f64,
    pub(super) stddev: f64,
    pub(super) threshold: f64,
    /// Indexes into `rows`, fattest first.
    pub(super) fat: Vec<usize>,
}

/// Measure every planner task — not the join, not the skeleton, not a task already split (a shard
/// or a merger), not a task owning nothing (rule (a) removes those) — and derive the fatness
/// threshold from the distribution: mean + one population standard deviation of
/// sections-per-file. No literal decides: a flat plan (stddev 0) flags nothing because no task is
/// strictly above its own mean; two tasks cannot flag (mean + stddev IS the max); r6c's six rows
/// put the threshold at 4.3 (web-viz 7.0 flagged, ledgerd-core 2.0 not) and r5's seven at 6.0
/// (viz-field 11.0 flagged, ledgerd-service 1.75 not).
pub(super) fn measure_fatness(
    plan: &serde_json::Value,
    claims: &HashMap<String, SectionClaim>,
) -> FatMeasure {
    let mut rows: Vec<TaskDensity> = Vec::new();
    for t in plan
        .get("subtasks")
        .and_then(|s| s.as_array())
        .into_iter()
        .flatten()
    {
        let Some(id) = t.get("id").and_then(|i| i.as_str()) else {
            continue;
        };
        if id == goose_swarm::SINK_ID
            || id == SKELETON_ID
            || t.get("shard_of").is_some()
            || t.get("merger_of").is_some()
        {
            continue;
        }
        let files = string_list(&t["files"]);
        if files.is_empty() {
            continue;
        }
        let slice = t
            .get("slice")
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(id);
        // A task no slice claims sections for (a patch-added task) measures as 0 sections — an
        // honest zero: the opener routed no spec to it.
        let claim = claims.get(slice).cloned().unwrap_or_default();
        let brief_chars = t
            .get("description")
            .and_then(|d| d.as_str())
            .map(|d| d.chars().count())
            .unwrap_or(0);
        let n = files.len() as f64;
        rows.push(TaskDensity {
            id: id.to_string(),
            files,
            sections: claim.sections,
            section_chars: claim.chars,
            brief_chars,
            sections_per_file: claim.sections as f64 / n,
            chars_per_file: claim.chars as f64 / n,
        });
    }
    let mut sorted: Vec<f64> = rows.iter().map(|r| r.sections_per_file).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    };
    let count = sorted.len() as f64;
    let mean = if sorted.is_empty() {
        0.0
    } else {
        sorted.iter().sum::<f64>() / count
    };
    let stddev = if sorted.is_empty() {
        0.0
    } else {
        (sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count).sqrt()
    };
    let threshold = mean + stddev;
    let mut fat: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.sections_per_file > threshold)
        .map(|(i, _)| i)
        .collect();
    fat.sort_by(|a, b| {
        rows[*b]
            .sections_per_file
            .partial_cmp(&rows[*a].sections_per_file)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    FatMeasure {
        rows,
        median,
        mean,
        stddev,
        threshold,
        fat,
    }
}

impl FatMeasure {
    /// One `plan_flag{kind: fat_task}` per fat task, carrying the distribution so the reader sees
    /// the multiple without recomputing it.
    pub(super) fn events(&self) -> Vec<serde_json::Value> {
        let distribution: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "task": r.id,
                    "files": r.files.len(),
                    "sections": r.sections,
                    "sections_per_file": r.sections_per_file,
                })
            })
            .collect();
        self.fat
            .iter()
            .map(|i| {
                let r = &self.rows[*i];
                let mut ev = r.to_json();
                if let Some(o) = ev.as_object_mut() {
                    o.insert("event".into(), "plan_flag".into());
                    o.insert("kind".into(), "fat_task".into());
                    o.insert("median".into(), self.median.into());
                    o.insert("mean".into(), self.mean.into());
                    o.insert("stddev".into(), self.stddev.into());
                    o.insert("threshold".into(), self.threshold.into());
                    o.insert("distribution".into(), serde_json::json!(distribution));
                }
                ev
            })
            .collect()
    }
}

/// What synthesis returns for one fat task: the module's declared interface and its shards.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub(super) struct ModuleSplit {
    #[serde(default)]
    pub(super) interface: ModuleInterface,
    #[serde(default)]
    pub(super) shards: Vec<ShardPlan>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub(super) struct ShardPlan {
    pub(super) id: String,
    #[serde(default)]
    pub(super) responsibility: String,
    #[serde(default)]
    pub(super) sections: Vec<String>,
    #[serde(default)]
    pub(super) provides: Vec<String>,
}

pub(super) fn split_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["interface", "shards"],
        "properties": {
            "interface": {
                "type": "object",
                "required": ["exports", "shared_state", "layout"],
                "properties": {
                    "exports": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["name", "kind", "signature", "purpose"],
                            "properties": {
                                "name": {"type": "string"},
                                "kind": {"type": "string"},
                                "signature": {"type": "string"},
                                "purpose": {"type": "string"}
                            }
                        }
                    },
                    "shared_state": {"type": "string"},
                    "layout": {"type": "array", "items": {"type": "string"}}
                }
            },
            "shards": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "responsibility", "sections", "provides"],
                    "properties": {
                        "id": {"type": "string"},
                        "responsibility": {"type": "string"},
                        "sections": {"type": "array", "items": {"type": "string"}},
                        "provides": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }
    })
}

/// The split request's instructions. The planner DECLARES and PARTITIONS; it writes no code and
/// no stub — the declaration is plan text every shard and the merger read.
pub(super) fn split_system_prompt() -> String {
    "You are the SYNTHESIS step, asked to SPLIT ONE FAT TASK so several machines build it in \
     parallel. The task below owns one module (its final file or files) and was handed several \
     spec sections for it — measured: more spec per file than any other task in this plan, which \
     makes it the run's long pole when one model session writes it alone.\n\n\
     Do TWO things and nothing else:\n\
     1. DECLARE THE MODULE'S INTERFACE as plan text: every exported/public name the module must \
        define (functions, classes, the debug/graded API's methods, event handlers wired at load), \
        each with its kind, exact signature (parameters, return shape) and one-line purpose; the \
        SHARED STATE shape every part reads or writes (object/record names and their fields and \
        types); and the LAYOUT — the final file(s) in assembly order (which regions come first: \
        constants, state, helpers, mechanisms, the exported API, boot). Names you declare are \
        BINDING on every shard and the merger, so declare real, complete signatures, not \
        placeholders.\n\
     2. PARTITION the task's spec sections into 2 or more SHARDS that can be written independently \
        and in parallel — usually one per mechanism or section group. Each shard: a short kebab-case \
        id, one sentence of responsibility, the exact spec section headings it implements (every \
        section of the task goes to exactly one shard), and the declared names it provides. Shards \
        write PIECES (functions/classes) in private folders; a MERGER assembles the final file from \
        them afterwards — so no shard needs another shard's file to exist.\n\n\
     Do not restate the spec and do not write code. Call the final_output tool once with \
     {interface: {exports: [{name, kind, signature, purpose}], shared_state, layout: []}, \
     shards: [{id, responsibility, sections: [], provides: []}]}."
        .to_string()
}

/// The split request's body: THIS task's facts — id, files, the measured density, the brief whole
/// (its claimed sections are spliced in it verbatim, so the planner partitions real headings).
pub(super) fn split_user_text(task: &serde_json::Value, density: &serde_json::Value) -> String {
    let id = task.get("id").and_then(|i| i.as_str()).unwrap_or("?");
    let files = string_list(&task["files"]);
    let desc = task
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    format!(
        "## The fat task\nid: `{id}`\nfiles (the module's FINAL files — the merger writes these; \
         shards write pieces in their own folders): {}\nmeasured: {} spec sections for {} file(s) = \
         {:.2} sections per file (plan median {:.2}, threshold {:.2}); brief {} chars\n\n## Its \
         brief (the spec sections it must implement are the `###` blocks)\n{desc}",
        files
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", "),
        density["sections"].as_u64().unwrap_or(0),
        files.len(),
        density["sections_per_file"].as_f64().unwrap_or(0.0),
        density["median"].as_f64().unwrap_or(0.0),
        density["threshold"].as_f64().unwrap_or(0.0),
        density["brief_chars"].as_u64().unwrap_or(0),
    )
}

/// Parse the planner's reply. Refuses (loudly, for `split_declined`) anything that cannot be
/// built into shards: no JSON, fewer than two shards, a shard without an id.
pub(super) fn parse_module_split(reply: &str) -> Result<ModuleSplit, String> {
    let v = super::parse_json_lenient(reply)
        .ok_or_else(|| "no JSON object in the reply".to_string())?;
    let split: ModuleSplit =
        serde_json::from_value(v).map_err(|e| format!("split is not the declared shape: {e}"))?;
    if split.shards.len() < 2 {
        return Err(format!(
            "{} shard(s) — a split needs at least two",
            split.shards.len()
        ));
    }
    if split
        .shards
        .iter()
        .any(|s| s.id.trim().is_empty() || s.responsibility.trim().is_empty())
    {
        return Err("a shard without an id or a responsibility".to_string());
    }
    Ok(split)
}

fn kebab(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

pub(super) fn render_interface(interface: &ModuleInterface) -> String {
    let mut s = String::new();
    if interface.exports.is_empty() {
        s.push_str("(synthesis declared no exports — implement the names your split's spec sections require and list every one in your README's PROVIDES)\n");
    }
    for e in &interface.exports {
        let DeclaredExport {
            name,
            kind,
            signature,
            purpose,
        } = e;
        s.push_str(&format!("- `{name}`"));
        if !kind.is_empty() {
            s.push_str(&format!(" ({kind})"));
        }
        if !signature.is_empty() {
            s.push_str(&format!(": `{signature}`"));
        }
        if !purpose.is_empty() {
            s.push_str(&format!(" — {purpose}"));
        }
        s.push('\n');
    }
    if !interface.shared_state.trim().is_empty() {
        s.push_str(&format!(
            "Shared state: {}\n",
            interface.shared_state.trim()
        ));
    }
    if !interface.layout.is_empty() {
        s.push_str(&format!(
            "Assembly order of the final file(s): {}\n",
            interface.layout.join(" → ")
        ));
    }
    s
}

/// The README every shard leaves — STRUCTURED, one line per item, so the engine and the merger
/// read it without guessing (S3 parses these lines into the handoff channel).
pub(super) const README_FIELDS: [&str; 4] = ["PROVIDES", "ASSUMES", "UNFINISHED", "CHECKED_WITH"];

/// A shard's brief, assembled by CODE from this run's facts: its split, its siblings' splits, the
/// declared interface, its folder, the README contract, and the module's whole brief (the settled
/// answers and the spec sections every shard shares — Mihai: trust the model with the information).
pub(super) fn shard_brief(
    module_id: &str,
    module_files: &[String],
    module_brief: &str,
    shard: &ShardPlan,
    siblings: &[ShardPlan],
    folder: &str,
    interface: &ModuleInterface,
) -> String {
    let final_files = module_files
        .iter()
        .map(|f| format!("`{f}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut s = format!(
        "SHARD `{shard_id}` OF MODULE `{module_id}` — one of {n} shards building {final_files} in \
         parallel on different machines.\n\n\
         WHERE YOU WORK: ONLY inside your folder `{folder}/` (create it). Write your PIECES there as \
         files in the module's language — the functions, classes and sections your split names, \
         e.g. `{folder}/<piece>.<ext>` — plus `{folder}/README.md` (structure below). NEVER write \
         {final_files}: the MERGER task `{module_id}` assembles the final file(s) from every shard's \
         pieces after all shards finish, and a shard that writes the final file overwrites its \
         siblings' work. Pieces cannot run alone — check each with a parse/lint (`node --check`, \
         `python3 -m py_compile`, or the language's equivalent) and say which you ran.\n\n\
         YOUR SPLIT: {responsibility}\n",
        shard_id = shard.id,
        n = siblings.len(),
        responsibility = shard.responsibility.trim(),
    );
    if !shard.sections.is_empty() {
        s.push_str(
            "Spec sections THIS shard implements (their text is in the module brief below):\n",
        );
        for h in &shard.sections {
            s.push_str(&format!("- {h}\n"));
        }
    }
    if !shard.provides.is_empty() {
        s.push_str(&format!(
            "You provide: {}\n",
            shard
                .provides
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let others: Vec<&ShardPlan> = siblings.iter().filter(|o| o.id != shard.id).collect();
    if !others.is_empty() {
        s.push_str("Your SIBLINGS implement the rest — read their split so you neither duplicate nor depend on writing it:\n");
        for o in others {
            s.push_str(&format!(
                "- `{}`: {}{}\n",
                o.id,
                o.responsibility.trim(),
                if o.provides.is_empty() {
                    String::new()
                } else {
                    format!(" (provides {})", o.provides.join(", "))
                }
            ));
        }
    }
    s.push_str(&format!(
        "\nTHE MODULE'S DECLARED INTERFACE — synthesis declared these names for the whole module; \
         implement EXACTLY the ones that fall in your split, with these signatures; never rename \
         one, never define a sibling's export a second time, never invent a parallel name for a \
         declared one:\n{}\n",
        render_interface(interface)
    ));
    s.push_str(&format!(
        "README.md — STRUCTURED, the engine parses it and the merger reads it. One line per item, \
         each line starting with its field name:\n\
         {p}: <symbol>(<signature>) — one exported/defined symbol per line\n\
         {a}: <what you assume about a sibling's symbol or the shared state> — one per line\n\
         {u}: <what you did not finish> — one per line, or `{u}: none`\n\
         {c}: <the parse/lint command you ran and what it printed>\n\
         End your final message with the same four fields (they are your HANDOFF to the merger).\n\n\
         THE MODULE'S BRIEF — whole. Your split is the part named above; the rest is the context \
         your siblings implement and the answers you all build to:\n\n{module_brief}",
        p = README_FIELDS[0],
        a = README_FIELDS[1],
        u = README_FIELDS[2],
        c = README_FIELDS[3],
    ));
    s
}

/// Build and apply the split as a PATCH: N shard tasks added (folder README as the owned file,
/// no deps, the shard brief), the module's deps widened to the shards; then the engine's own
/// annotations `shard_of` / `merger_of` (plan metadata the scheduler carries to dispatch — a
/// model never writes these). Returns the patched plan and the `plan_patched` event.
pub(super) fn apply_module_split(
    plan_json: &str,
    module_id: &str,
    split: &ModuleSplit,
) -> Result<(String, serde_json::Value), String> {
    let plan: serde_json::Value =
        serde_json::from_str(plan_json).map_err(|e| format!("plan is not valid JSON: {e}"))?;
    let subtasks = plan
        .get("subtasks")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "plan has no `subtasks` array".to_string())?;
    let module = subtasks
        .iter()
        .find(|t| t.get("id").and_then(|i| i.as_str()) == Some(module_id))
        .ok_or_else(|| format!("module task `{module_id}` is not in the plan"))?;
    let module_files = string_list(&module["files"]);
    let module_brief = module
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let module_deps = string_list(&module["depends_on"]);
    let difficulty = module
        .get("difficulty")
        .and_then(|d| d.as_str())
        .unwrap_or("hard")
        .to_string();
    let existing: std::collections::HashSet<String> = subtasks
        .iter()
        .filter_map(|t| t.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect();
    let mut shard_ids: Vec<String> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut adds: Vec<TaskAdd> = Vec::new();
    let mut annotations: Vec<(String, ShardOf)> = Vec::new();
    for shard in &split.shards {
        let short = kebab(&shard.id);
        if short.is_empty() {
            return Err(format!("shard id `{}` has no usable characters", shard.id));
        }
        let mut id = format!("{module_id}-{short}");
        if existing.contains(&id) || shard_ids.contains(&id) {
            id.push_str("-shard");
        }
        if existing.contains(&id) || shard_ids.contains(&id) {
            return Err(format!("shard id `{id}` collides with an existing task"));
        }
        let folder = format!("{SHARDS_DIR}/{module_id}/{short}");
        adds.push(TaskAdd {
            id: id.clone(),
            description: shard_brief(
                module_id,
                &module_files,
                module_brief,
                shard,
                &split.shards,
                &folder,
                &split.interface,
            ),
            difficulty: Some(difficulty.clone()),
            model: None,
            files: vec![format!("{folder}/README.md")],
            depends_on: Vec::new(),
        });
        annotations.push((
            id.clone(),
            ShardOf {
                module: module_id.to_string(),
                shard: short,
                folder: folder.clone(),
                responsibility: shard.responsibility.trim().to_string(),
                interface: split.interface.clone(),
            },
        ));
        shard_ids.push(id);
        folders.push(folder);
    }
    let mut merger_deps = module_deps;
    merger_deps.extend(shard_ids.iter().cloned());
    let patch = PlanPatch {
        replace: vec![TaskEdit {
            id: module_id.to_string(),
            files: None,
            depends_on: Some(merger_deps),
        }],
        add: adds,
        remove: Vec::new(),
    };
    let patched = apply_patch(plan_json, &patch)?;
    let mut v: serde_json::Value =
        serde_json::from_str(&patched).map_err(|e| format!("patched plan unreadable: {e}"))?;
    if let Some(tasks) = v.get_mut("subtasks").and_then(|s| s.as_array_mut()) {
        for t in tasks.iter_mut() {
            let Some(id) = t.get("id").and_then(|i| i.as_str()).map(String::from) else {
                continue;
            };
            if id == module_id {
                t["merger_of"] = serde_json::to_value(MergerOf {
                    module: module_id.to_string(),
                    shards: shard_ids.clone(),
                    folders: folders.clone(),
                    interface: split.interface.clone(),
                })
                .map_err(|e| e.to_string())?;
            } else if let Some((_, shard_of)) = annotations.iter().find(|(sid, _)| *sid == id) {
                t["shard_of"] = serde_json::to_value(shard_of).map_err(|e| e.to_string())?;
            }
        }
    }
    let out = v.to_string();
    let event = serde_json::json!({
        "event": "plan_patched",
        "source": "split",
        "module": module_id,
        "shards": shard_ids,
        "exports_declared": split.interface.exports.len(),
        "replace": patch.replace.len(),
        "add": patch.add.len(),
        "remove": patch.remove.len(),
        "after": decomposition_of(&out),
    });
    Ok((out, event))
}

impl GooseAgentDispatcher {
    /// ONE split request to synthesis for ONE fat task — the planner model, structured output,
    /// read-only, keyed `split-<task>` so the panel shows the call. The reply is parsed by
    /// `parse_module_split`; declining is the caller's `split_declined`.
    pub(super) async fn request_module_split(
        &self,
        planner_model: &str,
        task: &serde_json::Value,
        density: &serde_json::Value,
    ) -> Result<String> {
        let id = task.get("id").and_then(|i| i.as_str()).unwrap_or("task");
        let out = self
            .run_agent_timed_at(
                planner_model,
                split_system_prompt(),
                split_user_text(task, density),
                Some(goose::recipe::Response {
                    json_schema: Some(split_schema()),
                }),
                super::planner_side_turns(),
                &[],
                None,
                Some(&format!("split-{id}")),
                true,
                false,
            )
            .await?;
        Ok(out.final_output.unwrap_or(out.text))
    }
}

/// The split step of `plan_slices_to_dag`: measure, flag, request one patch per fat task, apply,
/// and walk the patched plan through the one door again. `split` is injected (the real one calls
/// `request_module_split`; a test hands back a canned reply) so the whole sequence runs without a
/// model. A plan with no fat task returns byte-identical and emits nothing.
pub(super) async fn split_fat_tasks<P, PFut>(
    plan_json: String,
    opened: &OpenOutput,
    spec: &str,
    every_decision_settled: bool,
    split: P,
    sink: &Arc<dyn EventSink>,
) -> String
where
    P: Fn(serde_json::Value, serde_json::Value) -> PFut,
    PFut: std::future::Future<Output = Result<String>>,
{
    let Ok(plan) = serde_json::from_str::<serde_json::Value>(&plan_json) else {
        return plan_json;
    };
    let measure = measure_fatness(&plan, &section_claims(opened, spec));
    if measure.fat.is_empty() {
        return plan_json;
    }
    for ev in measure.events() {
        sink.write_value(ev);
    }
    let mut current = plan_json.clone();
    let mut applied = 0usize;
    for i in &measure.fat {
        let row = &measure.rows[*i];
        let Some(task) = plan
            .get("subtasks")
            .and_then(|s| s.as_array())
            .and_then(|a| {
                a.iter()
                    .find(|t| t.get("id").and_then(|x| x.as_str()) == Some(row.id.as_str()))
            })
            .cloned()
        else {
            continue;
        };
        let mut density = row.to_json();
        if let Some(o) = density.as_object_mut() {
            o.insert("median".into(), measure.median.into());
            o.insert("threshold".into(), measure.threshold.into());
        }
        eprintln!(
            "  · fat task `{}`: {} spec sections for {} file(s) = {:.1}/file (median {:.1}, threshold {:.1}) — asking synthesis for a split patch",
            row.id,
            row.sections,
            row.files.len(),
            row.sections_per_file,
            measure.median,
            measure.threshold
        );
        let declined = |reason: String, sink: &Arc<dyn EventSink>| {
            eprintln!("  · split of `{}` declined: {reason}", row.id);
            sink.write_value(serde_json::json!({
                "event": "split_declined",
                "task": row.id,
                "reason": reason,
            }));
        };
        let reply = match split(task, density).await {
            Ok(r) => r,
            Err(e) => {
                declined(format!("split request did not return: {e}"), sink);
                continue;
            }
        };
        let parsed = match parse_module_split(&reply) {
            Ok(p) => p,
            Err(e) => {
                declined(e, sink);
                continue;
            }
        };
        match apply_module_split(&current, &row.id, &parsed) {
            Ok((next, event)) => {
                eprintln!(
                    "  · `{}` split into {} shards + a merger; {} exports declared",
                    row.id,
                    parsed.shards.len(),
                    parsed.interface.exports.len()
                );
                sink.write_value(event);
                current = next;
                applied += 1;
            }
            Err(e) => declined(format!("patch rejected: {e}"), sink),
        }
    }
    if applied == 0 {
        return plan_json;
    }
    // ONE DOOR: the shard tasks meet every repair the first pass ran — rule (e) marks the module's
    // final file not-theirs-to-write, the skeleton's brief regenerates without them, the join's
    // deps widen to them (rule (f)).
    finalize_plan_before_dag(current, spec, every_decision_settled, sink, "split")
}

#[cfg(test)]
mod tests {
    use super::super::opener::{OpenDecision, OpenSlice};
    use super::*;
    use goose_swarm::NullSink;
    use std::sync::Mutex;

    fn claims(rows: &[(&str, usize)]) -> HashMap<String, SectionClaim> {
        rows.iter()
            .map(|(id, n)| {
                (
                    id.to_string(),
                    SectionClaim {
                        sections: *n,
                        chars: n * 1000,
                    },
                )
            })
            .collect()
    }

    /// A synthesis-shaped plan (no skeleton yet — `finalize_plan_before_dag` prepends the real one).
    fn plan(rows: &[(&str, &[&str])]) -> serde_json::Value {
        let mut tasks: Vec<serde_json::Value> = rows
            .iter()
            .map(|(id, files)| {
                serde_json::json!({"id": id, "slice": id, "files": files, "depends_on": [], "description": format!("{id} brief")})
            })
            .collect();
        let ids: Vec<&str> = rows.iter().map(|(id, _)| *id).collect();
        tasks.push(serde_json::json!({"id": "integrate-verify", "files": [], "depends_on": ids, "description": "verify"}));
        serde_json::json!({"subtasks": tasks})
    }

    /// r6c's plan_loaded (seq 1387) and r5's, verbatim files and claimed-section counts: the
    /// long pole web-viz (7 sections → 1 file, 519 min) is the ONE fat task in r6c — ledgerd-core
    /// (12 → 6, 431 min) is not, at 2.0/file against a 1.94 mean; viz-field is r5's. The
    /// threshold comes from each plan's own distribution.
    #[test]
    fn fatness_flags_r6cs_web_viz_and_r5s_viz_field_from_their_own_distributions() {
        let r6c = plan(&[
            (
                "ledgerd-core",
                &[
                    "app/ledgerd/impl.py",
                    "app/db.py",
                    "app/sync.py",
                    "app/ledger.py",
                    "app/outbox.py",
                    "README.md",
                ],
            ),
            (
                "ledgerd-api",
                &[
                    "app/api.py",
                    "app/webhooks.py",
                    "app/drafts.py",
                    "app/auth.py",
                ],
            ),
            (
                "notifierd",
                &["app/notifierd/impl.py", "app/notify_store.py"],
            ),
            ("decisions-doc", &["DECISIONS.md"]),
            (
                "web-console",
                &["web/index.html", "web/styles.css", "web/app.js"],
            ),
            ("web-viz", &["web/viz.js"]),
        ]);
        let m = measure_fatness(
            &r6c,
            &claims(&[
                ("ledgerd-core", 12),
                ("ledgerd-api", 6),
                ("notifierd", 1),
                ("web-console", 2),
                ("web-viz", 7),
            ]),
        );
        assert_eq!(m.rows.len(), 6, "the join is not measured");
        let fat: Vec<&str> = m.fat.iter().map(|i| m.rows[*i].id.as_str()).collect();
        assert_eq!(fat, vec!["web-viz"], "{m:?}");
        assert!((m.median - 1.0833).abs() < 0.01, "median {}", m.median);
        assert!(
            m.threshold > 2.0 && m.threshold < 7.0,
            "threshold {}",
            m.threshold
        );
        let ev = &m.events()[0];
        assert_eq!(ev["event"], "plan_flag");
        assert_eq!(ev["kind"], "fat_task");
        assert_eq!(ev["task"], "web-viz");
        assert_eq!(ev["sections_per_file"], 7.0);
        assert_eq!(ev["distribution"].as_array().unwrap().len(), 6);

        let r5 = plan(&[
            (
                "boot-contract",
                &[
                    "app/__init__.py",
                    "app/ledgerd/impl.py",
                    "app/notifierd/impl.py",
                    "README.md",
                ],
            ),
            ("decisions", &["DECISIONS.md"]),
            ("brush-contract", &["web/brush.js"]),
            (
                "ledgerd-service",
                &[
                    "app/vendor_client.py",
                    "app/sync.py",
                    "app/ledgerdb.py",
                    "app/events.py",
                    "app/outbox.py",
                    "app/webhooks.py",
                    "app/drafts.py",
                    "app/httpapi.py",
                ],
            ),
            ("viz-field", &["web/viz.js"]),
            (
                "frontend-core",
                &["web/index.html", "web/styles.css", "web/app.js"],
            ),
            (
                "notifierd-service",
                &["app/notifierdb.py", "app/notifierapi.py"],
            ),
        ]);
        let m = measure_fatness(
            &r5,
            &claims(&[
                ("boot-contract", 3),
                ("decisions", 1),
                ("ledgerd-service", 14),
                ("viz-field", 11),
                ("frontend-core", 5),
                ("notifierd-service", 3),
            ]),
        );
        let fat: Vec<&str> = m.fat.iter().map(|i| m.rows[*i].id.as_str()).collect();
        assert_eq!(fat, vec!["viz-field"], "{m:?}");
    }

    /// No literal decides: a flat plan and a two-task plan flag nothing; a shard or merger already
    /// split is not measured again (idempotent through a second finalize).
    #[test]
    fn a_flat_or_tiny_distribution_flags_nothing_and_split_tasks_are_not_remeasured() {
        let flat = plan(&[
            ("a", &["a.py"]),
            ("b", &["b.py"]),
            ("c", &["c.py"]),
            ("d", &["d.py"]),
        ]);
        let m = measure_fatness(&flat, &claims(&[("a", 2), ("b", 2), ("c", 2), ("d", 2)]));
        assert!(m.fat.is_empty(), "{m:?}");
        let two = plan(&[("a", &["a.py"]), ("b", &["b.py"])]);
        let m = measure_fatness(&two, &claims(&[("a", 1), ("b", 9)]));
        assert!(m.fat.is_empty(), "mean + stddev is the max of two: {m:?}");
        let mut split = plan(&[
            ("a", &["a.py"]),
            ("b", &["b.py"]),
            ("c", &["c.py"]),
            ("viz", &["web/viz.js"]),
        ]);
        split["subtasks"][3]["merger_of"] =
            serde_json::json!({"module": "viz", "shards": [], "folders": []});
        let m = measure_fatness(&split, &claims(&[("a", 1), ("b", 1), ("c", 1), ("viz", 9)]));
        assert!(m.fat.is_empty(), "a merger is already split: {m:?}");
        assert_eq!(m.rows.len(), 3);
    }

    fn viz_split() -> ModuleSplit {
        serde_json::from_value(serde_json::json!({
            "interface": {
                "exports": [
                    {"name": "window.vs7dbg.layout", "kind": "function", "signature": "layout() -> {d0, D0, R0}", "purpose": "the locked layout basis"},
                    {"name": "window.vs7dbg.pick", "kind": "function", "signature": "pick(sx, sy) -> {id, index} | null", "purpose": "occlusion-correct pick from the FBO"},
                    {"name": "buildScene", "kind": "function", "signature": "buildScene(data: {ids, day, rank, amount_minor, currency, status}) -> void", "purpose": "fill the instance buffers"}
                ],
                "shared_state": "S = {yaw, pitch, distance, brush: Set<id>, count, dirty}",
                "layout": ["constants", "state S", "math helpers", "GL programs", "pick FBO", "camera", "labels", "brush", "stream", "window.vs7dbg", "boot"]
            },
            "shards": [
                {"id": "render", "responsibility": "WebGL programs, instanced geometry, demand rendering", "sections": ["Rendering — bounded draw calls, demand rendering", "8. The 3D field — 12,288 instances, five mechanisms"], "provides": ["initGL", "render", "buildScene"]},
                {"id": "pick-camera", "responsibility": "the pick FBO and the orbit camera with inertia", "sections": ["The pick buffer", "Camera — orbit + inertia"], "provides": ["rebuildPickFBO", "readPickAt", "window.vs7dbg.pick", "updateBasis", "project"]},
                {"id": "labels-brush-api", "responsibility": "label culling, the linked brush, streaming and the vs7dbg API", "sections": ["Screen-space labels — deterministic collision culling", "The linked brush — table ⇄ instances", "`vs7dbg` — REQUIRED and graded"], "provides": ["updateLabels", "toggleBrush", "window.vs7dbg.layout"]}
            ]
        }))
        .unwrap()
    }

    /// THE SHARD SHAPE (Mihai 11:3x): shards own only their folder's README under .swarm/shards,
    /// depend on nothing, carry the declaration and the NEVER-write rule; the module task becomes
    /// the merger — keeps `web/viz.js`, gains the shards as deps, carries `merger_of`. The patched
    /// plan loads as a DAG and survives the one door: rule (e) marks `web/viz.js` not-the-shard's,
    /// the skeleton's PLANNED MODULES lists no README, the join waits on the shards too.
    #[test]
    fn the_split_patch_adds_shards_in_temp_folders_and_the_module_becomes_the_merger() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let p = plan(&[
            (
                "web-console",
                &["web/index.html", "web/styles.css", "web/app.js"],
            ),
            ("web-viz", &["web/viz.js"]),
        ]);
        let (out, event) = apply_module_split(&p.to_string(), "web-viz", &viz_split()).unwrap();
        assert_eq!(event["event"], "plan_patched");
        assert_eq!(event["source"], "split");
        assert_eq!(event["add"], 3);
        assert_eq!(event["replace"], 1);
        assert_eq!(event["exports_declared"], 3);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let task = |id: &str| {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .cloned()
                .unwrap_or_else(|| panic!("{id} missing"))
        };
        let render = task("web-viz-render");
        assert_eq!(
            string_list(&render["files"]),
            vec![".swarm/shards/web-viz/render/README.md"]
        );
        assert!(
            string_list(&render["depends_on"]).is_empty(),
            "independent by construction"
        );
        assert_eq!(render["shard_of"]["module"], "web-viz");
        assert_eq!(render["shard_of"]["folder"], ".swarm/shards/web-viz/render");
        let brief = render["description"].as_str().unwrap();
        assert!(brief.contains("NEVER write `web/viz.js`"), "{brief}");
        assert!(
            brief.contains("`window.vs7dbg.pick` (function): `pick(sx, sy) -> {id, index} | null`"),
            "{brief}"
        );
        assert!(
            brief.contains("Shared state: S = {yaw, pitch, distance"),
            "{brief}"
        );
        assert!(
            brief.contains("PROVIDES:") && brief.contains("CHECKED_WITH:"),
            "{brief}"
        );
        assert!(
            brief.contains("`pick-camera`: the pick FBO"),
            "siblings named: {brief}"
        );
        assert!(
            brief.ends_with("web-viz brief"),
            "the module brief rides whole at the end"
        );
        let merger = task("web-viz");
        assert_eq!(
            string_list(&merger["files"]),
            vec!["web/viz.js"],
            "the merger owns the final file"
        );
        let deps = string_list(&merger["depends_on"]);
        for d in [
            "web-viz-render",
            "web-viz-pick-camera",
            "web-viz-labels-brush-api",
        ] {
            assert!(deps.contains(&d.to_string()), "{deps:?}");
        }
        assert_eq!(merger["merger_of"]["shards"].as_array().unwrap().len(), 3);
        assert_eq!(
            merger["merger_of"]["folders"][1],
            ".swarm/shards/web-viz/pick-camera"
        );
        let specs = goose_swarm::specs_from_plan_json(&out).unwrap();
        let ts = specs.iter().find(|s| s.id == "web-viz-render").unwrap();
        assert_eq!(ts.shard_of.as_ref().unwrap().shard, "render");
        assert_eq!(ts.shard_of.as_ref().unwrap().interface.exports.len(), 3);
        assert!(specs
            .iter()
            .find(|s| s.id == "web-viz")
            .unwrap()
            .merger_of
            .is_some());
        goose_swarm::Dag::from_planner_json(&out).expect("loads");

        // Through the one door: the shards survive every repair (they own a file), the module's
        // final file is marked not-theirs, the skeleton lists no README, the join waits on them.
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let finalized = finalize_plan_before_dag(out.clone(), spec, false, &sink, "split");
        let f: serde_json::Value = serde_json::from_str(&finalized).unwrap();
        let tasks = f["subtasks"].as_array().unwrap();
        let get = |id: &str| {
            tasks
                .iter()
                .find(|t| t["id"] == id)
                .unwrap_or_else(|| panic!("{id}"))
        };
        let d = get("web-viz-render")["description"].as_str().unwrap();
        assert!(
            d.contains("- `web/viz.js` → owned by task `web-viz`"),
            "{}",
            d
        );
        let skel = get("skeleton")["description"].as_str().unwrap();
        assert!(
            !skel.contains("README.md"),
            "PLANNED MODULES lists no shard folder:\n{skel}"
        );
        assert!(skel.contains("web-viz: web/viz.js"), "{skel}");
        let join = string_list(&get("integrate-verify")["depends_on"]);
        assert!(join.contains(&"web-viz-render".to_string()), "{join:?}");
        assert!(
            get("skeleton")["files"].as_array().unwrap().len() >= 3,
            "the real skeleton was prepended by finalize"
        );
        let again = finalize_plan_before_dag(finalized.clone(), spec, false, &sink, "split");
        assert_eq!(again, finalized, "idempotent");
    }

    #[test]
    fn a_split_reply_needs_two_shards_with_ids_and_responsibilities() {
        assert!(parse_module_split("no json here").is_err());
        assert!(parse_module_split(
            r#"{"interface":{},"shards":[{"id":"only","responsibility":"x"}]}"#
        )
        .is_err());
        assert!(parse_module_split(r#"{"interface":{},"shards":[{"id":"a","responsibility":"x"},{"id":"","responsibility":"y"}]}"#).is_err());
        let ok = parse_module_split(
            "Sure:\n```json\n{\"interface\":{\"exports\":[],\"shared_state\":\"\",\"layout\":[]},\"shards\":[{\"id\":\"a\",\"responsibility\":\"x\",\"sections\":[],\"provides\":[]},{\"id\":\"b\",\"responsibility\":\"y\",\"sections\":[],\"provides\":[]}]}\n```",
        )
        .unwrap();
        assert_eq!(ok.shards.len(), 2);
        assert_eq!(kebab("Pick & Camera!"), "pick-camera");
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for RecordingSink {
        fn emit(&self, _event: &goose_swarm::SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    fn r6c_like_opened() -> OpenOutput {
        let slice = |id: &str, objective: &str, sections: &[&str]| OpenSlice {
            id: id.into(),
            title: id.into(),
            objective: objective.into(),
            questions: Vec::new(),
            weight: 3,
            sections: sections.iter().map(|s| s.to_string()).collect(),
        };
        OpenOutput {
            slices: vec![
                slice(
                    "ledgerd-core",
                    "Own `app/db.py`, `app/sync.py`, `app/ledger.py` — the ledger store and sync",
                    &["3. ledgerd — the ledger service", "Boot", "Storage"],
                ),
                slice(
                    "notifierd",
                    "Own `app/notifierd/impl.py`, `app/notify_store.py`",
                    &["4. notifierd"],
                ),
                slice(
                    "web-console",
                    "Own `web/index.html`, `web/styles.css`, `web/app.js` — the console",
                    &["7. The web console", "Console table"],
                ),
                slice(
                    "web-viz",
                    "Own `web/viz.js` — the 3D engine",
                    &[
                        "8. The 3D field — 12,288 instances, five mechanisms",
                        "Rendering — bounded draw calls, demand rendering",
                        "The pick buffer",
                        "Camera — orbit + inertia",
                        "Screen-space labels — deterministic collision culling",
                        "The linked brush — table ⇄ instances",
                        "`vs7dbg` — REQUIRED and graded",
                    ],
                ),
            ],
            open_decisions: vec![OpenDecision {
                line: "which storage".into(),
                options: vec![],
            }],
        }
    }

    /// END TO END through the planner's one door with fake model closures (the seam
    /// `plan_slices_to_dag` exists for): synthesis returns r6c's shape, web-viz's 7 sections on
    /// 1 file are flagged, ONE split request goes out for web-viz only, its patch lands, and the
    /// DAG carries three shards + a merger with the declared interface — `plan_flag`,
    /// `plan_patched{source: split}` and a second `plan_repaired{source: split}` in the log.
    #[tokio::test]
    async fn a_fat_task_gets_one_split_patch_and_the_dag_carries_shards_and_a_merger() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sink = Arc::new(RecordingSink::default());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Mutex::new(Vec::<String>::new()));
        let asked_in = asked.clone();
        let (plan_json, dag) = super::super::plan_slices_to_dag(
            r6c_like_opened(),
            spec,
            dir.path(),
            Vec::new(),
            super::super::TargetLang::Python,
            &[],
            &[],
            |_briefs, _tree| async move {
                Ok(serde_json::json!({"subtasks": [
                    {"id": "ledgerd-core", "slice": "ledgerd-core", "difficulty": "hard", "files": ["app/ledgerd/impl.py", "app/db.py", "app/sync.py", "app/ledger.py", "app/outbox.py", "README.md"], "depends_on": []},
                    {"id": "notifierd", "slice": "notifierd", "difficulty": "hard", "files": ["app/notifierd/impl.py", "app/notify_store.py"], "depends_on": []},
                    {"id": "web-console", "slice": "web-console", "difficulty": "hard", "files": ["web/index.html", "web/styles.css", "web/app.js"], "depends_on": []},
                    {"id": "web-viz", "slice": "web-viz", "difficulty": "hard", "files": ["web/viz.js"], "depends_on": []},
                    {"id": "integrate-verify", "difficulty": "hard", "files": [], "depends_on": ["ledgerd-core", "notifierd", "web-console", "web-viz"]},
                ]})
                .to_string())
            },
            move |task, density| {
                let asked = asked_in.clone();
                async move {
                    asked.lock().unwrap().push(format!(
                        "{}:{}",
                        task["id"].as_str().unwrap(),
                        density["sections"].as_u64().unwrap()
                    ));
                    Ok(serde_json::to_string(&serde_json::json!({
                        "interface": {"exports": [{"name": "window.vs7dbg.pick", "kind": "function", "signature": "pick(sx, sy) -> {id, index} | null", "purpose": "pick"}], "shared_state": "S", "layout": ["a", "b"]},
                        "shards": [
                            {"id": "render", "responsibility": "programs and geometry", "sections": ["Rendering — bounded draw calls, demand rendering"], "provides": ["render"]},
                            {"id": "pick-camera", "responsibility": "pick FBO and camera", "sections": ["The pick buffer", "Camera — orbit + inertia"], "provides": ["window.vs7dbg.pick"]},
                            {"id": "labels-brush-api", "responsibility": "labels, brush, API", "sections": ["`vs7dbg` — REQUIRED and graded"], "provides": ["updateLabels"]}
                        ]
                    }))
                    .unwrap())
                }
            },
            &sink_dyn,
        )
        .await
        .unwrap();
        assert_eq!(
            *asked.lock().unwrap(),
            vec!["web-viz:7".to_string()],
            "ONE request, for the fat task only"
        );
        for id in [
            "web-viz",
            "web-viz-render",
            "web-viz-pick-camera",
            "web-viz-labels-brush-api",
            "skeleton",
            "integrate-verify",
        ] {
            assert!(
                dag.tasks.contains_key(id),
                "{id} missing from {:?}",
                dag.tasks.keys().collect::<Vec<_>>()
            );
        }
        let merger = &dag.tasks["web-viz"].spec;
        assert!(merger.merger_of.is_some());
        assert!(merger.deps.contains(&"web-viz-render".to_string()));
        assert!(merger.owned_files == vec!["web/viz.js".to_string()]);
        let shard = &dag.tasks["web-viz-render"].spec;
        assert!(
            shard.deps.is_empty(),
            "shards depend on nothing: {:?}",
            shard.deps
        );
        assert_eq!(
            shard.owned_files,
            vec![".swarm/shards/web-viz/render/README.md".to_string()]
        );
        assert_eq!(
            shard.shard_of.as_ref().unwrap().interface.exports[0].name,
            "window.vs7dbg.pick"
        );
        assert!(plan_json.contains("\"merger_of\""));
        let events = sink.0.lock().unwrap().clone();
        let names: Vec<(String, String)> = events
            .iter()
            .map(|e| {
                (
                    e["event"].as_str().unwrap_or("?").to_string(),
                    e.get("source")
                        .or(e.get("kind"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .collect();
        let idx = |n: &str, s: &str| {
            names
                .iter()
                .position(|x| x.0 == n && x.1 == s)
                .unwrap_or_else(|| panic!("missing {n}/{s} in {names:?}"))
        };
        assert!(idx("plan_repaired", "plan") < idx("plan_flag", "fat_task"));
        assert!(idx("plan_flag", "fat_task") < idx("plan_patched", "split"));
        assert!(idx("plan_patched", "split") < idx("plan_repaired", "split"));
        assert_eq!(
            names.iter().filter(|x| x.0 == "plan_flag").count(),
            1,
            "{names:?}"
        );
        assert!(!names.iter().any(|x| x.0 == "split_declined"), "{names:?}");
        let fired = events.iter().find(|e| e["event"] == "plan_flag").unwrap();
        assert_eq!(fired["task"], "web-viz");
        assert!(
            fired["section_chars"].as_u64().unwrap() > 5000,
            "the claimed sections' real chars: {fired}"
        );
    }

    /// MILD: an unparseable split leaves the plan byte-identical, the flag loud, and says why.
    #[tokio::test]
    async fn a_declined_split_leaves_the_plan_untouched_and_the_flag_loud() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sink = Arc::new(RecordingSink::default());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let before = plan(&[
            (
                "ledgerd-core",
                &["app/db.py", "app/sync.py", "app/ledger.py"],
            ),
            (
                "notifierd",
                &["app/notifierd/impl.py", "app/notify_store.py"],
            ),
            (
                "web-console",
                &["web/index.html", "web/styles.css", "web/app.js"],
            ),
            ("web-viz", &["web/viz.js"]),
        ])
        .to_string();
        let after = split_fat_tasks(
            before.clone(),
            &r6c_like_opened(),
            spec,
            false,
            |_task, _density| async move { Ok("I would rather not.".to_string()) },
            &sink_dyn,
        )
        .await;
        assert_eq!(after, before);
        let events = sink.0.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|e| e["event"] == "plan_flag" && e["task"] == "web-viz"));
        let declined = events
            .iter()
            .find(|e| e["event"] == "split_declined")
            .unwrap();
        assert_eq!(declined["task"], "web-viz");
        assert!(declined["reason"].as_str().unwrap().contains("no JSON"));
        assert!(!events.iter().any(|e| e["event"] == "plan_patched"));
    }
}
