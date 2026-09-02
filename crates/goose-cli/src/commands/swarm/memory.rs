//! VA-127: cross-run BUILD memory — facts a run measured about ITSELF, stored under the shape of
//! the tree it built, offered to the next build of the same shape. Not for the benchmark: under
//! `benchmark == true` every door here returns empty and says `memory_off{reason: benchmark}`, so a
//! measured run is byte-identical to a run without this module (r6h, 393a99351: every run.jsonl
//! row this module reads existed — `plan_repaired` ×2, `finding_flipped` ×2, `shard_promoted` ×2,
//! `merge_dossier` ×1 with three `assumptions_unmet` — and none of this module's rows was written).
//!
//! WHY THE LAST ATTEMPT DIED (EXPERIMENTS-LEDGER.md "LEARN & REFLECT", deleted 2026-09-01 as
//! VA-016): it harvested `judge_verdict`, an event no run ever emitted (`lessons: 0` on both runs
//! that wrote a skill); it keyed the stack from a spec ADJECTIVE (`angular` for a Python app); its
//! one load preceded a 0.1420; nothing measured a consumer. Each of those is a rule here:
//!
//! - HARVEST reads only rows the engine writes today, each cited at its emitter (`harvest`).
//! - The KEY is derived from the TREE and the vendor probe, never from the spec's words
//!   (`MemoryKey::derive`, `vendor_shape`).
//! - Every fact is one sentence of MEASURED evidence with its provenance (`run_id`, `event`, `ts`,
//!   the quoted text) — never advice, never a template (gates 1 and 2).
//! - The CONSUMER is the measurement (gate 9): `MemoryStore::render_for` is the only door that
//!   renders; it bumps `offered_by` (the runs a fact was loaded for) and `consumed` (renders into a
//!   prompt) on disk and says `memory_read{key, consumer, stored, facts}`; a fact offered to
//!   `min_runs` runs and rendered to nobody is dropped by `retire` (`memory_retired`).
//!
//! STORE: `<goose config dir>/swarm/memory/<lang>/<layout>/<vendor>/facts.jsonl`, one JSON object
//! per line (`Fact`). The config dir is `goose::config::paths::Paths::config_dir()` — the same
//! resolution that locates `config.yaml` (`~/.config/goose` on the dev machine, `GOOSE_PATH_ROOT`
//! in tests).
//!
//! RECALL COVERAGE: a greenfield run's tree is EMPTY at OPEN (swarm.rs `tree_at_start`), so an
//! exact-key lookup could never hit what a finished build wrote (`python/app+web/...`) — the
//! never-loaded half of VA-016. A stored key COVERS a query when lang and vendor match and the
//! query's layout is a SUBSET of the stored one; `flat` (no top-level dirs) is the empty set.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use goose::config::paths::Paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{EventSink, TargetLang};

const FACTS_FILE: &str = "facts.jsonl";
const FLAT_LAYOUT: &str = "flat";
const NO_VENDOR: &str = "no-vendor";

pub(super) const KIND_ARM_SKIPPED: &str = "arm_skipped";
pub(super) const KIND_DEP_TOO_LARGE: &str = "dep_too_large";
pub(super) const KIND_SHARD_GAP: &str = "shard_gap";
pub(super) const KIND_REPAIR_FLIP: &str = "repair_flip";
pub(super) const KIND_ANSWER_UNOWNED: &str = "answer_unowned";
pub(super) const KIND_LAYOUT_COLLISION: &str = "layout_collision";

/// Where a fact came from: the run, the event row, its timestamp and the words it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Provenance {
    pub(super) run_id: String,
    pub(super) event: String,
    pub(super) ts: Option<String>,
    pub(super) quote: String,
}

/// One measured sentence about an earlier build, with the files it names and its consumption record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Fact {
    pub(super) fact: String,
    pub(super) kind: String,
    #[serde(default)]
    pub(super) files: Vec<String>,
    pub(super) provenance: Provenance,
    pub(super) key: String,
    pub(super) written_at: String,
    /// The run ids this fact was LOADED for (one entry per run, whichever consumer loaded it).
    #[serde(default)]
    pub(super) offered_by: Vec<String>,
    /// How many times it was RENDERED into a prompt — the consumption gate 9 asks for.
    #[serde(default)]
    pub(super) consumed: u32,
}

impl Fact {
    fn identity(&self) -> (String, String, String) {
        (
            self.provenance.run_id.clone(),
            self.provenance.event.clone(),
            self.fact.clone(),
        )
    }
}

/// `<lang>/<layout>/<vendor>` — every segment derived from the run's own artefacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemoryKey {
    lang: String,
    layout: Vec<String>,
    vendor: String,
}

impl MemoryKey {
    /// `lang` is `lang::detect_language`'s verdict (its variant name, lowercased); `tree_files` are
    /// the tree-relative paths of `tree::snapshot_tree_files` — the engine's own walk, which already
    /// skips its bookkeeping dirs (`.swarm`, `.git`, `__pycache__`, …), so no second skip list lives
    /// here — whose top-level directories are the layout; `vendor` is `vendor_shape`'s string when a
    /// vendor probe ran, `None` otherwise.
    pub(super) fn derive(lang: TargetLang, tree_files: &[String], vendor: Option<&str>) -> Self {
        let layout: Vec<String> = tree_files
            .iter()
            .filter_map(|p| {
                let p = p.trim_start_matches("./");
                let (head, _) = p.split_once('/')?;
                (!head.is_empty()).then(|| segment(head))
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            lang: format!("{lang:?}").to_ascii_lowercase(),
            layout,
            vendor: match vendor {
                Some(v) => segment(v),
                None => NO_VENDOR.to_string(),
            },
        }
    }

    fn from_dirs(lang: &str, layout_dir: &str, vendor: &str) -> Self {
        let layout = if layout_dir == FLAT_LAYOUT {
            Vec::new()
        } else {
            layout_dir.split('+').map(str::to_string).collect()
        };
        Self {
            lang: lang.to_string(),
            layout,
            vendor: vendor.to_string(),
        }
    }

    fn layout_dir(&self) -> String {
        if self.layout.is_empty() {
            FLAT_LAYOUT.to_string()
        } else {
            self.layout.join("+")
        }
    }

    /// The store path under the memory root, e.g. `python/app+web/vendor-local-v3`.
    pub(super) fn path(&self) -> String {
        format!("{}/{}/{}", self.lang, self.layout_dir(), self.vendor)
    }

    /// A stored key covers a query when the language and vendor shape match and every top-level
    /// dir the query has is in the stored layout (an empty query layout — greenfield OPEN — is
    /// covered by every layout of that lang+vendor).
    fn covers(&self, query: &MemoryKey) -> bool {
        self.lang == query.lang
            && self.vendor == query.vendor
            && query.layout.iter().all(|d| self.layout.contains(d))
    }
}

/// The vendor's SHAPE from the probe (`vendor_probe.base_url` / `.endpoints`, swarm.rs
/// `probe_vendor` call): host class (`local` for a loopback host, else `remote`) and the version
/// segment every advertised endpoint shares (`/v3/payments`, `/v3/reversals` → `v3`). r6h:
/// `http://127.0.0.1:8850` + `[/v3/payments, /v3/payments?cursor=1, /v3/reversals]` →
/// `vendor-local-v3`. Never the literal host or port.
pub(super) fn vendor_shape(base_url: &str, endpoints: &[String]) -> String {
    let after_scheme = match base_url.split_once("://") {
        Some((_, rest)) => rest,
        None => base_url,
    };
    let authority = match after_scheme.split_once('/') {
        Some((a, _)) => a,
        None => after_scheme,
    };
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    let hostname = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((h, _)) => h,
            None => v6,
        },
        None => match authority.split_once(':') {
            Some((h, _)) => h,
            None => authority,
        },
    };
    let class = if matches!(hostname, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0") {
        "local"
    } else {
        "remote"
    };
    let versions: BTreeSet<&str> = endpoints
        .iter()
        .map(|e| {
            let p = e.trim_start_matches('/');
            let end = p.find(['/', '?']).unwrap_or(p.len());
            &p[..end]
        })
        .collect();
    match versions.iter().next() {
        Some(v) if versions.len() == 1 && is_version_segment(v) => format!("vendor-{class}-{v}"),
        _ => format!("vendor-{class}"),
    }
}

