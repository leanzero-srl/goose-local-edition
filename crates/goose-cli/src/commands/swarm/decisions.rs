//! DECISIONS INTO THE FAN — the plan-time settlement of the opener's open decisions.
//!
//! Fourth sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases). The r5 receipt this exists for: 5 open decisions were
//! asked and 0 answered (the benchmark's ask window folds instantly with no answers — the
//! low_confidence_ask_timeout event says "no answers arrived"), so every brief carried "choose
//! the most CONVENTIONAL option", SYNTHESIS emitted a w1 docs task ("decisions") to hold the
//! choices, four implementation tasks were made to DEPEND on it for consistency, and the plan's
//! post-skeleton width collapsed to 2 while gabee idled. The fix is to SETTLE the unanswered
//! remainder at plan time — one uncapped, judged, ledgered research call per still-open decision,
//! riding the research fan — and then splice the settled partition into every brief and every
//! worker prompt, so no docs task has to serialize the build to deliver consistency.
//!
//! Three channels, by amendment:
//!   (b) every brief (including the docs task's own) carries the settled/still-open PARTITION,
//!       with settled choices QUOTED verbatim — never re-derived from the per-slice row fold,
//!       which matches on `r.slice == sl.id` and can never see a decision row;
//!   (c) DELETED (VA-030): workers used to receive every research-settled answer a second time,
//!       cut at 1,500 chars, under a provenance header appended to `DispatchRequest.user_decisions`;
//!       the brief (the task description, i.e. the worker prompt's body) carries them once, per
//!       slice and whole, so `user_decisions` is the USER's channel only. A REPAIR shard has no
//!       brief: it reads its OWNING task's block, found again in the loaded DAG
//!       (`BriefDecisions`, D11) — the deletion first left that channel empty;
//!   (e) when EVERY open decision folded as settled, the plan repair strips
//!       implementation-task -> docs-only-task dependency edges (loud, MILD, rides
//!       `plan_repaired.actions`); one unanswered decision KEEPS every such dep — the doc task is
//!       then the consistency mechanism and the serialization is the honest price.

use super::research::{budget_research_answer, ResearchRow};
use super::USER_DECISIONS_HEADER;

/// The reserved slice id decision rows ride the fan under. Dunder-fenced so no opener-produced
/// slice id collides with it (slice ids are kebab-case words from a model; the partition in
/// `run_linear_plan` siphons rows by exact equality on this constant).
pub(super) const DECISION_SLICE: &str = "__open_decisions__";

