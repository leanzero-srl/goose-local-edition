//! The research fan's TERMINAL-ROW cluster: the question identity, the row every dispatched
//! question folds into, and the pure helpers that classify, persist and splice its outcome.
//!
//! Second sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink. Moved
//! verbatim from swarm.rs — behavior unchanged; the WHY of every part stays in each item's own
//! doc. The fan itself (`research_fan`, on `GooseAgentDispatcher`) stays in the root with the
//! other dispatcher methods; what lives here is everything about it that is pure.
//!
//! THE LANES RESEARCH (VA-089): a research lane runs for EVERY slice and carries NO questions —
//! it is dealt its slice's sections one per turn (VA-128), the sources and the sibling slices'
//! objectives, DERIVES its own design/external questions and answers them in the same session
//! (`ResearchLane`, `fold_research_lane`). Spec lookups are not questions any more: the brief
//! and the lane both hold the section text. MEASURED: r6h's opener reasoned ~66 minutes on one
//! node over dozens of "What do request.md:A-B fix for …" lookups while two nodes idled, and
//! r6g's fan ran lanes for only the 4 of 6 slices that had dispatch-kind questions. The one
//! lane that still carries known, tagged questions is the DECISIONS lane (`DECISION_SLICE`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::decisions::{self, DecisionState, PlanDecision, DECISION_SLICE};
use super::findings::FINDING_PATH_EXTS;
use super::opener::{OpenOutput, OpenSlice};
use super::orientation::{children_of, heading_key, top_level};
use super::spec_surface::{
    mount_prefixes, path_token_named, resource_word_named, resource_words, spec_surface_rows,
};
use super::{activity_digest_key, head_to_sentence_end, one_lane_per_host, parse_json_lenient};
use super::{orientation_armed, spec_sections, SliceBrief};
use super::{phase_banner, spec_orientation, spec_vendor, write_forming_atomic};
use super::{EventSink, SpecSection};
use super::{JUDGE_ENDED_NEEDLE, LEDGER_DIR, USER_DECISIONS_HEADER};

/// What kind of question a research lane says it derived (VA-089): `design` — the request leaves
/// it open and the lane DECIDES, naming the alternatives it chose between and why the request
/// does not settle it (VA-118); `external` — the vendor's documentation or another source
/// outside the request answers it (the lane cites the doc section). There is NO lookup kind: a
/// fact the request states is not a question — the lane holds the section text and reads it.
/// `SpecRestated` is not a kind a lane may choose either: it is the CLASSIFIER's reading
/// (`classify_design_entry`) of a `design` entry that named fewer than two alternatives — by the
/// contract's own definition a decision the request left open has two admissible answers, so an
/// entry that can show only one is the request's fact restated (r6i: 35 of 35 `design` tags,
/// 2 of the 6 a reader checked were request.md lines rewritten as code). `Unkinded` is the
/// parse-time reading of a kind the contract does not name, kept (the answer is still an
/// answer) and visible on `research_question_kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuestionKind {
    Design,
    External,
    SpecRestated,
    Unkinded,
}

impl QuestionKind {
    /// Lenient on decoration (case, `-`/` ` for `_`), strict on vocabulary: only the two names
    /// the schema enumerates resolve; anything else is `Unkinded`, never a guess at what the
    /// model meant. `spec_restated` is deliberately NOT parsed: a lane that knows an entry
    /// restates the request writes no entry for it.
    fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().replace(['-', ' '], "_").as_str() {
            "design" => Self::Design,
            "external" => Self::External,
            _ => Self::Unkinded,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::External => "external",
            Self::SpecRestated => "spec_restated",
            Self::Unkinded => "unkinded",
        }
    }

    /// Who decided this kind — `research_question_kind.source`, so the vigil sees whether the
    /// lane's self-tag or the code's reading of it is speaking. A function of the kind, not a
    /// stored field: only the classifier produces `spec_restated`, and it produces nothing else.
    pub(crate) fn source(self) -> &'static str {
        match self {
            Self::SpecRestated => "classifier",
            _ => "model",
        }
    }

    /// The parse-time reading of a stored kind string (`ResearchRow::kind`, `question_kind` on
    /// disk): the classifier's name resolves too, so a resumed row reports the same source.
    pub(crate) fn from_stored(raw: &str) -> Self {
        if raw.trim() == "spec_restated" {
            Self::SpecRestated
        } else {
            Self::parse(raw)
        }
    }
}

/// One question addressed by (slice, q_index) — the identity the mini filename and the brief
/// partition share. A DECISION question (`decision`) is known before its lane runs — the
/// decisions lane answers the open decisions the user left, tagged `[qN]`; a slice lane's
/// questions are the lane's OWN (VA-089) and exist only as its answers (`fold_research_lane`).
#[derive(Clone, Debug)]
pub(super) struct ResearchQuestion {
    pub(crate) slice: String,
    pub(crate) q_index: usize,
    pub(crate) question: String,
    pub(crate) kind: QuestionKind,
    pub(crate) cite: String,
}

impl ResearchQuestion {
    /// A decision the user left open, riding the fan under `DECISION_SLICE` (decisions.rs). Its
    /// kind is `design` by construction — a decision is a choice the request leaves open.
    pub(super) fn decision(q_index: usize, line: &str) -> Self {
        Self {
            slice: DECISION_SLICE.to_string(),
            q_index,
            question: line.to_string(),
            kind: QuestionKind::Design,
            cite: String::new(),
        }
    }
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
    /// The lane's kind for its question (`QuestionKind::as_str`: design | external | unkinded);
    /// "" on a lane-outcome row (`question` empty) and on a mini written before this field
    /// reached disk. On disk it is `question_kind`: the mini's `kind` is the ledger rollup's
    /// DISCRIMINATOR (`Some("research")` beside `task`/`gate`/`repair` in swarm.rs's rollup
    /// match), and `write_research_ledger` used to write that literal INTO this field — r6h's 8
    /// `research_question_kind{external}` tags reached no mini and every resumed row read
    /// `kind: research` (fallback-hunter, 2026-09-02).
    #[serde(default, rename = "question_kind")]
    pub(crate) kind: String,
    /// The evidence the lane cited — the request line and the grep that found no match for a
    /// design question, the vendor doc section for an external one; "" when it named none.
    #[serde(default)]
    pub(crate) cite: String,
    /// C3: how many questions the lane that produced this row answered in the SAME session —
    /// `secs` is that session's whole wall time, shared by every row of the batch, never a
    /// per-question split (a split would be a fabricated number). 0 on a pre-cut mini and on
    /// a panicked lane's row (no session ran to completion).
    #[serde(default)]
    pub(crate) batch: usize,
}

/// The DECISIONS lane's structured deliverable (A1, batched by C3): `{answers: [{question_index,
/// answer, raised}]}` — ONE lane answers every open decision the user left in one session and
/// the ledger still gets one mini per decision (`fold_research_batch` keys each entry by its
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

/// A SLICE lane's structured deliverable (VA-089, widened by VA-118): the lane's OWN questions
/// and answers — `{answers: [{question, kind, cite, alternatives, open_because, answer, raised,
/// raised_for}], builder_decides}`, `kind` one of the two the contract names. The position of an
/// entry IS its q_index (`fold_research_lane`): the prompt carried no questions, so no tag table
/// exists between prompt and ledger. `alternatives` (two or more) and `open_because` are what
/// make a `design` entry a decision instead of a restatement (`classify_design_entry`);
/// `raised_for` gives a point that belongs to ANOTHER slice its destination (r6i's structure
/// lane spent reasoning at 60k and 100k chars on whether such points were "accidentally
/// claimed" — with nowhere to put them); `builder_decides` is the lane-level list of choices
/// only this slice's builder feels — named, unanswered, cheap. `cite`, `alternatives`,
/// `open_because`, `raised`, `raised_for` and `builder_decides` legitimately default to empty;
/// an empty `answer` is classified honestly as unanswered/empty_answer rather than rejected at
/// validation, and an unknown `kind` is kept and named (`unkinded`), never refused — a refusal
/// re-streams the whole session.
pub(super) fn research_derived_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answers"],
        "properties": {
            "answers": {
                "type": "array",
                "items": research_answer_entry_schema()
            },
            "builder_decides": {"type": "array", "items": {"type": "string"}}
        }
    })
}

/// ONE derived entry's schema — the item of `research_derived_schema`'s `answers` and the whole
/// argument of the per-answer `research_answer` tool (`research_answer_tool_schema`), so the two
/// landing paths cannot drift.
pub(super) fn research_answer_entry_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["question", "kind", "answer"],
        "properties": {
            "question": {"type": "string"},
            "kind": {"type": "string", "enum": ["design", "external"]},
            "cite": {"type": "string"},
            "alternatives": {"type": "array", "items": {"type": "string"}},
            "open_because": {"type": "string"},
            "answer": {"type": "string"},
            "raised": {"type": "array", "items": {"type": "string"}},
            "raised_for": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["slice", "text"],
                    "properties": {
                        "slice": {"type": "string"},
                        "text": {"type": "string"}
                    }
                }
            }
        }
    })
}

/// The per-answer landing tool (VA-118 item 4, wired r6j in `research_tool.rs`): one settled
/// question lands as one mini the moment the lane calls it, so the lane's frame never sits at
/// 0 bytes for an hour (r6i's structure lane: 113,720 reasoning chars, output frame empty until
/// minute 63, nine answers in one final_output). The tool's argument is exactly one entry
/// (`research_answer_entry_schema`) and `ResearchToolCall::into_row` turns it into the same row
/// `fold_research_lane_from` would have built at that position; the lane's final_output then
/// folds only the REMAINDER (`fold_research_lane_from(.., next_q_index)`), so the numbering
/// never collides with a landed mini. Registered as a frontend extension on the research call
/// (`GooseAgentDispatcher::research_answer_extension_for`) and answered in the lane's own stream
/// loop (`frontend_tool_result`) — the agent parks the lane on the result channel until the
/// swarm replies. VA-128: the argument is the entry widened by the SECTION signal —
/// `section_done` closes the section in hand (the next one's text rides the result),
/// `builder_decides` names the choices only this slice's builder makes — and nothing is
/// `required` at the tool level, so a bare `{"section_done": true}` is a valid call under
/// schema-constrained decoding; an entry without a question is still the stray it always was
/// (`ResearchToolCall::into_row`).
pub(super) fn research_answer_tool_schema() -> serde_json::Value {
    let mut schema = research_answer_entry_schema();
    schema["required"] = serde_json::json!([]);
    schema["properties"]["section_done"] = serde_json::json!({"type": "boolean"});
    schema["properties"]["builder_decides"] =
        serde_json::json!({"type": "array", "items": {"type": "string"}});
    schema
}

pub(super) const RESEARCH_ANSWER_TOOL: &str = "research_answer";

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

/// One research lane (VA-089). A SLICE lane derives its own questions: `head` is the slice's
/// prompt head (its sections verbatim, objective, user decisions, tree, sources), `siblings` the
/// other slices' objectives (so a question another slice owns is not asked here), `questions`
/// EMPTY. The DECISIONS lane carries the open decisions the user left, tagged `[qN]`, under
/// `DECISION_SLICE`. `material` is the text the cross-slice snowball and relay match route paths
/// against (`path_tokens`): the slice's objective and claimed sections' bodies, or the
/// decisions' lines — a lane has no questions to match on before it runs.
#[derive(Clone, Debug)]
pub(super) struct ResearchLane {
    pub(super) slice: String,
    pub(super) head: String,
    pub(super) siblings: String,
    pub(super) questions: Vec<ResearchQuestion>,
    pub(super) material: String,
    /// VA-128: the slice's claimed sections in claim order, handed to the lane ONE AT A TIME
    /// (`section_in_hand_block`) — the first rides the dispatch text, each next one rides the
    /// result of the `research_answer` call that closes the current one (`section_done`).
    /// Empty for the decisions lane and for a slice whose claims matched no heading.
    pub(super) hand: Vec<HandedSection>,
}

impl ResearchLane {
    /// A slice lane derives its questions; the decisions lane is handed them.
    pub(super) fn derives(&self) -> bool {
        self.questions.is_empty()
    }

    /// What both snowball channels match a stranger's row against (`stranger_admission`):
    /// built here for the late relay's enrolment and again by `prior_minis_for` at dispatch,
    /// so the two channels read ONE derivation of the lane.
    pub(super) fn relay_target(&self) -> RelayTarget {
        RelayTarget {
            slice: self.slice.clone(),
            paths: path_tokens(&self.material),
            files: declared_files(&self.material),
            landing: self.derives(),
        }
    }
}

/// What the late relay (E7) knows about a running lane: its slice (a lane never receives its
/// own slice's row), the route paths its material names, and the files its material declares
/// in backticks (VA-131) — lowercased, as `names_a_file` matches them.
#[derive(Clone, Debug)]
pub(super) struct RelayTarget {
    pub(super) slice: String,
    pub(super) paths: BTreeSet<String>,
    pub(super) files: BTreeSet<String>,
    /// VA-033: whether the fan opens a `ResearchLanding` for this lane (a slice lane derives
    /// and lands through the tool; the decisions lane never does). The relay reads it to tell
    /// a lane whose landing is GONE (closed — skip, loud) from one that never has one (open on
    /// `research_running` alone, as before): the landing map by itself cannot say which.
    pub(super) landing: bool,
}

/// VA-089: the text the cross-slice path rule reads for a slice lane — its objective and the
/// bodies of the sections it claimed (matched on `heading_key`, exactly as the splice matches
/// them). A lane carries no questions, so this is what a landed stranger's `/api/…` is matched
/// against, at dispatch (`prior_minis_for`) and while it runs (`relay_targets`).
pub(super) fn slice_material(sl: &OpenSlice, sections: &[SpecSection]) -> String {
    let mut material = sl.objective.clone();
    for want in &sl.sections {
        let key = heading_key(want);
        if let Some(sec) = sections.iter().find(|s| heading_key(&s.heading) == key) {
            material.push('\n');
            material.push_str(&sec.heading);
            material.push('\n');
            material.push_str(&sec.body);
        }
    }
    material
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

/// An entry the fold cannot attribute — a `question_index` that is none of the decisions lane's
/// tags, or a slice lane's entry with no question text (`question_index` is its position):
/// never silently dropped — the fan names it (`research_batch_stray_answer`) with the answer's head.
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
        batch,
    }
}

/// Fold the DECISIONS lane's outcome — its whole tagged batch — into one TERMINAL row PER QUESTION. Pure,
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

/// One slice lane's entry (VA-089): the lane's own question, kind, evidence and answer; since
/// VA-118 also the alternatives a design entry chose between, why the request leaves it open,
/// and the points raised FOR other slices.
#[derive(serde::Deserialize, Default)]
struct DerivedAnswer {
    #[serde(default)]
    question: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    cite: String,
    #[serde(default)]
    alternatives: Vec<String>,
    #[serde(default)]
    open_because: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    raised: Vec<String>,
    #[serde(default)]
    raised_for: Vec<RaisedFor>,
}

/// A point the lane raised that belongs to ANOTHER slice: the destination it had no way to name
/// before VA-118 (r6i's structure lane, @60k: "are there any questions that are actually OTHER
/// slices' territory that I'm accidentally claiming?").
#[derive(serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
struct RaisedFor {
    #[serde(default)]
    slice: String,
    #[serde(default)]
    text: String,
}

/// How a raised line rides in `ResearchRow::raised`, whose shape this commit cannot widen
/// (swarm.rs:33260 and :33312 build the row as struct literals and swarm.rs is outside this
/// change's boundary): a point for another slice is `[for <slice>] text`, a choice only this
/// slice's builder makes is `[builder decides] text`, anything else is a raised question for
/// this slice's builder as before. ONE writer (`row_from_entry` / `fold_research_lane`), ONE
/// reader (`raised_destination`), consumed by `emit_research_outcome` (three distinct events) —
/// the brief block renders each line with its label so the builder sees whose point it is.
/// The honest shape is two fields on the row; that is the swarm.rs surgeon's one-line follow-up.
pub(super) const RAISED_FOR_PREFIX: &str = "[for ";
pub(super) const BUILDER_DECIDES_PREFIX: &str = "[builder decides] ";

/// Where a raised line goes, read back from its label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RaisedDestination<'a> {
    ThisBuilder(&'a str),
    OtherSlice { slice: &'a str, text: &'a str },
    BuilderDecides(&'a str),
}

pub(super) fn raised_destination(line: &str) -> RaisedDestination<'_> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(BUILDER_DECIDES_PREFIX) {
        return RaisedDestination::BuilderDecides(rest.trim());
    }
    if let Some(rest) = line.strip_prefix(RAISED_FOR_PREFIX) {
        if let Some((slice, text)) = rest.split_once("] ") {
            let slice = slice.trim();
            if !slice.is_empty() {
                return RaisedDestination::OtherSlice {
                    slice,
                    text: text.trim(),
                };
            }
        }
    }
    RaisedDestination::ThisBuilder(line)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// THE CLASSIFIER (VA-118 item 2). A `design` entry is a decision the request leaves open — by
/// definition one with at least two admissible answers. An entry tagged `design` that names
/// fewer than two distinct alternatives is recorded `spec_restated`: it shows no choice, so
/// either the request settled it or the lane asserted a pick without showing the alternatives;
/// both are the builder's to read from the sections, not research. `external` and unknown kinds
/// pass through untouched. Returns the kind and the evidence line the row carries as `cite`
/// (the lane's cite, then `open because: …`, then `alternatives: a | b`) so the brief's
/// EVIDENCE line and the mini hold the words the classifier read.
///
/// WHY a structural rule and not a token-overlap threshold (the brief asked for one, derived
/// from r6i's six read answers — the derivation was run, and the threshold does not exist):
/// share of the answer's content words (`content_words`) found in the slice's handed sections,
/// r6i archive — the two a reader marked SPEC_RESTATED: behavior-q1 0.53, behavior-q11 0.61;
/// the two DESIGN-INTRA: viz-q1 0.55, behavior-q6 0.51; the two DESIGN-REAL: viz-q4 0.72,
/// viz-q7 0.62. Sentence-level (share of sentences ≥ 0.85 in-section): 0.25 / 0.10 vs
/// 0.05 / 0.00 vs 0.17 / 0.00. In-order 4-gram share: 0.03 / 0.03 vs 0.05 / 0.03 vs
/// 0.10 / 0.03. No cut separates the restated pair from the design pairs on any of the three;
/// the reader's verdict rested on WHICH claims restated the request, which no lexical share
/// sees (gate 7: the words decide, shapes corroborate). A threshold fitted to six points would
/// be an instrument impersonating a reader. What code can honestly read is whether the entry
/// SHOWS a choice — and the prompt now makes showing it the contract.
pub(super) fn classify_design_entry(
    model_kind: &str,
    cite: &str,
    alternatives: &[String],
    open_because: &str,
) -> (QuestionKind, String) {
    let parsed = QuestionKind::parse(model_kind);
    let mut alts: Vec<String> = Vec::new();
    for a in alternatives {
        let a = one_line(a);
        if !a.is_empty() && !alts.contains(&a) {
            alts.push(a);
        }
    }
    let open_because = one_line(open_because);
    let mut evidence = one_line(cite);
    if parsed == QuestionKind::Design && !open_because.is_empty() {
        if !evidence.is_empty() {
            evidence.push_str("; ");
        }
        evidence.push_str("open because: ");
        evidence.push_str(&open_because);
    }
    if parsed == QuestionKind::Design && !alts.is_empty() {
        if !evidence.is_empty() {
            evidence.push_str("; ");
        }
        evidence.push_str("alternatives: ");
        evidence.push_str(&alts.join(" | "));
    }
    let kind = if parsed == QuestionKind::Design && alts.len() < 2 {
        QuestionKind::SpecRestated
    } else {
        parsed
    };
    (kind, evidence)
}

