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
use super::{spec_orientation, EventSink, SpecSection};
use super::{JUDGE_ENDED_NEEDLE, LEDGER_DIR, USER_DECISIONS_HEADER};

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

/// The EXISTING-TREE block both prompt builders share (`research_user_text` here and
/// `decision_user_text` in the decisions module) — one join, so the greenfield/manifest framing
/// cannot drift between the slice lanes and the decision lanes.
pub(super) fn research_tree_block(tree_at_start: &[String]) -> String {
    if tree_at_start.is_empty() {
        "\n\nEXISTING TREE: nothing is on disk yet — a greenfield build.".to_string()
    } else {
        format!(
            "\n\nEXISTING TREE (the files already on disk — read them with your tools before \
             answering anything they could settle):\n{}",
            tree_at_start.join("\n")
        )
    }
}

/// The ONE find+splice loop for a slice's claimed sections. Both consumers — the brief a
/// builder reads (`briefs_from_slices`) and the research prompt (`research_request_block`) —
/// call THIS, so the heading-match rule cannot diverge between them (the digestStreamFields
/// law: one shared join, never a hand-copied loop; the loop had already been duplicated
/// verbatim at both sites).
///
/// A claimed heading that matches NO spec section is a MEASURED absence, never a silent drop:
/// r5's boot slice claimed a typo'd heading and lost 3,501 chars from BOTH its research
/// prompts and its brief, surfacing only through the generic `spec_sections_unclaimed` on the
/// real heading. Each miss emits `slice_claimed_section_unmatched{slice, claimed}` — loud,
/// MILD, never blocks; the matching sections still splice.
pub(super) fn splice_claimed_sections(
    slice_id: &str,
    claimed: &[String],
    sections: &[SpecSection],
    events: &dyn EventSink,
) -> String {
    let mut spliced = String::new();
    for want in claimed {
        let key = want.trim().to_lowercase();
        match sections
            .iter()
            .find(|s| s.heading.trim().to_lowercase() == key)
        {
            Some(sec) => {
                spliced.push_str(&format!("\n### {}\n{}", sec.heading, sec.body.trim()));
            }
            None => {
                events.write_value(serde_json::json!({
                    "event": "slice_claimed_section_unmatched",
                    "slice": slice_id,
                    "claimed": want,
                }));
            }
        }
    }
    spliced
}

/// The per-slice REQUEST block for a research prompt (A5): the prompt NEVER carries the raw ~50k
/// spec when orientation is armed — it carries the orientation index plus the slice's claimed
/// sections' FULL text, the exact splice path `briefs_from_slices` uses. Below the arming floor
/// the whole spec is the better input, exactly as OPEN's own message formation decides it.
pub(super) fn research_request_block(
    spec: &str,
    sections: &[SpecSection],
    armed: bool,
    slice_id: &str,
    claimed: &[String],
    events: &dyn EventSink,
) -> String {
    if !armed {
        return format!("THE REQUEST:\n{spec}");
    }
    let spliced = splice_claimed_sections(slice_id, claimed, sections, events);
    let orientation = spec_orientation(sections);
    if spliced.is_empty() {
        format!(
            "THE REQUEST, AS ITS ORIENTATION INDEX (this slice claimed no sections — the \
             sections' full text lives in the request itself):\n\n{orientation}"
        )
    } else {
        format!(
            "THE REQUEST, AS ITS ORIENTATION INDEX:\n\n{orientation}\n\nTHE SPEC'S OWN SECTIONS \
             FOR THIS SLICE — verbatim, and the authority over any paraphrase:{spliced}"
        )
    }
}

/// One research prompt, assembled from THIS run's facts (the specificity gate): the request
/// block, the owning slice, the USER DECISIONS the ASK handshake resolved (A6 — the fan runs
/// AFTER the handshake so decisions inform research), the tree as the run found it, and the
/// question VERBATIM. Absences are stated, never papered over.
pub(super) fn research_user_text(
    request_block: &str,
    slice_id: &str,
    slice_title: &str,
    slice_objective: &str,
    user_decisions: &str,
    tree_at_start: &[String],
    question: &str,
) -> String {
    let decisions_block = if user_decisions.trim().is_empty() {
        String::new()
    } else {
        // The one USER_DECISIONS_HEADER constant, so the binding framing cannot drift from the
        // copies the spec and every worker prompt carry.
        format!("{USER_DECISIONS_HEADER}{user_decisions}")
    };
    let tree_block = research_tree_block(tree_at_start);
    format!(
        "{request_block}\n\nTHE SLICE THIS QUESTION BELONGS TO:\n{slice_id} — {slice_title}\n\
         {slice_objective}{decisions_block}{tree_block}\n\nTHE QUESTION:\n{question}"
    )
}

pub(super) fn research_system_text() -> String {
    "You are answering ONE question that must be settled before this software is built. Ground \
     your answer: read the request text you were given, read the existing tree's files with your \
     shell and tree tools, and when the request names a documentation URL, fetch it — an answer \
     copied from the real source beats any paraphrase. Do NOT create or edit files: you have no \
     write or edit tool, and your structured reply IS your deliverable.\n\n\
     Your answer is a HANDOFF to the builder: name exact files, exact key/field literals, exact \
     endpoints or signatures where the request implies them; where the request is silent, state \
     the most CONVENTIONAL choice and say it is a convention. If the question cannot be settled \
     from the request or the sources, say exactly that in one line and still name the \
     conventional choice. Keep it under a page.\n\n\
     When you are done, call the final_output tool ONCE with {\"answer\": \"...\", \"raised\": \
     [...]} — `raised` lists further questions you could NOT settle, for the record only: do not \
     answer them, and nothing will dispatch them."
        .to_string()
}
