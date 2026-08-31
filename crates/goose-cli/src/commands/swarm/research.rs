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

use super::decisions::DECISION_SLICE;
use super::{activity_digest_key, head_to_sentence_end, one_lane_per_host, parse_json_lenient};
use super::{phase_banner, spec_orientation, spec_vendor, write_forming_atomic};
use super::{EventSink, SpecSection};
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
    }
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

/// The whole user text of one research call: the per-slice head, the snowball block (empty on
/// a first dispatch — no heading, no filler), then the question VERBATIM under the label its
/// kind carries — a slice question or an open decision the user left unanswered (the decision
/// head, `decisions::decision_user_text`, frames the same tail).
pub(super) fn research_user_text(head: &str, prior_block: &str, q: &ResearchQuestion) -> String {
    let label = if q.slice == DECISION_SLICE {
        "THE OPEN DECISION"
    } else {
        "THE QUESTION"
    };
    format!("{head}{prior_block}\n\n{label}:\n{}", q.question)
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

/// The already-answered minis a dispatching lane should see (fix B, the snowball inside the
/// fan): every ANSWERED row of its own slice (r6c: ledgerd-core q0 and q1 contradicted each
/// other on cursor persistence — "in-memory per walk" vs "never held only in memory" — because
/// neither could see the other), plus an answered row of ANOTHER slice when the two QUESTIONS
/// name the same path (r6c: ledgerd-api-q0's question named `/api/health` and its answer
/// carried the exact Health shape ten minutes before ledgerd-core-q2 asked what `/api/health`
/// exposes — and invented one). Own slice first, then the path-matched strangers. Unanswered
/// rows are never spliced: their absence already rode `research_unanswered`.
pub(super) fn prior_minis_for<'a>(
    q: &ResearchQuestion,
    rows: &'a [ResearchRow],
) -> Vec<&'a ResearchRow> {
    let mine = path_tokens(&q.question);
    let mut same: Vec<&ResearchRow> = Vec::new();
    let mut matched: Vec<&ResearchRow> = Vec::new();
    for r in rows {
        if r.status != RESEARCH_ANSWERED {
            continue;
        }
        if r.slice == q.slice {
            if r.q_index != q.q_index {
                same.push(r);
            }
        } else if !mine.is_disjoint(&path_tokens(&r.question)) {
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
pub(super) fn prior_minis_block(q: &ResearchQuestion, prior: &[&ResearchRow]) -> String {
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
        let from = match (r.slice == q.slice, r.slice == DECISION_SLICE) {
            (true, true) => "an earlier open decision this fan settled".to_string(),
            (true, false) => "this slice's own earlier lane".to_string(),
            (false, true) => {
                "an open decision this fan settled — it names the same path as your question"
                    .to_string()
            }
            (false, false) => format!(
                "slice `{}` — its question names the same path as yours",
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

/// The dispatch-time assembly of one lane's user text, and the one `research_context` event per
/// dispatch that lets the tick print a lane's grounding: how many prior minis it saw (and
/// which), and how many sections the index named for it (0 when the orientation is not armed
/// and the whole request rides inline).
pub(super) fn research_dispatch_text(
    root: &Path,
    events: &dyn EventSink,
    head: &str,
    q: &ResearchQuestion,
    activity_key: &str,
    index_sections: usize,
) -> String {
    let rows = load_research_minis(root);
    let prior = prior_minis_for(q, &rows);
    events.write_value(serde_json::json!({
        "event": "research_context",
        "task": activity_key,
        "slice": q.slice,
        "q_index": q.q_index,
        "prior_minis": prior.len(),
        "prior_from": prior
            .iter()
            .map(|r| research_mini_name(&r.slice, r.q_index))
            .collect::<Vec<_>>(),
        "index_sections": index_sections,
    }));
    research_user_text(head, &prior_minis_block(q, &prior), q)
}

/// The fan's QUEUE as one event, emitted once when it is built and before anything dispatches:
/// how many questions the opener left (`questions`), how many of them dispatch now versus
/// arrive settled from the ledger on resume, and the per-slice count — every number derived
/// from the queue itself. Before this the vigil derived the total by counting '?' in the
/// opener's output (r6c); an instrument that has to guess the denominator is not one.
pub(super) fn emit_research_planned(
    events: &dyn EventSink,
    dispatching: &[ResearchQuestion],
    resumed: &[ResearchRow],
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
        "per_slice": per_slice,
    }));
}

/// The fan's phase announcement — the stderr banner AND the `phase` event run.jsonl readers
/// fold (tick.py's phase line, the panel's ribbon via ENGINE_PHASE). Before this the banner was
/// console-only and a 30-minute fan ran under `phase=ask` in every instrument (r6c). Called once,
/// when the fan has something to dispatch — a fully-resumed fan announces no phase it does not run.
pub(super) fn announce_research_phase(events: &dyn EventSink) {
    phase_banner(
        "RESEARCH",
        "every host answers the opener's own questions in parallel",
    );
    events.write_value(serde_json::json!({"event": "phase", "phase": "research"}));
}

pub(super) fn research_system_text() -> String {
    "You are answering ONE question that must be settled before this software is built. Ground \
     your answer: read the request text you were given, read the existing tree's files with your \
     shell and tree tools, and when the request names a documentation URL, fetch it — an answer \
     copied from the real source beats any paraphrase. Do NOT create or edit files: you have no \
     write or edit tool, and your structured reply IS your deliverable.\n\n\
     Your answer is a HANDOFF to the builder: name exact files, exact key/field literals, exact \
     endpoints or signatures where the request implies them; where the request is silent, state \
     the most CONVENTIONAL choice and say it is a convention. Before you call anything a \
     convention or raise it as not frozen, check the orientation index for a section that names \
     it and read that section from the request file named under SOURCES — silence in your \
     excerpt is not silence in the request. If the question cannot be settled from the request \
     or the sources, say exactly that in one line and still name the conventional choice. Keep \
     it under a page.\n\n\
     When you are done, call the final_output tool ONCE with {\"answer\": \"...\", \"raised\": \
     [...]} — `raised` lists further questions you could NOT settle: do not answer them, and \
     nothing will dispatch them; they are handed VERBATIM to the builder of this slice as open \
     points, so phrase each as a decision that builder can make in one line, naming the \
     conventional choice when you have one."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::{
        briefs_from_slices, orientation_armed, spec_sections, write_research_ledger, NullSink,
        OpenOutput, OpenSlice, SwarmEvent,
    };
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
        let q = ResearchQuestion {
            slice: "payments".into(),
            q_index: 0,
            question: "What is the frozen payment record structure from section 2?".into(),
        };
        let text = research_user_text(&head, "", &q);
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
        assert!(text.ends_with(
            "\n\nTHE QUESTION:\nWhat is the frozen payment record structure from section 2?"
        ));
        assert!(
            text.contains("app/__main__.py"),
            "the existing tree rides in"
        );
        // Below the floor: the spec as-is is the better input, exactly like OPEN's own message.
        let small = "build a tiny thing";
        let small_block =
            research_request_block(small, &spec_sections(small), false, "s1", &[], &NullSink);
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
        let q0 = ResearchQuestion {
            slice: "ledgerd-core".into(),
            q_index: 0,
            question: "What is the exact ledger.db schema and index set?".into(),
        };
        let first =
            research_dispatch_text(root, &sink, "HEAD", &q0, "research-ledgerd-core-q0", 28);
        assert_eq!(
            first, "HEAD\n\nTHE QUESTION:\nWhat is the exact ledger.db schema and index set?",
            "a first dispatch: head, question, nothing invented between them"
        );
        {
            let ev = sink.0.lock().unwrap();
            assert_eq!(ev.len(), 1);
            assert_eq!(ev[0]["event"], "research_context");
            assert_eq!(ev[0]["task"], "research-ledgerd-core-q0");
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

        let q1 = ResearchQuestion {
            slice: "ledgerd-core".into(),
            q_index: 1,
            question: "How is sync cursor state persisted so a dropped connection resumes?".into(),
        };
        let second =
            research_dispatch_text(root, &sink, "HEAD", &q1, "research-ledgerd-core-q1", 28);
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
            second.find("ALREADY ANSWERED").unwrap() < second.find("THE QUESTION:").unwrap(),
            "the snowball precedes the question"
        );

        let q2 = ResearchQuestion {
            slice: "ledgerd-core".into(),
            q_index: 2,
            question:
                "What does /api/health expose as the degraded state while the vendor is down?"
                    .into(),
        };
        let third =
            research_dispatch_text(root, &sink, "HEAD", &q2, "research-ledgerd-core-q2", 28);
        assert!(third.contains("before your dispatch (2)"), "{third}");
        assert!(third.contains(
            "[slice `ledgerd-api` — its question names the same path as yours; \
             .swarm/ledger/research-ledgerd-api-q0.json]"
        ));
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
            assert_eq!(
                ev[2]["prior_from"],
                serde_json::json!([
                    "research-ledgerd-core-q0.json",
                    "research-ledgerd-api-q0.json"
                ])
            );
        }
        let d = ResearchQuestion {
            slice: DECISION_SLICE.into(),
            q_index: 0,
            question: "D2: is rejected terminal?".into(),
        };
        let decision = research_dispatch_text(root, &sink, "HEAD", &d, "research-decisions-q0", 0);
        assert!(decision.ends_with("\n\nTHE OPEN DECISION:\nD2: is rejected terminal?"));
        assert!(
            !decision.contains("ALREADY ANSWERED"),
            "no decision settled yet and no question shares a path — nothing is spliced"
        );
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
            ResearchQuestion {
                slice: "ledgerd-core".into(),
                q_index: 1,
                question: "q".into(),
            },
            ResearchQuestion {
                slice: "ledgerd-core".into(),
                q_index: 2,
                question: "q".into(),
            },
            ResearchQuestion {
                slice: DECISION_SLICE.into(),
                q_index: 0,
                question: "d".into(),
            },
        ];
        let resumed = vec![row("ledgerd-core", 0, RESEARCH_ANSWERED, &[])];
        emit_research_planned(&sink, &dispatching, &resumed);
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            ev[0],
            serde_json::json!({
                "event": "research_planned",
                "questions": 4,
                "dispatching": 3,
                "resumed": 1,
                "per_slice": {"__open_decisions__": 1, "ledgerd-core": 3},
            })
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
}
