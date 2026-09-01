//! The research fan's TERMINAL-ROW cluster: the question identity, the row every dispatched
//! question folds into, and the pure helpers that classify, persist and splice its outcome.
//!
//! Second sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink. Moved
//! verbatim from swarm.rs — behavior unchanged; the WHY of every part stays in each item's own
//! doc. The fan itself (`research_fan`, on `GooseAgentDispatcher`) stays in the root with the
//! other dispatcher methods; what lives here is everything about it that is pure.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::decisions::{self, DecisionState, PlanDecision, DECISION_SLICE};
use super::findings::FINDING_PATH_EXTS;
use super::opener::{OpenOutput, OpenQuestion, QuestionKind};
use super::orientation::{children_of, heading_key, top_level};
use super::research_plan::{content_words, decision_ids};
use super::spec_surface::{path_token_named, spec_surface_rows};
use super::{activity_digest_key, head_to_sentence_end, one_lane_per_host, parse_json_lenient};
use super::{orientation_armed, spec_sections, SliceBrief};
use super::{phase_banner, spec_orientation, spec_vendor, write_forming_atomic};
use super::{EventSink, SpecSection};
use super::{JUDGE_ENDED_NEEDLE, LEDGER_DIR, USER_DECISIONS_HEADER};

/// One opener question, addressed by (slice, q_index) — the identity the mini filename, the
/// activity key and the brief partition all share. `kind`/`cite`/`fact` are the opener's own
/// words about it (the question contract in opener.rs), carried so the row can record them.
#[derive(Clone, Debug)]
pub(super) struct ResearchQuestion {
    pub(crate) slice: String,
    pub(crate) q_index: usize,
    pub(crate) question: String,
    pub(crate) kind: QuestionKind,
    pub(crate) cite: String,
    pub(crate) fact: String,
}

impl ResearchQuestion {
    pub(super) fn of(slice: &str, q_index: usize, q: &OpenQuestion) -> Self {
        Self {
            slice: slice.to_string(),
            q_index,
            question: q.text.clone(),
            kind: q.kind,
            cite: q.cite.clone(),
            fact: q.fact.clone(),
        }
    }

    /// A decision the user left open, riding the fan under `DECISION_SLICE` (decisions.rs). Its
    /// kind is `design` by construction — a decision is a choice the request leaves open.
    pub(super) fn decision(q_index: usize, line: &str) -> Self {
        Self {
            slice: DECISION_SLICE.to_string(),
            q_index,
            question: line.to_string(),
            kind: QuestionKind::Design,
            cite: String::new(),
            fact: String::new(),
        }
    }
}

pub(super) const RESEARCH_ANSWERED: &str = "answered";
pub(super) const RESEARCH_UNANSWERED: &str = "unanswered";

/// `ResearchRow.origin` for a row the OPENER settled by citing the request (no lane ran). The
/// empty origin is a lane's own answer — every pre-cut mini reads that way.
pub(super) const ORIGIN_SPEC_FACT: &str = "spec_fact";
/// `ResearchRow.origin` prefix for a row COVERED by an earlier-landed mini of another question
/// (`research_plan::covering_mini`): `covered:<the original mini's file name>`.
pub(super) const ORIGIN_COVERED_PREFIX: &str = "covered:";

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
    /// The opener's kind for the question (`QuestionKind::as_str`); "" on a pre-cut mini.
    #[serde(default)]
    pub(crate) kind: String,
    /// The request line/heading the opener cited for it; "" when it named none.
    #[serde(default)]
    pub(crate) cite: String,
    /// "" = a lane answered it (every pre-cut mini); `ORIGIN_SPEC_FACT` = the opener's cited
    /// fact, no lane ran. Read by `briefs_from_slices` to render a fact under its own heading.
    #[serde(default)]
    pub(crate) origin: String,
    /// C3: how many questions the lane that produced this row answered in the SAME session —
    /// `secs` is that session's whole wall time, shared by every row of the batch, never a
    /// per-question split (a split would be a fabricated number). 0 on a pre-cut mini, a spec
    /// fact or a covered row (no lane).
    #[serde(default)]
    pub(crate) batch: usize,
}

impl ResearchRow {
    /// A SPEC FACT as a terminal row: the opener answered the question from the request and cited
    /// where. `status` is answered — the brief, the ledger block and the snowball all read it as
    /// settled — while `origin` says NO LANE RAN, so `research_answered` is never emitted for it
    /// (the per-question dispatched/answered accounting stays a lane count). `model` is empty
    /// and `secs` 0 honestly: nothing was called.
    pub(super) fn spec_fact(q: &ResearchQuestion) -> Self {
        Self {
            slice: q.slice.clone(),
            q_index: q.q_index,
            question: q.question.clone(),
            status: RESEARCH_ANSWERED.to_string(),
            answer: q.fact.clone(),
            reason: None,
            detail: None,
            raised: Vec::new(),
            model: String::new(),
            secs: 0,
            kind: q.kind.as_str().to_string(),
            cite: q.cite.clone(),
            origin: ORIGIN_SPEC_FACT.to_string(),
            batch: 0,
        }
    }

    /// C2(b): `q` answered by an earlier-landed row of another question (`covering_mini`, rule
    /// named). The answer is COPIED — the brief's Q/A shape and the ledger block read it like
    /// any settled row — and `origin` names the ORIGINAL mini (a copy of a copy resolves to the
    /// first), so provenance is one hop everywhere. The cover's `raised` stay with the cover's
    /// slice; `secs` is 0 (nothing was called); `model` is the cover's, the one that answered.
    pub(super) fn covered_by(q: &ResearchQuestion, cover: &ResearchRow, _rule: &str) -> Self {
        let original = match cover.origin.strip_prefix(ORIGIN_COVERED_PREFIX) {
            Some(m) => m.to_string(),
            None => research_mini_name(&cover.slice, cover.q_index),
        };
        // A FACT cover (no lane ran — `model` is empty on a spec-fact row and on every copy of
        // one) hands over ITS cite: the request line the opener read is the provenance of the
        // answer, and the brief's VIA line quotes it (VA-030 D10-7). A lane cover keeps the
        // covered question's own cite — the lane's answer is the provenance there.
        let cite = if cover.model.is_empty() {
            cover.cite.clone()
        } else {
            q.cite.clone()
        };
        Self {
            slice: q.slice.clone(),
            q_index: q.q_index,
            question: q.question.clone(),
            status: RESEARCH_ANSWERED.to_string(),
            answer: cover.answer.clone(),
            reason: None,
            detail: None,
            raised: Vec::new(),
            model: cover.model.clone(),
            secs: 0,
            kind: q.kind.as_str().to_string(),
            cite,
            origin: format!("{ORIGIN_COVERED_PREFIX}{original}"),
            batch: 0,
        }
    }
}

/// The structured deliverable (A1, batched by C3): `{answers: [{question_index, answer,
/// raised}]}` — ONE lane answers all of a slice's remaining questions in one session and the
/// ledger still gets one mini per question (`fold_research_batch` keys each entry by its
/// `[qN]` tag). Declaring a `Response` is what arms the judge's whole ladder for these lanes —
/// `wants_structured_reply` becomes true, the `recipe__final_output` tool exists, and (with
/// `may_terminate: true`) the `judge_out_of_moves` ending is reachable — the progress-based
/// terminator that makes "all questions terminal" reachable without any clock. `raised`
/// legitimately defaults to empty (the permissive-schema lesson from `review_patch_schema`); an
/// empty or missing `answer` is classified honestly as unanswered/empty_answer rather than
/// rejected at validation.
pub(super) fn research_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answers"],
        "properties": {
            "answers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["question_index", "answer"],
                    "properties": {
                        "question_index": {"type": "integer"},
                        "answer": {"type": "string"},
                        "raised": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
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

/// One entry of a lane's batched reply. `question_index` is the `[qN]` tag the prompt put on the
/// question; absent only on the pre-C3 single shape, which a ONE-question batch still accepts.
#[derive(serde::Deserialize, Default)]
struct BatchAnswer {
    #[serde(default)]
    question_index: Option<usize>,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    raised: Vec<String>,
}

/// An entry whose `question_index` is not one of the lane's tags: never silently dropped — the
/// fan names it (`research_batch_stray_answer`) with the tag and the answer's head.
#[derive(Clone, Debug)]
pub(super) struct StrayAnswer {
    pub(super) question_index: Option<usize>,
    pub(super) answer_head: String,
}

fn unanswered_row(q: &ResearchQuestion, model: &str, secs: u64, batch: usize) -> ResearchRow {
    ResearchRow {
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
        kind: q.kind.as_str().to_string(),
        cite: q.cite.clone(),
        origin: String::new(),
        batch,
    }
}

/// Fold ONE lane's outcome — a whole slice's batch — into one TERMINAL row PER QUESTION. Pure,
/// so the classification is testable without a model. Ok + a parseable `{answers: [...]}`:
/// each question takes the entry whose `question_index` is its tag — non-empty => answered,
/// blank => unanswered/empty_answer, no entry => unanswered/empty_answer with the fact stated
/// in `detail` ("no entry for [qN]"); an entry with a tag the batch does not carry comes back
/// as a `StrayAnswer` for the fan to name. Ok + the pre-C3 single `{answer, raised}` shape:
/// accepted for a ONE-question batch (that is exactly what it means), and for a larger batch
/// every question is unanswered/empty_answer with `detail` saying the lane answered N
/// questions with one entry — never a guess at which. Ok + nothing parseable: every question
/// unanswered/empty_answer with the raw head in `detail` (300, the last_failure_tail idiom).
/// Err from the `judge_out_of_moves` ending => every question unanswered/judge_ended; any other
/// Err => unanswered/provider_error with the error head. `secs` is the session's wall time on
/// every row, `batch` its size (the row doc says why it is not split).
pub(super) fn fold_research_batch(
    qs: &[ResearchQuestion],
    model: &str,
    secs: u64,
    out: Result<String, String>,
) -> (Vec<ResearchRow>, Vec<StrayAnswer>) {
    #[derive(serde::Deserialize, Default)]
    struct BatchReply {
        #[serde(default)]
        answers: Vec<BatchAnswer>,
    }
    let n = qs.len();
    let mut rows: Vec<ResearchRow> = qs
        .iter()
        .map(|q| unanswered_row(q, model, secs, n))
        .collect();
    let mut strays = Vec::new();
    let raw = match out {
        Ok(raw) => raw,
        Err(e) => {
            // The one engine terminator's own words (emitted at exactly one site, the
            // judge_out_of_moves ending): a lane the ENGINE ended is named as such, not
            // laundered into a transport failure.
            let reason = if e.contains(JUDGE_ENDED_NEEDLE) {
                "judge_ended"
            } else {
                "provider_error"
            };
            for row in &mut rows {
                row.reason = Some(reason.to_string());
                row.detail = Some(e.chars().take(300).collect());
            }
            return (rows, strays);
        }
    };
    let entries: Vec<BatchAnswer> = match parse_json_lenient::<BatchReply>(&raw) {
        Some(reply) if !reply.answers.is_empty() => reply.answers,
        _ => match parse_json_lenient::<BatchAnswer>(&raw) {
            // The pre-C3 single shape, or a batch reply whose `answers` is empty and that also
            // carries a top-level `answer`: for one question it IS the answer to that question.
            Some(single) if n == 1 && !single.answer.trim().is_empty() => vec![BatchAnswer {
                question_index: Some(qs[0].q_index),
                ..single
            }],
            Some(single) if !single.answer.trim().is_empty() => {
                for row in &mut rows {
                    row.reason = Some("empty_answer".to_string());
                    row.detail = Some(format!(
                        "the lane answered {n} questions with ONE {{answer}} entry and no \
                         question_index — not attributed to any of them; head: {}",
                        single.answer.chars().take(200).collect::<String>()
                    ));
                }
                return (rows, strays);
            }
            _ => {
                // Nothing parseable in the reply. The head rides in `detail` so the operator
                // can see WHAT came back instead of answers — the absence stays loud, nothing
                // is substituted.
                for row in &mut rows {
                    row.reason = Some("empty_answer".to_string());
                    row.detail = Some(raw.chars().take(300).collect());
                }
                return (rows, strays);
            }
        },
    };
    let mut seen: Vec<usize> = Vec::new();
    for entry in entries {
        let Some(slot) = entry
            .question_index
            .and_then(|i| rows.iter().position(|r| r.q_index == i))
            .filter(|p| !seen.contains(p))
        else {
            strays.push(StrayAnswer {
                question_index: entry.question_index,
                answer_head: entry.answer.chars().take(200).collect(),
            });
            continue;
        };
        seen.push(slot);
        let row = &mut rows[slot];
        if entry.answer.trim().is_empty() {
            // Parsed, but the deliverable slot is blank — a named absence, never a stub.
            row.reason = Some("empty_answer".to_string());
            row.raised = entry.raised;
        } else {
            row.status = RESEARCH_ANSWERED.to_string();
            row.answer = entry.answer;
            row.raised = entry.raised;
        }
    }
    for row in rows
        .iter_mut()
        .filter(|r| r.status != RESEARCH_ANSWERED && r.reason.is_none())
    {
        row.reason = Some("empty_answer".to_string());
        row.detail = Some(format!(
            "the lane's reply carried no entry for [q{}]",
            row.q_index
        ));
    }
    (rows, strays)
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
        kind: q.kind.as_str().to_string(),
        cite: q.cite.clone(),
        origin: String::new(),
        batch: 0,
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
            "batch": row.batch,
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
/// MILD, never blocks; the matching sections still splice. Both sides are compared on
/// `heading_key` — decoration folds (r6d: "vs7dbg — REQUIRED and graded" claimed against
/// "#### `vs7dbg` — REQUIRED and graded" missed twice), letters do not.
pub(super) fn splice_claimed_sections(
    slice_id: &str,
    claimed: &[String],
    sections: &[SpecSection],
    events: &dyn EventSink,
) -> String {
    let mut spliced = String::new();
    for want in claimed {
        let key = heading_key(want);
        match sections.iter().find(|s| heading_key(&s.heading) == key) {
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

/// The routes a section's endpoint table ADVERTISES, as base paths: `spec_surface_rows` (the one
/// table parser) over the section's own body, each row's path cut at its query and at its
/// first template segment (`/api/drafts/<id>/submit` → `/api/drafts`), trailing slash dropped.
/// The bare `/` is not a route anyone "calls into" by name.
fn advertised_paths(sec: &SpecSection) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, row) in spec_surface_rows(&sec.body).rows {
        let Some(path) = row.split_whitespace().nth(1) else {
            continue;
        };
        let base = path
            .split(['?', '<', '{'])
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        if base.starts_with('/') && base.len() > 1 && !out.iter().any(|p| p == base) {
            out.push(base.to_string());
        }
    }
    out
}

/// The sections a slice CONSUMES without owning them (VA-008), rendered in the same
/// `### heading\nbody` shape as its own splice, plus the sections that bind every slice.
pub(super) struct ConsumedSections {
    /// Rules (a) and (b): the spec's own text for what this slice reaches into.
    pub(super) called_into: String,
    /// Rule (c): the cross-cutting top-level sections, for every slice that did not claim them.
    pub(super) cross_cutting: String,
}

/// SPEC SECTIONS ROUTE TO CONSUMERS, not only to owners — THE ONE helper beside
/// `splice_claimed_sections`, called by both the brief (`briefs_from_slices`) and the research
/// prompt (`research_request_block`), so the routing rule cannot diverge between them.
///
/// WHY (r6c, the product-killer, archive local-sb7-swarm-r6c-FINISHED-0.1420-…-build-608m):
/// the opener claimed a perfect 28-section PARTITION (0 overlaps) and the splice gave each
/// slice only its OWN claims. `#### Endpoints` (4,927 chars — the `sort` values at
/// request.md:148-149: `created_at`, `-created_at`, `amount_minor`, `-amount_minor`) went to
/// ledgerd-api alone; web-console (claims: §7, §9) shipped `sort=date_desc`, api.py answered
/// 400, `apiFetch` saw `r.ok` false and the table showed zero rows for the whole run. `Data →
/// scene` and `Streaming diffs` — `####` children of web-viz's claimed §8 — went to ledgerd-api;
/// the four cross-cutting `##` sections (What WILL happen / Consistency rules / Performance
/// budgets / Rules — 7,698 chars) went to ledgerd-core only. Three rules, each derived from the
/// document's own structure and this slice's own claims — no budget on any of it, and a section
/// already in the slice's claims is never spliced twice:
///
/// (a) ADVERTISED ROUTE — a section whose endpoint table advertises a path that the slice's
///     claimed bodies or declared files name (token-bounded) is called into by the slice. The
///     cross-cutting sections are excluded from the slice's vocabulary: under (c) they belong to
///     everyone, so one slice's claim on the graded schedule does not make it the caller of
///     every route the schedule exercises.
/// (b) CHILD OF A CLAIMED PARENT — the descendants of a claimed section BELOW the top level
///     (a `###` component's `####` details). A claimed top-level grouping (`## What to build`)
///     inherits nothing: its children are the components other slices own.
/// (c) CROSS-CUTTING — a top-level section with no children, claimed by at most one slice, in a
///     document where some top-level section HAS children (a flat document with only top-level
///     sections has no rules-vs-components distinction to read and broadcasts nothing).
///
/// Each rule that fires emits `spec_sections_consumed{slice, rule, sections}` beside the
/// existing `spec_sections_unclaimed`, so the tick can read where every section went.
pub(super) fn consumed_spec_sections(
    slice_id: &str,
    claimed: &[String],
    files: &[String],
    every_claim: &[&[String]],
    sections: &[SpecSection],
    events: &dyn EventSink,
) -> ConsumedSections {
    let index_of = |heading: &str| {
        let key = heading_key(heading);
        sections.iter().position(|s| heading_key(&s.heading) == key)
    };
    let own: BTreeSet<usize> = claimed.iter().filter_map(|h| index_of(h)).collect();
    let top = top_level(sections);
    let cross: BTreeSet<usize> = match top {
        Some(top)
            if sections
                .iter()
                .enumerate()
                .any(|(i, s)| s.level == top && !children_of(sections, i).is_empty()) =>
        {
            let mut claim_count: std::collections::HashMap<usize, usize> = Default::default();
            for claims in every_claim {
                for h in claims.iter() {
                    if let Some(i) = index_of(h) {
                        *claim_count.entry(i).or_insert(0) += 1;
                    }
                }
            }
            sections
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    s.level == top
                        && children_of(sections, *i).is_empty()
                        && claim_count.get(i).copied().unwrap_or(0) <= 1
                })
                .map(|(i, _)| i)
                .collect()
        }
        _ => BTreeSet::new(),
    };
    let mut vocabulary = String::new();
    for i in own.iter().filter(|i| !cross.contains(i)) {
        vocabulary.push_str(&sections[*i].body);
        vocabulary.push('\n');
    }
    for f in files {
        vocabulary.push_str(f);
        vocabulary.push('\n');
    }
    let mut by_route: Vec<usize> = Vec::new();
    for (i, sec) in sections.iter().enumerate() {
        if own.contains(&i) || cross.contains(&i) {
            continue;
        }
        if advertised_paths(sec)
            .iter()
            .any(|p| path_token_named(p, &vocabulary))
        {
            by_route.push(i);
        }
    }
    let mut by_parent: Vec<usize> = Vec::new();
    if let Some(top) = top {
        for parent in own.iter().filter(|i| sections[**i].level > top) {
            for child in children_of(sections, *parent) {
                if !own.contains(&child)
                    && !cross.contains(&child)
                    && !by_route.contains(&child)
                    && !by_parent.contains(&child)
                {
                    by_parent.push(child);
                }
            }
        }
    }
    let broadcast: Vec<usize> = cross.iter().copied().filter(|i| !own.contains(i)).collect();
    let render = |ids: &[usize]| {
        let mut ordered = ids.to_vec();
        ordered.sort_unstable();
        ordered
            .iter()
            .map(|i| {
                format!(
                    "\n### {}\n{}",
                    sections[*i].heading,
                    sections[*i].body.trim()
                )
            })
            .collect::<String>()
    };
    for (rule, ids) in [
        ("advertised_route", &by_route),
        ("child_of_claimed", &by_parent),
        ("cross_cutting", &broadcast),
    ] {
        if !ids.is_empty() {
            events.write_value(serde_json::json!({
                "event": "spec_sections_consumed",
                "slice": slice_id,
                "rule": rule,
                "sections": ids.iter().map(|i| sections[*i].heading.clone()).collect::<Vec<_>>(),
            }));
        }
    }
    let mut called: Vec<usize> = by_route;
    called.extend(by_parent);
    ConsumedSections {
        called_into: render(&called),
        cross_cutting: render(&broadcast),
    }
}

/// The per-slice REQUEST block for a research prompt (A5): the prompt NEVER carries the raw ~50k
/// spec when orientation is armed — it carries the orientation index plus the slice's claimed
/// sections' FULL text, the exact splice path `briefs_from_slices` uses. Below the arming floor
/// the whole spec is the better input, exactly as OPEN's own message formation decides it.
#[allow(clippy::too_many_arguments)]
pub(super) fn research_request_block(
    spec: &str,
    sections: &[SpecSection],
    armed: bool,
    slice_id: &str,
    claimed: &[String],
    files: &[String],
    every_claim: &[&[String]],
    events: &dyn EventSink,
) -> String {
    if !armed {
        return format!("THE REQUEST:\n{spec}");
    }
    let spliced = splice_claimed_sections(slice_id, claimed, sections, events);
    // The same helper, the same plan-wide inputs the brief builder hands it (VA-030): every
    // slice's claims so rule (c) counts claimants across the plan, and this slice's declared
    // files so rule (a) reads the routes its files name. Before, the research prompt passed
    // only its own claims — every childless top-level section read as cross-cutting and rule
    // (a) had no files to read.
    let consumed = consumed_spec_sections(slice_id, claimed, files, every_claim, sections, events);
    let orientation = spec_orientation(sections);
    let mut block = if spliced.is_empty() {
        format!(
            "THE REQUEST, AS ITS ORIENTATION INDEX (this slice claimed no sections — every \
             section's full text is in the request file named under SOURCES; open the ones your \
             question needs):\n\n{orientation}"
        )
    } else {
        // r6c, research-ledgerd-core-q2: the Health shape lived in a section this slice did not
        // claim; the lane saw only the index's excerpt ("shape below"), called the shape "not
        // pinned in the provided spec text" and invented one. The index is a MAP, not a wall:
        // the owned sections are the lane's area, and any other section it needs is one read
        // away in the request file. Stated here, where the sections are.
        format!(
            "THE REQUEST, AS ITS ORIENTATION INDEX (every section: heading, size, opening \
             sentences):\n\n{orientation}\n\nTHE SPEC'S OWN SECTIONS FOR THIS SLICE — the \
             sections this slice OWNS, verbatim, and the authority over any paraphrase. A \
             question may reach into a section that is only INDEXED above (an endpoint's \
             response shape, a counter's lifetime, a boot flag): open that section in the \
             request file named under SOURCES and answer from its words — never from the \
             index's excerpt alone:{spliced}"
        )
    };
    block.push_str(&consumed_sections_blocks(&consumed));
    block
}

/// The two labeled blocks a consumer appends after the slice's own sections — one rendering for
/// the brief and the research prompt, so the labels a builder and a researcher read are the
/// same words. Empty when nothing was consumed (no heading, no filler).
fn consumed_sections_blocks(consumed: &ConsumedSections) -> String {
    let mut out = String::new();
    if !consumed.called_into.is_empty() {
        out.push_str(&format!(
            "\n\nSECTIONS THIS SLICE CALLS INTO — not owned, verbatim: the request's own text for \
             the routes this slice's sections and files name and for the details under its \
             claimed parents. Another slice builds these; this slice must match their exact \
             paths, parameter VALUES and shapes — never a paraphrase of them:{}",
            consumed.called_into
        ));
    }
    if !consumed.cross_cutting.is_empty() {
        out.push_str(&format!(
            "\n\nCROSS-CUTTING SPEC RULES — the request's top-level rules that bind every \
             slice, verbatim; no slice owns them and every slice is graded on them:{}",
            consumed.cross_cutting
        ));
    }
    out
}

/// The HEAD of one slice-question prompt, assembled from THIS run's facts (the specificity
/// gate): the request block, the owning slice, the USER DECISIONS the ASK handshake resolved
/// (A6 — the fan runs AFTER the handshake so decisions inform research), the tree as the run
/// found it, and the SOURCES block. Built once per slice before the fan; the two parts that
/// depend on WHEN a lane dispatches — the already-answered minis and the question — are added
/// by `research_dispatch_text` at dispatch. Absences are stated, never papered over.
pub(super) fn research_prompt_head(
    request_block: &str,
    slice_id: &str,
    slice_title: &str,
    slice_objective: &str,
    user_decisions: &str,
    tree_at_start: &[String],
    sources_block: &str,
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
         {slice_objective}{decisions_block}{tree_block}{sources_block}"
    )
}

/// The whole user text of one research call (C3: one lane per slice): the per-slice head, the
/// snowball block (empty on a first dispatch — no heading, no filler), then EVERY remaining
/// question of the slice VERBATIM, each tagged `[qN]` with its q_index — the tag the reply's
/// `question_index` repeats, so no translation table exists between the prompt and the ledger
/// — under the label its kind carries: slice questions, or the open decisions the user left
/// unanswered (the decision head, `decisions::decision_user_text`, frames the same tail).
pub(super) fn research_user_text(head: &str, prior_block: &str, qs: &[ResearchQuestion]) -> String {
    let decisions = qs.first().is_some_and(|q| q.slice == DECISION_SLICE);
    let label = if decisions {
        format!(
            "THE OPEN DECISIONS ({}) — each was put to the user and the user did not answer it; \
             settle EVERY one in this session",
            qs.len()
        )
    } else {
        format!(
            "THE QUESTIONS ({}) — answer EVERY one of them in this session",
            qs.len()
        )
    };
    let tagged: Vec<String> = qs
        .iter()
        .map(|q| format!("[q{}] {}", q.q_index, q.question))
        .collect();
    format!(
        "{head}{prior_block}\n\n{label}. Each is tagged [qN]; your final_output carries one \
         entry per tag with question_index = N:\n{}",
        tagged.join("\n")
    )
}

/// Where the fan persists the request's FULL text so a lane can READ any section its question
/// needs instead of answering from the index's 400-char excerpt (r6c: the ledgerd-core lanes
/// answered the /api/health shape and the webhook counters' lifetime WRONG — both pinned in an
/// Endpoints section their slice did not claim, both "not pinned in the provided spec text" to
/// a lane that had only the excerpt; the ledgerd-api lane, whose slice claimed Endpoints,
/// answered the same shapes correctly — the text, not the model, was the difference). Under
/// `.swarm/` because the run root IS the product tree: a file left there would be measured as
/// outside the plan by fs_delta and could ride into a deliverable count.
pub(super) const REQUEST_FILE: &str = ".swarm/request.md";

/// Writes the request file (tmp+rename, the forming sidecar's own writer) and returns its path;
/// a write that fails is a NAMED absence — `research_request_not_persisted` with the error —
/// and the SOURCES block then states the file is missing instead of pointing at nothing.
pub(super) fn persist_request_text(
    root: &Path,
    spec: &str,
    events: &dyn EventSink,
) -> Option<PathBuf> {
    let path = root.join(REQUEST_FILE);
    let written = match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|_| write_forming_atomic(&path, spec));
    match written {
        Ok(()) => Some(path),
        Err(e) => {
            events.write_value(serde_json::json!({
                "event": "research_request_not_persisted",
                "path": path.display().to_string(),
                "error": e.to_string(),
            }));
            None
        }
    }
}