fn is_version_segment(s: &str) -> bool {
    matches!(s.strip_prefix('v'), Some(d) if !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// A path segment safe for the store: alphanumerics, `.`, `_`, `-`; anything else (including `+`,
/// which joins layout dirs) becomes `_`; a dot-only segment (`..`) cannot leave the store.
fn segment(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        "_".to_string()
    } else {
        cleaned
    }
}

/// Who a block is rendered for. The OPENER plans the whole tree and reads every plan-level fact
/// (everything but a repair flip, which names files the next app may not have); a WORKER reads only
/// the facts that name one of its owned files — so a repair fact about `web/viz.js` is consumed
/// exactly when a later build owns `web/viz.js`, and `retire` measures the rest.
pub(super) enum Consumer<'a> {
    Opener,
    Worker {
        task_id: &'a str,
        owned_files: &'a [String],
    },
}

impl Consumer<'_> {
    fn name(&self) -> String {
        match self {
            Consumer::Opener => "opener".to_string(),
            Consumer::Worker { task_id, .. } => format!("worker:{task_id}"),
        }
    }

    fn wants(&self, fact: &Fact) -> bool {
        match self {
            Consumer::Opener => fact.kind != KIND_REPAIR_FLIP,
            Consumer::Worker { owned_files, .. } => fact
                .files
                .iter()
                .any(|f| owned_files.iter().any(|o| same_path(o, f))),
        }
    }
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_start_matches("./").trim_end_matches('/')
        == b.trim_start_matches("./").trim_end_matches('/')
}

/// The facts a finished run measured about itself, from the rows the engine ACTUALLY writes —
/// each named here with its emitter, checked by grep on 2026-09-02:
///
/// - `lang_unsupported{arm, lang, skipped}` — lang_arms.rs:16 `event_row`, said by
///   `LangArms::python_only` (lang_arms.rs:54) from swarm.rs:12106 and :29940.
/// - `dep_source_truncated{task_id, file, bytes, kept, reason}` — dep_sources.rs:437
///   `DepSourcesBlock::cut_events`, written at swarm.rs:25106. One fact per FILE (the size is the
///   file's, not the task's).
/// - `merge_hole{module, task_id, shards_missing, readmes_missing}` — merge_holes.rs:322
///   (`dispatch_incomplete_event`, :234), written at swarm.rs:25710.
/// - `merge_dossier.assumptions_unmet[{shard, assumes}]` — shards.rs:4384 `summary_json`, named
///   `merge_dossier` at shards.rs:5189.
/// - `finding_flipped{round, shard, task_id, finding, check, fails_before, fails_after}` —
///   repair_waves.rs:1069, paired with the `shard_promoted{task_id, files}` (repair_waves.rs:477)
///   of the same task_id, which names the file the fix WROTE (r6h seq 1335/1336: a `web/viz.js`
///   finding fixed by writing `web/index.html`). `finding` rides whole — the emitter already
///   writes `finding_short`.
/// - `research_answer_unowned{from_slice, q_index, names}` — answer_routing.rs:297
///   (`route_cross_slice_answers`, :214).
/// - `plan_repaired{source, actions}` — swarm.rs:20129; only the shared-file (plan_repairs.rs:671)
///   and module-shadowed-by-package (plan_repairs.rs:805/813/825) actions are layout facts.
///
/// Any other event yields nothing. A named event missing a field it always carries is said as
/// `memory_harvest_skipped{source, missing}`, never guessed.
pub(super) fn harvest(
    events: &[Value],
    key: &MemoryKey,
    run_id: &str,
    benchmark: bool,
    sink: &dyn EventSink,
) -> Vec<Fact> {
    if benchmark {
        sink.write_value(serde_json::json!({
            "event": "memory_off",
            "reason": "benchmark",
            "at": "harvest",
        }));
        return Vec::new();
    }
    let written_at = chrono::Utc::now().to_rfc3339();
    let mut promoted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ev in events
        .iter()
        .filter(|ev| text(ev, "event") == Some("shard_promoted"))
    {
        if let Some(task) = text(ev, "task_id") {
            promoted
                .entry(task.to_string())
                .or_default()
                .extend(texts(ev, "files"));
        }
    }
    let mut out = Vec::new();
    let mut seen_cut: BTreeSet<String> = BTreeSet::new();
    for ev in events {
        let Some(name) = text(ev, "event") else {
            continue;
        };
        let drafted = match name {
            "lang_unsupported" => from_lang_unsupported(ev),
            "dep_source_truncated" => from_dep_source_truncated(ev, &mut seen_cut),
            "merge_hole" => from_merge_hole(ev),
            "merge_dossier" => from_merge_dossier(ev),
            "finding_flipped" => from_finding_flipped(ev, &promoted),
            "research_answer_unowned" => from_research_answer_unowned(ev),
            "plan_repaired" => from_plan_repaired(ev),
            _ => continue,
        };
        match drafted {
            Ok(drafts) => out.extend(
                drafts
                    .into_iter()
                    .map(|d| d.into_fact(ev, name, key, run_id, &written_at)),
            ),
            Err(missing) => sink.write_value(serde_json::json!({
                "event": "memory_harvest_skipped",
                "source": name,
                "missing": missing,
            })),
        }
    }
    out
}

struct Draft {
    kind: &'static str,
    fact: String,
    files: Vec<String>,
    quote: String,
}

impl Draft {
    fn into_fact(
        self,
        ev: &Value,
        event: &str,
        key: &MemoryKey,
        run_id: &str,
        written_at: &str,
    ) -> Fact {
        Fact {
            fact: self.fact,
            kind: self.kind.to_string(),
            files: self.files,
            provenance: Provenance {
                run_id: text(ev, "run_id").unwrap_or(run_id).to_string(),
                event: event.to_string(),
                ts: text(ev, "ts").map(str::to_string),
                quote: self.quote,
            },
            key: key.path(),
            written_at: written_at.to_string(),
            offered_by: Vec::new(),
            consumed: 0,
        }
    }
}

type Drafted = Result<Vec<Draft>, &'static str>;

fn from_lang_unsupported(ev: &Value) -> Drafted {
    let arm = text(ev, "arm").ok_or("arm")?;
    let lang = text(ev, "lang").ok_or("lang")?;
    let skipped = text(ev, "skipped").ok_or("skipped")?;
    Ok(vec![Draft {
        kind: KIND_ARM_SKIPPED,
        fact: format!(
            "the `{arm}` arm did not run on this {lang} build (it is Python-only): \"{skipped}\""
        ),
        files: Vec::new(),
        quote: skipped.to_string(),
    }])
}

fn from_dep_source_truncated(ev: &Value, seen: &mut BTreeSet<String>) -> Drafted {
    let file = text(ev, "file").ok_or("file")?;
    let task = text(ev, "task_id").ok_or("task_id")?;
    let bytes = number(ev, "bytes").ok_or("bytes")?;
    let kept = number(ev, "kept").ok_or("kept")?;
    let reason = text(ev, "reason").ok_or("reason")?;
    if !seen.insert(file.to_string()) {
        return Ok(Vec::new());
    }
    Ok(vec![Draft {
        kind: KIND_DEP_TOO_LARGE,
        fact: format!(
            "`{file}` ({bytes} B) could not ride whole in `{task}`'s brief: {kept} B were kept — \"{reason}\""
        ),
        files: vec![file.to_string()],
        quote: reason.to_string(),
    }])
}