/// ONE derived entry → ONE row at `q_index` — the shared body of `fold_research_lane` (every
/// entry of the final reply) and `ResearchToolCall::into_row` (the per-answer tool), so a question lands
/// identically whichever door it came through. `None` when the entry has no question text: a
/// `StrayAnswer` for the caller to name, never a row. A non-empty answer is answered, a blank one
/// unanswered/empty_answer with its raised lines kept; `raised_for` lines are labelled for their
/// slice (`RAISED_FOR_PREFIX`) and ride behind the plain raised lines.
fn row_from_entry(
    slice: &str,
    q_index: usize,
    entry: DerivedAnswer,
    model: &str,
    secs: u64,
) -> Result<ResearchRow, StrayAnswer> {
    let question = one_line(&entry.question);
    if question.is_empty() {
        return Err(StrayAnswer {
            question_index: Some(q_index),
            answer_head: entry.answer.chars().take(200).collect(),
        });
    }
    let answered = !entry.answer.trim().is_empty();
    let (kind, cite) = classify_design_entry(
        &entry.kind,
        &entry.cite,
        &entry.alternatives,
        &entry.open_because,
    );
    let mut raised: Vec<String> = entry
        .raised
        .iter()
        .map(|r| one_line(r))
        .filter(|r| !r.is_empty())
        .collect();
    for rf in &entry.raised_for {
        let text = one_line(&rf.text);
        let target = one_line(&rf.slice);
        if text.is_empty() {
            continue;
        }
        if target.is_empty() {
            // A point with no destination is a point for this slice's builder — stated as it
            // came, never dropped.
            raised.push(text);
        } else {
            raised.push(format!("{RAISED_FOR_PREFIX}{target}] {text}"));
        }
    }
    Ok(ResearchRow {
        slice: slice.to_string(),
        q_index,
        question,
        status: if answered {
            RESEARCH_ANSWERED.to_string()
        } else {
            RESEARCH_UNANSWERED.to_string()
        },
        answer: if answered {
            entry.answer
        } else {
            String::new()
        },
        // Parsed, but the deliverable slot is blank — a named absence, never a stub.
        reason: (!answered).then(|| "empty_answer".to_string()),
        detail: None,
        raised,
        model: model.to_string(),
        secs,
        kind: kind.as_str().to_string(),
        cite,
        batch: 0,
    })
}

/// The per-answer tool's ARGUMENT (VA-128): one entry (`DerivedAnswer`, the same fields the final
/// reply's `answers` items carry) and/or the section signal — `section_done` closes the section
/// in hand, `builder_decides` names the choices only this slice's builder makes. All three may
/// ride one call: the entry lands, the choices ride its raised lines, the section closes.
#[derive(serde::Deserialize, Default)]
pub(super) struct ResearchToolCall {
    #[serde(flatten)]
    entry: DerivedAnswer,
    #[serde(default)]
    section_done: bool,
    #[serde(default)]
    builder_decides: Vec<String>,
}

impl ResearchToolCall {
    pub(super) fn parse(arguments: &str) -> Option<Self> {
        parse_json_lenient::<ResearchToolCall>(arguments)
    }

    pub(super) fn section_done(&self) -> bool {
        self.section_done
    }

    fn carries_question(&self) -> bool {
        !one_line(&self.entry.question).is_empty()
    }

    /// The choices, one line each, blanks and repeats dropped — the final reply's own rule
    /// (`fold_research_lane_from`).
    fn decides(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for d in &self.builder_decides {
            let d = one_line(d);
            if !d.is_empty() && !out.contains(&d) {
                out.push(d);
            }
        }
        out
    }

    /// The row this call lands at `q_index`, if any: the entry, its `builder_decides` labelled
    /// (`BUILDER_DECIDES_PREFIX`) behind its raised lines; with NO question, the choices alone
    /// ride one `builder_decides` outcome row (the final reply's `remainder_empty` convention —
    /// the row cannot grow, and a choice held back for a later row would be lost at the
    /// landing's close, which persists nothing); a call carrying neither lands nothing
    /// (`Ok(None)`: a bare section_done). An answer with no question is the stray it always was.
    pub(super) fn into_row(
        self,
        slice: &str,
        q_index: usize,
        model: &str,
        secs: u64,
        section: Option<&HandedSection>,
    ) -> Result<Option<ResearchRow>, StrayAnswer> {
        let decides = self.decides();
        let labelled = || -> Vec<String> {
            decides
                .iter()
                .map(|d| format!("{BUILDER_DECIDES_PREFIX}{d}"))
                .collect()
        };
        if self.carries_question() {
            let mut row = row_from_entry(slice, q_index, self.entry, model, secs)?;
            row.raised.extend(labelled());
            return Ok(Some(row));
        }
        if !self.entry.answer.trim().is_empty() {
            return Err(StrayAnswer {
                question_index: Some(q_index),
                answer_head: self.entry.answer.chars().take(200).collect(),
            });
        }
        if decides.is_empty() {
            return Ok(None);
        }
        let detail = match section {
            Some(sec) => format!(
                "section `{}` ({}): no question; {} choice(s) only this slice's builder makes",
                sec.heading,
                sec.span(),
                decides.len()
            ),
            None => format!(
                "no section in hand; {} choice(s) only this slice's builder makes",
                decides.len()
            ),
        };
        let mut outcome = lane_outcome_row(slice, "builder_decides", &detail, model, secs);
        outcome.q_index = q_index;
        outcome.raised = labelled();
        Ok(Some(outcome))
    }
}

/// The one row a slice lane leaves when it produced NO question rows — the lane's OUTCOME as a
/// fact the ledger holds (`question` empty, `reason` says which: no_questions — the lane read its
/// sections and derived nothing; empty_answer — nothing parseable; provider_error / judge_ended /
/// lane_panicked). It is the slice's resume watermark exactly as an unanswered row is: the lane
/// never re-runs, and the brief states the outcome (never a fabricated answer).
pub(super) fn lane_outcome_row(
    slice: &str,
    reason: &str,
    detail: &str,
    model: &str,
    secs: u64,
) -> ResearchRow {
    ResearchRow {
        slice: slice.to_string(),
        q_index: 0,
        question: String::new(),
        status: RESEARCH_UNANSWERED.to_string(),
        answer: String::new(),
        reason: Some(reason.to_string()),
        detail: Some(detail.chars().take(300).collect()),
        raised: Vec::new(),
        model: model.to_string(),
        secs,
        kind: String::new(),
        cite: String::new(),
        batch: 0,
    }
}

/// VA-089: fold ONE slice lane's reply — the lane's OWN questions and answers — into terminal
/// rows. `{answers: [{question, kind, cite, answer, raised}]}`: entry i is question q_index i
/// (the position IS the identity — the prompt carried no questions, so no tag table exists
/// between prompt and ledger); a non-empty answer is answered, a blank one unanswered/
/// empty_answer (its `raised` kept), an entry with no question text is a `StrayAnswer` (named by
/// the fan, never a row), an unknown `kind` is kept as `unkinded`. A reply that parses to ZERO
/// entries is the lane saying its sections settle everything — ONE lane-outcome row (reason
/// no_questions) holds that fact and is the slice's resume watermark; nothing parseable → one
/// row reason empty_answer with the raw head (300, the last_failure_tail idiom); Err → one row
/// judge_ended / provider_error. Every path leaves the slice at least one row, so "every slice
/// lane terminal" is a property of the type, never of a clock. `secs` is the session's wall time
/// on every row and `batch` the number of question rows (the row doc says why it is not split).
/// The run path calls `fold_research_lane_from` directly (the lane's `research_answer` calls may
/// have landed rows first); this zero-offset form is the tests' shorthand.
#[cfg(test)]
pub(super) fn fold_research_lane(
    slice: &str,
    model: &str,
    secs: u64,
    out: Result<String, String>,
) -> (Vec<ResearchRow>, Vec<StrayAnswer>) {
    fold_research_lane_from(slice, model, secs, out, 0)
}

/// `fold_research_lane` with the numbering started at `first_q_index` — the remainder of a lane
/// whose earlier answers already landed one by one through the per-answer tool
/// (`ResearchToolCall::into_row`), so the final reply's entries never collide with landed minis. An
/// OUTCOME row (the Err, unparseable and all-stray arms) sits at `first_q_index` for the same
/// reason — at 0 it would overwrite the first landed mini — and an empty remainder behind landed
/// rows is no row at all (the minis are the record) unless a `builder_decides` list needs a
/// home, which then rides one `remainder_empty` outcome row at that index.
/// The lane-level `builder_decides` list (VA-118 item 3) rides labelled
/// (`BUILDER_DECIDES_PREFIX`) in the FIRST row's `raised` — or in the lane-outcome row when no
/// question landed — because the row cannot grow in this commit (see the prefix consts); a lane
/// that derived no question but listed builder decisions is still `no_questions`, with the count
/// stated in its detail.
pub(super) fn fold_research_lane_from(
    slice: &str,
    model: &str,
    secs: u64,
    out: Result<String, String>,
    first_q_index: usize,
) -> (Vec<ResearchRow>, Vec<StrayAnswer>) {
    #[derive(serde::Deserialize, Default)]
    struct DerivedReply {
        #[serde(default)]
        answers: Vec<DerivedAnswer>,
        #[serde(default)]
        builder_decides: Vec<String>,
    }
    let raw = match out {
        Ok(raw) => raw,
        Err(e) => {
            let reason = if e.contains(JUDGE_ENDED_NEEDLE) {
                "judge_ended"
            } else {
                "provider_error"
            };
            let mut outcome = lane_outcome_row(slice, reason, &e, model, secs);
            outcome.q_index = first_q_index;
            return (vec![outcome], Vec::new());
        }
    };
    let (entries, builder_decides): (Vec<DerivedAnswer>, Vec<String>) =
        match parse_json_lenient::<DerivedReply>(&raw) {
            Some(reply) => (reply.answers, reply.builder_decides),
            None => {
                let mut outcome = lane_outcome_row(slice, "empty_answer", &raw, model, secs);
                outcome.q_index = first_q_index;
                return (vec![outcome], Vec::new());
            }
        };
    let mut decides: Vec<String> = Vec::new();
    for d in &builder_decides {
        let d = one_line(d);
        if !d.is_empty() && !decides.contains(&d) {
            decides.push(d);
        }
    }
    let labelled_decides = || -> Vec<String> {
        decides
            .iter()
            .map(|d| format!("{BUILDER_DECIDES_PREFIX}{d}"))
            .collect()
    };
    if entries.is_empty() {
        if first_q_index > 0 {
            if decides.is_empty() {
                return (Vec::new(), Vec::new());
            }
            let mut outcome = lane_outcome_row(
                slice,
                "remainder_empty",
                &format!(
                    "{first_q_index} question(s) landed through {RESEARCH_ANSWER_TOOL}; the \
                     final reply added none and listed {} builder_decides",
                    decides.len()
                ),
                model,
                secs,
            );
            outcome.q_index = first_q_index;
            outcome.raised = labelled_decides();
            return (vec![outcome], Vec::new());
        }
        let detail = if decides.is_empty() {
            "the lane read its sections and derived no design or external question".to_string()
        } else {
            format!(
                "the lane read its sections and derived no design or external question; it \
                 listed {} choice(s) only this slice's builder makes (builder_decides)",
                decides.len()
            )
        };
        let mut outcome = lane_outcome_row(slice, "no_questions", &detail, model, secs);
        outcome.raised = labelled_decides();
        return (vec![outcome], Vec::new());
    }
    let mut rows: Vec<ResearchRow> = Vec::new();
    let mut strays: Vec<StrayAnswer> = Vec::new();
    for (position, entry) in entries.into_iter().enumerate() {
        match row_from_entry(slice, first_q_index + position, entry, model, secs) {
            Ok(row) => rows.push(row),
            Err(stray) => strays.push(stray),
        }
    }
    if rows.is_empty() {
        // Every entry lacked a question: the strays are named by the fan; the slice still needs
        // its terminal row.
        let mut outcome = lane_outcome_row(
            slice,
            "empty_answer",
            "every entry of the lane's reply lacked a question",
            model,
            secs,
        );
        outcome.q_index = first_q_index;
        outcome.raised = labelled_decides();
        return (vec![outcome], strays);
    }
    let n = rows.len();
    for row in &mut rows {
        row.batch = n;
    }
    rows[0].raised.extend(labelled_decides());
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
        batch: 0,
    }
}

/// THE ONE emission site for a terminal row's events (the digestStreamFields law: the fan's
/// lane closure and its panicked-lane arm each carried a verbatim `research_unanswered`
/// writer). `research_answered` / `research_unanswered` exactly as before, then ONE
/// `research_raised_folded` per question the lane raised — the WORDS, not a count: the
/// `raised` count on `research_answered` was the only trace r6b's 48 raised questions left, so
/// tick.py could count them and nobody could read them. Since VA-089 the row's
/// `research_question_kind{slice, q_index, kind, cite, question}` rides first — the lane's own
/// question, named as it lands. `raised_by` is the parent row's durable
/// mini — the primary material an operator opens to read the whole row — and the question rides
/// as a hard 200-char head because this feeds an event, not a model (the head_to_sentence_end
/// rule's own exemption, the same cut `research_dispatched` makes).
pub(super) fn emit_research_outcome(events: &dyn EventSink, row: &ResearchRow) {
    // VA-089: the question's KIND is the lane's word about its own question, named as the row
    // lands (the opener names none). A lane-outcome row has no question and no kind. VA-118:
    // `source` says who decided — the lane (`model`) or `classify_design_entry` (`classifier`,
    // kind `spec_restated`, whose `model_kind` was `design` — the only tag the classifier
    // overrides).
    if !row.question.is_empty() {
        let kind = QuestionKind::from_stored(&row.kind);
        events.write_value(serde_json::json!({
            "event": "research_question_kind",
            "slice": row.slice,
            "q_index": row.q_index,
            "kind": row.kind,
            "source": kind.source(),
            "model_kind": (kind == QuestionKind::SpecRestated).then_some("design"),
            "cite": (!row.cite.is_empty()).then(|| row.cite.clone()),
            "question": row.question.chars().take(200).collect::<String>(),
        }));
    }
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
        match raised_destination(q) {
            // VA-118 item 5: a point for another slice, named with its destination — the field
            // answer_routing reads at plan time once the row carries it as a field.
            RaisedDestination::OtherSlice { slice, text } => {
                events.write_value(serde_json::json!({
                    "event": "research_raised_for",
                    "from": row.slice,
                    "to": slice,
                    "q_index": row.q_index,
                    "raised_by": research_mini_name(&row.slice, row.q_index),
                    "text": text.chars().take(200).collect::<String>(),
                }));
            }
            // VA-118 item 3: a choice only this slice's builder makes — the named absence of a
            // research question, one event per line so the vigil reads the words.
            RaisedDestination::BuilderDecides(text) => {
                events.write_value(serde_json::json!({
                    "event": "research_builder_decides",
                    "slice": row.slice,
                    "q_index": row.q_index,
                    "text": text.chars().take(200).collect::<String>(),
                }));
            }
            RaisedDestination::ThisBuilder(text) => {
                events.write_value(serde_json::json!({
                    "event": "research_raised_folded",
                    "slice": row.slice,
                    "q_index": row.q_index,
                    "raised_by": research_mini_name(&row.slice, row.q_index),
                    "question": text.chars().take(200).collect::<String>(),
                }));
            }
        }
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

/// The brief's rendering of a slice's claimed sections, all at once (`briefs_from_slices`).
/// The research lane reads the SAME sections through `section_hand` — the one matcher both
/// call, so the heading-match rule cannot diverge between them (the digestStreamFields law:
/// one shared join, never a hand-copied loop; the loop had already been duplicated verbatim
/// at both sites once).
///
/// A claimed heading that matches NO spec section is a MEASURED absence, never a silent drop:
/// r5's boot slice claimed a typo'd heading and lost 3,501 chars from BOTH its research
/// prompts and its brief, surfacing only through the generic `spec_sections_unclaimed` on the
/// real heading. Each miss emits `slice_claimed_section_unmatched{slice, claimed}` — loud,
/// MILD, never blocks; the matching sections still splice. Both sides are compared on
/// `heading_key` — decoration folds (r6d: "vs7dbg — REQUIRED and graded" claimed against
/// "#### `vs7dbg` — REQUIRED and graded" missed twice), letters do not.
///
/// VA-128: the matcher is `section_hand`; this renders every matched section at once — the
/// brief's shape. The research lane no longer receives this: it is handed the same sections one
/// at a time (`section_in_hand_block`).
pub(super) fn splice_claimed_sections(
    slice_id: &str,
    claimed: &[String],
    sections: &[SpecSection],
    events: &dyn EventSink,
) -> String {
    section_hand(slice_id, claimed, sections, events)
        .iter()
        .map(|sec| format!("\n{}", sec.render()))
        .collect()
}

/// One claimed section as the research lane receives it (VA-128): heading, request.md span and
/// body — the same three facts `splice_claimed_sections` renders for the brief, kept apart so
/// the lane's hand can be dealt one section per turn. VA-118: the span rides under the heading
/// so a lane's `cite` is the handed lines — r6i's structure lane ran 14 sed/grep calls over
/// ranges it already held to learn the line numbers it wanted to cite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HandedSection {
    pub(super) heading: String,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
    pub(super) body: String,
}

impl HandedSection {
    pub(super) fn render(&self) -> String {
        format!(
            "### {}\n[request.md:{}-{}]\n{}",
            self.heading,
            self.line_start,
            self.line_end,
            self.body.trim()
        )
    }

    fn span(&self) -> String {
        format!("request.md:{}-{}", self.line_start, self.line_end)
    }
}

/// THE ONE matcher for a slice's claimed headings (VA-128; the loop `splice_claimed_sections`
/// carried): the matched sections in claim order, each miss a loud
/// `slice_claimed_section_unmatched{slice, claimed}`.
pub(super) fn section_hand(
    slice_id: &str,
    claimed: &[String],
    sections: &[SpecSection],
    events: &dyn EventSink,
) -> Vec<HandedSection> {
    let mut hand: Vec<HandedSection> = Vec::new();
    for want in claimed {
        let key = heading_key(want);
        match sections.iter().find(|s| heading_key(&s.heading) == key) {
            Some(sec) => hand.push(HandedSection {
                heading: sec.heading.clone(),
                line_start: sec.line_start,
                line_end: sec.line_end,
                body: sec.body.clone(),
            }),
            None => {
                events.write_value(serde_json::json!({
                    "event": "slice_claimed_section_unmatched",
                    "slice": slice_id,
                    "claimed": want,
                }));
            }
        }
    }
    hand
}

/// The section a research lane holds RIGHT NOW (VA-128, `index` 0-based into `hand`): its full
/// text, then a one-line index of what follows — headings and spans only, never their text —
/// so the lane knows the order without holding the words. THE MEASURED WASTE (r6j, read from
/// the lanes' words): handed all nine of its sections at once, the api lane reasoned 182k
/// chars over 79 minutes and landed NOTHING, re-drafting its entry list four times across
/// tool-call turns ("Entry A-G" → "1-8" → "A…") because a turn's reasoning is not carried into
/// the next and each turn restarted the sort; core held 57 minutes then landed 12 in 10;
/// web-viz (10 sections) held 21 minutes then landed 4 in 2. The shape of the message, not the
/// model: a stateless model is handed ONE section's frozen lines per turn and lands (or closes
/// the section) before the next appears. Rendered at dispatch (section 1) and in the tool's
/// result at every `section_done` — one writer, so the two turns read the same words.
pub(super) fn section_in_hand_block(hand: &[HandedSection], index: usize) -> String {
    let of = hand.len();
    let sec = &hand[index];
    let mut block = format!(
        "SECTION {} of {of}, in hand now — settle it, then call {RESEARCH_ANSWER_TOOL} with \
         {{\"section_done\": true}}:\n{}",
        index + 1,
        sec.render()
    );
    let after: Vec<String> = hand[index + 1..]
        .iter()
        .enumerate()
        .map(|(i, s)| format!("§{} {} [{}]", index + 2 + i, s.heading, s.span()))
        .collect();
    if after.is_empty() {
        block.push_str(
            "\n\nTHIS IS THE LAST SECTION: when it is settled, {\"section_done\": true} closes \
             it and final_output follows.",
        );
    } else {
        block.push_str(&format!(
            "\n\nAFTER THIS SECTION (handed in this order, one per section_done — their text \
             arrives then, not now): {}",
            after.join("; ")
        ));
    }
    block
}

