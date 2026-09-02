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
//! 1. MEASURE — per task, spec sections claimed per owned file (and claimed chars per file). FAT
//!    is TWO plan-derived tests, both required: sections-per-file ABOVE mean + one standard
//!    deviation of the tasks that claim any section, AND at least 2× their median. Mean+stddev
//!    alone flags the maximum of almost any plan (r6c without web-viz: ledgerd-core 2.0/file vs a
//!    1.78 threshold); the median floor is what makes "fat" mean "twice the typical task". Never a
//!    literal; median, floor and threshold ride the event. A fat task is a loud
//!    `plan_flag{kind: fat_task, …}`.
//! 2. REQUEST — ONE split request to synthesis per fat task (a PATCH, invariant 3, never a
//!    re-emission): the planner DECLARES the module's interface as plan text — exported names,
//!    kinds, signatures, the shared-state shape, the assembly order — and partitions the CLAIMED
//!    headings the request lists (not every `###` block of the brief, which also carries consumed
//!    context) into SHARDS. Declining is a ramp, not a failure: ONE shard with id `whole` and the
//!    reason, or an unparseable reply, is loud (`split_declined{task, reason}`), the plan stays
//!    byte-identical and the flag stays.
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

/// ASSEMBLE, then glue (DESIGN-SPLIT-V2 §1): before the merger model runs, CODE writes every
/// piece's definitions into `ASSEMBLED.<ext>` in the declared interface's order; the merger's job
/// becomes the glue. A child of this module so swarm.rs gains no wiring line.
mod assembly;
mod assumes;

/// Where a module's shards work. Under `.swarm/` on purpose: every tree lister, snapshot and
/// manifest already excludes it (`tree::SNAPSHOT_EXCLUDES`), so pieces never reach the scored tree
/// and never read as stray files — the merger reads them by path from its dossier.
pub(super) const SHARDS_DIR: &str = ".swarm/shards";

/// The opener's claim per slice: how many spec sections, and how many characters of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SectionClaim {
    pub(super) sections: usize,
    pub(super) chars: usize,
    /// The claimed headings verbatim — the split request lists THESE as the sections to partition.
    pub(super) headings: Vec<String>,
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
                    headings: sl.sections.clone(),
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
    pub(super) headings: Vec<String>,
}

impl TaskDensity {
    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task": self.id,
            "files": self.files,
            "sections": self.sections,
            "claimed_sections": self.headings,
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
    /// 2× the median — the second test; both are required.
    pub(super) floor: f64,
    /// Indexes into `rows`, fattest first.
    pub(super) fat: Vec<usize>,
    /// Tasks whose `slice` (or id) names NO opener slice — measured as zero sections because the
    /// opener routed nothing to them by that name. Recorded, not defaulted away: when EVERY row
    /// is unclaimed the opener's and the planner's slice names disagree and the split can never
    /// fire (`fatness_unmeasurable` says so with these ids and the opener's names).
    pub(super) unclaimed: Vec<String>,
}

/// Measure every planner task — not the join, not the skeleton, not a task already split (a shard
/// or a merger), not a task owning nothing (rule (a) removes those) — and derive BOTH fatness tests
/// from the distribution of the tasks that claim any section: sections-per-file strictly above
/// mean + one population standard deviation, AND at least 2× the median. No literal decides. What
/// each test does on the measured plans: mean + stddev alone flags the maximum of almost any
/// plan — r6c WITHOUT web-viz puts ledgerd-core (2.0/file) above a 1.78 threshold — so it cannot
/// mean "fat" by itself; the median floor (2.17 there) is what says "twice the typical task". On
/// r6c's five section-claiming rows the pair is threshold 4.73 / floor 3.0 (web-viz 7.0 flagged,
/// ledgerd-core 2.0 not); on r5's six it is 6.56 / 3.17 (viz-field 11.0 flagged, ledgerd-service
/// 1.75 not). A flat plan (stddev 0) flags nothing (no task is strictly above its own mean); two
/// tasks cannot flag (mean + stddev IS the max).
pub(super) fn measure_fatness(
    plan: &serde_json::Value,
    claims: &HashMap<String, SectionClaim>,
) -> FatMeasure {
    let mut rows: Vec<TaskDensity> = Vec::new();
    let mut unclaimed: Vec<String> = Vec::new();
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
        // A task no opener slice is named for (a patch-added task, r6c's decisions-doc) measures
        // as 0 sections — an honest zero: the opener routed no spec to it by that name — and is
        // RECORDED in `unclaimed`, so a plan whose every task is unclaimed (the opener's and the
        // planner's slice names disagree) is said rather than measured as a flat nothing. It
        // rides `rows` for the event's distribution but is excluded from the statistics below
        // (`sections > 0`) and can never be fat.
        let claim = match claims.get(slice) {
            Some(c) => c.clone(),
            None => {
                unclaimed.push(id.to_string());
                SectionClaim::default()
            }
        };
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
            headings: claim.headings,
        });
    }
    let mut sorted: Vec<f64> = rows
        .iter()
        .filter(|r| r.sections > 0)
        .map(|r| r.sections_per_file)
        .collect();
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
    let floor = 2.0 * median;
    let mut fat: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.sections > 0 && r.sections_per_file > threshold && r.sections_per_file >= floor
        })
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
        floor,
        fat,
        unclaimed,
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
                    o.insert("floor".into(), self.floor.into());
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
    /// The shared state this shard is the single WRITER of (split v2 §4); a pure reader lists none.
    #[serde(default)]
    pub(super) writes: Vec<String>,
    /// The declared clusters this shard runs — filled by `size_shards_to_hosts`, never by the
    /// planner (VA-102 refuter: grouping kept only the union, and the piece boundaries the brief
    /// needs to name a FIRST write were gone; r6h's `camera`+`labels-brush` shard could not say
    /// `camera` was its lightest piece). Empty means unsized: the shard is its own one cluster.
    #[serde(default)]
    pub(super) clusters: Vec<ClusterPlan>,
}

/// One declared cluster inside a sized shard: its id, the names it provides, and the weight the
/// partition used (its claimed sections, floor 1 — `split_sized.weights`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub(super) struct ClusterPlan {
    pub(super) id: String,
    #[serde(default)]
    pub(super) provides: Vec<String>,
    #[serde(default)]
    pub(super) weight: usize,
}

