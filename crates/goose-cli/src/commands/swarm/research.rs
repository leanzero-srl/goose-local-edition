//! The research fan's TERMINAL-ROW cluster: the question identity, the row every dispatched
//! question folds into, and the pure helpers that classify, persist and splice its outcome.
//!
//! Second sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink. Moved
//! verbatim from swarm.rs — behavior unchanged; the WHY of every part stays in each item's own
//! doc. The fan itself (`research_fan`, on `GooseAgentDispatcher`) stays in the root with the
//! other dispatcher methods; what lives here is everything about it that is pure.

use std::path::Path;

use super::{activity_digest_key, one_lane_per_host, parse_json_lenient};
use super::{JUDGE_ENDED_NEEDLE, LEDGER_DIR};

/// One opener question, addressed by (slice, q_index) — the identity the mini filename, the
/// activity key and the brief partition all share.
#[derive(Clone, Debug)]
pub(super) struct ResearchQuestion {
    pub(crate) slice: String,
    pub(crate) q_index: usize,
    pub(crate) question: String,
}

pub(super) const RESEARCH_ANSWERED: &str = "answered";
pub(super) const RESEARCH_UNANSWERED: &str = "unanswered";

/// One terminal research outcome. `status` is always set — answered or unanswered — which is what
/// makes "every dispatched question terminal" a property of the type rather than of a clock.
/// `secs` is a provenance measurement OUTPUT (how long the answer took), never an input: no time
/// value bounds the call (gate 5 is structural — `run_agent_timed_at` carries no time parameter).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResearchRow {
    pub(crate) slice: String,
    pub(crate) q_index: usize,
    pub(crate) question: String,
    /// "answered" | "unanswered" — both TERMINAL. Nothing retries; a miss flows to REPAIR.
    pub(crate) status: String,
    pub(crate) answer: String,
    /// On unanswered: "provider_error" | "empty_answer" | "judge_ended".
    #[serde(default)]
    pub(crate) reason: Option<String>,
    #[serde(default)]
    pub(crate) detail: Option<String>,
    /// Questions the researcher RAISED but could not settle — recorded here for the operator,
    /// NEVER dispatched (research dispatches only the opener's own questions).
    #[serde(default)]
    pub(crate) raised: Vec<String>,
    pub(crate) model: String,
    pub(crate) secs: u64,
}