/// The hand as the dispatch text opens it: the framing, then section 1 (`section_in_hand_block`);
/// an EMPTY hand is stated as the measured absence it is — the slice claimed no heading, or
/// none matched (`slice_claimed_section_unmatched` named each) — never filled from the index.
pub(super) fn hand_block_at_dispatch(hand: &[HandedSection]) -> String {
    if hand.is_empty() {
        return "\n\nNO SECTION OF THE REQUEST IS IN HAND for this slice — it claimed no heading, \
                or none it claimed matched one (`slice_claimed_section_unmatched` names each \
                miss): every section's full text is in the request file named under SOURCES; \
                open the ones your question needs."
            .to_string();
    }
    format!(
        "\n\nTHE SPEC'S OWN SECTIONS FOR THIS SLICE — the sections this slice OWNS, verbatim, the \
         authority over any paraphrase — are handed to you ONE AT A TIME ({} in all). Settle the \
         section in hand: one {RESEARCH_ANSWER_TOOL} call per question you derive from it, then \
         one call with {{\"section_done\": true}} (add \"builder_decides\": [...] for the choices \
         only this slice's builder makes) — the next section's text arrives in THAT call's result. \
         A section with nothing to settle is that one section_done call, never a silent skip. Do \
         not open a later section from the request file ahead of its turn.\n\n{}",
        hand.len(),
        section_in_hand_block(hand, 0)
    )
}

/// The progress event both hand-off sites write (VA-128): the dispatch text for section 1
/// (`research_dispatch_text`) and the tool result for every next one
/// (`ResearchLanding::land`) — one writer, so tick.py reads one shape per section.
pub(super) fn emit_section_handed(
    events: &dyn EventSink,
    task: &str,
    slice: &str,
    hand: &[HandedSection],
    index: usize,
) {
    let sec = &hand[index];
    events.write_value(serde_json::json!({
        "event": "research_section_handed",
        "task": task,
        "slice": slice,
        "heading": sec.heading,
        "index": index + 1,
        "of": hand.len(),
        "lines": sec.span(),
        "chars": sec.body.chars().count(),
    }));
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
/// prompt (`research_request_block`), so the routing RULE cannot diverge between them. The
/// INPUTS still can (VA-045): the brief hands `sl.objective` as the vocabulary's third source
/// and `research_request_block` hands `""` — its one swarm.rs caller passes
/// `files_from_objective(&sl.objective)` and never the objective — so until that one-liner lands
/// (2c S9) a research prompt reads a smaller vocabulary than the brief of the same slice.
/// Measured on r6c's five REAL objectives (`r6c_slices`): the same sections arrive either way
/// (16/11/7/9/14), but notifierd's and web-viz's `Endpoints` arrive under rule (a) in the brief
/// (the objectives name `/api/notifications`, `/api/viz/records`, `/api/stream`) and under
/// rule (d) in the research prompt (the words `webhook`/`draft`, `stream`/`viz`).
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
/// (d) RESOURCE TOKEN (VA-032, the 2b refuter's finding — "the same class as the sort bug, one
///     step over") — a section whose endpoint table advertises a route whose RESOURCE WORD
///     (`spec_surface::resource_words`: the first segment after the request's own mount
///     prefixes, with its singular/plural sibling) the slice's vocabulary uses as a word. r6c's
///     §7 describes the drafts panel — `#draft-form`, `#draft-list`, `#approve-btn`, "the
///     drafts call", "the draft's state" (request.md:432-437) — and never writes `/api/drafts`,
///     so §5's route table (the five drafts rows, request.md:316-320) did not route to
///     web-console under (a); that slice saw `/api/drafts` only inside D2's handoff paragraph.
///     The vocabulary is the slice's claimed bodies, declared files AND objective; the words
///     come from the request's routes, never from a list. Coarser than (a) by design — a slice
///     that talks about the resource is handed the resource's table — and reported under its
///     own rule name so the tick can tell the two apart.
///     Two filters, both derived from the plan and never from a list (VA-044 F2; measured on
///     r6c before them: §6 reached web-viz by "events" — §8's "wheel events" — and Endpoints
///     reached notifierd by "health" and "notifications", its OWN routes spelled like ledgerd's):
///     (i) UBIQUITY — a resource that EVERY slice's claimed bodies name distinguishes nobody and
///     does not route; judged on the resource's word set (rule d fires on any spelling, so the
///     same predicate is asked of every slice), over the slices that claimed bodies at all, and
///     only when at least two did — one slice has nobody to be told apart from. (ii) OWN
///     RESOURCES — the resources this slice's own claimed sections advertise are what it
///     SERVES; the same word in another section's routes is not a call into that section. A
///     word that survives both routes (MILD), and every word a filter removed rides
///     `resource_token_filtered{slice, section, own, ubiquitous, carried}` so the tick can read
///     why a section did or did not arrive.
///
/// Each rule that fires emits `spec_sections_consumed{slice, rule, sections}` beside the
/// existing `spec_sections_unclaimed`, so the tick can read where every section went.
pub(super) fn consumed_spec_sections(
    slice_id: &str,
    claimed: &[String],
    files: &[String],
    objective: &str,
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
    vocabulary.push_str(objective);
    vocabulary.push('\n');
    let advertised: Vec<Vec<String>> = sections.iter().map(advertised_paths).collect();
    let mount = mount_prefixes(&advertised.iter().flatten().cloned().collect::<Vec<_>>());
    // Rule (d)'s filter (i): the resources every claimed body names. The bodies are the plan's
    // own claims — this slice's included — minus the cross-cutting sections, exactly the text
    // rule (d) reads for this slice; a slice that claimed nothing has no bodies and is not
    // counted, and one body has nobody to be told apart from.
    let claimed_bodies: Vec<String> = every_claim
        .iter()
        .map(|claims| {
            claims
                .iter()
                .filter_map(|h| index_of(h))
                .filter(|i| !cross.contains(i))
                .map(|i| sections[i].body.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|body| !body.trim().is_empty())
        .collect();
    let names_resource =
        |text: &str, words: &[String]| words.iter().any(|w| resource_word_named(w, text));
    let mut ubiquitous: BTreeSet<String> = BTreeSet::new();
    if claimed_bodies.len() >= 2 {
        for path in advertised.iter().flatten() {
            let words = resource_words(path, &mount);
            if !words.is_empty() && claimed_bodies.iter().all(|b| names_resource(b, &words)) {
                ubiquitous.extend(words);
            }
        }
    }
    // Filter (ii): the resources this slice's own claimed sections advertise.
    let own_resources: BTreeSet<String> = own
        .iter()
        .flat_map(|i| advertised[*i].iter())
        .flat_map(|p| resource_words(p, &mount))
        .collect();
    let mut by_route: Vec<usize> = Vec::new();
    let mut by_resource: Vec<usize> = Vec::new();
    for (i, paths) in advertised.iter().enumerate() {
        if own.contains(&i) || cross.contains(&i) {
            continue;
        }
        if paths.iter().any(|p| path_token_named(p, &vocabulary)) {
            by_route.push(i);
            continue;
        }
        let mut carried: BTreeSet<String> = BTreeSet::new();
        let mut own_hits: BTreeSet<String> = BTreeSet::new();
        let mut ubiquitous_hits: BTreeSet<String> = BTreeSet::new();
        for path in paths {
            let words = resource_words(path, &mount);
            let named = words
                .iter()
                .filter(|w| resource_word_named(w, &vocabulary))
                .cloned();
            if words.iter().any(|w| own_resources.contains(w)) {
                own_hits.extend(named);
            } else if words.iter().any(|w| ubiquitous.contains(w)) {
                ubiquitous_hits.extend(named);
            } else {
                carried.extend(named);
            }
        }
        if !carried.is_empty() {
            by_resource.push(i);
        }
        if !own_hits.is_empty() || !ubiquitous_hits.is_empty() {
            events.write_value(serde_json::json!({
                "event": "resource_token_filtered",
                "slice": slice_id,
                "section": sections[i].heading,
                "own": own_hits,
                "ubiquitous": ubiquitous_hits,
                "carried": carried,
            }));
        }
    }
    let mut by_parent: Vec<usize> = Vec::new();
    if let Some(top) = top {
        for parent in own.iter().filter(|i| sections[**i].level > top) {
            for child in children_of(sections, *parent) {
                if !own.contains(&child)
                    && !cross.contains(&child)
                    && !by_route.contains(&child)
                    && !by_resource.contains(&child)
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
                    "\n### {}\n[request.md:{}-{}]\n{}",
                    sections[*i].heading,
                    sections[*i].line_start,
                    sections[*i].line_end,
                    sections[*i].body.trim()
                )
            })
            .collect::<String>()
    };
    for (rule, ids) in [
        ("advertised_route", &by_route),
        ("resource_token", &by_resource),
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
    called.extend(by_resource);
    called.extend(by_parent);
    ConsumedSections {
        called_into: render(&called),
        cross_cutting: render(&broadcast),
    }
}

/// The per-slice REQUEST block for a research prompt (A5): the prompt NEVER carries the raw ~50k
/// spec when orientation is armed — it carries the orientation index and the sections the slice
/// CALLS INTO; the slice's own claimed sections are its HAND (VA-128: `section_hand`, dealt one
/// per turn by `hand_block_at_dispatch` and the tool's result — no longer spliced whole here).
/// Below the arming floor the whole spec is the better input, exactly as OPEN's own message
/// formation decides it.
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
    // The same helper, the same plan-wide inputs the brief builder hands it (VA-030): every
    // slice's claims so rule (c) counts claimants across the plan, and this slice's declared
    // files so rule (a) reads the routes its files name. Before, the research prompt passed
    // only its own claims — every childless top-level section read as cross-cutting and rule
    // (a) had no files to read. The slice's OBJECTIVE (the third vocabulary source of rules
    // (a) and (d)) is not in this signature: its one caller (swarm.rs, the research fan) hands
    // `files_from_objective(&sl.objective)` and not the objective itself, so here rules (a) and
    // (d) read the claimed bodies and the declared files only — a smaller vocabulary, never a
    // substituted one, and a DIVERGENCE from the brief until 2c S9 adds the objective here.
    // Measured on r6c's five REAL objectives (VA-045, `r6c_slices`): the objective adds no
    // SECTION the bodies do not already route (same 16/11/7/9/14), but it does add routes —
    // notifierd's names `/api/notifications`, web-viz's `/api/viz/records` and `/api/stream`
    // — so those two slices' `Endpoints` arrive under (a) in the brief and under (d) here.
    let consumed =
        consumed_spec_sections(slice_id, claimed, files, "", every_claim, sections, events);
    let orientation = spec_orientation(sections);
    // r6c, research-ledgerd-core-q2: the Health shape lived in a section this slice did not
    // claim; the lane saw only the index's excerpt ("shape below"), called the shape "not
    // pinned in the provided spec text" and invented one. The index is a MAP, not a wall:
    // the owned sections are the lane's area — its HAND, dealt one section per turn (VA-128,
    // `hand_block_at_dispatch`; the head no longer carries them) — and any other section it
    // needs is one read away in the request file. Stated here, where the index is.
    let mut block = format!(
        "THE REQUEST, AS ITS ORIENTATION INDEX (every section: heading, size, opening \
         sentences). The sections this slice OWNS are handed to you further down, one at a \
         time; a question may reach into a section that is only INDEXED here (an endpoint's \
         response shape, a counter's lifetime, a boot flag): open that section in the request \
         file named under SOURCES and answer from its words — never from the index's excerpt \
         alone:\n\n{orientation}"
    );
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

/// The whole user text of one research call. The DECISIONS lane: its head, the snowball block
/// (empty on a first dispatch — no heading, no filler), then EVERY open decision VERBATIM, each
/// tagged `[qN]` with its q_index — the tag the reply's `question_index` repeats, so no
/// translation table exists between the prompt and the ledger. A SLICE lane (VA-089): its head
/// (the orientation index, objective, sources), the HAND's first section (VA-128 —
/// `hand_block_at_dispatch`; the rest arrive one per `section_done` in the tool's result), the
/// snowball block, the sibling slices' objectives, then the DERIVE instruction — the two
/// question kinds, the evidence each carries, what is NOT a question (a fact the section
/// states; an open decision), and that every question is answered in this same session. No
/// question text rides in: the lane writes its own.
pub(super) fn research_user_text(prior_block: &str, lane: &ResearchLane) -> String {
    if !lane.derives() {
        let tagged: Vec<String> = lane
            .questions
            .iter()
            .map(|q| format!("[q{}] {}", q.q_index, q.question))
            .collect();
        return format!(
            "{}{prior_block}\n\nTHE OPEN DECISIONS ({}) — each was put to the user and the user \
             did not answer it; settle EVERY one in this session. Each is tagged [qN]; your \
             final_output carries one entry per tag with question_index = N:\n{}",
            lane.head,
            lane.questions.len(),
            tagged.join("\n")
        );
    }
    let siblings = if lane.siblings.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\n\nTHE OTHER SLICES OF THIS REQUEST — their objectives, what THEIR builders own. A \
             question about one of these is theirs, not yours: ask it here only when this slice's \
             sections must match its exact shape (a path, a payload, a DOM id, a signature both \
             builders must agree on); a point you notice that belongs to one of them goes in \
             `raised_for` with that slice's id — it is handed to that slice, so do not deliberate \
             whether to drop it:\n{}",
            lane.siblings
        )
    };
    format!(
        "{}{}{prior_block}{siblings}\n\nYOUR WORK, slice `{}`: DERIVE this slice's questions, \
         then ANSWER them, in this one session, ONE SECTION AT A TIME. THE SECTION IN HAND above \
         IS the request for this slice right now: every fact it states you already hold, verbatim, \
         with its request.md lines under its heading — do not re-read it from the request file and \
         do not search the request to prove a silence; a silence is stated by naming the handed \
         section that would have carried the fact and does not. Sort every candidate three ways \
         before you write: a \
         pasted line answers it → it is NOT a question, write nothing (the builder holds the same \
         text); the vendor's documentation answers it → kind external; the request leaves it OPEN → \
         kind design, only when another slice's builder or the vendor must AGREE to the answer, or \
         its consequence reaches beyond this slice's own files:\n\
         — kind design: name in `alternatives` the two or more answers the request admits, in \
         `open_because` why the handed lines do not settle it, in `cite` the request.md line(s) of \
         the handed section nearest to it; then DECIDE — the answer names the choice and the reason. \
         A design entry that can show only one option is recorded as spec_restated: the request \
         settled it.\n\
         — kind external: the vendor's documentation (or another source outside the request) \
         settles it. Fetch it, answer from its words, and put the doc section (URL and heading) \
         in `cite`.\n\
         Not a question: a fact a pasted line states; one of the open decisions (USER DECISIONS \
         above, or one the request assigns to the builder as a decision); a choice only this \
         slice's builder feels at the keyboard — a buffer layout, a debounce, a helper's name, an \
         internal state shape — list those under `builder_decides` on the section_done call that \
         closes the section, one line each, no answer. Answer every question you write; the reply \
         shape is in the system message.",
        lane.head,
        hand_block_at_dispatch(&lane.hand),
        lane.slice
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

/// The ROUTE PATHS a question names — `/api/health`, `/api/events`, `/api/payments/<id>/note`
/// — lowercased: the one literal, explainable link between two lanes' questions across slices.
/// A PATH RULE (VA-032, the works-prover): a token is a path only when it starts with `/`.
/// "Contains a slash" was the rule before, and r6d's questions carried `maker/checker`,
/// `status/currency`, `d1/d2/d3`, `size/framing`, `html/css/js/ico`, `received/applied/duplicate`
/// — prose alternations, none of them a place two lanes could be talking about. Punctuation
/// that prose hangs on a path (`/api/health,` or `(/api/events)`) is trimmed; a trailing full
/// stop too. A route written without its leading slash (`api/health`) is not recognised: the
/// clause that would recognise it — a match against the request's advertised route prefixes —
/// needs the spec surface at `relay_targets`' and `research_dispatch_text`'s call sites (both
/// in swarm.rs); measured on r6c's 26 and r6d's 29 questions no route was written that way.
fn path_tokens(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| "`'\"(),;:?![]{}<>".contains(c))
                .trim_end_matches('.')
        })
        .filter(|t| t.starts_with('/') && t.len() > 2 && t.chars().any(char::is_alphanumeric))
        .map(str::to_lowercase)
        .collect()
}

/// The first route path a lane's MATERIAL (`ResearchLane::material` — its objective and claimed
/// sections, or the decisions' lines; VA-089: a lane has no questions before it runs) shares
/// with `row`'s question — `/api/health`, `/api/events` — or None. One of the three links
/// `stranger_admission` reads; before VA-131 it was the only one.
fn names_a_shared_path(paths: &BTreeSet<String>, row: &ResearchRow) -> Option<String> {
    let theirs = path_tokens(&row.question);
    paths.intersection(&theirs).next().cloned()
}

/// The files a lane's MATERIAL declares in backticks (`files_from_objective`, lowercased). The
/// opener is told to name each slice's owned files in its objective, which leads the material;
/// a backticked file in a claimed section's body is one the request itself places in this
/// slice's territory. A `ResearchLane` carries no `files` of its own (swarm.rs builds it as a
/// literal of slice/head/siblings/questions/material — a `files` field there is that file's
/// owner's one-line follow-up), so both channels read the declaration out of the same text the
/// path rule reads.
fn declared_files(material: &str) -> BTreeSet<String> {
    files_from_objective(material)
        .into_iter()
        .map(|f| f.to_lowercase())
        .collect()
}

/// The first of `files` that `text` names as a bare token — `web/app.js` in "between web/app.js
/// (web-page) and web/viz.js (web-viz)" — trimmed of the punctuation prose hangs on it exactly
/// as `path_tokens` trims a route, `./` stripped, lowercased. A possessive or a glued suffix
/// (`web/app.js's`) is not recognised: conservative on purpose, like `files_from_objective`.
fn names_a_file(files: &BTreeSet<String>, text: &str) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    text.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| "`'\"(),;:?![]{}<>".contains(c))
                .trim_end_matches('.')
        })
        .map(|t| t.strip_prefix("./").unwrap_or(t).to_lowercase())
        .find(|t| files.contains(t))
}

/// WHY a landed row reaches a lane — rendered into `research_context.prior_from` as
/// `<mini> (<label>)` and into the snowball block, so the tick reads the link, not only a count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// The lane's own slice (the decisions lane's resumed decisions).
    OwnSlice,
    /// The row's question names this route path and so does the lane's material.
    Path(String),
    /// The row's lane raised a point FOR this slice: `[for <slice>]` in `raised`, the on-disk
    /// form of the reply's `raised_for` (`row_from_entry` writes it, `raised_destination` reads it).
    RaisedFor,
    /// The row's question or answer names this file, which the lane's material declares.
    File(String),
}

impl Admission {
    pub(super) fn label(&self) -> String {
        match self {
            Admission::OwnSlice => "own slice".to_string(),
            Admission::Path(p) => format!("path {p}"),
            Admission::RaisedFor => "raised_for".to_string(),
            Admission::File(f) => format!("file {f}"),
        }
    }
}

/// One prior mini a dispatching lane sees, and why.
#[derive(Clone, Debug)]
pub(super) struct PriorMini<'a> {
    pub(super) row: &'a ResearchRow,
    pub(super) why: Admission,
}

/// THE ONE admission rule for a STRANGER's row (another slice's answered mini), shared by the
/// dispatch-time snowball (`prior_minis_for`) and the late relay (`relay_targets`) so the two
/// channels cannot disagree about which stranger's mini a lane should see. Three links, the
/// most explicit first: (1) the row's lane RAISED a point for this slice — r6j web-viz-q0's
/// `[for web-page] Implement window.page.showRecord(id)…` addressed web-page by name and the
/// path filter alone dropped it, so web-page (dispatched 17:02:23Z, prior_minis=0 with four
/// web-viz minis on disk) designed a second, conflicting bridge; (2) the row's question names a
/// route path the lane's material names (r6c's `/api/health`); (3) the row's question or answer
/// names a file the lane's material declares (web-viz-q0's question named `web/app.js`, the
/// file web-page owns). The caller decides what "stranger" means: `prior_minis_for` admits its
/// own slice first, `relay_targets` never relays to the row's own slice.
fn stranger_admission(target: &RelayTarget, row: &ResearchRow) -> Option<Admission> {
    if row.status != RESEARCH_ANSWERED {
        return None;
    }
    let raised_for_me = row.raised.iter().any(|line| {
        matches!(
            raised_destination(line),
            RaisedDestination::OtherSlice { slice, .. } if slice.eq_ignore_ascii_case(&target.slice)
        )
    });
    if raised_for_me {
        return Some(Admission::RaisedFor);
    }
    if let Some(path) = names_a_shared_path(&target.paths, row) {
        return Some(Admission::Path(path));
    }
    names_a_file(&target.files, &row.question)
        .or_else(|| names_a_file(&target.files, &row.answer))
        .map(Admission::File)
}