/// The header prefixes a brief's decisions block opens with — `decisions_brief_block` below and
/// research.rs's `slice_decisions_block` both start theirs with the first; the anchors
/// `BriefDecisions` finds the block again by.
pub(super) const SETTLED_DECISIONS_HEADER: &str = "DECISIONS SETTLED AT PLAN TIME";
pub(super) const OPEN_DECISIONS_HEADER: &str = "OPEN DECISIONS —";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DecisionState {
    /// The human answered in the ASK handshake — quoted from the clarify Q/A block, binding.
    SettledByUser { answer: String },
    /// A fan lane answered from the request (or named a convention) — binding for consistency.
    SettledByResearch { answer: String },
    /// Nobody settled it (no user answer, and the fan lane missed or was never dispatched).
    Open,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanDecision {
    pub(crate) q_index: usize,
    pub(crate) question: String,
    pub(crate) state: DecisionState,
}

/// The user's clarify answers, parsed back out of the exact block `ask_clarifying_questions`
/// emits (`Q: {question}\nA: {answer}\n` pairs; the free-form guidance line matches nothing here).
/// A decision whose text the opener wrapped across lines would fail this exact-line match and
/// read as unanswered — the honest degradation: it then rides the fan instead of being guessed.
fn user_answers(user_qa: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut pending_q: Option<String> = None;
    for line in user_qa.lines() {
        if let Some(q) = line.strip_prefix("Q: ") {
            pending_q = Some(q.to_string());
        } else if let Some(a) = line.strip_prefix("A: ") {
            if let Some(q) = pending_q.take() {
                if !a.trim().is_empty() {
                    out.insert(q, a.trim().to_string());
                }
            }
        } else {
            pending_q = None;
        }
    }
    out
}

/// The pre-fan remainder: every open decision the user did NOT answer, with its stable index into
/// the opener's own list (the index is the resume identity of the decision's ledger mini).
/// Amendment (f): this — per-decision answer-absence — is the ONLY trigger. An attended run where
/// the human answers everything returns empty and nothing downstream fires.
pub(super) fn still_open_after_user(
    open_decisions: &[String],
    user_qa: &str,
) -> Vec<(usize, String)> {
    let answered = user_answers(user_qa);
    open_decisions
        .iter()
        .enumerate()
        .filter(|(_, d)| !answered.contains_key(d.as_str()))
        .map(|(i, d)| (i, d.clone()))
        .collect()
}

/// The post-fan partition: every opener decision, in opener order, with how (or whether) it
/// settled. User answers outrank research answers by construction — a research row exists only
/// for decisions the user left open.
pub(super) fn partition_decisions(
    open_decisions: &[String],
    user_qa: &str,
    decision_rows: &[ResearchRow],
) -> Vec<PlanDecision> {
    let answered = user_answers(user_qa);
    open_decisions
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let state = if let Some(a) = answered.get(d.as_str()) {
                DecisionState::SettledByUser { answer: a.clone() }
            } else if let Some(row) = decision_rows.iter().find(|r| {
                r.q_index == i
                    && r.status == super::research::RESEARCH_ANSWERED
                    && !r.answer.trim().is_empty()
            }) {
                DecisionState::SettledByResearch {
                    answer: row.answer.clone(),
                }
            } else {
                DecisionState::Open
            };
            PlanDecision {
                q_index: i,
                question: d.clone(),
                state,
            }
        })
        .collect()
}

/// True only when decisions EXISTED and every one folded as settled. An empty list is false on
/// purpose: with no open decisions there is no decisions gate to strip, and the repair backstop
/// must not fire on a docs task that gates the build for some other reason.
pub(super) fn all_settled(decisions: &[PlanDecision]) -> bool {
    !decisions.is_empty()
        && decisions
            .iter()
            .all(|d| !matches!(d.state, DecisionState::Open))
}

