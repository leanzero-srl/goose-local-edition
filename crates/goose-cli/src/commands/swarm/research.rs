//! The research fan's TERMINAL-ROW cluster: the question identity, the row every dispatched
//! question folds into, and the pure helpers that classify, persist and splice its outcome.
//!
//! Second sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink. Moved
//! verbatim from swarm.rs — behavior unchanged; the WHY of every part stays in each item's own
//! doc. The fan itself (`research_fan`, on `GooseAgentDispatcher`) stays in the root with the
//! other dispatcher methods; what lives here is everything about it that is pure.

use std::path::Path;

use super::{activity_digest_key, head_to_sentence_end, one_lane_per_host, parse_json_lenient};
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
    /// Questions the researcher RAISED but could not settle. NEVER dispatched — research answers
    /// only the opener's own questions (measured r6b: 33 dispatched, 48 raised, 0 of the raised
    /// chased; a fan that chased raises would have doubled a 176-minute phase). Each one is
    /// named by a `research_raised_folded` event (`emit_research_outcome`) and folded VERBATIM
    /// into the owning slice's brief (`raised_questions_brief_block`) for the BUILDER to settle
    /// — before that fold only a count rode `research_answered.raised`, and r6b's 48 raised
    /// questions reached no builder at all.
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

/// A panicked lane is a TERMINAL unanswered outcome like any other miss: the caller writes the
/// mini (the absence is a fact the ledger holds, and on resume it stays settled like every
/// unanswered row), emits through the one outcome funnel, and the brief keeps the raw question.
/// `model` is honestly empty — the lane died before any call attribution — and `secs` is 0 for
/// the same reason. The error head rides at 300, the ledger's last_failure_tail idiom.
pub(super) fn fold_research_panic(q: &ResearchQuestion, error: &str) -> ResearchRow {
    ResearchRow {
        slice: q.slice.clone(),
        q_index: q.q_index,
        question: q.question.clone(),
        status: RESEARCH_UNANSWERED.to_string(),
        answer: String::new(),
        reason: Some("lane_panicked".to_string()),
        detail: Some(error.chars().take(300).collect()),
        raised: Vec::new(),
        model: String::new(),
        secs: 0,
    }
}

/// THE ONE emission site for a terminal row's events (the digestStreamFields law: the fan's
/// lane closure and its panicked-lane arm each carried a verbatim `research_unanswered`
/// writer). `research_answered` / `research_unanswered` exactly as before, then ONE
/// `research_raised_folded` per question the lane raised — the WORDS, not a count: the
/// `raised` count on `research_answered` was the only trace r6b's 48 raised questions left, so
/// tick.py could count them and nobody could read them. `raised_by` is the parent row's durable
/// mini — the primary material an operator opens to read the whole row — and the question rides
/// as a hard 200-char head because this feeds an event, not a model (the head_to_sentence_end
/// rule's own exemption, the same cut `research_dispatched` makes).
pub(super) fn emit_research_outcome(events: &dyn EventSink, row: &ResearchRow) {
    if row.status == RESEARCH_ANSWERED {
        events.write_value(serde_json::json!({
            "event": "research_answered",
            "slice": row.slice,
            "q_index": row.q_index,
            "chars": row.answer.chars().count(),
            "raised": row.raised.len(),
            "secs": row.secs,
            "model": row.model,
        }));
    } else {
        events.write_value(serde_json::json!({
            "event": "research_unanswered",
            "slice": row.slice,
            "q_index": row.q_index,
            "reason": row.reason,
            "detail": row.detail,
            "secs": row.secs,
            "model": row.model,
        }));
    }
    for q in &row.raised {
        events.write_value(serde_json::json!({
            "event": "research_raised_folded",
            "slice": row.slice,
            "q_index": row.q_index,
            "raised_by": research_mini_name(&row.slice, row.q_index),
            "question": q.chars().take(200).collect::<String>(),
        }));
    }
}