fn from_merge_hole(ev: &Value) -> Drafted {
    let module = text(ev, "module").ok_or("module")?;
    let task = text(ev, "task_id").ok_or("task_id")?;
    let shards = ev
        .get("shards_missing")
        .and_then(Value::as_array)
        .ok_or("shards_missing")?;
    let shards: Vec<String> = shards
        .iter()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    let readmes = texts(ev, "readmes_missing");
    let readmes = if readmes.is_empty() {
        "none".to_string()
    } else {
        backticked(&readmes)
    };
    Ok(vec![Draft {
        kind: KIND_SHARD_GAP,
        fact: format!(
            "merging `{module}` ({task}) found no pieces from {} (READMEs missing: {readmes})",
            backticked(&shards)
        ),
        files: vec![module.to_string()],
        quote: shards.join(", "),
    }])
}

fn from_merge_dossier(ev: &Value) -> Drafted {
    let module = text(ev, "module").ok_or("module")?;
    let unmet = ev
        .get("assumptions_unmet")
        .and_then(Value::as_array)
        .ok_or("assumptions_unmet")?;
    let mut drafts = Vec::new();
    for entry in unmet {
        let shard = text(entry, "shard").ok_or("assumptions_unmet.shard")?;
        let assumes = text(entry, "assumes").ok_or("assumptions_unmet.assumes")?;
        drafts.push(Draft {
            kind: KIND_SHARD_GAP,
            fact: format!(
                "shard `{shard}` of `{module}` assumed \"{assumes}\" — no sibling shard provided it"
            ),
            files: vec![module.to_string()],
            quote: assumes.to_string(),
        });
    }
    Ok(drafts)
}

fn from_finding_flipped(ev: &Value, promoted: &BTreeMap<String, Vec<String>>) -> Drafted {
    let round = number(ev, "round").ok_or("round")?;
    let shard = text(ev, "shard").ok_or("shard")?;
    let task = text(ev, "task_id").ok_or("task_id")?;
    let finding = text(ev, "finding").ok_or("finding")?;
    let before = number(ev, "fails_before").ok_or("fails_before")?;
    let after = number(ev, "fails_after").ok_or("fails_after")?;
    let check = text(ev, "check").unwrap_or("no authoring check");
    let mut files = vec![shard.to_string()];
    let written = match promoted.get(task) {
        Some(w) if !w.is_empty() => {
            for f in w {
                if !files.contains(f) {
                    files.push(f.clone());
                }
            }
            backticked(w)
        }
        _ => format!("(no shard_promoted row for `{task}`)"),
    };
    Ok(vec![Draft {
        kind: KIND_REPAIR_FLIP,
        fact: format!(
            "REPAIR round {round} fixed `{shard}` by writing {written}: \"{finding}\" ({check}: {before} → {after} failing)"
        ),
        files,
        quote: finding.to_string(),
    }])
}

fn from_research_answer_unowned(ev: &Value) -> Drafted {
    let slice = text(ev, "from_slice").ok_or("from_slice")?;
    let q = number(ev, "q_index").ok_or("q_index")?;
    let names = ev.get("names").and_then(Value::as_array).ok_or("names")?;
    let names: Vec<String> = names
        .iter()
        .filter_map(|n| n.as_str().map(str::to_string))
        .collect();
    Ok(vec![Draft {
        kind: KIND_ANSWER_UNOWNED,
        fact: format!(
            "research answer q{q} of slice `{slice}` named {} — files no plan task owned",
            backticked(&names)
        ),
        quote: names.join(", "),
        files: names,
    }])
}