/// A declared shard as the one cluster it is.
fn cluster_of(s: &ShardPlan) -> ClusterPlan {
    ClusterPlan {
        id: s.id.clone(),
        provides: s.provides.clone(),
        weight: s.sections.len().max(1),
    }
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
                    "required": ["id", "responsibility", "sections", "provides", "writes"],
                    "properties": {
                        "id": {"type": "string"},
                        "responsibility": {"type": "string"},
                        "sections": {"type": "array", "items": {"type": "string"}},
                        "provides": {"type": "array", "items": {"type": "string"}},
                        "writes": {"type": "array", "items": {"type": "string"}}
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
        each with its kind, exact signature (parameters, return shape) and one-line purpose; EVERY \
        SHARED STATE (an object, record or typed array more than one part reads or writes) with its \
        name, its exact SHAPE — fields and types, and for a typed array its stride and offsets — \
        and the ONE shard that WRITES it (every other shard only reads; two shards that would both \
        write one state are one shard, not two — merge them); and the LAYOUT — the final file(s) in \
        assembly order (which regions come first: \
        constants, state, helpers, mechanisms, the exported API, boot). Names you declare are \
        BINDING on every shard and the merger, so declare real, complete signatures, not \
        placeholders.\n\
     2. PARTITION the CLAIMED SECTIONS the request lists — those exact headings, not every `###` \
        block of the brief (the brief also carries consumed context) — into 2 or more SHARDS that \
        can be written independently and in parallel — usually one per mechanism or section group. \
        Each shard: a short kebab-case id, one sentence of responsibility, the exact claimed \
        headings it implements (every claimed heading goes to exactly one shard), the declared \
        names it provides, and `writes`: the shared-state names it is the single writer of (an \
        empty list for a pure reader). Shards write PIECES (functions/classes) in private folders; a MERGER \
        assembles the final file from them afterwards — so no shard needs another shard's file to \
        exist.\n\n\
     IF THIS TASK SHOULD NOT BE SPLIT — its sections are one mechanism no two people could write \
     apart, or the measurement is an artifact (a few short sections on one small file) — do not \
     invent shards: return exactly ONE shard with id `whole`, its responsibility stating the reason \
     in one sentence, and an empty interface; the plan then stays exactly as it is.\n\n\
     Do not restate the spec and do not write code. Call the final_output tool once with \
     {interface: {exports: [{name, kind, signature, purpose}], shared_state, layout: []}, \
     shards: [{id, responsibility, sections: [], provides: [], writes: []}]}."
        .to_string()
}

/// The split request's body: THIS task's facts — id, files, the measured density, the CLAIMED
/// headings as the list to partition (S10: the brief's `###` blocks also carry consumed context,
/// and a planner told to partition "the `###` blocks" partitioned those too), then the brief whole
/// as context.
pub(super) fn split_user_text(task: &serde_json::Value, density: &serde_json::Value) -> String {
    let id = task.get("id").and_then(|i| i.as_str()).unwrap_or("?");
    let files = string_list(&task["files"]);
    let desc = task
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let claimed = string_list(&density["claimed_sections"]);
    format!(
        "## The fat task\nid: `{id}`\nfiles (the module's FINAL files — the merger writes these; \
         shards write pieces in their own folders): {}\nmeasured: {} spec sections for {} file(s) = \
         {:.2} sections per file (plan median {:.2}, floor 2×median {:.2}, mean+stddev threshold \
         {:.2}); brief {} chars\n\n## The sections to partition — the opener's {} claimed \
         heading(s) for this task; every one goes to exactly one shard\n{}\n\n## Its brief \
         (context — the `###` blocks it consumed are spliced in; partition the headings above, not \
         these)\n{desc}",
        files
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", "),
        density["sections"].as_u64().unwrap_or(0),
        files.len(),
        density["sections_per_file"].as_f64().unwrap_or(0.0),
        density["median"].as_f64().unwrap_or(0.0),
        density["floor"].as_f64().unwrap_or(0.0),
        density["threshold"].as_f64().unwrap_or(0.0),
        density["brief_chars"].as_u64().unwrap_or(0),
        claimed.len(),
        claimed
            .iter()
            .map(|h| format!("- {h}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Parse the planner's reply. Refuses (loudly, for `split_declined`) anything that cannot be
/// built into shards: no JSON, fewer than two shards, a shard without an id — and the prompt's
/// own decline ramp, ONE shard named `whole` whose responsibility is the reason.
pub(super) fn parse_module_split(reply: &str) -> Result<ModuleSplit, String> {
    let v = super::parse_json_lenient(reply)
        .ok_or_else(|| "no JSON object in the reply".to_string())?;
    let split: ModuleSplit =
        serde_json::from_value(v).map_err(|e| format!("split is not the declared shape: {e}"))?;
    if let [only] = split.shards.as_slice() {
        if only.id.trim().eq_ignore_ascii_case("whole") {
            let reason = only.responsibility.trim();
            return Err(format!(
                "declined by synthesis: {}",
                if reason.is_empty() {
                    "(no reason given)"
                } else {
                    reason
                }
            ));
        }
    }
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
/// read it without guessing (S3 parses these lines into the handoff channel). `WRITES` (split v2
/// §4): the shared state this shard is the ONE writer of — two shards naming the same state is
/// `shard_shared_state_writers`, the `cooperate` edge the declaration should have merged.
pub(super) const README_FIELDS: [&str; 5] = [
    "PROVIDES",
    "ASSUMES",
    "UNFINISHED",
    "CHECKED_WITH",
    "WRITES",
];

/// VA-102: the shard brief's FIRST instruction — a concrete write, before any design. r6h
/// (BUILD 05:13→06:00, three shards of `viz-engine`, zero live bytes): `camera-labels-brush`
/// reasoned 102k chars in 46 minutes — 46,410 of them inside code fences, full piece bodies it
/// would then have to retype — with one `ls` of its empty folder and no file, because the brief
/// listed every declared name, put the README's structure LAST, and never said which file comes
/// first. The README comes first with its PROVIDES lines rendered from the declaration (copy-in,
/// signatures included — the refuter caught "each with its declared signature" pointing at an
/// interface the text said not to read yet), then ONE piece: the LIGHTEST DECLARED CLUSTER the
/// shard runs (`split_sized.weights`; r6h: `camera`, weight 1 — the string-length proxy this
/// replaced picked `viz3d.brush`, a one-line getter, and `initViz`, the GL setup). MILD: text,
/// nothing refuses.
fn first_action_paragraph(
    module_files: &[String],
    shard: &ShardPlan,
    folder: &str,
    interface: &ModuleInterface,
) -> String {
    let (p, a, u, c, w) = (
        README_FIELDS[0],
        README_FIELDS[1],
        README_FIELDS[2],
        README_FIELDS[3],
        README_FIELDS[4],
    );
    let clusters = shard_clusters(shard);
    let provides = if shard.provides.is_empty() {
        // The absence is SAID (`shard_provides_empty`, apply_module_split) and stated here where
        // the copy-in lines would be — never a phrase standing in for names.
        if shard.sections.is_empty() {
            format!(
                "{p}: (synthesis declared neither exports nor sections for this shard — \
                 `shard_provides_empty` is on the run log; its responsibility is the one fact: {})",
                shard.responsibility.trim()
            )
        } else {
            format!(
                "{p}: (synthesis declared no exports for this shard — `shard_provides_empty` is on \
                 the run log; list each symbol you define for the sections {})",
                shard
                    .sections
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    } else {
        field_lines(
            p,
            &shard
                .provides
                .iter()
                .map(|n| provides_line(n, interface))
                .collect::<Vec<_>>(),
        )
    };
    let unfinished = field_lines(
        u,
        &clusters.iter().map(|k| k.id.clone()).collect::<Vec<_>>(),
    );
    let writes = if shard.writes.is_empty() {
        format!("{w}: none")
    } else {
        field_lines(w, &shard.writes)
    };
    let Some(first) = clusters.iter().min_by_key(|k| k.weight) else {
        unreachable!("shard_clusters returns at least the shard itself")
    };
    let path = format!("{folder}/{}{}", kebab(&first.id), piece_ext(module_files));
    let names = if first.provides.is_empty() {
        "the definitions its sections require".to_string()
    } else {
        first.provides.join(", ")
    };
    let piece = if clusters.len() == 1 {
        format!(
            "Then write your FIRST PIECE — your one cluster `{id}` is your one piece: `{path}` — \
             {names}. You may split it across more files; the README's {u}: lists whichever are \
             not yet written.",
            id = first.id
        )
    } else {
        let others = clusters
            .iter()
            .filter(|k| k.id != first.id)
            .map(|k| format!("`{}`", k.id))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Then write your FIRST PIECE: `{path}` — the `{id}` cluster, the lightest of your {k} \
             (weight {wt}: its claimed spec sections, floor 1): {names} — alone, complete in one \
             `write`, and check it with a parse/lint. Only then think about the next piece \
             ({others}), one `write` per piece.",
            id = first.id,
            k = clusters.len(),
            wt = first.weight
        )
    };
    format!(
        "YOUR FIRST ACTION — before any design: write `{folder}/README.md`, first version, these \
         lines copied in:\n\
         {provides}\n\
         {a}: <the sibling names and shared state you will read — one per line>\n\
         {unfinished}\n\
         {c}: none yet\n\
         {writes}\n\
         {piece} Update the README's {u}: lines as each piece lands. Do NOT draft a file's body in \
         your reasoning — a body drafted there has to be typed twice; write it to the file and read \
         the tool result back. An open question goes under {u}: in the README, not into more \
         reasoning.\n\n"
    )
}

/// The clusters a shard runs: what sizing recorded, or — unsized (a declaration with no more
/// clusters than hosts that never passed `size_shards_to_hosts`, a merger's gap shard) — the
/// shard as its own one cluster. Never empty.
fn shard_clusters(shard: &ShardPlan) -> Vec<ClusterPlan> {
    if shard.clusters.is_empty() {
        vec![cluster_of(shard)]
    } else {
        shard.clusters.clone()
    }
}

/// One README field as copy-in lines: `FIELD: first`, then `- next` per item (the shape
/// `parse_shard_note` reads back). Empty for no items — callers pass at least one.
fn field_lines(field: &str, items: &[String]) -> String {
    let mut s = String::new();
    for (i, it) in items.iter().enumerate() {
        if i == 0 {
            s.push_str(&format!("{field}: {it}"));
        } else {
            s.push_str(&format!("\n- {it}"));
        }
    }
    s
}

/// A PROVIDES line for one declared name: the declaration's signature with the FULL name in
/// front — `viz3d.brush` + `brush(): string[]` → `viz3d.brush(): string[]`, so the merger's
/// identifier read (`ident_at`) sees the dotted name and not a bare `brush` two shards both
/// provide. A signature that does not start with the name's last segment rides after a dash; a
/// name synthesis gave no signature stands alone.
fn provides_line(name: &str, interface: &ModuleInterface) -> String {
    let short = name.rsplit('.').next().unwrap_or(name);
    match interface
        .exports
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.signature.trim())
        .filter(|sig| !sig.is_empty())
    {
        Some(sig) => match sig.strip_prefix(short) {
            Some(rest) => format!("{name}{rest}"),
            None => format!("{name} — {sig}"),
        },
        None => name.to_string(),
    }
}

/// The piece files' extension: the module's own final file's. A final file with no extension (a
/// `Makefile`-class module) yields pieces with none — that empty IS the module's own naming.
fn piece_ext(module_files: &[String]) -> String {
    match module_files
        .first()
        .and_then(|f| std::path::Path::new(f).extension())
        .and_then(|e| e.to_str())
    {
        Some(e) => format!(".{e}"),
        None => String::new(),
    }
}

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
         parallel on different machines.\n\n",
        shard_id = shard.id,
        n = siblings.len(),
    );
    s.push_str(&first_action_paragraph(
        module_files,
        shard,
        folder,
        interface,
    ));
    s.push_str(&format!(
        "WHERE YOU WORK: ONLY inside your folder `{folder}/` (create it). Write your PIECES there as \
         files in the module's language — the functions, classes and sections your split names, \
         e.g. `{folder}/<piece>.<ext>` — plus `{folder}/README.md` (structure below; its first \
         version is your FIRST write, above). NEVER write \
         {final_files}: the MERGER task `{module_id}` assembles the final file(s) from every shard's \
         pieces after all shards finish, and a shard that writes the final file overwrites its \
         siblings' work. Pieces cannot run alone — check each with a parse/lint (`node --check`, \
         `python3 -m py_compile`, or the language's equivalent) and say which you ran.\n\n\
         YOUR SPLIT: {responsibility}\n",
        responsibility = shard.responsibility.trim(),
    ));
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
    if !shard.writes.is_empty() {
        s.push_str(&format!(
            "You are the ONLY WRITER of shared state: {} — every sibling reads it through the \
             declared shape and writes it nowhere; declare each under {w}: in your README with the \
             shape you write.\n",
            shard
                .writes
                .iter()
                .map(|st| format!("`{st}`"))
                .collect::<Vec<_>>()
                .join(", "),
            w = README_FIELDS[4]
        ));
    }
    let others: Vec<&ShardPlan> = siblings.iter().filter(|o| o.id != shard.id).collect();
    if !others.is_empty() {
        s.push_str("Your SIBLINGS implement the rest — read their split so you neither duplicate nor depend on writing it:\n");
        for o in others {
            let mut detail = String::new();
            if !o.provides.is_empty() {
                detail.push_str(&format!(" (provides {})", o.provides.join(", ")));
            }
            if !o.writes.is_empty() {
                detail.push_str(&format!(
                    " (the ONLY writer of {} — you read it, never write it)",
                    o.writes.join(", ")
                ));
            }
            s.push_str(&format!(
                "- `{}`: {}{}\n",
                o.id,
                o.responsibility.trim(),
                detail
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
         {w}: <a shared state you WRITE, with the shape you write (fields/types, stride/offsets)> — \
         one per line, or `{w}: none`; you write ONLY the state your split names you the writer of, \
         every other state you read\n\
         End your final message with the same five fields (they are your HANDOFF to the merger).\n\n\
         THE MODULE'S BRIEF — whole. Your split is the part named above; the rest is the context \
         your siblings implement and the answers you all build to:\n\n{module_brief}",
        p = README_FIELDS[0],
        a = README_FIELDS[1],
        u = README_FIELDS[2],
        c = README_FIELDS[3],
        w = README_FIELDS[4],
    ));
    s
}

/// Build and apply the split as a PATCH: N shard tasks added (folder README as the owned file,
/// no deps, the shard brief), the module's deps widened to the shards; then the engine's own
/// annotations `shard_of` / `merger_of` (plan metadata the scheduler carries to dispatch — a
/// model never writes these). Returns the patched plan and its events: the `plan_patched`, and
/// before it a `shard_difficulty_defaulted` when the module task carries no difficulty for its
/// shards to inherit (VA-080: a literal `hard` stood in for the absence and nothing said so).
pub(super) fn apply_module_split(
    plan_json: &str,
    module_id: &str,
    split: &ModuleSplit,
) -> Result<(String, Vec<serde_json::Value>), String> {
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
    // VA-113: a shard's weight is its share of the module's — `max(1, weight / shards)` — so the
    // scheduler ranks it by its real size; the merger keeps the module's whole weight (its chain
    // is what the shards' remaining-chain weight counts). A module without a weight (a caller
    // that never weighed, or a zero-section task) leaves its shards without one — the absence
    // stays absent for the scheduler's own derivation.
    let shard_weight: Option<u64> = module
        .get("weight")
        .and_then(|w| w.as_u64())
        .map(|w| (w / split.shards.len().max(1) as u64).max(1));
    // A shard is a piece of its module and as hard as the module: its difficulty is the module
    // task's own, verbatim ("medium" stays "medium"). A module with none leaves its shards with
    // none — `specs_from_plan_json` reads the absence the same way for the merger and its shards,
    // so they still sort together at claim time — and the absence is SAID once below, never a
    // literal in its place.
    let difficulty: Option<String> = module
        .get("difficulty")
        .and_then(|d| d.as_str())
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(String::from);
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
            difficulty: difficulty.clone(),
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
                module_files: module_files.clone(),
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
                if let Some(w) = shard_weight {
                    t["weight"] = serde_json::json!(w);
                }
            }
        }
    }
    let out = v.to_string();
    let mut events = Vec::new();
    if difficulty.is_none() {
        events.push(serde_json::json!({
            "event": "shard_difficulty_defaulted",
            "module": module_id,
            "shards": shard_ids,
            "reason": "the module task carries no difficulty; its shards carry none either and the DAG reads both as its default",
        }));
    }
    // VA-102: a shard synthesis gave no exports has no PROVIDES lines to copy into its README; the
    // brief says so in their place and the run log says it here — never a phrase standing in.
    for (s, id) in split.shards.iter().zip(shard_ids.iter()) {
        if s.provides.is_empty() {
            events.push(serde_json::json!({
                "event": "shard_provides_empty",
                "module": module_id,
                "shard": id,
                "sections": s.sections,
                "reason": "synthesis declared no exports for this shard; its brief states the absence where the README's PROVIDES lines would be copied in",
            }));
        }
    }
    // ONE WRITER PER SHARED STATE (split v2 §4): the declaration names each state's single writer;
    // two shards claiming one state is the `cooperate` edge synthesis should have merged into one
    // shard. Said here from the declaration and again from the READMEs at the merger's dispatch;
    // the patch applies either way (MILD).
    let declared_writers: Vec<(String, Vec<String>)> = split
        .shards
        .iter()
        .zip(shard_ids.iter())
        .map(|(s, id)| (id.clone(), s.writes.clone()))
        .collect();
    events.extend(shared_state_writer_events(
        module_id,
        &shared_state_writers(&declared_writers),
        "declaration",
        None,
    ));
    events.push(serde_json::json!({
        "event": "plan_patched",
        "source": "split",
        "module": module_id,
        "shards": shard_ids,
        "difficulty": difficulty,
        "exports_declared": split.interface.exports.len(),
        "replace": patch.replace.len(),
        "add": patch.add.len(),
        "remove": patch.remove.len(),
        "after": decomposition_of(&out),
    }));
    Ok((out, events))
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

/// What sizing did to one declaration (split v2 §6): the clusters synthesis declared, the free
/// hosts at split time, and the shards that will run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Sizing {
    pub(super) declared: usize,
    pub(super) hosts: Option<usize>,
    /// The declared shard ids behind each shard that runs — identity when nothing was grouped.
    pub(super) groups: Vec<Vec<String>>,
    /// The weight each declared cluster carried into the partition: its claimed sections, floor 1.
    pub(super) weights: Vec<usize>,
}

/// Exactly `k` contiguous, non-empty groups over `weights` minimising the largest group's sum —
/// the linear-partition DP (n clusters × k hosts, both single digits here). `k > n` is `n` groups.
fn partition_contiguous(weights: &[usize], k: usize) -> Vec<std::ops::Range<usize>> {
    let n = weights.len();
    if k == 0 || n == 0 {
        return Vec::new();
    }
    let k = k.min(n);
    let mut prefix = vec![0usize; n + 1];
    for (i, w) in weights.iter().enumerate() {
        prefix[i + 1] = prefix[i] + w;
    }
    let sum = |a: usize, b: usize| prefix[b] - prefix[a];
    let inf = usize::MAX;
    // best[j][i]: the smallest possible largest-group sum splitting clusters 0..i into j groups;
    // cut[j][i]: where that split's last group starts.
    let mut best = vec![vec![inf; n + 1]; k + 1];
    let mut cut = vec![vec![0usize; n + 1]; k + 1];
    best[0][0] = 0;
    for j in 1..=k {
        for i in j..=n {
            for s in (j - 1)..i {
                if best[j - 1][s] == inf {
                    continue;
                }
                let cand = best[j - 1][s].max(sum(s, i));
                if cand < best[j][i] {
                    best[j][i] = cand;
                    cut[j][i] = s;
                }
            }
        }
    }
    let mut ranges = Vec::with_capacity(k);
    let mut i = n;
    for j in (1..=k).rev() {
        let s = cut[j][i];
        ranges.push(s..i);
        i = s;
    }
    ranges.reverse();
    ranges
}

/// SIZE BY THE FLEET, not by the declaration's count (split v2 §6: "three focused shards on three
/// nodes beat eight queued" — r6e declared EIGHT shards for a THREE-node pool and five queued).
/// The declaration's clusters arrive in layout order and are grouped CONTIGUOUSLY onto the free
/// hosts, the largest group's weight minimised (a cluster weighs its claimed sections, floor 1);
/// one shard per group — id, responsibility, sections, provides and writes the union. No more
/// clusters than hosts leaves the declaration as it is; no host count leaves it too and is SAID
/// (`split_sized{hosts: null}`), never replaced by a literal.
pub(super) fn size_shards_to_hosts(
    split: ModuleSplit,
    free_hosts: Option<usize>,
) -> (ModuleSplit, Sizing) {
    let declared = split.shards.len();
    let weights: Vec<usize> = split
        .shards
        .iter()
        .map(|s| s.sections.len().max(1))
        .collect();
    let Some(hosts) = free_hosts.filter(|h| *h >= 2 && *h < declared) else {
        let groups = split.shards.iter().map(|s| vec![s.id.clone()]).collect();
        let shards = split
            .shards
            .into_iter()
            .map(|s| ShardPlan {
                clusters: vec![cluster_of(&s)],
                ..s
            })
            .collect();
        return (
            ModuleSplit {
                interface: split.interface,
                shards,
            },
            Sizing {
                declared,
                hosts: free_hosts,
                groups,
                weights,
            },
        );
    };
    let mut shards: Vec<ShardPlan> = Vec::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for r in partition_contiguous(&weights, hosts) {
        let members = &split.shards[r];
        groups.push(members.iter().map(|s| s.id.clone()).collect());
        if let [only] = members {
            shards.push(ShardPlan {
                clusters: vec![cluster_of(only)],
                ..only.clone()
            });
            continue;
        }
        let mut merged = ShardPlan {
            id: members
                .iter()
                .map(|s| kebab(&s.id))
                .collect::<Vec<_>>()
                .join("-"),
            responsibility: members
                .iter()
                .map(|s| s.responsibility.trim().to_string())
                .collect::<Vec<_>>()
                .join("; "),
            sections: Vec::new(),
            provides: Vec::new(),
            writes: Vec::new(),
            clusters: members.iter().map(cluster_of).collect(),
        };
        let union = |into: &mut Vec<String>, from: &[String]| {
            for x in from {
                if !into.contains(x) {
                    into.push(x.clone());
                }
            }
        };
        for s in members {
            union(&mut merged.sections, &s.sections);
            union(&mut merged.provides, &s.provides);
            union(&mut merged.writes, &s.writes);
        }
        shards.push(merged);
    }
    (
        ModuleSplit {
            interface: split.interface,
            shards,
        },
        Sizing {
            declared,
            hosts: free_hosts,
            groups,
            weights,
        },
    )
}

pub(super) fn sized_event(module: &str, sizing: &Sizing) -> serde_json::Value {
    let source = if sizing.hosts.is_none() {
        "declaration — free host count not passed by the caller (plan_slices_to_dag); the clusters stand as declared"
    } else if sizing.groups.len() < sizing.declared {
        "fleet — clusters grouped contiguously onto the free hosts, largest group minimised"
    } else {
        "declaration — no more clusters than free hosts"
    };
    serde_json::json!({
        "event": "split_sized",
        "module": module,
        "declared": sizing.declared,
        "hosts": sizing.hosts,
        "shards": sizing.groups.len(),
        "groups": sizing.groups,
        "weights": sizing.weights,
        "source": source,
    })
}

/// THE TASK'S WEIGHT IS ITS CLAIMED SECTION COUNT (VA-113, the CLI half of the scheduler's
/// 859a2b419): `Dag::from_planner_json` reads an optional per-task `"weight"` (u32 > 0) and
/// orders READY tasks by remaining chain weight; absent, it derives files × difficulty. The
/// number the engine already measured here — `TaskDensity.sections`, the same unit
/// `plan_flag{sections}` and `research_planned.per_slice_sections` report — is written onto every
/// section-claiming task, so the heaviest module is dispatched first instead of wherever plan
/// order put it (r6i: ledgerd's 17 sections dispatched last, to the slowest node). A task with
/// zero claimed sections gets NO weight — the scheduler's derivation stands and the absence is
/// honest (`unclaimed` names the rows the opener routed nothing to). Returns what was weighed.
pub(super) fn weigh_tasks_by_sections(
    plan: &mut serde_json::Value,
    measure: &FatMeasure,
) -> Vec<(String, usize)> {
    let mut weighed = Vec::new();
    let Some(tasks) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) else {
        return weighed;
    };
    for r in measure.rows.iter().filter(|r| r.sections > 0) {
        if let Some(t) = tasks.iter_mut().find(|t| t["id"] == r.id) {
            t["weight"] = serde_json::json!(r.sections);
            weighed.push((r.id.clone(), r.sections));
        }
    }
    weighed
}

/// The split step of `plan_slices_to_dag`: measure, weigh, flag, request one patch per fat task,
/// apply, and walk the patched plan through the one door again. `split` is injected (the real
/// one calls `request_module_split`; a test hands back a canned reply) so the whole sequence
/// runs without a model. A plan with no fat task returns with only the section weights added
/// (`plan_weighted`, VA-113) and no other event; a plan with no weighable task returns
/// byte-identical.
///
/// SIZE BY THE FLEET (split v2 §6): `free_hosts` is the pool free at split time — the pool
/// `run_swarm` resolved (`pool_resolved.worker_count`; r6e: 3), all free during planning by
/// construction, threaded to the one call in `plan_slices_to_dag`. `None` = the caller did not
/// pass it, which is SAID (`split_sized{hosts: null, source: "declaration — free host count not
/// passed…"}`) and leaves the shard count as declared — never a literal in its place. One free
/// host declines every split before synthesis is asked (N shards would queue serially on it and
/// still need a merge — the module is built whole by the node that would build it anyway; the
/// flag stays); otherwise the declared clusters are sized onto the hosts (`size_shards_to_hosts`,
/// `split_sized`) before the patch.
pub(super) async fn split_fat_tasks_sized<P, PFut>(
    plan_json: String,
    opened: &OpenOutput,
    spec: &str,
    every_decision_settled: bool,
    free_hosts: Option<usize>,
    split: P,
    sink: &Arc<dyn EventSink>,
) -> String
where
    P: Fn(serde_json::Value, serde_json::Value) -> PFut,
    PFut: std::future::Future<Output = Result<String>>,
{
    let Ok(mut plan) = serde_json::from_str::<serde_json::Value>(&plan_json) else {
        return plan_json;
    };
    let claims = section_claims(opened, spec);
    let measure = measure_fatness(&plan, &claims);
    // VA-113: the weights ride EVERY return path below — a declined split, a flat plan and an
    // unmeasurable one all carry them — because they are a fact about the measured plan, not
    // about the split. Re-serialized only when something was weighed, so a plan with nothing to
    // weigh stays byte-identical.
    let weighed = weigh_tasks_by_sections(&mut plan, &measure);
    let plan_json = if weighed.is_empty() {
        plan_json
    } else {
        sink.write_value(serde_json::json!({
            "event": "plan_weighted",
            "unit": "claimed spec sections",
            "weights": weighed
                .iter()
                .map(|(id, n)| (id.clone(), *n))
                .collect::<std::collections::BTreeMap<_, _>>(),
        }));
        plan.to_string()
    };
    // NOTHING is measurable when no row carries a section: the opener's slice names matched no
    // plan slice (every row unclaimed) or the opener claimed no sections at all. `plan_flag`
    // fires only for FAT rows, so this plan would otherwise pass the split in silence looking
    // like a flat distribution (VA-080 item 3). Said with both sides' names; the plan is untouched.
    if measure.rows.iter().all(|r| r.sections == 0) {
        let tasks: Vec<&str> = measure.rows.iter().map(|r| r.id.as_str()).collect();
        let mut opener_slices: Vec<&str> = claims.keys().map(String::as_str).collect();
        opener_slices.sort_unstable();
        let reason = if measure.rows.is_empty() {
            "no task owns a file"
        } else {
            "no task carries sections"
        };
        eprintln!(
            "  · fatness unmeasurable ({reason}): tasks {tasks:?}, unclaimed by any opener slice {:?}, opener slices {opener_slices:?}",
            measure.unclaimed
        );
        sink.write_value(serde_json::json!({
            "event": "fatness_unmeasurable",
            "reason": reason,
            "tasks": tasks,
            "unclaimed_tasks": measure.unclaimed,
            "opener_slices": opener_slices,
        }));
        return plan_json;
    }
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
            o.insert("floor".into(), measure.floor.into());
        }
        eprintln!(
            "  · fat task `{}`: {} spec sections for {} file(s) = {:.1}/file (median {:.1}, floor {:.1}, threshold {:.1}) — asking synthesis for a split patch",
            row.id,
            row.sections,
            row.files.len(),
            row.sections_per_file,
            measure.median,
            measure.floor,
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
        // SIZE BY THE FLEET (split v2 §6): with fewer than two FREE hosts at split time the shards
        // cannot be partitioned onto hosts, so the declaration's clusters are the shards as declared
        // (`size_shards_to_hosts` applies only at >= 2). The split itself is NEVER declined for
        // scarcity — r6g measured shards dispatching as nodes freed (two at BUILD start, two later),
        // so "they would queue" is a fleet fact, not a reason to build the fat file whole. Said:
        if let Some(h) = free_hosts.filter(|h| *h < 2) {
            sink.write_value(serde_json::json!({
                "event": "split_hosts_scarce",
                "task": row.id,
                "free_hosts": h,
                "detail": "fewer than two free hosts at split time — shards are sized by the declaration's clusters and queue as nodes free",
            }));
        }
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
        let (parsed, sizing) = size_shards_to_hosts(parsed, free_hosts);
        sink.write_value(sized_event(&row.id, &sizing));
        match apply_module_split(&current, &row.id, &parsed) {
            Ok((next, events)) => {
                eprintln!(
                    "  · `{}` split into {} shards + a merger; {} exports declared",
                    row.id,
                    parsed.shards.len(),
                    parsed.interface.exports.len()
                );
                for event in events {
                    sink.write_value(event);
                }
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
                        headings: (1..=*n).map(|k| format!("### {k}. Section {k}")).collect(),
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
        // decisions-doc claims no section: it rides the distribution row but not the statistics
        // (five section-claiming rows: 0.5, 0.667, 1.5, 2.0, 7.0).
        assert!((m.median - 1.5).abs() < 1e-9, "median {}", m.median);
        assert!((m.floor - 3.0).abs() < 1e-9, "floor {}", m.floor);
        assert!(
            m.threshold > 4.7 && m.threshold < 4.8,
            "threshold {}",
            m.threshold
        );
        let ev = &m.events()[0];
        assert_eq!(ev["event"], "plan_flag");
        assert_eq!(ev["kind"], "fat_task");
        assert_eq!(ev["task"], "web-viz");
        assert_eq!(ev["sections_per_file"], 7.0);
        assert_eq!(ev["floor"], 3.0);
        assert_eq!(ev["claimed_sections"].as_array().unwrap().len(), 7);
        assert_eq!(ev["distribution"].as_array().unwrap().len(), 6);

        // S10(2): the same plan WITHOUT web-viz — mean + stddev alone (1.78) would flag
        // ledgerd-core (2.0/file, the new maximum); the 2×median floor (2.17) is what says no.
        let mut r6c_minus = r6c.clone();
        r6c_minus["subtasks"]
            .as_array_mut()
            .unwrap()
            .retain(|t| t["id"] != "web-viz");
        let m = measure_fatness(
            &r6c_minus,
            &claims(&[
                ("ledgerd-core", 12),
                ("ledgerd-api", 6),
                ("notifierd", 1),
                ("web-console", 2),
            ]),
        );
        let core = m.rows.iter().find(|r| r.id == "ledgerd-core").unwrap();
        assert!(
            core.sections_per_file > m.threshold,
            "mean+stddev alone flags the maximum: 2.0 vs {}",
            m.threshold
        );
        assert!(core.sections_per_file < m.floor, "floor {}", m.floor);
        assert!(m.fat.is_empty(), "{m:?}");

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
        assert!((m.floor - 3.1667).abs() < 0.001, "floor {}", m.floor);
        assert!(
            m.threshold > 6.5 && m.threshold < 6.6,
            "threshold {}",
            m.threshold
        );
    }

    /// S10(1)+(2): the request lists the CLAIMED headings as the partition (the brief is context),
    /// and the prompt's decline ramp — one shard named `whole` — parses as a loud decline with the
    /// planner's reason, so `split_fat_tasks_sized` emits `split_declined` and leaves the plan alone.
    #[test]
    fn the_split_request_partitions_the_claimed_headings_and_whole_declines_with_a_reason() {
        let task = serde_json::json!({
            "id": "web-viz",
            "files": ["web/viz.js"],
            "description": "### 4. Boot\nconsumed context\n\n### 7. WebGL field\nthe work",
        });
        let density = serde_json::json!({
            "sections": 2,
            "claimed_sections": ["### 7. WebGL field", "### 8. Picking & camera"],
            "sections_per_file": 2.0,
            "median": 1.0,
            "floor": 2.0,
            "threshold": 1.8,
            "brief_chars": 60,
        });
        let text = split_user_text(&task, &density);
        let partition = text
            .split("## The sections to partition")
            .nth(1)
            .and_then(|t| t.split("## Its brief").next())
            .expect("the partition section precedes the brief");
        assert!(partition.contains("- ### 7. WebGL field"), "{text}");
        assert!(partition.contains("- ### 8. Picking & camera"), "{text}");
        assert!(
            !partition.contains("### 4. Boot"),
            "consumed context is not a heading to partition: {text}"
        );
        assert!(text.contains("2 claimed heading(s)"), "{text}");
        assert!(text.contains("floor 2×median 2.00"), "{text}");
        assert!(split_system_prompt().contains("return exactly ONE shard with id `whole`"));

        let declined = parse_module_split(
            r#"{"interface": {"exports": [], "shared_state": "", "layout": []}, "shards": [{"id": "whole", "responsibility": "one render loop; the seven sections share every buffer", "sections": [], "provides": []}]}"#,
        );
        assert_eq!(
            declined,
            Err(
                "declined by synthesis: one render loop; the seven sections share every buffer"
                    .to_string()
            )
        );
        assert_eq!(
            parse_module_split(r#"{"shards": [{"id": "WHOLE", "responsibility": ""}]}"#),
            Err("declined by synthesis: (no reason given)".to_string())
        );
        assert_eq!(
            parse_module_split(r#"{"shards": [{"id": "render", "responsibility": "x"}]}"#),
            Err("1 shard(s) — a split needs at least two".to_string())
        );
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

    /// VA-080 item 3: when the opener's slice names match no plan slice, every row measures
    /// zero and nothing is ever fat — `plan_flag` fires only for fat rows, so the plan used to
    /// pass in silence. The measure RECORDS the unclaimed ids; `split_fat_tasks_sized` says
    /// `fatness_unmeasurable` with both sides' names and leaves the plan byte-identical. r6c's
    /// shape (SOME rows carry sections) says nothing — the NET half.
    #[tokio::test]
    async fn a_plan_whose_slices_the_opener_never_named_is_said_unmeasurable() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let renamed = plan(&[
            ("ledger-service", &["app/db.py", "app/ledger.py"]),
            ("notifier-service", &["app/notifierd/impl.py"]),
            ("console-ui", &["web/index.html", "web/app.js"]),
            ("viz-engine", &["web/viz.js"]),
        ]);
        let m = measure_fatness(&renamed, &section_claims(&r6c_like_opened(), spec));
        assert_eq!(
            m.unclaimed,
            vec![
                "ledger-service",
                "notifier-service",
                "console-ui",
                "viz-engine"
            ]
        );
        assert!(m.fat.is_empty(), "{m:?}");
        assert!(m.rows.iter().all(|r| r.sections == 0));

        let sink = Arc::new(RecordingSink::default());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let before = renamed.to_string();
        let after = split_fat_tasks_sized(
            before.clone(),
            &r6c_like_opened(),
            spec,
            false,
            None,
            |_task, _density| async move { panic!("nothing measurable, nothing to split") },
            &sink_dyn,
        )
        .await;
        assert_eq!(after, before, "byte-identical");
        let events = sink.0.lock().unwrap().clone();
        assert_eq!(events.len(), 1, "{events:?}");
        let said = &events[0];
        assert_eq!(said["event"], "fatness_unmeasurable");
        assert_eq!(said["reason"], "no task carries sections");
        assert_eq!(
            string_list(&said["tasks"]),
            vec![
                "ledger-service",
                "notifier-service",
                "console-ui",
                "viz-engine"
            ]
        );
        assert_eq!(
            string_list(&said["unclaimed_tasks"]),
            string_list(&said["tasks"])
        );
        assert_eq!(
            string_list(&said["opener_slices"]),
            vec!["ledgerd-core", "notifierd", "web-console", "web-viz"]
        );

        // The NET half: r6c's own names — SOME rows carry sections, nothing is said.
        let m = measure_fatness(
            &plan(&[
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
                ("decisions-doc", &["DECISIONS.md"]),
            ]),
            &section_claims(&r6c_like_opened(), spec),
        );
        assert_eq!(
            m.unclaimed,
            vec!["decisions-doc"],
            "one patch-added task, recorded"
        );
        assert!(m.rows.iter().any(|r| r.sections > 0));
        assert_eq!(m.fat.len(), 1, "web-viz stays the one fat task: {m:?}");
    }

    /// r6h's declaration of `viz-engine`, verbatim from the run's `.swarm/plan-loaded.json`
    /// (30 exports: name/kind/signature; the 9 provides of `camera-labels-brush`) and its
    /// `split_sized` event (seq 466: five clusters, weights [2,3,1,2,4], three hosts, groups
    /// [[data-stream, render-pick], [camera, labels-brush], [debug-api]]). Purposes omitted —
    /// the brief's first paragraph never reads them.
    fn r6h_viz_engine_declaration() -> ModuleSplit {
        serde_json::from_value(serde_json::json!({
            "interface": {
                "exports": [
                    {"name": "viz3d.toggleBrush", "kind": "method (window.viz3d)", "signature": "toggleBrush(id: string): void"},
                    {"name": "viz3d.clearBrush", "kind": "method (window.viz3d)", "signature": "clearBrush(): void"},
                    {"name": "viz3d.brush", "kind": "method (window.viz3d)", "signature": "brush(): string[]"},
                    {"name": "viz3d.onBrush", "kind": "method (window.viz3d)", "signature": "onBrush(cb: (ids: string[]) => void): void"},
                    {"name": "vs7dbg.layout", "kind": "method (window.vs7dbg)", "signature": "layout(): {d0: string, D0: number, R0: number}"},
                    {"name": "vs7dbg.sceneDigest", "kind": "method (window.vs7dbg)", "signature": "sceneDigest(): {count: number, Sh: number, Sh2: number, Sx: number, Sz: number, Sxh: number, Szh: number, brushedCount: number}"},
                    {"name": "vs7dbg.camera", "kind": "method (window.vs7dbg)", "signature": "camera(): {yaw: number, pitch: number, distance: number, vyaw: number, vpitch: number}"},
                    {"name": "vs7dbg.setCamera", "kind": "method (window.vs7dbg)", "signature": "setCamera(yaw: number, pitch: number, distance: number): void"},
                    {"name": "vs7dbg.pick", "kind": "method (window.vs7dbg)", "signature": "pick(sx: number, sy: number): {id: string, index: number} | null"},
                    {"name": "vs7dbg.pickPixel", "kind": "method (window.vs7dbg)", "signature": "pickPixel(sx: number, sy: number): [number, number, number, number]"},
                    {"name": "vs7dbg.brush", "kind": "method (window.vs7dbg)", "signature": "brush(): string[]"},
                    {"name": "vs7dbg.frames", "kind": "method (window.vs7dbg)", "signature": "frames(): number"},
                    {"name": "loadRecords", "kind": "function (data-stream)", "signature": "loadRecords(): Promise<void>"},
                    {"name": "applyBatch", "kind": "function (data-stream)", "signature": "applyBatch(batch: {batch: number, records: object[]}): Set<string>"},
                    {"name": "heightFor", "kind": "function (data-stream)", "signature": "heightFor(amountMinor: number, currency: string): number"},
                    {"name": "topColorRGB", "kind": "function (data-stream)", "signature": "topColorRGB(status: string): [number, number, number]"},
                    {"name": "onStreamMessage", "kind": "event-handler (data-stream)", "signature": "onStreamMessage(event: MessageEvent): void"},
                    {"name": "initViz", "kind": "function (render-pick)", "signature": "initViz(): boolean"},
                    {"name": "renderFrame", "kind": "function (render-pick)", "signature": "renderFrame(): void"},
                    {"name": "requestRender", "kind": "function (render-pick)", "signature": "requestRender(): void"},
                    {"name": "pickCore", "kind": "function (render-pick)", "signature": "pickCore(sx: number, sy: number): {id: string, index: number} | null"},
                    {"name": "pickPixelCore", "kind": "function (render-pick)", "signature": "pickPixelCore(sx: number, sy: number): [number, number, number, number]"},
                    {"name": "setPanelState", "kind": "function (render-pick)", "signature": "setPanelState(state: 'ready' | 'empty' | 'error' | 'unavailable'): void"},
                    {"name": "bindClickInput", "kind": "event-handler (render-pick)", "signature": "bindClickInput(canvas: HTMLCanvasElement): void"},
                    {"name": "project", "kind": "function (camera)", "signature": "project(x: number, y: number, z: number): {sx: number, sy: number} | null"},
                    {"name": "getCamera", "kind": "function (camera)", "signature": "getCamera(): {yaw: number, pitch: number, distance: number, vyaw: number, vpitch: number}"},
                    {"name": "setCameraCore", "kind": "function (camera)", "signature": "setCameraCore(yaw: number, pitch: number, distance: number): void"},
                    {"name": "bindCameraInput", "kind": "event-handler (camera)", "signature": "bindCameraInput(canvas: HTMLCanvasElement): void"},
                    {"name": "updateLabels", "kind": "function (labels-brush)", "signature": "updateLabels(): void"},
                    {"name": "boot", "kind": "function (debug-api)", "signature": "boot(): Promise<void>"}
                ],
                "shared_state": "records — columnar full collection; WRITTEN by data-stream. instanceGeom — Float32Array stride 6 [x, z, h, topR, topG, topB]; WRITTEN by data-stream. layoutBasis — {d0, D0, R0}; WRITTEN by data-stream. digestSums; WRITTEN by data-stream. camera — {yaw, pitch, distance, vyaw, vpitch}; WRITTEN by camera. brushSet + brushFlag; WRITTEN by labels-brush. frames; WRITTEN by render-pick.",
                "layout": ["Constants", "Shared state", "Data → scene", "Streaming", "Camera", "Rendering", "Pick buffer", "Labels", "Brush", "vs7dbg facade", "Boot"]
            },
            "shards": [
                {"id": "data-stream", "responsibility": "Fetch /api/viz/records once, build and maintain the per-instance scene state (stable arrival index n, locked layout basis {d0,D0,R0}, x/z/h geometry with currency-exponent heights, exact status colors, float64 digest sums), and apply SSE batches to exactly the minimal changed set under the byte-accounting budget.", "sections": ["Data → scene", "Streaming diffs — SSE with byte accounting"], "provides": ["loadRecords", "applyBatch", "heightFor", "topColorRGB", "onStreamMessage"], "writes": ["records", "instanceGeom", "layoutBasis", "digestSums"]},
                {"id": "render-pick", "responsibility": "Create the main-thread WebGL context on #viz3d with DPR-sized backing store, run instanced draws within the ≤8 default-FBO budget under demand-only rendering, maintain the offscreen RGBA8+depth pick FBO with idNum encoding and real-pass accounting for pick/pickPixel, own click-to-brush semantics, and drive the canvas panel states (#viz-empty, #viz-error, 3D-unavailable).", "sections": ["7. `web/` — the frontend", "Rendering — bounded draw calls, demand rendering", "The pick buffer"], "provides": ["initViz", "renderFrame", "requestRender", "pickCore", "pickPixelCore", "setPanelState", "bindClickInput"], "writes": ["frames"]},
                {"id": "camera", "responsibility": "Own the orbit camera state and exact projection contract, wire drag/wheel-consumed/double-click input on the canvas, and implement the closed-form τ=0.4 s inertia coast with continuous clamps, stop threshold, and cancel rules satisfying the remaining-coast identity and settle budget.", "sections": ["Camera — orbit + inertia"], "provides": ["project", "getCamera", "setCameraCore", "bindCameraInput"], "writes": ["camera"]},
                {"id": "labels-brush", "responsibility": "Maintain the ONE shared brush set exposed via window.viz3d (with per-instance dim flag upload ≤ stride+4096 and no realloc, plus D1 behavior for streamed mutations) and render the 12 top-a_major DOM labels with pick-buffer occlusion culling and deterministic greedy collision culling.", "sections": ["Screen-space labels — deterministic collision culling", "The linked brush — table ⇄ instances"], "provides": ["viz3d.toggleBrush", "viz3d.clearBrush", "viz3d.brush", "viz3d.onBrush", "updateLabels"], "writes": ["brushSet", "brushFlag"]},
                {"id": "debug-api", "responsibility": "Wire window.vs7dbg as the synchronous, truthful graded facade over all other shards and own boot-time assembly, carrying the cross-cutting contracts (section 8 overview, performance budgets, rules) that the whole module must satisfy.", "sections": ["8. The 3D field — 12,288 instances, five mechanisms", "`vs7dbg` — REQUIRED and graded", "Performance budgets", "Rules"], "provides": ["vs7dbg.layout", "vs7dbg.sceneDigest", "vs7dbg.camera", "vs7dbg.setCamera", "vs7dbg.pick", "vs7dbg.pickPixel", "vs7dbg.brush", "vs7dbg.frames", "boot"]}
            ]
        }))
        .unwrap()
    }

    /// VA-102 (r6h, BUILD 05:13→06:00, three shards of `viz-engine`, zero live bytes): the shard
    /// `camera-labels-brush` reasoned 102k chars — 49% inside code fences, full piece bodies — with
    /// one `ls` and no file; `data-stream-render-pick` 102k chars, 3 calls, 0 files. Their briefs
    /// listed every declared name and put the README's structure last. Now, on r6h's own
    /// declaration sized onto its three hosts: the first instruction names the README with its
    /// PROVIDES lines copied in from the declaration (dotted names rejoined with their
    /// signatures), then the LIGHTEST DECLARED CLUSTER as the first piece — `camera` (weight 1),
    /// not `viz3d.brush` (the shortest signature, a one-line getter, which the string-length proxy
    /// picked); `data-stream` (weight 2) before `render-pick` (3), not `initViz`; and `debug-api`,
    /// one cluster, is told it is its one piece.
    #[test]
    fn the_shard_brief_orders_the_readme_write_before_the_design_r6h() {
        let (sized, sizing) = size_shards_to_hosts(r6h_viz_engine_declaration(), Some(3));
        assert_eq!(
            sizing.weights,
            vec![2, 3, 1, 2, 4],
            "the run's split_sized.weights"
        );
        assert_eq!(
            sizing.groups,
            vec![
                vec!["data-stream".to_string(), "render-pick".to_string()],
                vec!["camera".to_string(), "labels-brush".to_string()],
                vec!["debug-api".to_string()],
            ],
            "the run's split_sized.groups"
        );
        assert_eq!(
            sized.shards[1]
                .clusters
                .iter()
                .map(|k| (k.id.as_str(), k.weight))
                .collect::<Vec<_>>(),
            vec![("camera", 1), ("labels-brush", 2)],
            "grouping keeps the cluster boundaries"
        );
        let p = plan(&[
            ("console-page", &["web/index.html"]),
            ("viz-engine", &["web/viz.js"]),
        ]);
        let (out, events) = apply_module_split(&p.to_string(), "viz-engine", &sized).unwrap();
        assert!(
            !events.iter().any(|e| e["event"] == "shard_provides_empty"),
            "every r6h shard provides names"
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let brief_of = |id: &str| -> String {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .unwrap_or_else(|| panic!("{id} missing"))["description"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let b = brief_of("viz-engine-camera-labels-brush");
        let first = b.find("YOUR FIRST ACTION").expect("the first instruction");
        let where_you_work = b.find("WHERE YOU WORK").unwrap();
        assert!(first < where_you_work, "{b}");
        assert!(first < b.find("THE MODULE'S DECLARED INTERFACE").unwrap());
        assert!(first < b.find("THE MODULE'S BRIEF").unwrap());
        let readme_at = b
            .find("write `.swarm/shards/viz-engine/camera-labels-brush/README.md`, first version")
            .expect("the README is the first path the instruction names");
        assert!(first < readme_at && readme_at < where_you_work);
        assert!(
            b.contains(
                "PROVIDES: project(x: number, y: number, z: number): {sx: number, sy: number} | null\n\
                 - getCamera(): {yaw: number, pitch: number, distance: number, vyaw: number, vpitch: number}\n\
                 - setCameraCore(yaw: number, pitch: number, distance: number): void\n\
                 - bindCameraInput(canvas: HTMLCanvasElement): void\n\
                 - viz3d.toggleBrush(id: string): void\n\
                 - viz3d.clearBrush(): void\n\
                 - viz3d.brush(): string[]\n\
                 - viz3d.onBrush(cb: (ids: string[]) => void): void\n\
                 - updateLabels(): void\n"
            ),
            "the 9 provides as copy-in signature lines, dotted names rejoined: {b}"
        );
        assert!(b.contains("UNFINISHED: camera\n- labels-brush\n"), "{b}");
        assert!(
            b.contains("WRITES: camera\n- brushSet\n- brushFlag\n"),
            "{b}"
        );
        assert!(
            b.contains(
                "Then write your FIRST PIECE: `.swarm/shards/viz-engine/camera-labels-brush/camera.js` \
                 — the `camera` cluster, the lightest of your 2 (weight 1: its claimed spec sections, \
                 floor 1): project, getCamera, setCameraCore, bindCameraInput — alone"
            ),
            "the lightest declared cluster, its exports listed: {b}"
        );
        assert!(
            !b.contains("viz3d-brush.js"),
            "REFUTED proxy: the shortest signature (`brush(): string[]`) is not the first piece"
        );
        assert!(b.contains("next piece (`labels-brush`)"));
        assert!(b.contains("Do NOT draft a file's body in your reasoning"));
        assert_eq!(
            kebab("viz3d.brush"),
            "viz3d-brush",
            "a dotted cluster id's piece path"
        );
        let ds = brief_of("viz-engine-data-stream-render-pick");
        assert!(
            ds.contains(
                "FIRST PIECE: `.swarm/shards/viz-engine/data-stream-render-pick/data-stream.js` — the \
                 `data-stream` cluster, the lightest of your 2 (weight 2"
            ),
            "weight 2 before weight 3; never `initViz` by signature length: {ds}"
        );
        assert!(!ds.contains("initviz.js"));
        let dbg = brief_of("viz-engine-debug-api");
        assert!(
            dbg.contains(
                "your one cluster `debug-api` is your one piece: \
                 `.swarm/shards/viz-engine/debug-api/debug-api.js` — vs7dbg.layout, vs7dbg.sceneDigest"
            ),
            "a single-cluster shard is told its one piece is the cluster: {dbg}"
        );
        assert!(dbg.contains("WRITES: none\n"));
        assert!(dbg.contains("- vs7dbg.brush(): string[]\n"));
        for id in [
            "viz-engine-camera-labels-brush",
            "viz-engine-data-stream-render-pick",
            "viz-engine-debug-api",
        ] {
            let b = brief_of(id);
            assert!(
                b.find("YOUR FIRST ACTION").unwrap() < b.find("WHERE YOU WORK").unwrap(),
                "{id}: the write precedes the design material"
            );
        }
    }

    /// A shard synthesis gave no exports: the absence is SAID (`shard_provides_empty`) and stated
    /// in the brief where the PROVIDES lines would be — the real section titles, never a phrase.
    #[test]
    fn a_shard_with_no_declared_exports_says_so_instead_of_a_phrase() {
        let mut split = r6h_viz_engine_declaration();
        split.shards[4].provides.clear();
        let (sized, _) = size_shards_to_hosts(split, Some(3));
        let p = plan(&[("viz-engine", &["web/viz.js"])]);
        let (out, events) = apply_module_split(&p.to_string(), "viz-engine", &sized).unwrap();
        let ev = events
            .iter()
            .find(|e| e["event"] == "shard_provides_empty")
            .expect("the absence is said");
        assert_eq!(ev["shard"], "viz-engine-debug-api");
        assert_eq!(ev["sections"][1], "`vs7dbg` — REQUIRED and graded");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let dbg = v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "viz-engine-debug-api")
            .unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            dbg.contains(
                "PROVIDES: (synthesis declared no exports for this shard — `shard_provides_empty` is on \
                 the run log; list each symbol you define for the sections `8. The 3D field"
            ),
            "{dbg}"
        );
        assert!(!dbg.contains("the names your sections require"));
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
        let (out, events) = apply_module_split(&p.to_string(), "web-viz", &viz_split()).unwrap();
        let event = events
            .iter()
            .find(|e| e["event"] == "plan_patched")
            .expect("the patch event");
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

    /// VA-080 item 1: a shard's difficulty is its module's, verbatim — never a literal standing in.
    /// A module planned "medium" gives "medium" shards and nothing is said; a module with no
    /// difficulty gives shards with none and ONE `shard_difficulty_defaulted` naming them.
    #[test]
    fn shards_carry_the_modules_difficulty_verbatim_and_a_module_without_one_is_said() {
        let mut p = plan(&[
            ("web-console", &["web/app.js"]),
            ("web-viz", &["web/viz.js"]),
        ]);
        p["subtasks"][1]["difficulty"] = "medium".into();
        let (out, events) = apply_module_split(&p.to_string(), "web-viz", &viz_split()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let shards: Vec<&serde_json::Value> = v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t.get("shard_of").is_some())
            .collect();
        assert_eq!(shards.len(), 3);
        for s in &shards {
            assert_eq!(s["difficulty"], "medium", "{}", s["id"]);
        }
        assert!(
            !events
                .iter()
                .any(|e| e["event"] == "shard_difficulty_defaulted"),
            "{events:?}"
        );
        let patched = events
            .iter()
            .find(|e| e["event"] == "plan_patched")
            .unwrap();
        assert_eq!(patched["difficulty"], "medium");

        let p = plan(&[
            ("web-console", &["web/app.js"]),
            ("web-viz", &["web/viz.js"]),
        ]);
        let (out, events) = apply_module_split(&p.to_string(), "web-viz", &viz_split()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        for t in v["subtasks"].as_array().unwrap() {
            if t.get("shard_of").is_some() {
                assert!(
                    t.get("difficulty").is_none(),
                    "no literal stands in for the module's absence: {t}"
                );
            }
        }
        let said: Vec<&serde_json::Value> = events
            .iter()
            .filter(|e| e["event"] == "shard_difficulty_defaulted")
            .collect();
        assert_eq!(said.len(), 1, "{events:?}");
        assert_eq!(said[0]["module"], "web-viz");
        assert_eq!(said[0]["shards"].as_array().unwrap().len(), 3);
        assert_eq!(
            events.last().unwrap()["event"],
            "plan_patched",
            "the absence is said before the patch it describes"
        );
        assert!(events.last().unwrap()["difficulty"].is_null());
    }

    /// VA-063, the r6e seam verbatim (killed at BUILD+4m): `viz3d-engine` (`web/viz.js`, no planner
    /// deps) split into these EIGHT shards, then walked the door with `every_decision_settled ==
    /// true` (D1–D3 settled by the `__open_decisions__` lane). The run's decision-doc gate read all
    /// eight shard READMEs as docs-only and stripped them off the merger — `plan_repaired{source:
    /// split}` 16:28:46Z "dep dropped", `task_dispatched viz3d-engine deps: []` at plan_loaded,
    /// `merge_dossier{pieces: 0}`. The sibling test above passes `false` and could never see it.
    #[test]
    fn the_split_merger_keeps_its_shards_through_the_door_when_every_decision_settled() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let archived_shards = [
            "data-scene",
            "rendering-core",
            "pick-buffer",
            "camera-inertia",
            "labels-culling",
            "linked-brush",
            "streaming-diffs",
            "vs7dbg-boot",
        ];
        let split: ModuleSplit = serde_json::from_value(serde_json::json!({
            "interface": {
                "exports": [
                    {"name": "window.vs7dbg.layout", "kind": "function", "signature": "layout() -> {d0, D0, R0}", "purpose": "the locked layout basis"},
                    {"name": "buildScene", "kind": "function", "signature": "buildScene(data) -> void", "purpose": "fill the instance buffers"}
                ],
                "shared_state": "S = {yaw, pitch, distance, brush: Set<id>, count, dirty}",
                "layout": ["constants", "state S", "GL programs", "pick FBO", "camera", "labels", "brush", "stream", "window.vs7dbg", "boot"]
            },
            "shards": archived_shards.iter().map(|id| serde_json::json!({
                "id": id, "responsibility": format!("the {id} piece"), "sections": [], "provides": []
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        let p = plan(&[
            (
                "frontend-console",
                &[
                    "web/index.html",
                    "web/styles.css",
                    "web/app.js",
                    "DECISIONS.md",
                ],
            ),
            ("viz3d-engine", &["web/viz.js"]),
        ]);
        let (patched, events) = apply_module_split(&p.to_string(), "viz3d-engine", &split).unwrap();
        let event = events
            .iter()
            .find(|e| e["event"] == "plan_patched")
            .expect("the patch event");
        let expected: Vec<String> = archived_shards
            .iter()
            .map(|x| format!("viz3d-engine-{x}"))
            .collect();
        assert_eq!(string_list(&event["shards"]), expected, "the archived ids");

        let sink = Arc::new(RecordingSink::default());
        let dyn_sink: Arc<dyn EventSink> = sink.clone();
        let finalized = finalize_plan_before_dag(patched, spec, true, &dyn_sink, "split");
        let f: serde_json::Value = serde_json::from_str(&finalized).unwrap();
        let merger = f["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "viz3d-engine")
            .expect("the merger survives the door");
        let deps = string_list(&merger["depends_on"]);
        for id in &expected {
            assert!(deps.contains(id), "merger lost `{id}`: {deps:?}");
        }
        assert!(merger["merger_of"].is_object(), "{merger}");
        let repaired = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["event"] == "plan_repaired")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(repaired.len(), 1, "{repaired:?}");
        let actions = repaired[0]["actions"].as_array().unwrap();
        assert!(
            !actions
                .iter()
                .any(|a| a.as_str().unwrap_or("").contains("gated on docs-only")),
            "no shard is a decision doc: {actions:?}"
        );
        goose_swarm::Dag::from_planner_json(&finalized).expect("loads");
    }

    /// Split v2 §4, at the declaration: synthesis is asked for every shared state's shape and its
    /// ONE writer (`writes` per shard, required by the schema); a shard's brief names it the only
    /// writer and its siblings read; two shards declaring the same state is
    /// `shard_shared_state_writers{source: declaration}` before `plan_patched` — said, the patch
    /// applies.
    #[test]
    fn two_declared_writers_of_one_shared_state_are_said_and_the_brief_names_the_writer() {
        assert!(split_system_prompt().contains("the ONE shard that WRITES it"));
        assert!(split_system_prompt().contains("provides: [], writes: []"));
        assert!(split_schema()["properties"]["shards"]["items"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "writes"));
        let mut split = viz_split();
        split.shards[0].writes = vec!["S.brush".into(), "instanceData".into()];
        split.shards[2].writes = vec!["S.brush".into()];
        let p = plan(&[
            ("web-console", &["web/app.js"]),
            ("web-viz", &["web/viz.js"]),
        ]);
        let (out, events) = apply_module_split(&p.to_string(), "web-viz", &split).unwrap();
        let said: Vec<&serde_json::Value> = events
            .iter()
            .filter(|e| e["event"] == "shard_shared_state_writers")
            .collect();
        assert_eq!(said.len(), 1, "{events:?}");
        assert_eq!(said[0]["module"], "web-viz");
        assert_eq!(said[0]["state"], "S.brush");
        assert_eq!(
            said[0]["shards"],
            serde_json::json!(["web-viz-render", "web-viz-labels-brush-api"])
        );
        assert_eq!(said[0]["source"], "declaration");
        assert!(said[0]["task_id"].is_null(), "no task exists at plan time");
        assert_eq!(events.last().unwrap()["event"], "plan_patched");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let brief_of = |id: &str| {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .and_then(|t| t["description"].as_str())
                .unwrap_or_else(|| panic!("{id}"))
                .to_string()
        };
        let render = brief_of("web-viz-render");
        assert!(
            render.contains("You are the ONLY WRITER of shared state: `S.brush`, `instanceData`"),
            "{render}"
        );
        assert!(
            render.contains("WRITES: <a shared state you WRITE, with the shape you write"),
            "{render}"
        );
        assert!(
            render.contains("End your final message with the same five fields"),
            "{render}"
        );
        let pick = brief_of("web-viz-pick-camera");
        assert!(
            pick.contains("`render`: WebGL programs, instanced geometry, demand rendering (provides initGL, render, buildScene) (the ONLY writer of S.brush, instanceData — you read it, never write it)"),
            "{pick}"
        );
        assert!(!pick.contains("You are the ONLY WRITER"), "{pick}");
        // A declaration with one writer per state says nothing.
        let (_, quiet) = apply_module_split(&p.to_string(), "web-viz", &viz_split()).unwrap();
        assert!(!quiet
            .iter()
            .any(|e| e["event"] == "shard_shared_state_writers"));
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

    /// Split v2 §6, size by the fleet. r6e declared EIGHT shards (seq 522) for a THREE-node pool
    /// (seq 1 `pool_resolved.worker_count: 3`) — five queued behind the first three. Sized to 3
    /// hosts the eight clusters (weight 1 each: no sections declared) group contiguously 2/3/3
    /// with ids, responsibilities and provides unioned; 8 hosts leave the declaration; no host
    /// count (the caller does not pass the pool yet) leaves it and SAYS so; uneven weights balance
    /// by claimed sections; one free host declines before any planner call.
    #[tokio::test]
    async fn shard_count_follows_the_free_hosts_and_an_absent_count_is_said() {
        let archived = [
            "data-scene",
            "rendering-core",
            "pick-buffer",
            "camera-inertia",
            "labels-culling",
            "linked-brush",
            "streaming-diffs",
            "vs7dbg-boot",
        ];
        let split: ModuleSplit = serde_json::from_value(serde_json::json!({
            "interface": {"exports": [], "shared_state": "S", "layout": []},
            "shards": archived.iter().map(|id| serde_json::json!({
                "id": id, "responsibility": format!("the {id} piece"), "sections": [], "provides": [format!("{id}Fn")], "writes": []
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        let (sized, s) = size_shards_to_hosts(split.clone(), Some(3));
        assert_eq!(s.declared, 8);
        assert_eq!(s.hosts, Some(3));
        assert_eq!(s.weights, vec![1; 8]);
        assert_eq!(
            s.groups,
            vec![
                vec!["data-scene", "rendering-core"],
                vec!["pick-buffer", "camera-inertia", "labels-culling"],
                vec!["linked-brush", "streaming-diffs", "vs7dbg-boot"],
            ]
        );
        assert_eq!(sized.shards.len(), 3);
        assert_eq!(sized.shards[0].id, "data-scene-rendering-core");
        assert_eq!(
            sized.shards[0].responsibility,
            "the data-scene piece; the rendering-core piece"
        );
        assert_eq!(
            sized.shards[1].provides,
            vec!["pick-bufferFn", "camera-inertiaFn", "labels-cullingFn"]
        );
        assert_eq!(sized.interface, split.interface);
        let ev = sized_event("viz3d-engine", &s);
        assert_eq!(ev["event"], "split_sized");
        assert_eq!(ev["module"], "viz3d-engine");
        assert_eq!(ev["declared"], 8);
        assert_eq!(ev["hosts"], 3);
        assert_eq!(ev["shards"], 3);
        assert_eq!(ev["groups"][1][2], "labels-culling");
        assert!(ev["source"].as_str().unwrap().starts_with("fleet"), "{ev}");

        // Unsized, the declaration stands — and each shard now carries itself as its one cluster
        // (VA-102: the brief names a first piece from the clusters, so the unsized path records them too).
        let stands = |same: &ModuleSplit, why: &str| {
            assert_eq!(same.interface, split.interface, "{why}");
            assert_eq!(same.shards.len(), split.shards.len(), "{why}");
            for (s, d) in same.shards.iter().zip(split.shards.iter()) {
                assert_eq!(
                    (
                        &s.id,
                        &s.responsibility,
                        &s.sections,
                        &s.provides,
                        &s.writes
                    ),
                    (
                        &d.id,
                        &d.responsibility,
                        &d.sections,
                        &d.provides,
                        &d.writes
                    ),
                    "{why}"
                );
                assert_eq!(
                    s.clusters,
                    vec![cluster_of(d)],
                    "{why}: the shard is its own one cluster"
                );
            }
        };
        let (same, eight) = size_shards_to_hosts(split.clone(), Some(8));
        stands(&same, "eight hosts for eight clusters");
        assert_eq!(eight.groups.len(), 8);
        assert!(sized_event("m", &eight)["source"]
            .as_str()
            .unwrap()
            .starts_with("declaration — no more clusters"));
        let (same, none) = size_shards_to_hosts(split.clone(), None);
        stands(&same, "no count: the declaration stands");
        assert!(none.hosts.is_none());
        let ev = sized_event("m", &none);
        assert!(ev["hosts"].is_null());
        assert!(
            ev["source"]
                .as_str()
                .unwrap()
                .contains("not passed by the caller"),
            "{ev}"
        );

        let mut uneven = split.clone();
        uneven.shards.truncate(5);
        uneven.shards[0].sections = (1..=4).map(|k| format!("### {k}. Section {k}")).collect();
        let (grouped, u) = size_shards_to_hosts(uneven, Some(2));
        assert_eq!(u.weights, vec![4, 1, 1, 1, 1]);
        assert_eq!(
            u.groups,
            vec![
                vec!["data-scene"],
                vec![
                    "rendering-core",
                    "pick-buffer",
                    "camera-inertia",
                    "labels-culling"
                ],
            ]
        );
        assert_eq!(
            grouped.shards[0].id, "data-scene",
            "a lone cluster keeps its id"
        );
        assert_eq!(grouped.shards[0].sections.len(), 4);
        assert_eq!(partition_contiguous(&[3, 3, 3], 3), vec![0..1, 1..2, 2..3]);
        assert_eq!(partition_contiguous(&[1, 1], 5), vec![0..1, 1..2]);
        assert!(partition_contiguous(&[], 3).is_empty());

        // ONE free host: the split is declined before synthesis is asked; flag and plan intact.
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
        // ONE free host: the split is asked anyway (r6g measured shards dispatching as nodes
        // freed), the scarcity is SAID, and the shards are the declaration's clusters — sizing by
        // hosts applies only at two or more.
        let reply = serde_json::json!({
            "interface": {"exports": [], "shared_state": "S", "layout": []},
            "shards": archived
                .iter()
                .map(|n| serde_json::json!({
                    "id": n, "responsibility": format!("{n} pieces"), "sections": [],
                    "provides": [], "writes": []
                }))
                .collect::<Vec<_>>(),
        })
        .to_string();
        let after = split_fat_tasks_sized(
            before.clone(),
            &r6c_like_opened(),
            spec,
            false,
            Some(1),
            move |_task, _density| {
                let r = reply.clone();
                async move { Ok(r) }
            },
            &sink_dyn,
        )
        .await;
        let events = sink.0.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|e| e["event"] == "plan_flag" && e["task"] == "web-viz"));
        let scarce = events
            .iter()
            .find(|e| e["event"] == "split_hosts_scarce")
            .expect("scarcity is said, never a decline");
        assert_eq!(scarce["task"], "web-viz");
        assert_eq!(scarce["free_hosts"], 1);
        assert!(!events.iter().any(|e| e["event"] == "split_declined"));
        // sizing is SAID at any host count (hosts: 1 here); partitioning onto hosts applies only at >= 2
        assert!(events.iter().any(|e| e["event"] == "split_sized"));
        assert_ne!(
            after, before,
            "the fat task was split despite one free host"
        );
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
            0, // free_hosts: a test passes 0 (SPLIT v2 §6; run_linear_plan measures the pool)
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
        // VA-113: every section-claiming task carries `weight == sections` (the unit `plan_flag`
        // reports), the merger keeps the module's 7, and each of its three shards carries
        // max(1, 7 / 3) = 2 — its real share.
        let weighted: serde_json::Value = serde_json::from_str(&plan_json).unwrap();
        let weight_of = |id: &str| -> Option<u64> {
            weighted["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .and_then(|t| t.get("weight"))
                .and_then(|w| w.as_u64())
        };
        let claims = section_claims(&r6c_like_opened(), spec);
        for id in ["ledgerd-core", "notifierd", "web-console"] {
            assert_eq!(
                weight_of(id),
                Some(claims[id].sections as u64),
                "{id}: weight is the claimed section count"
            );
            assert!(weight_of(id).unwrap() > 0, "{id} claims sections in sb7");
        }
        assert_eq!(
            weight_of("web-viz"),
            Some(7),
            "the merger keeps the module's weight"
        );
        for shard in [
            "web-viz-render",
            "web-viz-pick-camera",
            "web-viz-labels-brush-api",
        ] {
            assert_eq!(weight_of(shard), Some(2), "{shard}: max(1, 7 / 3)");
        }
        assert_eq!(
            weight_of("integrate-verify"),
            None,
            "the join owns nothing and claims nothing: no weight, the scheduler derives"
        );
        assert!(
            sink.0
                .lock()
                .unwrap()
                .iter()
                .any(|e| e["event"] == "plan_weighted"
                    && e["weights"]["web-viz"] == 7
                    && e["unit"] == "claimed spec sections"),
            "the weighing is said"
        );
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
        let after = split_fat_tasks_sized(
            before.clone(),
            &r6c_like_opened(),
            spec,
            false,
            None,
            |_task, _density| async move { Ok("I would rather not.".to_string()) },
            &sink_dyn,
        )
        .await;
        // VA-113: the ONE change a declined split leaves is the section weights, which are a fact
        // about the measured plan, not about the split — strip them and the plan is `before`.
        let mut stripped: serde_json::Value = serde_json::from_str(&after).unwrap();
        let mut weights = std::collections::BTreeMap::new();
        for t in stripped["subtasks"].as_array_mut().unwrap() {
            if let Some(w) = t.as_object_mut().unwrap().remove("weight") {
                weights.insert(t["id"].as_str().unwrap().to_string(), w.as_u64().unwrap());
            }
        }
        assert_eq!(
            stripped,
            serde_json::from_str::<serde_json::Value>(&before).unwrap()
        );
        assert_eq!(weights["web-viz"], 7, "{weights:?}");
        assert!(
            !weights.contains_key("integrate-verify"),
            "the join is never weighed: {weights:?}"
        );
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

// ---- S2/S3: the shard at DISPATCH and at COMPLETION -------------------------------------------

/// The YOU OWN header a shard sees instead of the file-author's "EXACTLY these ABSOLUTE paths".
pub(super) const SHARD_OWNED_HEADER: &str = "YOU OWN THIS FOLDER — write your pieces and your \
     README.md inside it and NOTHING outside it (it already exists; never `mkdir` elsewhere):";

/// The folder line(s) under the header — absolute, like every YOU OWN path.
pub(super) fn shard_owned_lines(cwd: &str, shard: &ShardOf) -> String {
    format!(
        "  {cwd}/{folder}/   (every piece file you write, in the module's language)\n  {cwd}/{folder}/README.md   (your structured handoff — required)",
        folder = shard.folder
    )
}

/// The shard's owner body — replaces the file author's WRITE FIRST script, whose every clause
/// ("write your owned file IN FULL", "run pytest") is wrong for a piece that cannot run alone.
pub(super) fn shard_owner_body(shard: &ShardOf) -> String {
    format!(
        "YOU ARE SHARD `{shard}` OF MODULE `{module}` ({responsibility}). WRITE PIECES, NOT THE \
         MODULE: put each function/class/section your split names in its own file inside your \
         folder, in the module's language, implementing EXACTLY the declared names and signatures \
         that fall in your split. NEVER write the module's final file — the merger assembles it \
         from every shard's pieces; a shard that writes it overwrites its siblings. Do not run the \
         app or the test suite (your pieces cannot run alone); CHECK each piece with a parse/lint \
         (`node --check`, `python3 -m py_compile`, the language's equivalent) and record the \
         command and its result. START by writing `README.md` in your folder — its first version, \
         with the five fields — {p}: / {a}: / {u}: / {c}: / {w}: — one item per line, {u}: listing \
         every piece not yet written — and KEEP IT CURRENT as each piece lands. Then ONE piece per \
         `write`, checked; never draft a piece's body in your reasoning to type it later. End your \
         final message with the same five fields (they are your handoff to the merger). A turn that \
         ends without the README FAILS and is retried.\n\n",
        shard = shard.shard,
        module = shard.module,
        responsibility = shard.responsibility.trim(),
        p = README_FIELDS[0],
        a = README_FIELDS[1],
        u = README_FIELDS[2],
        c = README_FIELDS[3],
        w = README_FIELDS[4],
    )
}

/// The deliverable gate's hint for a shard that finished without its README.
pub(super) fn readme_missing_hint(shard: &ShardOf) -> String {
    format!(
        "You finished WITHOUT `{folder}/README.md`. Write it NOW with the five fields — {p}: \
         (every symbol you defined, with its signature), {a}: (what you assume about siblings' \
         symbols and the shared state), {u}: (what you did not finish, or `none`), {c}: (the \
         parse/lint command you ran and its result), {w}: (the shared state you write, with its \
         shape, or `none`) — then finish. Do not rewrite your pieces.",
        folder = shard.folder,
        p = README_FIELDS[0],
        a = README_FIELDS[1],
        u = README_FIELDS[2],
        c = README_FIELDS[3],
        w = README_FIELDS[4],
    )
}

pub(super) fn merge_note_missing_event(
    shard: &ShardOf,
    task_id: &str,
    reason: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event": "merge_note_missing",
        "module": shard.module,
        "shard": shard.shard,
        "task_id": task_id,
        "folder": shard.folder,
        "reason": reason,
    })
}

/// A shard's structured handoff, parsed from its README (or its final message when the README
/// carries no field): what it provides, what it assumes about siblings, what is unfinished, how
/// it checked. Field lines are `FIELD: item`; items may continue as bullets or indented lines
/// under the field; `none` is the empty list said aloud.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ShardNote {
    pub(super) provides: Vec<String>,
    pub(super) assumes: Vec<String>,
    pub(super) unfinished: Vec<String>,
    pub(super) checked_with: Vec<String>,
    /// The shared state this shard WRITES (`WRITES:` lines, split v2 §4) — empty for a pure
    /// reader, and for a README that carries no WRITES line at all; the dossier only ever compares
    /// non-empty lists, so an absent line can never manufacture a conflict.
    pub(super) writes: Vec<String>,
}

impl ShardNote {
    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "provides": self.provides,
            "assumes": self.assumes,
            "unfinished": self.unfinished,
            "checked_with": self.checked_with,
            "writes": self.writes,
        })
    }
}

/// Parse the four fields out of a README or a final message (`parse_fields` over
/// `README_FIELDS`). None when no field line exists — the caller names that absence
/// (`merge_note_missing`), never a default note.
pub(super) fn parse_shard_note(text: &str) -> Option<ShardNote> {
    let lists = parse_fields(text, &README_FIELDS)?;
    // `parse_fields` returns exactly one list per field — a different length is a programming
    // error, and `?` says so rather than defaulting a field to empty.
    let [provides, assumes, unfinished, checked_with, writes]: [Vec<String>; 5] =
        lists.try_into().ok()?;
    Some(ShardNote {
        provides,
        assumes,
        unfinished,
        checked_with,
        writes,
    })
}

/// The state a WRITES item names: the text before its shape (` — `, `: `, ` (`), trimmed —
/// `S.brush — Set<id>` and `S.brush: the brushed ids` are one state; `S.brush` and `S.yaw` two;
/// compared case-insensitively, reported as first written.
fn state_key(item: &str) -> Option<(String, String)> {
    let mut name = item.trim();
    for sep in [" — ", " – ", ": ", " (", " - "] {
        if let Some((head, _)) = name.split_once(sep) {
            name = head;
        }
    }
    let name = name.trim().trim_end_matches(':').trim();
    (!name.is_empty()).then(|| (name.to_string(), name.to_lowercase()))
}

/// ONE WRITER PER SHARED STATE (split v2 §4): every state more than one shard WRITES, from
/// `(shard id, its WRITES items)` pairs — the declaration's `writes` at plan time or the READMEs'
/// lines at the merger's dispatch. Two writers is the `cooperate` edge the research names (SpecDB:
/// modules that must be co-designed are one unit); said, never refused.
pub(super) fn shared_state_writers(
    writers: &[(String, Vec<String>)],
) -> Vec<(String, Vec<String>)> {
    let mut by_state: Vec<(String, String, Vec<String>)> = Vec::new();
    for (shard, writes) in writers {
        for w in writes {
            let Some((display, key)) = state_key(w) else {
                continue;
            };
            match by_state.iter_mut().find(|(_, k, _)| *k == key) {
                Some((_, _, shards)) => {
                    if !shards.contains(shard) {
                        shards.push(shard.clone());
                    }
                }
                None => by_state.push((display, key, vec![shard.clone()])),
            }
        }
    }
    by_state
        .into_iter()
        .filter(|(_, _, s)| s.len() > 1)
        .map(|(display, _, s)| (display, s))
        .collect()
}

/// `shard_shared_state_writers{module, task_id, state, shards, source}` per conflicting state;
/// `source` is `declaration` (plan time, no task) or `readme` (the merger's dispatch).
pub(super) fn shared_state_writer_events(
    module: &str,
    conflicts: &[(String, Vec<String>)],
    source: &str,
    task_id: Option<&str>,
) -> Vec<serde_json::Value> {
    conflicts
        .iter()
        .map(|(state, shards)| {
            serde_json::json!({
                "event": "shard_shared_state_writers",
                "module": module,
                "task_id": task_id,
                "state": state,
                "shards": shards,
                "source": source,
            })
        })
        .collect()
}

/// S10(3): the module's FINAL files as they stand when a shard lane is dispatched — the bytes, or
/// None when the file is not readable (nobody has written it yet is the expected case; a
/// permission fault surfaces loudly at the merger's own read, never here). Paired with
/// `final_files_written` at the lane's completion.
pub(super) fn snapshot_final_files(
    root: &std::path::Path,
    shard: Option<&ShardOf>,
) -> Vec<(String, Option<Vec<u8>>)> {
    match shard {
        Some(sh) => sh
            .module_files
            .iter()
            .map(|f| (f.clone(), std::fs::read(root.join(f)).ok()))
            .collect(),
        None => Vec::new(),
    }
}

/// The final files a shard lane changed: readable now and absent at dispatch or different from
/// then. A shard's write surface is its folder; a write here is the merger's file authored
/// outside the piece protocol — reported (`shard_wrote_final_file`), never refused (MILD).
pub(super) fn final_files_written(
    root: &std::path::Path,
    before: &[(String, Option<Vec<u8>>)],
) -> Vec<String> {
    before
        .iter()
        .filter(|(f, was)| {
            std::fs::read(root.join(f))
                .ok()
                .is_some_and(|now| was.as_ref() != Some(&now))
        })
        .map(|(f, _)| f.clone())
        .collect()
}

impl GooseAgentDispatcher {
    /// S3, at a shard's completion: read `<folder>/README.md` (the file is the handoff; the final
    /// message is the fallback when the file carries no field), emit `shard_note{…}` or
    /// `merge_note_missing{…}`, and return the extra keys for the shard's ledger row —
    /// `shard_note` and its `handoffs` through the existing channel (`parse_handoffs`), plus
    /// `wrote_final` (S10(3)): every merger file this lane wrote directly, each also a
    /// `shard_wrote_final_file{module, shard, task_id, path}` event; the merger's dossier reads
    /// the row and lists the file as one more piece to reconcile.
    pub(super) fn record_shard_note(
        &self,
        shard: &ShardOf,
        req: &goose_swarm::DispatchRequest,
        root: &std::path::Path,
        final_text: &str,
        final_before: &[(String, Option<Vec<u8>>)],
    ) -> serde_json::Value {
        let wrote_final = final_files_written(root, final_before);
        for path in &wrote_final {
            self.events.write_value(serde_json::json!({
                "event": "shard_wrote_final_file",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": req.task_id,
                "path": path,
            }));
        }
        let readme_path = root.join(&shard.folder).join("README.md");
        let readme = std::fs::read_to_string(&readme_path).ok();
        let (note, source) = match readme.as_deref().and_then(parse_shard_note) {
            Some(n) => (Some(n), "README.md"),
            None => match parse_shard_note(final_text) {
                Some(n) => (Some(n), "final_message"),
                None => (None, "absent"),
            },
        };
        let pieces: Vec<String> = std::fs::read_dir(root.join(&shard.folder))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| n != "README.md")
            .collect();
        match &note {
            Some(n) => {
                let mut ev = n.to_json();
                if let Some(o) = ev.as_object_mut() {
                    o.insert("event".into(), "shard_note".into());
                    o.insert("module".into(), shard.module.clone().into());
                    o.insert("shard".into(), shard.shard.clone().into());
                    o.insert("task_id".into(), req.task_id.clone().into());
                    o.insert("source".into(), source.into());
                    o.insert("pieces".into(), serde_json::json!(pieces));
                    o.insert(
                        "readme_present".into(),
                        readme.as_ref().is_some_and(|r| !r.trim().is_empty()).into(),
                    );
                }
                self.events.write_value(ev);
            }
            None => {
                let reason = if readme.is_none() {
                    "no README.md in the shard folder and no field in the final message"
                } else {
                    "README.md carries none of PROVIDES/ASSUMES/UNFINISHED/CHECKED_WITH/WRITES"
                };
                self.events
                    .write_value(merge_note_missing_event(shard, &req.task_id, reason));
            }
        }
        let handoffs: Vec<serde_json::Value> =
            super::attribution::parse_handoffs(final_text, &req.all_files, &req.owned_files)
                .into_iter()
                .map(|h| serde_json::json!({"path": h.path, "symbol": h.symbol, "note": h.note}))
                .collect();
        serde_json::json!({
            "shard_of": {"module": shard.module, "shard": shard.shard, "folder": shard.folder},
            "shard_note": note.as_ref().map(|n| n.to_json()),
            "shard_note_source": source,
            "pieces": pieces,
            "handoffs": handoffs,
            "wrote_final": wrote_final,
        })
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    fn shard() -> ShardOf {
        ShardOf {
            module: "web-viz".into(),
            shard: "render".into(),
            folder: ".swarm/shards/web-viz/render".into(),
            responsibility: "programs and geometry".into(),
            interface: ModuleInterface::default(),
            module_files: vec!["web/viz.js".into()],
        }
    }

    /// The README a shard leaves, in the brief's own shape — fields become lists, `none` is empty,
    /// bullets and continuation lines belong to the field above them.
    #[test]
    fn a_structured_readme_parses_into_the_four_fields() {
        let readme = "# render shard\n\nPROVIDES: initGL() -> void\n- render() -> void\n- `buildScene(data) -> void`\nASSUMES: S.brush is a Set<id>\n  S.dirty is a bool the render loop clears\nUNFINISHED: none\nCHECKED_WITH: node --check render.js (ok)\n\nSome trailing prose that is not a field.\n";
        let n = parse_shard_note(readme).expect("fields present");
        assert_eq!(
            n.provides,
            vec![
                "initGL() -> void",
                "render() -> void",
                "buildScene(data) -> void"
            ]
        );
        assert_eq!(
            n.assumes,
            vec![
                "S.brush is a Set<id>",
                "S.dirty is a bool the render loop clears"
            ]
        );
        assert!(
            n.unfinished.is_empty(),
            "`none` is the empty list said aloud"
        );
        assert_eq!(n.checked_with, vec!["node --check render.js (ok)"]);
        // A final message in markdown dress parses the same way.
        let msg = "Done.\n\n**PROVIDES:** `drawBrush(ids)`\n## ASSUMES\n- scene.points is a Float32Array\n**UNFINISHED:** label culling for ties\n**CHECKED_WITH:** `node --check`\n";
        let n = parse_shard_note(msg).unwrap();
        assert_eq!(n.provides, vec!["drawBrush(ids)"]);
        assert_eq!(n.assumes, vec!["scene.points is a Float32Array"]);
        assert_eq!(n.unfinished, vec!["label culling for ties"]);
        assert!(parse_shard_note("I wrote some files and finished.").is_none());
    }

    #[test]
    fn the_shard_prompt_names_the_folder_and_forbids_the_final_file() {
        let sh = shard();
        let lines = shard_owned_lines("/tmp/run", &sh);
        assert!(lines.contains("/tmp/run/.swarm/shards/web-viz/render/"));
        assert!(lines.contains("README.md"));
        let body = shard_owner_body(&sh);
        assert!(body.contains("NEVER write the module's final file"));
        assert!(body.contains("PROVIDES: / ASSUMES: / UNFINISHED: / CHECKED_WITH:"));
        assert!(body.contains("START by writing `README.md`"), "{body}");
        assert!(
            !body.contains("FINISH by writing"),
            "VA-102 (r6h): the README ordered LAST deferred every write behind the whole design"
        );
        assert!(
            !body.contains("pytest ONCE"),
            "the file author's script is not the shard's"
        );
        let hint = readme_missing_hint(&sh);
        assert!(hint.contains(".swarm/shards/web-viz/render/README.md"));
        let ev = merge_note_missing_event(&sh, "web-viz-render", "why");
        assert_eq!(ev["event"], "merge_note_missing");
        assert_eq!(ev["module"], "web-viz");
        assert_eq!(ev["shard"], "render");
    }

    /// Split v2 §4: the WRITES line parses (bullets continue it; a README written without the
    /// field reads as writing nothing — never a manufactured conflict); the state is the text
    /// before its shape, so `S.brush — Set<id>` and `S.brush: the brushed ids` are ONE state
    /// written by two shards, `instanceData` and `labelSlots` single-writer; the event carries the
    /// state as first written, the shards, and the source.
    #[test]
    fn writes_lines_parse_and_two_readme_writers_of_one_state_are_said() {
        let n = parse_shard_note(
            "PROVIDES: a()\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: x\nWRITES: S.brush — Set<id> of brushed ids\n- instanceData: Float32Array stride 8\n",
        )
        .unwrap();
        assert_eq!(
            n.writes,
            vec![
                "S.brush — Set<id> of brushed ids",
                "instanceData: Float32Array stride 8"
            ]
        );
        assert_eq!(
            n.to_json()["writes"][1],
            "instanceData: Float32Array stride 8"
        );
        let before_the_field =
            parse_shard_note("PROVIDES: a()\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: x\n")
                .unwrap();
        assert!(before_the_field.writes.is_empty());
        assert!(parse_shard_note("PROVIDES: a()\nWRITES: none\n")
            .unwrap()
            .writes
            .is_empty());

        let conflicts = shared_state_writers(&[
            (
                "web-viz-render".to_string(),
                vec![
                    "S.brush — Set<id> of brushed ids".to_string(),
                    "instanceData: Float32Array stride 8".to_string(),
                ],
            ),
            (
                "web-viz-brush".to_string(),
                vec!["s.brush: the brushed ids".to_string()],
            ),
            (
                "web-viz-labels".to_string(),
                vec!["labelSlots (Float32Array)".to_string()],
            ),
        ]);
        assert_eq!(
            conflicts,
            vec![(
                "S.brush".to_string(),
                vec!["web-viz-render".to_string(), "web-viz-brush".to_string()]
            )]
        );
        assert!(shared_state_writers(&[
            ("a".to_string(), vec!["S.brush".to_string()]),
            ("b".to_string(), vec!["S.yaw".to_string()]),
        ])
        .is_empty());
        let events = shared_state_writer_events("web-viz", &conflicts, "readme", Some("web-viz"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "shard_shared_state_writers");
        assert_eq!(events[0]["module"], "web-viz");
        assert_eq!(events[0]["task_id"], "web-viz");
        assert_eq!(events[0]["state"], "S.brush");
        assert_eq!(
            events[0]["shards"],
            serde_json::json!(["web-viz-render", "web-viz-brush"])
        );
        assert_eq!(events[0]["source"], "readme");
        let sh = shard();
        assert!(shard_owner_body(&sh).contains("/ CHECKED_WITH: / WRITES:"));
        assert!(readme_missing_hint(&sh).contains("WRITES: (the shared state you write"));
    }
}

// ---- S4: THE MERGER — code builds the dossier, a judicious model merges, code checks after -----

/// The merge README the merger leaves beside the shard folders: which duplicate it kept and why,
/// what it filled itself, what it sent out. `.swarm/shards/<module>/MERGE.md`.
pub(super) const MERGE_README: &str = "MERGE.md";
pub(super) const MERGE_FIELDS: [&str; 4] = ["KEPT", "DROPPED", "FILLED", "SENT_OUT"];
/// The merger's handoff line for a gap it judges too big to fill in the merge: one per line in
/// its final message. The engine dispatches each to a free node as a new shard and calls the
/// merger back when they land (the merge-gap door, scheduler.rs).
pub(super) const MERGE_GAP_PREFIX: &str = "MERGE_GAP:";

/// What a definition IS — the one classification the dossier, the assembly, `check_merge` and
/// `shard_verify` share (VA-097). r6g's labels-brush shard defined `const brushSet = new Set()`,
/// `let uBrushActive = 0`, `const dimFlags = new Uint8Array(65536)`, `const LABEL_W = 110` and
/// `window.vs7 = {…}`; the function-only rule read 6 of its 13 PROVIDES as unbacked and told the
/// merger to write them itself beside "retyping a definition is FORBIDDEN" — a second
/// `const brushSet` is the SyntaxError r6c shipped as "five names defined twice".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SymbolKind {
    Function,
    Class,
    /// Module-level `const` with a scalar literal, or an UPPER_SNAKE name.
    Constant,
    /// Module-level state: `let`/`var`, a `const` holding a container or an instance, a
    /// top-level dotted assignment (`window.vs7 = {…}`), a Python module-level `NAME = …`.
    State,
    /// A shorthand-property mention; never a definition (`shorthand` is true).
    Reference,
}

impl SymbolKind {
    /// The brief's suffix after a name: a function shows its parameters instead.
    pub(super) fn suffix(self) -> &'static str {
        match self {
            SymbolKind::Function | SymbolKind::Reference => "",
            SymbolKind::Class => " (class)",
            SymbolKind::Constant => " (constant)",
            SymbolKind::State => " (state)",
        }
    }
}

/// One definition found in a piece or in the final file — or a shorthand-property MENTION of one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Symbol {
    pub(super) name: String,
    pub(super) params: String,
    pub(super) line: usize,
    pub(super) kind: SymbolKind,
    /// An object-literal shorthand property (`{ pick,\n drawBrush,\n }`) — a REFERENCE to a name
    /// defined elsewhere, never a definition: with nothing defining it the file throws
    /// `ReferenceError` at load (S14-1). `defined()`/`defines()` skip shorthand-only names.
    pub(super) shorthand: bool,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn ident_at(s: &str) -> Option<&str> {
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if i == 0 && !is_ident_start(c) {
            return None;
        }
        if !is_ident_char(c) && c != '.' {
            break;
        }
        end = i + c.len_utf8();
    }
    (end > 0).then(|| s.split_at(end).0)
}

fn params_after(s: &str) -> Option<String> {
    let open = s.find('(')?;
    let (_, from_open) = s.split_at(open);
    let inner = from_open.split_at(1).1;
    let mut depth = 0i32;
    for (i, c) in from_open.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(inner.split_at(i.saturating_sub(1)).0.trim().to_string());
                }
            }
            _ => {}
        }
    }
    Some(inner.trim().to_string())
}

/// The text after the matching `)` of the first `(` in `s`.
fn after_params(s: &str) -> Option<&str> {
    let open = s.find('(')?;
    let (_, from_open) = s.split_at(open);
    let mut depth = 0i32;
    for (i, c) in from_open.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(from_open.split_at(i + 1).1);
                }
            }
            _ => {}
        }
    }
    None
}

const JS_KEYWORDS: [&str; 16] = [
    "if", "for", "while", "switch", "catch", "return", "function", "else", "do", "try", "new",
    "typeof", "await", "yield", "throw", "case",
];

/// The language a PIECE or a FINAL FILE is read as — ITS extension decides, never the run's target
/// language. r6e's split module was `web/viz.js` in a Python-target run: a `.js` piece read by the
/// Python extractor (`def`/`class` heads only) yields no definition at all — every export
/// "missing", every PROVIDES "unbacked", nothing to assemble. The run's language stands only for an
/// extension that names none.
pub(super) fn lang_for_path(path: &str, run_lang: super::TargetLang) -> super::TargetLang {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("py") => super::TargetLang::Python,
        Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx") => super::TargetLang::TypeScript,
        Some("rs") => super::TargetLang::Rust,
        Some("go") => super::TargetLang::Go,
        _ => run_lang,
    }
}

/// THE DEFINITION RULE — line-based and deterministic, read by the merger's dossier
/// (`unbacked_provides`, `declared_missing`, duplicates), the assembly (`segments`), the
/// after-check (`check_merge`) and the shard scan (`shard_verify`), so "present in the final
/// file" means what "defined in a piece" meant and a README's PROVIDES is judged by the same
/// reading everywhere (VA-097: one rule, one place). JavaScript: `function name(`, `class Name`,
/// `const/let/var name = function|(…) =>|async`, `a.b.c = function|(…) =>`, object-literal
/// methods `name(…) {` and `name: function(`/`name: (…) =>`; and at the piece's TOP LEVEL only,
/// `const/let/var name [= …]` holding anything else (state or a constant) and `a.b = …` (an
/// installed name — `window.vs7 = { toggleBrush, onBrushChange }`). A `const` inside a body is a
/// local, not what the module provides, so indentation decides: a local read as a definition
/// would be a "duplicate" across shards and a "dropped" name after every merge. Python:
/// `def`/`async def`/`class`, and top-level `NAME = …`. Other languages: `fn`/`func` heads.
/// Is `rhs` (the text after `=`) a function VALUE — `function …`, `async …`, `(params) => …` or
/// `x => …` — as opposed to a value that merely CONTAINS an arrow inside a call? VA-100 (r6g,
/// `.swarm/shards/viz-engine/labels-brush/updateLabels.js:47`): the indented
/// `const candIds = new Set(cands.map((n) => rec.ids[n]))` read as a Function `candIds` under
/// "contains `=>`" — a false duplicate the moment two shards share an inner arrow name. ONE rule
/// for the `const`/`let`/`var` arm and the dotted-assignment arm: the arrow must be the rhs's
/// OWN — right after its leading parenthesised parameter list, or right after one bare
/// identifier. Indentation is deliberately NOT the law for JS: an IIFE-wrapped module (r6c's
/// viz.js, `the_extractor_finds_every_definition_shape_once`) indents every real definition.
fn rhs_is_function(rhs: &str) -> bool {
    if rhs.starts_with("function") || rhs.starts_with("async") {
        return true;
    }
    if rhs.starts_with('(') {
        return after_params(rhs).is_some_and(|rest| rest.trim_start().starts_with("=>"));
    }
    ident_at(rhs).is_some_and(|name| rhs.split_at(name.len()).1.trim_start().starts_with("=>"))
}

pub(super) fn extract_symbols(source: &str, lang: super::TargetLang) -> Vec<Symbol> {
    let mut out: Vec<Symbol> = Vec::new();
    // JS object-literal depth, so `{ pick, brush }` shorthand PROPERTIES are recorded — flagged
    // `shorthand`, as MENTIONS of the names the object exports (S14-1; S12-A counted them as
    // definitions, and a multi-line export object naming an undefined `drawBrush` then read as
    // conforming). A text scan would also count `// TODO drawBrush`; this arm records only
    // property positions.
    let mut object_depth: i32 = 0;
    let mut push = |name: &str, params: Option<String>, line: usize, kind: SymbolKind| {
        let shorthand = kind == SymbolKind::Reference;
        let name = name.trim_end_matches('.').to_string();
        if name.is_empty() || JS_KEYWORDS.contains(&name.as_str()) {
            return;
        }
        match out.iter_mut().find(|s| s.name == name) {
            // A definition outranks a shorthand mention of the same name, and one WITH parameters
            // supplies the signature (the property is the export, the function is its signature).
            Some(existing) => {
                if !shorthand {
                    if existing.shorthand {
                        existing.kind = kind;
                    }
                    existing.shorthand = false;
                    if existing.params.is_empty() {
                        if let Some(p) = params.filter(|p| !p.is_empty()) {
                            existing.params = p;
                        }
                    }
                }
            }
            None => out.push(Symbol {
                name,
                // A class, a constant, state or a property has no parameter list — empty MEANS
                // empty (fallback gate).
                params: params.unwrap_or_default(),
                line,
                kind,
                shorthand,
            }),
        }
    };
    for (i, raw) in source.lines().enumerate() {
        let t = raw.trim_start();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*") {
            continue;
        }
        let line = i + 1;
        let top_level = raw.len() == t.len();
        match lang {
            super::TargetLang::Python => {
                for head in ["async def ", "def ", "class "] {
                    if let Some(rest) = t.strip_prefix(head) {
                        if let Some(name) = ident_at(rest) {
                            let (params, kind) = if head == "class " {
                                (None, SymbolKind::Class)
                            } else {
                                (params_after(rest), SymbolKind::Function)
                            };
                            push(name, params, line, kind);
                        }
                    }
                }
                // Module-level `NAME = …` / `NAME: T = …` — state or a constant the module
                // provides; an indented assignment is a local or an attribute, not a definition.
                if top_level {
                    if let Some(name) = ident_at(t).filter(|n| !n.contains('.')) {
                        let after = t.split_at(name.len()).1.trim_start();
                        let after = match after.strip_prefix(':') {
                            Some(annotated) => match annotated.find('=') {
                                Some(eq) => annotated.split_at(eq).1,
                                None => "",
                            },
                            None => after,
                        };
                        if let Some(rhs) = after.strip_prefix('=') {
                            if !rhs.starts_with('=') {
                                push(name, None, line, state_kind(name, "", rhs.trim_start()));
                            }
                        }
                    }
                }
            }
            super::TargetLang::Rust | super::TargetLang::Go => {
                let core = t
                    .trim_start_matches("pub(crate) ")
                    .trim_start_matches("pub ")
                    .trim_start_matches("async ");
                for head in ["fn ", "func "] {
                    if let Some(rest) = core.strip_prefix(head) {
                        if let Some(name) = ident_at(rest) {
                            push(name, params_after(rest), line, SymbolKind::Function);
                        }
                    }
                }
            }
            _ => {
                // object-literal shorthand properties: `pick,` / `pick, brush` / `{ pick, brush }`
                // on a line inside an object literal opened by `= {` / `return {` / `: {`.
                let opens_object = t.ends_with("= {")
                    || t.ends_with("return {")
                    || t.ends_with(": {")
                    || t.ends_with("({")
                    || t == "{";
                if object_depth > 0 {
                    let inner = t
                        .trim_start_matches('{')
                        .trim_end_matches(['}', ';', ','])
                        .trim();
                    let props: Vec<&str> = inner.split(',').map(str::trim).collect();
                    if !inner.is_empty()
                        && props.iter().all(|p| {
                            !p.is_empty()
                                && p.chars().next().is_some_and(is_ident_start)
                                && p.chars().all(is_ident_char)
                                && !JS_KEYWORDS.contains(p)
                        })
                    {
                        for prop in props {
                            push(prop, None, line, SymbolKind::Reference);
                        }
                    }
                }
                // Methods written INLINE on the object's own line — `X = { pick(sx, sy) { … } }`.
                if let Some((_, after_brace)) = t.split_once('{') {
                    if t.contains("= {") || t.contains("return {") || object_depth > 0 {
                        let mut rest = after_brace;
                        while let Some(pos) = rest.find(|c: char| is_ident_start(c)) {
                            let cand = rest.split_at(pos).1;
                            let Some(name) = ident_at(cand) else { break };
                            let after = cand.split_at(name.len()).1.trim_start();
                            if after.starts_with('(')
                                && !name.contains('.')
                                && !JS_KEYWORDS.contains(&name)
                                && after_params(after)
                                    .is_some_and(|r| r.trim_start().starts_with('{'))
                            {
                                push(name, params_after(after), line, SymbolKind::Function);
                            }
                            rest = cand.split_at(name.len()).1;
                        }
                    }
                }
                if opens_object {
                    object_depth += 1;
                } else if t.starts_with('}') && object_depth > 0 {
                    object_depth -= 1;
                }
                // function name(
                for head in [
                    "async function ",
                    "function ",
                    "export function ",
                    "export async function ",
                ] {
                    if let Some(rest) = t.strip_prefix(head) {
                        if let Some(name) = ident_at(rest) {
                            push(name, params_after(rest), line, SymbolKind::Function);
                        }
                    }
                }
                for head in ["class ", "export class ", "export default class "] {
                    if let Some(rest) = t.strip_prefix(head) {
                        if let Some(name) = ident_at(rest) {
                            push(name, None, line, SymbolKind::Class);
                        }
                    }
                }
                // const name = function | (…) => | async (…) => | x =>   → a function;
                // top-level const/let/var holding anything else (or nothing yet) → state/constant.
                for head in [
                    "const ",
                    "let ",
                    "var ",
                    "export const ",
                    "export let ",
                    "export var ",
                ] {
                    if let Some(rest) = t.strip_prefix(head) {
                        if let Some(name) = ident_at(rest) {
                            let after = rest.split_at(name.len()).1.trim_start();
                            let rhs = after
                                .strip_prefix('=')
                                .filter(|r| !r.starts_with('='))
                                .map(str::trim_start);
                            match rhs {
                                Some(rhs) if rhs_is_function(rhs) => {
                                    push(name, params_after(rhs), line, SymbolKind::Function);
                                }
                                _ if top_level && !name.contains('.') => {
                                    push(
                                        name,
                                        None,
                                        line,
                                        state_kind(name, head, rhs.unwrap_or("")),
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // a.b.c = function | (…) =>   (assignment of a function to a dotted path); a
                // top-level `a.b = <anything else>` installs a name (`window.vs7 = {…}`) → state.
                if let Some(name) = ident_at(t) {
                    if name.contains('.') {
                        let after = t.split_at(name.len()).1.trim_start();
                        if let Some(rhs) = after.strip_prefix('=') {
                            let rhs = rhs.trim_start();
                            if !rhs.starts_with('=') {
                                if rhs_is_function(rhs) {
                                    push(name, params_after(rhs), line, SymbolKind::Function);
                                } else if top_level {
                                    push(name, None, line, SymbolKind::State);
                                }
                            }
                        }
                    }
                    // object-literal method shorthand `name(...) {` and `name: function(` / `name: (…) =>`
                    if !name.contains('.') {
                        let after = t.split_at(name.len()).1.trim_start();
                        if after.starts_with('(')
                            && after_params(after)
                                .is_some_and(|rest| rest.trim_start().starts_with('{'))
                        {
                            push(name, params_after(after), line, SymbolKind::Function);
                        } else if let Some(rhs) = after.strip_prefix(':') {
                            let rhs = rhs.trim_start();
                            if rhs.starts_with("function")
                                || rhs.starts_with("async")
                                || (rhs.starts_with('(') && rhs.contains("=>"))
                            {
                                push(name, params_after(rhs), line, SymbolKind::Function);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Constant or state, for a non-function module-level binding — a LABEL for the brief, never a
/// gate. A `const` with a scalar literal (number, string, boolean, null), or an UPPER_SNAKE name
/// bound to anything but a container, is a constant; a container or an instance (`{…}`, `[…]`,
/// `new Set()` — r6e's `const S = {yaw, pitch…}` and `VS` are the module's STATE whatever their
/// case), `let`/`var`, a Python module-level object — is state.
fn state_kind(name: &str, head: &str, rhs: &str) -> SymbolKind {
    let rhs_value = rhs.trim_end_matches([';', ',']).trim();
    let container = rhs_value.starts_with(['{', '['])
        || rhs_value.starts_with("new ")
        || rhs_value.starts_with("new(");
    let upper_snake = !container
        && !rhs_value.is_empty()
        && name.chars().any(|c| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    let scalar = rhs_value.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.')
        || rhs_value.starts_with(['"', '\'', '`'])
        || [
            "true",
            "false",
            "null",
            "undefined",
            "True",
            "False",
            "None",
        ]
        .iter()
        .any(|lit| rhs_value.split([' ', ';', ',']).next() == Some(lit));
    if upper_snake || (head.trim_start_matches("export ") == "const " && scalar) {
        SymbolKind::Constant
    } else {
        SymbolKind::State
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Same symbol? Exact, or the declared dotted name's last segment equals the definition (a
/// declared `window.vs7dbg.pick` is met by a method `pick(sx, sy)` on the object the pieces build).
/// MILD by design: the last-segment rule also lets `window.vs7dbg.pick` match a `foo.pick` — a
/// false "present"/"disagreement" the merger reads and dismisses in one line, preferred over a
/// false "missing" that would send a real export out as a gap.
fn same_symbol(declared: &str, found: &str) -> bool {
    declared == found
        || last_segment(declared) == found
        || declared == last_segment(found)
        || last_segment(declared) == last_segment(found)
}

/// Parameter NAMES from a parameter list. Split at depth 0 only: `batch: {batch: number,
/// records: object[]}` is ONE parameter (VA-108 — r6h's `applyBatch` declared exactly that, and
/// the comma inside its object type read as a second parameter, so the dossier reported a
/// `signature_disagreement` and completion emitted `merge_signature_mismatch{applyBatch, found
/// "batch"}` against a definition that matched its declaration).
fn normalize_params(p: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut prev = ' ';
    for c in p.chars() {
        match c {
            '(' | '[' | '{' | '<' => {
                depth += 1;
                cur.push(c);
            }
            // The `>` of an arrow type (`cb: (ids) => void`) closes nothing: counted as a closer
            // it left the following `Map<string, number>` at depth 0 and split its type's comma.
            '>' if prev == '=' => cur.push(c),
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth <= 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
        prev = c;
    }
    parts.push(cur);
    parts
        .into_iter()
        .map(|x| {
            x.trim()
                .split([':', '='])
                .next()
                .unwrap_or("")
                .trim()
                .trim_start_matches("...")
                .to_lowercase()
        })
        .filter(|x| !x.is_empty())
        .collect()
}

/// One shard as the dossier saw it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ShardDossier {
    pub(super) id: String,
    pub(super) folder: String,
    pub(super) readme_present: bool,
    pub(super) note: Option<ShardNote>,
    /// (relative piece path, parse verdict — None = parsed / unchecked, Some(err) = error, symbols)
    pub(super) pieces: Vec<(String, Option<String>, Vec<Symbol>)>,
    /// Merger files this shard wrote directly (its ledger row's `wrote_final`, S10(3)).
    pub(super) wrote_final: Vec<String>,
    /// README PROVIDES items (verbatim) no piece in the folder DEFINES — promises, not deliveries
    /// (DESIGN-SPLIT-V2 §3). Each is `shard_provides_unbacked` at the merger's dispatch and a GAP
    /// in its brief, never a delivered part. An item naming no symbol at all is unbacked too.
    pub(super) provides_unbacked: Vec<String>,
}

/// A shard task's ledger row as `write_task_ledger` left it — None when the row was never written
/// (the dossier then says "writer not recorded", never guesses).
fn shard_ledger_row(root: &std::path::Path, task_id: &str) -> Option<serde_json::Value> {
    let path = root
        .join(super::LEDGER_DIR)
        .join(format!("{}.json", super::activity_digest_key(task_id)));
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

impl ShardDossier {
    fn defines(&self) -> impl Iterator<Item = &Symbol> {
        self.pieces
            .iter()
            .flat_map(|(_, _, s)| s.iter())
            .filter(|s| !s.shorthand)
    }
    /// A DEFINITION in a piece — `extract_symbols`' rule, the one `check_merge` uses on the final
    /// file. A README's PROVIDES claim is not this (v1's `provides_or_defines` let a claim stand in
    /// for the code; r6e's eight README-only shards would have "provided" every export).
    fn defines_name(&self, name: &str) -> bool {
        self.defines().any(|s| same_symbol(name, &s.name))
    }

    /// The README's PROVIDES items no piece backs — by identifier (`ident_at`, the same reading
    /// the assumptions use); an item with no identifier names nothing a piece could define.
    fn unbacked_provides(&self) -> Vec<String> {
        match &self.note {
            Some(n) => n
                .provides
                .iter()
                .filter(|p| !ident_at(p).is_some_and(|i| self.defines_name(i)))
                .cloned()
                .collect(),
            // No README → no PROVIDES claims → nothing to back; the README's absence itself rode
            // `merge_note_missing` at the shard's completion and `readmes_missing` here, so empty
            // MEANS empty (fallback gate).
            None => Vec::new(),
        }
    }
}

/// THE MERGE DOSSIER — built by CODE, no model: parse result per piece, the cross-shard symbol
/// table (duplicates, signatures that disagree with the declaration, declared names nobody
/// defines), every README's assumptions no sibling provides, every unfinished item, the declared
/// interface and layout, and the previous merge README when this is a second pass (a merger
/// called back after its gaps landed).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MergeDossier {
    pub(super) module: String,
    pub(super) files: Vec<String>,
    pub(super) interface: ModuleInterface,
    pub(super) shards: Vec<ShardDossier>,
    pub(super) duplicates: Vec<(String, Vec<String>)>,
    pub(super) declared_missing: Vec<String>,
    /// (declared name, declared signature, found params, shard)
    pub(super) signature_disagreements: Vec<(String, String, String, String)>,
    /// (shard, ASSUMES clause) — clauses naming at least one identifier no shard defines
    /// (`assumes::resolve`, VA-108); the names themselves are `assumptions_unbacked`.
    pub(super) assumptions_unmet: Vec<(String, String)>,
    /// One per (shard, name) an ASSUMES clause names that no shard's piece defines and no declared
    /// shared-state root covers, with the nearest sibling name when one is close — r6h's `gl`
    /// (data-stream wrote `vizGL`) and `uBrushActive` (its uniform is `uBrush`).
    pub(super) assumptions_unbacked: Vec<assumes::AssumeUnbacked>,
    /// (shard, ASSUMES clause) — clauses of words only, no code-shaped identifier; the merger
    /// reads them, nothing resolves them.
    pub(super) assumptions_prose: Vec<(String, String)>,
    pub(super) unfinished: Vec<(String, String)>,
    pub(super) prior_merge: Option<String>,
    pub(super) final_file_symbols: Vec<Symbol>,
    /// (final file, bytes, writer shard id — None = not recorded in the ledger): files already on
    /// disk at the merger's dispatch that no merge wrote (S10(3), `shard_wrote_final_file`).
    pub(super) final_on_disk: Vec<(String, u64, Option<String>)>,
    /// (shared state, shard ids) — states more than one README's WRITES names (split v2 §4).
    pub(super) shared_state_writers: Vec<(String, Vec<String>)>,
    /// (shard, ASSUMES item, its candidate names) — assumptions the DECLARED interface does not
    /// cover: no export they name, no declared shared-state root (split v2 §5). Coordination
    /// outside the declaration, met by a sibling or not; measured so the declaration can be judged
    /// before it moves into synthesis's own call.
    pub(super) interface_leaks: Vec<(String, String, Vec<String>)>,
}

fn candidate_names(item: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // backticked spans first
    let mut parts = item.split('`');
    parts.next();
    for span in parts.step_by(2) {
        if let Some(id) = ident_at(span.trim()) {
            out.push(id.trim_end_matches('.').to_string());
        }
    }
    if out.is_empty() {
        // `(` stays IN the token so `drawBrush(ids)` reads as a function reference (S12-E: the
        // old split set ate the parens and no un-backticked function was ever a candidate).
        for tok in item.split(|c: char| c.is_whitespace() || ",;:[]{}\"'".contains(c)) {
            if tok.contains('(') || tok.contains('.') {
                if let Some(id) = ident_at(tok) {
                    out.push(id.trim_end_matches('.').to_string());
                }
            }
        }
    }
    out.retain(|n| n.chars().count() >= 3);
    out
}

pub(super) async fn parse_piece(path: &std::path::Path) -> Option<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => super::parse_checks::py_syntax_error(path).await,
        Some("js") | Some("mjs") | Some("cjs") => {
            let out = tokio::process::Command::new("node")
                .arg("--check")
                .arg(path)
                .kill_on_drop(true)
                .output()
                .await;
            match out {
                Ok(o) if o.status.success() => None,
                Ok(o) => Some(
                    String::from_utf8_lossy(&o.stderr)
                        .lines()
                        .find(|l| l.contains("SyntaxError") || l.contains("Error"))
                        .unwrap_or("syntax error")
                        .trim()
                        .to_string(),
                ),
                Err(_) => Some("node not available — unchecked".to_string()),
            }
        }
        // S12-B: a file with no per-file parser is UNCHECKED, said verbatim — never "parses".
        // (`.rs` is checked at merge time by `rust_compile_error` over the module's files.)
        Some(ext) => Some(format!("unchecked ({ext}) — no per-file parser")),
        None => Some("unchecked (no extension) — no per-file parser".to_string()),
    }
}

pub(super) async fn build_merge_dossier(
    root: &std::path::Path,
    merger: &MergerOf,
    module_files: &[String],
    lang: super::TargetLang,
) -> MergeDossier {
    let mut shards: Vec<ShardDossier> = Vec::new();
    // (shard id, its pieces as (file name, source)) — the ASSUMES resolver's vocabulary and
    // free-reference scan read the text the symbols were extracted from.
    let mut sources: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for (i, id) in merger.shards.iter().enumerate() {
        let folder = merger
            .folders
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("{SHARDS_DIR}/{}/{}", merger.module, id));
        let dir = root.join(&folder);
        let readme = std::fs::read_to_string(dir.join("README.md")).ok();
        let note = readme.as_deref().and_then(parse_shard_note);
        let mut pieces = Vec::new();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| n != "README.md")
            .collect();
        names.sort();
        let mut srcs: Vec<(String, String)> = Vec::new();
        for n in names {
            let path = dir.join(&n);
            // An unreadable piece (non-UTF-8, a permission error) is SAID — never rendered as
            // "parses — no definitions found" (fallback gate, S12-D).
            let (verdict, symbols) = match std::fs::read_to_string(&path) {
                Ok(src) => {
                    let symbols = extract_symbols(&src, lang_for_path(&n, lang));
                    let verdict = parse_piece(&path).await;
                    srcs.push((n.clone(), src));
                    (verdict, symbols)
                }
                Err(e) => (Some(format!("unreadable: {e}")), Vec::new()),
            };
            pieces.push((format!("{folder}/{n}"), verdict, symbols));
        }
        sources.push((id.clone(), srcs));
        shards.push(ShardDossier {
            id: id.clone(),
            folder,
            readme_present: readme.as_ref().is_some_and(|r| !r.trim().is_empty()),
            note,
            pieces,
            wrote_final: match shard_ledger_row(root, id) {
                Some(row) => string_list(&row["wrote_final"]),
                None => Vec::new(),
            },
            provides_unbacked: Vec::new(),
        });
    }
    // PROVIDES MUST BE BACKED (DESIGN-SPLIT-V2 §3): a README item with no definition behind it
    // is recorded per shard; the brief lists it under GAPS and the dispatch says it by name.
    for sh in shards.iter_mut() {
        sh.provides_unbacked = sh.unbacked_provides();
    }
    // duplicates: a name defined in two shards
    let mut by_name: Vec<(String, Vec<String>)> = Vec::new();
    for sh in &shards {
        for s in sh.defines() {
            match by_name.iter_mut().find(|(n, _)| *n == s.name) {
                Some((_, owners)) => {
                    if !owners.contains(&sh.id) {
                        owners.push(sh.id.clone());
                    }
                }
                None => by_name.push((s.name.clone(), vec![sh.id.clone()])),
            }
        }
    }
    let duplicates: Vec<(String, Vec<String>)> = by_name
        .iter()
        .filter(|(_, o)| o.len() > 1)
        .cloned()
        .collect();
    let mut declared_missing = Vec::new();
    let mut signature_disagreements = Vec::new();
    for e in &merger.interface.exports {
        let mut found = false;
        for sh in &shards {
            for s in sh.defines() {
                if same_symbol(&e.name, &s.name) {
                    found = true;
                    if let Some(declared_params) = params_after(&e.signature) {
                        let d = normalize_params(&declared_params);
                        let f = normalize_params(&s.params);
                        if !d.is_empty() && !f.is_empty() && d != f {
                            signature_disagreements.push((
                                e.name.clone(),
                                e.signature.clone(),
                                s.params.clone(),
                                sh.id.clone(),
                            ));
                        }
                    }
                }
            }
        }
        // A DEFINITION or nothing: a README's PROVIDES claim with no piece behind it is a promise
        // (`provides_unbacked`), and a promised export is a GAP the merger must fill — v1 let the
        // claim stand in for the code (DESIGN-SPLIT-V2 §3).
        if !found {
            declared_missing.push(e.name.clone());
        }
    }
    let mut unfinished = Vec::new();
    let mut interface_leaks: Vec<(String, String, Vec<String>)> = Vec::new();
    // ASSUMES are resolved per NAME against every shard's DEFINITIONS and the declared
    // shared-state roots (`assumes::resolve`, VA-108) — the old any-candidate-met rule never saw
    // r6h's `gl` or `uBrushActive` and flagged a keyword and a global's member instead.
    let resolved = assumes::resolve(&shards, &sources, &merger.interface.shared_state);
    // An assumption about the DECLARED shared state (`S.dirty` when the declaration names `S`)
    // is covered by the declaration — the merger reconciles the shape.
    let declared_roots: Vec<String> = merger
        .interface
        .shared_state
        .split(|c: char| !(is_ident_char(c) || c == '.'))
        .filter(|w| !w.is_empty())
        .map(|w| w.split('.').next().unwrap_or(w).to_string())
        .collect();
    for sh in &shards {
        if let Some(n) = &sh.note {
            for a in &n.assumes {
                let names = candidate_names(a);
                // INTERFACE LEAK (split v2 §5): does the DECLARATION cover this assumption — an
                // export it names (any word of it, `same_symbol`) or a declared shared-state root?
                // If not, the shards coordinated outside the declaration, whether or not a sibling
                // happens to define the name; a prose assumption naming nothing is a leak with no
                // names. Measured, never refused.
                let mentions: Vec<&str> = a
                    .split(|c: char| c.is_whitespace() || ",;:\"'()[]{}`".contains(c))
                    .filter(|t| !t.is_empty())
                    .chain(names.iter().map(String::as_str))
                    .collect();
                let covered = mentions.iter().any(|m| {
                    merger
                        .interface
                        .exports
                        .iter()
                        .any(|e| same_symbol(&e.name, m))
                        || m.split('.')
                            .next()
                            .is_some_and(|root| declared_roots.iter().any(|r| r == root))
                });
                if !covered {
                    interface_leaks.push((sh.id.clone(), a.clone(), names.clone()));
                }
            }
            for u in &n.unfinished {
                unfinished.push((sh.id.clone(), u.clone()));
            }
        }
    }
    let prior_merge = std::fs::read_to_string(
        root.join(SHARDS_DIR)
            .join(&merger.module)
            .join(MERGE_README),
    )
    .ok()
    .filter(|t| !t.trim().is_empty());
    let mut final_file_symbols = Vec::new();
    for f in module_files {
        if let Ok(src) = std::fs::read_to_string(root.join(f)) {
            final_file_symbols.extend(extract_symbols(&src, lang_for_path(f, lang)));
        }
    }
    // S10(3): a final file on disk BEFORE any merge was written by someone else — a shard that
    // wrote outside the piece protocol (its row names it) or a writer the ledger did not record
    // (said as such). On a second pass the merger's own file is expected and only a shard-claimed
    // write is listed.
    let mut final_on_disk: Vec<(String, u64, Option<String>)> = Vec::new();
    for f in module_files {
        let Ok(meta) = std::fs::metadata(root.join(f)) else {
            continue;
        };
        let writer = shards
            .iter()
            .find(|sh| sh.wrote_final.iter().any(|w| w == f))
            .map(|sh| sh.id.clone());
        if writer.is_some() || prior_merge.is_none() {
            final_on_disk.push((f.clone(), meta.len(), writer));
        }
    }
    let shared_state_writers = shared_state_writers(
        &shards
            .iter()
            .filter_map(|sh| sh.note.as_ref().map(|n| (sh.id.clone(), n.writes.clone())))
            .collect::<Vec<_>>(),
    );
    MergeDossier {
        module: merger.module.clone(),
        files: module_files.to_vec(),
        interface: merger.interface.clone(),
        shards,
        duplicates,
        declared_missing,
        signature_disagreements,
        assumptions_unmet: resolved.unmet,
        assumptions_unbacked: resolved.unbacked,
        assumptions_prose: resolved.prose,
        unfinished,
        prior_merge,
        final_file_symbols,
        final_on_disk,
        shared_state_writers,
        interface_leaks,
    }
}

impl MergeDossier {
    /// The unbacked names of one (shard, ASSUMES clause), in clause order.
    fn unbacked_of<'a>(
        &'a self,
        shard: &'a str,
        clause: &'a str,
    ) -> impl Iterator<Item = &'a assumes::AssumeUnbacked> + 'a {
        self.assumptions_unbacked
            .iter()
            .filter(move |u| u.shard == shard && u.clause == clause)
    }

    /// A shard that delivered at least one piece file — the only kind THE PIECES lists and the
    /// assembly reads. One with none (README-only, or a bare folder) is named ONCE, as a GAP, by
    /// the dispatch paragraph swarm.rs appends last (`merge_holes::gap_paragraph`); the brief's
    /// numbered items skip it so the merger never reads a part that does not exist (VA-085).
    fn built(&self, shard: &str) -> bool {
        self.shards
            .iter()
            .any(|sh| sh.id == shard && !sh.pieces.is_empty())
    }

    /// One `shard_provides_unbacked{module, task_id, shard, names}` per shard whose README promises
    /// a symbol no piece defines (DESIGN-SPLIT-V2 §3) — said at the merger's dispatch; the brief
    /// lists the same names under GAPS.
    pub(super) fn provides_unbacked_events(&self, task_id: &str) -> Vec<serde_json::Value> {
        self.shards
            .iter()
            .filter(|s| !s.provides_unbacked.is_empty())
            .map(|s| {
                serde_json::json!({
                    "event": "shard_provides_unbacked",
                    "module": self.module,
                    "task_id": task_id,
                    "shard": s.id,
                    "names": s.provides_unbacked,
                })
            })
            .collect()
    }

    /// One `interface_leak{module, task_id, shard, assumption, names}` per ASSUMES item the declared
    /// interface does not cover (split v2 §5) — at the merger's dispatch, an instrument only.
    pub(super) fn interface_leak_events(&self, task_id: &str) -> Vec<serde_json::Value> {
        self.interface_leaks
            .iter()
            .map(|(shard, assumption, names)| {
                serde_json::json!({
                    "event": "interface_leak",
                    "module": self.module,
                    "task_id": task_id,
                    "shard": shard,
                    "assumption": assumption,
                    "names": names,
                })
            })
            .collect()
    }

    pub(super) fn summary_json(&self) -> serde_json::Value {
        serde_json::json!({
            "module": self.module,
            "shards": self.shards.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            "pieces": self.shards.iter().map(|s| s.pieces.len()).sum::<usize>(),
            "pieces_with_parse_errors": self.shards.iter().flat_map(|s| s.pieces.iter()).filter(|(_, v, _)| v.as_deref().is_some_and(|e| !e.contains("unchecked"))).count(),
            "readmes_missing": self.shards.iter().filter(|s| s.note.is_none()).map(|s| s.id.clone()).collect::<Vec<_>>(),
            "pieces_absent": self.shards.iter().filter(|s| s.note.is_some() && s.pieces.is_empty()).map(|s| s.id.clone()).collect::<Vec<_>>(),
            "duplicates": self.duplicates.iter().map(|(n, o)| serde_json::json!({"symbol": n, "shards": o})).collect::<Vec<_>>(),
            "declared_missing": self.declared_missing,
            "signature_disagreements": self.signature_disagreements.iter().map(|(n, d, f, s)| serde_json::json!({"symbol": n, "declared": d, "found_params": f, "shard": s})).collect::<Vec<_>>(),
            "assumptions_unmet": self.assumptions_unmet.iter().map(|(s, a)| serde_json::json!({"shard": s, "assumes": a, "names": self.unbacked_of(s, a).map(assumes::AssumeUnbacked::to_json).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "assumptions_prose": self.assumptions_prose.iter().map(|(s, a)| serde_json::json!({"shard": s, "assumes": a})).collect::<Vec<_>>(),
            "unfinished": self.unfinished.iter().map(|(s, u)| serde_json::json!({"shard": s, "item": u})).collect::<Vec<_>>(),
            "provides_unbacked": self.shards.iter().filter(|s| !s.provides_unbacked.is_empty()).map(|s| serde_json::json!({"shard": s.id, "names": s.provides_unbacked})).collect::<Vec<_>>(),
            "shared_state_writers": self.shared_state_writers.iter().map(|(st, sh)| serde_json::json!({"state": st, "shards": sh})).collect::<Vec<_>>(),
            "interface_leaks": self.interface_leaks.iter().map(|(sh, a, names)| serde_json::json!({"shard": sh, "assumes": a, "names": names})).collect::<Vec<_>>(),
            "second_pass": self.prior_merge.is_some(),
            "final_on_disk": self.final_on_disk.iter().map(|(f, n, w)| serde_json::json!({"path": f, "bytes": n, "written_by_shard": w})).collect::<Vec<_>>(),
        })
    }

    /// The MERGER BRIEF: a NUMBERED, SPECIFIC task list from the dossier — never "merge the
    /// module". Mihai 11:4x: "the merge node should actually be judicious in its work not just go
    /// in and copy… the task of doing the merge needs to be specific". With an `assembly` (code
    /// placed the definitions, DESIGN-SPLIT-V2 §1) the job is the GLUE and retyping a definition
    /// is named a defect; without one (no parser for the module's extension, no piece of its
    /// language) the v1 shell-assembly item stands.
    pub(super) fn merger_brief(&self, cwd: &str, assembly: Option<&assembly::Assembly>) -> String {
        let final_files = self
            .files
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let on_disk = if self.final_on_disk.is_empty() {
            format!("NOBODY has written {final_files}")
        } else {
            self.final_on_disk
                .iter()
                .map(|(f, n, w)| match w {
                    Some(w) => format!(
                        "shard `{w}` wrote `{f}` DIRECTLY ({n} bytes) — one more piece to reconcile, not the finished module"
                    ),
                    None => format!(
                        "`{f}` is already on disk ({n} bytes) and no shard's ledger row claims it — one more piece to reconcile, not the finished module"
                    ),
                })
                .collect::<Vec<_>>()
                .join("; ")
        };
        let built = self
            .shards
            .iter()
            .filter(|sh| !sh.pieces.is_empty())
            .count();
        // A shard with no piece file is counted here and NAMED only by the dispatch paragraph
        // (`merge_holes::gap_paragraph`) — the brief and the assembly agree on which pieces exist.
        let absent = match self.shards.len() - built {
            0 => String::new(),
            k => format!(
                " {k} more shard{plural} delivered NO piece file — nothing of {it} is in THE PIECES below{asm}; \
                 CODE names {it} by id and folder under the dispatch GAPS at the end of this brief.",
                plural = if k == 1 { "" } else { "s" },
                it = if k == 1 { "it" } else { "them" },
                asm = if assembly.is_some() { " or the assembled file" } else { "" },
            ),
        };
        let mut s = match assembly {
            Some(a) => format!(
                "YOU ARE THE MERGER OF MODULE `{module}`. {n} shards built its pieces in parallel, each in \
                 its own folder under `{cwd}/{dir}/{module}/`; {on_disk}.{absent} CODE HAS ALREADY ASSEMBLED their \
                 definitions into `{cwd}/{asm}`: {defs} definition block(s) from {pieces} piece(s) — {by_if} \
                 placed in the declared interface's order, {unk} appended after it because no export names \
                 them — with {imports} import line(s) first and {stmts} top-level statement(s) (state, \
                 wiring, boot) collected at the end, each under its `shard:` line. YOUR JOB IS THE GLUE, NOT \
                 THE DEFINITIONS: start FROM the assembled file (copy `{cwd}/{asm}` to {final_files} and \
                 edit there), write what only the merger can — the imports/exports, the shared state's ONE \
                 initialisation, the wiring and event plumbing, every UNFINISHED item and every GAP numbered \
                 below — and produce {final_files}. THE PIECES ARE ALREADY IN PLACE: copying or retyping a \
                 definition is FORBIDDEN and is a defect (a retyped definition is where pieces get dropped \
                 and signatures drift); change a definition's body only where a numbered task says so.\n\n",
                module = self.module,
                n = built,
                dir = SHARDS_DIR,
                asm = a.path,
                defs = a.definitions,
                pieces = a.pieces,
                by_if = a.ordered_by_interface,
                unk = a.appended_unknown.len(),
                imports = a.imports,
                stmts = a.statements.iter().map(|(_, n)| n).sum::<usize>(),
            ),
            None => format!(
                "YOU ARE THE MERGER OF MODULE `{module}`. {n} shards built its pieces in parallel, each in \
                 its own folder under `{cwd}/{dir}/{module}/`; {on_disk}.{absent} You write {final_files} \
                 from their pieces, judiciously: read every piece and every README below, reconcile, dedupe, \
                 fill the small gaps yourself, send the big ones out, then ASSEMBLE.\n\n",
                module = self.module,
                n = built,
                dir = SHARDS_DIR,
            ),
        };
        if let Some(prior) = &self.prior_merge {
            s.push_str(&format!(
                "SECOND PASS: you already merged once; the gaps you sent out have landed as new shards \
                 (listed below). Fold their pieces into the EXISTING {final_files} — do not restart from \
                 the pieces. Your previous merge README:\n{prior}\n\n"
            ));
        }
        s.push_str("THE PIECES (path — parse — definitions):\n");
        for sh in self.shards.iter().filter(|sh| !sh.pieces.is_empty()) {
            s.push_str(&format!(
                "shard `{}` — folder `{cwd}/{}`{}:\n",
                sh.id,
                sh.folder,
                if sh.readme_present {
                    ""
                } else {
                    " — NO README (its handoff is missing; read the pieces harder)"
                }
            ));
            for (path, verdict, symbols) in &sh.pieces {
                let names: Vec<String> = symbols
                    .iter()
                    .map(|x| {
                        if x.params.is_empty() {
                            format!("`{}`{}", x.name, x.kind.suffix())
                        } else {
                            format!("`{}({})`", x.name, x.params)
                        }
                    })
                    .collect();
                s.push_str(&format!(
                    "  - `{cwd}/{path}` — {} — {}\n",
                    match verdict {
                        None => "parses".to_string(),
                        Some(e) if e.contains("unchecked") => e.clone(),
                        Some(e) => format!("PARSE ERROR: {e}"),
                    },
                    if names.is_empty() {
                        "no definitions found".to_string()
                    } else {
                        names.join(", ")
                    }
                ));
            }
            if let Some(n) = &sh.note {
                let backed: Vec<&str> = n
                    .provides
                    .iter()
                    .filter(|p| !sh.provides_unbacked.contains(*p))
                    .map(String::as_str)
                    .collect();
                if !backed.is_empty() {
                    s.push_str(&format!(
                        "  PROVIDES (each backed by a definition above): {}\n",
                        backed.join("; ")
                    ));
                }
                if !sh.provides_unbacked.is_empty() {
                    s.push_str(&format!(
                        "  PROVIDES WITHOUT A DEFINITION (promises — GAPS below, not deliveries): {}\n",
                        sh.provides_unbacked.join("; ")
                    ));
                }
                if !n.assumes.is_empty() {
                    s.push_str(&format!("  ASSUMES: {}\n", n.assumes.join("; ")));
                }
                if !n.unfinished.is_empty() {
                    s.push_str(&format!("  UNFINISHED: {}\n", n.unfinished.join("; ")));
                }
                if !n.checked_with.is_empty() {
                    s.push_str(&format!("  CHECKED_WITH: {}\n", n.checked_with.join("; ")));
                }
            }
        }
        s.push_str(&format!(
            "\nTHE DECLARED INTERFACE (binding — every export below must exist in the final file with this signature):\n{}\n",
            render_interface(&self.interface)
        ));
        s.push_str("YOUR TASKS, in order — each one is specific, do it and say what you did in MERGE.md:\n");
        let mut k = 0usize;
        let mut item = |s: &mut String, text: String| {
            k += 1;
            s.push_str(&format!("{k}. {text}\n"));
        };
        for (f, n, w) in &self.final_on_disk {
            let names: Vec<String> = self
                .final_file_symbols
                .iter()
                .map(|x| format!("`{}`", x.name))
                .collect();
            item(&mut s, format!(
                "`{cwd}/{f}` already exists ({n} bytes; {}) — it was written outside the piece protocol. Read it as ONE MORE PIECE: check its definitions ({}) against the declaration and the other shards' pieces, keep what is right and name it under KEPT, then REBUILD the file in the declared order from the pieces; do not treat it as finished.",
                match w {
                    Some(w) => format!("written by shard `{w}`"),
                    None => "writer not recorded in the ledger".to_string(),
                },
                if names.is_empty() { "none found".to_string() } else { names.join(", ") }
            ));
        }
        for (name, owners) in &self.duplicates {
            item(
                &mut s,
                format!(
                    "`{name}` is defined in shards {} — {}name which and why under KEPT/DROPPED.",
                    owners
                        .iter()
                        .map(|o| format!("`{o}`"))
                        .collect::<Vec<_>>()
                        .join(" and "),
                    if assembly.is_some() {
                        format!(
                        "BOTH definitions are in the assembled file under `{}` markers; keep ONE (delete the other, or rename if they are different things) and ",
                        assembly::DUPLICATE_MARKER
                    )
                    } else {
                        "keep ONE definition (or rename if they are different things), ".to_string()
                    }
                ),
            );
        }
        for (name, declared, found, shard) in &self.signature_disagreements {
            item(&mut s, format!(
                "`{name}` is declared `{declared}` but shard `{shard}` defines it with `({found})` — reconcile to the DECLARED signature and fix every caller."
            ));
        }
        for (state, writers) in &self.shared_state_writers {
            let writers: Vec<&String> = writers.iter().filter(|w| self.built(w)).collect();
            if writers.len() < 2 {
                continue;
            }
            item(&mut s, format!(
                "shared state `{state}` is WRITTEN by shards {} (their READMEs' WRITES) — the declaration names ONE writer per state; keep one shard's writes and route the other's through it (or the declared API), and say which under KEPT/DROPPED.",
                writers.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(" and ")
            ));
        }
        for (shard, assumption) in &self.assumptions_unmet {
            if !self.built(shard) {
                continue;
            }
            // Per NAME: the two names, the two shards, the rule — and the decision left to the
            // merger (VA-108; r6h's `gl` → `vizGL`, `uBrushActive` → `uBrush`).
            let names: Vec<String> = self
                .unbacked_of(shard, assumption)
                .map(assumes::AssumeUnbacked::glue)
                .collect();
            item(
                &mut s,
                format!(
                    "shard `{shard}` ASSUMES \"{assumption}\" and no shard provides it — {}",
                    if names.is_empty() {
                        "reconcile to the declared interface/shared state; if that means new code, write it or send it out.".to_string()
                    } else {
                        names.join(" ")
                    }
                ),
            );
        }
        for name in &self.declared_missing {
            item(&mut s, format!(
                "`{name}` is DECLARED and defined by no shard — write it yourself if it is small, else send it out (MERGE_GAP below)."
            ));
        }
        for sh in &self.shards {
            if !sh.pieces.is_empty() && !sh.provides_unbacked.is_empty() {
                item(&mut s, format!(
                    "shard `{}`'s README PROVIDES {} but no piece in `{cwd}/{}` DEFINES them — promises, not deliveries: they are GAPS. Write each yourself if it is small, else send it out (MERGE_GAP below); never list one under KEPT.",
                    sh.id,
                    sh.provides_unbacked.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", "),
                    sh.folder
                ));
            }
        }
        for (shard, u) in &self.unfinished {
            if !self.built(shard) {
                continue;
            }
            item(&mut s, format!(
                "shard `{shard}` left UNFINISHED: \"{u}\" — fill it yourself if it is small, else send it out (MERGE_GAP below); either way name it under FILLED or SENT_OUT."
            ));
        }
        for sh in &self.shards {
            for (path, verdict, _) in &sh.pieces {
                if let Some(e) = verdict {
                    if !e.contains("unchecked") {
                        item(
                            &mut s,
                            format!("`{path}` does not parse ({e}) — fix it as you fold it in."),
                        );
                    }
                }
            }
            if !sh.pieces.is_empty() && !sh.readme_present {
                item(&mut s, format!("shard `{}` left no README — derive what it provides from its pieces and say so under KEPT.", sh.id));
            }
        }
        let order = if self.interface.layout.is_empty() {
            "NO LAYOUT DECLARED — synthesis named no assembly order; choose the order the module's brief implies and say the order you chose under KEPT".to_string()
        } else {
            self.interface.layout.join(" → ")
        };
        match assembly {
            Some(a) => item(&mut s, format!(
                "WRITE THE GLUE into {final_files}, starting from `{cwd}/{asm}` — never from memory. The declared \
                 layout is {order}: move each collected top-level statement to where the layout puts it (state \
                 before its first use, boot last), initialise the shared state ONCE, wire the exports exactly as \
                 declared, add the imports/exports the final file needs, and remove every `{marker}` marker by \
                 keeping ONE definition per name. Glue the engine measured as needed: {glue}. Do not retype a \
                 definition block; do not `cat` the piece folders again — they are already in the assembled file.",
                asm = a.path,
                marker = assembly::DUPLICATE_MARKER,
                glue = if a.glue_needed.is_empty() {
                    "none measured beyond the layout".to_string()
                } else {
                    a.glue_needed.join(", ")
                },
            )),
            None => item(&mut s, format!(
                "ASSEMBLE {final_files} in the declared order — {order}. ASSEMBLE, DON'T RETYPE: build the file by \
                 concatenating the piece files in that order with the shell (e.g. `cat <piece> <piece> … > <file>`), \
                 then EDIT the result for the glue — one shared state object, one definition per name, the exports \
                 wired exactly as declared, no leftover duplicate. Retyping 40 KB from memory is the slowest and least \
                 faithful path and is how pieces get silently dropped."
            )),
        }
        item(&mut s, format!(
            "Check the assembled file parses (`node --check` / `python3 -m py_compile` / the language's equivalent) \
             and that EVERY declared export exists in it with the declared signature; then write \
             `{cwd}/{dir}/{module}/{readme}` with four fields, one item per line: {f0}: (which duplicate/version you kept and why), \
             {f1}: (any piece symbol you left out and why), {f2}: (gaps you filled yourself), {f3}: (gaps you sent out).",
            dir = SHARDS_DIR, module = self.module, readme = MERGE_README,
            f0 = MERGE_FIELDS[0], f1 = MERGE_FIELDS[1], f2 = MERGE_FIELDS[2], f3 = MERGE_FIELDS[3]
        ));
        s.push_str(&format!(
            "\nSENDING WORK OUT: for a gap you judge too big to fill during the merge, put ONE line per gap in your \
             final message, exactly `{gap} <what is missing — the declared names, the spec section, what it must do>`. \
             The engine dispatches each to a free machine as a new shard immediately and calls you back to fold its \
             pieces in; the module is not done until then. Use it for real gaps, not for work you can finish now.\n\n\
             THE MODULE'S BRIEF (whole — what the finished module must do; the spec sections are the `###` blocks):\n\n",
            gap = MERGE_GAP_PREFIX
        ));
        s
    }
}

/// S14-4: the deliverable gate's retry hint for a MERGER that finished without its final file(s) —
/// ASSEMBLE from the pieces, never the file author's "write EACH of them IN FULL" (the retype the
/// numbered brief forbids). The retry is the gate's own, unchanged.
pub(super) fn merger_missing_hint(merger: &MergerOf, missing: &[String]) -> String {
    format!(
        "You finished WITHOUT writing module `{module}`'s final file(s): {files}. You are the MERGER: \
         ASSEMBLE them from the shard pieces your numbered brief names — start from the engine's \
         `{dir}/{module}/{asm}.<ext>` when the brief names one (the definitions are already in place; \
         copy it over the final file and write the glue), else `cat` the piece files in the declared \
         order into the final file, then edit the glue — and write `{dir}/{module}/{readme}` \
         ({fields}). Do not retype the module from memory and do not explore beyond the piece folders; \
         the pieces are the source.",
        asm = assembly::ASSEMBLED_STEM,
        module = merger.module,
        files = missing.join(", "),
        dir = SHARDS_DIR,
        readme = MERGE_README,
        fields = MERGE_FIELDS.join(" / "),
    )
}

/// The MERGER's owner body — replaces the file author's WRITE FIRST / STATIC ASSETS scripts,
/// whose every clause ("write your owned file IN FULL from the spec", "NEVER `cat` the module",
/// "`node --check` … nothing else") is the retype the numbered brief forbids (S12-C).
pub(super) fn merger_owner_body() -> String {
    "YOU ARE THE MERGER. Your task statement below is a NUMBERED list built by the engine from the \
     shards' pieces and READMEs — work it in order. READ every piece folder and README it names \
     (that reading IS the job, not over-reading). When the list names an ASSEMBLED file the engine \
     built, START FROM IT: the definitions are already in place, you write the GLUE (imports/exports, \
     the shared state's one initialisation, wiring, the unfinished items and gaps) and never retype a \
     definition — a retyped definition is a defect. Otherwise ASSEMBLE the final file(s) by \
     concatenating the piece files in the declared order with the shell, then EDIT the result for the \
     glue. Never retype the module from memory — the pieces are the source. Check the assembled file parses, \
     every declared export is DEFINED with its declared signature, write MERGE.md (KEPT / DROPPED \
     / FILLED / SENT_OUT), and put one `MERGE_GAP:` line per gap you send out in your final \
     message.\n\n"
        .to_string()
}

/// The merger's reading rule for the system prompt's rules block (S12-C): the kind-generic "Read
/// AT MOST the ONE file you will edit" is wrong-job text for a task whose job is reading N folders.
pub(super) const MERGER_READING_RULE: &str = "- READ EVERY PIECE FOLDER AND README your task \
     names — the merge IS reading them. Then assemble by shell and edit the glue. Do not re-read \
     the rest of the project.\n";

/// `MERGE_GAP: …` lines in the merger's final message, in order, deduped.
pub(super) fn parse_merge_gaps(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line
            .trim()
            .trim_start_matches(['-', '*', '•', '>', '#'])
            .trim()
            .trim_matches('`')
            .trim_matches('*');
        let Some((head, tail)) = t.split_at_checked(MERGE_GAP_PREFIX.len()) else {
            continue;
        };
        if head.eq_ignore_ascii_case(MERGE_GAP_PREFIX) {
            let rest = tail
                .trim_start_matches(['*', '`', ' '])
                .trim()
                .trim_end_matches(['*', '`'])
                .trim();
            if !rest.is_empty()
                && !rest.eq_ignore_ascii_case("none")
                && !out.iter().any(|o| o == rest)
            {
                out.push(rest.to_string());
            }
        }
    }
    out
}

/// The shard specs for a merger's gaps — the engine's follow-ups the scheduler splices through the
/// merge-gap door: each a new shard of the same module in `gap-<k>/`, owning only its README,
/// depending on nothing, briefed with the gap text, the siblings' provides and the declaration.
pub(super) fn gap_specs(
    merger: &MergerOf,
    module_files: &[String],
    module_brief: &str,
    dossier: &MergeDossier,
    gaps: &[String],
) -> Vec<goose_swarm::TaskSpec> {
    let mut siblings: Vec<ShardPlan> = dossier
        .shards
        .iter()
        .map(|sh| ShardPlan {
            id: last_segment(&sh.id).to_string(),
            responsibility: sh
                .note
                .as_ref()
                .map(|n| n.provides.join(", "))
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| format!("(no README) pieces in {}", sh.folder)),
            sections: Vec::new(),
            // No README → no PROVIDES / WRITES lists; the absence itself rode `merge_note_missing`
            // at the shard's completion, so empty MEANS empty here (fallback gate) — one match, no
            // silent default.
            provides: match sh.note.as_ref() {
                Some(n) => n.provides.clone(),
                None => Vec::new(),
            },
            writes: match sh.note.as_ref() {
                Some(n) => n.writes.clone(),
                None => Vec::new(),
            },
            // a finished shard is one piece to its merger; no partition ran over it
            clusters: Vec::new(),
        })
        .collect();
    let base = merger.shards.len();
    let plans: Vec<ShardPlan> = gaps
        .iter()
        .enumerate()
        .map(|(i, g)| ShardPlan {
            id: format!("gap-{}", base + i + 1),
            responsibility: format!(
                "MERGE GAP sent out by the merger of `{}`: {g}",
                merger.module
            ),
            sections: Vec::new(),
            // a gap shard fills a hole; it writes no shared state of its own (split v2 §4)
            writes: Vec::new(),
            provides: candidate_names(g),
            clusters: Vec::new(),
        })
        .collect();
    siblings.extend(plans.iter().cloned());
    plans
        .iter()
        .map(|p| {
            let folder = format!("{SHARDS_DIR}/{}/{}", merger.module, p.id);
            let id = format!("{}-{}", merger.module, p.id);
            let brief = format!(
                "MERGE GAP — the merger of `{}` read every shard's pieces and README and found this MISSING; \
                 it is sent to you to build as a new shard, the merger folds it in when you finish:\n{}\n\n{}",
                merger.module,
                p.responsibility,
                shard_brief(&merger.module, module_files, module_brief, p, &siblings, &folder, &merger.interface)
            );
            goose_swarm::TaskSpec {
                id: id.clone(),
                description: brief,
                difficulty: goose_swarm::Difficulty::Hard,
                preferred_model: None,
                owned_files: vec![format!("{folder}/README.md")],
                deps: Vec::new(),
                subsplit: Vec::new(),
                shard_of: Some(ShardOf {
                    module: merger.module.clone(),
                    shard: p.id.clone(),
                    folder,
                    responsibility: p.responsibility.clone(),
                    interface: merger.interface.clone(),
                    module_files: module_files.to_vec(),
                }),
                merger_of: None,
            }
        })
        .collect()
}

/// Does a merger's `MERGE_GAP:` line name a README's UNFINISHED item — a symbol the item names,
/// or either text inside the other? The rule `check_merge` reconciles `gaps_open` with;
/// `predictable_gaps` reads it the other way round.
pub(super) fn gap_covers_unfinished(gap: &str, unfinished: &str) -> bool {
    candidate_names(unfinished)
        .iter()
        .any(|n| gap.contains(n.as_str()))
        || gap.contains(unfinished)
        || unfinished.contains(gap)
        || shares_identifier(gap, unfinished)
}

/// Two free-text items name the same thing when they share an identifier-shaped token (a name a
/// piece could define: alphabetic start, >= 4 chars, case kept) — "drawBrush(ids) — dim
/// non-members" and "drawBrush the non-members" share `drawBrush` without either being a
/// substring of the other. Plain English words are excluded by the identifier shape (a camelCase
/// or snake_case token, or one holding a digit).
fn shares_identifier(a: &str, b: &str) -> bool {
    let idents = |t: &str| -> Vec<String> {
        t.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| w.chars().count() >= 4)
            .filter(|w| w.chars().next().is_some_and(|c| c.is_alphabetic()))
            .filter(|w| {
                w.contains('_')
                    || w.chars().any(|c| c.is_ascii_digit())
                    || (w.chars().any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase()))
            })
            .map(str::to_string)
            .collect()
    };
    let la = idents(a);
    idents(b).iter().any(|w| la.contains(w))
}

/// Split v2 §5, the other half of the interface-fidelity measure: a gap the merger sent out that
/// some README had ALREADY listed UNFINISHED was predictable before the merger started — the
/// shard said it and the engine could have dispatched it beside the merge; a gap no README foresaw
/// was discovered at the merge. (gap, shard, unfinished item) per predictable gap.
pub(super) fn predictable_gaps(
    dossier: &MergeDossier,
    gaps: &[String],
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for g in gaps {
        for (sh, u) in &dossier.unfinished {
            if gap_covers_unfinished(g, u) {
                out.push((g.clone(), sh.clone(), u.clone()));
            }
        }
    }
    out
}

/// What CODE found after the merge (MILD — reported, never a refusal).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MergeCheck {
    pub(super) parse_errors: Vec<(String, String)>,
    /// Final files no parser could check (said, and they block `promoted`).
    pub(super) unchecked: Vec<(String, String)>,
    pub(super) declared_present: Vec<String>,
    pub(super) declared_missing: Vec<String>,
    /// (name, declared signature, params found on the definition)
    pub(super) signature_mismatch: Vec<(String, String, String)>,
    /// (shard, symbol, referenced-but-undefined in the final file)
    pub(super) dropped: Vec<(String, String, bool)>,
    pub(super) gaps_open: Vec<(String, String)>,
    pub(super) merge_readme_present: bool,
    pub(super) promoted: bool,
}

pub(super) async fn check_merge(
    root: &std::path::Path,
    dossier: &MergeDossier,
    lang: super::TargetLang,
    gaps_sent: &[String],
) -> MergeCheck {
    let mut parse_errors = Vec::new();
    let mut unchecked: Vec<(String, String)> = Vec::new();
    let mut final_symbols: Vec<Symbol> = Vec::new();
    let mut final_text = String::new();
    // S12-B: `.rs` has no per-file parser — the crate check over the module's files is its parse.
    // S14-3: the check is a TRI-STATE; cargo not running (no manifest, no toolchain, the build
    // failing outside these files) is UNCHECKED with the reason, never "checked".
    let rust_check = super::parse_checks::rust_compile_error(root, &dossier.files).await;
    for f in &dossier.files {
        let path = root.join(f);
        match parse_piece(&path).await {
            Some(e) if e.contains("unchecked") => {
                if f.ends_with(".rs") {
                    match &rust_check {
                        super::parse_checks::RustCheck::Ran(Some((file, err))) if file == f => {
                            parse_errors.push((file.clone(), err.clone()));
                        }
                        super::parse_checks::RustCheck::Ran(_) => {}
                        super::parse_checks::RustCheck::DidNotRun(reason) => unchecked.push((
                            f.clone(),
                            format!("unchecked (rs) — cargo check did not run: {reason}"),
                        )),
                    }
                } else {
                    unchecked.push((f.clone(), e));
                }
            }
            Some(e) => parse_errors.push((f.clone(), e)),
            None => {}
        }
        if let Ok(src) = std::fs::read_to_string(&path) {
            final_symbols.extend(extract_symbols(&src, lang_for_path(f, lang)));
            final_text.push_str(&src);
            final_text.push('\n');
        }
    }
    // S12-A: conformance is a DEFINITION in the final file (`extract_symbols`, the same extractor
    // the dossier used on the pieces) — never a text mention: `// TODO drawBrush` and a dangling
    // `initGL()` call both mention a name and define nothing.
    let defined = |name: &str| -> Option<&Symbol> {
        final_symbols
            .iter()
            .find(|s| !s.shorthand && same_symbol(name, &s.name))
    };
    let referenced = |name: &str| -> bool {
        let seg = last_segment(name);
        final_text.match_indices(seg).any(|(i, _)| {
            let (before_text, rest) = final_text.split_at(i);
            let before = before_text.chars().next_back();
            let after = rest.split_at(seg.len()).1.chars().next();
            !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
        })
    };
    let mut declared_present = Vec::new();
    let mut declared_missing = Vec::new();
    let mut signature_mismatch = Vec::new();
    for e in &dossier.interface.exports {
        match defined(&e.name) {
            Some(sym) => {
                declared_present.push(e.name.clone());
                if let Some(declared_params) = params_after(&e.signature) {
                    let d = normalize_params(&declared_params);
                    let f = normalize_params(&sym.params);
                    if !d.is_empty() && !f.is_empty() && d != f {
                        signature_mismatch.push((
                            e.name.clone(),
                            e.signature.clone(),
                            sym.params.clone(),
                        ));
                    }
                }
            }
            None => declared_missing.push(e.name.clone()),
        }
    }
    let merge_readme = std::fs::read_to_string(
        root.join(SHARDS_DIR)
            .join(&dossier.module)
            .join(MERGE_README),
    )
    .ok();
    // No MERGE.md (or one with no field) → no explanations; `merge_readme_present` reports the
    // absence and `promoted` requires the README, so empty MEANS empty here (fallback gate).
    let merge_fields = merge_readme
        .as_deref()
        .and_then(|t| parse_fields(t, &MERGE_FIELDS))
        .unwrap_or_default();
    let explained = |name: &str, field: usize| -> bool {
        merge_fields
            .get(field)
            .is_some_and(|items| items.iter().any(|it| it.contains(name)))
    };
    let mut dropped = Vec::new();
    for sh in &dossier.shards {
        for s in sh.defines() {
            if defined(&s.name).is_none() && !explained(&s.name, 1) {
                // referenced-but-undefined is the WORST drop: the merged file CALLS what it lost.
                dropped.push((sh.id.clone(), s.name.clone(), referenced(&s.name)));
            }
        }
    }
    let mut gaps_open = Vec::new();
    for (sh, u) in &dossier.unfinished {
        let sent = gaps_sent.iter().any(|g| gap_covers_unfinished(g, u));
        let filled = merge_fields.get(2).is_some_and(|items| {
            items.iter().any(|it| {
                it.contains(u.as_str()) || candidate_names(u).iter().any(|n| it.contains(n))
            })
        }) || merge_fields.get(3).is_some_and(|items| {
            items.iter().any(|it| {
                it.contains(u.as_str()) || candidate_names(u).iter().any(|n| it.contains(n))
            })
        });
        if !sent && !filled {
            gaps_open.push((sh.clone(), u.clone()));
        }
    }
    // Promotion is a LABEL (no consumer acts on it yet; REPAIR owns what is left): parse ran and
    // passed on every final file, every declared export is DEFINED with its declared signature,
    // no gap open or out, no REFERENCED drop (the final file calls what it lost — a load-time
    // failure; S14-2), and the merger wrote its MERGE.md. MILD: `referenced` also matches a
    // comment mention, which can only WITHHOLD the label, never refuse the task.
    let promoted = parse_errors.is_empty()
        && unchecked.is_empty()
        && declared_missing.is_empty()
        && signature_mismatch.is_empty()
        && gaps_open.is_empty()
        && gaps_sent.is_empty()
        && !dropped.iter().any(|(_, _, referenced)| *referenced)
        && merge_readme.is_some()
        && !dossier.files.iter().any(|f| !root.join(f).is_file());
    MergeCheck {
        parse_errors,
        unchecked,
        declared_present,
        declared_missing,
        signature_mismatch,
        dropped,
        gaps_open,
        merge_readme_present: merge_readme.is_some(),
        promoted,
    }
}

/// Field-line parser shared by the shard README (`README_FIELDS`) and the merge README
/// (`MERGE_FIELDS`): `FIELD: item`, bullets/indented continuations belong to the field above,
/// `none` is the empty list said aloud. None when no field line exists.
pub(super) fn parse_fields(text: &str, fields: &[&str]) -> Option<Vec<Vec<String>>> {
    let mut lists: Vec<Vec<String>> = vec![Vec::new(); fields.len()];
    let field_of = |line: &str| -> Option<(usize, String)> {
        let t = line
            .trim()
            .trim_start_matches(['#', '*', '-', '>', '•'])
            .trim()
            .trim_matches('`')
            .trim_matches('*');
        for (i, f) in fields.iter().enumerate() {
            let Some((head, tail)) = t.split_at_checked(f.len()) else {
                continue;
            };
            if head.eq_ignore_ascii_case(f) {
                let rest = tail.trim_start_matches(['*', '`']).trim_start();
                if let Some(r) = rest.strip_prefix(':') {
                    return Some((i, r.trim().to_string()));
                }
                if rest.is_empty() {
                    return Some((i, String::new()));
                }
            }
        }
        None
    };
    let push = |lists: &mut Vec<Vec<String>>, i: usize, item: &str| {
        let item = item
            .trim()
            .trim_start_matches(['-', '*', '•'])
            .trim()
            .replace('`', "");
        let item = item.trim();
        if item.is_empty() || item.eq_ignore_ascii_case("none") || item == "-" {
            return;
        }
        if !lists[i].iter().any(|x| x == item) {
            lists[i].push(item.to_string());
        }
    };
    let mut current: Option<usize> = None;
    let mut seen = false;
    for line in text.lines() {
        if let Some((i, rest)) = field_of(line) {
            seen = true;
            current = Some(i);
            push(&mut lists, i, &rest);
            continue;
        }
        let Some(i) = current else {
            continue;
        };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let is_item = line.starts_with(' ')
            || line.starts_with('\t')
            || t.starts_with('-')
            || t.starts_with('*')
            || t.starts_with('•');
        if is_item {
            push(&mut lists, i, t);
        } else {
            current = None;
        }
    }
    seen.then_some(lists)
}

impl GooseAgentDispatcher {
    /// At the merger's dispatch: CODE builds the dossier over the shards' folders, says what it
    /// found (`merge_dossier`), and returns the numbered brief with the module's own brief behind.
    pub(super) async fn merger_dispatch_brief(
        &self,
        req: &goose_swarm::DispatchRequest,
        merger: &MergerOf,
        root: &std::path::Path,
        lang: super::TargetLang,
    ) -> String {
        let dossier = build_merge_dossier(root, merger, &req.owned_files, lang).await;
        let mut ev = dossier.summary_json();
        if let Some(o) = ev.as_object_mut() {
            o.insert("event".into(), "merge_dossier".into());
            o.insert("task_id".into(), req.task_id.clone().into());
        }
        self.events.write_value(ev);
        for unbacked in dossier.provides_unbacked_events(&req.task_id) {
            self.events.write_value(unbacked);
        }
        for writers in shared_state_writer_events(
            &merger.module,
            &dossier.shared_state_writers,
            "readme",
            Some(&req.task_id),
        ) {
            self.events.write_value(writers);
        }
        for leak in dossier.interface_leak_events(&req.task_id) {
            self.events.write_value(leak);
        }
        // ASSEMBLE, THEN GLUE (DESIGN-SPLIT-V2 §1): code places every piece's definitions in the
        // declared order before the model runs, and the brief names the glue as the merger's job.
        // Unavailable — no parser for the module's extension, no piece of its language — is SAID
        // and the v1 brief stands; nothing is faked.
        let outcome = assembly::assemble(root, &dossier);
        let assembled = match &outcome {
            assembly::AssemblyOutcome::Assembled(a) => {
                for dup in assembly::duplicate_events(&merger.module, &req.task_id, a) {
                    self.events.write_value(dup);
                }
                self.events
                    .write_value(assembly::assembled_event(&merger.module, &req.task_id, a));
                Some(a.as_ref())
            }
            assembly::AssemblyOutcome::Unavailable { ext, reason } => {
                self.events.write_value(assembly::unavailable_event(
                    &merger.module,
                    &req.task_id,
                    ext,
                    reason,
                ));
                None
            }
        };
        format!(
            "{}{}",
            dossier.merger_brief(&root.display().to_string(), assembled),
            req.description
        )
    }

    /// At the merger's completion: gaps it sent out become follow-up shard specs (the scheduler
    /// splices them and calls the merger back); otherwise CODE checks the final file — parse,
    /// conformance to the declaration, pieces dropped without a stated reason, unfinished items
    /// neither filled nor sent — and reports (`merge_checked`, `merge_piece_dropped`,
    /// `merge_gap_open`, `merge_promoted`). MILD: the task completes either way.
    pub(super) async fn record_merge_result(
        &self,
        req: &goose_swarm::DispatchRequest,
        merger: &MergerOf,
        root: &std::path::Path,
        lang: super::TargetLang,
        final_text: &str,
    ) -> Vec<goose_swarm::TaskSpec> {
        let dossier = build_merge_dossier(root, merger, &req.owned_files, lang).await;
        let gaps = parse_merge_gaps(final_text);
        let follow_ups = gap_specs(merger, &req.owned_files, &req.description, &dossier, &gaps);
        // Split v2 §5: a gap a README already listed UNFINISHED was predictable before the merge.
        for (item, shard, unfinished) in predictable_gaps(&dossier, &gaps) {
            self.events.write_value(serde_json::json!({
                "event": "merge_gap_predictable",
                "module": merger.module,
                "task_id": req.task_id,
                "item": item,
                "shard": shard,
                "unfinished": unfinished,
            }));
        }
        // The DOOR emits `merge_gap` once it has validated and spliced the shard; this is the
        // REQUEST as the merger phrased it (S12-F: one event per fact, never two for one).
        for (spec, gap) in follow_ups.iter().zip(gaps.iter()) {
            self.events.write_value(serde_json::json!({
                "event": "merge_gap_requested",
                "module": merger.module,
                "shard": spec.id,
                "task_id": req.task_id,
                "missing": gap,
                "folder": spec.shard_of.as_ref().map(|s| s.folder.clone()),
            }));
        }
        let check = check_merge(root, &dossier, lang, &gaps).await;
        for (shard, symbol, referenced) in &check.dropped {
            self.events.write_value(serde_json::json!({
                "event": "merge_piece_dropped",
                "module": merger.module,
                "task_id": req.task_id,
                "shard": shard,
                "symbol": symbol,
                "referenced": referenced,
            }));
        }
        for (symbol, declared, found) in &check.signature_mismatch {
            self.events.write_value(serde_json::json!({
                "event": "merge_signature_mismatch",
                "module": merger.module,
                "task_id": req.task_id,
                "symbol": symbol,
                "declared": declared,
                "found": found,
            }));
        }
        for (shard, item) in &check.gaps_open {
            self.events.write_value(serde_json::json!({
                "event": "merge_gap_open",
                "module": merger.module,
                "task_id": req.task_id,
                "shard": shard,
                "item": item,
            }));
        }
        self.events.write_value(serde_json::json!({
            "event": "merge_checked",
            "module": merger.module,
            "task_id": req.task_id,
            "files": req.owned_files,
            "parse_errors": check.parse_errors.iter().map(|(f, e)| serde_json::json!({"file": f, "error": e})).collect::<Vec<_>>(),
            "parse": if check.unchecked.is_empty() { "checked" } else { "unchecked" },
            "unchecked": check.unchecked.iter().map(|(f, e)| serde_json::json!({"file": f, "why": e})).collect::<Vec<_>>(),
            "declared_present": check.declared_present,
            "declared_missing": check.declared_missing,
            "signature_mismatch": check.signature_mismatch.len(),
            "dropped": check.dropped.len(),
            "dropped_referenced": check.dropped.iter().filter(|(_, _, r)| *r).count(),
            "gaps_open": check.gaps_open.len(),
            "gaps_sent": gaps,
            "merge_readme_present": check.merge_readme_present,
            "promoted": check.promoted,
        }));
        if check.promoted {
            self.events.write_value(serde_json::json!({
                "event": "merge_promoted",
                "module": merger.module,
                "task_id": req.task_id,
                "files": req.owned_files,
            }));
        }
        follow_ups
    }
}

#[cfg(test)]
mod merger_tests {
    use super::*;
    use goose_swarm::DeclaredExport;

    fn viz_interface() -> ModuleInterface {
        ModuleInterface {
            exports: vec![
                DeclaredExport {
                    name: "window.vs7dbg.pick".into(),
                    kind: "function".into(),
                    signature: "pick(sx, sy) -> {id, index} | null".into(),
                    purpose: "pick".into(),
                },
                DeclaredExport {
                    name: "buildScene".into(),
                    kind: "function".into(),
                    signature: "buildScene(data) -> void".into(),
                    purpose: "fill buffers".into(),
                },
                DeclaredExport {
                    name: "drawBrush".into(),
                    kind: "function".into(),
                    signature: "drawBrush(ids) -> void".into(),
                    purpose: "dim non-members".into(),
                },
            ],
            shared_state: "S = {yaw, pitch, distance, brush: Set<id>}".into(),
            layout: vec![
                "constants".into(),
                "state".into(),
                "render".into(),
                "pick".into(),
                "api".into(),
            ],
        }
    }

    /// VA-100, on r6g's `.swarm/shards/viz-engine/labels-brush/updateLabels.js` (its lines 10-16,
    /// 18, 40, 46-49 verbatim, plus one indented real arrow and one dotted call value): the
    /// indented `const candIds = new Set(cands.map((n) => rec.ids[n]))` is a LOCAL holding a value
    /// that merely contains an arrow inside a call — "contains `=>`" recorded it as a Function
    /// `candIds`, a false duplicate whenever two shards share an inner name. The arrow must be the
    /// rhs's OWN: the module's real definitions — the four constants, the two state names, the two
    /// functions — are found once each, `cands` / `candIds` / `placed` / `shown` and the dotted
    /// `window.viz.pickLabel = new Set(…)` are not, and an indented REAL arrow (an IIFE-wrapped
    /// module) still is.
    #[test]
    fn a_value_that_merely_contains_an_arrow_inside_a_call_is_not_a_function_definition() {
        let js = "const LABEL_W = 110; // CSS px, border-box\nconst LABEL_H = 18;  // CSS px, border-box\nconst LABEL_DX = 10; // rect top-left = (A.sx + 10, A.sy - 9)\nconst LABEL_DY = -9;\n\nlet labelHost = null;        // #viz-labels element (absolutely positioned over the canvas)\nconst labelEls = new Map();  // record id -> persistent .viz-label element (reused across frames)\n\nfunction ensureLabelEl(id) {\n  return labelEls.get(id);\n}\n\nfunction updateLabels() {\n  labelHost = host;\n\n  const cands = labelCandidates(); // priority order: a_major DESC, id ASC\n  const candIds = new Set(cands.map((n) => rec.ids[n]));\n  const placed = [];               // rects of already-shown labels this pass\n  const shown = new Set();\n  const byId = (n) => rec.ids[n];\n  window.viz.pickLabel = new Set(cands.map((n) => n));\n}\n";
        let syms = extract_symbols(js, super::super::TargetLang::TypeScript);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "LABEL_W",
                "LABEL_H",
                "LABEL_DX",
                "LABEL_DY",
                "labelHost",
                "labelEls",
                "ensureLabelEl",
                "updateLabels",
                "byId"
            ],
            "{syms:?}"
        );
        let kind_of = |n: &str| &syms.iter().find(|s| s.name == n).unwrap().kind;
        assert_eq!(
            kind_of("byId"),
            &SymbolKind::Function,
            "an indented REAL arrow is still a function (IIFE-wrapped modules)"
        );
        assert_eq!(
            kind_of("labelEls"),
            &SymbolKind::State,
            "`new Map()` at top level is state, never a function"
        );
        assert_eq!(kind_of("LABEL_W"), &SymbolKind::Constant);
        assert_eq!(kind_of("ensureLabelEl"), &SymbolKind::Function);
        // The rule itself, on the shapes that decide it.
        assert!(rhs_is_function("(v, lo, hi) => v;"));
        assert!(rhs_is_function("n => rec.ids[n];"));
        assert!(rhs_is_function("async (x) => x;"));
        assert!(rhs_is_function("function (dt) {};"));
        assert!(!rhs_is_function("new Set(cands.map((n) => rec.ids[n]));"));
        assert!(!rhs_is_function("useCallback((a) => a, []);"));
        assert!(!rhs_is_function("110; // CSS px"));
    }

    /// The archived r6c viz.js (39,519 bytes) carries FIVE names defined twice — hoisted empty
    /// stubs at lines 98-102 redefined later (`buildScene`, `clearBrush`, `ensureSized`,
    /// `updateLabels`, `uploadSlotFloat`); the extractor finds function heads, arrow consts,
    /// dotted assignments and object methods, once each per source, so a dossier over shards
    /// that each define one of them names the duplicate.
    #[test]
    fn the_extractor_finds_every_definition_shape_once() {
        let js = "  function ensureSized() {}\n  function updateLabels() {}\n  const clamp = (v, lo, hi) => v;\n  let render = function (dt) {};\n  window.vs7dbg.pick = function(sx, sy) { return null; };\n  window.vs7dbg = {\n    layout() { return L; },\n    sceneDigest: function() {},\n    camera: () => S,\n  };\n  function ensureSized() { real(); }\n  if (x) { y(); }\n  for (let i = 0; i < n; i++) {}\n";
        let syms = extract_symbols(js, super::super::TargetLang::TypeScript);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "ensureSized",
                "updateLabels",
                "clamp",
                "render",
                "window.vs7dbg.pick",
                "layout",
                "sceneDigest",
                "camera"
            ],
            "{syms:?}"
        );
        assert_eq!(
            syms.iter()
                .find(|s| s.name == "window.vs7dbg.pick")
                .unwrap()
                .params,
            "sx, sy"
        );
        assert_eq!(
            syms.iter().find(|s| s.name == "clamp").unwrap().params,
            "v, lo, hi"
        );
        let py = "class Store:\n    def __init__(self, path):\n        pass\n\nasync def fetch_page(cursor=None):\n    pass\ndef helper(a, b):\n    return a\n";
        let syms = extract_symbols(py, super::super::TargetLang::Python);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Store", "__init__", "fetch_page", "helper"]);
        assert!(same_symbol("window.vs7dbg.pick", "pick"));
        assert!(!same_symbol("window.vs7dbg.pick", "pickPixel"));
        assert_eq!(
            normalize_params("sx: number, sy = 0, ...rest"),
            vec!["sx", "sy", "rest"]
        );
        // r6h: `applyBatch(batch: {batch: number, records: object[]})` is ONE parameter — the
        // comma inside the object type split it and manufactured `merge_signature_mismatch`.
        assert_eq!(
            normalize_params("batch: {batch: number, records: object[]}"),
            vec!["batch"]
        );
        assert_eq!(
            normalize_params("cb: (ids: string[]) => void, opts: Map<string, number>"),
            vec!["cb", "opts"]
        );
    }

    const R6G_README: &str = include_str!("shards/fixtures/r6g_labels_brush/README.md");
    const R6G_PIECES: [(&str, &str); 5] = [
        (
            "brushApi.js",
            include_str!("shards/fixtures/r6g_labels_brush/brushApi.js"),
        ),
        (
            "brushState.js",
            include_str!("shards/fixtures/r6g_labels_brush/brushState.js"),
        ),
        (
            "formatAmount.js",
            include_str!("shards/fixtures/r6g_labels_brush/formatAmount.js"),
        ),
        (
            "labelCandidates.js",
            include_str!("shards/fixtures/r6g_labels_brush/labelCandidates.js"),
        ),
        (
            "updateLabels.js",
            include_str!("shards/fixtures/r6g_labels_brush/updateLabels.js"),
        ),
    ];

    fn r6g_dossier(readme: &str) -> ShardDossier {
        ShardDossier {
            id: "viz-engine-labels-brush".into(),
            folder: ".swarm/shards/viz-engine/labels-brush".into(),
            readme_present: true,
            note: parse_shard_note(readme),
            pieces: R6G_PIECES
                .iter()
                .map(|(n, src)| {
                    (
                        format!(".swarm/shards/viz-engine/labels-brush/{n}"),
                        None,
                        extract_symbols(src, super::super::TargetLang::TypeScript),
                    )
                })
                .collect(),
            wrote_final: Vec::new(),
            provides_unbacked: Vec::new(),
        }
    }

    /// VA-097, on r6g's REAL labels-brush shard (five `.js` pieces + README, verbatim in
    /// `shards/fixtures/r6g_labels_brush/`): the function-only rule read 6 of its 13 PROVIDES as
    /// unbacked — `LABEL_W`, `brushSet`, `uBrushActive`, `dimFlags`, `brushCallbacks`,
    /// `window.vs7` — and the glue brief told the merger to write each itself beside "retyping a
    /// definition is FORBIDDEN". Module-level state, constants and installed names ARE
    /// definitions; a local inside a body is not; a name no piece defines stays unbacked.
    #[test]
    fn r6g_labels_brush_every_provides_is_backed_and_a_missing_name_stays_unbacked() {
        let d = r6g_dossier(R6G_README);
        let note = d.note.as_ref().expect("the README parses");
        assert_eq!(note.provides.len(), 13, "{:?}", note.provides);
        let defined: Vec<(&str, SymbolKind)> =
            d.defines().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(
            d.unbacked_provides(),
            Vec::<String>::new(),
            "defined: {defined:?}"
        );
        for (name, kind) in [
            ("brushSet", SymbolKind::State),
            ("uBrushActive", SymbolKind::State),
            ("dimFlags", SymbolKind::State),
            ("brushCallbacks", SymbolKind::State),
            ("labelHost", SymbolKind::State),
            ("labelEls", SymbolKind::State),
            ("LABEL_W", SymbolKind::Constant),
            ("LABEL_DY", SymbolKind::Constant),
            ("window.vs7", SymbolKind::State),
            ("toggleBrush", SymbolKind::Function),
            ("formatAmount", SymbolKind::Function),
        ] {
            assert!(defined.contains(&(name, kind)), "{name}: {defined:?}");
        }
        let toggle = d.defines().find(|s| s.name == "toggleBrush").unwrap();
        assert_eq!(toggle.params, "id");
        // A promise no piece keeps is still a promise.
        let with_gap = format!("{R6G_README}PROVIDES: drawBrush(ids) — dims non-members\n");
        let unbacked = r6g_dossier(&with_gap).unbacked_provides();
        assert_eq!(unbacked.len(), 1, "{unbacked:?}");
        assert!(unbacked[0].starts_with("drawBrush"), "{unbacked:?}");
        // Locals inside a body are not what the module provides — indentation decides.
        let local = extract_symbols(
            "function f() {\n  const local = 1;\n  let acc = [];\n  S.dirty = true;\n}\nS.ready = true;\nclass Store {}\n",
            super::super::TargetLang::TypeScript,
        );
        let names: Vec<(&str, SymbolKind)> =
            local.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("f", SymbolKind::Function),
                ("S.ready", SymbolKind::State),
                ("Store", SymbolKind::Class)
            ],
            "{local:?}"
        );
        let py = extract_symbols(
            "PORT = 8000\napp = Flask(__name__)\nDEBUG: bool = False\nif PORT == 8000:\n    x = 1\ndef run():\n    y = 2\n",
            super::super::TargetLang::Python,
        );
        let names: Vec<(&str, SymbolKind)> = py.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("PORT", SymbolKind::Constant),
                ("app", SymbolKind::State),
                ("DEBUG", SymbolKind::Constant),
                ("run", SymbolKind::Function)
            ],
            "{py:?}"
        );
    }

    /// The same rule through the dossier, the assembly and the after-check on r6g's shard: a
    /// declaration listing its state and constants has no false `declared_missing` (only the
    /// truly absent `drawBrush`), the assembly places `brushSet`/`dimFlags` as DEFINITIONS in
    /// the interface's order, and a final file holding the pieces drops nothing.
    #[tokio::test]
    async fn r6g_labels_brush_state_and_constants_are_definitions_through_every_reader() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let folder = ".swarm/shards/viz-engine/labels-brush";
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("README.md"), R6G_README).unwrap();
        let mut final_text = String::new();
        for (n, src) in R6G_PIECES {
            std::fs::write(root.join(folder).join(n), src).unwrap();
            final_text.push_str(src);
            final_text.push('\n');
        }
        let export = |name: &str, kind: &str, signature: &str| DeclaredExport {
            name: name.into(),
            kind: kind.into(),
            signature: signature.into(),
            purpose: "r6g".into(),
        };
        let merger = MergerOf {
            module: "viz-engine".into(),
            shards: vec!["viz-engine-labels-brush".into()],
            folders: vec![folder.into()],
            interface: ModuleInterface {
                exports: vec![
                    export("brushSet", "constant", "brushSet: Set<string>"),
                    export("dimFlags", "constant", "dimFlags: Uint8Array"),
                    export("uBrushActive", "state", "uBrushActive: number"),
                    export("LABEL_W", "constant", "LABEL_W: number"),
                    export("toggleBrush", "function", "toggleBrush(id) -> void"),
                    export(
                        "formatAmount",
                        "function",
                        "formatAmount(amountMinor, currency) -> string",
                    ),
                    export(
                        "window.vs7",
                        "object",
                        "window.vs7 = {toggleBrush, onBrushChange}",
                    ),
                    export("drawBrush", "function", "drawBrush(ids) -> void"),
                ],
                shared_state: String::new(),
                layout: Vec::new(),
            },
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(d.declared_missing, vec!["drawBrush".to_string()]);
        assert_eq!(d.shards[0].provides_unbacked, Vec::<String>::new());
        assert!(d.duplicates.is_empty(), "{:?}", d.duplicates);
        match assembly::assemble(root, &d) {
            assembly::AssemblyOutcome::Assembled(a) => {
                assert_eq!(a.declared_missing, vec!["drawBrush".to_string()]);
                assert_eq!(a.ordered_by_interface, 7, "{a:?}");
                let text = std::fs::read_to_string(root.join(&a.path)).unwrap();
                let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle}"));
                assert!(at("const brushSet = new Set()") < at("const dimFlags = new Uint8Array"));
                assert!(at("const dimFlags = new Uint8Array") < at("function toggleBrush(id)"));
                assert!(text.contains("— brushSet\n"), "{text}");
            }
            other => panic!("{other:?}"),
        }
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/viz.js"), &final_text).unwrap();
        let check = check_merge(root, &d, super::super::TargetLang::TypeScript, &[]).await;
        assert_eq!(check.declared_missing, vec!["drawBrush".to_string()]);
        assert_eq!(
            check.declared_present.len(),
            7,
            "{:?}",
            check.declared_present
        );
        assert!(check.dropped.is_empty(), "{:?}", check.dropped);
        let brief = d.merger_brief("/run", None);
        assert!(brief.contains("`brushSet` (state)"), "{brief}");
        assert!(brief.contains("`LABEL_W` (constant)"), "{brief}");
    }

    /// r6c's web-viz as S1 splits it (render / pick-camera / labels-brush-api), with the pieces the
    /// archived viz.js would have produced: `buildScene` in two shards (the archive's hoisted stub
    /// and its real body), `pick` defined with `(x, y)` against the declared `(sx, sy)`, the
    /// labels shard ASSUMING `scene.points` is an array nobody provides, `drawBrush` declared and
    /// written by nobody, one UNFINISHED item. The dossier names each; the brief numbers each; the
    /// after-check on an assembled file reports what was dropped and what stayed open.
    #[tokio::test]
    async fn the_dossier_names_duplicates_disagreements_unmet_assumptions_and_missing_exports() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/render", &[
            ("render.js", "function buildScene(data) {}\nfunction render() {}\nfunction initGL() {}\n"),
            ("README.md", "PROVIDES: buildScene(data)\n- render()\n- initGL()\nASSUMES: S.dirty is a bool\nUNFINISHED: none\nCHECKED_WITH: node --check render.js\n"),
        ]);
        mk(".swarm/shards/web-viz/pick-camera", &[
            ("pick.js", "function readPickAt(sx, sy) {}\nwindow.vs7dbg.pick = function(x, y) { return readPickAt(x, y); };\nfunction buildScene(data) { /* stub */ }\n"),
            ("README.md", "PROVIDES: readPickAt(sx, sy); window.vs7dbg.pick(x, y)\nASSUMES: buildScene fills geoCPU\nUNFINISHED: inertia coast stop\nCHECKED_WITH: node --check pick.js\n"),
        ]);
        mk(".swarm/shards/web-viz/labels-brush-api", &[
            ("labels.js", "function updateLabels() {}\n"),
            ("README.md", "PROVIDES: updateLabels()\nASSUMES: `scene.points` is an array\nUNFINISHED: none\nCHECKED_WITH: node --check labels.js\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec![
                "web-viz-render".into(),
                "web-viz-pick-camera".into(),
                "web-viz-labels-brush-api".into(),
            ],
            folders: vec![
                ".swarm/shards/web-viz/render".into(),
                ".swarm/shards/web-viz/pick-camera".into(),
                ".swarm/shards/web-viz/labels-brush-api".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(
            d.duplicates,
            vec![(
                "buildScene".to_string(),
                vec![
                    "web-viz-render".to_string(),
                    "web-viz-pick-camera".to_string()
                ]
            )]
        );
        assert_eq!(d.declared_missing, vec!["drawBrush".to_string()]);
        assert_eq!(
            d.signature_disagreements.len(),
            1,
            "{:?}",
            d.signature_disagreements
        );
        assert_eq!(d.signature_disagreements[0].0, "window.vs7dbg.pick");
        assert_eq!(d.signature_disagreements[0].2, "x, y");
        assert_eq!(
            d.assumptions_unmet,
            vec![(
                "web-viz-labels-brush-api".to_string(),
                "scene.points is an array".to_string()
            )]
        );
        assert_eq!(
            d.unfinished,
            vec![(
                "web-viz-pick-camera".to_string(),
                "inertia coast stop".to_string()
            )]
        );
        assert!(d.prior_merge.is_none());
        let brief = d.merger_brief("/run", None);
        assert!(brief.contains("1. `buildScene` is defined in shards `web-viz-render` and `web-viz-pick-camera` — keep ONE"), "{brief}");
        assert!(brief.contains("2. `window.vs7dbg.pick` is declared `pick(sx, sy) -> {id, index} | null` but shard `web-viz-pick-camera` defines it with `(x, y)`"), "{brief}");
        assert!(brief.contains("3. shard `web-viz-labels-brush-api` ASSUMES \"scene.points is an array\" and no shard provides it"), "{brief}");
        assert!(
            brief.contains("4. `drawBrush` is DECLARED and defined by no shard"),
            "{brief}"
        );
        assert!(
            brief
                .contains("5. shard `web-viz-pick-camera` left UNFINISHED: \"inertia coast stop\""),
            "{brief}"
        );
        assert!(brief.contains("6. ASSEMBLE `web/viz.js` in the declared order — constants → state → render → pick → api"), "{brief}");
        assert!(brief.contains("ASSEMBLE, DON'T RETYPE"), "{brief}");
        assert!(brief.contains("MERGE_GAP: <what is missing"), "{brief}");
        assert!(
            !brief.to_lowercase().contains("merge the module"),
            "never the generic task"
        );
        let summary = d.summary_json();
        assert_eq!(summary["pieces"], 3);
        assert_eq!(summary["duplicates"][0]["symbol"], "buildScene");

        // The merger assembled a file that keeps render's buildScene, drops initGL WITHOUT saying
        // so, writes pick with the declared params, never writes drawBrush, fills nothing, sends
        // the inertia item out.
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/viz.js"), "function buildScene(data) {}\nfunction render() {}\nfunction readPickAt(sx, sy) {}\nwindow.vs7dbg = { pick(sx, sy) { return readPickAt(sx, sy); } };\nfunction updateLabels() {}\n").unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/MERGE.md"), "KEPT: render's buildScene (the real body)\nDROPPED: none\nFILLED: none\nSENT_OUT: inertia coast stop\n").unwrap();
        let gaps = parse_merge_gaps("Merged.\n\nMERGE_GAP: inertia coast stop — camera coast must stop under 2 deg/s (spec §8 Camera)\nMERGE_GAP: drawBrush(ids) — dim non-members to 0.30\n");
        assert_eq!(gaps.len(), 2);
        let check = check_merge(root, &d, super::super::TargetLang::TypeScript, &gaps).await;
        assert!(
            check
                .declared_present
                .contains(&"window.vs7dbg.pick".to_string())
                && check.declared_present.contains(&"buildScene".to_string()),
            "{check:?}"
        );
        assert_eq!(check.declared_missing, vec!["drawBrush".to_string()]);
        assert_eq!(
            check.dropped,
            vec![("web-viz-render".to_string(), "initGL".to_string(), false)],
            "an unexplained drop is named; the explained duplicate is not: {:?}",
            check.dropped
        );
        assert!(
            check.gaps_open.is_empty(),
            "the unfinished item was sent out: {:?}",
            check.gaps_open
        );
        assert!(
            !check.promoted,
            "a declared export is missing and gaps are out"
        );
        let specs = gap_specs(&merger, &files, "web-viz brief", &d, &gaps);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].id, "web-viz-gap-4");
        assert_eq!(
            specs[0].owned_files,
            vec![".swarm/shards/web-viz/gap-4/README.md"]
        );
        assert!(specs[0].deps.is_empty());
        let sh = specs[0].shard_of.as_ref().unwrap();
        assert_eq!(sh.module, "web-viz");
        assert_eq!(sh.folder, ".swarm/shards/web-viz/gap-4");
        assert!(specs[0]
            .description
            .contains("MERGE GAP — the merger of `web-viz`"));
        assert!(specs[0].description.contains("inertia coast stop"));
        assert!(
            specs[0]
                .description
                .contains("`window.vs7dbg.pick` (function): `pick(sx, sy)"),
            "the declaration rides"
        );
        assert!(
            specs[1].description.contains("`gap-4`: MERGE GAP"),
            "the gap siblings know each other"
        );
        // A second pass sees the prior merge README and the assembled file's symbols.
        let d2 =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert!(d2.prior_merge.is_some());
        assert!(d2.merger_brief("/run", None).contains("SECOND PASS"));
        assert!(d2
            .final_file_symbols
            .iter()
            .any(|s| s.name == "updateLabels"));
    }

    /// S12-A/B (the refuter's measured trivialization): a final file that CALLS `initGL()` without
    /// defining it, mentions `drawBrush` in a comment only, and has no MERGE.md must NOT read as
    /// conforming or promoted — conformance is a DEFINITION, a referenced-but-undefined piece
    /// symbol is the worst drop, MERGE.md is required; a `.rs` module with no Cargo.toml is
    /// UNCHECKED, never "parses".
    #[tokio::test]
    async fn a_dangling_call_a_comment_mention_and_no_merge_readme_never_promote() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/render")).unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/render/render.js"),
            "function buildScene(data) {}\nfunction initGL() {}\n",
        )
        .unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/render/README.md"), "PROVIDES: buildScene(data)\n- initGL()\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check\n").unwrap();
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into()],
            folders: vec![".swarm/shards/web-viz/render".into()],
            interface: ModuleInterface {
                exports: vec![
                    DeclaredExport {
                        name: "window.vs7dbg.pick".into(),
                        kind: "function".into(),
                        signature: "pick(sx, sy) -> {id, index} | null".into(),
                        purpose: "pick".into(),
                    },
                    DeclaredExport {
                        name: "drawBrush".into(),
                        kind: "function".into(),
                        signature: "drawBrush(ids) -> void".into(),
                        purpose: "dim".into(),
                    },
                    DeclaredExport {
                        name: "buildScene".into(),
                        kind: "function".into(),
                        signature: "buildScene(data) -> void".into(),
                        purpose: "fill".into(),
                    },
                ],
                shared_state: String::new(),
                layout: Vec::new(),
            },
        };
        let files = vec!["web/viz.js".to_string()];
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/viz.js"), "function buildScene(data, extra) { initGL(); }\n// TODO drawBrush\nwindow.vs7dbg = {\n  pick,\n};\nfunction pick(x, y) { return null; }\n").unwrap();
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        let check = check_merge(root, &d, super::super::TargetLang::TypeScript, &[]).await;
        assert_eq!(
            check.declared_missing,
            vec!["drawBrush".to_string()],
            "a comment mention is not a definition: {check:?}"
        );
        assert!(
            check
                .declared_present
                .contains(&"window.vs7dbg.pick".to_string()),
            "the shorthand property + the function define it: {check:?}"
        );
        assert!(check.declared_present.contains(&"buildScene".to_string()));
        assert_eq!(
            check.signature_mismatch.len(),
            2,
            "buildScene(data, extra) vs (data); pick(x, y) vs (sx, sy): {:?}",
            check.signature_mismatch
        );
        assert_eq!(
            check.dropped,
            vec![("web-viz-render".to_string(), "initGL".to_string(), true)],
            "a dangling call is the referenced drop: {:?}",
            check.dropped
        );
        assert!(!check.merge_readme_present);
        assert!(!check.promoted);
        assert!(
            check.unchecked.is_empty() && check.parse_errors.is_empty(),
            "{check:?}"
        );
        // A module in a language with no per-file parser is UNCHECKED, said, and never promotes.
        let rs = vec!["src/lib.rs".to_string()];
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn broken( {").unwrap();
        let merger_rs = MergerOf {
            module: "lib".into(),
            shards: vec![],
            folders: vec![],
            interface: ModuleInterface::default(),
        };
        let d_rs = build_merge_dossier(root, &merger_rs, &rs, super::super::TargetLang::Rust).await;
        let check_rs = check_merge(root, &d_rs, super::super::TargetLang::Rust, &[]).await;
        assert_eq!(check_rs.unchecked.len(), 1, "{check_rs:?}");
        assert!(
            check_rs.unchecked[0]
                .1
                .contains("unchecked (rs) — cargo check did not run: no Cargo.toml"),
            "{check_rs:?}"
        );
        assert!(!check_rs.promoted);
        // S14-3: cargo present but producing no verdict about the owned files (here: a manifest
        // it cannot parse) is UNCHECKED with cargo's own reason — never "checked".
        std::fs::write(root.join("Cargo.toml"), "this is not a manifest = [").unwrap();
        let check_rs = check_merge(root, &d_rs, super::super::TargetLang::Rust, &[]).await;
        assert_eq!(check_rs.unchecked.len(), 1, "{check_rs:?}");
        assert!(
            check_rs.unchecked[0]
                .1
                .contains("cargo check did not run: cargo check failed outside the owned files"),
            "{check_rs:?}"
        );
        assert!(!check_rs.promoted);
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();
        assert_eq!(
            parse_piece(std::path::Path::new("x.txt")).await.as_deref(),
            Some("unchecked (txt) — no per-file parser")
        );
        // The dossier says an unreadable piece is unreadable, never "parses — no definitions".
        std::fs::write(
            root.join(".swarm/shards/web-viz/render/bad.js"),
            [0xff, 0xfe, 0xfd],
        )
        .unwrap();
        let d2 =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        let bad = d2.shards[0]
            .pieces
            .iter()
            .find(|(p, _, _)| p.ends_with("bad.js"))
            .unwrap();
        assert!(
            bad.1
                .as_deref()
                .is_some_and(|v| v.starts_with("unreadable:")),
            "{bad:?}"
        );
    }

    /// S12-E: an un-backticked ASSUMES about a function is a candidate too; S12-C: the merger's
    /// frame carries none of the author scripts' retype phrases.
    #[test]
    fn candidate_names_read_both_forms_and_the_merger_frame_never_orders_a_retype() {
        assert_eq!(
            candidate_names("drawBrush(ids) dims non-members"),
            vec!["drawBrush"]
        );
        assert_eq!(
            candidate_names("`scene.points` is an array"),
            vec!["scene.points"]
        );
        assert_eq!(
            candidate_names("S.dirty is cleared by render()"),
            vec!["S.dirty", "render"]
        );
        assert!(candidate_names("the brush stays").is_empty());
        let body = merger_owner_body();
        for banned in [
            "IN FULL from the spec",
            "NEVER `cat`",
            "nothing else",
            "write it now, in one `write`",
        ] {
            assert!(
                !body.contains(banned),
                "the merger frame must not say: {banned}"
            );
        }
        assert!(body.contains("retype the module from memory"));
        assert!(MERGER_READING_RULE.contains("READ EVERY PIECE FOLDER AND README"));
        // The frame in run_task_inner takes the merger arms (source tripwire: the prompt is
        // assembled inside the dispatcher and has no seam a unit test can render).
        let src = include_str!("../swarm.rs");
        assert!(src.contains("} else if req.merger_of.is_some() {\n                    shards::merger_owner_body()"), "owner_body's merger arm");
        assert!(
            src.contains("&& req.merger_of.is_none()\n            && !req.owned_files.is_empty()"),
            "the ACT-NOW nudge skips the merger"
        );
        assert!(
            src.contains("shards::MERGER_READING_RULE"),
            "the merger's reading rule"
        );
        // extract_symbols: shorthand properties inside an object literal are MENTIONS of the
        // exported names (S14-1) — recorded, flagged, and outranked by a real definition; the
        // top-level `window.vs7dbg = {…}` and `const x = {…}` that hold them are STATE (VA-097).
        let syms = extract_symbols(
            "window.vs7dbg = {\n  pick,\n  brush, layout\n};\nconst x = { if, y };\nfunction brush(ids) {}\n",
            super::super::TargetLang::TypeScript,
        );
        let names: Vec<(&str, bool)> = syms
            .iter()
            .map(|s| (s.name.as_str(), s.shorthand))
            .collect();
        assert_eq!(
            names,
            vec![
                ("window.vs7dbg", false),
                ("pick", true),
                ("brush", false),
                ("layout", true),
                ("x", false)
            ],
            "{syms:?}"
        );
        assert_eq!(syms[0].kind, SymbolKind::State);
        assert_eq!(syms[2].params, "ids");
    }

    /// S14-1/2 (the S12 refuter's residual holes). A multi-line export object `{ pick,\n drawBrush,\n }`
    /// MENTIONS `drawBrush`; with no definition anywhere the file throws at load, so it stays
    /// `declared_missing` and never promotes (the one-line form already failed correctly). And a
    /// REFERENCED drop — `initGL` dropped, still called, MERGE.md silent — withholds `promoted` even
    /// when every export is defined; a DROPPED line for it and no call restores the label.
    #[tokio::test]
    async fn a_shorthand_mention_is_not_a_definition_and_a_referenced_drop_never_promotes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/render")).unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/render/render.js"),
            "function pick(sx, sy) { return null; }\nfunction initGL() {}\nfunction drawBrush(ids) {}\n",
        )
        .unwrap();
        std::fs::write(root.join(".swarm/shards/web-viz/render/README.md"), "PROVIDES: pick(sx, sy)\n- initGL()\n- drawBrush(ids)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check\n").unwrap();
        let export = |name: &str, sig: &str| DeclaredExport {
            name: name.into(),
            kind: "function".into(),
            signature: sig.into(),
            purpose: name.into(),
        };
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into()],
            folders: vec![".swarm/shards/web-viz/render".into()],
            interface: ModuleInterface {
                exports: vec![
                    export("window.vs7dbg.pick", "pick(sx, sy) -> {id} | null"),
                    export("drawBrush", "drawBrush(ids) -> void"),
                ],
                shared_state: String::new(),
                layout: Vec::new(),
            },
        };
        let files = vec!["web/viz.js".to_string()];
        let merge_md = root.join(".swarm/shards/web-viz/MERGE.md");
        std::fs::write(
            &merge_md,
            "KEPT: render\nDROPPED: none\nFILLED: none\nSENT_OUT: none\n",
        )
        .unwrap();
        let lang = super::super::TargetLang::TypeScript;

        std::fs::write(root.join("web/viz.js"), "function pick(sx, sy) { return null; }\nwindow.vs7dbg = {\n  pick,\n  drawBrush,\n};\n").unwrap();
        let d = build_merge_dossier(root, &merger, &files, lang).await;
        let check = check_merge(root, &d, lang, &[]).await;
        assert_eq!(
            check.declared_missing,
            vec!["drawBrush".to_string()],
            "a multi-line shorthand is a mention: {check:?}"
        );
        assert!(check
            .declared_present
            .contains(&"window.vs7dbg.pick".to_string()));
        assert!(!check.promoted, "{check:?}");
        // the dossier side: the shard DEFINES drawBrush (a function), the final file only names it
        assert!(d.shards[0].defines().any(|s| s.name == "drawBrush"));

        std::fs::write(root.join("web/viz.js"), "function pick(sx, sy) { initGL(); return null; }\nfunction drawBrush(ids) {}\nwindow.vs7dbg = {\n  pick,\n  drawBrush,\n};\n").unwrap();
        let check = check_merge(root, &d, lang, &[]).await;
        assert!(check.declared_missing.is_empty(), "{check:?}");
        assert_eq!(
            check.dropped,
            vec![("web-viz-render".to_string(), "initGL".to_string(), true)]
        );
        assert!(
            !check.promoted,
            "a referenced drop never promotes: {check:?}"
        );

        std::fs::write(root.join("web/viz.js"), "function pick(sx, sy) { return null; }\nfunction drawBrush(ids) {}\nwindow.vs7dbg = {\n  pick,\n  drawBrush,\n};\n").unwrap();
        std::fs::write(&merge_md, "KEPT: render\nDROPPED: initGL (dead — WebGL context is created in boot)\nFILLED: none\nSENT_OUT: none\n").unwrap();
        let check = check_merge(root, &d, lang, &[]).await;
        assert!(check.dropped.is_empty(), "{check:?}");
        assert!(check.promoted, "{check:?}");
    }

    #[test]
    fn merge_fields_and_gap_lines_parse_in_markdown_dress() {
        let f = parse_fields("## KEPT\n- render's buildScene\n**DROPPED:** `initGL` (dead)\nFILLED: none\nSENT_OUT: drawBrush\n", &MERGE_FIELDS).unwrap();
        assert_eq!(f[0], vec!["render's buildScene"]);
        assert_eq!(f[1], vec!["initGL (dead)"]);
        assert!(f[2].is_empty());
        assert_eq!(f[3], vec!["drawBrush"]);
        assert!(parse_fields("nothing here", &MERGE_FIELDS).is_none());
        // Non-ASCII prefixes never panic (byte-index safety) and never match a field.
        assert!(parse_fields("Résumé: done\nVoilà — done\n", &MERGE_FIELDS).is_none());
        assert!(parse_shard_note("Résumé: done").is_none());
        assert!(parse_fields("PROVIDÉS: x", &MERGE_FIELDS).is_none());
        assert_eq!(
            parse_merge_gaps("- `MERGE_GAP: a`\nMERGE_GAP: none\n**MERGE_GAP:** a\nmerge_gap: b"),
            vec!["a", "b"]
        );
        assert!(parse_merge_gaps("MERGE_GAP:").is_empty());
    }

    /// S10(3): a shard that writes the merger's file directly is said at its completion
    /// (`final_files_written` over `snapshot_final_files`), rides its ledger row as `wrote_final`,
    /// and the merger's dossier lists the file as ONE MORE PIECE — attributed when a shard's row
    /// names the write, "not recorded" when no row does, and not at all on the merger's own
    /// second pass (its file is expected then).
    #[tokio::test]
    async fn a_shard_that_wrote_the_final_file_is_named_to_the_merger() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let shard = ShardOf {
            module: "web-viz".into(),
            shard: "render".into(),
            folder: ".swarm/shards/web-viz/render".into(),
            responsibility: "programs".into(),
            interface: viz_interface(),
            module_files: vec!["web/viz.js".into()],
        };
        let before = snapshot_final_files(root, Some(&shard));
        assert_eq!(before, vec![("web/viz.js".to_string(), None)]);
        assert!(final_files_written(root, &before).is_empty());
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/viz.js"), "function render() {}\n").unwrap();
        assert_eq!(
            final_files_written(root, &before),
            vec!["web/viz.js".to_string()]
        );
        let after = snapshot_final_files(root, Some(&shard));
        assert!(
            final_files_written(root, &after).is_empty(),
            "unchanged since the snapshot"
        );
        assert!(snapshot_final_files(root, None).is_empty());

        let ledger = root.join(super::super::LEDGER_DIR);
        std::fs::create_dir_all(&ledger).unwrap();
        std::fs::write(
            ledger.join(format!(
                "{}.json",
                super::super::activity_digest_key("web-viz-render")
            )),
            serde_json::json!({"wrote_final": ["web/viz.js"]}).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/render")).unwrap();
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into()],
            folders: vec![".swarm/shards/web-viz/render".into()],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(
            d.final_on_disk,
            vec![(
                "web/viz.js".to_string(),
                21,
                Some("web-viz-render".to_string())
            )]
        );
        let brief = d.merger_brief("/cwd", None);
        assert!(
            brief.contains("shard `web-viz-render` wrote `web/viz.js` DIRECTLY (21 bytes)"),
            "{brief}"
        );
        assert!(
            brief.contains(
                "`/cwd/web/viz.js` already exists (21 bytes; written by shard `web-viz-render`)"
            ),
            "{brief}"
        );
        assert!(!brief.contains("NOBODY has written"), "{brief}");
        assert_eq!(
            d.summary_json()["final_on_disk"][0]["written_by_shard"],
            "web-viz-render"
        );

        std::fs::remove_dir_all(&ledger).unwrap();
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(d.final_on_disk, vec![("web/viz.js".to_string(), 21, None)]);
        assert!(d
            .merger_brief("/cwd", None)
            .contains("no shard's ledger row claims it"));

        std::fs::write(
            root.join(".swarm/shards/web-viz/MERGE.md"),
            "KEPT: render\nDROPPED: none\nFILLED: none\nSENT_OUT: none\n",
        )
        .unwrap();
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert!(d.final_on_disk.is_empty(), "{:?}", d.final_on_disk);
        assert!(d.merger_brief("/cwd", None).contains("SECOND PASS"));
        assert!(d.merger_brief("/cwd", None).contains("NOBODY has written"));
    }

    /// DESIGN-SPLIT-V2 §1: with pieces on disk, CODE assembles them first and the brief's job is
    /// the GLUE — it names the assembled file, forbids retyping a definition, lists the measured glue
    /// classes and never orders a `cat` of the piece folders; the duplicate item points at the
    /// `MERGE_DUPLICATE` markers. Without an assembly (`None`) the v1 shell-assembly item stands. The
    /// dossier is built under a Python-target run (r6e's seam: `web/viz.js` in a Python run) and the
    /// `.js` pieces still yield their definitions — the piece's extension decides its language.
    #[tokio::test]
    async fn an_assembled_module_briefs_the_merger_for_glue_not_retyping() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/render", &[
            ("render.js", "const S = { yaw: 0 };\nfunction buildScene(data) {}\n"),
            ("README.md", "PROVIDES: buildScene(data)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check render.js\n"),
        ]);
        mk(".swarm/shards/web-viz/pick", &[
            ("pick.js", "function buildScene(data) { /* stub */ }\nwindow.vs7dbg.pick = function (sx, sy) { return null; };\n"),
            ("README.md", "PROVIDES: window.vs7dbg.pick(sx, sy)\nASSUMES: none\nUNFINISHED: inertia coast stop\nCHECKED_WITH: node --check pick.js\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into(), "web-viz-pick".into()],
            folders: vec![
                ".swarm/shards/web-viz/render".into(),
                ".swarm/shards/web-viz/pick".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d = build_merge_dossier(root, &merger, &files, super::super::TargetLang::Python).await;
        assert!(
            d.shards[0].defines().any(|s| s.name == "buildScene"),
            "a .js piece under a Python-target run still yields its definitions: {:?}",
            d.shards[0].pieces
        );
        assert_eq!(
            d.duplicates,
            vec![(
                "buildScene".to_string(),
                vec!["web-viz-render".to_string(), "web-viz-pick".to_string()]
            )]
        );
        let assembly::AssemblyOutcome::Assembled(a) = assembly::assemble(root, &d) else {
            panic!("two js pieces assemble");
        };
        assert_eq!(a.declared_missing, vec!["drawBrush".to_string()]);
        assert_eq!(
            a.glue_needed,
            vec![
                "shared_state_init",
                "wiring",
                "duplicates",
                "gaps",
                "unfinished"
            ]
        );
        let brief = d.merger_brief("/run", Some(&a));
        assert!(
            brief.contains("CODE HAS ALREADY ASSEMBLED their definitions into `/run/.swarm/shards/web-viz/ASSEMBLED.js`: 4 definition block(s) from 2 piece(s) — 3 placed in the declared interface's order, 1 appended after it"),
            "{brief}"
        );
        assert!(
            brief.contains("YOUR JOB IS THE GLUE, NOT THE DEFINITIONS"),
            "{brief}"
        );
        assert!(
            brief.contains("copying or retyping a definition is FORBIDDEN and is a defect"),
            "{brief}"
        );
        assert!(
            brief.contains("1. `buildScene` is defined in shards `web-viz-render` and `web-viz-pick` — BOTH definitions are in the assembled file under `MERGE_DUPLICATE` markers; keep ONE"),
            "{brief}"
        );
        assert!(
            brief.contains("WRITE THE GLUE into `web/viz.js`, starting from `/run/.swarm/shards/web-viz/ASSEMBLED.js` — never from memory."),
            "{brief}"
        );
        assert!(
            brief.contains("Glue the engine measured as needed: shared_state_init, wiring, duplicates, gaps, unfinished."),
            "{brief}"
        );
        assert!(
            !brief.contains("ASSEMBLE, DON'T RETYPE") && !brief.contains("`cat <piece>"),
            "the v1 cat-the-pieces item is gone once code assembled: {brief}"
        );
        let v1 = d.merger_brief("/run", None);
        assert!(v1.contains("ASSEMBLE, DON'T RETYPE"), "{v1}");
        assert!(!v1.contains("CODE HAS ALREADY ASSEMBLED"), "{v1}");
        let ev = assembly::assembled_event("web-viz", "web-viz", &a);
        assert_eq!(ev["path"], ".swarm/shards/web-viz/ASSEMBLED.js");
        assert_eq!(ev["duplicates"][0]["name"], "buildScene");
        assert!(merger_missing_hint(&merger, &files).contains("ASSEMBLED.<ext>"));
        assert!(merger_owner_body().contains("START FROM IT"));
    }

    /// DESIGN-SPLIT-V2 §3, PROVIDES must be BACKED: `render` defines `buildScene` and promises
    /// `initGL()` and `drawBrush(ids)` it never wrote (r6e's shape in miniature — its eight shards
    /// promised in READMEs and delivered no piece). The dossier names the two promises per shard,
    /// the declared `drawBrush` is MISSING (a claim no longer stands in for the code), a sibling's
    /// ASSUMES about `initGL` is UNMET (a promise provides nothing), the brief lists the promises
    /// under GAPS and not under PROVIDES, the dispatch says them by name, and the assembly's glue
    /// list carries `unbacked_provides`.
    #[tokio::test]
    async fn a_readme_provides_without_a_definition_is_unbacked_and_briefed_as_a_gap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/render", &[
            ("render.js", "function buildScene(data) {}\n"),
            ("README.md", "PROVIDES: buildScene(data)\n- initGL()\n- drawBrush(ids)\n- the render loop\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check render.js\n"),
        ]);
        mk(".swarm/shards/web-viz/labels", &[
            ("labels.js", "function updateLabels() {}\n"),
            ("README.md", "PROVIDES: updateLabels()\nASSUMES: initGL() has run before updateLabels\nUNFINISHED: none\nCHECKED_WITH: node --check labels.js\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into(), "web-viz-labels".into()],
            folders: vec![
                ".swarm/shards/web-viz/render".into(),
                ".swarm/shards/web-viz/labels".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(
            d.shards[0].provides_unbacked,
            vec!["initGL()", "drawBrush(ids)", "the render loop"],
            "{:?}",
            d.shards[0]
        );
        assert!(d.shards[1].provides_unbacked.is_empty());
        assert!(
            d.declared_missing.contains(&"drawBrush".to_string()),
            "a promised export is missing, not provided: {:?}",
            d.declared_missing
        );
        assert_eq!(
            d.assumptions_unmet,
            vec![(
                "web-viz-labels".to_string(),
                "initGL() has run before updateLabels".to_string()
            )],
            "a promise meets no assumption"
        );
        let events = d.provides_unbacked_events("web-viz");
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "shard_provides_unbacked");
        assert_eq!(events[0]["module"], "web-viz");
        assert_eq!(events[0]["shard"], "web-viz-render");
        assert_eq!(
            events[0]["names"],
            serde_json::json!(["initGL()", "drawBrush(ids)", "the render loop"])
        );
        assert_eq!(
            d.summary_json()["provides_unbacked"][0]["shard"],
            "web-viz-render"
        );
        let brief = d.merger_brief("/run", None);
        assert!(
            brief.contains("  PROVIDES (each backed by a definition above): buildScene(data)\n"),
            "{brief}"
        );
        assert!(
            brief.contains("  PROVIDES WITHOUT A DEFINITION (promises — GAPS below, not deliveries): initGL(); drawBrush(ids); the render loop\n"),
            "{brief}"
        );
        assert!(
            brief.contains("shard `web-viz-render`'s README PROVIDES `initGL()`, `drawBrush(ids)`, `the render loop` but no piece in `/run/.swarm/shards/web-viz/render` DEFINES them — promises, not deliveries: they are GAPS."),
            "{brief}"
        );
        let gap_item = brief.find("'s README PROVIDES").unwrap();
        let missing_item = brief
            .find("`drawBrush` is DECLARED and defined by no shard")
            .unwrap();
        assert!(missing_item < gap_item, "promises follow the declared gaps");
        let assembly::AssemblyOutcome::Assembled(a) = assembly::assemble(root, &d) else {
            panic!("js assembles");
        };
        assert!(
            a.glue_needed.contains(&"unbacked_provides".to_string()),
            "{:?}",
            a.glue_needed
        );
    }

    /// VA-085: a shard that delivered its README and NO piece file is not a piece. THE PIECES
    /// lists the shards that built (s1, s3) and nothing of s2; s2's promises, ASSUMES, UNFINISHED
    /// and WRITES raise no numbered item (its whole part is the gap), so the ONE place the merger
    /// reads its name is the dispatch paragraph swarm.rs appends last (`merge_holes::gap_paragraph`)
    /// — the brief and the assembly agree on which pieces exist. The loud channels stay:
    /// `merge_dossier.pieces_absent` names s2 and `shard_provides_unbacked` still fires for it.
    #[tokio::test]
    async fn a_readme_only_shard_is_named_once_as_a_gap_and_never_as_a_piece() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/s1", &[
            ("s1.js", "function buildScene(data) {}\n"),
            ("README.md", "PROVIDES: buildScene(data)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check s1.js\nWRITES: S.brush — Set<id>\n"),
        ]);
        mk(".swarm/shards/web-viz/s2", &[
            ("README.md", "PROVIDES: drawBrush(ids)\n- initGL()\nASSUMES: initGL() has run before the overlay draws\nUNFINISHED: the brush overlay\nCHECKED_WITH: none\nWRITES: S.brush: the brushed ids\n"),
        ]);
        mk(".swarm/shards/web-viz/s3", &[
            ("s3.js", "function updateLabels() {}\n"),
            ("README.md", "PROVIDES: updateLabels()\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check s3.js\nWRITES: none\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec![
                "web-viz-s1".into(),
                "web-viz-s2".into(),
                "web-viz-s3".into(),
            ],
            folders: vec![
                ".swarm/shards/web-viz/s1".into(),
                ".swarm/shards/web-viz/s2".into(),
                ".swarm/shards/web-viz/s3".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        // The dossier keeps every fact of s2 — the skipping is the brief's, never the measurement's.
        assert_eq!(
            d.shards[1].provides_unbacked,
            vec!["drawBrush(ids)", "initGL()"]
        );
        assert_eq!(
            d.assumptions_unmet,
            vec![(
                "web-viz-s2".to_string(),
                "initGL() has run before the overlay draws".to_string()
            )]
        );
        assert_eq!(
            d.unfinished,
            vec![("web-viz-s2".to_string(), "the brush overlay".to_string())]
        );
        assert_eq!(
            d.shared_state_writers,
            vec![(
                "S.brush".to_string(),
                vec!["web-viz-s1".to_string(), "web-viz-s2".to_string()]
            )]
        );
        let summary = d.summary_json();
        assert_eq!(summary["pieces"], 2);
        assert_eq!(summary["pieces_absent"], serde_json::json!(["web-viz-s2"]));
        assert_eq!(summary["readmes_missing"], serde_json::json!([]));
        assert_eq!(
            d.provides_unbacked_events("web-viz")[0]["shard"],
            "web-viz-s2"
        );

        let brief = d.merger_brief("/run", None);
        let pieces_start = brief.find("THE PIECES (path").unwrap();
        let pieces_end = brief.find("THE DECLARED INTERFACE").unwrap();
        let pieces = &brief[pieces_start..pieces_end];
        assert!(
            pieces.contains("shard `web-viz-s1` — folder `/run/.swarm/shards/web-viz/s1`:\n  - `/run/.swarm/shards/web-viz/s1/s1.js` — "),
            "{pieces}"
        );
        assert!(pieces.contains("shard `web-viz-s3` — folder"), "{pieces}");
        assert!(
            !pieces.contains("s2"),
            "a README-only shard is not a piece: {pieces}"
        );
        assert!(
            !brief.contains("delivered nothing but its README"),
            "{brief}"
        );
        assert!(
            brief.contains("; NOBODY has written `web/viz.js`. 1 more shard delivered NO piece file — nothing of it is in THE PIECES below; CODE names it by id and folder under the dispatch GAPS at the end of this brief. You write `web/viz.js` from their pieces"),
            "{brief}"
        );
        assert!(
            brief.contains("`web-viz`. 2 shards built its pieces"),
            "{brief}"
        );
        assert!(
            !brief.contains("web-viz-s2"),
            "no numbered item names a shard with no code: {brief}"
        );
        assert!(
            !brief.contains("shared state `S.brush` is WRITTEN"),
            "one writer with code is no conflict: {brief}"
        );
        assert!(
            brief.contains("`drawBrush` is DECLARED and defined by no shard"),
            "the declared gap stands on its own: {brief}"
        );

        // The brief as swarm.rs composes it — the dossier's list, then the dispatch paragraph —
        // names s2 exactly once, there.
        let states = super::super::merge_holes::shard_folder_states(root, &merger);
        let paragraph = super::super::merge_holes::gap_paragraph(&merger.module, &states).unwrap();
        assert!(
            paragraph.contains("shard `web-viz-s2` (folder `.swarm/shards/web-viz/s2`): README present but NO piece files"),
            "{paragraph}"
        );
        let composed = format!("{brief}{paragraph}");
        assert_eq!(composed.matches("`web-viz-s2`").count(), 1, "{composed}");
    }

    /// VA-085, the other no-piece shape: a bare folder (no README, no piece) is likewise out of
    /// THE PIECES and raises no "left no README — derive what it provides from its pieces" item
    /// (there are none). It is `readmes_missing`, not `pieces_absent` — the classing
    /// `merge_holes::dispatch_gaps` uses — and the dispatch paragraph names it once.
    #[tokio::test]
    async fn a_bare_shard_folder_is_readmes_missing_not_pieces_absent_and_never_a_piece() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/s1")).unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/s1/s1.js"),
            "function buildScene(data) {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".swarm/shards/web-viz/s1/README.md"),
            "PROVIDES: buildScene(data)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check s1.js\nWRITES: none\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".swarm/shards/web-viz/s2")).unwrap();
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-s1".into(), "web-viz-s2".into()],
            folders: vec![
                ".swarm/shards/web-viz/s1".into(),
                ".swarm/shards/web-viz/s2".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        let summary = d.summary_json();
        assert_eq!(summary["pieces_absent"], serde_json::json!([]));
        assert_eq!(
            summary["readmes_missing"],
            serde_json::json!(["web-viz-s2"])
        );
        let brief = d.merger_brief("/run", None);
        assert!(
            brief.contains("`web-viz`. 1 shards built its pieces"),
            "{brief}"
        );
        assert!(!brief.contains("web-viz-s2"), "{brief}");
        assert!(!brief.contains("left no README"), "{brief}");
        let states = super::super::merge_holes::shard_folder_states(root, &merger);
        let paragraph = super::super::merge_holes::gap_paragraph(&merger.module, &states).unwrap();
        assert!(
            paragraph.contains("shard `web-viz-s2` (folder `.swarm/shards/web-viz/s2`): its README.md handoff is MISSING — and the folder holds NO piece files; nothing of its part exists."),
            "{paragraph}"
        );
        assert_eq!(
            format!("{brief}{paragraph}")
                .matches("`web-viz-s2`")
                .count(),
            1
        );
    }

    /// Split v2 §4 at the merger's dispatch: two READMEs WRITE `S.brush` → the dossier names the
    /// state and both shards, the brief carries a numbered reconcile item, the summary and the
    /// `readme`-sourced event say the same; a lone writer says nothing.
    #[tokio::test]
    async fn two_readme_writers_of_one_state_reach_the_dossier_and_the_brief() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/render", &[
            ("render.js", "function buildScene(data) {}\n"),
            ("README.md", "PROVIDES: buildScene(data)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check render.js\nWRITES: S.brush — Set<id>\n- instanceData: Float32Array stride 8\n"),
        ]);
        mk(".swarm/shards/web-viz/brush", &[
            ("brush.js", "function drawBrush(ids) {}\n"),
            ("README.md", "PROVIDES: drawBrush(ids)\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: node --check brush.js\nWRITES: S.brush: the brushed ids\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-render".into(), "web-viz-brush".into()],
            folders: vec![
                ".swarm/shards/web-viz/render".into(),
                ".swarm/shards/web-viz/brush".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(
            d.shared_state_writers,
            vec![(
                "S.brush".to_string(),
                vec!["web-viz-render".to_string(), "web-viz-brush".to_string()]
            )]
        );
        assert_eq!(
            d.summary_json()["shared_state_writers"][0]["state"],
            "S.brush"
        );
        let brief = d.merger_brief("/run", None);
        assert!(
            brief.contains("shared state `S.brush` is WRITTEN by shards `web-viz-render` and `web-viz-brush` (their READMEs' WRITES) — the declaration names ONE writer per state"),
            "{brief}"
        );
        let events = shared_state_writer_events(
            &merger.module,
            &d.shared_state_writers,
            "readme",
            Some("web-viz"),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["source"], "readme");
        std::fs::write(
            root.join(".swarm/shards/web-viz/brush/README.md"),
            "PROVIDES: drawBrush(ids)\nASSUMES: S.brush is a Set<id>\nUNFINISHED: none\nCHECKED_WITH: node --check brush.js\nWRITES: none\n",
        )
        .unwrap();
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert!(
            d.shared_state_writers.is_empty(),
            "{:?}",
            d.shared_state_writers
        );
        assert!(!d
            .merger_brief("/run", None)
            .contains("is WRITTEN by shards"));
    }

    /// Split v2 §5, the interface-fidelity instruments. `labels` ASSUMES four things: `S.dirty`
    /// (the declared shared state's root — covered), "buildScene fills geoCPU" (a declared export
    /// named in prose — covered), "scene.points is an array" (no export, no declared root — a
    /// LEAK even though `pick` defines `points`: coordination outside the declaration, which the
    /// unmet-assumptions check cannot see) and "the canvas is square" (a prose leak with no
    /// names). The gap the merger sends out for `pick`'s UNFINISHED "inertia coast stop" was
    /// PREDICTABLE — the README said it; `drawBrush(ids)` was discovered at the merge.
    ///
    /// `assumptions_unmet` (VA-108, per NAME against the shards' DEFINITIONS) sees the dossier
    /// differently from the leak check: `scene.points` is met by pick's `points()`, "the canvas is
    /// square" is prose, and `buildScene` is UNBACKED — the interface declares it but no shard in
    /// this dossier defines it, and a declaration meets nothing (the r6h `gl` class).
    #[tokio::test]
    async fn an_assumption_outside_the_declaration_leaks_and_an_unfinished_gap_is_predictable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |folder: &str, files: &[(&str, &str)]| {
            let d = root.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            for (n, c) in files {
                std::fs::write(d.join(n), c).unwrap();
            }
        };
        mk(".swarm/shards/web-viz/pick", &[
            ("pick.js", "function readPickAt(sx, sy) {}\nfunction points() {}\n"),
            ("README.md", "PROVIDES: readPickAt(sx, sy)\n- points()\nASSUMES: none\nUNFINISHED: inertia coast stop\nCHECKED_WITH: node --check pick.js\n"),
        ]);
        mk(".swarm/shards/web-viz/labels", &[
            ("labels.js", "function updateLabels() {}\n"),
            ("README.md", "PROVIDES: updateLabels()\nASSUMES: S.dirty is cleared by the render loop\n- buildScene fills geoCPU before labels run\n- scene.points is an array\n- the canvas is square\nUNFINISHED: none\nCHECKED_WITH: node --check labels.js\n"),
        ]);
        let merger = MergerOf {
            module: "web-viz".into(),
            shards: vec!["web-viz-pick".into(), "web-viz-labels".into()],
            folders: vec![
                ".swarm/shards/web-viz/pick".into(),
                ".swarm/shards/web-viz/labels".into(),
            ],
            interface: viz_interface(),
        };
        let files = vec!["web/viz.js".to_string()];
        let d =
            build_merge_dossier(root, &merger, &files, super::super::TargetLang::TypeScript).await;
        assert_eq!(
            d.interface_leaks,
            vec![
                (
                    "web-viz-labels".to_string(),
                    "scene.points is an array".to_string(),
                    vec!["scene.points".to_string()]
                ),
                (
                    "web-viz-labels".to_string(),
                    "the canvas is square".to_string(),
                    vec![]
                ),
            ],
            "{:?}",
            d.interface_leaks
        );
        assert_eq!(
            d.assumptions_unmet,
            vec![(
                "web-viz-labels".to_string(),
                "buildScene fills geoCPU before labels run".to_string()
            )],
            "`scene.points` is met by pick's `points()`; `buildScene` is defined by no shard"
        );
        assert_eq!(
            d.assumptions_unbacked,
            vec![super::assumes::AssumeUnbacked {
                shard: "web-viz-labels".into(),
                name: "buildScene".into(),
                clause: "buildScene fills geoCPU before labels run".into(),
                nearest: None,
            }],
            "no sibling name is close to `buildScene` — nothing is invented"
        );
        assert_eq!(
            d.assumptions_prose,
            vec![(
                "web-viz-labels".to_string(),
                "the canvas is square".to_string()
            )]
        );
        let events = d.interface_leak_events("web-viz");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "interface_leak");
        assert_eq!(events[0]["module"], "web-viz");
        assert_eq!(events[0]["shard"], "web-viz-labels");
        assert_eq!(events[0]["assumption"], "scene.points is an array");
        assert_eq!(events[0]["names"], serde_json::json!(["scene.points"]));
        assert_eq!(events[1]["names"], serde_json::json!([]));
        assert_eq!(
            d.summary_json()["interface_leaks"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let gaps = parse_merge_gaps(
            "Merged.\nMERGE_GAP: inertia coast stop — the coast must stop under 2 deg/s\nMERGE_GAP: drawBrush(ids) — dim non-members to 0.30\n",
        );
        assert_eq!(gaps.len(), 2);
        assert_eq!(
            predictable_gaps(&d, &gaps),
            vec![(
                "inertia coast stop — the coast must stop under 2 deg/s".to_string(),
                "web-viz-pick".to_string(),
                "inertia coast stop".to_string()
            )]
        );
        assert!(gap_covers_unfinished(
            "drawBrush(ids) — dim non-members",
            "drawBrush the non-members"
        ));
        assert!(!gap_covers_unfinished(
            "drawBrush(ids) — dim non-members to 0.30",
            "inertia coast stop"
        ));
    }
}