/// The top-level entries of the tree as the run found it (`app/`, `web/`, `README.md`),
/// deduped and sorted — the on-disk research material, derived from the manifest, never a
/// baked list. `.swarm/` never appears (the manifest walker skips it; this filter is the belt).
fn top_level_entries(tree_at_start: &[String]) -> Vec<String> {
    tree_at_start
        .iter()
        .filter(|p| !p.starts_with(".swarm"))
        .map(|p| match p.split_once('/') {
            Some((dir, _)) => format!("{dir}/"),
            None => p.to_string(),
        })
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// The SOURCES block every research prompt carries (fix C, the specificity gate): what IS
/// research material in this run — the request file (or its stated absence), the vendor
/// documentation URL when the spec names one (`spec_vendor`, the run's own derivation), the
/// tree's top-level entries when any exist — and the one thing that is NOT: `.swarm/`, this
/// engine's own state. r6c, research-ledgerd-api-q1 read `.swarm/activity` as evidence: "The
/// activity file is a live swarm log (my own calls are being recorded in it…)". Every name here
/// is a fact of THIS run; nothing is baked beyond `.swarm/` itself.
pub(super) fn research_sources_block(
    request_path: Option<&Path>,
    spec: &str,
    tree_at_start: &[String],
) -> String {
    let mut s = String::from("\n\nSOURCES — what is research material here:\n");
    match request_path {
        Some(p) => s.push_str(&format!(
            "- The request's FULL text is on disk at `{0}`. When your question reaches into a \
             part of the request this message only indexes, open that section from the file \
             with your shell (`grep -n '^#' {0}` lists every heading with its line number) and \
             answer from its words.\n",
            p.display()
        )),
        None => s.push_str(
            "- The request's full text could NOT be written to disk \
             (`research_request_not_persisted` rode the event log): the request text in this \
             message is all you have — say so when a question reaches past it.\n",
        ),
    }
    let (docs_url, _, _) = spec_vendor(spec);
    if let Some(docs_url) = docs_url {
        s.push_str(&format!(
            "- The vendor's documentation is at {docs_url} — fetch it whenever the question \
             touches the vendor's contract.\n"
        ));
    }
    let entries = top_level_entries(tree_at_start);
    if !entries.is_empty() {
        s.push_str(&format!(
            "- Files already on disk live under: {}.\n",
            entries.join(", ")
        ));
    }
    s.push_str(
        "- `.swarm/` is this engine's own state — activity logs (your own calls are being \
         recorded there as you work), ledgers, telemetry — NOT research material: read from it \
         only what this message names (the request file above, and any ledger mini quoted \
         below).",
    );
    s
}

/// Every research mini the ledger holds right now, in (slice, q_index) order. Read at DISPATCH
/// time, not at fan start, so a lane sees what finished before it left. No ledger dir means no
/// minis yet — the honest empty of a fresh run's first dispatch, not a swallowed failure; a
/// mini that does not parse is skipped here exactly as `load_research_mini` skips it, and is
/// already named loudly by `rebuild_ledger_rollup`'s rows_dropped (the ledger block prints the
/// WARNING) — nothing is substituted for it.
pub(super) fn load_research_minis(root: &Path) -> Vec<ResearchRow> {
    let Ok(rd) = std::fs::read_dir(root.join(LEDGER_DIR)) else {
        return Vec::new();
    };
    let mut rows: Vec<ResearchRow> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("research-") && name.ends_with(".json")) {
                return None;
            }
            serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()
        })
        .collect();
    rows.sort_by(|a, b| (&a.slice, a.q_index).cmp(&(&b.slice, b.q_index)));
    rows
}

/// The path-shaped words of a question — `/api/health`, `app/db.py`, `web/` — lowercased: the
/// one literal, explainable link between two lanes' questions across slices. Punctuation that
/// prose hangs on a path (`/api/health,` or `(web/)`) is trimmed; a trailing full stop too.
fn path_tokens(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| "`'\"(),;:?![]{}<>".contains(c))
                .trim_end_matches('.')
        })
        .filter(|t| t.contains('/') && t.len() > 2 && t.chars().any(char::is_alphanumeric))
        .map(str::to_lowercase)
        .collect()
}

/// Does any question of a lane's batch name a path `row`'s question names? THE ONE cross-slice
/// link — `/api/health`, `app/db.py`, `web/` — shared by the dispatch-time snowball
/// (`prior_minis_for`) and the late relay (`relay_targets`), so the two channels cannot
/// disagree about which stranger's mini a lane should see.
fn names_a_shared_path(batch: &[ResearchQuestion], row: &ResearchRow) -> bool {
    let theirs = path_tokens(&row.question);
    !theirs.is_empty()
        && batch
            .iter()
            .any(|q| !path_tokens(&q.question).is_disjoint(&theirs))
}

/// The already-answered minis a dispatching lane should see (fix B, the snowball inside the
/// fan), for the whole BATCH the lane carries: every ANSWERED row of its own slice that is not
/// in the batch — resumed minis, or a sibling settled earlier (r6c: ledgerd-core q0 and q1
/// contradicted each other on cursor persistence — "in-memory per walk" vs "never held only in
/// memory" — because neither could see the other) — plus an answered row of ANOTHER slice when
/// its question names a path one of the batch's questions names (r6c: ledgerd-api-q0's
/// question named `/api/health` and its answer carried the exact Health shape ten minutes
/// before ledgerd-core-q2 asked what `/api/health` exposes — and invented one). Own slice
/// first, then the path-matched strangers, each row once. Unanswered rows are never spliced:
/// their absence already rode `research_unanswered`.
pub(super) fn prior_minis_for<'a>(
    batch: &[ResearchQuestion],
    rows: &'a [ResearchRow],
) -> Vec<&'a ResearchRow> {
    let Some(slice) = batch.first().map(|q| q.slice.as_str()) else {
        return Vec::new();
    };
    let mut same: Vec<&ResearchRow> = Vec::new();
    let mut matched: Vec<&ResearchRow> = Vec::new();
    for r in rows {
        if r.status != RESEARCH_ANSWERED {
            continue;
        }
        if r.slice == slice {
            if !batch.iter().any(|q| q.q_index == r.q_index) {
                same.push(r);
            }
        } else if names_a_shared_path(batch, r) {
            matched.push(r);
        }
    }
    same.extend(matched);
    same
}

/// The snowball block: NOTHING when no prior mini qualifies (a first dispatch carries no
/// heading and no filler), else the real count and each prior row — its provenance (own lane
/// or which other slice, and the durable mini), its question, and its answer under the same
/// per-answer splice budget the brief uses (`budget_research_answer`: a render budget on the
/// splice, never a cap; the full text is in the mini it names).
pub(super) fn prior_minis_block(slice: &str, prior: &[&ResearchRow]) -> String {
    if prior.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "\n\nALREADY ANSWERED BY THIS FAN before your dispatch ({}) — earlier lanes settled these \
         from the same request. Build on them: where your answer depends on one, agree with it \
         or NAME the disagreement and the request's words that decide it (the builder receives \
         both answers) — never contradict one silently:\n",
        prior.len()
    );
    for r in prior {
        let from = match (r.slice == slice, r.slice == DECISION_SLICE) {
            (true, true) => "an earlier open decision this fan settled".to_string(),
            (true, false) => "this slice's own earlier lane".to_string(),
            (false, true) => {
                "an open decision this fan settled — it names the same path as your question"
                    .to_string()
            }
            (false, false) => format!(
                "slice `{}` — its question names the same path as one of yours",
                r.slice
            ),
        };
        s.push_str(&format!(
            "\n[{from}; .swarm/ledger/{}]\nQ: {}\nA: {}\n",
            research_mini_name(&r.slice, r.q_index),
            r.question,
            budget_research_answer(&r.answer, &r.slice, r.q_index)
        ));
    }
    s
}