/// The brief block for the questions this slice's OWN research lanes raised and nobody chased,
/// so the builder — the only party left who can settle them — sees them (r6b: 48 raised, 0
/// reached a brief). Assembled from facts only: the real count and each raised text verbatim,
/// as a sentence-bounded head at 400 (the ledger block's own per-item budget; the full text is
/// durable in the mini — a render budget, never a cap). Exact duplicates across a slice's rows
/// fold to one line so the count stays honest. A slice whose lanes raised nothing renders
/// NOTHING — no heading, no filler (the specificity gate). `slice_rows` is the caller's
/// per-slice filter (`r.slice == sl.id`), so a decision row can never reach here; rows of
/// EVERY status contribute — an empty_answer row may still have raised.
pub(super) fn raised_questions_brief_block(slice_rows: &[&ResearchRow]) -> String {
    let mut raised: Vec<&str> = Vec::new();
    for r in slice_rows {
        for q in &r.raised {
            let q = q.trim();
            if !q.is_empty() && !raised.contains(&q) {
                raised.push(q);
            }
        }
    }
    if raised.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = raised
        .iter()
        .map(|q| format!("- {}", head_to_sentence_end(q, 400).replace('\n', " ")))
        .collect();
    format!(
        "\n\nOPEN QUESTIONS the research fan raised while answering this slice's questions and \
         did not chase ({}) — settle each while building: where the request speaks, the request \
         decides; where a line below already names a convention, follow it; where nothing does, \
         choose the most CONVENTIONAL option and note the choice in a code comment, exactly as \
         for an open decision:\n{}",
        raised.len(),
        lines.join("\n")
    )
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
     [...]} — `raised` lists further questions you could NOT settle: do not answer them, and \
     nothing will dispatch them; they are handed VERBATIM to the builder of this slice as open \
     points, so phrase each as a decision that builder can make in one line, naming the \
     conventional choice when you have one."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::{briefs_from_slices, NullSink, OpenOutput, OpenSlice, SwarmEvent};
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    fn row(slice: &str, q_index: usize, status: &str, raised: &[&str]) -> ResearchRow {
        ResearchRow {
            slice: slice.to_string(),
            q_index,
            question: format!("{slice} question {q_index}"),
            status: status.to_string(),
            answer: if status == RESEARCH_ANSWERED {
                "Port 8850, from the boot table.".to_string()
            } else {
                String::new()
            },
            reason: (status == RESEARCH_UNANSWERED).then(|| "empty_answer".to_string()),
            detail: None,
            raised: raised.iter().map(|s| s.to_string()).collect(),
            model: "m".to_string(),
            secs: 9,
        }
    }

    /// r6b's shape at the event layer: `research_answered.raised` carried only a COUNT (48 over
    /// the run), so the raised words were invisible outside the minis. The one funnel now names
    /// each raised question with the parent's durable mini as `raised_by`; the panicked-lane arm
    /// rides the same funnel, so a reworded `research_unanswered` can no longer diverge between
    /// the two writers.
    #[test]
    fn every_raised_question_is_named_by_its_own_event_through_the_one_funnel() {
        let sink = ValueSink::default();
        let answered = row(
            "app-boot",
            1,
            RESEARCH_ANSWERED,
            &[
                "Should the wrapper run services in-process instead of as subprocesses?",
                "Exact SIGTERM grace before SIGKILL — chose 5 s by convention.",
            ],
        );
        emit_research_outcome(&sink, &answered);
        let q = ResearchQuestion {
            slice: "viz-field".into(),
            q_index: 3,
            question: "How is the Europe/Berlin day computed?".into(),
        };
        let panicked = fold_research_panic(&q, "task panicked: index out of bounds");
        assert_eq!(panicked.status, RESEARCH_UNANSWERED);
        assert_eq!(panicked.reason.as_deref(), Some("lane_panicked"));
        assert!(panicked.model.is_empty() && panicked.secs == 0);
        assert_eq!(
            panicked.detail.as_deref(),
            Some("task panicked: index out of bounds")
        );
        emit_research_outcome(&sink, &panicked);
        let events = sink.0.lock().unwrap();
        let names: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "research_answered",
                "research_raised_folded",
                "research_raised_folded",
                "research_unanswered",
            ],
            "one named event per raised question, none for a row that raised nothing"
        );
        assert_eq!(
            events[0]["raised"], 2,
            "the count still rides research_answered"
        );
        assert_eq!(events[1]["slice"], "app-boot");
        assert_eq!(events[1]["q_index"], 1);
        assert_eq!(
            events[1]["raised_by"], "research-app-boot-q1.json",
            "raised_by is the parent row's durable mini, derived from the row itself"
        );
        assert_eq!(
            events[2]["question"],
            "Exact SIGTERM grace before SIGKILL — chose 5 s by convention."
        );
        assert_eq!(events[3]["reason"], "lane_panicked");
    }

    /// The fold into the brief: the owning slice's brief carries the heading with the REAL
    /// count (exact duplicates folded, an empty_answer row's raises included), placed after the
    /// QUESTIONS partition; a slice whose lanes raised nothing renders no heading at all; a
    /// decision row's raises never leak into any slice's brief (the caller's `r.slice == sl.id`
    /// filter is the only door).
    #[test]
    fn raised_questions_fold_into_the_owning_brief_with_the_real_count_and_nowhere_else() {
        let opened = OpenOutput {
            slices: vec![
                OpenSlice {
                    id: "api".into(),
                    title: "the api".into(),
                    objective: "serve GET /health".into(),
                    questions: vec!["which port".into(), "which storage".into()],
                    weight: 3,
                    sections: Vec::new(),
                },
                OpenSlice {
                    id: "web".into(),
                    title: "the console".into(),
                    objective: "render the table".into(),
                    questions: vec!["which filter params".into()],
                    weight: 2,
                    sections: Vec::new(),
                },
            ],
            open_decisions: Vec::new(),
        };
        let rows = vec![
            row(
                "api",
                0,
                RESEARCH_ANSWERED,
                &[
                    "Whether each role may hold multiple bearer tokens — assumed one.",
                    "Whether each role may hold multiple bearer tokens — assumed one.",
                ],
            ),
            row(
                "api",
                1,
                RESEARCH_UNANSWERED,
                &["Journal mode (WAL vs rollback) is not specified — WAL as convention."],
            ),
            row("web", 0, RESEARCH_ANSWERED, &[]),
            row(
                "__open_decisions__",
                0,
                RESEARCH_ANSWERED,
                &["D3: empty-with-progress or blocking loader before the first sync?"],
            ),
        ];
        let briefs = briefs_from_slices(&opened, "build the app", &rows, &[], &NullSink);
        let api = &briefs[0].brief;
        let heading = "OPEN QUESTIONS the research fan raised while answering this slice's \
                       questions and did not chase (2)";
        assert!(
            api.contains(heading),
            "the real count, duplicates folded:\n{api}"
        );
        assert!(api.contains("- Whether each role may hold multiple bearer tokens — assumed one."));
        assert!(
            api.contains("- Journal mode (WAL vs rollback) is not specified — WAL as convention.")
        );
        assert_eq!(
            api.matches("multiple bearer tokens").count(),
            1,
            "an exact duplicate raise renders once"
        );
        assert!(
            api.find("QUESTIONS this slice must settle").unwrap() < api.find(heading).unwrap(),
            "the raised block follows the slice's own open questions"
        );
        assert!(
            !api.contains("D3:"),
            "a decision row's raise is not this slice's"
        );
        let web = &briefs[1].brief;
        assert!(
            !web.contains("OPEN QUESTIONS the research fan raised"),
            "a slice whose lanes raised nothing gets no heading and no filler:\n{web}"
        );
        assert!(!web.contains("D3:"));
        assert!(raised_questions_brief_block(&[]).is_empty());
    }
}