fn from_plan_repaired(ev: &Value) -> Drafted {
    let source = text(ev, "source").ok_or("source")?;
    let actions = ev
        .get("actions")
        .and_then(Value::as_array)
        .ok_or("actions")?;
    Ok(actions
        .iter()
        .filter_map(Value::as_str)
        .filter(|a| a.starts_with("shared file `") || a.contains("shadowed by package"))
        .map(|action| Draft {
            kind: KIND_LAYOUT_COLLISION,
            fact: format!("plan repair ({source}): {action}"),
            files: backticked_paths(action),
            quote: action.to_string(),
        })
        .collect())
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn number(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

fn texts(v: &Value, key: &str) -> Vec<String> {
    match v.get(key).and_then(Value::as_array) {
        Some(items) => items
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}

fn backticked(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The backticked tokens of an action string that look like paths (`app/ledgerd.py`,
/// `app/ledgerd/`), not the task ids beside them (`skeleton`, `ledgerd-core`).
fn backticked_paths(s: &str) -> Vec<String> {
    s.split('`')
        .skip(1)
        .step_by(2)
        .filter(|t| t.contains('/') || t.contains('.'))
        .map(str::to_string)
        .collect()
}

/// The run's event stream read back from its `run.jsonl` (the `JsonlSink` path) for `harvest`.
/// A line that does not parse is counted and said (`memory_unreadable{path, bad_lines}`), never
/// silently dropped.
pub(super) fn load_events(run_jsonl: &Path, sink: &dyn EventSink) -> Vec<Value> {
    let body = match std::fs::read_to_string(run_jsonl) {
        Ok(b) => b,
        Err(e) => {
            sink.write_value(serde_json::json!({
                "event": "memory_unreadable",
                "path": run_jsonl.display().to_string(),
                "error": e.to_string(),
            }));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut bad = 0usize;
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => out.push(v),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        sink.write_value(serde_json::json!({
            "event": "memory_unreadable",
            "path": run_jsonl.display().to_string(),
            "bad_lines": bad,
        }));
    }
    out
}

/// The on-disk store. `open(MemoryStore::default_root())` in the engine; `open(<tempdir>)` in tests.
pub(super) struct MemoryStore {
    root: PathBuf,
}

struct Stored {
    path: PathBuf,
    facts: Vec<Fact>,
    unparsed: Vec<String>,
}

impl MemoryStore {
    /// `<config dir>/swarm/memory`, by the resolution that finds `config.yaml`.
    pub(super) fn default_root() -> PathBuf {
        Paths::config_dir().join("swarm").join("memory")
    }

    pub(super) fn open(root: PathBuf) -> Self {
        Self { root }
    }

    fn facts_path(&self, key: &MemoryKey) -> PathBuf {
        self.root.join(key.path()).join(FACTS_FILE)
    }

    /// Persist a run's harvested facts under `key`. A fact already stored for the same
    /// (run_id, event, sentence) — a resumed run harvesting twice — is skipped and counted.
    /// Says `memory_written{key, facts, duplicates_skipped, path}` every time, zero included;
    /// under benchmark says `memory_off` and writes nothing.
    pub(super) fn append(
        &self,
        key: &MemoryKey,
        facts: &[Fact],
        benchmark: bool,
        sink: &dyn EventSink,
    ) -> usize {
        if benchmark {
            sink.write_value(serde_json::json!({
                "event": "memory_off",
                "reason": "benchmark",
                "at": "append",
            }));
            return 0;
        }
        let path = self.facts_path(key);
        let mut added = 0usize;
        let mut skipped = 0usize;
        if !facts.is_empty() {
            let (mut existing, unparsed) = if path.is_file() {
                load_facts(&path, sink)
            } else {
                (Vec::new(), Vec::new())
            };
            let mut known: BTreeSet<(String, String, String)> =
                existing.iter().map(Fact::identity).collect();
            for f in facts {
                if !known.insert(f.identity()) {
                    skipped += 1;
                    continue;
                }
                let mut f = f.clone();
                f.key = key.path();
                existing.push(f);
                added += 1;
            }
            if added > 0 {
                save_facts(&path, &existing, &unparsed, sink);
            }
        }
        sink.write_value(serde_json::json!({
            "event": "memory_written",
            "key": key.path(),
            "facts": added,
            "duplicates_skipped": skipped,
            "path": path.display().to_string(),
        }));
        added
    }

    /// THE CONSUMER DOOR: every stored fact under a key that covers `key` is marked offered to
    /// `run_id`; those `consumer` wants are marked consumed and rendered as the
    /// `LEARNED FROM EARLIER BUILDS` block (empty string when there is nothing to say). Says
    /// `memory_read{key, consumer, run_id, stored, facts, chars}`; under benchmark says
    /// `memory_off` and touches nothing.
    pub(super) fn render_for(
        &self,
        key: &MemoryKey,
        consumer: &Consumer<'_>,
        run_id: &str,
        benchmark: bool,
        sink: &dyn EventSink,
    ) -> String {
        if benchmark {
            sink.write_value(serde_json::json!({
                "event": "memory_off",
                "reason": "benchmark",
                "at": format!("render:{}", consumer.name()),
            }));
            return String::new();
        }
        let mut rendered: Vec<Fact> = Vec::new();
        let mut stored = 0usize;
        for mut entry in self.covering(key, sink) {
            stored += entry.facts.len();
            let mut dirty = false;
            for f in entry.facts.iter_mut() {
                if !f.offered_by.iter().any(|r| r == run_id) {
                    f.offered_by.push(run_id.to_string());
                    dirty = true;
                }
                if consumer.wants(f) {
                    f.consumed += 1;
                    dirty = true;
                    rendered.push(f.clone());
                }
            }
            if dirty {
                save_facts(&entry.path, &entry.facts, &entry.unparsed, sink);
            }
        }
        let block = render_block(&key.path(), &rendered);
        sink.write_value(serde_json::json!({
            "event": "memory_read",
            "key": key.path(),
            "consumer": consumer.name(),
            "run_id": run_id,
            "stored": stored,
            "facts": rendered.len(),
            "chars": block.chars().count(),
        }));
        block
    }

    /// Drop every fact under the covering keys that was offered to at least `min_runs` runs and
    /// rendered to no consumer — the measurement gate 9 asks for, acted on. Says
    /// `memory_retired{key, min_runs, facts, retired: [{fact, kind, run_id, offered_by}]}` when
    /// anything went; under benchmark says `memory_off` and touches nothing (a benchmark run must
    /// never rewrite what a real build stored).
    pub(super) fn retire(
        &self,
        key: &MemoryKey,
        min_runs: usize,
        benchmark: bool,
        sink: &dyn EventSink,
    ) -> usize {
        if benchmark {
            sink.write_value(serde_json::json!({
                "event": "memory_off",
                "reason": "benchmark",
                "at": "retire",
            }));
            return 0;
        }
        let mut dropped: Vec<Value> = Vec::new();
        for mut entry in self.covering(key, sink) {
            let (keep, drop): (Vec<Fact>, Vec<Fact>) = std::mem::take(&mut entry.facts)
                .into_iter()
                .partition(|f| f.consumed > 0 || f.offered_by.len() < min_runs);
            if drop.is_empty() {
                continue;
            }
            entry.facts = keep;
            dropped.extend(drop.iter().map(|f| {
                serde_json::json!({
                    "fact": f.fact,
                    "kind": f.kind,
                    "run_id": f.provenance.run_id,
                    "offered_by": f.offered_by,
                })
            }));
            save_facts(&entry.path, &entry.facts, &entry.unparsed, sink);
        }
        if !dropped.is_empty() {
            sink.write_value(serde_json::json!({
                "event": "memory_retired",
                "key": key.path(),
                "min_runs": min_runs,
                "facts": dropped.len(),
                "retired": dropped,
            }));
        }
        dropped.len()
    }

    /// Every `facts.jsonl` under `<root>/<lang>/<layout>/<vendor>` whose key covers `query`, in
    /// layout order. A missing lang dir is honestly empty (nothing was ever written); any other
    /// read failure is said as `memory_unreadable`.
    fn covering(&self, query: &MemoryKey, sink: &dyn EventSink) -> Vec<Stored> {
        let lang_dir = self.root.join(&query.lang);
        let layouts = match std::fs::read_dir(&lang_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                sink.write_value(serde_json::json!({
                    "event": "memory_unreadable",
                    "path": lang_dir.display().to_string(),
                    "error": e.to_string(),
                }));
                return Vec::new();
            }
        };
        let mut names: Vec<String> = layouts
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let mut out = Vec::new();
        for layout in names {
            let stored = MemoryKey::from_dirs(&query.lang, &layout, &query.vendor);
            if !stored.covers(query) {
                continue;
            }
            let path = self.facts_path(&stored);
            if !path.is_file() {
                continue;
            }
            let (facts, unparsed) = load_facts(&path, sink);
            out.push(Stored {
                path,
                facts,
                unparsed,
            });
        }
        out
    }
}

/// Read one `facts.jsonl`; lines that do not parse ride back as `unparsed` so a rewrite keeps
/// them, and their count is said (`memory_unreadable{path, bad_lines}`).
fn load_facts(path: &Path, sink: &dyn EventSink) -> (Vec<Fact>, Vec<String>) {
    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(e) => {
            sink.write_value(serde_json::json!({
                "event": "memory_unreadable",
                "path": path.display().to_string(),
                "error": e.to_string(),
            }));
            return (Vec::new(), Vec::new());
        }
    };
    let mut facts = Vec::new();
    let mut unparsed = Vec::new();
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Fact>(line) {
            Ok(f) => facts.push(f),
            Err(_) => unparsed.push(line.to_string()),
        }
    }
    if !unparsed.is_empty() {
        sink.write_value(serde_json::json!({
            "event": "memory_unreadable",
            "path": path.display().to_string(),
            "bad_lines": unparsed.len(),
        }));
    }
    (facts, unparsed)
}

fn save_facts(path: &Path, facts: &[Fact], unparsed: &[String], sink: &dyn EventSink) {
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut body = String::new();
        for f in facts {
            body.push_str(&serde_json::to_string(f).map_err(std::io::Error::other)?);
            body.push('\n');
        }
        for line in unparsed {
            body.push_str(line);
            body.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write() {
        sink.write_value(serde_json::json!({
            "event": "memory_write_failed",
            "path": path.display().to_string(),
            "error": e.to_string(),
        }));
    }
}

/// The prompt block: the key, the count, then one line per distinct sentence with the run(s) it
/// was measured in. Empty for no facts — a header over nothing is noise, and `memory_read` already
/// says `facts: 0`.
pub(super) fn render_block(key_label: &str, facts: &[Fact]) -> String {
    if facts.is_empty() {
        return String::new();
    }
    let mut lines: Vec<(String, String, Vec<String>)> = Vec::new();
    for f in facts {
        match lines.iter_mut().find(|(_, fact, _)| *fact == f.fact) {
            Some((_, _, runs)) => {
                if !runs.contains(&f.provenance.run_id) {
                    runs.push(f.provenance.run_id.clone());
                }
            }
            None => lines.push((
                f.kind.clone(),
                f.fact.clone(),
                vec![f.provenance.run_id.clone()],
            )),
        }
    }
    let noun = if lines.len() == 1 { "fact" } else { "facts" };
    let mut out = format!(
        "LEARNED FROM EARLIER BUILDS ({key_label}, {} {noun})\n\
         Each line is something an earlier build of this shape measured about itself, with the run \
         it happened in — evidence, not instructions:\n",
        lines.len()
    );
    for (kind, fact, runs) in &lines {
        out.push_str(&format!("- [{kind} · {}] {fact}\n", runs.join(", ")));
    }
    out
}

/// The retire floor `MemoryStore::retire` runs with after every run (`RunMemory::close_run`): a
/// fact offered to this many runs and rendered into no prompt is dropped. RECEIPT: on the sb-7
/// shape every owned file recurred across the three same-shape builds on record (r5, r6c and r6h
/// each owned `web/viz.js` and `web/index.html`), so a worker fact naming a file no three builds
/// of a shape owned names a file that shape does not have; the opener's plan-level facts render
/// on their first offer (`Consumer::wants`) and never reach the floor.
pub(super) const MEMORY_RETIRE_MIN_RUNS: usize = 3; // ratio: 3 offered runs to 0 renders

/// The run's handle on the store: opened at OPEN by `run_linear_plan` (swarm.rs, beside
/// `lang_arms`) from the tree at start, the vendor's probed shape and the run's language; read
/// at the opener's turn and at every worker dispatch (`MemoryCell::render_for`); closed after
/// `run_finished` (`MemoryCell::close_run`) under the key of the tree the run BUILT.
pub(super) struct RunMemory {
    store: MemoryStore,
    lang: TargetLang,
    vendor: Option<String>,
    key: MemoryKey,
    run_id: String,
    benchmark: bool,
}

impl RunMemory {
    pub(super) fn new(
        store: MemoryStore,
        lang: TargetLang,
        tree_at_start: &[String],
        vendor: Option<&str>,
        run_id: &str,
        benchmark: bool,
    ) -> Self {
        Self {
            key: MemoryKey::derive(lang, tree_at_start, vendor),
            store,
            lang,
            vendor: vendor.map(str::to_string),
            run_id: run_id.to_string(),
            benchmark,
        }
    }

    fn render_for(&self, consumer: &Consumer<'_>, sink: &dyn EventSink) -> String {
        self.store
            .render_for(&self.key, consumer, &self.run_id, self.benchmark, sink)
    }

    /// THE WRITE DOOR: the run's own stream read back, harvested, appended under the key of the
    /// tree it built (OPEN's lang and vendor, the FINISHED layout), then the facts
    /// `MEMORY_RETIRE_MIN_RUNS` runs were offered and none read are retired. No run log
    /// (`--no-log`) is a loud `memory_harvest_skipped{source: run_jsonl}`; under benchmark the
    /// log is not read at all and each door says `memory_off`.
    fn close_run(&self, tree_files: &[String], run_jsonl: Option<&Path>, sink: &dyn EventSink) {
        let key = MemoryKey::derive(self.lang, tree_files, self.vendor.as_deref());
        let events = match (self.benchmark, run_jsonl) {
            (true, _) => Vec::new(),
            (false, Some(p)) => load_events(p, sink),
            (false, None) => {
                sink.write_value(serde_json::json!({
                    "event": "memory_harvest_skipped",
                    "source": "run_jsonl",
                    "missing": "no run log (--no-log): nothing to read the run's own rows from",
                }));
                Vec::new()
            }
        };
        let facts = harvest(&events, &key, &self.run_id, self.benchmark, sink);
        self.store.append(&key, &facts, self.benchmark, sink);
        self.store
            .retire(&key, MEMORY_RETIRE_MIN_RUNS, self.benchmark, sink);
    }
}

/// ONE field on `GooseAgentDispatcher`, set once by `run_linear_plan`. A run that never set it
/// (a RESUME skips OPEN) renders nothing and says `memory_unset{at}` — a loud absence, never a
/// default (gate 1).
#[derive(Default)]
pub(super) struct MemoryCell(OnceLock<RunMemory>);

impl MemoryCell {
    pub(super) fn set(&self, memory: RunMemory) {
        let _ = self.0.set(memory);
    }

    /// The `LEARNED FROM EARLIER BUILDS` block for `consumer` — empty under benchmark, with
    /// nothing stored, and when no run set the cell.
    pub(super) fn render_for(&self, consumer: &Consumer<'_>, sink: &dyn EventSink) -> String {
        match self.0.get() {
            Some(m) => m.render_for(consumer, sink),
            None => {
                sink.write_value(serde_json::json!({
                    "event": "memory_unset",
                    "at": format!("render:{}", consumer.name()),
                }));
                String::new()
            }
        }
    }

    /// `RunMemory::close_run` for the run that set the cell; `memory_unset{at: close_run}` otherwise.
    pub(super) fn close_run(
        &self,
        tree_files: &[String],
        run_jsonl: Option<&Path>,
        sink: &dyn EventSink,
    ) {
        match self.0.get() {
            Some(m) => m.close_run(tree_files, run_jsonl, sink),
            None => sink.write_value(serde_json::json!({
                "event": "memory_unset",
                "at": "close_run",
            })),
        }
    }
}

/// `text` with the LEARNED block on its own paragraph after it; `text` unchanged, byte for byte,
/// when the block is empty — every benchmark run (r6k) and every run with nothing stored.
pub(super) fn with_learned(text: &str, learned: &str) -> String {
    if learned.is_empty() {
        text.to_string()
    } else {
        format!("{text}\n\n{learned}")
    }
}

/// The opener's user turn (swarm.rs `open_slices`): the request — whole, or its orientation
/// index — then the LEARNED block, then the SOURCES block (the instructions on what is research
/// material). With an empty block the bytes are exactly the pre-VA-139 turn,
/// `{request}{sources_block}`.
pub(super) fn opener_user_text(request: &str, learned: &str, sources_block: &str) -> String {
    format!("{}{sources_block}", with_learned(request, learned))
}

#[cfg(test)]
mod tests {
    use super::super::SwarmEvent;
    use super::*;
    use std::sync::Mutex;

    const RUN: &str = "swarm-20260902-002811909";

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: Value) {
            self.0.lock().unwrap().push(value);
        }
    }
    impl ValueSink {
        fn named(&self, event: &str) -> Vec<Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e["event"] == event)
                .cloned()
                .collect()
        }
    }

    fn r6h_tree() -> Vec<String> {
        [
            "app/__init__.py",
            "app/__main__.py",
            "app/ledgerd/impl.py",
            "app/notifierd/impl.py",
            "web/index.html",
            "web/viz.js",
            "README.md",
            "app/__pycache__/b.pyc",
        ]
        .map(String::from)
        .to_vec()
    }

    fn r6h_vendor() -> String {
        vendor_shape(
            "http://127.0.0.1:8850",
            &[
                "/v3/payments".to_string(),
                "/v3/payments?cursor=1".to_string(),
                "/v3/reversals".to_string(),
            ],
        )
    }

    fn facts_off_benchmark() -> Vec<Fact> {
        harvest(
            &r6h_like_stream(),
            &r6h_key(),
            RUN,
            false,
            &ValueSink::default(),
        )
    }

    fn r6h_key() -> MemoryKey {
        MemoryKey::derive(TargetLang::Python, &r6h_tree(), Some(&r6h_vendor()))
    }

    /// Rows shaped exactly like r6h's run.jsonl (393a99351), plus the events this module ignores.
    fn r6h_like_stream() -> Vec<Value> {
        vec![
            serde_json::json!({"event": "run_started", "prompt": "# Build `app`", "run_id": RUN}),
            serde_json::json!({"event": "vendor_probe", "ok": true, "base_url": "http://127.0.0.1:8850",
                "endpoints": ["/v3/payments"], "run_id": RUN}),
            serde_json::json!({"event": "plan_repaired", "source": "plan", "actions": [
                "shared file `app/__main__.py`: kept by `skeleton` (first claimant), dropped from `ledgerd-core`",
                "module `app/ledgerd.py` shadowed by package `app/ledgerd/`: rewritten to `app/ledgerd/impl.py` (kept by `ledgerd-core` — the task keeps its work at an unshadowed path; prose_rewrites: 2)",
                "`skeleton` brief regenerated from the repaired plan — its PLANNED MODULES and ownership blocks baked pre-repair paths",
            ], "run_id": RUN, "ts": "2026-09-02T01:53:32.657873+00:00"}),
            serde_json::json!({"event": "dep_source_truncated", "task_id": "viz-engine", "file": "app/ledgerd/impl.py",
                "bytes": 61240, "kept": 24000, "reason": "over the dependency-source budget", "run_id": RUN}),
            serde_json::json!({"event": "dep_source_truncated", "task_id": "console-page", "file": "app/ledgerd/impl.py",
                "bytes": 61240, "kept": 24000, "reason": "over the dependency-source budget", "run_id": RUN}),
            serde_json::json!({"event": "finding_flipped", "round": 0, "shard": "web/viz.js",
                "task_id": "complete-fix::web/viz.js#1",
                "finding": "web/viz.js:573 references DOM id `viz-labels` which NO html file in the app defines — getElementById returns null there and the page throws at runtime (the rendered-nothing class). Either add the id to the HTML or fix the reference to an id that exists.",
                "check": "dom-id contract scan | web/viz.js:# references dom id `viz-labels` which no html file in the app defines",
                "command": null, "fails_before": 1, "fails_after": 0,
                "run_id": RUN, "ts": "2026-09-02T08:38:46.792259+00:00"}),
            serde_json::json!({"event": "shard_promoted", "task_id": "complete-fix::web/viz.js#1",
                "files": ["web/index.html"], "three_way_merged": 0, "created_copied": 0, "run_id": RUN}),
            serde_json::json!({"event": "lang_unsupported", "arm": "pytest_tail", "lang": "TypeScript",
                "skipped": "pytest summary parse"}),
            serde_json::json!({"event": "research_answer_unowned", "from_slice": "webhooks-workflow", "q_index": 5,
                "names": ["app/register.py"], "run_id": RUN}),
            serde_json::json!({"event": "merge_hole", "module": "web/viz.js", "task_id": "viz-engine",
                "shards_missing": ["viz-engine-debug-api"], "readmes_missing": [], "merge_readme_present": true,
                "run_id": RUN}),
            serde_json::json!({"event": "merge_dossier", "module": "web/viz.js", "task_id": "viz-engine",
                "assumptions_unmet": [{"shard": "viz-engine-data-stream-render-pick",
                    "assumes": "boot (debug-api) wires new EventSource('/api/stream') with .onmessage = onStreamMessage after loadRecords",
                    "names": []}],
                "run_id": RUN}),
            serde_json::json!({"event": "judge_look", "task": "viz-engine", "run_id": RUN}),
        ]
    }

    #[test]
    fn harvest_yields_cited_facts_with_provenance_and_nothing_from_unknown_events() {
        let sink = ValueSink::default();
        let key = r6h_key();
        let mut stream = r6h_like_stream();
        stream.push(
            serde_json::json!({"event": "finding_flipped", "round": 1, "shard": "web/viz.js",
            "task_id": "t", "fails_before": 1, "fails_after": 0}),
        );
        let facts = harvest(&stream, &key, "caller-run", false, &sink);
        let kinds: Vec<&str> = facts.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                KIND_LAYOUT_COLLISION,
                KIND_LAYOUT_COLLISION,
                KIND_DEP_TOO_LARGE,
                KIND_REPAIR_FLIP,
                KIND_ARM_SKIPPED,
                KIND_ANSWER_UNOWNED,
                KIND_SHARD_GAP,
                KIND_SHARD_GAP,
            ],
            "{facts:#?}"
        );
        let harvested = [
            "plan_repaired",
            "dep_source_truncated",
            "finding_flipped",
            "lang_unsupported",
            "research_answer_unowned",
            "merge_hole",
            "merge_dossier",
        ];
        for f in &facts {
            assert!(harvested.contains(&f.provenance.event.as_str()), "{f:?}");
            assert_eq!(f.key, "python/app+web/vendor-local-v3");
            assert!(!f.written_at.is_empty());
            assert!(f.offered_by.is_empty());
            assert_eq!(f.consumed, 0);
        }
        let arm = facts.iter().find(|f| f.kind == KIND_ARM_SKIPPED).unwrap();
        assert_eq!(
            arm.provenance.run_id, "caller-run",
            "no run_id on the row → the caller's"
        );
        assert!(facts
            .iter()
            .filter(|f| f.kind != KIND_ARM_SKIPPED)
            .all(|f| f.provenance.run_id == RUN));

        let flip = facts.iter().find(|f| f.kind == KIND_REPAIR_FLIP).unwrap();
        assert_eq!(flip.files, vec!["web/viz.js", "web/index.html"]);
        assert!(
            flip.fact.contains("by writing `web/index.html`"),
            "{}",
            flip.fact
        );
        assert!(flip.fact.contains("viz-labels"));
        assert!(flip.fact.contains("1 → 0 failing"));
        assert_eq!(
            flip.provenance.ts.as_deref(),
            Some("2026-09-02T08:38:46.792259+00:00")
        );
        assert!(flip.provenance.quote.starts_with("web/viz.js:573"));

        let shadow = &facts[1];
        assert_eq!(
            shadow.files,
            vec!["app/ledgerd.py", "app/ledgerd/", "app/ledgerd/impl.py"]
        );
        assert!(shadow
            .fact
            .starts_with("plan repair (plan): module `app/ledgerd.py`"));
        assert_eq!(facts[0].files, vec!["app/__main__.py"]);

        let dep = facts.iter().find(|f| f.kind == KIND_DEP_TOO_LARGE).unwrap();
        assert!(dep.fact.contains("61240 B"), "{}", dep.fact);
        assert_eq!(dep.files, vec!["app/ledgerd/impl.py"]);

        let unowned = facts
            .iter()
            .find(|f| f.kind == KIND_ANSWER_UNOWNED)
            .unwrap();
        assert_eq!(unowned.files, vec!["app/register.py"]);
        assert!(unowned.fact.contains("q5 of slice `webhooks-workflow`"));

        let gaps: Vec<&Fact> = facts.iter().filter(|f| f.kind == KIND_SHARD_GAP).collect();
        assert!(gaps[0]
            .fact
            .contains("no pieces from `viz-engine-debug-api`"));
        assert!(gaps[1].fact.contains("assumed \"boot (debug-api) wires"));

        let skipped = sink.named("memory_harvest_skipped");
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert_eq!(skipped[0]["source"], "finding_flipped");
        assert_eq!(skipped[0]["missing"], "finding");
    }

    #[test]
    fn key_is_lang_layout_and_vendor_shape_from_the_tree_not_the_spec() {
        assert_eq!(r6h_vendor(), "vendor-local-v3");
        assert_eq!(r6h_key().path(), "python/app+web/vendor-local-v3");
        assert_eq!(
            MemoryKey::derive(TargetLang::Python, &r6h_tree(), None).path(),
            "python/app+web/no-vendor"
        );
        assert_eq!(
            MemoryKey::derive(TargetLang::TypeScript, &[], None).path(),
            "typescript/flat/no-vendor"
        );
        assert_eq!(
            vendor_shape("https://api.meridian.example/", &["/payments".to_string()]),
            "vendor-remote"
        );
        assert_eq!(vendor_shape("http://localhost:9000", &[]), "vendor-local");
        assert_eq!(
            vendor_shape(
                "http://127.0.0.1:8850",
                &["/v3/payments".to_string(), "/v2/legacy".to_string()]
            ),
            "vendor-local",
            "two versions is no shared version"
        );
        let odd = ["a+b/x.py".to_string(), "../escape/y.py".to_string()];
        assert_eq!(
            MemoryKey::derive(TargetLang::Go, &odd, Some("v/1")).path(),
            "go/_+a_b/v_1"
        );
    }

    #[test]
    fn rendered_block_carries_the_key_and_each_facts_run_id_and_bumps_consumed_at_the_consumer() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        let sink = ValueSink::default();
        let key = r6h_key();
        let facts = harvest(&r6h_like_stream(), &key, RUN, false, &sink);
        assert_eq!(store.append(&key, &facts, false, &sink), 8);
        let written = sink.named("memory_written");
        assert_eq!(written[0]["facts"], 8);
        assert_eq!(written[0]["key"], "python/app+web/vendor-local-v3");

        let block = store.render_for(&key, &Consumer::Opener, "swarm-next", false, &sink);
        assert!(
            block.starts_with(
                "LEARNED FROM EARLIER BUILDS (python/app+web/vendor-local-v3, 7 facts)\n"
            ),
            "{block}"
        );
        assert!(block.contains(&format!("· {RUN}]")), "{block}");
        assert!(block.contains("app/ledgerd/impl.py"));
        assert!(block.contains("shadowed by package"));
        assert!(
            !block.contains("viz-labels"),
            "a repair flip names files — the worker channel, not the opener's"
        );
        let read = sink.named("memory_read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["consumer"], "opener");
        assert_eq!(read[0]["stored"], 8);
        assert_eq!(read[0]["facts"], 7);
        assert_eq!(read[0]["run_id"], "swarm-next");

        let (on_disk, unparsed) = load_facts(&store.facts_path(&key), &sink);
        assert!(unparsed.is_empty());
        assert_eq!(on_disk.len(), 8);
        assert_eq!(on_disk.iter().filter(|f| f.consumed == 1).count(), 7);
        let unread = on_disk.iter().find(|f| f.consumed == 0).unwrap();
        assert_eq!(unread.kind, KIND_REPAIR_FLIP);
        assert!(on_disk
            .iter()
            .all(|f| f.offered_by == vec!["swarm-next".to_string()]));

        assert_eq!(render_block("k", &[]), "");
        let one = render_block("k", &facts[..1]);
        assert!(
            one.starts_with("LEARNED FROM EARLIER BUILDS (k, 1 fact)\n"),
            "{one}"
        );
    }

    #[test]
    fn benchmark_true_yields_nothing_and_says_memory_off() {
        let sink = ValueSink::default();
        let key = r6h_key();
        assert!(harvest(&r6h_like_stream(), &key, RUN, true, &sink).is_empty());
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        assert_eq!(
            store.render_for(&key, &Consumer::Opener, RUN, true, &sink),
            ""
        );
        assert_eq!(store.append(&key, &facts_off_benchmark(), true, &sink), 0);
        assert_eq!(store.retire(&key, 1, true, &sink), 0);
        let off = sink.named("memory_off");
        assert_eq!(off.len(), 4, "{off:?}");
        assert!(off.iter().all(|e| e["reason"] == "benchmark"));
        assert_eq!(off[0]["at"], "harvest");
        assert_eq!(off[1]["at"], "render:opener");
        assert_eq!(off[2]["at"], "append");
        assert_eq!(off[3]["at"], "retire");
        assert!(!dir.path().join("memory").exists(), "nothing is created");
        assert_eq!(
            sink.0.lock().unwrap().len(),
            4,
            "nothing else is said under benchmark"
        );
    }

    #[test]
    fn retire_drops_a_fact_offered_to_runs_and_rendered_to_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        let sink = ValueSink::default();
        let key = r6h_key();
        let facts = harvest(&r6h_like_stream(), &key, RUN, false, &sink);
        store.append(&key, &facts, false, &sink);
        for run in ["swarm-a", "swarm-b", "swarm-c"] {
            store.render_for(&key, &Consumer::Opener, run, false, &sink);
        }
        assert_eq!(
            store.retire(&key, 4, false, &sink),
            0,
            "three offers do not reach a floor of four"
        );
        assert!(sink.named("memory_retired").is_empty());
        assert_eq!(store.retire(&key, 3, false, &sink), 1);
        let (left, _) = load_facts(&store.facts_path(&key), &sink);
        assert_eq!(left.len(), 7);
        assert!(left.iter().all(|f| f.kind != KIND_REPAIR_FLIP));
        let retired = sink.named("memory_retired");
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0]["facts"], 1);
        assert_eq!(retired[0]["min_runs"], 3);
        assert_eq!(retired[0]["retired"][0]["kind"], KIND_REPAIR_FLIP);
        assert_eq!(
            retired[0]["retired"][0]["offered_by"],
            serde_json::json!(["swarm-a", "swarm-b", "swarm-c"])
        );
    }

    #[test]
    fn a_worker_gets_only_facts_naming_its_owned_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        let sink = ValueSink::default();
        let key = r6h_key();
        let facts = harvest(&r6h_like_stream(), &key, RUN, false, &sink);
        store.append(&key, &facts, false, &sink);

        let owned = vec!["web/index.html".to_string()];
        let worker = Consumer::Worker {
            task_id: "web-page",
            owned_files: owned.as_slice(),
        };
        let block = store.render_for(&key, &worker, "swarm-next", false, &sink);
        assert!(
            block.starts_with(
                "LEARNED FROM EARLIER BUILDS (python/app+web/vendor-local-v3, 1 fact)\n"
            ),
            "{block}"
        );
        assert!(block.contains("viz-labels"));
        assert!(!block.contains("app/__main__.py"));
        let read = sink.named("memory_read");
        assert_eq!(read[0]["consumer"], "worker:web-page");
        assert_eq!(read[0]["facts"], 1);

        let owned = vec!["./app/ledgerd/impl.py".to_string()];
        let worker = Consumer::Worker {
            task_id: "ledgerd-core",
            owned_files: owned.as_slice(),
        };
        let block = store.render_for(&key, &worker, "swarm-next", false, &sink);
        assert!(block.contains("shadowed by package"), "{block}");
        assert!(block.contains("could not ride whole"), "{block}");
        assert!(!block.contains("viz-labels"));

        let owned = vec!["app/other.py".to_string()];
        let worker = Consumer::Worker {
            task_id: "other",
            owned_files: owned.as_slice(),
        };
        assert_eq!(
            store.render_for(&key, &worker, "swarm-next", false, &sink),
            ""
        );
        assert_eq!(sink.named("memory_read")[2]["facts"], 0);

        let (on_disk, _) = load_facts(&store.facts_path(&key), &sink);
        assert!(
            on_disk
                .iter()
                .all(|f| f.offered_by == vec!["swarm-next".to_string()]),
            "three consumers in one run offer once"
        );
    }

    #[test]
    fn a_greenfield_open_key_recalls_what_the_built_layout_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        let sink = ValueSink::default();
        let key = r6h_key();
        let facts = harvest(&r6h_like_stream(), &key, RUN, false, &sink);
        store.append(&key, &facts, false, &sink);

        let open_key = MemoryKey::derive(TargetLang::Python, &[], Some("vendor-local-v3"));
        assert_eq!(open_key.path(), "python/flat/vendor-local-v3");
        let block = store.render_for(&open_key, &Consumer::Opener, "swarm-next", false, &sink);
        assert!(
            block.contains("python/flat/vendor-local-v3, 7 facts"),
            "{block}"
        );

        let partial = MemoryKey::derive(
            TargetLang::Python,
            &["app/__init__.py".to_string()],
            Some("vendor-local-v3"),
        );
        assert!(!store
            .render_for(&partial, &Consumer::Opener, "swarm-next", false, &sink)
            .is_empty());

        for miss in [
            MemoryKey::derive(
                TargetLang::Python,
                &["api/main.py".to_string()],
                Some("vendor-local-v3"),
            ),
            MemoryKey::derive(TargetLang::Python, &[], None),
            MemoryKey::derive(TargetLang::TypeScript, &[], Some("vendor-local-v3")),
        ] {
            assert_eq!(
                store.render_for(&miss, &Consumer::Opener, "swarm-next", false, &sink),
                "",
                "{}",
                miss.path()
            );
        }
        assert!(sink.named("memory_unreadable").is_empty());
    }

    #[test]
    fn append_skips_a_facts_second_write_from_the_same_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory"));
        let sink = ValueSink::default();
        let key = r6h_key();
        let facts = harvest(&r6h_like_stream(), &key, RUN, false, &sink);
        assert_eq!(store.append(&key, &facts, false, &sink), 8);
        assert_eq!(store.append(&key, &facts, false, &sink), 0);
        let written = sink.named("memory_written");
        assert_eq!(written[1]["facts"], 0);
        assert_eq!(written[1]["duplicates_skipped"], 8);
        let (on_disk, _) = load_facts(&store.facts_path(&key), &sink);
        assert_eq!(on_disk.len(), 8);
        assert_eq!(store.append(&key, &[], false, &sink), 0);
        assert_eq!(sink.named("memory_written")[2]["facts"], 0);
    }

    #[test]
    fn load_events_reads_the_run_stream_and_says_how_many_lines_did_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.jsonl");
        std::fs::write(
            &path,
            "{\"event\":\"run_started\"}\nnot json\n\n{\"event\":\"plan_repaired\",\"source\":\"plan\",\"actions\":[]}\n",
        )
        .unwrap();
        let sink = ValueSink::default();
        let events = load_events(&path, &sink);
        assert_eq!(events.len(), 2);
        let bad = sink.named("memory_unreadable");
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0]["bad_lines"], 1);
        assert!(load_events(&dir.path().join("absent.jsonl"), &sink).is_empty());
        assert_eq!(sink.named("memory_unreadable").len(), 2);
    }

    /// VA-139 at the CONSUMERS: the opener's user turn and a worker's brief. Under benchmark
    /// (r6k) both are the pre-wiring bytes with facts on disk; off benchmark the block sits
    /// between the request and SOURCES, and the worker's brief ends with the fact naming its
    /// owned file.
    #[test]
    fn under_benchmark_the_opener_turn_and_the_worker_brief_are_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("memory");
        let sink = ValueSink::default();
        let key = r6h_key();
        let store = MemoryStore::open(root.clone());
        assert_eq!(store.append(&key, &facts_off_benchmark(), false, &sink), 8);

        let cell = MemoryCell::default();
        cell.set(RunMemory::new(
            MemoryStore::open(root.clone()),
            TargetLang::Python,
            &[],
            Some("vendor-local-v3"),
            "swarm-r6k",
            true,
        ));
        let request = "The request:\n\n# Build `app`";
        let sources = "\n\nSOURCES — what is research material here:\n- `.swarm/` is this \
                       engine's own state.\n";
        let learned = cell.render_for(&Consumer::Opener, &sink);
        assert_eq!(learned, "");
        assert_eq!(
            opener_user_text(request, &learned, sources),
            format!("{request}{sources}"),
            "the pre-VA-139 opener turn, byte for byte"
        );
        let desc = "Write `web/index.html`: the console page the spec's §6 describes.";
        let owned = vec!["web/index.html".to_string()];
        let worker = Consumer::Worker {
            task_id: "web-page",
            owned_files: owned.as_slice(),
        };
        let learned = cell.render_for(&worker, &sink);
        assert_eq!(with_learned(desc, &learned), desc);
        let off = sink.named("memory_off");
        assert_eq!(off.len(), 2, "{off:?}");
        assert_eq!(off[0]["at"], "render:opener");
        assert_eq!(off[1]["at"], "render:worker:web-page");
        assert!(sink.named("memory_read").is_empty());
        let (on_disk, _) = load_facts(&store.facts_path(&key), &sink);
        assert!(
            on_disk
                .iter()
                .all(|f| f.consumed == 0 && f.offered_by.is_empty()),
            "a benchmark run touches nothing on disk"
        );

        let cell = MemoryCell::default();
        cell.set(RunMemory::new(
            MemoryStore::open(root),
            TargetLang::Python,
            &[],
            Some("vendor-local-v3"),
            "swarm-next",
            false,
        ));
        let turn = opener_user_text(request, &cell.render_for(&Consumer::Opener, &sink), sources);
        assert!(turn.starts_with(request), "{turn}");
        let block_at = turn
            .find("LEARNED FROM EARLIER BUILDS (python/flat/vendor-local-v3, 7 facts)")
            .unwrap_or_else(|| panic!("{turn}"));
        let sources_at = turn.find("SOURCES — ").unwrap();
        assert!(block_at > request.len() && block_at < sources_at, "{turn}");
        let brief = with_learned(desc, &cell.render_for(&worker, &sink));
        assert!(brief.starts_with(desc), "{brief}");
        assert!(brief.contains("viz-labels"), "{brief}");
        assert_eq!(sink.named("memory_read").len(), 2);
    }

    /// VA-139's write door: the FINISHED tree's key (not OPEN's flat one), a loud absence for a
    /// run with no log, a loud `memory_unset` before OPEN set the cell, and no read at all under
    /// benchmark.
    #[test]
    fn close_run_stores_under_the_built_tree_and_an_unset_cell_is_loud() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("memory");
        let sink = ValueSink::default();
        let cell = MemoryCell::default();
        assert_eq!(cell.render_for(&Consumer::Opener, &sink), "");
        cell.close_run(&r6h_tree(), None, &sink);
        let unset = sink.named("memory_unset");
        assert_eq!(unset.len(), 2, "{unset:?}");
        assert_eq!(unset[0]["at"], "render:opener");
        assert_eq!(unset[1]["at"], "close_run");
        assert!(!root.exists());

        let run_jsonl = dir.path().join("run.jsonl");
        let body: String = r6h_like_stream().iter().map(|v| format!("{v}\n")).collect();
        std::fs::write(&run_jsonl, body).unwrap();
        cell.set(RunMemory::new(
            MemoryStore::open(root.clone()),
            TargetLang::Python,
            &[],
            Some("vendor-local-v3"),
            RUN,
            false,
        ));
        cell.close_run(&r6h_tree(), Some(&run_jsonl), &sink);
        let written = sink.named("memory_written");
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(
            written[0]["key"], "python/app+web/vendor-local-v3",
            "the BUILT tree's key, not OPEN's flat one"
        );
        assert_eq!(written[0]["facts"], 8);
        assert!(root
            .join("python/app+web/vendor-local-v3/facts.jsonl")
            .is_file());
        assert!(
            sink.named("memory_retired").is_empty(),
            "nothing was ever offered"
        );

        cell.close_run(&r6h_tree(), None, &sink);
        let skipped = sink.named("memory_harvest_skipped");
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert_eq!(skipped[0]["source"], "run_jsonl");
        assert_eq!(sink.named("memory_written")[1]["facts"], 0);

        let bench = MemoryCell::default();
        bench.set(RunMemory::new(
            MemoryStore::open(root),
            TargetLang::Python,
            &[],
            Some("vendor-local-v3"),
            "swarm-r6k",
            true,
        ));
        let sink = ValueSink::default();
        bench.close_run(&r6h_tree(), Some(&dir.path().join("absent.jsonl")), &sink);
        let at: Vec<String> = sink
            .named("memory_off")
            .iter()
            .map(|e| e["at"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(at, vec!["harvest", "append", "retire"]);
        assert_eq!(
            sink.0.lock().unwrap().len(),
            3,
            "an absent log is never read under benchmark: no memory_unreadable"
        );
    }
}