/// The dispatch-time assembly of one lane's user text (the slice's whole remaining batch), and
/// the one `research_context` event per dispatch that lets the tick print a lane's grounding:
/// which questions it carries, how many prior minis it saw (and which), and how many sections
/// the index named for it (0 when the orientation is not armed and the whole request rides
/// inline).
pub(super) fn research_dispatch_text(
    root: &Path,
    events: &dyn EventSink,
    head: &str,
    batch: &[ResearchQuestion],
    activity_key: &str,
    index_sections: usize,
) -> String {
    let rows = load_research_minis(root);
    let prior = prior_minis_for(batch, &rows);
    let slice = batch.first().map(|q| q.slice.as_str()).unwrap_or("");
    events.write_value(serde_json::json!({
        "event": "research_context",
        "task": activity_key,
        "slice": slice,
        "q_indexes": batch.iter().map(|q| q.q_index).collect::<Vec<_>>(),
        "questions": batch.len(),
        "prior_minis": prior.len(),
        "prior_from": prior
            .iter()
            .map(|r| research_mini_name(&r.slice, r.q_index))
            .collect::<Vec<_>>(),
        "index_sections": index_sections,
    }));
    research_user_text(head, &prior_minis_block(slice, &prior), batch)
}

/// One sibling mini handed to a still-running lane (r6e E7 — the research fan's LATE
/// snowball). `prior_minis_block` splices the minis answered BEFORE a lane's dispatch and
/// nothing else: r6d dispatched research-ledger-core-q5 at 04:46:55Z with prior_minis=1 (q4);
/// q2's write-through mini landed 33 s later (04:47:28Z) and never reached it, and q5's own
/// mini then HEDGED against the version rule q2 had settled ("re-sending the identical snapshot
/// is harmless" while request.md:189-194 has the vendor bump version on every note write). A
/// relay note is the same fact at the same splice budget, delivered as the lane's next user
/// message through the existing steer channel — never a restream, never a bound on anything.
#[derive(Clone, Debug)]
pub(super) struct RelayNote {
    pub(super) from_mini: String,
    pub(super) from_question: String,
    pub(super) text: String,
}

/// The lanes a just-landed mini is relayed to — RE-AIMED by C3. Under one lane per slice there
/// is no same-slice sibling left to relay to (a slice's rows all land when its one lane ends),
/// so the E7 target rule became unreachable; the refuter confirmed E7 with that correction.
/// The relay now uses the SAME rule the dispatch-time snowball uses for strangers
/// (`names_a_shared_path`): every STILL-RUNNING lane of another slice whose batch names a path
/// the landed question names — the set `prior_minis_for` would have spliced had that lane
/// dispatched a moment later. Under C3 the first wave's lanes all dispatch at once and see NO
/// prior minis, so this relay is the only way ledger-api's `/api/health` shape reaches a
/// running ledger-core lane (the r6c invention). Only an answered row relays (an unanswered
/// one already rode `research_unanswered`); a lane never receives its own row. `running` is
/// (activity key, the lane's batch) for every lane between dispatch and its rows.
pub(super) fn relay_targets(
    landed: &ResearchRow,
    running: &[(String, Vec<ResearchQuestion>)],
) -> Vec<String> {
    if landed.status != RESEARCH_ANSWERED {
        return Vec::new();
    }
    running
        .iter()
        .filter(|(_, batch)| {
            batch.first().is_some_and(|q| q.slice != landed.slice)
                && names_a_shared_path(batch, landed)
        })
        .map(|(k, _)| k.clone())
        .collect()
}

/// The note itself: provenance (the durable mini and the slice that produced it), the
/// question and its answer under the brief's splice budget (`budget_research_answer` — a render
/// budget, never a cap; the full text is in the mini it names), and the same rule the
/// dispatch-time block states — build on it or NAME the disagreement, never contradict it
/// silently. The ISO stamp is data (it dates the block in the durable log); nothing reads it.
pub(super) fn relay_note(landed: &ResearchRow) -> RelayNote {
    let from_mini = research_mini_name(&landed.slice, landed.q_index);
    let text = format!(
        "A MINI LANDED ({}) — the lane of slice `{}` settled this while you were working, and \
         its question names a path one of yours names; it is now in .swarm/ledger/{from_mini}. \
         Build on it: where an answer of yours depends on it, agree with it or NAME the \
         disagreement and the request's words that decide it (the builder receives both \
         answers) — never contradict it silently. Continue the SAME questions; do not restart.\n\
         Q: {}\nA: {}",
        chrono::Utc::now().to_rfc3339(),
        landed.slice,
        landed.question,
        budget_research_answer(&landed.answer, &landed.slice, landed.q_index)
    );
    RelayNote {
        from_mini,
        from_question: head_to_sentence_end(&landed.question, 200),
        text,
    }
}

/// RESEARCH FAN v2's row, through the same funnel as every other mini so idempotency and the
/// rollup rebuild come free. Written for answered AND unanswered outcomes — the absence is a
/// fact the ledger holds (the fallback gate) — and the mini's presence is the resume watermark
/// `load_research_mini` reads. `ts` is provenance (data, never an input to anything). (Moved
/// verbatim from swarm.rs under the incremental-split law, paying for E7's relay wiring.)
pub(super) fn write_research_ledger(root: &Path, row: &ResearchRow) -> Result<PathBuf, String> {
    let mut v = serde_json::to_value(row).map_err(|e| format!("serialize: {e}"))?;
    if let Some(o) = v.as_object_mut() {
        o.insert("kind".to_string(), serde_json::json!("research"));
        o.insert(
            "ts".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }
    super::write_ledger_mini_checked(root, &research_mini_name(&row.slice, row.q_index), &v)
}

/// The fan's ONE way to land a row (VA-030 D10-5, gate 1): the mini is written, and a write that
/// fails is a named event — `research_mini_write_failed {slice, q_index, path, error}` — never a
/// discarded Option. Before this the fact rows, the covered rows, the lane rows and the panic rows
/// all dropped the result: a fact mini that never reached disk was counted in
/// `research_planned.facts` and rendered into the brief from memory while resume, cover and the
/// snowball (which read the disk) never saw it, and nothing said so. The row still flows to its
/// in-memory consumers — the absence on disk is stated, not substituted for.
pub(super) fn persist_research_row(root: &Path, events: &dyn EventSink, row: &ResearchRow) {
    if let Err(error) = write_research_ledger(root, row) {
        events.write_value(serde_json::json!({
            "event": "research_mini_write_failed",
            "slice": row.slice,
            "q_index": row.q_index,
            "path": root
                .join(super::LEDGER_DIR)
                .join(research_mini_name(&row.slice, row.q_index))
                .display()
                .to_string(),
            "error": error,
        }));
    }
}

/// C3: the fan's lanes — ONE per slice with anything left to ask, in the queue's own order (the
/// queue is built slice by slice, decisions last, so consecutive grouping is exact), each lane
/// carrying every remaining question of its slice and the slice's prompt head once. Never yields
/// an empty batch. Work-stealing over the hosts is unchanged: `fanout_over_fleet` hands each
/// lane the next free host, so 6 slices on 3 nodes run 3 + 3, not 38 single-question sessions.
pub(super) fn batch_by_slice(
    queue: Vec<(ResearchQuestion, String)>,
) -> Vec<(Vec<ResearchQuestion>, String)> {
    let mut out: Vec<(Vec<ResearchQuestion>, String)> = Vec::new();
    for (q, head) in queue {
        match out.last_mut() {
            Some((batch, _)) if batch.first().is_some_and(|b| b.slice == q.slice) => {
                batch.push(q);
            }
            _ => out.push((vec![q], head)),
        }
    }
    out
}

/// The fan's QUEUE as one event, emitted once when it is built and before anything dispatches:
/// how many questions reach a lane (`questions` = dispatching now + settled from the ledger on
/// resume — the denominator tick.py's `queued` subtracts dispatched from, so a fact that never
/// dispatches is NOT in it), how many the opener settled as cited SPEC FACTS (`facts`, the fan
/// cut's saving), and the per-slice lane count — every number derived from the queue itself.
/// Before this the vigil derived the total by counting '?' in the opener's output (r6c); an
/// instrument that has to guess the denominator is not one.
pub(super) fn emit_research_planned(
    events: &dyn EventSink,
    dispatching: &[ResearchQuestion],
    resumed: &[ResearchRow],
    facts: usize,
    lanes: usize,
) {
    let mut per_slice: std::collections::BTreeMap<&str, usize> = Default::default();
    for slice in dispatching
        .iter()
        .map(|q| q.slice.as_str())
        .chain(resumed.iter().map(|r| r.slice.as_str()))
    {
        *per_slice.entry(slice).or_insert(0) += 1;
    }
    events.write_value(serde_json::json!({
        "event": "research_planned",
        "questions": dispatching.len() + resumed.len(),
        "dispatching": dispatching.len(),
        "resumed": resumed.len(),
        "facts": facts,
        // C3: one lane per slice with anything left to ask (+1 for the open decisions) — the
        // number of sessions the fan runs, against `dispatching` questions (r6d: 38 vs 38).
        "lanes": lanes,
        "per_slice": per_slice,
    }));
}

/// The opener's per-question disposition, one event per question when the fan's queue is built
/// (the loud channel for the fan cut): `kind` and `cite` as the opener wrote them (`cite` null
/// when it named none), whether a `fact` came with it, and what the engine did — `fact` (a
/// cited spec fact, no lane), `dispatch` (rides a lane), or `resumed` (its mini already
/// existed). A `spec_lookup` dispatched with `cite: null` is the opener saying it searched and
/// found nothing — visible here, never silently dropped.
pub(super) fn emit_question_disposition(
    events: &dyn EventSink,
    q: &ResearchQuestion,
    disposition: &str,
) {
    events.write_value(serde_json::json!({
        "event": "research_question_kind",
        "slice": q.slice,
        "q_index": q.q_index,
        "kind": q.kind.as_str(),
        "cite": (!q.cite.is_empty()).then(|| q.cite.clone()),
        "fact": !q.fact.is_empty(),
        "disposition": disposition,
        "question": q.question.chars().take(200).collect::<String>(),
    }));
}

/// The owner's OWNERSHIP DECLARATIONS, read back out of its objective's backticks.
///
/// Since 14831a321 the opener is told to NAME EACH SLICE'S OWNED FILES IN ITS OBJECTIVE — but
/// `briefs_from_slices` still shipped `files: Vec::new()`, so `slice_files_unnamed` fired for
/// EVERY slice on EVERY run (measured 5 and 7 on the last two runs) and every `.files` reader
/// — the index's named-files caption, the fallback plan's declared ownership — was a dead path.
///
/// Conservative on purpose: only backticked tokens shaped like relative file paths — a '/' or a
/// known source extension (`FINDING_PATH_EXTS`, the one list), no whitespace, no scheme, not
/// absolute (`/api/ledger` is a route, not a file), no trailing '/' (a directory is territory,
/// not a plan file) — deduped in objective order. An objective that declares nothing keeps the
/// empty vec, and the absence event keeps firing for exactly those slices. (Moved verbatim from
/// swarm.rs beside its one caller, paying for the fan cut's C2 wiring.)
pub(super) fn files_from_objective(objective: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, seg) in objective.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        let tok = seg.trim();
        let tok = tok.strip_prefix("./").unwrap_or(tok);
        if tok.is_empty()
            || tok.chars().any(char::is_whitespace)
            || tok.contains("://")
            || tok.starts_with('/')
            || tok.ends_with('/')
        {
            continue;
        }
        let lower = tok.to_lowercase();
        let pathish = tok.contains('/') || FINDING_PATH_EXTS.iter().any(|e| lower.ends_with(e));
        if pathish && !out.iter().any(|f| f == tok) {
            out.push(tok.to_string());
        }
    }
    out
}

/// What a slice can be NAMED BY (VA-012): its id, its declared files and their basenames, the
/// file-like tokens of its claimed headings (`DECISIONS.md`; a bare `web/` or `app` is not a
/// name), the routes its claimed sections advertise, and the decision ids (`D1`) its claimed
/// bodies cite. Lowercased — `content_words` lowercases the text it is matched against.
struct SliceVocabulary {
    names: BTreeSet<String>,
    routes: BTreeSet<String>,
    decision_ids: BTreeSet<u32>,
}

fn slice_vocabulary(
    slice_id: &str,
    files: &[String],
    claimed: &[String],
    sections: &[SpecSection],
) -> SliceVocabulary {
    let mut names: BTreeSet<String> = BTreeSet::new();
    names.insert(slice_id.to_lowercase());
    for f in files {
        names.insert(f.to_lowercase());
        if let Some((_, base)) = f.rsplit_once('/') {
            names.insert(base.to_lowercase());
        }
    }
    let mut routes: BTreeSet<String> = BTreeSet::new();
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    for want in claimed {
        for (i, seg) in want.split('`').enumerate() {
            let tok = seg.trim().trim_end_matches('/');
            if i % 2 == 1 && (tok.contains('/') || tok.contains('.')) {
                names.insert(tok.to_lowercase());
            }
        }
        let key = heading_key(want);
        if let Some(sec) = sections.iter().find(|s| heading_key(&s.heading) == key) {
            routes.extend(advertised_paths(sec).into_iter().map(|p| p.to_lowercase()));
            ids.extend(decision_ids(&sec.body));
        }
    }
    SliceVocabulary {
        names,
        routes,
        decision_ids: ids,
    }
}

impl SliceVocabulary {
    /// Does `text` name this slice — a content word that IS one of its names or files (or a
    /// path ending in one of its basenames), or that is one of its routes (or a template under
    /// one: `/api/drafts/` from `/api/drafts/<id>/submit` names `/api/drafts`)?
    fn named_in(&self, text: &str) -> bool {
        content_words(text).iter().any(|tok| {
            self.names
                .iter()
                .any(|n| tok == n || (!n.contains('/') && tok.ends_with(&format!("/{n}"))))
                || self
                    .routes
                    .iter()
                    .any(|r| tok == r || tok.starts_with(&format!("{r}/")))
        })
    }
}

