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
//!   (c) workers receive research-settled answers under `PLAN_SETTLED_DECISIONS_HEADER` appended
//!       to `DispatchRequest.user_decisions` — NEVER under `USER_DECISIONS_HEADER`, whose text
//!       ("The user was ASKED and chose") would be a GEN-4 overclaim for research answers;
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

/// The provenance header for research-settled decisions on the worker channel. Deliberately NOT
/// `USER_DECISIONS_HEADER`: these answers were researched from the request AFTER the user declined
/// to answer, and a header claiming the user chose them would be the exact overclaim gate GEN-4
/// exists to refuse. sb-7 fails "a document that contradicts observed behavior" — so the framing
/// is binding-for-consistency, subordinate to the request and to real user decisions.
pub(super) const PLAN_SETTLED_DECISIONS_HEADER: &str =
    "\n\n## DECISIONS SETTLED AT PLAN TIME — BINDING CONVENTIONS\n\
     Settled at plan time by research from the request; the user was asked and did not answer \
     these. Each answer is drawn strictly from the request, or is the named CONVENTIONAL choice \
     where the request is silent. They are conventions, BINDING FOR CONSISTENCY — implement each \
     as written so every module makes the same choice. They never override the request itself or \
     a USER DECISIONS block:\n";

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
            "\n\nDECISIONS SETTLED AT PLAN TIME — quoted verbatim and BINDING; implement each \
             exactly as written and never substitute your own convention:\n{settled}"
        ));
    }
    if !open.is_empty() {
        out.push_str(&format!(
            "\n\nOPEN DECISIONS — unless a USER DECISIONS block in the request settles one of \
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

/// The worker-channel block (amendment c): research-settled Q/A pairs only, appended under
/// `PLAN_SETTLED_DECISIONS_HEADER` to `DispatchRequest.user_decisions` — the channel already
/// verified verbatim-to-every-worker at the four dispatch sites. User-settled decisions are NOT
/// repeated here: they already ride under `USER_DECISIONS_HEADER` in the spec and the same
/// user_decisions channel, in the user's own words.
pub(super) fn research_settled_worker_block(decisions: &[PlanDecision]) -> String {
    let mut out = String::new();
    for d in decisions {
        if let DecisionState::SettledByResearch { answer } = &d.state {
            out.push_str(&format!(
                "Q: {}\nA: {}\n",
                d.question.trim(),
                budget_research_answer(answer, DECISION_SLICE, d.q_index)
            ));
        }
    }
    out
}

/// One decision lane's prompt HEAD (the fan adds the snowball block and the decision text under
/// THE OPEN DECISION at dispatch through `research_user_text`, exactly as the slice path does
/// for its question). The full request rides whole: a decision is global — no claimed-section
/// subset exists to splice — and "answer strictly from the request" requires the request.
/// User-settled decisions ride under the ONE `USER_DECISIONS_HEADER` constant so settled
/// choices inform the still-open ones; the SOURCES block is the same one every slice lane gets.
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

/// Amendment (e), gate-6 class (loud, MILD, rides `plan_repaired.actions`): when EVERY open
/// decision folded as settled at plan time, an implementation task no longer needs a docs-only
/// task upstream for decision consistency — the settled choices ride every brief and every
/// worker prompt — so those edges are stripped and the width the r5 plan lost comes back
/// (post-skeleton width 2 -> 4 on r5's shape, which saturates 3 nodes). One unanswered decision
/// KEEPS every such dep: the doc task is then the consistency mechanism, and serializing behind
/// it is the honest price of an unsettled choice. Docs-only = non-sink, owns at least one file,
/// every owned file documentation (.md/.rst/.txt). The sink keeps its deps (it owns nothing and
/// must wait for everything); doc-on-doc deps are untouched.
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
    let is_doc = |f: &str| {
        let l = f.to_lowercase();
        l.ends_with(".md") || l.ends_with(".rst") || l.ends_with(".txt")
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
    let doc_only_ids: std::collections::HashSet<String> = subtasks
        .iter()
        .filter(|t| {
            let files = files_of(t);
            t.get("id").and_then(|i| i.as_str()) != Some(goose_swarm::SINK_ID)
                && !files.is_empty()
                && files.iter().all(|f| is_doc(f))
        })
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
        if !files_of(t).iter().any(|f| !is_doc(f)) {
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
        // Worker channel: research-settled only, and NEVER under the user header's overclaim.
        let w = research_settled_worker_block(&p);
        assert!(w.contains("Q: which port") && w.contains("A: 8000"));
        assert!(
            !w.contains("sqlite"),
            "user answers already ride the user header"
        );
        assert!(!PLAN_SETTLED_DECISIONS_HEADER.contains("chose"));
        assert!(PLAN_SETTLED_DECISIONS_HEADER.contains("did not answer"));
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
}