/// The partition block spliced into EVERY brief (amendment b) — including the decisions/docs
/// task's own, whose description must QUOTE the settled choices verbatim (splice_briefs puts each
/// brief into its task's description verbatim, so the quote survives to dispatch). Settled
/// answers are quoted with their provenance; still-open decisions keep the pre-fan conventional
/// framing verbatim. Research answers ride through `budget_research_answer`, so a page-long
/// answer lands as its line-bounded head plus the ledger mini's path — the full text is durable.
pub(super) fn decisions_brief_block(decisions: &[PlanDecision]) -> String {
    if decisions.is_empty() {
        return String::new();
    }
    let mut settled = String::new();
    let mut open: Vec<&String> = Vec::new();
    for d in decisions {
        match &d.state {
            DecisionState::SettledByUser { answer } => {
                settled.push_str(&format!(
                    "- {}\n  THE USER CHOSE: {answer}\n",
                    d.question.trim()
                ));
            }
            DecisionState::SettledByResearch { answer } => {
                settled.push_str(&format!(
                    "- {}\n  SETTLED BY PLAN-TIME RESEARCH (the user did not answer; a \
                     convention, binding for consistency): {}\n",
                    d.question.trim(),
                    budget_research_answer(answer, DECISION_SLICE, d.q_index)
                ));
            }
            DecisionState::Open => open.push(&d.question),
        }
    }
    let mut out = String::new();
    if !settled.is_empty() {
        out.push_str(&format!(
            "\n\n{SETTLED_DECISIONS_HEADER} — quoted verbatim and BINDING; implement each \
             exactly as written and never substitute your own convention:\n{settled}"
        ));
    }
    if !open.is_empty() {
        out.push_str(&format!(
            "\n\n{OPEN_DECISIONS_HEADER} unless a USER DECISIONS block in the request settles one of \
             these, choose the most CONVENTIONAL option and note the choice in a code comment; \
             never invent a novel one:\n{}",
            open.iter()
                .map(|d| format!("- {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    out
}

/// VA-030 D11 — the decisions block a REPAIR shard reads. A shard owns FILES, not a slice, so it
/// has no brief of its own; the tasks that own its files are its owners, and their briefs'
/// blocks — the selection the brief already made, rendered once at plan time — are what it
/// reads. Indexed from the DAG the run loaded (the in-memory plan), never from a sidecar:
/// plan_store's law is that the sidecars have no engine reader. r6c's seven shards: five owned
/// by web-viz / ledgerd-api / web-console (each brief carrying a 5,584-char block), two by the
/// skeleton (no block) — after the worker-channel copy was deleted (D10-3) all seven read none.
#[derive(Clone, Default)]
pub(super) struct BriefDecisions {
    /// `(task id, owned files, the brief's decisions block)` — tasks whose brief carries none
    /// are not indexed, so `for_files` measures the absence as `owners` without a block.
    per_task: Vec<(String, Vec<String>, String)>,
}

pub(super) struct ShardDecisions {
    /// Every task owning one of the shard's files, whether or not its brief carried a block.
    pub(super) owners: Vec<String>,
    /// The owners' blocks, an identical block delivered once; empty when no owner carries one.
    pub(super) block: String,
}

impl BriefDecisions {
    pub(super) fn from_tasks<'a>(
        tasks: impl Iterator<Item = (&'a str, &'a [String], &'a str)>,
    ) -> Self {
        let mut per_task: Vec<(String, Vec<String>, String)> = tasks
            .map(|(id, files, description)| {
                (
                    id.to_string(),
                    files.to_vec(),
                    brief_decisions_block(description).unwrap_or("").to_string(),
                )
            })
            .collect();
        per_task.sort_by(|a, b| a.0.cmp(&b.0));
        Self { per_task }
    }

    pub(super) fn for_files(&self, files: &[String]) -> ShardDecisions {
        let mut owners = Vec::new();
        let mut block = String::new();
        for (id, owned, b) in &self.per_task {
            if !owned.iter().any(|f| files.contains(f)) {
                continue;
            }
            owners.push(id.clone());
            if !b.is_empty() && !block.contains(b.as_str()) {
                block.push_str(b);
            }
        }
        ShardDecisions { owners, block }
    }
}

/// A brief's decisions block, from its header to the brief's end — minus the plan repairs'
/// tails, which are the OWNER's: rule (e)'s UNOWNED-FILES list and rule (d)'s ADVERTISED SURFACE
/// note (rule (d) runs first in the chain, so an entry owner's brief carries the endpoint note
/// BETWEEN its block and the unowned list; a cut at the list alone handed the shard the owner's
/// entry instruction as "decisions" — 2a D11's refuter). The cut is at whichever tail comes
/// first. None when the brief carries no block (a run with no open decisions, or a brief that
/// never got one — r6c's 387-char decisions-doc brief).
pub(super) fn brief_decisions_block(description: &str) -> Option<&str> {
    let start = [SETTLED_DECISIONS_HEADER, OPEN_DECISIONS_HEADER]
        .iter()
        .filter_map(|h| description.find(&format!("\n\n{h}")))
        .min()?;
    let block = description.get(start..)?;
    let end = [
        super::plan_repairs::UNOWNED_FILES_HEADER,
        super::plan_repairs::ADVERTISED_SURFACE_HEADER,
    ]
    .iter()
    .filter_map(|tail| block.find(&format!("\n\n{tail}")))
    .min()
    .unwrap_or(block.len());
    block.get(..end)
}

/// The decisions lane's prompt HEAD (the fan adds the snowball block and EVERY still-open
/// decision, tagged, under THE OPEN DECISIONS at dispatch through `research_user_text`, exactly
/// as a slice lane carries its batch — C3: one lane settles them all in one session). The full
/// request rides whole: a decision is global — no claimed-section subset exists to splice — and
/// "answer strictly from the request" requires the request. User-settled decisions ride under
/// the ONE `USER_DECISIONS_HEADER` constant so settled choices inform the still-open ones; the
/// SOURCES block is the same one every slice lane gets.
pub(super) fn decision_user_text(
    spec: &str,
    user_decisions: &str,
    tree_at_start: &[String],
    sources_block: &str,
) -> String {
    let decisions_block = if user_decisions.trim().is_empty() {
        String::new()
    } else {
        format!("{USER_DECISIONS_HEADER}{user_decisions}")
    };
    format!(
        "THE REQUEST:\n{spec}{decisions_block}{}{sources_block}\n\nThe OPEN DECISION below was \
         put to the user and the user did not answer it. Answer STRICTLY from the request; where \
         the request is silent, name the most CONVENTIONAL choice and say it is a convention.",
        super::research::research_tree_block(tree_at_start)
    )
}

/// A decision document is documentation the app SHIPS: a `.md`/`.rst`/`.txt` inside the scored
/// tree. Anything under the engine's own work area (`tree::SNAPSHOT_EXCLUDES` — `.swarm/`, where
/// `shards::SHARDS_DIR` lives) never reaches the scored tree, so a file there is a build artifact
/// whatever its extension, never a decision document.
fn is_shipped_doc_file(f: &str) -> bool {
    let l = f.trim().trim_start_matches("./").to_lowercase();
    let in_engine_area = super::tree::SNAPSHOT_EXCLUDES
        .iter()
        .any(|ex| l == *ex || l.starts_with(&format!("{ex}/")));
    !in_engine_area && (l.ends_with(".md") || l.ends_with(".rst") || l.ends_with(".txt"))
}

/// Amendment (e), gate-6 class (loud, MILD, rides `plan_repaired.actions`): when EVERY open
/// decision folded as settled at plan time, an implementation task no longer needs a docs-only
/// task upstream for decision consistency — the settled choices ride every brief and every
/// worker prompt — so those edges are stripped and the width the r5 plan lost comes back
/// (post-skeleton width 2 -> 4 on r5's shape, which saturates 3 nodes). One unanswered decision
/// KEEPS every such dep: the doc task is then the consistency mechanism, and serializing behind
/// it is the honest price of an unsettled choice. A decision-doc task = non-sink, not a shard or
/// merger of THE SPLIT, owns at least one file, every owned file a SHIPPED document
/// (`is_shipped_doc_file`). The sink keeps its deps (it owns nothing and must wait for
/// everything); doc-on-doc deps are untouched.
///
/// The r6e receipt (VA-063, killed at BUILD+4m): the split gave `viz3d-engine` eight shard tasks
/// each owning ONLY `.swarm/shards/viz3d-engine/<shard>/README.md` and made the module their
/// merger (`depends_on` = its planner deps `[]` + the eight shards). The extension test alone read
/// all eight shards as docs-only and this gate dropped them — `plan_repaired{source: split}`
/// 16:28:46Z: "`viz3d-engine` was gated on docs-only `viz3d-engine-data-scene`, … — dep dropped";
/// `task_dispatched viz3d-engine deps: []` in the same instant as `plan_loaded`;
/// `merge_dossier{pieces: 0, readmes_missing: [all 8]}`. A shard's README is the merger's INPUT,
/// a build artifact under the engine's work area, never a decision document.
pub(super) fn repair_decision_doc_gates(
    plan: &mut serde_json::Value,
    every_decision_settled: bool,
    actions: &mut Vec<String>,
) {
    if !every_decision_settled {
        return;
    }
    let Some(subtasks) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) else {
        return;
    };
    let files_of = |t: &serde_json::Value| -> Vec<String> {
        match t.get("files").and_then(|f| f.as_array()) {
            // An absent/non-array `files` field IS the desired reading — the task owns nothing —
            // the same honest-empty `repair_sink_files` documents. Stated as a branch, not a
            // silent default, per the fallback gate.
            None => Vec::new(),
            Some(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
        }
    };
    // The split's own markers (`shards::apply_module_split` writes them; a model never does): a
    // shard is a build task whatever it owns, and the merger is the build task the shards feed.
    let is_decision_doc_task = |t: &serde_json::Value| -> bool {
        let files = files_of(t);
        t.get("id").and_then(|i| i.as_str()) != Some(goose_swarm::SINK_ID)
            && t.get("shard_of").is_none()
            && t.get("merger_of").is_none()
            && !files.is_empty()
            && files.iter().all(|f| is_shipped_doc_file(f))
    };
    let doc_only_ids: std::collections::HashSet<String> = subtasks
        .iter()
        .filter(|t| is_decision_doc_task(t))
        .filter_map(|t| t.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect();
    if doc_only_ids.is_empty() {
        return;
    }
    for t in subtasks.iter_mut() {
        // The un-gating action names the task, so it needs a real id — and an id-less task
        // cannot load as a DAG anyway (the same reasoning as repair_sink_files' home), so
        // skipping one here loses nothing.
        let Some(id) = t.get("id").and_then(|i| i.as_str()).map(String::from) else {
            continue;
        };
        if files_of(t).is_empty() || is_decision_doc_task(t) {
            continue; // not an implementation task: the sink and doc tasks keep their deps
        }
        let Some(deps) = t.get_mut("depends_on").and_then(|d| d.as_array_mut()) else {
            continue;
        };
        let dropped: Vec<String> = deps
            .iter()
            .filter_map(|d| d.as_str())
            .filter(|d| doc_only_ids.contains(*d))
            .map(String::from)
            .collect();
        if dropped.is_empty() {
            continue;
        }
        deps.retain(|d| {
            d.as_str()
                .map(|x| !doc_only_ids.contains(x))
                .unwrap_or(true)
        });
        actions.push(format!(
            "`{id}` was gated on docs-only `{}`: every open decision settled at plan time, so \
             the doc no longer serializes the build — dep dropped",
            dropped.join("`, `")
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(q_index: usize, status: &str, answer: &str) -> ResearchRow {
        ResearchRow {
            slice: DECISION_SLICE.to_string(),
            q_index,
            question: format!("d{q_index}"),
            status: status.to_string(),
            answer: answer.to_string(),
            reason: None,
            detail: None,
            raised: Vec::new(),
            model: "m".to_string(),
            secs: 1,
            kind: "design".to_string(),
            cite: String::new(),
            batch: 0,
        }
    }

    /// The r5 shape: 5 decisions, 0 user answers (the benchmark fold), the fan settles 4 and
    /// misses 1 — the partition must quote the 4 and keep the 1 open, verbatim.
    #[test]
    fn partition_is_user_first_then_research_then_open() {
        let decisions: Vec<String> = (0..5).map(|i| format!("d{i}")).collect();
        let qa = "Q: d1\nA: pipe-separated\n";
        let rows = vec![
            row(0, "answered", "HTTP 409 on concurrent sync"),
            row(2, "answered", "ThreadingHTTPServer"),
            row(3, "answered", ""),          // parsed-but-blank: NOT settled
            row(4, "unanswered", "ignored"), // terminal miss: NOT settled
        ];
        let p = partition_decisions(&decisions, qa, &rows);
        assert_eq!(p.len(), 5);
        assert_eq!(
            p[1].state,
            DecisionState::SettledByUser {
                answer: "pipe-separated".into()
            }
        );
        assert_eq!(
            p[0].state,
            DecisionState::SettledByResearch {
                answer: "HTTP 409 on concurrent sync".into()
            }
        );
        assert_eq!(p[3].state, DecisionState::Open);
        assert_eq!(p[4].state, DecisionState::Open);
        assert!(!all_settled(&p), "an open decision means NOT all settled");
        // Pre-fan remainder: everything the user did not answer, with stable indices.
        let open = still_open_after_user(&decisions, qa);
        assert_eq!(
            open.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![0, 2, 3, 4]
        );
        // Attended run, everything answered: nothing fires (amendment f).
        let all_qa = "Q: d0\nA: x\nQ: d1\nA: x\nQ: d2\nA: x\nQ: d3\nA: x\nQ: d4\nA: x\n";
        assert!(still_open_after_user(&decisions, all_qa).is_empty());
        assert!(all_settled(&partition_decisions(&decisions, all_qa, &[])));
        // No decisions at all: never "all settled" (the repair backstop must not arm).
        assert!(!all_settled(&[]));
    }

    #[test]
    fn brief_block_quotes_settled_verbatim_and_keeps_open_conventional() {
        let p = vec![
            PlanDecision {
                q_index: 0,
                question: "which storage backend".into(),
                state: DecisionState::SettledByUser {
                    answer: "sqlite".into(),
                },
            },
            PlanDecision {
                q_index: 1,
                question: "which port".into(),
                state: DecisionState::SettledByResearch {
                    answer: "8000, the request's own default".into(),
                },
            },
            PlanDecision {
                q_index: 2,
                question: "which palette".into(),
                state: DecisionState::Open,
            },
        ];
        let b = decisions_brief_block(&p);
        assert!(b.contains("THE USER CHOSE: sqlite"));
        assert!(b.contains("8000, the request's own default"));
        assert!(b.contains("binding for consistency"));
        assert!(b.contains("- which palette") && b.contains("CONVENTIONAL"));
        assert!(decisions_brief_block(&[]).is_empty());
    }

    /// r6c's real shape: web-console's brief ended "...the run exercises all three.\n\n---" and
    /// then the block; the D5 repair appends an UNOWNED-FILES tail after it. The shard for
    /// `web/app.js` reads exactly the block; `app/ledgerd/__init__.py` (skeleton, no block) reads
    /// none and names its owner; two owners carrying the identical block deliver it once.
    #[test]
    fn a_shard_reads_its_owners_brief_block_once() {
        let block = decisions_brief_block(&[PlanDecision {
            q_index: 0,
            question: "D1 — does the brush survive a streamed mutation of a brushed record?".into(),
            state: DecisionState::SettledByResearch {
                answer: "Stay brushed.\n\n3. `web/app.js` behavior contract: keep the row.".into(),
            },
        }]);
        let unowned_tail = format!(
            "\n\n{} — read them if you need them, never write them:\n- `DECISIONS.md` → owned by task `decisions-doc`\n",
            super::super::plan_repairs::UNOWNED_FILES_HEADER
        );
        // Rule (d)'s note lands BEFORE rule (e)'s list in the chain; both are the owner's, not
        // the shard's — and the cut must hold whichever order they arrive in.
        let advertised_tail = format!(
            "\n\n{}: the spec's endpoint table lists these on this service… This task owns the \
             entry of `python -m app.ledgerd`, so it serves each one exactly as the table says:\n\
             - `GET /api/health`\n",
            super::super::plan_repairs::ADVERTISED_SURFACE_HEADER
        );
        let console = format!(
            "Ship the console. The run exercises all three.\n\n---{block}{advertised_tail}{unowned_tail}"
        );
        let reversed = format!("Ship it.{block}{unowned_tail}{advertised_tail}");
        assert_eq!(
            brief_decisions_block(&reversed),
            Some(block.as_str()),
            "the cut is at the FIRST tail whichever order the repairs appended them"
        );
        let viz = format!("Draw the canvas.{block}");
        let skeleton = "Boot both packages; DONE means every route answers.".to_string();
        let files = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let console_files = files(&["web/index.html", "web/styles.css", "web/app.js"]);
        let viz_files = files(&["web/viz.js"]);
        let skeleton_files = files(&["app/__main__.py", "app/ledgerd/__init__.py"]);
        let index = BriefDecisions::from_tasks(
            [
                ("web-console", console_files.as_slice(), console.as_str()),
                ("web-viz", viz_files.as_slice(), viz.as_str()),
                ("skeleton", skeleton_files.as_slice(), skeleton.as_str()),
            ]
            .into_iter(),
        );

        let app_js = index.for_files(&files(&["web/app.js"]));
        assert_eq!(app_js.owners, vec!["web-console".to_string()]);
        assert_eq!(
            app_js.block, block,
            "exactly the brief's block, no prose before, no tail after"
        );
        assert!(
            !app_js.block.contains("DECISIONS.md"),
            "the owner's unowned-files list is not the shard's"
        );
        assert!(
            !app_js.block.contains("ADVERTISED SURFACE") && !app_js.block.contains("python -m"),
            "the owner's entry instruction is not the shard's decisions: {}",
            app_js.block
        );

        let init = index.for_files(&files(&["app/ledgerd/__init__.py"]));
        assert_eq!(init.owners, vec!["skeleton".to_string()]);
        assert!(
            init.block.is_empty(),
            "no block is measured as chars 0, never invented"
        );

        let both = index.for_files(&files(&["web/app.js", "web/viz.js"]));
        assert_eq!(
            both.owners,
            vec!["web-console".to_string(), "web-viz".to_string()]
        );
        assert_eq!(
            both.block.matches("D1 — does the brush").count(),
            1,
            "one identical block, once"
        );

        assert!(index
            .for_files(&files(&["nobody/owns.py"]))
            .owners
            .is_empty());
        assert!(brief_decisions_block("no block here").is_none());
    }

    /// The r5 receipt verbatim: ledgerd/brush-contract/frontend/viz gated on the w1 docs task.
    /// All settled -> the four edges drop (width 2 -> 4); one open -> every edge stays.
    #[test]
    fn decision_doc_gate_strips_only_when_every_decision_settled() {
        let plan = serde_json::json!({"subtasks": [
            {"id": "decisions", "files": ["DECISIONS.md"], "depends_on": []},
            {"id": "ledgerd", "files": ["app/ledgerd/__init__.py"], "depends_on": ["decisions"]},
            {"id": "brush-contract", "files": ["app/brush.py"], "depends_on": ["decisions"]},
            {"id": "frontend", "files": ["web/index.html"], "depends_on": ["decisions"]},
            {"id": "viz", "files": ["web/viz.js"], "depends_on": ["decisions", "frontend"]},
            {"id": "integrate-verify", "files": [], "depends_on":
                ["decisions", "ledgerd", "brush-contract", "frontend", "viz"]},
        ]});
        let mut settled = plan.clone();
        let mut actions = Vec::new();
        repair_decision_doc_gates(&mut settled, true, &mut actions);
        let deps = |v: &serde_json::Value, id: &str| -> Vec<String> {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .unwrap()["depends_on"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_str().unwrap().to_string())
                .collect()
        };
        for id in ["ledgerd", "brush-contract", "frontend"] {
            assert!(deps(&settled, id).is_empty(), "{id} no longer gated");
        }
        assert_eq!(deps(&settled, "viz"), vec!["frontend"], "real deps survive");
        assert!(
            deps(&settled, "integrate-verify").contains(&"decisions".to_string()),
            "the sink owns nothing and keeps waiting for everything"
        );
        assert_eq!(actions.len(), 4, "one loud action per un-gated task");
        // One decision still open: the doc task IS the consistency mechanism — nothing moves.
        let mut kept = plan.clone();
        let mut actions = Vec::new();
        repair_decision_doc_gates(&mut kept, false, &mut actions);
        assert_eq!(kept, plan);
        assert!(actions.is_empty());
    }

    /// VA-063, the r6e shape reduced: merger `m` (keeps `web/viz.js`, `merger_of`) depends on its
    /// shards `m-a`/`m-b` (each owns ONLY `.swarm/shards/m/<x>/README.md`, `shard_of`) AND on a
    /// real decision doc `decisions-doc` (`DECISIONS.md`). Every decision settled: the merger drops
    /// ONLY the decision doc and keeps both shards — the run dropped all three classes at once and
    /// dispatched the merger over zero pieces. Both halves of the predicate hold alone: with the
    /// `shard_of` marker removed, the `.swarm/` work-area test still keeps the shard.
    #[test]
    fn decision_doc_gate_never_reads_a_shard_readme_as_a_decision_doc() {
        let shard = |x: &str| {
            serde_json::json!({
                "id": format!("m-{x}"),
                "files": [format!(".swarm/shards/m/{x}/README.md")],
                "depends_on": [],
                "shard_of": {"module": "m", "shard": x, "folder": format!(".swarm/shards/m/{x}")},
            })
        };
        let plan = serde_json::json!({"subtasks": [
            {"id": "decisions-doc", "files": ["DECISIONS.md"], "depends_on": []},
            {"id": "m", "files": ["web/viz.js"], "depends_on": ["decisions-doc", "m-a", "m-b"],
             "merger_of": {"module": "m", "shards": ["m-a", "m-b"],
                           "folders": [".swarm/shards/m/a", ".swarm/shards/m/b"]}},
            shard("a"),
            shard("b"),
            {"id": "integrate-verify", "files": [], "depends_on": ["decisions-doc", "m", "m-a", "m-b"]},
        ]});
        let deps = |v: &serde_json::Value, id: &str| -> Vec<String> {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .unwrap()["depends_on"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_str().unwrap().to_string())
                .collect()
        };
        let mut settled = plan.clone();
        let mut actions = Vec::new();
        repair_decision_doc_gates(&mut settled, true, &mut actions);
        assert_eq!(
            deps(&settled, "m"),
            vec!["m-a", "m-b"],
            "the merger waits for its shards; only the decision doc is un-gated"
        );
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(
            actions[0].contains("`m` was gated on docs-only `decisions-doc`")
                && !actions[0].contains("m-a"),
            "{}",
            actions[0]
        );
        assert_eq!(
            deps(&settled, "integrate-verify").len(),
            4,
            "the join owns nothing and keeps every dep"
        );

        // The work-area test alone: strip the split's marker off the shards and the `.swarm/`
        // path still says build artifact, not decision document.
        let mut unmarked = plan.clone();
        for t in unmarked["subtasks"].as_array_mut().unwrap() {
            t.as_object_mut().unwrap().remove("shard_of");
        }
        let mut actions = Vec::new();
        repair_decision_doc_gates(&mut unmarked, true, &mut actions);
        assert_eq!(deps(&unmarked, "m"), vec!["m-a", "m-b"]);
        assert_eq!(actions.len(), 1);

        assert!(is_shipped_doc_file("DECISIONS.md"));
        assert!(is_shipped_doc_file("docs/notes.rst"));
        assert!(!is_shipped_doc_file(".swarm/shards/m/a/README.md"));
        assert!(!is_shipped_doc_file("./.swarm/shards/m/a/README.md"));
        assert!(!is_shipped_doc_file("web/viz.js"));
    }
}