/// The structured deliverable (A1): `{answer, raised}`. Declaring a `Response` is what arms the
/// judge's whole ladder for these lanes — `wants_structured_reply` becomes true, the
/// `recipe__final_output` tool exists, and (with `may_terminate: true`) the `judge_out_of_moves`
/// ending is reachable — the progress-based terminator that makes "all questions terminal"
/// reachable without any clock. Only `answer` is required (the permissive-schema lesson from
/// `review_patch_schema`): `raised` legitimately defaults to empty, and an empty `answer` is
/// classified honestly as unanswered/empty_answer rather than rejected at validation.
pub(super) fn research_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answer"],
        "properties": {
            "answer": {"type": "string"},
            "raised": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(super) fn research_mini_name(slice: &str, q_index: usize) -> String {
    format!("research-{}-q{}.json", activity_digest_key(slice), q_index)
}

/// The resume watermark: a mini that parses IS the question's terminal outcome and is never
/// re-dispatched (an unanswered row stays unanswered on resume — revival would be an explicit
/// engine decision, never a silent retry). A missing or corrupt mini means "not yet researched",
/// so re-dispatching is the honest action, not a substitution — and a corrupt mini is already
/// named loudly by `rebuild_ledger_rollup`'s rows_dropped.
pub(super) fn load_research_mini(root: &Path, slice: &str, q_index: usize) -> Option<ResearchRow> {
    let p = root
        .join(LEDGER_DIR)
        .join(research_mini_name(slice, q_index));
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// The fan's lane pool (A3): ONE lane per host, never the raw slot-expanded pool — two calls
/// stacked on one host degrade each other (F623; measured r16 vs r17: the same detail fan cleared
/// 14/14 on 4 slots, then dropped three details when a host's lanes doubled the fan width).
/// `fanout_over_fleet` then applies `order_fleet_by_speed`, so the weight-4 host is first in line
/// instead of structurally last (r13).
pub(super) fn research_fan_lanes(slot_models: Vec<String>) -> Vec<String> {
    one_lane_per_host(slot_models)
}

/// A per-answer SPLICE budget on the text carried into a brief — the measured-good brief size
/// (~1,500 chars: the RESEARCH size table above `synthesize_plan` measured a 1,497-char brief at
/// 88.7% against a 6,443-char median that lost). A render budget on splice text, never a cap on
/// model work: the FULL answer is durable in the ledger mini, and the cut lands on a line
/// boundary and says so (F196 — a cut that ends mid-line reads as a complete fact that is wrong).
pub(super) fn budget_research_answer(answer: &str, slice: &str, q_index: usize) -> String {
    let budget = 1_500;
    if answer.chars().count() <= budget {
        return answer.trim_end().to_string();
    }
    let head: String = answer.chars().take(budget).collect();
    let whole = head.rsplit_once('\n').map(|(h, _)| h).unwrap_or(&head);
    format!(
        "{}\n… ANSWER TRUNCATED — full text in .swarm/ledger/{}",
        whole.trim_end(),
        research_mini_name(slice, q_index)
    )
}

/// Fold one lane's outcome into a TERMINAL row. Pure, so the classification is testable without
/// a model: Ok with a non-empty answer => answered; Ok with an empty/unparseable reply =>
/// unanswered/empty_answer (never a stub answer — the fallback gate); Err from the
/// `judge_out_of_moves` ending => unanswered/judge_ended; any other Err =>
/// unanswered/provider_error with the error head (300, the ledger's last_failure_tail idiom).
pub(super) fn fold_research_outcome(
    q: &ResearchQuestion,
    model: &str,
    secs: u64,
    out: Result<String, String>,
) -> ResearchRow {
    #[derive(serde::Deserialize, Default)]
    struct ResearchReply {
        #[serde(default)]
        answer: String,
        #[serde(default)]
        raised: Vec<String>,
    }
    let mut row = ResearchRow {
        slice: q.slice.clone(),
        q_index: q.q_index,
        question: q.question.clone(),
        status: RESEARCH_UNANSWERED.to_string(),
        answer: String::new(),
        reason: None,
        detail: None,
        raised: Vec::new(),
        model: model.to_string(),
        secs,
    };
    match out {
        Ok(raw) => match parse_json_lenient::<ResearchReply>(&raw) {
            Some(reply) if !reply.answer.trim().is_empty() => {
                row.status = RESEARCH_ANSWERED.to_string();
                row.answer = reply.answer;
                row.raised = reply.raised;
            }
            Some(reply) => {
                // Parsed, but the deliverable slot is blank — a named absence, never a stub.
                row.reason = Some("empty_answer".to_string());
                row.raised = reply.raised;
            }
            None => {
                // Nothing parseable in the reply. The head rides in `detail` so the operator
                // can see WHAT came back instead of an answer (300, the last_failure_tail
                // idiom) — the absence stays loud, nothing is substituted.
                row.reason = Some("empty_answer".to_string());
                row.detail = Some(raw.chars().take(300).collect());
            }
        },
        Err(e) => {
            // The one engine terminator's own words (emitted at exactly one site, the
            // judge_out_of_moves ending): a lane the ENGINE ended is named as such, not
            // laundered into a transport failure.
            if e.contains(JUDGE_ENDED_NEEDLE) {
                row.reason = Some("judge_ended".to_string());
            } else {
                row.reason = Some("provider_error".to_string());
            }
            row.detail = Some(e.chars().take(300).collect());
        }
    }
    row
}