/// Does a paragraph of a decision's answer cite the request — `request.md:NNN`, "section 7",
/// "Sections 5 and 9", `§`? Those are the GROUNDING lines a builder can check against the spec;
/// the rest of a handoff addressed to another slice is that slice's.
fn cites_request(paragraph: &str) -> bool {
    let lower = paragraph.to_lowercase();
    if lower.contains("request.md:") || lower.contains('§') {
        return true;
    }
    lower.match_indices("section").any(|(at, word)| {
        lower.get(at + word.len()..).is_some_and(|rest| {
            rest.trim_start_matches('s')
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
    })
}

/// The slices each plan decision NAMES (VA-012): by index into `opened.slices`. A decision names
/// a slice when its question or answer carries one of the slice's names, files or routes
/// (`SliceVocabulary::named_in`), when its question cites a decision id the slice's claimed
/// bodies cite (`D1` in §9's body, claimed by r6c's web-console), or when one of the slice's own
/// questions was routed to it (C2(a)). A decision naming NO slice is every slice's — MILD: it
/// broadcasts, and `decision_broadcast{decision, question}` says so.
fn decision_consumers(
    opened: &OpenOutput,
    vocabularies: &[SliceVocabulary],
    d: &PlanDecision,
    events: &dyn EventSink,
) -> Vec<usize> {
    let answer = match &d.state {
        DecisionState::SettledByUser { answer } | DecisionState::SettledByResearch { answer } => {
            answer.as_str()
        }
        DecisionState::Open => "",
    };
    let text = format!("{}\n{answer}", d.question);
    let question_ids = decision_ids(&d.question);
    let mut consumers: Vec<usize> = opened
        .slices
        .iter()
        .enumerate()
        .filter(|(i, sl)| {
            sl.questions.iter().any(|q| q.decision == Some(d.q_index))
                || !question_ids.is_disjoint(&vocabularies[*i].decision_ids)
                || vocabularies[*i].named_in(&text)
        })
        .map(|(i, _)| i)
        .collect();
    if consumers.is_empty() {
        events.write_value(serde_json::json!({
            "event": "decision_broadcast",
            "decision": d.q_index,
            "question": head_to_sentence_end(&d.question, 200),
        }));
        consumers = (0..opened.slices.len()).collect();
    }
    consumers
}

/// One slice's DECISIONS block (VA-012): only the decisions that name it. A RESEARCH-settled
/// decision renders as its VERDICT (the answer's first paragraph), then the answer's paragraphs
/// that name THIS slice or cite the request — verbatim, whole — and the durable mini's path.
/// Never a head cut: r6c's five briefs each carried the same 5,582-char block, every research
/// answer cut at 1,500 chars with "ANSWER TRUNCATED" (27,910 chars, 22,328 duplicate), and
/// web-console's copy of D1 ended before "3. `web/app.js` behavior contract" (char 2,057 of
/// 2,562) — the one paragraph addressed to it. The user-settled and still-open decisions that
/// name the slice carry no transcript and keep their exact pre-VA-012 rendering
/// (`decisions::decisions_brief_block` over that subset) — the arm that had the defect is the
/// only arm that changed.
fn slice_decisions_block(
    decisions: &[PlanDecision],
    consumers: &[Vec<usize>],
    slice_index: usize,
    vocabulary: &SliceVocabulary,
) -> String {
    let mut settled = String::new();
    let mut without_transcript: Vec<PlanDecision> = Vec::new();
    for (d, who) in decisions.iter().zip(consumers) {
        if !who.contains(&slice_index) {
            continue;
        }
        match &d.state {
            DecisionState::SettledByUser { .. } | DecisionState::Open => {
                without_transcript.push(d.clone());
            }
            DecisionState::SettledByResearch { answer } => {
                let mut paragraphs = answer
                    .split("\n\n")
                    .map(str::trim)
                    .filter(|p| !p.is_empty());
                let verdict = paragraphs.next().unwrap_or("");
                let indent = |p: &str| {
                    p.lines()
                        .map(|l| format!("  {l}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                let mut lines = format!(
                    "- {}\n  SETTLED BY PLAN-TIME RESEARCH (the user did not answer; a \
                     convention, binding for consistency): {}\n",
                    d.question.trim(),
                    verdict.replace('\n', " ")
                );
                for p in paragraphs.filter(|p| vocabulary.named_in(p) || cites_request(p)) {
                    lines.push_str(&indent(p));
                    lines.push('\n');
                }
                lines.push_str(&format!(
                    "  FULL ANSWER (every slice's handoff): .swarm/ledger/{}\n",
                    research_mini_name(DECISION_SLICE, d.q_index)
                ));
                settled.push_str(&lines);
            }
        }
    }
    let mut out = String::new();
    if !settled.is_empty() {
        out.push_str(&format!(
            "\n\nDECISIONS SETTLED AT PLAN TIME BY RESEARCH that name this slice — the verdict, \
             the request-grounded lines and the paragraphs addressed to this slice, verbatim and \
             BINDING; implement each exactly as written and never substitute your own \
             convention (a decision naming no slice is repeated in every brief):\n{settled}"
        ));
    }
    out.push_str(&decisions::decisions_brief_block(&without_transcript));
    out
}

/// The brief partition every slice's builder reads, assembled from the opener's slice and the
/// fan's terminal rows — the objective, then what the fan settled (SPEC FACTS the opener cited,
/// answers the lanes established or an earlier mini covered), the questions that ARE open
/// decisions (pointed at the decisions partition — decided once), then what stayed open, the
/// raised questions, the claimed spec sections verbatim, and the decisions partition. (Moved
/// from swarm.rs under the incremental-split law, paying for the fan cut's wiring; the WHY of
/// each block is inline.)
pub(super) fn briefs_from_slices(
    opened: &OpenOutput,
    spec: &str,
    research: &[ResearchRow],
    plan_decisions: &[decisions::PlanDecision],
    events: &dyn EventSink,
) -> Vec<SliceBrief> {
    let sections = spec_sections(spec);
    let armed = orientation_armed(spec, &sections);
    let every_claim: Vec<&[String]> = opened
        .slices
        .iter()
        .map(|sl| sl.sections.as_slice())
        .collect();
    // ITEM 0, amendment (b), per slice since VA-012: the settled/still-open DECISIONS PARTITION
    // — but each brief carries the decisions that NAME its slice (files, routes, claimed
    // sections' decision ids, routed questions), a decision naming none goes to every brief
    // (loud), and a research answer renders as its verdict plus its slice-addressed and
    // request-citing paragraphs, whole — never a 1,500-char head. With no decisions the block
    // is empty and every brief is byte-identical to the pre-partition form. (The worker
    // channel, `research_settled_worker_block`, is unchanged here.)
    let vocabularies: Vec<SliceVocabulary> = opened
        .slices
        .iter()
        .map(|sl| {
            slice_vocabulary(
                &sl.id,
                &files_from_objective(&sl.objective),
                &sl.sections,
                &sections,
            )
        })
        .collect();
    let consumers: Vec<Vec<usize>> = plan_decisions
        .iter()
        .map(|d| decision_consumers(opened, &vocabularies, d, events))
        .collect();
    opened
        .slices
        .iter()
        .enumerate()
        .map(|(slice_index, sl)| {
            let mut brief = sl.objective.clone();
            let files = files_from_objective(&sl.objective);
            // RESEARCH FAN v2: the slice's own questions, partitioned against what the fan
            // settled. A cited SPEC FACT (the opener read the request; no lane ran) renders
            // FIRST, under its own heading, with the cite — the builder can check it against
            // the request line. An answered question moves OUT of the QUESTIONS block and into
            // a settled-facts block ABOVE it; an unanswered one stays exactly as before — the
            // absence already rode `research_unanswered` and the brief carries the raw
            // question, never a fabricated answer (the fallback gate). With no research rows
            // the brief is byte-identical to the pre-fan form.
            let slice_rows: Vec<&ResearchRow> =
                research.iter().filter(|r| r.slice == sl.id).collect();
            let answered: std::collections::HashMap<usize, &&ResearchRow> = slice_rows
                .iter()
                .filter(|r| r.status == RESEARCH_ANSWERED)
                .map(|r| (r.q_index, r))
                .collect();
            let mut facts_block = String::new();
            let mut answers_block = String::new();
            let mut decided_block = String::new();
            let mut open_questions: Vec<&str> = Vec::new();
            for (i, q) in sl.questions.iter().enumerate() {
                match answered.get(&i) {
                    Some(row) if row.origin == ORIGIN_SPEC_FACT => {
                        facts_block.push_str(&format!(
                            "\nQ: {}\nFACT: {}\nCITE: {}\n",
                            q.text,
                            row.answer.trim_end(),
                            row.cite
                        ));
                    }
                    Some(row) => {
                        // C2(b): a covered row says WHOSE answer this is — the original mini —
                        // so the builder can open it; a lane's own row carries no VIA line (an
                        // honest absence of provenance-to-elsewhere, not a default).
                        // D10-7: a cover that was a FACT row had no lane — it was answered from
                        // the request by the opener — and the VIA line says so, with the cite.
                        let via = match row.origin.strip_prefix(ORIGIN_COVERED_PREFIX) {
                            Some(m) if row.model.is_empty() => format!(
                                "\nVIA: .swarm/ledger/{m} — answered from the request by the \
                                 opener (FACT, CITE {}); this is that fact",
                                row.cite
                            ),
                            Some(m) => format!(
                                "\nVIA: .swarm/ledger/{m} — another slice asked the same \
                                 question and its lane answered it; this is that answer"
                            ),
                            None => String::new(),
                        };
                        // WHOLE (VA-030): the answer to this slice's OWN question is addressed
                        // to this slice in every paragraph. The 1,500-char head cut left r6c's
                        // five briefs with 4-5 "ANSWER TRUNCATED — full text in .swarm/ledger/…"
                        // each, pointing at a file no worker is told to read (the five slices'
                        // 23 answers: 34,500 chars budgeted against 75,247 whole). Mihai: trust
                        // the model with the information.
                        answers_block.push_str(&format!(
                            "\nQ: {}\nA: {}{via}\n",
                            q.text,
                            row.answer.trim_end()
                        ));
                    }
                    // C2(a): the question IS an open decision — settled once, in the DECISIONS
                    // block below (the user's answer, the decisions lane's row, or the
                    // conventional-choice framing); the builder must not decide it again here.
                    None if q.decision.is_some() => {
                        let i = q.decision.unwrap_or(0);
                        let line = opened
                            .open_decisions
                            .get(i)
                            .map(|d| d.line.as_str())
                            .unwrap_or("(decision index out of range — see the DECISIONS block)");
                        decided_block.push_str(&format!(
                            "\n- {}\n  → OPEN DECISION #{}: {}",
                            q.text,
                            i + 1,
                            head_to_sentence_end(line, 200).replace('\n', " ")
                        ));
                    }
                    None => open_questions.push(&q.text),
                }
            }
            if !facts_block.is_empty() {
                brief.push_str(&format!(
                    "\n\nSPEC FACTS (cited) — the opener read these in the request itself; each \
                     names the line or heading it came from, and the request's own text there \
                     is the authority if the two ever disagree:{facts_block}"
                ));
            }
            if !answers_block.is_empty() {
                brief.push_str(&format!(
                    "\n\nANSWERS SETTLED AT PLAN TIME — facts gathered from the request before \
                     building; build to these unless the spec or a USER DECISIONS block \
                     contradicts them:{answers_block}"
                ));
            }
            if !decided_block.is_empty() {
                brief.push_str(&format!(
                    "\n\nQUESTIONS THAT ARE OPEN DECISIONS — each one IS a decision in the \
                     DECISIONS block below, settled there once for every slice; implement that \
                     settlement, never decide it again here:{decided_block}"
                ));
            }
            if !open_questions.is_empty() {
                brief.push_str(&format!(
                    "\n\nQUESTIONS this slice must settle in its implementation (conventional \
                     answers unless the request says otherwise):\n{}",
                    open_questions
                        .iter()
                        .map(|q| format!("- {q}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            // The questions this slice's OWN lanes raised and nobody chased, verbatim, for the
            // builder to settle — r6b's 48 such questions reached no builder at all. Empty
            // when nothing was raised (no heading, no filler).
            brief.push_str(&raised_questions_brief_block(&slice_rows));
            // OPEN-1, the detailing half: the opener saw an orientation index, so the FULL text
            // of each section this slice claimed is spliced here by CODE — the builder reads
            // the spec's own words, never a planner paraphrase. A slice that claimed nothing
            // gets the orientation plus a stated absence (the fallback rule): the map exists
            // even when the owner named no territory, and the caller emits the event.
            if armed {
                let spliced = splice_claimed_sections(&sl.id, &sl.sections, &sections, events);
                if spliced.is_empty() {
                    brief.push_str(&format!(
                        "\n\nSPEC SECTIONS: this slice claimed none — the orientation index of \
                         the whole request follows; the sections' full text lives in the \
                         request itself:\n{}",
                        spec_orientation(&sections)
                    ));
                } else {
                    brief.push_str(&format!(
                        "\n\nTHE SPEC'S OWN SECTIONS FOR THIS SLICE — verbatim, and the \
                         authority over any paraphrase above:{spliced}"
                    ));
                }
                // VA-008: the sections this slice CALLS INTO and the rules that bind every
                // slice — the same helper the research prompt uses, here with the plan-wide
                // view (every slice's claims, this slice's declared files).
                let consumed = consumed_spec_sections(
                    &sl.id,
                    &sl.sections,
                    &files,
                    &every_claim,
                    &sections,
                    events,
                );
                brief.push_str(&consumed_sections_blocks(&consumed));
            }
            brief.push_str(&slice_decisions_block(
                plan_decisions,
                &consumers,
                slice_index,
                &vocabularies[slice_index],
            ));
            // The one-line settled digest slice_index renders for SYNTHESIS — answered/total
            // plus the first answer's head, cut exactly the way the index's own summary line
            // is (non-empty lines joined, sentence-end cut within 400).
            let settled = if slice_rows.is_empty() {
                String::new()
            } else {
                let first_answer_head = sl.questions.iter().enumerate().find_map(|(i, _)| {
                    answered.get(&i).map(|r| {
                        head_to_sentence_end(
                            &r.answer
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .take(3)
                                .collect::<Vec<_>>()
                                .join(" "),
                            400,
                        )
                    })
                });
                match first_answer_head {
                    Some(h) => {
                        format!("{}/{} — {}", answered.len(), slice_rows.len(), h.trim_end())
                    }
                    None => format!("{}/{}", answered.len(), slice_rows.len()),
                }
            };
            SliceBrief {
                id: sl.id.clone(),
                title: sl.title.clone(),
                objective: sl.objective.clone(),
                brief,
                files,
                settled,
            }
        })
        .collect()
}

/// The fan's phase announcement — the stderr banner AND the `phase` event run.jsonl readers
/// fold (tick.py's phase line, the panel's ribbon via ENGINE_PHASE). Before this the banner was
/// console-only and a 30-minute fan ran under `phase=ask` in every instrument (r6c). Called once,
/// when the fan has something to dispatch — a fully-resumed fan announces no phase it does not run.
pub(super) fn announce_research_phase(events: &dyn EventSink) {
    phase_banner(
        "RESEARCH",
        "one lane per slice answers that slice's questions in one session; slices queue across \
         the hosts",
    );
    events.write_value(serde_json::json!({"event": "phase", "phase": "research"}));
}

pub(super) fn research_system_text() -> String {
    "You are answering the tagged QUESTIONS of ONE slice of this request — all of them, in this \
     one session; each must be settled before the slice is built. Ground every answer: read the \
     request text you were given, read the existing tree's files with your shell and tree \
     tools, and when the request names a documentation URL, fetch it — an answer copied from the \
     real source beats any paraphrase. Do NOT create or edit files: you have no write or edit \
     tool, and your structured reply IS your deliverable.\n\n\
     Each answer is a HANDOFF to the builder: name exact files, exact key/field literals, exact \
     endpoints or signatures where the request implies them; where the request is silent, state \
     the most CONVENTIONAL choice and say it is a convention. Before you call anything a \
     convention or raise it as not frozen, check the orientation index for a section that names \
     it and read that section from the request file named under SOURCES — silence in your \
     excerpt is not silence in the request. The questions of one slice overlap: settle a shared \
     fact ONCE and let the later answers refer back to it, never contradict it. If a question \
     cannot be settled from the request or the sources, say exactly that in one line and still \
     name the conventional choice. Keep each answer under a page.\n\n\
     When ALL of them are done, call the final_output tool ONCE with {\"answers\": \
     [{\"question_index\": N, \"answer\": \"...\", \"raised\": [...]}, ...]} — one entry per \
     [qN] tag with question_index = N, in any order. A tag you omit is recorded as UNANSWERED, \
     so include every one, even as \"cannot be settled: <why>; convention: <choice>\". `raised` \
     lists further questions you could NOT settle: do not answer them, and nothing will dispatch \
     them; they are handed VERBATIM to the builder of this slice as open points, so phrase each \
     as a decision that builder can make in one line, naming the conventional choice when you \
     have one."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::opener::OpenDecision;
    use super::super::{unclaimed_sections, NullSink, OpenSlice, SwarmEvent};
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
            kind: "design".to_string(),
            cite: String::new(),
            origin: String::new(),
            batch: 0,
        }
    }

    fn rq(slice: &str, q_index: usize, question: &str) -> ResearchQuestion {
        ResearchQuestion::of(slice, q_index, &OpenQuestion::from(question))
    }

    /// The one-question fold the pre-C3 tests were written against: a batch of one, its row
    /// (the run path folds through `fold_research_batch` and names strays; a one-question batch
    /// has none worth acting on).
    fn fold_research_outcome(
        q: &ResearchQuestion,
        model: &str,
        secs: u64,
        out: Result<String, String>,
    ) -> ResearchRow {
        let (mut rows, _) = fold_research_batch(std::slice::from_ref(q), model, secs, out);
        rows.remove(0)
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
        let q = rq("viz-field", 3, "How is the Europe/Berlin day computed?");
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

    /// A5 + A6 + fix A: when orientation is armed the research prompt carries the orientation
    /// index and the slice's claimed sections' FULL text — NEVER the raw spec — plus the USER
    /// DECISIONS block, the OWNED framing that points past the index, the SOURCES block naming
    /// the on-disk request file, and the question verbatim under THE QUESTION. Below the floor
    /// the whole spec is the input, as-is.
    #[test]
    fn research_prompt_carries_the_index_the_request_path_and_never_the_raw_spec_when_armed() {
        let filler =
            "This sentence pads the specification body across the arming floor. ".repeat(100);
        let spec = format!(
            "# Alpha\n{filler}The deep claimed fact is CLAIMED_DEEP_MARKER.\n\n\
             # Beta\n{filler}The deep unclaimed fact is UNCLAIMED_DEEP_MARKER.\n\n\
             # Gamma\nA short tail section.\n"
        );
        let sections = spec_sections(&spec);
        assert!(
            orientation_armed(&spec, &sections),
            "the fixture must cross the real arming floor"
        );
        let block = research_request_block(
            &spec,
            &sections,
            true,
            "s1",
            &["Alpha".to_string()],
            &[],
            &[],
            &NullSink,
        );
        let tree = vec!["app/__main__.py".to_string()];
        let sources =
            research_sources_block(Some(Path::new("/run/.swarm/request.md")), &spec, &tree);
        let head = research_prompt_head(
            &block,
            "payments",
            "the payments service",
            "sync payments from the vendor",
            "Q: which separator?\nA: pipe-separated CSV.\n",
            &tree,
            &sources,
        );
        let q = rq(
            "payments",
            0,
            "What is the frozen payment record structure from section 2?",
        );
        let text = research_user_text(&head, "", std::slice::from_ref(&q));
        assert!(
            text.contains("CLAIMED_DEEP_MARKER"),
            "the claimed section's FULL text rides in"
        );
        assert!(
            !text.contains("UNCLAIMED_DEEP_MARKER"),
            "an unclaimed section arrives only as its orientation head — the raw spec never rides"
        );
        assert!(
            text.contains("## Beta ["),
            "the index names the unclaimed section by heading, with its size"
        );
        assert!(
            text.contains("the sections this slice OWNS")
                && text.contains("open that section in the request file named under SOURCES"),
            "claimed = owned; the index names the rest; the file is where to read it:\n{text}"
        );
        assert!(
            text.contains("SOURCES — what is research material here")
                && text.contains("on disk at `/run/.swarm/request.md`"),
            "the request's on-disk path rides in every research prompt"
        );
        assert!(
            text.contains("USER DECISIONS") && text.contains("pipe-separated CSV"),
            "the ASK handshake's decisions inform every research call (A6)"
        );
        assert!(
            text.contains("THE QUESTIONS (1) — answer EVERY one of them in this session")
                && text.ends_with(
                    "\n[q0] What is the frozen payment record structure from section 2?"
                ),
            "the batch tail, tagged by q_index:\n{text}"
        );
        assert!(
            text.contains("app/__main__.py"),
            "the existing tree rides in"
        );
        // Below the floor: the spec as-is is the better input, exactly like OPEN's own message.
        let small = "build a tiny thing";
        let small_block = research_request_block(
            small,
            &spec_sections(small),
            false,
            "s1",
            &[],
            &[],
            &[],
            &NullSink,
        );
        assert_eq!(small_block, format!("THE REQUEST:\n{small}"));
    }

    /// Fix B, the snowball inside the fan, on r6c's own shape: a FIRST dispatch carries no
    /// prior block (no heading, no filler) and `research_context` says 0; a later dispatch of
    /// the same slice carries the earlier lane's mini; another slice's mini rides only when the
    /// two QUESTIONS name the same path (`/api/health` — ledgerd-api-q0 had the exact Health
    /// shape ledgerd-core-q2 invented); an unanswered mini and an unrelated stranger never
    /// ride; a decision lane's text lands under THE OPEN DECISION.
    #[test]
    fn a_later_dispatch_carries_the_earlier_minis_and_a_first_dispatch_carries_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sink = ValueSink::default();
        let q0 = rq(
            "ledgerd-core",
            0,
            "What is the exact ledger.db schema and index set?",
        );
        let first = research_dispatch_text(
            root,
            &sink,
            "HEAD",
            std::slice::from_ref(&q0),
            "research-ledgerd-core",
            28,
        );
        assert!(
            first.starts_with("HEAD\n\nTHE QUESTIONS (1)")
                && first.ends_with("\n[q0] What is the exact ledger.db schema and index set?")
                && !first.contains("ALREADY ANSWERED"),
            "a first dispatch: head, the tagged question, nothing invented between them:\n{first}"
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev.len(), 1);
            assert_eq!(ev[0]["event"], "research_context");
            assert_eq!(ev[0]["task"], "research-ledgerd-core");
            assert_eq!(ev[0]["q_indexes"], serde_json::json!([0]));
            assert_eq!(ev[0]["questions"], 1);
            assert_eq!(ev[0]["prior_minis"], 0);
            assert_eq!(ev[0]["index_sections"], 28);
            assert_eq!(ev[0]["prior_from"], serde_json::json!([]));
        }
        // Lanes finish and write their minis: q0 (same slice), api-q0 (another slice, its
        // question names /api/health), core-q3 unanswered, web-q0 an unrelated stranger.
        let mut core_q0 = row("ledgerd-core", 0, RESEARCH_ANSWERED, &[]);
        core_q0.answer =
            "Cursor state is persisted durably in ledger.db (sync_state table).".into();
        write_research_ledger(root, &core_q0).unwrap();
        let mut api_q0 = row("ledgerd-api", 0, RESEARCH_ANSWERED, &[]);
        api_q0.question =
            "What are the exact response shapes for /api/health, /api/summary, and /api/buckets?"
                .into();
        api_q0.answer = "GET /api/health: {\"status\": \"ok\", \"payments\": <int>, \
                         \"last_sync\": <str or null>, \"webhook\": {...}}"
            .into();
        write_research_ledger(root, &api_q0).unwrap();
        let core_q3 = row(
            "ledgerd-core",
            3,
            RESEARCH_UNANSWERED,
            &["where are the counters exposed?"],
        );
        write_research_ledger(root, &core_q3).unwrap();
        let mut web_q0 = row("web-console", 0, RESEARCH_ANSWERED, &[]);
        web_q0.question = "Which filter params does the table use?".into();
        web_q0.answer = "status and currency, from section 7.".into();
        write_research_ledger(root, &web_q0).unwrap();

        let q1 = rq(
            "ledgerd-core",
            1,
            "How is sync cursor state persisted so a dropped connection resumes?",
        );
        let second = research_dispatch_text(
            root,
            &sink,
            "HEAD",
            std::slice::from_ref(&q1),
            "research-ledgerd-core",
            28,
        );
        assert!(
            second.contains("ALREADY ANSWERED BY THIS FAN before your dispatch (1)"),
            "the real count:\n{second}"
        );
        assert!(second.contains(
            "[this slice's own earlier lane; .swarm/ledger/research-ledgerd-core-q0.json]"
        ));
        assert!(second.contains("A: Cursor state is persisted durably in ledger.db"));
        assert!(
            !second.contains("/api/health")
                && !second.contains("section 7")
                && !second.contains("counters exposed"),
            "no shared path, unrelated, and unanswered rows stay out:\n{second}"
        );
        assert!(
            second.find("ALREADY ANSWERED").unwrap() < second.find("THE QUESTIONS (1)").unwrap(),
            "the snowball precedes the question"
        );

        let q2 = rq(
            "ledgerd-core",
            2,
            "What does /api/health expose as the degraded state while the vendor is down?",
        );
        // C3: q1 and q2 ride ONE lane — the batch's snowball is the union: q0 (own slice, not
        // in the batch) and api-q0 (a stranger naming q2's /api/health), each once.
        let third = research_dispatch_text(
            root,
            &sink,
            "HEAD",
            &[q1.clone(), q2.clone()],
            "research-ledgerd-core",
            28,
        );
        assert!(third.contains("before your dispatch (2)"), "{third}");
        assert!(third.contains(
            "[slice `ledgerd-api` — its question names the same path as one of yours; \
             .swarm/ledger/research-ledgerd-api-q0.json]"
        ));
        assert!(
            third.contains("THE QUESTIONS (2)")
                && third.contains("\n[q1] How is sync cursor state persisted")
                && third.ends_with("\n[q2] What does /api/health expose as the degraded state while the vendor is down?"),
            "both questions, tagged, in batch order:\n{third}"
        );
        assert!(
            third.contains("\"payments\": <int>"),
            "the exact Health shape reaches the lane that invented one in r6c"
        );
        assert!(
            third.find("research-ledgerd-core-q0.json").unwrap()
                < third.find("research-ledgerd-api-q0.json").unwrap(),
            "own slice first, then the path-matched stranger"
        );
        assert!(
            !third.contains("section 7"),
            "a stranger with no shared path stays out"
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev.len(), 3);
            assert_eq!(ev[2]["prior_minis"], 2);
            assert_eq!(ev[2]["q_indexes"], serde_json::json!([1, 2]));
            assert_eq!(
                ev[2]["prior_from"],
                serde_json::json!([
                    "research-ledgerd-core-q0.json",
                    "research-ledgerd-api-q0.json"
                ])
            );
        }
        let d = [
            ResearchQuestion::decision(0, "D2: is rejected terminal?"),
            ResearchQuestion::decision(2, "D3: empty-with-progress or loading?"),
        ];
        let decision = research_dispatch_text(root, &sink, "HEAD", &d, "research-decisions", 0);
        assert!(
            decision.contains("THE OPEN DECISIONS (2)")
                && decision.contains("\n[q0] D2: is rejected terminal?")
                && decision.ends_with("\n[q2] D3: empty-with-progress or loading?"),
            "{decision}"
        );
        assert!(
            !decision.contains("ALREADY ANSWERED"),
            "no decision settled yet and no question shares a path — nothing is spliced"
        );
    }

    /// C3's fold, on a three-question batch: one answered, one blank, one the reply skipped
    /// (named "no entry for [qN]"), plus a stray tag the batch never carried (returned, not
    /// dropped) and a duplicate tag (the second is a stray); the pre-C3 single shape is the
    /// answer for a ONE-question batch and an attributed-to-nobody miss for a larger one; an
    /// Err reaches every row; `secs` is the session's on every row and `batch` its size.
    #[test]
    fn a_batched_reply_folds_to_one_terminal_row_per_question_and_names_strays() {
        let qs = [
            rq("ledger-api", 1, "sort keys?"),
            rq("ledger-api", 2, "SSE framing?"),
            rq("ledger-api", 4, "static hosting?"),
        ];
        let reply = serde_json::json!({"answers": [
            {"question_index": 4, "answer": "text/html; charset=utf-8 for index.html", "raised": ["cache headers?"]},
            {"question_index": 1, "answer": "   "},
            {"question_index": 7, "answer": "an answer to a question this lane never carried"},
            {"question_index": 4, "answer": "a second entry for q4"},
        ]})
        .to_string();
        let (rows, strays) = fold_research_batch(&qs, "m", 1800, Ok(reply));
        assert_eq!(rows.len(), 3, "one row per question, in batch order");
        assert_eq!(rows[0].q_index, 1);
        assert_eq!(rows[0].status, RESEARCH_UNANSWERED);
        assert_eq!(rows[0].reason.as_deref(), Some("empty_answer"));
        assert_eq!(rows[1].q_index, 2);
        assert_eq!(rows[1].reason.as_deref(), Some("empty_answer"));
        assert_eq!(
            rows[1].detail.as_deref(),
            Some("the lane's reply carried no entry for [q2]")
        );
        assert_eq!(rows[2].q_index, 4);
        assert_eq!(rows[2].status, RESEARCH_ANSWERED);
        assert_eq!(rows[2].answer, "text/html; charset=utf-8 for index.html");
        assert_eq!(rows[2].raised, vec!["cache headers?".to_string()]);
        assert!(rows
            .iter()
            .all(|r| r.secs == 1800 && r.batch == 3 && r.model == "m"));
        assert_eq!(strays.len(), 2, "{strays:?}");
        assert_eq!(strays[0].question_index, Some(7));
        assert_eq!(
            strays[1].question_index,
            Some(4),
            "a duplicate tag is a stray"
        );
        assert!(strays[1].answer_head.starts_with("a second entry"));
        // The pre-C3 single shape on a one-question batch IS the answer.
        let one = [rq("web-page", 0, "tokens?")];
        let (rows, strays) = fold_research_batch(
            &one,
            "m",
            5,
            Ok(r##"{"answer": "#role-token input", "raised": []}"##.into()),
        );
        assert_eq!(rows[0].status, RESEARCH_ANSWERED);
        assert_eq!(rows[0].answer, "#role-token input");
        assert!(strays.is_empty());
        assert_eq!(
            fold_research_outcome(&one[0], "m", 5, Ok(r#"{"answer": "x"}"#.into())).answer,
            "x",
            "the one-question wrapper keeps its name and shape"
        );
        // The single shape on a larger batch attributes to nobody, and says so.
        let (rows, _) = fold_research_batch(&qs, "m", 5, Ok(r#"{"answer": "one blob"}"#.into()));
        assert!(rows.iter().all(|r| r.status == RESEARCH_UNANSWERED
            && r.reason.as_deref() == Some("empty_answer")
            && r.detail
                .as_deref()
                .is_some_and(|d| d.contains("answered 3 questions with ONE"))));
        // Nothing parseable: every row carries the raw head.
        let (rows, _) = fold_research_batch(&qs, "m", 5, Ok("I could not decide.".into()));
        assert!(rows
            .iter()
            .all(|r| r.detail.as_deref() == Some("I could not decide.")));
        // The engine's own ending and a transport error reach every row, named apart.
        let (rows, _) = fold_research_batch(
            &qs,
            "m",
            5,
            Err(format!("{JUDGE_ENDED_NEEDLE}: out of moves")),
        );
        assert!(rows
            .iter()
            .all(|r| r.reason.as_deref() == Some("judge_ended")));
        let (rows, _) = fold_research_batch(&qs, "m", 5, Err("connection reset".into()));
        assert!(rows
            .iter()
            .all(|r| r.reason.as_deref() == Some("provider_error")
                && r.detail.as_deref() == Some("connection reset")));
        // The lanes: consecutive same-slice questions group, decisions last as one lane, the
        // head carried once, never an empty batch.
        let queue = vec![
            (rq("ledger-core", 0, "a"), "H-core".to_string()),
            (rq("ledger-core", 2, "b"), "H-core".to_string()),
            (rq("web-page", 1, "c"), "H-web".to_string()),
            (ResearchQuestion::decision(0, "d0"), "H-dec".to_string()),
            (ResearchQuestion::decision(1, "d1"), "H-dec".to_string()),
        ];
        let lanes = batch_by_slice(queue);
        assert_eq!(lanes.len(), 3);
        assert_eq!(
            lanes[0].0.iter().map(|q| q.q_index).collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(lanes[0].1, "H-core");
        assert_eq!(lanes[1].0.len(), 1);
        assert_eq!(lanes[2].0.len(), 2);
        assert_eq!(lanes[2].0[0].slice, DECISION_SLICE);
        assert!(batch_by_slice(Vec::new()).is_empty());
    }

    /// Fix C: the SOURCES block is derived from THIS run — the request file (or its stated
    /// absence), the vendor docs URL the spec names, the tree's top-level entries — and names
    /// `.swarm/` as engine state, not research material (r6c: research-ledgerd-api-q1 read the
    /// activity log as evidence). `persist_request_text` puts the request where the block says,
    /// and a write that cannot land is a NAMED absence.
    #[test]
    fn the_sources_block_derives_from_the_run_and_names_swarm_as_engine_state() {
        let spec = "Build it. The Meridian API v3 documentation is at \
                    `http://127.0.0.1:8850/v3/docs`. Base URL `http://127.0.0.1:8850/v3`.";
        let tree = vec![
            "app/db.py".to_string(),
            "app/api.py".to_string(),
            "web/app.js".to_string(),
            "README.md".to_string(),
        ];
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let path = persist_request_text(dir.path(), spec, &sink).expect("the request file writes");
        assert!(path.ends_with(".swarm/request.md"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), spec);
        assert!(
            sink.0.lock().unwrap().is_empty(),
            "a clean write emits nothing"
        );
        let block = research_sources_block(Some(path.as_path()), spec, &tree);
        assert!(block.contains(&format!("on disk at `{}`", path.display())));
        assert!(block.contains(&format!("grep -n '^#' {}", path.display())));
        assert!(block.contains("documentation is at http://127.0.0.1:8850/v3/docs"));
        assert!(
            block.contains("live under: README.md, app/, web/."),
            "top-level entries, derived and deduped:\n{block}"
        );
        assert!(
            block.contains("`.swarm/` is this engine's own state")
                && block.contains("your own calls are being recorded there")
        );
        let greenfield = research_sources_block(None, "build a tiny thing", &[]);
        assert!(
            greenfield.contains("could NOT be written to disk")
                && greenfield.contains("research_request_not_persisted"),
            "a missing request file is stated, never pointed at:\n{greenfield}"
        );
        assert!(
            !greenfield.contains("documentation is at")
                && !greenfield.contains("Files already on disk"),
            "nothing the run lacks is named"
        );
        assert!(greenfield.contains("`.swarm/` is this engine's own state"));
        let file_as_root = dir.path().join("a-file");
        std::fs::write(&file_as_root, "x").unwrap();
        assert!(persist_request_text(&file_as_root, spec, &sink).is_none());
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["event"], "research_request_not_persisted");
        assert!(ev[0]["error"].as_str().is_some_and(|e| !e.is_empty()));
    }

    /// Fix D: the fan's phase reaches run.jsonl — ONE `phase: research` event beside the banner
    /// — so tick.py's phase line and the panel's ribbon (ENGINE_PHASE maps `research`) stop
    /// showing a half-hour fan under `ask` (r6c). The fan calls the announcer at exactly one
    /// site, the point where it has something to dispatch.
    #[test]
    fn the_research_phase_is_announced_once_with_its_event() {
        let sink = ValueSink::default();
        announce_research_phase(&sink);
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            ev[0],
            serde_json::json!({"event": "phase", "phase": "research"})
        );
        let fan_src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/swarm.rs"
        ))
        .unwrap();
        assert_eq!(
            fan_src.matches("announce_research_phase(").count(),
            1,
            "one announcement site, at the fan's dispatch point"
        );
    }

    /// The fan's queue as an event: the total, the split between what dispatches now and what
    /// resumed settled from the ledger, and the per-slice count — derived from the queue, so
    /// the vigil stops counting question marks in the opener's output (r6c).
    #[test]
    fn the_planned_queue_is_emitted_once_with_its_per_slice_counts() {
        let sink = ValueSink::default();
        let dispatching = vec![
            rq("ledgerd-core", 1, "q"),
            rq("ledgerd-core", 2, "q"),
            ResearchQuestion::decision(0, "d"),
        ];
        let resumed = vec![row("ledgerd-core", 0, RESEARCH_ANSWERED, &[])];
        emit_research_planned(&sink, &dispatching, &resumed, 2, 2);
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            ev[0],
            serde_json::json!({
                "event": "research_planned",
                "questions": 4,
                "dispatching": 3,
                "resumed": 1,
                "facts": 2,
                "lanes": 2,
                "per_slice": {"__open_decisions__": 1, "ledgerd-core": 3},
            }),
            "facts are counted beside the lane denominator, never inside it; lanes = sessions"
        );
        let fan_src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/swarm.rs"
        ))
        .unwrap();
        assert_eq!(
            fan_src.matches("emit_research_planned(").count(),
            1,
            "one emission site, when the fan's queue is built"
        );
    }

    /// r6d's first tick (run swarm-20260901-035310576, seq 93/94): the opener claimed
    /// "vs7dbg — REQUIRED and graded" for web-page AND viz-field; request.md:718 reads
    /// "#### `vs7dbg` — REQUIRED and graded" and the exact-string compare missed twice, so the
    /// 1,148-char section of the graded debug API reached neither brief (the splice returns
    /// only matched sections — an unmatched claim contributes NOTHING to the brief) nor either
    /// slice's research prompts. The heading and the claim below are r6d's, verbatim.
    #[test]
    fn a_claim_without_the_headings_backticks_still_splices_its_section() {
        let spec = "### 8. The 3D field — 12,288 instances, five mechanisms\nfield intro\n\n\
                    #### `vs7dbg` — REQUIRED and graded\n\
                    A global `vs7dbg` object with all-synchronous methods.\n\n\
                    ### 9. `DECISIONS.md` — three corners you must decide\ndecide three\n";
        let sections = spec_sections(spec);
        let sink = ValueSink::default();
        let claimed = [
            "vs7dbg — REQUIRED and graded".to_string(),
            // dash variant, bold, trailing colon, leading hashes: decoration, not identity
            "8. The 3D field - 12,288 instances, five mechanisms".to_string(),
            "#### **9. `DECISIONS.md` — three corners you must decide**:".to_string(),
        ];
        let spliced = splice_claimed_sections("viz-field", &claimed, &sections, &sink);
        assert!(spliced.contains("all-synchronous methods"), "{spliced}");
        assert!(spliced.contains("field intro"), "{spliced}");
        assert!(spliced.contains("decide three"), "{spliced}");
        assert!(
            spliced.contains("### `vs7dbg` — REQUIRED and graded"),
            "the spec's own heading is what the brief shows: {spliced}"
        );
        assert!(
            sink.0.lock().unwrap().is_empty(),
            "no miss: {:?}",
            sink.0.lock().unwrap()
        );
        // A typo is still a miss — letters do not fold.
        let typo = ["vs7dgb — REQUIRED and graded".to_string()];
        assert!(splice_claimed_sections("viz-field", &typo, &sections, &sink).is_empty());
        assert_eq!(sink.0.lock().unwrap().len(), 1);
        // On the real sb-7 spec (line 718 is that heading), r6d's exact claim splices the section.
        let sb7 = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let real = splice_claimed_sections(
            "viz-field",
            &["vs7dbg — REQUIRED and graded".to_string()],
            &spec_sections(sb7),
            &ValueSink::default(),
        );
        assert!(real.contains("vs7dbg"), "{real}");
        assert!(
            real.chars().count() > 1_000,
            "the whole graded-API section, not an excerpt: {} chars",
            real.chars().count()
        );
        // The coverage gap agrees: the backtick-free claims cover the decorated headings.
        let opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "viz-field".to_string(),
                title: String::new(),
                objective: String::new(),
                questions: Vec::new(),
                weight: 5,
                sections: claimed.to_vec(),
            }],
            open_decisions: Vec::new(),
        };
        assert!(
            super::super::unclaimed_sections(&opened, &sections).is_empty(),
            "{:?}",
            super::super::unclaimed_sections(&opened, &sections)
        );
    }

    /// THE r6c SHAPE, under C3's one-lane-per-slice: ledger-api's lane lands its q0 (the exact
    /// /api/health shape) while the ledger-core lane (q1 cursor, q2 "what does /api/health
    /// expose") and the web-page lane (q0 brush) are still running. The relay reaches the core
    /// lane — one of its questions names the same path — and only it: not web-page (no shared
    /// path), never a lane of the landed row's own slice, never for an unanswered row, never
    /// for a landed question that names no path. The note carries the mini's path, the slice,
    /// the question and the budgeted answer.
    #[test]
    fn a_landed_mini_is_relayed_to_running_lanes_whose_questions_name_its_path() {
        let running = vec![
            (
                "research-ledger-core".to_string(),
                vec![
                    rq("ledger-core", 1, "How is sync cursor state persisted?"),
                    rq(
                        "ledger-core",
                        2,
                        "What does /api/health expose as the degraded state?",
                    ),
                ],
            ),
            (
                "research-web-page".to_string(),
                vec![rq(
                    "web-page",
                    0,
                    "Does the brush survive a streamed mutation?",
                )],
            ),
            (
                "research-ledger-api".to_string(),
                vec![rq(
                    "ledger-api",
                    3,
                    "Which header verifies signed webhooks?",
                )],
            ),
        ];
        let mut landed = row("ledger-api", 0, RESEARCH_ANSWERED, &[]);
        landed.question =
            "What are the exact response shapes for /api/health, /api/summary and /api/buckets?"
                .into();
        landed.answer = "GET /api/health: {\"status\": \"ok\", \"payments\": <int>, \
                         \"last_sync\": <str or null>, \"webhook\": {...}}"
            .into();
        assert_eq!(
            relay_targets(&landed, &running),
            vec!["research-ledger-core".to_string()]
        );
        let note = relay_note(&landed);
        assert_eq!(note.from_mini, "research-ledger-api-q0.json");
        assert!(note.text.starts_with("A MINI LANDED ("));
        assert!(note
            .text
            .contains("the lane of slice `ledger-api` settled this"));
        assert!(note
            .text
            .contains(".swarm/ledger/research-ledger-api-q0.json"));
        assert!(note
            .text
            .contains("Q: What are the exact response shapes for /api/health"));
        assert!(note
            .text
            .contains("A: GET /api/health: {\"status\": \"ok\""));
        assert!(note
            .from_question
            .starts_with("What are the exact response shapes"));
        let missed = row("ledger-api", 0, RESEARCH_UNANSWERED, &[]);
        assert!(
            relay_targets(&missed, &running).is_empty(),
            "a miss relays nothing"
        );
        assert!(
            relay_targets(&landed, &[]).is_empty(),
            "no running lane, no relay"
        );
        let mut pathless = landed.clone();
        pathless.question = "Is the dedupe key the event seq alone?".into();
        assert!(
            relay_targets(&pathless, &running).is_empty(),
            "a landed question naming no path links to nobody"
        );
    }

    /// OPEN-1's detailing half: a slice's claimed sections arrive in its brief VERBATIM (the
    /// builder reads the spec's own words, not a planner paraphrase); a slice that claimed none
    /// gets the orientation map plus a stated absence (the fallback rule), and
    /// unclaimed_sections measures the coverage gap deterministically.
    #[test]
    fn a_slice_gets_its_claimed_sections_verbatim_and_absence_is_stated() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sections = spec_sections(spec);
        let table_heading = sections
            .iter()
            .find(|s| s.body.contains("/api/payments"))
            .expect("the endpoint table has a section")
            .heading
            .clone();
        let opened = OpenOutput {
            slices: vec![
                OpenSlice {
                    id: "ledger".into(),
                    title: "ledger".into(),
                    objective: "the ledger service".into(),
                    questions: Vec::new(),
                    weight: 3,
                    sections: vec![table_heading.clone()],
                },
                OpenSlice {
                    id: "web".into(),
                    title: "web".into(),
                    objective: "the dashboard".into(),
                    questions: Vec::new(),
                    weight: 2,
                    sections: Vec::new(),
                },
            ],
            open_decisions: Vec::new(),
        };
        let briefs = briefs_from_slices(&opened, spec, &[], &[], &NullSink);
        assert!(
            briefs[0].brief.contains("THE SPEC'S OWN SECTIONS")
                && briefs[0].brief.contains("/api/payments"),
            "the claimed section's full text rides in the brief"
        );
        assert!(
            briefs[1].brief.contains("this slice claimed none")
                && briefs[1].brief.contains(&table_heading),
            "the unclaiming slice gets the stated absence plus the orientation map"
        );
        let un = unclaimed_sections(&opened, &sections);
        assert!(
            !un.is_empty() && !un.contains(&table_heading),
            "the coverage gap is measured: {} unclaimed, claimed one absent",
            un.len()
        );
    }

    /// The opener declares ownership in objective text (14831a321) and the engine reads it
    /// back. Before `files_from_objective`, `briefs_from_slices` shipped `files: Vec::new()`
    /// unconditionally, so `slice_files_unnamed` fired for EVERY slice on EVERY run (measured
    /// 5 and 7 on the last two) and the index's named-files caption was a dead path.
    #[test]
    fn objective_ownership_declarations_populate_slice_files() {
        let opened = OpenOutput {
            slices: vec![
                OpenSlice {
                    id: "ledgerd".into(),
                    title: "the ledger daemon".into(),
                    objective: "Owns `app/ledgerd/impl.py` (state machine) and `web/app.js` \
                                (the poller, again `web/app.js`). Serves GET `/api/ledger` \
                                (a route, not a file) per `https://example.test/docs`."
                        .into(),
                    questions: Vec::new(),
                    weight: 3,
                    sections: Vec::new(),
                },
                OpenSlice {
                    id: "web".into(),
                    title: "the dashboard".into(),
                    objective: "draw the dashboard".into(),
                    questions: Vec::new(),
                    weight: 2,
                    sections: Vec::new(),
                },
            ],
            open_decisions: Vec::new(),
        };
        let briefs = briefs_from_slices(&opened, "build the app", &[], &[], &NullSink);
        assert_eq!(
            briefs[0].files,
            vec!["app/ledgerd/impl.py".to_string(), "web/app.js".to_string()],
            "exactly the declared paths, deduped, in objective order — routes and URLs excluded"
        );
        // Empty is the ARMING CONDITION for `slice_files_unnamed`: synthesize_plan emits the
        // event per brief with `files.is_empty()`, so a slice that declared nothing still fires.
        assert!(
            briefs[1].files.is_empty(),
            "a declaration-free objective keeps the empty vec and the absence event"
        );
    }

    /// THE FAN CUT (C1) on r6d's ledger-api-q1 (a 5-minute lane for a fact request.md:148
    /// states outright): a cited spec fact is a terminal row with `origin: spec_fact`, no model,
    /// no seconds; the brief renders it under SPEC FACTS (cited) with its cite, ABOVE the lane
    /// answers, and the question leaves the QUESTIONS block; `research_question_kind` names the
    /// disposition with the cite; a lookup WITHOUT a fact is not a fact (it dispatches). The
    /// event funnel never emits `research_answered` for a fact — that count stays a lane count.
    #[test]
    fn a_cited_spec_fact_is_a_row_with_no_lane_and_renders_under_its_own_heading() {
        let fact: OpenQuestion = serde_json::from_value(serde_json::json!({
            "question": "Which sort keys does sort=<k> accept and in what direction(s); which status/currency values do the filters accept?",
            "kind": "spec_lookup",
            "cite": "request.md:148",
            "fact": "`status` filters to one of `settled`, `pending`, `refunded`, `failed`; `currency` to one of `EUR`, `USD`, `JPY`, `KWD`; `sort` is one of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`; default `created_at` (ascending by INSTANT)."
        }))
        .unwrap();
        let searched: OpenQuestion = serde_json::from_value(serde_json::json!({
            "question": "Static hosting: which content types (html/css/js/ico) and any cache headers?",
            "kind": "spec_lookup",
            "cite": "grep -n -i 'content-type\\|cache' request.md"
        }))
        .unwrap();
        assert!(fact.is_cited_fact() && !searched.is_cited_fact());
        let q1 = ResearchQuestion::of("ledger-api", 1, &fact);
        let row = ResearchRow::spec_fact(&q1);
        assert_eq!(row.status, RESEARCH_ANSWERED);
        assert_eq!(row.origin, ORIGIN_SPEC_FACT);
        assert_eq!(row.cite, "request.md:148");
        assert_eq!(row.kind, "spec_lookup");
        assert!(row.model.is_empty() && row.secs == 0, "nothing was called");
        let sink = ValueSink::default();
        emit_question_disposition(&sink, &q1, "fact");
        emit_question_disposition(
            &sink,
            &ResearchQuestion::of("ledger-api", 4, &searched),
            "dispatch",
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev[0]["event"], "research_question_kind");
            assert_eq!(ev[0]["disposition"], "fact");
            assert_eq!(ev[0]["cite"], "request.md:148");
            assert_eq!(ev[0]["fact"], true);
            assert_eq!(ev[1]["disposition"], "dispatch");
            assert_eq!(ev[1]["kind"], "spec_lookup");
            assert_eq!(ev[1]["fact"], false);
            assert!(ev[1]["cite"].as_str().unwrap().starts_with("grep -n"));
        }
        let opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "ledger-api".into(),
                title: "the api".into(),
                objective: "serve the endpoints".into(),
                questions: vec![
                    OpenQuestion::from("What are the exact /api/health shapes?"),
                    fact,
                    OpenQuestion::from("SSE framing: what is the first batch number?"),
                ],
                weight: 3,
                sections: Vec::new(),
            }],
            open_decisions: Vec::new(),
        };
        let mut lane = row_answered(
            "ledger-api",
            0,
            "GET /api/health: {\"status\": \"ok\", ...}",
        );
        lane.question = "What are the exact /api/health shapes?".into();
        let briefs = briefs_from_slices(&opened, "build the app", &[lane, row], &[], &NullSink);
        let b = &briefs[0].brief;
        let facts_at = b.find("SPEC FACTS (cited)").expect("the facts heading");
        let answers_at = b
            .find("ANSWERS SETTLED AT PLAN TIME")
            .expect("the lane answers");
        let questions_at = b
            .find("QUESTIONS this slice must settle")
            .expect("the open one");
        assert!(facts_at < answers_at && answers_at < questions_at, "{b}");
        assert!(b.contains("FACT: `status` filters to one of"));
        assert!(b.contains("CITE: request.md:148"));
        assert!(
            !b.split_at(questions_at).1.contains("Which sort keys"),
            "the fact left the QUESTIONS block:\n{b}"
        );
        assert!(b.split_at(questions_at).1.contains("- SSE framing"));
        assert!(
            briefs[0].settled.starts_with("2/2 — "),
            "the settled digest counts the fact: {}",
            briefs[0].settled
        );
    }

    fn row_answered(slice: &str, q_index: usize, answer: &str) -> ResearchRow {
        let mut r = row(slice, q_index, RESEARCH_ANSWERED, &[]);
        r.answer = answer.to_string();
        r
    }

    /// C2 in the brief, on r6d's web-page slice: web-q0 routed to the token decision renders
    /// under QUESTIONS THAT ARE OPEN DECISIONS pointing at decision #2 (and leaves the QUESTIONS
    /// block); a row covered by another slice's mini renders in ANSWERS SETTLED with a VIA line
    /// naming the ORIGINAL mini; a lane's own row carries no VIA; the settled digest counts the
    /// covered row (it is settled) and not the routed question (its settlement is the
    /// decision's, in the DECISIONS block).
    #[test]
    fn a_routed_question_points_at_its_decision_and_a_covered_row_names_its_source() {
        let tokens_line = "How the browser obtains the three bearer tokens for drafts endpoints — \
                           options: prompt field | config | hardcoded dev tokens in the page";
        let mut routed = OpenQuestion::from(
            "How does the browser obtain the three bearer tokens for drafts endpoints?",
        );
        routed.decision = Some(1);
        let mut opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "web-page".into(),
                title: "the page".into(),
                objective: "Owns `web/app.js`.".into(),
                questions: vec![
                    routed,
                    OpenQuestion::from("Do maker/checker see /api/events at all?"),
                    OpenQuestion::from("Notifications feed: SSE, polling, or both?"),
                ],
                weight: 3,
                sections: Vec::new(),
            }],
            open_decisions: vec![
                OpenDecision {
                    line: "HTTP framework — options: stdlib | Flask".into(),
                    options: vec!["stdlib".into(), "Flask".into()],
                },
                OpenDecision {
                    line: tokens_line.into(),
                    options: vec!["prompt field".into(), "config".into()],
                },
            ],
        };
        let mut api_q5 = row_answered(
            "ledger-api",
            5,
            "It requires a bearer token (any of the three roles).",
        );
        api_q5.cite = "request.md:218".into();
        let q1 = rq("web-page", 1, "Do maker/checker see /api/events at all?");
        let covered = ResearchRow::covered_by(&q1, &api_q5, "cite");
        let own = row_answered(
            "web-page",
            2,
            "Both: SSE-driven refresh, polling as fallback.",
        );
        // D10-7: a third question covered by a FACT row (the opener read request.md:148).
        opened.slices[0].questions.push(OpenQuestion::from(
            "Which sort keys does the table's sort control send?",
        ));
        let api_q1 = ResearchQuestion::of(
            "ledger-api",
            1,
            &serde_json::from_value(serde_json::json!({
                "question": "Which sort keys does sort=<k> accept?",
                "kind": "spec_lookup", "cite": "request.md:148",
                "fact": "`sort` is one of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`."
            }))
            .unwrap(),
        );
        let fact = ResearchRow::spec_fact(&api_q1);
        let q3 = rq(
            "web-page",
            3,
            "Which sort keys does the table's sort control send?",
        );
        let fact_covered = ResearchRow::covered_by(&q3, &fact, "cite");
        assert_eq!(
            fact_covered.cite, "request.md:148",
            "a fact cover hands over its cite"
        );
        let briefs = briefs_from_slices(
            &opened,
            "build the app",
            &[covered, own, fact_covered],
            &[],
            &NullSink,
        );
        let b = &briefs[0].brief;
        let decided_at = b
            .find("QUESTIONS THAT ARE OPEN DECISIONS")
            .expect("the routed block");
        assert!(b.contains("→ OPEN DECISION #2: How the browser obtains the three bearer tokens"));
        assert!(
            !b.contains("QUESTIONS this slice must settle"),
            "nothing stayed open: routed + covered + answered:\n{b}"
        );
        let answers_at = b.find("ANSWERS SETTLED AT PLAN TIME").unwrap();
        assert!(
            answers_at < decided_at,
            "settled facts first, then the pointers"
        );
        assert!(b.contains(
            "VIA: .swarm/ledger/research-ledger-api-q5.json — another slice asked the same question"
        ));
        assert!(
            b.contains(
                "VIA: .swarm/ledger/research-ledger-api-q1.json — answered from the request by the \
                 opener (FACT, CITE request.md:148); this is that fact"
            ),
            "a fact cover names the opener and the line, never a lane:\n{b}"
        );
        assert_eq!(
            b.matches("VIA:").count(),
            2,
            "the lane's own row carries no VIA line"
        );
        assert!(
            briefs[0].settled.starts_with("3/3 — "),
            "covered counts as settled; the routed question is the decision's: {}",
            briefs[0].settled
        );
        assert_eq!(briefs[0].files, vec!["web/app.js".to_string()]);
    }

    /// The brief partition: an answered question MOVES out of the QUESTIONS block into the
    /// settled-facts block above it; an unanswered one stays verbatim; with no research rows
    /// the brief is byte-identical to the pre-fan form; a long answer is spliced under the
    /// measured-good budget with a stated truncation naming the durable mini.
    #[test]
    fn answered_questions_move_from_questions_block_to_settled_facts() {
        let opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "api".into(),
                title: "the api".into(),
                objective: "serve GET /health".into(),
                questions: vec!["which port".into(), "which storage".into()],
                weight: 3,
                sections: Vec::new(),
            }],
            open_decisions: Vec::new(),
        };
        let rows = vec![
            ResearchRow {
                slice: "api".into(),
                q_index: 0,
                question: "which port".into(),
                status: RESEARCH_ANSWERED.into(),
                answer: "Port 8850, from the spec's own boot table.".into(),
                reason: None,
                detail: None,
                raised: Vec::new(),
                model: "m".into(),
                secs: 12,
                kind: "design".into(),
                cite: String::new(),
                origin: String::new(),
                batch: 0,
            },
            ResearchRow {
                slice: "api".into(),
                q_index: 1,
                question: "which storage".into(),
                status: RESEARCH_UNANSWERED.into(),
                answer: String::new(),
                reason: Some("provider_error".into()),
                detail: Some("connection reset".into()),
                raised: Vec::new(),
                model: "m".into(),
                secs: 3,
                kind: "design".into(),
                cite: String::new(),
                origin: String::new(),
                batch: 0,
            },
        ];
        let briefs = briefs_from_slices(&opened, "build the app", &rows, &[], &NullSink);
        let b = &briefs[0].brief;
        assert!(b.contains("ANSWERS SETTLED AT PLAN TIME"));
        assert!(b.contains("Q: which port") && b.contains("A: Port 8850"));
        let questions_at = b.find("QUESTIONS this slice must settle").unwrap();
        assert!(
            b.find("ANSWERS SETTLED AT PLAN TIME").unwrap() < questions_at,
            "the settled facts sit ABOVE the open questions"
        );
        let from_questions = b.split_at(questions_at).1;
        assert!(
            from_questions.contains("- which storage") && !from_questions.contains("- which port"),
            "the answered question left the QUESTIONS block; the unanswered one stayed:\n{b}"
        );
        assert_eq!(
            briefs[0].settled, "1/2 — Port 8850, from the spec's own boot table.",
            "the slice_index settled line carries answered/total and the first answer's head"
        );
        let plain = briefs_from_slices(&opened, "build the app", &[], &[], &NullSink);
        assert!(
            !plain[0].brief.contains("ANSWERS SETTLED")
                && plain[0].brief.contains("- which port")
                && plain[0].brief.contains("- which storage")
                && plain[0].settled.is_empty(),
            "no research rows => the pre-fan brief, byte for byte"
        );
        let long = "a fact line that keeps going and going.\n".repeat(60);
        let cut = budget_research_answer(&long, "api", 0);
        assert!(cut.chars().count() < long.chars().count());
        assert!(
            cut.contains("ANSWER TRUNCATED — full text in .swarm/ledger/research-api-q0.json"),
            "a cut answer says so and names the durable mini"
        );
        assert!(
            cut.lines()
                .rev()
                .nth(1)
                .unwrap()
                .ends_with("going and going."),
            "the cut lands on a line boundary"
        );
    }

    /// r5's silent 3,501-char loss, pinned. The boot slice claimed a typo'd heading; the splice
    /// loop found no match and dropped the claim from BOTH the research prompts and the brief
    /// with no per-slice signal — only the generic spec_sections_unclaimed fired, on the REAL
    /// heading. The shared splice now names each unmatched claim (loud, MILD, never blocks:
    /// the matching sections still splice), from both consumers.
    #[test]
    fn a_typoed_claimed_heading_is_named_not_silently_dropped() {
        let spec = "# Alpha\nalpha body text\n\n# Beta\nbeta body text\n";
        let sections = spec_sections(spec);
        let sink = ValueSink::default();
        let spliced = splice_claimed_sections(
            "boot",
            &["Alpha".to_string(), "Bta".to_string()],
            &sections,
            &sink,
        );
        assert!(
            spliced.contains("alpha body text"),
            "the matching claim still splices"
        );
        assert!(!spliced.contains("beta body text"), "no fuzzy substitution");
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1, "exactly the one miss is named");
        assert_eq!(
            events[0].get("event").and_then(|v| v.as_str()),
            Some("slice_claimed_section_unmatched")
        );
        assert_eq!(
            events[0].get("slice").and_then(|v| v.as_str()),
            Some("boot")
        );
        assert_eq!(
            events[0].get("claimed").and_then(|v| v.as_str()),
            Some("Bta"),
            "the typo VERBATIM, so the operator can see what to fix"
        );
    }

    /// RESEARCH FAN v2, the terminal fold: every outcome — a real answer, an empty or
    /// unparseable reply, a transport failure, the judge_out_of_moves ending — lands as a
    /// TERMINAL row (answered | unanswered + named reason), which is what makes "all dispatched
    /// questions terminal" reachable with no clock. A miss is a loud named absence, never a
    /// substituted answer (the fallback gate).
    #[test]
    fn research_terminal_fold_classifies_every_outcome() {
        let q = ResearchQuestion::of(
            "payments",
            0,
            &OpenQuestion::from("What is the frozen payment record structure from section 2?"),
        );
        let ok = fold_research_outcome(
            &q,
            "workhorse-q",
            7,
            Ok(r#"{"answer":"The record is {id, amount_minor, currency}.","raised":["what about refunds?"]}"#.into()),
        );
        assert_eq!(ok.status, RESEARCH_ANSWERED);
        assert!(ok.answer.contains("amount_minor"));
        assert_eq!(
            ok.raised,
            vec!["what about refunds?".to_string()],
            "raised questions are RECORDED on the row — never dispatched"
        );
        let empty = fold_research_outcome(&q, "m", 3, Ok(r#"{"answer":"  "}"#.into()));
        assert_eq!(
            (empty.status.as_str(), empty.reason.as_deref()),
            ("unanswered", Some("empty_answer")),
            "an empty reply is a named absence, never a stub answer"
        );
        let prose = fold_research_outcome(&q, "m", 3, Ok("no json at all".into()));
        assert_eq!(prose.reason.as_deref(), Some("empty_answer"));
        // Built FROM the shared needle, exactly as the emit site builds its Err — so this test
        // pins emit-site==matcher. Its own copy of the words would stay green through a
        // rewording that silently degraded every judge_ended lane to provider_error.
        let judge = fold_research_outcome(
            &q,
            "m",
            900,
            Err(format!("call {JUDGE_ENDED_NEEDLE}: 4 nudges")),
        );
        assert_eq!(
            (judge.status.as_str(), judge.reason.as_deref()),
            ("unanswered", Some("judge_ended")),
            "an engine-ended lane is named as such, not laundered into a transport failure"
        );
        let prov = fold_research_outcome(&q, "m", 3, Err("connection reset by peer".into()));
        assert_eq!(prov.reason.as_deref(), Some("provider_error"));
        assert_eq!(prov.detail.as_deref(), Some("connection reset by peer"));
        for row in [&ok, &empty, &prose, &judge, &prov] {
            assert!(
                row.status == RESEARCH_ANSWERED || row.status == RESEARCH_UNANSWERED,
                "every outcome is terminal"
            );
        }
    }

    /// r6c's five slices with the sections they claimed, verbatim from the run's plan_loaded
    /// task descriptions (the `### ` headings the splice wrote — a perfect 28-section
    /// partition, 0 overlaps) and the heads of their objectives (which declared no backticked
    /// files: `slice_files_unnamed` fired five times in r6c, so routing rests on the claimed
    /// BODIES exactly as it did there).
    fn r6c_slices() -> OpenOutput {
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
                    "Own app/__main__.py (single-command wrapper that boots BOTH services), \
                     app/ledgerd.py (ledgerd entrypoint), app/db.py (ledger.db schema), \
                     app/sync.py, app/ledger.py, app/outbox.py.",
                    &[
                        "Build `app` — Meridian Payments Console",
                        "What to build",
                        "1. The `app` package — two services, one boot contract",
                        "2. The collection you are syncing",
                        "3. `ledgerd` — vendor sync, event ledger, API, UI host",
                        "Sync discipline",
                        "The event ledger",
                        "The outbox",
                        "What WILL happen during a graded run",
                        "Consistency rules — graded continuously over the live run",
                        "Performance budgets",
                        "Rules",
                    ],
                ),
                slice(
                    "ledgerd-api",
                    "Own app/api.py (every ledgerd endpoint from the Endpoints table), \
                     app/webhooks.py, app/drafts.py, app/auth.py.",
                    &[
                        "Endpoints",
                        "Error envelope",
                        "4. Webhooks — the vendor calls YOU",
                        "5. The approval workflow — maker, checker, admin",
                        "Data → scene",
                        "Streaming diffs — SSE with byte accounting",
                    ],
                ),
                slice(
                    "notifierd",
                    "Own app/notifierd.py (service entrypoint per the boot contract) and \
                     app/notify_store.py.",
                    &["6. `notifierd` — the idempotent consumer"],
                ),
                slice(
                    "web-console",
                    "Own web/index.html (structure only), web/styles.css (all styling), \
                     web/app.js (page behavior: payments table with status/currency filters, \
                     sorting on the instant-backed fields, pagination).",
                    &[
                        "7. `web/` — the frontend",
                        "9. `DECISIONS.md` — three corners you must decide",
                    ],
                ),
                slice(
                    "web-viz",
                    "Own web/viz.js (the 3D engine and nothing else).",
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
            open_decisions: Vec::new(),
        }
    }

    /// VA-008, the r6c product-killer: with the opener's real 28-section partition against the
    /// same spec, the web-console brief carried §7 and §9 only — never `#### Endpoints`, where
    /// request.md:148 lists the four `sort` values — and app.js sent `sort=date_desc` (400,
    /// zero rows for 608 minutes). Now every section routes to its CONSUMERS too: web-console
    /// gets Endpoints (rule a: §7's body names `/api/viz/records`, `/api/sync`,
    /// `/api/notifications`) with `-created_at` in it; web-viz gets `Data → scene` and
    /// `Streaming diffs` (rule b: children of its claimed §8, which r6c gave to ledgerd-api);
    /// every brief gets the four cross-cutting `##` sections (rule c) — and nothing twice.
    #[test]
    fn r6c_s_partitioned_claims_route_each_section_to_its_consumers_too() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let opened = r6c_slices();
        let sections = spec_sections(spec);
        let sink = ValueSink::default();
        let briefs = briefs_from_slices(&opened, spec, &[], &[], &sink);
        let by_id = |id: &str| &briefs.iter().find(|b| b.id == id).unwrap().brief;

        let console = by_id("web-console");
        assert!(
            console.contains("SECTIONS THIS SLICE CALLS INTO"),
            "{console}"
        );
        assert!(console.contains("\n### Endpoints\n"), "{console}");
        assert!(
            console.contains("`-created_at`"),
            "the sort VALUES reach the slice that sends sort="
        );
        let viz = by_id("web-viz");
        assert!(viz.contains("\n### Data → scene\n"), "{viz}");
        assert!(
            viz.contains("\n### Streaming diffs — SSE with byte accounting\n"),
            "{viz}"
        );
        for b in &briefs {
            assert!(
                b.brief.contains("\n### Performance budgets\n"),
                "{}: every slice is graded on the budgets",
                b.id
            );
            for heading in [
                "Endpoints",
                "Performance budgets",
                "Rules",
                "Data → scene",
                "What WILL happen during a graded run",
            ] {
                let needle = format!("\n### {heading}\n");
                assert!(
                    b.brief.matches(&needle).count() <= 1,
                    "{}: {heading} spliced twice",
                    b.id
                );
            }
        }
        assert_eq!(
            by_id("ledgerd-core")
                .matches("CROSS-CUTTING SPEC RULES")
                .count(),
            0,
            "the slice that claimed the cross-cutting sections owns them; no second copy"
        );
        assert!(by_id("ledgerd-api").contains("CROSS-CUTTING SPEC RULES"));
        assert_eq!(
            by_id("ledgerd-api").matches("\n### Endpoints\n").count(),
            1,
            "the owner's own splice, once"
        );
        assert!(
            !by_id("ledgerd-api").contains("6. `notifierd`"),
            "notifierd's `/health` row is not found inside ledgerd's `/api/health` (token-bounded)"
        );
        assert!(
            by_id("ledgerd-core").contains("\n### Error envelope\n")
                && by_id("ledgerd-core").contains("\n### Endpoints\n"),
            "§3's children reach the slice that claimed §3 (r6c's core lanes invented the \
             health shape for want of them)"
        );

        // Where every section went, as events the tick reads beside spec_sections_unclaimed.
        let ev = sink.0.lock().unwrap();
        let consumed: Vec<&serde_json::Value> = ev
            .iter()
            .filter(|e| e["event"] == "spec_sections_consumed")
            .collect();
        let rule_for = |slice: &str, rule: &str| -> Vec<String> {
            consumed
                .iter()
                .filter(|e| e["slice"] == slice && e["rule"] == rule)
                .flat_map(|e| e["sections"].as_array().unwrap().iter())
                .map(|h| h.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            rule_for("web-console", "advertised_route"),
            vec!["Endpoints".to_string()]
        );
        assert_eq!(
            rule_for("web-viz", "child_of_claimed"),
            vec![
                "Data → scene".to_string(),
                "Streaming diffs — SSE with byte accounting".to_string()
            ]
        );
        assert_eq!(rule_for("notifierd", "cross_cutting").len(), 4);
        assert!(rule_for("ledgerd-core", "cross_cutting").is_empty());

        // The gate-8 table: own / called-into / cross-cutting section chars per r6c slice.
        let every_claim: Vec<&[String]> = opened
            .slices
            .iter()
            .map(|s| s.sections.as_slice())
            .collect();
        for sl in &opened.slices {
            let own = splice_claimed_sections(&sl.id, &sl.sections, &sections, &NullSink);
            let c = consumed_spec_sections(
                &sl.id,
                &sl.sections,
                &[],
                &every_claim,
                &sections,
                &NullSink,
            );
            eprintln!(
                "r6c {:13} own {:2}/{:6} | calls-into {}/{:5} | cross {}/{:5} | sections after {}",
                sl.id,
                sl.sections.len(),
                own.chars().count(),
                c.called_into.matches("\n### ").count(),
                c.called_into.chars().count(),
                c.cross_cutting.matches("\n### ").count(),
                c.cross_cutting.chars().count(),
                own.chars().count()
                    + c.called_into.chars().count()
                    + c.cross_cutting.chars().count()
            );
        }
    }

    /// The three routing rules on a small document, each edge named: a claimed TOP-LEVEL
    /// grouping inherits no children (its children are other slices' components); a section
    /// two slices claim is not cross-cutting; a flat document (only top-level sections)
    /// broadcasts nothing; the route match is token-bounded; the research prompt's one-claimant
    /// view yields the same blocks through the same helper.
    #[test]
    fn consumer_routing_follows_the_documents_own_structure_and_nothing_else() {
        let doc = "# Title\n\n## Build\n\n### X\nX calls `/api/y` and reads `/health` of nobody.\n\n\
                   #### X1\nx1 detail\n\n#### X2\nx2 detail\n\n### Y\n\n| Method | Path | Response |\n\
                   |---|---|---|\n| `GET` | `/api/y` | `{\"y\": 1}` |\n| `GET` | `/api/yz/<id>` | `{\"z\": 1}` |\n\n\
                   ### Z\n\n| Method | Path | Response |\n|---|---|---|\n| `GET` | `/api/health` | `{\"ok\": 1}` |\n\n\
                   ## Rules\nrule text\n\n## Budgets\nbudget text\n\n## Shared\nshared text\n";
        let sections = spec_sections(doc);
        assert_eq!(top_level(&sections), Some(2));
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (x, y, shared, build) = (
            s(&["X"]),
            s(&["Y", "Shared"]),
            s(&["Shared"]),
            s(&["Build"]),
        );
        let every: Vec<&[String]> = vec![&x, &y, &shared, &build];
        let sink = ValueSink::default();
        let cx = consumed_spec_sections("x", &x, &[], &every, &sections, &sink);
        assert!(
            cx.called_into.contains("\n### Y\n"),
            "rule a: X's body names /api/y"
        );
        assert!(
            !cx.called_into.contains("\n### Z\n"),
            "`/health` in X's body is not `/api/health`: {}",
            cx.called_into
        );
        assert!(cx.called_into.contains("\n### X1\n") && cx.called_into.contains("\n### X2\n"));
        assert!(
            cx.cross_cutting.contains("\n### Rules\n")
                && cx.cross_cutting.contains("\n### Budgets\n")
        );
        assert!(
            !cx.cross_cutting.contains("Shared"),
            "claimed by two slices: theirs, not everyone's"
        );
        let cb = consumed_spec_sections("b", &build, &[], &every, &sections, &NullSink);
        assert!(
            cb.called_into.is_empty(),
            "a claimed top-level grouping inherits none of its components: {}",
            cb.called_into
        );
        let cy = consumed_spec_sections("y", &y, &[], &every, &sections, &NullSink);
        assert!(cy.called_into.is_empty(), "{}", cy.called_into);
        assert_eq!(cy.cross_cutting.matches("\n### ").count(), 2);
        let ev = sink.0.lock().unwrap();
        let rules: Vec<&str> = ev
            .iter()
            .filter(|e| e["event"] == "spec_sections_consumed")
            .map(|e| e["rule"].as_str().unwrap())
            .collect();
        assert_eq!(
            rules,
            vec!["advertised_route", "child_of_claimed", "cross_cutting"]
        );
        drop(ev);

        // The research prompt: the same helper from the one-claimant view (armed by a spec
        // over the floor), the same labels.
        let padded = format!("{doc}\n{}", "padding line\n".repeat(1_000));
        let padded_sections = spec_sections(&padded);
        let block = research_request_block(
            &padded,
            &padded_sections,
            true,
            "x",
            &x,
            &[],
            &[x.as_slice()],
            &NullSink,
        );
        assert!(block.contains("SECTIONS THIS SLICE CALLS INTO") && block.contains("\n### X1\n"));
        assert!(block.contains("CROSS-CUTTING SPEC RULES") && block.contains("\n### Rules\n"));

        // A flat document: nothing to broadcast, nothing to inherit.
        let flat = spec_sections("## A\na\n\n## B\nb\n\n## C\nc\n");
        let a = s(&["A"]);
        let cf = consumed_spec_sections("a", &a, &[], &[&a], &flat, &NullSink);
        assert!(cf.called_into.is_empty() && cf.cross_cutting.is_empty());
    }

    /// r6c's three plan-time decisions, verbatim: the questions from `low_confidence_ask` and
    /// the answers from `.swarm/ledger/research-__open_decisions__-q{0,1,2}.json` (2,562 /
    /// 3,066 / 3,243 chars).
    const R6C_D1_Q: &str = "D1 — does the brush survive a streamed mutation of a brushed record (stay brushed vs drop out)? Documented in DECISIONS.md, owned by the web-console slice; affects viz.js dimming and app.js row state.";
    const R6C_D2_Q: &str = "D2 — is a rejected draft terminal, or resubmittable? Affects the drafts state machine (api slice) and workflow UI (web-console slice); note the frozen state machine lists no rejected→submitted edge, which constrains the answer.";
    const R6C_D3_Q: &str = "D3 — before the first sync completes, does the table render empty-with-progress, or block behind a loading state? Implemented in web/index.html + web/app.js (web-console slice).";
    const R6C_D1_A: &str = r#"D1 DECISION: **stay brushed** — a streamed mutation of a brushed record leaves it in the brush set.

Grounding: the request does not settle this (section 8 delegates it verbatim; vendor docs at 127.0.0.1:8850/v3/docs are silent on UI selection). So this is the published decision, chosen as the conventional choice: selections bind to entity identity, not current attribute values. It also matches the spec's own framing — "ONE brush set of record ids" — because a vendor mutation changes only status/note/version (never id, and per "Amount immutability" never amount), so an id-keyed Set survives automatically with zero extra logic.

Handoff to builder:

1. `DECISIONS.md` — add under EXACTLY the heading `## D1` (2–3 sentences, choice + why; it must not contradict observed behavior):
"The brush survives a streamed mutation of a brushed record — it stays brushed. The brush is a set of record ids and a vendor mutation changes only status/note/version, never the id, so selection binds to identity: dropping a payment from the team's active investigation the instant a webhook flips its status would be surprising in an ops console where such mutations are expected traffic. Consequence: a batch mutating a brushed record re-colors that instance to the new status hex at full brightness (it remains a member of the non-empty brush) and leaves the dim flag, `#brush-count`, table `data-brushed` attributes, and `vs7dbg.brush()` untouched."

2. `web/viz.js` behavior contract: on an SSE batch whose record id ∈ brush set — update that instance's top face to the new status's exact hex (`settled #059669`, `pending #D97706`, `refunded #7C3AED`, `failed #B91C1C`), sides `round(0.55·top)` per channel, NO 0.30 dim (members keep full status hex); do NOT touch its per-instance dim flag → no upload beyond the changed-instance bytes (`|S|·stride + 4096`, no realloc, S = {that instance}); `sceneDigest()` is invariant (amounts immutable ⇒ h/x/z unchanged; `brushedCount` unchanged) — only pixels move; `vs7dbg.brush()` still returns the id, ascending by id.

3. `web/app.js` behavior contract: if the mutated record's row is rendered under the current filter/page, keep `data-brushed="true"`; do not re-fetch, reorder, or bump `#brush-count`; emit no notification for it (`payment.updated` is not one of the five outbox-crossing types — do not invent a row).

4. Boundaries: a streamed CREATE appends a new id that cannot be in the brush (never selected) → simply not brushed; background click still clears the whole set; row/instance click toggles are unaffected."#;
    const R6C_D2_A: &str = r#"**D2 DECISION: `rejected` is TERMINAL.** No `rejected → submitted` edge exists; a maker who wants to retry creates a NEW draft via `POST /api/drafts`.

**Grounding (from the request, not convention):** Section 5 declares the machine "frozen" and enumerates its complete edge set — `draft → submitted`, `submitted → approved | rejected`, `approved → sent`. A resubmittable design would require inventing an edge the frozen machine does not contain, plus ambiguous ledger semantics (a resubmit would have to reuse `draft.submitted`, since the event vocabulary is closed: `draft.created | draft.submitted | draft.approved | draft.rejected | payment.sent`). Section 9 allows either answer but fails "a document that contradicts observed behavior" — terminal is the only choice consistent with the frozen contract. It also matches conventional one-shot maker/checker flows (rejection closes the ticket; re-create to retry). Vendor docs at 127.0.0.1:8850/v3/docs are silent on drafts (verified) — drafts are our own construct.

**Handoff to builder:**

1. `DECISIONS.md` — under EXACTLY the heading `## D2`, 2–3 sentences, e.g.: "A rejected draft is terminal: there is no path from `rejected` back to `submitted`. The frozen state machine lists no such edge, and a maker who wants to retry creates a new draft via `POST /api/drafts` (fresh `draft.created`). This keeps the ledger event vocabulary closed and matches one-shot maker/checker practice where rejection closes the ticket."

2. **ledgerd API slice** — legal actions per state: `draft`: submit; `submitted`: approve, reject; `approved`: none (send is automatic inside approve); `rejected`: NONE; `sent`: none. Any `POST /api/drafts/<id>/submit|approve|reject` against a draft in state `rejected` (or `sent`) → HTTP 409 with the single error envelope, code `"conflict"`, state untouched, NO ledger event, no outbox row. NOTE: the request does not pin an envelope code for illegal transitions — `conflict`/409 is a **convention** (state-machine violation; `conflict` is already in the frozen vocabulary and used for the note-write 412 case). Do NOT return 403/`approval_forbidden` here — that code is reserved for four-eyes violations on a legal transition. `GET /api/drafts?state=` accepts exactly the reachable states `draft|submitted|approved|rejected|sent` (grounded: `sent` is a frozen edge); any other value → 400 validation error per section 3's rule.

3. **web-console slice (`web/app.js`)** — button enablement on the selected row of `#draft-list` (rows carry `data-draft-id`, `data-state`): `#submit-btn` enabled iff `data-state="draft"`; `#approve-btn` and `#reject-btn` enabled iff `data-state="submitted"`; all three DISABLED for `rejected` and `sent`. Ship no resubmit control anywhere. 401/403/`approval_forbidden` from the API still surface non-blocking in `#notice` as specified.

Consequence check: nothing in the graded schedule (section "What WILL happen") resubmits a rejected draft, and the UI journey (maker create→submit, checker approve/reject) is fully satisfiable with terminal semantics."#;
    const R6C_D3_A: &str = r#"D3 DECISION: empty-with-progress — the table renders immediately with a visible progress indicator; it does NOT block behind a loading state.

Grounding (request text):
1. Section 7 defines the empty state as "no payments yet — with a call to sync" — i.e., the empty state is a real, usable state tied to the existing `#sync-now` button, not a placeholder overlay.
2. Section 7: "Never a blank panel, never a spinner that never resolves." The graded run holds the vendor DOWN for the first 3–8 s (retries ≥ every 5 s), so a full-page block waiting on the first sync would be an unresolvable-looking spinner — the exact forbidden failure.
3. Self-driven rule: "a run that needs a human to click, restart or nudge anything has already failed." A blocking gate only self-unblocks when a sync succeeds; empty-with-progress keeps the whole page live (summary, viz `#viz-empty`, filters) and needs no operator.
4. Budget: "First data rows render within 2 seconds of page load (local data present)." On restart against an existing `--db-dir` (the common graded case), local rows must paint instantly; a gate that waits for a fresh sync before painting needlessly delays this.
5. Symmetry: section 8 gives the viz panel its own non-blocking empty state (`#viz-empty`) instead of a block — the table should mirror it.

Implementation handoff (web/index.html + web/app.js):
- index.html: inside the table region add two convention-named elements (spec is silent on exact markup; these names are conventions): `<div id="table-progress" role="status" class="table-progress">Syncing…</div>` and `<div id="table-empty" class="table-empty" hidden>`, whose copy references the existing `#sync-now` button ("No payments yet — press Sync now").
- app.js: drive a `data-state` on the table wrapper (convention, mirroring viz vocabulary): `"loading"` only while NO local data exists AND the first read returned nothing; `"empty"` when a successful read returns `total === 0` (settled state, no spinner — points at `#sync-now`); `"ready"` once rows exist. On load do one server-driven fetch (`GET /api/payments?limit=50&offset=0…`, never the full collection); if local data is present, go straight to `ready`. While in `loading`, poll the same paginated endpoint at a light cadence (convention: ~2 s) until `total > 0`, then flip to `ready` — the indicator always self-resolves; on sync failure it degrades to the `#notice` path and the button returns to idle, never an eternal spinner. Rows remain server-paginated in all states.
- DECISIONS.md `## D3` (2–3 sentences): "The table renders empty-with-progress rather than blocking behind a loading state. On load it immediately paints any local rows (so restarts meet the 2 s first-row budget) and, only when no rows exist yet, shows a visible 'Syncing…' progress row plus the existing Sync-now call; it self-resolves to rows as soon as the first sync lands, so there is never a spinner that cannot resolve while the vendor is down (the graded run holds it down 3–8 s) and nothing requires an operator."

Exact attribute names (`table-progress`, `table-empty`, `data-state` values) and the ~2 s poll cadence are conventions — the spec mandates only that the three states exist, be visibly distinct, and never hang."#;

    fn r6c_decisions() -> Vec<PlanDecision> {
        [
            (R6C_D1_Q, R6C_D1_A),
            (R6C_D2_Q, R6C_D2_A),
            (R6C_D3_Q, R6C_D3_A),
        ]
        .iter()
        .enumerate()
        .map(|(i, (q, a))| PlanDecision {
            q_index: i,
            question: q.to_string(),
            state: DecisionState::SettledByResearch {
                answer: a.to_string(),
            },
        })
        .collect()
    }

    /// VA-012 on r6c's real decisions and claims: the same 5,582-char block rode all five
    /// briefs (27,910 chars, 22,328 duplicate), each answer cut at 1,500 chars, and the
    /// web-console copy of D1 ended before the one paragraph addressed to it ("3. `web/app.js`
    /// behavior contract", char 2,057). Now ledgerd-core and notifierd — named by no decision —
    /// carry none; web-viz carries D1 only, with its `web/viz.js` paragraph whole; ledgerd-api
    /// carries D2 (its §5 rows advertise `/api/drafts`) with the API paragraph; web-console
    /// carries all three with the app.js paragraph that was behind the cut; nothing is
    /// truncated; a decision naming no slice is broadcast and said so.
    #[test]
    fn decisions_render_per_slice_from_r6c_s_three_settled_decisions() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let mut opened = r6c_slices();
        // r6c's objectives declared no backticked files; declare web-console's and web-viz's
        // the way the opener prompt asks (NAME EACH SLICE'S OWNED FILES IN ITS OBJECTIVE), so
        // the file half of the vocabulary is exercised beside the id/route/decision-id halves.
        opened.slices[3].objective =
            "Own `web/index.html`, `web/styles.css`, `web/app.js` (page behavior).".into();
        opened.slices[4].objective = "Own `web/viz.js` (the 3D engine and nothing else).".into();
        let decisions = r6c_decisions();
        let sink = ValueSink::default();
        let briefs = briefs_from_slices(&opened, spec, &[], &decisions, &sink);
        let by_id = |id: &str| &briefs.iter().find(|b| b.id == id).unwrap().brief;
        let block = |b: &str| -> String {
            match b.find("DECISIONS SETTLED AT PLAN TIME") {
                Some(at) => b.get(at..).unwrap().to_string(),
                None => String::new(),
            }
        };
        let before = decisions::decisions_brief_block(&decisions);
        assert!(
            before.matches("ANSWER TRUNCATED").count() == 3 && before.chars().count() > 5_000,
            "the pre-VA-012 block, for the table: {} chars",
            before.chars().count()
        );
        for b in &briefs {
            assert!(
                !b.brief.contains("ANSWER TRUNCATED"),
                "{}: a decision renders whole paragraphs, never a head cut",
                b.id
            );
            eprintln!(
                "r6c {:13} decisions before {:5} | after {:5} | D1 {} D2 {} D3 {}",
                b.id,
                before.chars().count(),
                block(&b.brief).chars().count(),
                b.brief.contains("D1 DECISION") as u8,
                b.brief.contains("D2 DECISION") as u8,
                b.brief.contains("D3 DECISION") as u8,
            );
        }
        assert!(
            block(by_id("ledgerd-core")).is_empty(),
            "no decision names the core"
        );
        assert!(
            block(by_id("notifierd")).is_empty(),
            "no decision names notifierd"
        );
        let viz = by_id("web-viz");
        assert!(viz.contains("D1 DECISION: **stay brushed**"), "{viz}");
        assert!(!viz.contains("D2 DECISION") && !viz.contains("D3 DECISION"));
        assert!(
            viz.contains("2. `web/viz.js` behavior contract"),
            "the paragraph addressed to viz rides whole"
        );
        assert!(
            !viz.contains("3. `web/app.js` behavior contract"),
            "the paragraph addressed to app.js is web-console's"
        );
        let api = by_id("ledgerd-api");
        assert!(api.contains("D2 DECISION: `rejected` is TERMINAL"), "{api}");
        assert!(
            api.contains("2. **ledgerd API slice** — legal actions per state"),
            "{api}"
        );
        assert!(
            api.contains("Section 5 declares the machine \"frozen\""),
            "the grounding paragraph cites the request and rides"
        );
        let console = by_id("web-console");
        for (verdict, paragraph) in [
            (
                "D1 DECISION: **stay brushed**",
                "3. `web/app.js` behavior contract",
            ),
            (
                "D2 DECISION: `rejected` is TERMINAL",
                "1. `DECISIONS.md` — under EXACTLY the heading `## D2`",
            ),
            (
                "D3 DECISION: empty-with-progress",
                "Implementation handoff (web/index.html + web/app.js)",
            ),
        ] {
            assert!(console.contains(verdict), "{console}");
            assert!(
                console.contains(paragraph),
                "web-console: {paragraph} rides whole (r6c cut D1 at char 1,443)"
            );
        }
        assert!(
            console.contains("FULL ANSWER (every slice's handoff): .swarm/ledger/research-__open_decisions__-q0.json")
        );
        let ev = sink.0.lock().unwrap();
        assert!(
            !ev.iter().any(|e| e["event"] == "decision_broadcast"),
            "every r6c decision names at least one slice"
        );
        drop(ev);

        // A decision naming no slice is every slice's, and the absence of a consumer is loud.
        let mut with_stray = decisions.clone();
        with_stray.push(PlanDecision {
            q_index: 3,
            question: "Logging format — options: plain | json".into(),
            state: DecisionState::SettledByResearch {
                answer: "DECISION: plain lines.\n\nGrounding: the request is silent.".into(),
            },
        });
        let sink = ValueSink::default();
        let briefs = briefs_from_slices(&opened, spec, &[], &with_stray, &sink);
        for b in &briefs {
            assert!(
                b.brief.contains("DECISION: plain lines."),
                "{}: a decision naming no slice reaches every brief",
                b.id
            );
        }
        let ev = sink.0.lock().unwrap();
        let bc: Vec<&serde_json::Value> = ev
            .iter()
            .filter(|e| e["event"] == "decision_broadcast")
            .collect();
        assert_eq!(bc.len(), 1);
        assert_eq!(bc[0]["decision"], 3);
        drop(ev);

        // A user-settled decision is quoted whole; an open one keeps the conventional framing;
        // a slice whose question was ROUTED to a decision carries that decision even when its
        // words name nothing of the slice.
        let mut routed_opened = r6c_slices();
        routed_opened.slices[2].objective =
            "Own `app/notifierd.py` and `app/notify_store.py`.".into();
        let mut q = OpenQuestion::from("How should the notifier log?");
        q.decision = Some(0);
        routed_opened.slices[2].questions.push(q);
        let mixed = vec![
            PlanDecision {
                q_index: 0,
                question: "Logging format — options: plain | json".into(),
                state: DecisionState::SettledByUser {
                    answer: "json".into(),
                },
            },
            PlanDecision {
                q_index: 1,
                question: "Retry cadence for app/notifierd.py — options: 1s | 5s".into(),
                state: DecisionState::Open,
            },
        ];
        let briefs = briefs_from_slices(&routed_opened, spec, &[], &mixed, &NullSink);
        let notifier = &briefs[2].brief;
        assert!(notifier.contains("THE USER CHOSE: json"), "{notifier}");
        assert!(
            notifier.contains("OPEN DECISIONS") && notifier.contains("Retry cadence"),
            "{notifier}"
        );
        assert!(
            !briefs[4].brief.contains("Retry cadence"),
            "the open decision names notifierd's file, not viz"
        );
        assert!(
            !briefs[4].brief.contains("THE USER CHOSE: json"),
            "notifierd's routed question names the user's decision, so it is notifierd's alone"
        );
    }
}