/// The already-answered minis a dispatching lane should see (fix B, the snowball inside the
/// fan): every ANSWERED row of its own slice that is not in the lane (the decisions lane's
/// resumed decisions — a slice lane never dispatches once its slice has rows), plus an answered
/// row of ANOTHER slice that `stranger_admission` links to the lane (r6c: ledgerd-api-q0's
/// question named `/api/health` and its answer carried the exact Health shape ten minutes
/// before ledgerd-core asked what `/api/health` exposes — and invented one; r6j: web-page was
/// dispatched with four web-viz minis on disk and saw none, because web-viz-q0's links to it
/// were its `[for web-page]` raise and the file it named, never a route path). Own slice first,
/// then the admitted strangers, each row once with its reason. Unanswered rows are never
/// spliced: their absence already rode `research_unanswered`.
pub(super) fn prior_minis_for<'a>(
    lane: &ResearchLane,
    rows: &'a [ResearchRow],
) -> Vec<PriorMini<'a>> {
    let target = lane.relay_target();
    let mut same: Vec<PriorMini<'a>> = Vec::new();
    let mut matched: Vec<PriorMini<'a>> = Vec::new();
    for r in rows {
        if r.status != RESEARCH_ANSWERED {
            continue;
        }
        if r.slice == lane.slice {
            if !lane.questions.iter().any(|q| q.q_index == r.q_index) {
                same.push(PriorMini {
                    row: r,
                    why: Admission::OwnSlice,
                });
            }
        } else if let Some(why) = stranger_admission(&target, r) {
            matched.push(PriorMini { row: r, why });
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
pub(super) fn prior_minis_block(slice: &str, prior: &[PriorMini<'_>]) -> String {
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
    for p in prior {
        let r = p.row;
        let link = match &p.why {
            Admission::OwnSlice => String::new(),
            Admission::Path(path) => format!(
                "its question names `{path}`, a path this slice's objective or sections name"
            ),
            Admission::RaisedFor => {
                format!(
                    "its lane raised a point FOR this slice (`[for {slice}]` in its raised lines)"
                )
            }
            Admission::File(file) => {
                format!("it names `{file}`, a file this slice's objective or sections declare")
            }
        };
        let from = match (r.slice == slice, r.slice == DECISION_SLICE) {
            (true, true) => "an earlier open decision this fan settled".to_string(),
            (true, false) => "this slice's own earlier lane".to_string(),
            (false, true) => format!("an open decision this fan settled — {link}"),
            (false, false) => format!("slice `{}` — {link}", r.slice),
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

/// The dispatch-time assembly of one lane's user text, and the one `research_context` event per
/// dispatch that lets the tick print a lane's grounding: whether it derives its own questions or
/// carries the open decisions (and how many), how many prior minis it saw (and which), and how
/// many sections the index named for it (0 when the orientation is not armed and the whole
/// request rides inline).
pub(super) fn research_dispatch_text(
    root: &Path,
    events: &dyn EventSink,
    lane: &ResearchLane,
    activity_key: &str,
    index_sections: usize,
) -> String {
    let rows = load_research_minis(root);
    let prior = prior_minis_for(lane, &rows);
    events.write_value(serde_json::json!({
        "event": "research_context",
        "task": activity_key,
        "slice": lane.slice,
        "derives": lane.derives(),
        "decisions": lane.questions.len(),
        "prior_minis": prior.len(),
        "prior_from": prior
            .iter()
            .map(|p| {
                format!(
                    "{} ({})",
                    research_mini_name(&p.row.slice, p.row.q_index),
                    p.why.label()
                )
            })
            .collect::<Vec<_>>(),
        "index_sections": index_sections,
        "sections_in_hand": lane.hand.len(),
    }));
    // VA-128: section 1 is handed with the dispatch text; the landing hands every next one.
    if !lane.hand.is_empty() {
        emit_section_handed(events, activity_key, &lane.slice, &lane.hand, 0);
    }
    research_user_text(&prior_minis_block(&lane.slice, &prior), lane)
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

/// The lanes a just-landed mini is relayed to — RE-AIMED by C3, then by VA-089, widened by
/// VA-131. Under one lane per slice there is no same-slice sibling left to relay to (a slice's
/// rows all land when its one lane ends), and a lane carries no questions before it runs, so
/// the relay reads each running lane's `RelayTarget` with the same rule the dispatch-time
/// snowball uses (`stranger_admission`): every STILL-RUNNING lane of another slice the landed
/// row raised a point for, or whose objective or claimed sections name a path the landed
/// question names or a file the landed row names — the set `prior_minis_for` would have
/// spliced had that lane dispatched a moment later. The first wave's lanes all dispatch at once
/// and see NO prior minis, so this relay is the only way ledger-api's `/api/health` shape
/// reaches a running ledger-core lane (the r6c invention). Only an answered row relays (an
/// unanswered one already rode `research_unanswered`); a lane never receives its own slice's
/// row. `running` is (activity key, the lane's relay target) for every lane between dispatch
/// and its rows.
pub(super) fn relay_targets(
    landed: &ResearchRow,
    running: &[(String, RelayTarget)],
) -> Vec<String> {
    relay_admissions(landed, running)
        .into_iter()
        .map(|(key, _)| key)
        .collect()
}

/// `relay_targets` with each target's reason — the one `stranger_admission` the dispatch-time
/// snowball applies, so a test can pin that the two channels agree on one fixture.
/// `research_tool::queue_research_relay` reads only the keys: one note serves every target.
pub(super) fn relay_admissions(
    landed: &ResearchRow,
    running: &[(String, RelayTarget)],
) -> Vec<(String, Admission)> {
    if landed.status != RESEARCH_ANSWERED {
        return Vec::new();
    }
    running
        .iter()
        .filter(|(_, t)| t.slice != landed.slice)
        .filter_map(|(key, t)| stranger_admission(t, landed).map(|why| (key.clone(), why)))
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
         it links to this slice: it raised a point for you, or it names a route path or a file \
         this slice's objective or sections name; it is now in .swarm/ledger/{from_mini}. \
         Build on it: where an answer of yours depends on it, agree with it or NAME the \
         disagreement and the request's words that decide it (the builder receives both \
         answers) — never contradict it silently. Continue the SAME work; do not restart.\n\
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
        // The MINI's kind — the rollup's discriminator for what this file is, never the
        // question's kind, which serde writes as `question_kind` from the row itself.
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
/// discarded Option. Before this the lane rows and the panic rows dropped the result: a mini
/// that never reached disk was rendered into the brief from memory while resume and the
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

/// The fan's PLAN as one event, emitted once when it is built and before anything dispatches
/// (VA-089): how many LANES run (one per slice not resumed, plus the decisions lane when any
/// decision is left) — questions are not known at planning, the lanes derive them — the
/// sections each slice's lane reads (`per_slice_sections`, from the opener's claims), the slices
/// whose rows the ledger already held (`resumed_slices`) and the open decisions the decisions
/// lane carries. Every number derived from the plan itself; an instrument that has to guess the
/// denominator is not one.
pub(super) fn emit_research_planned(
    events: &dyn EventSink,
    lanes: usize,
    per_slice_sections: &std::collections::BTreeMap<String, usize>,
    resumed_slices: &[String],
    decisions: usize,
) {
    events.write_value(serde_json::json!({
        "event": "research_planned",
        "lanes": lanes,
        "per_slice_sections": per_slice_sections,
        "resumed_slices": resumed_slices,
        "decisions": decisions,
    }));
}

/// The request's own decision ids as they appear in text — `D1`, `D2:`, `D1/D2/D3` — the spec's
/// "## D1 / ## D2 / ## D3" vocabulary. Uppercase D followed by digits, not inside a longer word
/// (`D1` yes, `ID1` no, `D1x` no). (Moved from the deleted research_plan.rs, VA-089: its C2
/// routing/cover door went with the opener's questions; this and `content_words` stayed because
/// `slice_vocabulary` and `decision_consumers` read them.)
pub(super) fn decision_ids(text: &str) -> BTreeSet<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        let boundary_before = i == 0 || !chars[i - 1].is_alphanumeric();
        if chars[i] == 'D' && boundary_before {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let boundary_after = j == chars.len() || !chars[j].is_alphanumeric();
            if j > i + 1 && boundary_after {
                if let Ok(n) = chars[i + 1..j].iter().collect::<String>().parse::<u32>() {
                    out.insert(n);
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Function words that carry no identity. A linguistic constant, not a tuning knob.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "for", "and", "or", "is", "are", "be", "it", "its",
    "this", "that", "what", "which", "how", "does", "do", "any", "at", "as", "by", "with", "from",
    "into", "per", "vs", "so", "must", "can", "if", "than", "then", "they", "their", "there",
    "these", "those", "when", "where", "who", "whom", "will", "would", "should", "not", "no",
    "yes", "all", "each", "every", "one", "two", "three", "via", "over", "under", "about", "after",
    "before", "between", "while", "both", "either", "neither", "exactly", "exact", "also", "only",
];

/// The words a text's identity rests on: lowercased runs of `[a-z0-9_/#.-]` (so `/api/events`
/// and `notify.db` stay whole), stripped of surrounding dots and dashes, three letters or more,
/// not a function word.
pub(super) fn content_words(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    let mut out = BTreeSet::new();
    for tok in lower.split(|c: char| !(c.is_ascii_alphanumeric() || "_/#.-".contains(c))) {
        let t = tok.trim_matches(|c| c == '.' || c == '-');
        if t.chars().count() >= 3 && !STOPWORDS.contains(&t) {
            out.insert(t.to_string());
        }
    }
    out
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
/// bodies cite (`D1` in §9's body, claimed by r6c's web-console). A decision naming NO slice is
/// every slice's — MILD: it
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
        .filter(|(i, _)| {
            !question_ids.is_disjoint(&vocabularies[*i].decision_ids)
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
            "\n\n{} BY RESEARCH that name this slice — the verdict, \
             the request-grounded lines and the paragraphs addressed to this slice, verbatim and \
             BINDING; implement each exactly as written and never substitute your own \
             convention (a decision naming no slice is repeated in every brief):\n{settled}",
            decisions::SETTLED_DECISIONS_HEADER
        ));
    }
    out.push_str(&decisions::decisions_brief_block(&without_transcript));
    out
}

/// The brief partition every slice's builder reads, assembled from the opener's slice and the
/// fan's terminal rows — the objective, then what the slice's research lane settled (its OWN
/// questions, answered; VA-089), the questions it raised and could not answer, its lane's
/// outcome when no answer landed, the raised questions, the claimed spec sections verbatim, and
/// the decisions partition. (Moved
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
            // VA-008: the sections this slice CALLS INTO and the rules that bind every slice —
            // the same helper the research prompt uses, here with the plan-wide view (every
            // slice's claims, this slice's declared files).
            let consumed = armed.then(|| {
                consumed_spec_sections(
                    &sl.id,
                    &sl.sections,
                    &files,
                    &sl.objective,
                    &every_claim,
                    &sections,
                    events,
                )
            });
            // VA-089: the slice's research rows are its LANE's own questions and answers (the
            // opener writes none), read from the ledger in q_index order. An answered row renders
            // under ANSWERS SETTLED AT PLAN TIME with its kind and the evidence it cited; an
            // unanswered row that still names its question stays a question for the builder (the
            // absence already rode `research_unanswered` — never a fabricated answer, the fallback
            // gate); a lane-outcome row (no question: the lane derived none, or failed) is a
            // stated absence under its own heading. With no research rows the brief carries none
            // of the three blocks. The whole answer rides (VA-030): the 1,500-char head cut left
            // r6c's briefs pointing at files no worker is told to read.
            let mut slice_rows: Vec<&ResearchRow> =
                research.iter().filter(|r| r.slice == sl.id).collect();
            slice_rows.sort_by_key(|r| r.q_index);
            let answered_n = slice_rows
                .iter()
                .filter(|r| r.status == RESEARCH_ANSWERED)
                .count();
            let mut answers_block = String::new();
            let mut open_questions: Vec<&str> = Vec::new();
            let mut lane_outcomes: Vec<String> = Vec::new();
            for row in &slice_rows {
                if row.question.trim().is_empty() {
                    let detail = match &row.detail {
                        Some(d) => format!(": {}", head_to_sentence_end(d, 300).replace('\n', " ")),
                        None => String::new(),
                    };
                    lane_outcomes.push(format!(
                        "- {}{detail}",
                        row.reason.as_deref().unwrap_or(RESEARCH_UNANSWERED)
                    ));
                    continue;
                }
                if row.status != RESEARCH_ANSWERED {
                    open_questions.push(&row.question);
                    continue;
                }
                let kind = if row.kind.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", row.kind)
                };
                let evidence = if row.cite.trim().is_empty() {
                    String::new()
                } else {
                    format!("\nEVIDENCE: {}", row.cite.trim())
                };
                answers_block.push_str(&format!(
                    "\nQ:{kind} {}\nA: {}{evidence}\n",
                    row.question,
                    row.answer.trim_end()
                ));
            }
            if !answers_block.is_empty() {
                brief.push_str(&format!(
                    "\n\nANSWERS SETTLED AT PLAN TIME — this slice's research lane derived these \
                     questions from the sections below and answered them from the request and the \
                     sources (kind design: a convention the request leaves open, decided with its \
                     evidence; kind external: the vendor's documentation, cited); build to these \
                     unless the spec or a USER DECISIONS block contradicts them:{answers_block}"
                ));
            }
            if !open_questions.is_empty() {
                brief.push_str(&format!(
                    "\n\nQUESTIONS this slice must settle in its implementation — its research lane \
                     derived these and could not answer them (conventional answers unless the \
                     request says otherwise):\n{}",
                    open_questions
                        .iter()
                        .map(|q| format!("- {q}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            if !lane_outcomes.is_empty() {
                brief.push_str(&format!(
                    "\n\nRESEARCH LANE OUTCOME for this slice — no answer landed; the lane's own \
                     outcome, from its ledger row:\n{}",
                    lane_outcomes.join("\n")
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
                if let Some(consumed) = &consumed {
                    brief.push_str(&consumed_sections_blocks(consumed));
                }
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
                let first_answer_head = slice_rows
                    .iter()
                    .find(|r| r.status == RESEARCH_ANSWERED)
                    .map(|r| {
                        head_to_sentence_end(
                            &r.answer
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .take(3)
                                .collect::<Vec<_>>()
                                .join(" "),
                            400,
                        )
                    });
                match first_answer_head {
                    Some(h) => format!("{}/{} — {}", answered_n, slice_rows.len(), h.trim_end()),
                    None => format!("{}/{}", answered_n, slice_rows.len()),
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
        "one lane per slice derives that slice's design and external questions from its sections \
         and answers them in one session; slices queue across the hosts",
    );
    events.write_value(serde_json::json!({"event": "phase", "phase": "research"}));
}

/// The system text of one research call. The DECISIONS lane keeps the tagged-questions text; a
/// SLICE lane (VA-089) is told to DERIVE its own questions from its sections and answer them —
/// the reply shape carries `question`, `kind`, `cite`, `answer`, `raised` per entry, composed
/// inside the final_output call.
pub(super) fn research_system_text(lane: &ResearchLane) -> String {
    if !lane.derives() {
        return "You are answering the tagged OPEN DECISIONS of this request — all of them, in this \
     one session; each must be settled before anything is built. Ground every answer: read the \
     request text you were given, read the existing tree's files with your shell and tree \
     tools, and when the request names a documentation URL, fetch it — an answer copied from the \
     real source beats any paraphrase. Do NOT create or edit files: you have no write or edit \
     tool, and your structured reply IS your deliverable.\n\n\
     Each answer is a HANDOFF to the builders: name exact files, exact key/field literals, exact \
     endpoints or signatures where the request implies them; where the request is silent, state \
     the most CONVENTIONAL choice and say it is a convention. Before you call anything a \
     convention, check the orientation index for a section that names it and read that section \
     from the request file named under SOURCES — silence in your excerpt is not silence in the \
     request. Settle a shared fact ONCE and let the later answers refer back to it, never \
     contradict it. If a decision cannot be settled from the request or the sources, say exactly \
     that in one line and still name the conventional choice. Keep each answer under a page.\n\n\
     When ALL of them are done, call the final_output tool ONCE with {\"answers\": \
     [{\"question_index\": N, \"answer\": \"...\", \"raised\": [...]}, ...]} — one entry per \
     [qN] tag with question_index = N, in any order. COMPOSE EACH ANSWER INSIDE THAT CALL'S \
     ARGUMENTS: an answer drafted in your reasoning first is written twice and read by no one \
     until the call lands — once a fact is settled, the next thing you write is the tool call \
     that carries it. A tag you omit is recorded as UNANSWERED, \
     so include every one, even as \"cannot be settled: <why>; convention: <choice>\". `raised` \
     lists further questions you could NOT settle: do not answer them, and nothing will dispatch \
     them; they are handed VERBATIM to the builders as open points, so phrase each \
     as a decision a builder can make in one line, naming the conventional choice when you \
     have one."
            .to_string();
    }
    "You are the RESEARCHER of ONE slice of this request. The request's sections for this slice \
     are handed to you ONE AT A TIME: the section in your message IS the request for this slice \
     right now — verbatim, with its request.md lines under its heading — so you already hold \
     every fact it states; re-reading it from disk or searching the request for what it says is \
     not research. The next section arrives in the result of the research_answer call that \
     closes the current one ({\"section_done\": true}); do not read ahead. You derive this \
     slice's questions yourself and answer them, section by section, all in this one session. \
     Ground every answer in the section you were given, the existing \
     tree's files (your shell and tree tools) and, when the request names a documentation URL, \
     the vendor's documentation — fetch it; an answer copied from the real source beats any \
     paraphrase. Do NOT create or edit files: you have no write or edit tool, and your structured \
     reply IS your deliverable.\n\n\
     A question is worth writing only if a builder holding the same sections would still have to \
     settle it WITH someone else: another slice's builder (a path, a payload, a DOM id, a \
     signature both sides must agree on), the vendor (kind external — its documentation; cite the \
     section), or a decision the request leaves open whose consequence reaches beyond this \
     slice's files (kind design — name the alternatives the request admits, why the handed lines \
     do not settle it, then decide with the reason). What the sections already state is not a \
     question: write no entry for it, the builder reads the same text. A choice only this slice's \
     builder feels — a buffer layout, a debounce, a helper's name, an internal state shape — is not \
     research: list it under `builder_decides`, one line each, no answer. A design entry that can \
     name only one option is recorded as spec_restated — the request settled it. Each answer is a \
     HANDOFF to the builder: exact files, exact key/field literals, exact endpoints or signatures \
     where the request implies them; a convention is stated as a convention. Settle a shared fact \
     ONCE and let later answers refer back to it, never contradict it. Keep each answer under a \
     page.\n\n\
     THE MOMENT ONE QUESTION IS SETTLED, call the research_answer tool with that ONE entry: \
     {\"question\": \"...\", \"kind\": \"design\" | \"external\", \"cite\": \"request.md:<lines> \
     or <doc section>\", \"alternatives\": [\"...\", \"...\"], \"open_because\": \"...\", \
     \"answer\": \"...\", \"raised\": [...], \"raised_for\": [{\"slice\": \"<other slice id>\", \
     \"text\": \"...\"}]} — it lands in the ledger at once, the other slices' lanes can read it \
     while you settle the next question, and the tool's reply names the file it landed in. \
     COMPOSE EACH ENTRY INSIDE THAT CALL'S ARGUMENTS: a question or an answer drafted in your \
     reasoning first is written twice and read by no one until the call lands — once you have a \
     question and its evidence, the next thing you write is the research_answer call that \
     carries it. Then the next question, one call each, in the order you settle them. When the \
     section in hand has nothing more to settle — or nothing at all — call research_answer with \
     {\"section_done\": true, \"builder_decides\": [\"...\"]} (the choices only this slice's \
     builder makes, one line each, no answer; the list may be empty): that ONE call closes the \
     section, never a silent skip, and its result carries the next section's text. An entry may \
     carry \"section_done\": true itself when it is the section's last. When every section is \
     closed, call the final_output tool ONCE with {\"answers\": [<only the entries you did NOT \
     land through research_answer, same shape>], \"builder_decides\": [\"...\"]} — an EMPTY \
     answers list when every question already landed, and also when the sections settle \
     everything and no design or external question remains (builder_decides may still be \
     filled): that is a complete, honest reply. Never repeat a landed entry in final_output — it \
     would land again under a new index. A question you could not answer still gets its entry \
     with an empty answer and the reason in `raised`. `raised` lists points for THIS slice's builder \
     you could not settle: do not answer them, and nothing will dispatch them; they are handed \
     VERBATIM to that builder as open points, so phrase each as a decision the builder can make in \
     one line, naming the conventional choice when you have one. `raised_for` lists points that \
     belong to ANOTHER slice — its id from THE OTHER SLICES list and the point in one line; they \
     are handed to that slice, so do not deliberate whether to drop them."
        .to_string()
}

#[cfg(test)]
mod tests {
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
            batch: 0,
        }
    }

    fn rq(slice: &str, q_index: usize, question: &str) -> ResearchQuestion {
        ResearchQuestion {
            slice: slice.to_string(),
            q_index,
            question: question.to_string(),
            kind: QuestionKind::Design,
            cite: String::new(),
        }
    }

    /// A SLICE lane as the fan builds it (VA-089): no questions — the lane derives its own — and
    /// `material` is what the cross-slice path rule reads.
    fn lane(slice: &str, head: &str, material: &str) -> ResearchLane {
        ResearchLane {
            slice: slice.to_string(),
            head: head.to_string(),
            siblings: String::new(),
            questions: Vec::new(),
            material: material.to_string(),
            hand: Vec::new(),
        }
    }

    fn decisions_lane(head: &str, qs: Vec<ResearchQuestion>) -> ResearchLane {
        let material = qs
            .iter()
            .map(|q| q.question.clone())
            .collect::<Vec<_>>()
            .join("\n");
        ResearchLane {
            slice: DECISION_SLICE.to_string(),
            head: head.to_string(),
            siblings: String::new(),
            questions: qs,
            material,
            hand: Vec::new(),
        }
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
                "research_question_kind",
                "research_answered",
                "research_raised_folded",
                "research_raised_folded",
                "research_question_kind",
                "research_unanswered",
            ],
            "VA-089: the lane's own question is named (kind) as its row lands, then the outcome; \
             one named event per raised question, none for a row that raised nothing"
        );
        assert_eq!(events[0]["kind"], "design");
        assert_eq!(events[0]["q_index"], 1);
        assert_eq!(
            events[1]["raised"], 2,
            "the count still rides research_answered"
        );
        assert_eq!(events[2]["slice"], "app-boot");
        assert_eq!(events[2]["q_index"], 1);
        assert_eq!(
            events[2]["raised_by"], "research-app-boot-q1.json",
            "raised_by is the parent row's durable mini, derived from the row itself"
        );
        assert_eq!(
            events[3]["question"],
            "Exact SIGTERM grace before SIGKILL — chose 5 s by convention."
        );
        assert_eq!(events[5]["reason"], "lane_panicked");
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
                    weight: 3,
                    sections: Vec::new(),
                },
                OpenSlice {
                    id: "web".into(),
                    title: "the console".into(),
                    objective: "render the table".into(),
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
        let mut payments = lane("payments", &head, "sync payments from the vendor");
        payments.hand = section_hand("s1", &["Alpha".to_string()], &sections, &NullSink);
        let text = research_user_text("", &payments);
        assert!(
            text.contains("CLAIMED_DEEP_MARKER") && text.contains("SECTION 1 of 1, in hand now"),
            "the claimed section's FULL text rides in, as the section in hand (VA-128):\n{text}"
        );
        assert!(
            !block.contains("CLAIMED_DEEP_MARKER"),
            "the head no longer carries the claimed sections — the hand does"
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
            text.contains("YOUR WORK, slice `payments`: DERIVE this slice's questions")
                && text.contains("kind design")
                && text.contains("kind external")
                && !text.contains("[q0]"),
            "VA-089: the lane derives its own questions — no tagged list rides in:\n{text}"
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

    /// VA-128 (a): a three-section slice is dispatched with section 1's text and the INDEX of
    /// §2 and §3 — their headings and spans, never their words — and `research_section_handed`
    /// {index: 1, of: 3} rides the dispatch. r6j's api lane was handed nine sections at once and
    /// landed nothing in 79 minutes; the hand deals one.
    #[test]
    fn a_three_section_slice_is_dispatched_with_section_one_and_only_the_index_of_the_rest() {
        let spec = "# Boot\nBOOT_WORDS on port 8850.\n\n# Endpoints\nENDPOINT_WORDS GET \
                    /api/health.\n\n# Rules\nRULE_WORDS bump the version.\n";
        let sections = spec_sections(spec);
        let sink = ValueSink::default();
        let mut api = lane("api", "HEAD", "Own `app/api.py`.");
        api.hand = section_hand(
            "api",
            &[
                "Boot".to_string(),
                "Endpoints".to_string(),
                "Rules".to_string(),
            ],
            &sections,
            &sink,
        );
        assert_eq!(api.hand.len(), 3);
        let dir = tempfile::tempdir().unwrap();
        let text = research_dispatch_text(dir.path(), &sink, &api, "research-api", 3);
        assert!(
            text.contains("handed to you ONE AT A TIME (3 in all)")
                && text.contains("SECTION 1 of 3, in hand now")
                && text.contains("### Boot\n[request.md:1-3]\nBOOT_WORDS on port 8850."),
            "section 1 rides whole:\n{text}"
        );
        assert!(
            text.contains(
                "AFTER THIS SECTION (handed in this order, one per section_done — their text \
                 arrives then, not now): §2 Endpoints [request.md:4-6]; §3 Rules [request.md:7-8]"
            ),
            "the rest is an index of headings and spans:\n{text}"
        );
        assert!(
            !text.contains("ENDPOINT_WORDS") && !text.contains("RULE_WORDS"),
            "no later section's words ride the dispatch:\n{text}"
        );
        assert!(
            text.contains("THE SECTION IN HAND above IS the request for this slice right now"),
            "the instruction binds the lane to the section in hand:\n{text}"
        );
        let ev = sink.0.lock().unwrap();
        let names: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["research_context", "research_section_handed"]);
        assert_eq!(ev[0]["sections_in_hand"], 3);
        assert_eq!(ev[1]["task"], "research-api");
        assert_eq!(ev[1]["slice"], "api");
        assert_eq!(ev[1]["heading"], "Boot");
        assert_eq!(ev[1]["index"], 1);
        assert_eq!(ev[1]["of"], 3);
        assert_eq!(ev[1]["lines"], "request.md:1-3");
        drop(ev);
        // A lane with nothing in hand states the absence — never the index's excerpts as text.
        let bare = lane("web", "HEAD", "Own `web/app.js`.");
        let text = research_dispatch_text(dir.path(), &sink, &bare, "research-web", 3);
        assert!(
            text.contains("NO SECTION OF THE REQUEST IS IN HAND for this slice")
                && !text.contains("SECTION 1 of"),
            "{text}"
        );
        assert!(
            section_in_hand_block(&api.hand, 2).contains("THIS IS THE LAST SECTION"),
            "the last section says so instead of indexing nothing"
        );
    }

    /// VA-089's snowball: a lane carries no questions, so the cross-slice link reads its
    /// MATERIAL (objective + claimed sections). A first dispatch carries no prior block (no
    /// heading, no filler) and `research_context` says 0; once other lanes land, an answered
    /// stranger whose question names a path the material names rides in (r6c: ledgerd-api's exact
    /// Health shape, which ledgerd-core invented); an unanswered mini and an unrelated stranger
    /// stay out; the decisions lane's text is the tagged list and says it derives nothing.
    #[test]
    fn a_lane_sees_the_landed_minis_that_name_its_paths_and_a_first_dispatch_carries_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sink = ValueSink::default();
        let core = lane(
            "ledgerd-core",
            "HEAD",
            "Own `app/ledgerd/core.py`: the sync loop, ledger.db and the /api/health degraded state.",
        );
        let first = research_dispatch_text(root, &sink, &core, "research-ledgerd-core", 28);
        assert!(
            first.starts_with("HEAD\n\nNO SECTION OF THE REQUEST IS IN HAND for this slice")
                && first.contains("\n\nYOUR WORK, slice `ledgerd-core`")
                && !first.contains("ALREADY ANSWERED")
                && !first.contains("[q0]"),
            "a first dispatch: head, the hand's measured absence (this fixture claims no \
             section), the derive instruction, nothing invented between them:\n{first}"
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev.len(), 1);
            assert_eq!(ev[0]["event"], "research_context");
            assert_eq!(ev[0]["task"], "research-ledgerd-core");
            assert_eq!(ev[0]["slice"], "ledgerd-core");
            assert_eq!(ev[0]["derives"], true);
            assert_eq!(ev[0]["decisions"], 0);
            assert_eq!(ev[0]["prior_minis"], 0);
            assert_eq!(ev[0]["index_sections"], 28);
            assert_eq!(ev[0]["prior_from"], serde_json::json!([]));
        }
        // Lanes finish and write their minis: api-q0 (another slice, its question names
        // /api/health), api-q3 unanswered, web-q0 an unrelated stranger.
        let mut api_q0 = row("ledgerd-api", 0, RESEARCH_ANSWERED, &[]);
        api_q0.question =
            "What are the exact response shapes for /api/health, /api/summary, and /api/buckets?"
                .into();
        api_q0.answer = "GET /api/health: {\"status\": \"ok\", \"payments\": <int>, \
                         \"last_sync\": <str or null>, \"webhook\": {...}}"
            .into();
        write_research_ledger(root, &api_q0).unwrap();
        let mut api_q3 = row("ledgerd-api", 3, RESEARCH_UNANSWERED, &[]);
        api_q3.question = "Where are the /api/health counters exposed?".into();
        write_research_ledger(root, &api_q3).unwrap();
        let mut web_q0 = row("web-console", 0, RESEARCH_ANSWERED, &[]);
        web_q0.question = "Which filter params does the table use?".into();
        web_q0.answer = "status and currency, from section 7.".into();
        write_research_ledger(root, &web_q0).unwrap();

        let second = research_dispatch_text(root, &sink, &core, "research-ledgerd-core", 28);
        assert!(
            second.contains("ALREADY ANSWERED BY THIS FAN before your dispatch (1)"),
            "the real count:\n{second}"
        );
        assert!(second.contains(
            "[slice `ledgerd-api` — its question names `/api/health`, a path this slice's \
             objective or sections name; .swarm/ledger/research-ledgerd-api-q0.json]"
        ));
        assert!(
            second.contains("\"payments\": <int>"),
            "the exact Health shape reaches the lane that invented one in r6c"
        );
        assert!(
            !second.contains("section 7") && !second.contains("counters exposed"),
            "no shared path, and unanswered rows stay out:\n{second}"
        );
        assert!(
            second.find("ALREADY ANSWERED").unwrap() < second.find("YOUR WORK").unwrap(),
            "the snowball precedes the derive instruction"
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev.len(), 2);
            assert_eq!(ev[1]["prior_minis"], 1);
            assert_eq!(
                ev[1]["prior_from"],
                serde_json::json!(["research-ledgerd-api-q0.json (path /api/health)"])
            );
        }
        let d = decisions_lane(
            "HEAD",
            vec![
                ResearchQuestion::decision(0, "D2: is rejected terminal?"),
                ResearchQuestion::decision(2, "D3: empty-with-progress or loading?"),
            ],
        );
        let decision = research_dispatch_text(root, &sink, &d, "research-decisions", 0);
        assert!(
            decision.contains("THE OPEN DECISIONS (2)")
                && decision.contains("\n[q0] D2: is rejected terminal?")
                && decision.ends_with("\n[q2] D3: empty-with-progress or loading?"),
            "{decision}"
        );
        assert!(
            !decision.contains("ALREADY ANSWERED") && !decision.contains("YOUR WORK"),
            "no decision settled yet and no path shared — nothing is spliced; no derive text"
        );
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev[2]["derives"], false);
        assert_eq!(ev[2]["decisions"], 2);
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

    /// VA-089: the fan's plan is LANES, not questions — one per slice always (questions are not
    /// known at planning), the sections each lane reads, the slices resumed from the ledger and
    /// the open decisions the decisions lane carries; emitted once, before anything dispatches.
    #[test]
    fn the_planned_queue_is_emitted_once_with_one_lane_per_slice() {
        let sink = ValueSink::default();
        let per_slice: std::collections::BTreeMap<String, usize> = [
            ("ledgerd-core".to_string(), 3usize),
            ("web-console".to_string(), 0),
            ("ledgerd-api".to_string(), 2),
        ]
        .into_iter()
        .collect();
        emit_research_planned(&sink, 3, &per_slice, &["ledgerd-api".to_string()], 1);
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            ev[0],
            serde_json::json!({
                "event": "research_planned",
                "lanes": 3,
                "per_slice_sections": {"ledgerd-api": 2, "ledgerd-core": 3, "web-console": 0},
                "resumed_slices": ["ledgerd-api"],
                "decisions": 1,
            }),
            "VA-089: lanes = sessions — two slice lanes (one slice resumed from the ledger) plus \
             the decisions lane; a slice with zero claimed sections still gets a lane; no question \
             count exists at planning"
        );
        let fan_src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/swarm.rs"
        ))
        .unwrap();
        assert_eq!(
            fan_src.matches("emit_research_planned(").count(),
            1,
            "one emission site, when the fan's plan is built"
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

    /// THE r6c SHAPE, under one lane per slice (VA-089: a lane carries no questions, so the relay
    /// reads its MATERIAL — objective + claimed sections): ledger-api's lane lands its q0 (the
    /// exact /api/health shape) while the ledger-core lane (its material names the degraded
    /// state of /api/health) and the web-page lane (the brush) are still running. The relay
    /// reaches the core lane — its MATERIAL names the same path — and only it: not web-page (no shared
    /// path), never a lane of the landed row's own slice, never for an unanswered row, never
    /// for a landed question that names no path. The note carries the mini's path, the slice,
    /// the question and the budgeted answer.
    #[test]
    fn a_landed_mini_is_relayed_to_running_lanes_whose_material_names_its_path() {
        let running = vec![
            (
                "research-ledger-core".to_string(),
                lane(
                    "ledger-core",
                    "H",
                    "Own `app/ledgerd/core.py`: sync cursor persistence and what /api/health \
                     exposes as the degraded state.",
                )
                .relay_target(),
            ),
            (
                "research-web-page".to_string(),
                lane(
                    "web-page",
                    "H",
                    "Own `web/app.js`: the brush and the streamed mutation.",
                )
                .relay_target(),
            ),
            (
                "research-ledger-api".to_string(),
                lane(
                    "ledger-api",
                    "H",
                    "Own `app/ledgerd/api.py`: every /api/* route including /api/health.",
                )
                .relay_target(),
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
            vec!["research-ledger-core".to_string()],
            "the core lane's material names /api/health; the api lane is the row's own slice"
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

    /// VA-032 (the works-prover): the cross-slice link is a PATH rule. r6d's real questions
    /// carried `maker/checker`, `status/currency`, `d1/d2/d3` and `size/framing` — prose
    /// alternations that "contains a slash" took for paths — beside the two real links the
    /// fan measured: drafts-workflow-q4 ⇄ ledger-api-q5 via `/api/events` and ledger-api-q0
    /// ⇄ ledger-core-q4 via `/api/health`. The four are not paths; the two are; a stranger
    /// that repeats an alternation word-for-word links to nobody, and both real pairs survive
    /// in the relay and in the dispatch-time snowball alike.
    #[test]
    fn the_cross_slice_link_is_a_path_rule_not_a_slash_rule() {
        for prose in [
            "maker/checker",
            "status/currency",
            "d1/d2/d3",
            "size/framing",
        ] {
            assert!(
                path_tokens(&format!("Do {prose} see it (the {prose}) — {prose}?")).is_empty(),
                "{prose} is an alternation, not a path"
            );
        }
        assert_eq!(
            path_tokens("Do maker/checker see /api/events at all (auth applies to the endpoint)?"),
            BTreeSet::from(["/api/events".to_string()])
        );
        assert_eq!(
            path_tokens("How is 'vendor down' surfaced to /api/health and the UI degraded state?"),
            BTreeSet::from(["/api/health".to_string()])
        );
        assert_eq!(
            path_tokens(
                "What are the exact /api/health, /api/summary and /api/buckets response shapes?"
            ),
            BTreeSet::from([
                "/api/health".to_string(),
                "/api/summary".to_string(),
                "/api/buckets".to_string()
            ])
        );
        assert!(path_tokens("a lone / and a trailing /. end").is_empty());

        // r6d, the two real pairs and one false pair under the old rule's shape.
        let api_q5 = rq(
            "ledger-api",
            5,
            "Does GET /api/events require a token of ANY known role, and does admin's \
             read-everything include the full event history from seq 1?",
        );
        let drafts_q4 = rq(
            "drafts-workflow",
            4,
            "Do maker/checker see /api/events at all (auth applies to the endpoint; is there \
             any role-based filtering of event visibility)?",
        );
        let core_q4 = rq(
            "ledger-core",
            4,
            "How is 'vendor down' surfaced to /api/health and the UI degraded state, and how \
             long does registration retry before giving up (it must keep retrying)?",
        );
        let api_q0 = rq(
            "ledger-api",
            0,
            "What are the exact /api/health, /api/summary and /api/buckets response shapes \
             ('shape below' in full text), and what is the bucket key/granularity?",
        );
        let stranger = rq(
            "web-page",
            9,
            "Which maker/checker controls show, and which status/currency filters become \
             visible for d1/d2/d3 and the size/framing of the table?",
        );
        let api_q1 = rq(
            "ledger-api",
            1,
            "Which sort keys does sort=<k> accept; which status/currency values do the filters \
             accept?",
        );
        let landed = |q: &ResearchQuestion| {
            let mut r = row(&q.slice, q.q_index, RESEARCH_ANSWERED, &[]);
            r.question = q.question.clone();
            r
        };
        // VA-089: a lane's link is its MATERIAL; here each lane's material is the words of the
        // question its slice would have asked.
        let running = vec![
            (
                "research-drafts-workflow".to_string(),
                lane("drafts-workflow", "H", &drafts_q4.question).relay_target(),
            ),
            (
                "research-ledger-core".to_string(),
                lane("ledger-core", "H", &core_q4.question).relay_target(),
            ),
            (
                "research-web-page".to_string(),
                lane("web-page", "H", &stranger.question).relay_target(),
            ),
        ];
        assert_eq!(
            relay_targets(&landed(&api_q5), &running),
            vec!["research-drafts-workflow".to_string()],
            "/api/events links api-q5 to the drafts lane and to nobody else"
        );
        assert_eq!(
            relay_targets(&landed(&api_q0), &running),
            vec!["research-ledger-core".to_string()],
            "/api/health links api-q0 to the core lane"
        );
        assert!(
            relay_targets(&landed(&api_q1), &running).is_empty(),
            "`status/currency` shared word-for-word with the stranger links nobody"
        );
        let rows = vec![landed(&api_q5), landed(&api_q0), landed(&api_q1)];
        let seen: Vec<String> =
            prior_minis_for(&lane("drafts-workflow", "H", &drafts_q4.question), &rows)
                .iter()
                .map(|p| format!("{}-q{}", p.row.slice, p.row.q_index))
                .collect();
        assert_eq!(seen, vec!["ledger-api-q5".to_string()]);
        assert!(
            prior_minis_for(&lane("web-page", "H", &stranger.question), &rows).is_empty(),
            "the snowball agrees with the relay: no alternation links a stranger"
        );
    }

    /// r6j (VA-131): web-viz-q0 landed 16:49:25Z with `[for web-page] Implement
    /// window.page.showRecord(id)…` in its raised lines; web-page was dispatched 17:02:23Z with
    /// `research_context.prior_minis=0` because the path filter read only the question's route
    /// paths — and designed a second bridge (`window.appApi`) against the landed one. The row
    /// now reaches a web-page lane whose material names NO shared path and declares NO file
    /// (the raise alone links it), at the consumer: the dispatch text carries the landed
    /// interface and `research_context.prior_from` says why.
    #[test]
    fn a_sibling_raise_for_this_slice_reaches_its_lane_without_a_shared_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sink = ValueSink::default();
        let mut viz_q0 = row(
            "web-viz",
            0,
            RESEARCH_ANSWERED,
            &[
                "[for web-page] Implement window.page.showRecord(id) -> bool: scroll the table to \
               the row and open the drawer; return false when the id is outside the current \
               filter.",
            ],
        );
        viz_q0.question = "What is the exact JS interface between web/app.js (web-page) and \
                           web/viz.js (web-viz) for the linked brush?"
            .into();
        viz_q0.answer = "window.viz.toggleRecord(id) and window.viz.clearBrush(); viz dispatches \
                         CustomEvents viz:brush and viz:batch; the page exposes \
                         window.page.showRecord(id) -> bool."
            .into();
        write_research_ledger(root, &viz_q0).unwrap();
        let page = lane(
            "web-page",
            "HEAD",
            "Own the record table, the drawer and the brush highlight on the page.",
        );
        assert!(
            page.relay_target().paths.is_empty() && page.relay_target().files.is_empty(),
            "the fixture isolates the raise: no path, no declared file"
        );
        let text = research_dispatch_text(root, &sink, &page, "research-web-page", 12);
        assert!(
            text.contains("ALREADY ANSWERED BY THIS FAN before your dispatch (1)")
                && text.contains("window.page.showRecord(id) -> bool")
                && text.contains(
                    "[slice `web-viz` — its lane raised a point FOR this slice (`[for web-page]` \
                     in its raised lines); .swarm/ledger/research-web-viz-q0.json]"
                ),
            "the landed bridge reaches the lane that would otherwise design a second one:\n{text}"
        );
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev[0]["event"], "research_context");
        assert_eq!(ev[0]["prior_minis"], 1);
        assert_eq!(
            ev[0]["prior_from"],
            serde_json::json!(["research-web-viz-q0.json (raised_for)"])
        );
    }

    /// VA-131's third link: a stranger's row naming a file the lane's material declares in
    /// backticks — in its question (web-viz's `web/app.js`) or only in its answer
    /// (`web/index.html`) — reaches the lane, tagged with the file; the raise, when present,
    /// is the tag (the most explicit link wins). A case-insensitive `[for Web-Page]` and a
    /// `./web/app.js` spelling both link.
    #[test]
    fn a_row_naming_a_file_the_lane_declares_reaches_it() {
        let page = lane(
            "web-page",
            "H",
            "Own `web/app.js` and `web/index.html`: the record table and the drawer.",
        );
        assert_eq!(
            page.relay_target().files,
            BTreeSet::from(["web/app.js".to_string(), "web/index.html".to_string()])
        );
        let mut in_question = row("web-viz", 1, RESEARCH_ANSWERED, &[]);
        in_question.question =
            "Which DOM hook does ./web/viz.js call on web/app.js when a bar is clicked?".into();
        in_question.answer = "viz dispatches viz:brush; the page listens.".into();
        let mut in_answer = row("api", 2, RESEARCH_ANSWERED, &[]);
        in_answer.question = "Where is the static bundle served from?".into();
        in_answer.answer = "GET / returns `web/index.html`; assets under ./web/.".into();
        let mut raised_too = in_question.clone();
        raised_too.q_index = 3;
        raised_too.raised = vec!["[for Web-Page] listen for viz:brush on document".to_string()];
        let rows = vec![in_question, in_answer, raised_too];
        let seen: Vec<(String, Admission)> = prior_minis_for(&page, &rows)
            .iter()
            .map(|p| (format!("{}-q{}", p.row.slice, p.row.q_index), p.why.clone()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (
                    "web-viz-q1".to_string(),
                    Admission::File("web/app.js".to_string())
                ),
                (
                    "api-q2".to_string(),
                    Admission::File("web/index.html".to_string())
                ),
                ("web-viz-q3".to_string(), Admission::RaisedFor),
            ]
        );
        let block = prior_minis_block("web-page", &prior_minis_for(&page, &rows));
        assert!(
            block.contains(
                "[slice `web-viz` — it names `web/app.js`, a file this slice's objective or \
                 sections declare; .swarm/ledger/research-web-viz-q1.json]"
            ) && block.contains("[slice `api` — it names `web/index.html`, a file"),
            "{block}"
        );
    }

    /// VA-131 keeps the filter a filter: a stranger that raised a point for ANOTHER slice, names
    /// a route the lane's material does not, names no declared file — or misspells the slice
    /// (`web-pages`) — stays out; so does an unanswered row that raised a point for this slice.
    #[test]
    fn an_unrelated_stranger_still_stays_out_of_the_snowball() {
        let page = lane(
            "web-page",
            "H",
            "Own `web/app.js`: the record table and the drawer; it reads /api/records.",
        );
        let mut for_viz = row(
            "api",
            0,
            RESEARCH_ANSWERED,
            &["[for web-viz] bucket the totals"],
        );
        for_viz.question = "How does /api/events paginate?".into();
        for_viz.answer = "By cursor; the page size is 100.".into();
        let mut near_miss = row("api", 1, RESEARCH_ANSWERED, &["[for web-pages] keep it"]);
        near_miss.question = "Which sort keys does sort=<k> accept?".into();
        near_miss.answer = "amount, ts, status — from app/ledgerd/api.py.".into();
        let mut unanswered = row(
            "web-viz",
            0,
            RESEARCH_UNANSWERED,
            &["[for web-page] showRecord"],
        );
        unanswered.question = "What does web/app.js expose?".into();
        let rows = vec![for_viz, near_miss, unanswered];
        assert!(
            prior_minis_for(&page, &rows).is_empty(),
            "no raise for web-page, no shared path, no declared file, no answered row"
        );
        let running = vec![("research-web-page".to_string(), page.relay_target())];
        for r in &rows {
            assert!(
                relay_targets(r, &running).is_empty(),
                "{}-q{}",
                r.slice,
                r.q_index
            );
        }
    }

    /// The two channels are ONE rule: for every (running lane, landed row) pair of one fixture —
    /// a raise for the lane, a shared route path, a declared file, a stranger, the row's own
    /// slice — `relay_admissions` names exactly the reason `prior_minis_for` would splice under,
    /// or neither admits. (The own-slice pair is the one asymmetry by design: the snowball
    /// splices a slice's resumed rows, the relay never sends a lane its own slice's row.)
    #[test]
    fn the_relay_and_the_snowball_agree_on_one_admission_rule() {
        let lanes = vec![
            (
                "research-web-page".to_string(),
                lane(
                    "web-page",
                    "H",
                    "Own `web/app.js`: the table, the drawer, the brush.",
                ),
            ),
            (
                "research-ledger-core".to_string(),
                lane(
                    "ledger-core",
                    "H",
                    "Own `app/ledgerd/core.py`: sync and /api/health.",
                ),
            ),
            (
                "research-webhooks".to_string(),
                lane(
                    "webhooks",
                    "H",
                    "Own `app/ledgerd/hooks.py`: registration and replay.",
                ),
            ),
        ];
        let running: Vec<(String, RelayTarget)> = lanes
            .iter()
            .map(|(k, l)| (k.clone(), l.relay_target()))
            .collect();
        let mut viz_q0 = row(
            "web-viz",
            0,
            RESEARCH_ANSWERED,
            &["[for web-page] showRecord(id)"],
        );
        viz_q0.question = "What is the JS interface between web/app.js and web/viz.js?".into();
        let mut api_q0 = row("ledger-api", 0, RESEARCH_ANSWERED, &[]);
        api_q0.question = "What are the exact /api/health and /api/summary shapes?".into();
        let mut core_q2 = row("ledger-core", 2, RESEARCH_ANSWERED, &[]);
        core_q2.question = "Does the sync loop call into app/ledgerd/hooks.py on a 5xx?".into();
        let mut stranger = row("drafts", 4, RESEARCH_ANSWERED, &[]);
        stranger.question = "Do maker/checker see the same drafts list?".into();
        let rows = vec![viz_q0, api_q0, core_q2, stranger];
        let mut admitted = 0;
        for r in &rows {
            let relay = relay_admissions(r, &running);
            for (key, l) in &lanes {
                if l.slice == r.slice {
                    let snow = prior_minis_for(l, std::slice::from_ref(r));
                    assert_eq!(snow.len(), 1);
                    assert_eq!(snow[0].why, Admission::OwnSlice);
                    assert!(relay.iter().all(|(k, _)| k != key));
                    continue;
                }
                let snow: Option<Admission> = prior_minis_for(l, std::slice::from_ref(r))
                    .first()
                    .map(|p| p.why.clone());
                let late: Option<Admission> = relay
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, why)| why.clone());
                assert_eq!(snow, late, "{}-q{} -> {key}", r.slice, r.q_index);
                admitted += usize::from(snow.is_some());
            }
        }
        assert_eq!(
            admitted, 3,
            "the raise, the path and the file each link once; the stranger links nobody"
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
                    weight: 3,
                    sections: vec![table_heading.clone()],
                },
                OpenSlice {
                    id: "web".into(),
                    title: "web".into(),
                    objective: "the dashboard".into(),
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
                    weight: 3,
                    sections: Vec::new(),
                },
                OpenSlice {
                    id: "web".into(),
                    title: "the dashboard".into(),
                    objective: "draw the dashboard".into(),
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

    fn row_answered(slice: &str, q_index: usize, answer: &str) -> ResearchRow {
        let mut r = row(slice, q_index, RESEARCH_ANSWERED, &[]);
        r.answer = answer.to_string();
        r
    }

    /// The mini's `kind` is the ledger rollup's DISCRIMINATOR (`Some("research")` beside
    /// `task`/`gate`/`repair`), and `write_research_ledger` wrote that literal INTO the row's
    /// question kind: r6h's 8 `research_question_kind{external}` tags reached no mini and every
    /// resumed row read `kind: research`. The question's kind rides as `question_kind` and
    /// round-trips; the discriminator stays; a lane-outcome row (no question) carries "" — the
    /// honest absence, never a fabricated kind; a pre-VA-104 mini loads with the kind absent.
    #[test]
    fn a_lanes_question_kind_survives_the_mini_write_and_the_resume_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut external = row_answered(
            "webhooks",
            5,
            "From v3 docs §8: registration is idempotent by URL.",
        );
        external.question =
            "What do the v3 docs prescribe for POST /v3/webhooks registration?".into();
        external.kind = "external".into();
        write_research_ledger(root, &external).unwrap();
        let mini = root
            .join(LEDGER_DIR)
            .join(research_mini_name("webhooks", 5));
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mini).unwrap()).unwrap();
        assert_eq!(
            on_disk["kind"], "research",
            "the rollup's discriminator stays"
        );
        assert_eq!(
            on_disk["question_kind"], "external",
            "the lane's own word reaches disk"
        );
        assert_eq!(
            load_research_mini(root, "webhooks", 5).unwrap().kind,
            "external"
        );
        let outcome = lane_outcome_row("web", "no_questions", "derived none", "m", 40);
        write_research_ledger(root, &outcome).unwrap();
        assert_eq!(
            load_research_mini(root, "web", outcome.q_index)
                .unwrap()
                .kind,
            "",
            "no question, no kind — stated, not invented"
        );
        let legacy = serde_json::json!({"slice": "api", "q_index": 0, "question": "q",
            "status": "answered", "answer": "a", "model": "m", "secs": 1, "kind": "research"});
        std::fs::write(
            root.join(LEDGER_DIR).join(research_mini_name("api", 0)),
            legacy.to_string(),
        )
        .unwrap();
        assert_eq!(load_research_mini(root, "api", 0).unwrap().kind, "");
    }

    /// The brief partition (VA-089: the rows are the LANE's own questions): an answered row
    /// renders under ANSWERS SETTLED AT PLAN TIME with its kind and the evidence it cited; an
    /// unanswered row that names its question stays a question for the builder, BELOW the
    /// answers; a lane-outcome row (no question — the lane derived none, or failed) is a stated
    /// absence under its own heading, never a fabricated answer; with no research rows the brief
    /// carries none of the three blocks; a long answer is spliced under the measured-good budget
    /// with a stated truncation naming the durable mini.
    #[test]
    fn a_lanes_answers_settle_above_its_open_questions_and_a_lane_outcome_is_stated() {
        let opened = OpenOutput {
            slices: vec![
                OpenSlice {
                    id: "api".into(),
                    title: "the api".into(),
                    objective: "serve GET /health".into(),
                    weight: 3,
                    sections: Vec::new(),
                },
                OpenSlice {
                    id: "web".into(),
                    title: "the web".into(),
                    objective: "draw the table".into(),
                    weight: 2,
                    sections: Vec::new(),
                },
            ],
            open_decisions: Vec::new(),
        };
        let mut port = row_answered("api", 0, "Port 8850, from the spec's own boot table.");
        port.question = "which port".into();
        port.kind = "design".into();
        port.cite = "request.md:12 'boots on'; grep -n -i 'port' → no match".into();
        let mut storage = row("api", 1, RESEARCH_UNANSWERED, &[]);
        storage.question = "which storage".into();
        let none = lane_outcome_row(
            "web",
            "no_questions",
            "the lane read its sections and derived no design or external question",
            "m",
            40,
        );
        // Out of q_index order on purpose: the brief sorts.
        let rows = vec![storage, port, none];
        let briefs = briefs_from_slices(&opened, "build the app", &rows, &[], &NullSink);
        let b = &briefs[0].brief;
        assert!(b.contains("ANSWERS SETTLED AT PLAN TIME"));
        assert!(
            b.contains(
                "Q: [design] which port\nA: Port 8850, from the spec's own boot table.\n\
                 EVIDENCE: request.md:12 'boots on'; grep -n -i 'port' → no match"
            ),
            "the kind and the evidence ride with the answer:\n{b}"
        );
        let questions_at = b.find("QUESTIONS this slice must settle").unwrap();
        assert!(
            b.find("ANSWERS SETTLED AT PLAN TIME").unwrap() < questions_at,
            "the settled answers sit ABOVE the open questions"
        );
        let from_questions = b.split_at(questions_at).1;
        assert!(
            from_questions.contains("- which storage") && !from_questions.contains("- which port"),
            "the answered question is settled; the unanswered one stays a question:\n{b}"
        );
        assert!(
            !b.contains("RESEARCH LANE OUTCOME"),
            "api's lane answered — no outcome block"
        );
        assert_eq!(
            briefs[0].settled, "1/2 — Port 8850, from the spec's own boot table.",
            "the slice_index settled line carries answered/total and the first answer's head"
        );
        let w = &briefs[1].brief;
        assert!(
            w.contains("RESEARCH LANE OUTCOME for this slice")
                && w.contains(
                    "- no_questions: the lane read its sections and derived no design or \
                     external question"
                ),
            "{w}"
        );
        assert!(
            !w.contains("ANSWERS SETTLED") && !w.contains("QUESTIONS this slice must settle"),
            "{w}"
        );
        assert_eq!(briefs[1].settled, "0/1");
        let plain = briefs_from_slices(&opened, "build the app", &[], &[], &NullSink);
        assert!(
            !plain[0].brief.contains("ANSWERS SETTLED")
                && !plain[0].brief.contains("QUESTIONS this slice must settle")
                && !plain[0].brief.contains("RESEARCH LANE OUTCOME")
                && plain[0].settled.is_empty(),
            "no research rows => none of the three blocks"
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

    /// VA-118 item 2, THE CLASSIFIER, on r6i's own material (archive local-sb7-swarm-r6i-STOPPED-
    /// by-Mihai-research-tail-…-research-107m-501e38a98, `.swarm/ledger/research-<slice>-q<N>.json`):
    /// the six answers the tick-surgeon read against the spec — behavior-q1 and -q11 SPEC_RESTATED
    /// (request.md:144/:405/:408 and :241-249/:392/:472 rewritten as code), viz-q1 and behavior-q6
    /// DESIGN-INTRA (the same builder's keyboard choices), viz-q4 and viz-q7 DESIGN-REAL — all 35 of
    /// the run's entries self-tagged `design` and none named an alternative (the contract had no
    /// field for one). The classifier does NOT reproduce the reader's 2/2/2 split and does not
    /// try: `classify_design_entry`'s doc carries the measurement (0.53/0.61 vs 0.55/0.51 vs
    /// 0.72/0.62 in-section word share — no cut exists). What it reads is whether the entry SHOWS
    /// a choice: shaped as r6i wrote them, all six are `spec_restated` by the classifier, and the
    /// event says so (`source: classifier`, `model_kind: design`); the same viz-q7 with its two
    /// admissible owners named stays `design` by the model, its evidence line carrying the words.
    #[test]
    fn the_classifier_reads_whether_a_design_entry_shows_a_choice_and_the_event_names_who_decided()
    {
        let r6i: [(&str, &str, &str, &str); 6] = [
            ("web-console-behavior",
             "What page size, offset handling, readout formula, sort mapping and DOM selectors does the payments table use?",
             "request.md:405 (showing X–Y of TOTAL), :407–408 (clickable headers, aria-sort), :117 + Endpoints 'Payments' (limit default 50/cap 200, sort vocabulary, total reflects filters), :827 (p95 at limit=50)",
             "app.js holds one view state `view = {limit: 50, offset: 0, status: \"\", currency: \"\", sort: \"created_at\"}` and always fetches `GET /api/payments` with `limit=50` … Prev/Next (`#prev`, `#next`) move offset by 50 … Readout text is exactly `showing ${offset+1}–${offset+data.length} of ${total}` (request.md:405)"),
            ("web-console-behavior",
             "How are ledgerd error envelopes displayed, and what shared fetch helper guarantees a clean console?",
             "request.md:235–252 (single envelope shape, snake_case codes), :392/:423 (non-blocking notice in #notice role=status), :472 (no alert/confirm/prompt)",
             "One shared `api(path, opts)` helper is the ONLY network path in app.js … On HTTP !ok with a parseable envelope `{error: {code, message, field_errors?}}`: set `#notice` (role=\"status\") textContent to EXACTLY `error.message`"),
            ("viz-engine",
             "How is the WebGL scene organized (geometry, instance buffer layout/stride, draw-call stream, dim scheme, context choice)?",
             "request.md:545–567 (Rendering), :689 (brush cost), :710–712 (upload accounting)",
             "DECIDE the minimal stream: ONE static unit-box vertex VBO … ONE interleaved instance VBO, DYNAMIC_DRAW, stride 32 bytes"),
            ("web-console-behavior",
             "How does app.js apply a streamed batch to the table and summary (immediate patch vs re-fetch)?",
             "request.md:695–717 (streaming diffs; batch shape), §7 (status badge hexes), :837 (250 ms apply budget)",
             "patch that row's cells in place immediately … Then schedule ONE debounced (300 ms) `loadPage()` + `/api/summary` refetch"),
            ("viz-engine",
             "What does vs7dbg.layout() (and the other vs7dbg methods) return before the first non-empty /api/viz/records response has been applied?",
             "request.md:501–504, :722, :737",
             "DECIDE: `layout()` returns `null` until the first non-empty response is applied, then returns the frozen `{d0, D0: 96, R0}` for the life of the page"),
            ("viz-engine",
             "Who owns the viz panel states (#viz-empty / #viz-error), and how must viz.js handle canvas resize and WebGL context loss?",
             "request.md:445–447 (§7 States); :549–550 (DPR sizing), :559 (at-rest budget)",
             "DECIDE: (1) viz.js owns visibility of both elements — it alone knows the fetch outcome and scene count"),
        ];
        for (slice, question, cite, answer) in r6i {
            let reply = serde_json::json!({"answers": [
                {"question": question, "kind": "design", "cite": cite, "answer": answer}
            ]})
            .to_string();
            let (rows, strays) = fold_research_lane(slice, "qwen3.8-27b", 3758, Ok(reply));
            assert!(strays.is_empty());
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].kind, "spec_restated",
                "an r6i-shaped design entry names no alternative: {question}"
            );
            assert_eq!(
                rows[0].cite, cite,
                "the lane's cite is kept verbatim (one line)"
            );
            let sink = ValueSink::default();
            emit_research_outcome(&sink, &rows[0]);
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev[0]["event"], "research_question_kind");
            assert_eq!(ev[0]["kind"], "spec_restated");
            assert_eq!(ev[0]["source"], "classifier");
            assert_eq!(ev[0]["model_kind"], "design");
            assert_eq!(ev[1]["event"], "research_answered");
        }
        // viz-q7 as the new contract asks for it: the two admissible owners named, the request's
        // silence stated — a decision, kept as the model's own word.
        let (kind, evidence) = classify_design_entry(
            "design",
            "request.md:445-447 (§7 States)",
            &[
                "viz.js owns #viz-empty / #viz-error".into(),
                "app.js owns them".into(),
                " viz.js owns #viz-empty / #viz-error ".into(),
            ],
            "§7 names both elements and no owner",
        );
        assert_eq!(kind, QuestionKind::Design);
        assert_eq!(kind.source(), "model");
        assert_eq!(
            evidence,
            "request.md:445-447 (§7 States); open because: §7 names both elements and no owner; \
             alternatives: viz.js owns #viz-empty / #viz-error | app.js owns them",
            "the evidence line carries the words the classifier read; a duplicate alternative counts once"
        );
        // One alternative is not a choice; external and unknown kinds pass through untouched.
        assert_eq!(
            classify_design_entry("design", "request.md:1", &["only this".into()], "").0,
            QuestionKind::SpecRestated
        );
        let (kind, evidence) =
            classify_design_entry("external", "docs §Webhooks", &["ignored".into()], "ignored");
        assert_eq!(
            (kind, evidence.as_str()),
            (QuestionKind::External, "docs §Webhooks")
        );
        assert_eq!(
            classify_design_entry("lookup", "", &[], "").0,
            QuestionKind::Unkinded
        );
        assert_eq!(
            QuestionKind::from_stored("spec_restated").source(),
            "classifier"
        );
        assert_eq!(QuestionKind::parse("spec_restated"), QuestionKind::Unkinded);
    }

    /// VA-118 items 1 and 3: the slice lane's prompt makes the pasted sections THE request for the
    /// slice and orders no search of the request file — r6i's structure lane ran 14 `sed`/grep
    /// calls over ranges it already held plus five sweeps for silence proofs, under a prompt that
    /// said "RUN `grep -n -i '<term>'` … AND that grep's 'no match'". A design entry must name its
    /// alternatives; intra-slice choices go to `builder_decides`; a point for another slice has
    /// `raised_for`. The splice writes each section's request.md span under its heading so the
    /// cite is the handed lines. The decisions lane's text is not this change's subject.
    #[test]
    fn the_slice_lane_prompt_makes_the_pasted_sections_the_spec_and_orders_no_search() {
        let mut lane = lane("web-console-structure", "HEAD", "material");
        lane.siblings = "viz-engine — owns web/viz.js".to_string();
        let system = research_system_text(&lane);
        let user = research_user_text("", &lane);
        let instruction = user.split("YOUR WORK, slice").nth(1).unwrap();
        for (name, text) in [("system", system.as_str()), ("instruction", instruction)] {
            assert!(
                !text.to_lowercase().contains("grep") && !text.contains("no match"),
                "{name} orders no search of the request:\n{text}"
            );
            for needle in [
                "alternatives",
                "open_because",
                "builder_decides",
                "spec_restated",
            ] {
                assert!(text.contains(needle), "{name} names `{needle}`:\n{text}");
            }
        }
        assert!(system.contains("the section in your message IS the request for this slice"));
        assert!(instruction.contains("THE SECTION IN HAND above IS the request for this slice"));
        assert!(instruction.contains("do not re-read it from the request file"));
        assert!(
            system.contains("{\"section_done\": true, \"builder_decides\": [\"...\"]}")
                && system.contains("never a silent skip"),
            "VA-128: the system text carries the section_done contract:\n{system}"
        );
        assert!(
            user.contains("goes in `raised_for` with that slice's id"),
            "the siblings block gives a cross-slice point its destination:\n{user}"
        );
        assert!(system.contains("`raised_for` lists points that belong to ANOTHER slice"));
        let schema = research_derived_schema();
        let item = &schema["properties"]["answers"]["items"]["properties"];
        for field in ["alternatives", "open_because", "raised_for"] {
            assert!(item.get(field).is_some(), "schema item carries `{field}`");
        }
        assert_eq!(
            item["raised_for"]["items"]["required"],
            serde_json::json!(["slice", "text"])
        );
        assert!(schema["properties"]["builder_decides"].is_object());
        // VA-128: the tool takes one entry — every item field, the same shape — widened by the
        // section signal, and nothing required at the tool level so a bare section_done call
        // is valid under schema-constrained decoding.
        let tool = research_answer_tool_schema();
        for (field, shape) in schema["properties"]["answers"]["items"]["properties"]
            .as_object()
            .unwrap()
        {
            assert_eq!(
                &tool["properties"][field], shape,
                "the tool's `{field}` is the final reply's item field"
            );
        }
        assert_eq!(
            tool["properties"]["section_done"],
            serde_json::json!({"type": "boolean"})
        );
        assert!(tool["properties"]["builder_decides"].is_object());
        assert_eq!(tool["required"], serde_json::json!([]));
        assert_eq!(RESEARCH_ANSWER_TOOL, "research_answer");
        assert!(
            system.contains(RESEARCH_ANSWER_TOOL),
            "the prompt names the per-answer tool the lane is registered with (wired r6j)"
        );
        assert!(
            system.contains("THE MOMENT ONE QUESTION IS SETTLED, call the research_answer tool")
        );
        assert!(system.contains("only the entries you did NOT land through research_answer"));
        let spec = "# Alpha\nalpha body text\n\n# Beta\nbeta body text\n";
        let spliced = splice_claimed_sections(
            "boot",
            &["Beta".to_string()],
            &spec_sections(spec),
            &NullSink,
        );
        assert_eq!(spliced, "\n### Beta\n[request.md:4-5]\nbeta body text");
    }

    /// VA-118 items 3, 4 and 5 at the fold: one entry landed through the per-answer door
    /// (`ResearchToolCall::into_row`) is byte-for-byte the row the final-reply fold builds at that
    /// position (`fold_research_lane_from`), it round-trips its mini (a `raised_for` point keeps
    /// its destination label), and the outcome funnel names each raised line by destination —
    /// `research_raised_for{from, to, text}`, `research_builder_decides{text}`,
    /// `research_raised_folded` — while a lane that derived nothing but listed builder decisions
    /// is still `no_questions`, with the list carried and counted.
    #[test]
    fn a_per_answer_entry_lands_the_same_row_as_the_lane_fold_and_round_trips_its_mini() {
        let entry = serde_json::json!({
            "question": "Which element carries the filter's data-value?",
            "kind": "design",
            "cite": "request.md:411-414",
            "alternatives": ["the wrapper div", "the trigger button"],
            "open_because": "L414 says the grader reads data-value and names no element",
            "answer": "The wrapper <div class=\"filter-dd\" id=\"status-filter\"> carries it.",
            "raised": ["label text for the off option"],
            "raised_for": [
                {"slice": "web-console-behavior", "text": "set data-value on the div only"},
                {"slice": "", "text": "a point with no destination stays with this builder"},
                {"slice": "viz-engine", "text": "   "}
            ]
        });
        let row = ResearchToolCall::parse(&entry.to_string())
            .unwrap()
            .into_row("web-console-structure", 3, "m", 120, None)
            .unwrap()
            .expect("an entry with a question lands a row");
        let (mut rows, strays) = fold_research_lane_from(
            "web-console-structure",
            "m",
            120,
            Ok(serde_json::json!({"answers": [entry]}).to_string()),
            3,
        );
        assert!(strays.is_empty());
        rows[0].batch = 0;
        assert_eq!(format!("{row:?}"), format!("{:?}", rows[0]));
        assert_eq!((row.q_index, row.kind.as_str()), (3, "design"));
        assert_eq!(
            row.raised,
            vec![
                "label text for the off option".to_string(),
                "[for web-console-behavior] set data-value on the div only".to_string(),
                "a point with no destination stays with this builder".to_string(),
            ],
            "a blank text is dropped, a blank destination keeps the point for this builder"
        );
        let dir = tempfile::tempdir().unwrap();
        write_research_ledger(dir.path(), &row).unwrap();
        let back = load_research_mini(dir.path(), "web-console-structure", 3).unwrap();
        assert_eq!(format!("{back:?}"), format!("{row:?}"));
        assert!(back
            .cite
            .ends_with("alternatives: the wrapper div | the trigger button"));
        let sink = ValueSink::default();
        emit_research_outcome(&sink, &back);
        let ev = sink.0.lock().unwrap();
        let names: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "research_question_kind",
                "research_answered",
                "research_raised_folded",
                "research_raised_for",
                "research_raised_folded",
            ]
        );
        assert_eq!(ev[0]["source"], "model");
        assert!(ev[0]["model_kind"].is_null());
        assert_eq!(ev[3]["from"], "web-console-structure");
        assert_eq!(ev[3]["to"], "web-console-behavior");
        assert_eq!(ev[3]["text"], "set data-value on the div only");
        assert_eq!(ev[3]["raised_by"], "research-web-console-structure-q3.json");
        assert_eq!(
            ev[4]["question"],
            "a point with no destination stays with this builder"
        );
        drop(ev);
        assert!(ResearchToolCall::parse("not json").is_none());
        assert!(
            ResearchToolCall::parse(r#"{"answer": "an answer with no question"}"#)
                .unwrap()
                .into_row("s", 0, "m", 1, None)
                .is_err(),
            "an answer without a question is a stray, never a row"
        );
        // builder_decides with no questions: still no_questions, the list carried and counted.
        let (rows, strays) = fold_research_lane(
            "viz-engine",
            "m",
            300,
            Ok(serde_json::json!({
                "answers": [],
                "builder_decides": ["instance VBO layout and stride", " ", "debounce interval", "instance VBO layout and stride"]
            })
            .to_string()),
        );
        assert!(strays.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reason.as_deref(), Some("no_questions"));
        assert!(
            rows[0].detail.as_deref().unwrap().ends_with(
                "it listed 2 choice(s) only this slice's builder makes (builder_decides)"
            ),
            "{:?}",
            rows[0].detail
        );
        assert_eq!(
            rows[0].raised,
            vec![
                "[builder decides] instance VBO layout and stride".to_string(),
                "[builder decides] debounce interval".to_string()
            ]
        );
        let sink = ValueSink::default();
        emit_research_outcome(&sink, &rows[0]);
        let ev = sink.0.lock().unwrap();
        let names: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "research_unanswered",
                "research_builder_decides",
                "research_builder_decides"
            ]
        );
        assert_eq!(ev[1]["text"], "instance VBO layout and stride");
        let block = raised_questions_brief_block(&[&rows[0]]);
        assert!(
            block.contains("- [builder decides] debounce interval"),
            "{block}"
        );
        // With questions, the list rides on the FIRST row only.
        let (rows, _) = fold_research_lane(
            "viz-engine",
            "m",
            300,
            Ok(serde_json::json!({
                "answers": [
                    {"question": "q0", "kind": "external", "cite": "docs §1", "answer": "a0"},
                    {"question": "q1", "kind": "external", "cite": "docs §2", "answer": "a1"}
                ],
                "builder_decides": ["debounce interval"]
            })
            .to_string()),
        );
        assert_eq!(
            rows[0].raised,
            vec!["[builder decides] debounce interval".to_string()]
        );
        assert!(rows[1].raised.is_empty());
        assert_eq!(
            raised_destination("[for viz-engine] who toggles #viz-empty"),
            RaisedDestination::OtherSlice {
                slice: "viz-engine",
                text: "who toggles #viz-empty"
            }
        );
        assert_eq!(
            raised_destination("[for ] malformed label"),
            RaisedDestination::ThisBuilder("[for ] malformed label"),
            "a label with no closing bracket is a plain raised line, never a lost point"
        );
    }

    /// VA-089's terminal fold for a slice lane whose questions are its OWN: every outcome — the
    /// lane's derived Q/A entries (kind and cite kept, position = q_index), a blank answer, an
    /// entry with no question (a stray, named), an unknown kind (kept as `unkinded`), a reply
    /// with ZERO entries (one lane-outcome row, reason no_questions — the resume watermark),
    /// nothing parseable, a transport failure and the judge_out_of_moves ending — lands as at
    /// least one TERMINAL row for the slice, which is what makes "every slice lane terminal"
    /// reachable with no clock. A miss is a loud named absence, never a substituted answer.
    #[test]
    fn a_slice_lanes_derived_answers_fold_to_rows_and_every_outcome_is_terminal() {
        // VA-118 re-pin: the first entry names two alternatives, so it stays `design` under the
        // classifier (a design entry naming fewer than two is recorded spec_restated — pinned in
        // the classifier's own test); its cite keeps the lane's words and gains the alternatives.
        let reply = serde_json::json!({"answers": [
            {"question": "Which journal mode for notify.db?", "kind": "design",
             "cite": "request.md:77 'SQLite'",
             "alternatives": ["WAL", "rollback journal (DELETE)"],
             "answer": "WAL — one writer, readers never block.", "raised": ["single writer?"]},
            {"question": "Which header carries the vendor's signature?", "kind": "external",
             "cite": "docs §Webhooks → Signing", "answer": "  "},
            {"question": "", "kind": "design", "answer": "an answer with no question"},
            {"question": "How are DST days bucketed?", "kind": "lookup",
             "answer": "By the Berlin instant, per the section."},
        ]})
        .to_string();
        let (rows, strays) = fold_research_lane("notifierd", "m", 900, Ok(reply));
        assert_eq!(
            rows.len(),
            3,
            "one row per entry with a question, in reply order"
        );
        assert_eq!(
            (
                rows[0].q_index,
                rows[0].status.as_str(),
                rows[0].kind.as_str()
            ),
            (0, RESEARCH_ANSWERED, "design")
        );
        assert!(rows[0].cite.starts_with("request.md:77"));
        assert_eq!(rows[0].raised, vec!["single writer?".to_string()]);
        assert_eq!(
            (
                rows[1].q_index,
                rows[1].status.as_str(),
                rows[1].reason.as_deref(),
                rows[1].kind.as_str()
            ),
            (1, RESEARCH_UNANSWERED, Some("empty_answer"), "external"),
            "a blank answer is a named absence, never a stub"
        );
        assert_eq!(
            (rows[2].q_index, rows[2].kind.as_str()),
            (3, "unkinded"),
            "the stray keeps its position out of the numbering; an unknown kind is kept and named"
        );
        assert!(rows
            .iter()
            .all(|r| r.slice == "notifierd" && r.model == "m" && r.secs == 900 && r.batch == 3));
        assert_eq!(strays.len(), 1);
        assert_eq!(strays[0].question_index, Some(2));
        assert!(strays[0]
            .answer_head
            .starts_with("an answer with no question"));
        // ZERO entries: the lane says its sections settle everything — one outcome row holds it.
        let (rows, strays) =
            fold_research_lane("notifierd", "m", 300, Ok(r#"{"answers": []}"#.into()));
        assert_eq!(rows.len(), 1);
        assert!(strays.is_empty());
        assert!(
            rows[0].question.is_empty()
                && rows[0].status == RESEARCH_UNANSWERED
                && rows[0].reason.as_deref() == Some("no_questions")
                && rows[0].q_index == 0
                && rows[0].secs == 300,
            "{:?}",
            rows[0]
        );
        // Nothing parseable: the raw head rides in detail.
        let (rows, _) = fold_research_lane("notifierd", "m", 5, Ok("I could not decide.".into()));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].reason.as_deref(), rows[0].detail.as_deref()),
            (Some("empty_answer"), Some("I could not decide."))
        );
        // The engine's own ending and a transport error, named apart.
        let (rows, _) = fold_research_lane(
            "notifierd",
            "m",
            5,
            Err(format!("{JUDGE_ENDED_NEEDLE}: out of moves")),
        );
        assert_eq!(rows[0].reason.as_deref(), Some("judge_ended"));
        let (rows, _) = fold_research_lane("notifierd", "m", 5, Err("connection reset".into()));
        assert_eq!(
            (rows[0].reason.as_deref(), rows[0].detail.as_deref()),
            (Some("provider_error"), Some("connection reset"))
        );
        // The outcome funnel: an outcome row (no question) names no kind — only its outcome.
        let sink = ValueSink::default();
        emit_research_outcome(&sink, &rows[0]);
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["event"], "research_unanswered");
        assert_eq!(ev[0]["reason"], "provider_error");
    }

    /// r6c's five slices with the sections they claimed AND their objectives, both verbatim from
    /// the run's `plan_loaded.tasks[i].description` (archive
    /// local-sb7-swarm-r6c-FINISHED-0.1420-…-build-608m/run.jsonl): the `### ` headings the
    /// splice wrote — a perfect 28-section partition, 0 overlaps — and the description's FIRST
    /// PARAGRAPH, which is `sl.objective` exactly as `briefs_from_slices` opens each brief with
    /// it (2,501 / 2,719 / 1,050 / 1,528 / 2,591 chars; `include_str!` from
    /// tests/fixtures/r6c-objective-<slice>.txt so no line is reflowed). VA-045: until then this
    /// fixture carried 43-190-char PARAPHRASES of the objectives, and since 080c430cf the
    /// objective is part of rule (a)/(d)'s vocabulary, so every routing trace on it ran on inputs
    /// r6c never had — the real objectives name `GET /api/payments`, `POST /notify/events`,
    /// `/api/viz/records`, `/api/stream` and `/api/notifications`. The objectives declared no
    /// backticked files (`slice_files_unnamed` fired five times in r6c), so `files_from_objective`
    /// still yields nothing and the file half of the vocabulary stays empty here as it did there.
    fn r6c_slices() -> OpenOutput {
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
                    include_str!("../../../tests/fixtures/r6c-objective-ledgerd-core.txt"),
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
                    include_str!("../../../tests/fixtures/r6c-objective-ledgerd-api.txt"),
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
                    include_str!("../../../tests/fixtures/r6c-objective-notifierd.txt"),
                    &["6. `notifierd` — the idempotent consumer"],
                ),
                slice(
                    "web-console",
                    include_str!("../../../tests/fixtures/r6c-objective-web-console.txt"),
                    &[
                        "7. `web/` — the frontend",
                        "9. `DECISIONS.md` — three corners you must decide",
                    ],
                ),
                slice(
                    "web-viz",
                    include_str!("../../../tests/fixtures/r6c-objective-web-viz.txt"),
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
    ///
    /// VA-032, one step over: §7 describes the drafts panel (`#draft-form`, `#approve-btn`,
    /// request.md:432-437) and never writes `/api/drafts`, so §5's five drafts rows did not
    /// route to web-console under (a). Rule (d) reads the resource WORD: web-console now gets
    /// §5 (its approve/reject/submit paths and the `GET /api/drafts?state=` row), still gets
    /// Endpoints under (a), and ledgerd-core receives no frontend section — §7, §8's children
    /// and §9 advertise no route, so no rule can carry them.
    ///
    /// VA-045: the fixture's objectives are now r6c's REAL first paragraphs (the brief opens
    /// with `sl.objective`, and the objective is in rule (a)/(d)'s vocabulary since 080c430cf).
    /// Measured against the paraphrased fixture: every slice receives the SAME sections
    /// (16/11/7/9/14), but notifierd's and web-viz's `Endpoints` move from rule (d) — the words
    /// `webhook`/`draft` and `stream`/`viz` — to rule (a): notifierd's objective says
    /// "ledgerd's /api/notifications proxy", web-viz's "Consumes /api/viz/records and SSE
    /// /api/stream". The objective-LESS view, what `research_request_block` hands today (its
    /// swarm.rs caller passes no objective; 2c S9), still routes both under (d) — asserted
    /// below on the same real bodies so the VA-044 F2 filters stay measured on r6c's words.
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
        const S5: &str = "\n### 5. The approval workflow — maker, checker, admin\n";
        assert_eq!(console.matches(S5).count(), 1, "{console}");
        assert!(
            console.contains("| `POST` | `/api/drafts/<id>/approve` | checker |")
                && console.contains("| `GET` | `/api/drafts?state=` | any role |"),
            "the drafts rows reach the slice that builds #approve-btn: {console}"
        );
        let viz = by_id("web-viz");
        assert!(viz.contains("\n### Data → scene\n"), "{viz}");
        assert!(
            viz.contains("\n### Streaming diffs — SSE with byte accounting\n"),
            "{viz}"
        );
        let core = by_id("ledgerd-core");
        for frontend in [
            "\n### 7. `web/` — the frontend\n",
            "\n### 8. The 3D field — 12,288 instances, five mechanisms\n",
            "\n### Rendering — bounded draw calls, demand rendering\n",
            "\n### The pick buffer\n",
            "\n### The linked brush — table ⇄ instances\n",
            "\n### `vs7dbg` — REQUIRED and graded\n",
            "\n### 9. `DECISIONS.md` — three corners you must decide\n",
        ] {
            assert!(
                !core.contains(frontend),
                "ledgerd-core receives no frontend section: {frontend}"
            );
        }
        for b in &briefs {
            assert!(
                b.brief.contains("\n### Performance budgets\n"),
                "{}: every slice is graded on the budgets",
                b.id
            );
            for heading in [
                "Endpoints",
                "5. The approval workflow — maker, checker, admin",
                "6. `notifierd` — the idempotent consumer",
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
        const S5_HEADING: &str = "5. The approval workflow — maker, checker, admin";
        const S6_HEADING: &str = "6. `notifierd` — the idempotent consumer";
        assert_eq!(
            rule_for("web-console", "advertised_route"),
            vec!["Endpoints".to_string()]
        );
        assert!(
            rule_for("web-console", "resource_token").contains(&S5_HEADING.to_string()),
            "§5 reaches web-console under rule (d), by the WORD `draft`: {:?}",
            rule_for("web-console", "resource_token")
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
        // VA-044 F2, the filters on r6c's real claims. §6 reached web-viz by "events" alone —
        // §8's "the canvas consumes its wheel events" — and every one of the five claimed bodies
        // names event/events, so the resource distinguishes nobody (filter i) and §6 no longer
        // arrives. Endpoints arrives under rule (a): web-viz's real objective names
        // `/api/viz/records` and `/api/stream` (VA-045; with the paraphrased objective it came
        // under (d) by the words `stream` and `viz`), so no resource filter is consulted for it.
        assert_eq!(
            rule_for("web-viz", "advertised_route"),
            vec!["Endpoints".to_string()]
        );
        assert!(
            rule_for("web-viz", "resource_token").is_empty(),
            "{:?}",
            rule_for("web-viz", "resource_token")
        );
        let filtered = |slice: &str, section: &str| -> Option<&serde_json::Value> {
            ev.iter().find(|e| {
                e["event"] == "resource_token_filtered"
                    && e["slice"] == slice
                    && e["section"] == section
            })
        };
        let words = |v: &serde_json::Value, key: &str| -> Vec<String> {
            v[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w.as_str().unwrap().to_string())
                .collect()
        };
        let viz6 = filtered("web-viz", S6_HEADING).expect("§6's words were filtered for web-viz");
        assert_eq!(words(viz6, "ubiquitous"), vec!["events"]);
        assert!(words(viz6, "carried").is_empty(), "{viz6}");
        // notifierd: Endpoints arrives under rule (a) — its real objective says "ledgerd's
        // /api/notifications proxy (owned by the api slice) forwards to this service" (VA-045),
        // so rule (d) and its filters are never consulted for it in the brief; §5 still comes
        // under (d) by `draft` (§6's `draft.submitted`, `draft.approved`, `draft.rejected`).
        assert_eq!(
            rule_for("notifierd", "advertised_route"),
            vec!["Endpoints".to_string()]
        );
        assert!(
            filtered("notifierd", "Endpoints").is_none(),
            "rule (a) fired first; no filter event for a section it carried"
        );
        assert_eq!(
            rule_for("notifierd", "resource_token"),
            vec![S5_HEADING.to_string()]
        );
        // web-console: §6 still arrives on "notifications" — four of five bodies say it (web-viz
        // does not), so it distinguishes; "event" (the feed's `data-event-seq`) is ubiquitous.
        let c6 = filtered("web-console", S6_HEADING).expect("web-console/§6 filtered words");
        assert_eq!(words(c6, "ubiquitous"), vec!["event"]);
        assert_eq!(words(c6, "carried"), vec!["notifications"]);
        assert!(rule_for("web-console", "resource_token").contains(&S6_HEADING.to_string()));
        // ledgerd-api owns Endpoints, so §6's health/notifications/events are its own routes'
        // words; §6 still arrives on "processed" (§4's "an event id already processed").
        let a6 = filtered("ledgerd-api", S6_HEADING).expect("ledgerd-api/§6 filtered words");
        assert_eq!(
            words(a6, "own"),
            vec!["event", "events", "health", "notification", "notifications"]
        );
        assert_eq!(words(a6, "carried"), vec!["processed"]);
        // Rule (a) stays token-bounded: notifierd's `/health` row is not found inside ledgerd's
        // `/api/health`. §6 reaches ledgerd-api by the WORDS its Endpoints body uses for
        // notifierd's resources (`/api/notifications` is "proxied to notifierd") — rule (d),
        // named as such.
        assert!(!rule_for("ledgerd-api", "advertised_route").contains(&S6_HEADING.to_string()));
        assert!(rule_for("ledgerd-api", "resource_token").contains(&S6_HEADING.to_string()));

        // THE OBJECTIVE-LESS VIEW — the inputs `research_request_block` hands this helper today
        // (its swarm.rs caller passes `files_from_objective(&sl.objective)`, never the
        // objective; 2c S9): the same real bodies, no objective. The same sections arrive, and
        // the two `Endpoints` the brief carries under (a) come under (d) here — the VA-044 F2
        // filters measured on r6c's own words: notifierd's "health", "notification(s)" and
        // "event(s)" are its OWN resources (§6's routes), filtered under (ii); "payment" is in
        // every body (i); Endpoints arrives on "webhook" ("like ledgerd's webhook counters")
        // and on "draft". web-viz's Endpoints arrives on `stream` and `viz`; "events" and
        // "payment" are ubiquitous. Both survive their filters: MILD, route.
        let every_claim: Vec<&[String]> = opened
            .slices
            .iter()
            .map(|sl| sl.sections.as_slice())
            .collect();
        let objectiveless = ValueSink::default();
        for sl in &opened.slices {
            consumed_spec_sections(
                &sl.id,
                &sl.sections,
                &[],
                "",
                &every_claim,
                &sections,
                &objectiveless,
            );
        }
        let no_objective = objectiveless.0.lock().unwrap();
        let no_objective_rule_for = |slice: &str, rule: &str| -> Vec<String> {
            no_objective
                .iter()
                .filter(|e| {
                    e["event"] == "spec_sections_consumed"
                        && e["slice"] == slice
                        && e["rule"] == rule
                })
                .flat_map(|e| e["sections"].as_array().unwrap().iter())
                .map(|h| h.as_str().unwrap().to_string())
                .collect()
        };
        let no_objective_filtered = |slice: &str, section: &str| -> Option<&serde_json::Value> {
            no_objective.iter().find(|e| {
                e["event"] == "resource_token_filtered"
                    && e["slice"] == slice
                    && e["section"] == section
            })
        };
        assert!(no_objective_rule_for("notifierd", "advertised_route").is_empty());
        assert_eq!(
            no_objective_rule_for("notifierd", "resource_token"),
            vec!["Endpoints".to_string(), S5_HEADING.to_string()]
        );
        let nd = no_objective_filtered("notifierd", "Endpoints")
            .expect("notifierd's own words filtered");
        assert_eq!(
            words(nd, "own"),
            vec!["event", "events", "health", "notification", "notifications"]
        );
        assert_eq!(words(nd, "ubiquitous"), vec!["payment"]);
        assert_eq!(words(nd, "carried"), vec!["draft", "webhook"]);
        assert!(no_objective_rule_for("web-viz", "advertised_route").is_empty());
        assert_eq!(
            no_objective_rule_for("web-viz", "resource_token"),
            vec!["Endpoints".to_string()]
        );
        let vz = no_objective_filtered("web-viz", "Endpoints")
            .expect("web-viz/Endpoints filtered words");
        assert_eq!(words(vz, "ubiquitous"), vec!["events", "payment"]);
        assert_eq!(words(vz, "carried"), vec!["stream", "viz"]);
        // The other three slices route identically with or without their objective.
        assert_eq!(
            no_objective_rule_for("web-console", "advertised_route"),
            vec!["Endpoints".to_string()]
        );
        assert_eq!(
            no_objective_rule_for("web-console", "resource_token"),
            vec![S5_HEADING.to_string(), S6_HEADING.to_string()]
        );
        assert_eq!(
            no_objective_rule_for("ledgerd-api", "resource_token"),
            vec![S6_HEADING.to_string()]
        );
        assert_eq!(
            no_objective_rule_for("ledgerd-core", "advertised_route"),
            vec!["Endpoints".to_string(), S6_HEADING.to_string()]
        );
        for sl in &opened.slices {
            eprintln!(
                "r6c-objectiveless {:13} a {:?} | d {:?}",
                sl.id,
                no_objective_rule_for(&sl.id, "advertised_route"),
                no_objective_rule_for(&sl.id, "resource_token")
            );
        }
        drop(no_objective);

        for sl in &opened.slices {
            for rule in [
                "advertised_route",
                "resource_token",
                "child_of_claimed",
                "cross_cutting",
            ] {
                for h in rule_for(&sl.id, rule) {
                    assert!(
                        !sl.sections
                            .iter()
                            .any(|c| heading_key(c) == heading_key(&h)),
                        "{}: {rule} never re-routes an owned section: {h}",
                        sl.id
                    );
                }
            }
        }

        // The gate-8 table: own / per-rule / cross-cutting sections and chars per r6c slice.
        let chars_of = |headings: &[String]| -> usize {
            headings
                .iter()
                .map(|h| {
                    let sec = sections
                        .iter()
                        .find(|s| heading_key(&s.heading) == heading_key(h))
                        .unwrap();
                    format!("\n### {}\n{}", sec.heading, sec.body.trim())
                        .chars()
                        .count()
                })
                .sum()
        };
        for sl in &opened.slices {
            let own = splice_claimed_sections(&sl.id, &sl.sections, &sections, &NullSink);
            let a = rule_for(&sl.id, "advertised_route");
            let d = rule_for(&sl.id, "resource_token");
            let b = rule_for(&sl.id, "child_of_claimed");
            let c = rule_for(&sl.id, "cross_cutting");
            for f in ev
                .iter()
                .filter(|e| e["event"] == "resource_token_filtered" && e["slice"] == sl.id)
            {
                eprintln!(
                    "r6c-filtered {:13} {:50} own {} ubiquitous {} carried {}",
                    sl.id,
                    f["section"].as_str().unwrap(),
                    f["own"],
                    f["ubiquitous"],
                    f["carried"]
                );
            }
            eprintln!(
                "r6c {:13} own {:2}/{:6} | a {}/{:5} | d {}/{:5} {:?} | b {}/{:5} | c {}/{:5} | after {}/{}",
                sl.id,
                sl.sections.len(),
                own.chars().count(),
                a.len(),
                chars_of(&a),
                d.len(),
                chars_of(&d),
                d,
                b.len(),
                chars_of(&b),
                c.len(),
                chars_of(&c),
                sl.sections.len() + a.len() + d.len() + b.len() + c.len(),
                own.chars().count() + chars_of(&a) + chars_of(&d) + chars_of(&b) + chars_of(&c)
            );
        }
    }

    /// The four routing rules on a small document, each edge named: a claimed TOP-LEVEL
    /// grouping inherits no children (its children are other slices' components); a section
    /// two slices claim is not cross-cutting; a flat document (only top-level sections)
    /// broadcasts nothing; the route match is token-bounded and the resource-word match is
    /// what carries a section the path match refused; the research prompt's one-claimant
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
        let cx = consumed_spec_sections("x", &x, &[], "", &every, &sections, &sink);
        assert!(
            cx.called_into.contains("\n### Y\n"),
            "rule a: X's body names /api/y"
        );
        assert!(
            cx.called_into.contains("\n### Z\n"),
            "`/health` in X's body is not the path `/api/health` (rule a refuses it) but it is \
             the WORD `health`, Z's resource (rule d carries it): {}",
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
        let cb = consumed_spec_sections("b", &build, &[], "", &every, &sections, &NullSink);
        assert!(
            cb.called_into.is_empty(),
            "a claimed top-level grouping inherits none of its components: {}",
            cb.called_into
        );
        let cy = consumed_spec_sections("y", &y, &[], "", &every, &sections, &NullSink);
        assert!(cy.called_into.is_empty(), "{}", cy.called_into);
        assert_eq!(cy.cross_cutting.matches("\n### ").count(), 2);
        // The objective is vocabulary too (a slice that claimed nothing has only it and its
        // files): "the health probe" routes Z to a claimless slice by the word.
        let none: Vec<String> = Vec::new();
        let co = consumed_spec_sections(
            "o",
            &none,
            &[],
            "Own probe.py — polls the health probe.",
            &every,
            &sections,
            &NullSink,
        );
        assert!(
            co.called_into.contains("\n### Z\n") && !co.called_into.contains("\n### Y\n"),
            "{}",
            co.called_into
        );
        let ev = sink.0.lock().unwrap();
        let by_rule: Vec<(&str, Vec<&str>)> = ev
            .iter()
            .filter(|e| e["event"] == "spec_sections_consumed")
            .map(|e| {
                (
                    e["rule"].as_str().unwrap(),
                    e["sections"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|h| h.as_str().unwrap())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            by_rule,
            vec![
                ("advertised_route", vec!["Y"]),
                ("resource_token", vec!["Z"]),
                ("child_of_claimed", vec!["X1", "X2"]),
                ("cross_cutting", vec!["Rules", "Budgets"]),
            ]
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
        let cf = consumed_spec_sections("a", &a, &[], "", &[&a], &flat, &NullSink);
        assert!(cf.called_into.is_empty() && cf.cross_cutting.is_empty());
    }

    /// VA-044 F2's two filters on a small document, each edge named. Four slices, four bodies:
    /// every body says `items`, so the `items` resource distinguishes nobody and carries nothing
    /// (i); slice `a` owns A, whose routes are `items` and `stats`, so B's `/svc/items/<id>` and
    /// `/svc/stats` rows are a's own resources and not a call into B (ii) — B is mounted under
    /// its own prefix because two sections advertising the SAME base path already carry each
    /// other under rule (a): a slice's own table names its own paths; `stats` and `health` in
    /// C's body still carry A and B to `c` (a word that survives both filters routes); with ONE
    /// claiming slice there is nobody to be told apart from and ubiquity is off. Every removed
    /// word rides `resource_token_filtered`.
    #[test]
    fn resource_token_filters_are_derived_from_the_plan_itself() {
        let doc = "# T\n\n## Build\n\n### A\n\n| Method | Path | Response |\n|---|---|---|\n\
                   | `GET` | `/api/items` | `{\"items\": 1}` |\n| `GET` | `/api/stats` | `{\"n\": 1}` |\n\n\
                   A serves items and stats.\n\n### B\n\n| Method | Path | Response |\n|---|---|---|\n\
                   | `GET` | `/svc/items/<id>` | `{\"item\": 1}` |\n| `GET` | `/svc/stats` | `{\"n\": 1}` |\n\
                   | `GET` | `/svc/health` | `{\"ok\": 1}` |\n\nB: every item has stats; health is here.\n\n\
                   ### C\nC renders items. Its stats panel polls health.\n\n### D\nD logs items.\n";
        let sections = spec_sections(doc);
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (a, b, c, d) = (s(&["A"]), s(&["B"]), s(&["C"]), s(&["D"]));
        let every: Vec<&[String]> = vec![&a, &b, &c, &d];
        let route = |id: &str, claims: &[String], every: &[&[String]]| {
            let sink = ValueSink::default();
            let out = consumed_spec_sections(id, claims, &[], "", every, &sections, &sink);
            let ev = sink.0.lock().unwrap().clone();
            (out.called_into, ev)
        };
        let filtered_words = |ev: &[serde_json::Value], section: &str, key: &str| -> Vec<String> {
            ev.iter()
                .filter(|e| e["event"] == "resource_token_filtered" && e["section"] == section)
                .flat_map(|e| e[key].as_array().unwrap().clone())
                .map(|w| w.as_str().unwrap().to_string())
                .collect()
        };
        // (ii): a owns items and stats; B advertises both — its own resources, not a call.
        let (called_a, ev_a) = route("a", &a, &every);
        assert!(called_a.is_empty(), "{called_a}");
        assert_eq!(filtered_words(&ev_a, "B", "own"), vec!["items", "stats"]);
        assert!(filtered_words(&ev_a, "B", "carried").is_empty());
        // (i): `items` is in all four bodies; d says nothing else — nothing arrives.
        let (called_d, ev_d) = route("d", &d, &every);
        assert!(called_d.is_empty(), "{called_d}");
        assert_eq!(filtered_words(&ev_d, "A", "ubiquitous"), vec!["items"]);
        assert_eq!(filtered_words(&ev_d, "B", "ubiquitous"), vec!["items"]);
        // A word that survives both filters routes: c's `stats` carries A, `stats`+`health` carry B.
        let (called_c, ev_c) = route("c", &c, &every);
        assert!(
            called_c.contains("\n### A\n") && called_c.contains("\n### B\n"),
            "{called_c}"
        );
        assert_eq!(filtered_words(&ev_c, "A", "ubiquitous"), vec!["items"]);
        assert_eq!(filtered_words(&ev_c, "A", "carried"), vec!["stats"]);
        assert_eq!(
            filtered_words(&ev_c, "B", "carried"),
            vec!["health", "stats"]
        );
        // Fewer than two claiming slices: ubiquity is off, `items` carries A to d.
        let (called_d1, ev_d1) = route("d", &d, &[d.as_slice()]);
        assert!(called_d1.contains("\n### A\n"), "{called_d1}");
        assert!(
            !ev_d1
                .iter()
                .any(|e| e["event"] == "resource_token_filtered"),
            "{ev_d1:?}"
        );
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
        // This REPLACES those two slices' real objectives (VA-045): `slice_vocabulary` reads an
        // objective's backticked files only, never its text, so the decision routing below is
        // otherwise r6c's; the section routing of the same call is asserted in the test above.
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
        // both reach the slice whose FILE they name (VA-089 retired the routed-question door).
        let mut routed_opened = r6c_slices();
        routed_opened.slices[2].objective =
            "Own `app/notifierd.py` and `app/notify_store.py`.".into();
        let mixed = vec![
            PlanDecision {
                q_index: 0,
                question: "Logging format for app/notifierd.py — options: plain | json".into(),
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
            "the user's decision names notifierd's file, so it is notifierd's alone"
        );
    }

    /// r6j wiring: a lane that landed rows through `research_answer` folds only its REMAINDER —
    /// an empty final reply behind landed rows is no row (q0's mini stays what the tool wrote),
    /// a builder_decides list rides one `remainder_empty` row at the next index, and an outcome
    /// row (Err / unparseable / all-stray) sits at the next index instead of overwriting q0.
    #[test]
    fn the_remainder_fold_never_overwrites_a_landed_mini() {
        let (rows, strays) = fold_research_lane_from(
            "web-console-structure",
            "m",
            3_780,
            Ok(serde_json::json!({"answers": []}).to_string()),
            9,
        );
        assert!(rows.is_empty() && strays.is_empty());
        let (rows, _) = fold_research_lane_from(
            "web-console-structure",
            "m",
            3_780,
            Ok(serde_json::json!({"answers": [], "builder_decides": ["row height"]}).to_string()),
            9,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].q_index, rows[0].reason.as_deref()),
            (9, Some("remainder_empty"))
        );
        assert!(rows[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("9 question(s) landed through research_answer"));
        assert_eq!(
            rows[0].raised,
            vec![format!("{BUILDER_DECIDES_PREFIX}row height")]
        );
        let (rows, _) = fold_research_lane_from("s", "m", 10, Err("provider down".to_string()), 2);
        assert_eq!(
            (rows[0].q_index, rows[0].reason.as_deref()),
            (2, Some("provider_error"))
        );
        let (rows, _) = fold_research_lane_from("s", "m", 10, Ok("not json".to_string()), 2);
        assert_eq!(
            (rows[0].q_index, rows[0].reason.as_deref()),
            (2, Some("empty_answer"))
        );
        let (rows, strays) = fold_research_lane_from(
            "s",
            "m",
            10,
            Ok(
                serde_json::json!({"answers": [{"question": "", "kind": "design", "answer": "x"}]})
                    .to_string(),
            ),
            2,
        );
        assert_eq!(strays.len(), 1);
        assert_eq!(
            (rows[0].q_index, rows[0].reason.as_deref()),
            (2, Some("empty_answer"))
        );
        // Unchanged at offset 0: the no_questions outcome row is q0, as before the wiring.
        let (rows, _) =
            fold_research_lane_from("s", "m", 10, Ok(r#"{"answers": []}"#.to_string()), 0);
        assert_eq!(
            (rows[0].q_index, rows[0].reason.as_deref()),
            (0, Some("no_questions"))
        );
    }
}
