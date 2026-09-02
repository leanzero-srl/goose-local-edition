//! VA-104: cross-slice research routing, and the flat fallback plan it rides beside.
//!
//! THE DEFECT (r6h, live, tick 3, 06:14–06:27): `briefs_from_slices` renders a research answer
//! into ONE brief — the slice whose lane asked (`research.iter().filter(|r| r.slice == sl.id)`).
//! webhooks-workflow's q5 (the vendor's `POST /v3/webhooks` registration contract) named
//! `app/webhooks.py` as the caller and landed only in webhooks-workflow's ANSWERS block;
//! ledgerd-core's 42,882-char brief had 0 hits for it while its objective names `app/webhooks.py`
//! as a collaborator ("the SSE fan-out that app/webhooks.py and app/drafts.py (owned by
//! webhooks-workflow) call"), and its lane spent calls 3–6 and ~15k reasoning chars re-deriving
//! the contract — "registered: set by registration at boot (my job? — pending doc check)" →
//! "registration is probably mine (health.registered)" — and drafted
//! `threading.Thread(target=_register_webhook, args=(ctx,), daemon=True).start()` into its boot.
//! Two lanes implementing one behaviour.
//!
//! THE MECHANISM, derived from the plan's facts only (no phrase list anywhere): AFTER
//! `finalize_plan_before_dag`'s repairs — so the owner map holds the plan's FINAL paths — every
//! answered row is matched against the plan's file ownership by whole-path occurrence, of the
//! file or of the module form the repair rewrote it from (`module_form`: `app/ledgerd.py` for
//! `app/ledgerd/impl.py`; the opener and the lanes wrote before the repair). An answer that names
//! a file ANOTHER task owns routes to that task (YOURS — the asker builds against your file; a
//! bare basename only when the owner's own objective names it, else a loud
//! `research_answer_unrouted{reason: passing_mention}`); an answer that names the asker's OWN
//! file routes to every other task whose objective names that file (NOT YOURS — the plan's owner
//! implements it; the task is quoted the sentence of its objective that names the file).
//!
//! MEASURED on r6h before a line was written, corrected by the refuter on d1b0e4f3b: the ledger
//! held 90 minis, 82 of them the opener's `spec_fact` rows from the mechanism VA-089 deletes.
//! 7 routings matched — webhooks-workflow q5/q6/q14 → ledgerd-core (the three the defect named,
//! 6,575 chars; these stand on the shipping engine) and three from `spec_fact` rows that cannot
//! occur on it (console-page q0 → viz-engine, notifierd q1 and webhooks-workflow q8 →
//! console-page, the last two bare `DECISIONS.md` mentions that motivated the quality floor). A
//! path-only ROUTE arm measured 3 hits, not 0 as first claimed — ledgerd-core q6/q8/q10 name the
//! VENDOR's `/v3/payments`, which webhooks-workflow's objective also names — none an app route
//! another task serves, so the arm is still not built (gate 9). Every routing is a
//! `research_answer_routed` event; a slash-written source path no plan file (or module form)
//! matches is a `research_answer_unowned` event, never a quiet skip.
//!
//! Sibling module under the incremental-split law: `flat_plan_from_briefs` moved verbatim from
//! swarm.rs (its tests stay beside `splice_briefs`'s in the root's test module), paying for the
//! routing's wiring in `plan_slices_to_dag`.

use std::collections::BTreeMap;

use super::decisions::{DECISION_SLICE, OPEN_DECISIONS_HEADER, SETTLED_DECISIONS_HEADER};
use super::findings::FINDING_PATH_EXTS;
use super::plan_repairs::PACKAGE_IMPL_FILE;
use super::research::{research_mini_name, ResearchRow, RESEARCH_ANSWERED};
use super::{plan_sink_description, EventSink, SliceBrief, TargetLang};

pub(super) const ROUTED_ANSWERS_HEADER: &str = "SETTLED BY ANOTHER SLICE'S RESEARCH";

/// Every position where `path` occurs in `text` as a WHOLE path — not the tail of a longer one
/// (`xapp/api.py`, `pkg/app/api.py`), not the head of one (`app/api.pyc`, `app/api.py/x`). The
/// plan's own file string is the needle: reference syntax, never a bare word (VA-103's rule).
// string_slice: `i` is a `match_indices` hit and `i + path.len()` that match's end — char
// boundaries by construction.
#[allow(clippy::string_slice)]
fn path_positions(text: &str, path: &str) -> Vec<usize> {
    let path_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '-');
    text.match_indices(path)
        .filter(|(i, _)| {
            let before = text[..*i].chars().next_back();
            let after = text[i + path.len()..].chars().next();
            !before.is_some_and(path_char)
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/'))
        })
        .map(|(i, _)| i)
        .collect()
}

fn find_path(text: &str, path: &str) -> Option<usize> {
    path_positions(text, path).first().copied()
}

/// The MODULE form the opener and the research lanes wrote for a file the plan repair has since
/// rewritten into its package: `app/ledgerd.py` for `app/ledgerd/impl.py` (`PACKAGE_IMPL_FILE`,
/// `repair_module_package_collisions`). The routing runs after the repairs, so the owner map
/// holds final paths while every objective and answer still carries the form written before.
fn module_form(file: &str) -> Option<String> {
    let dir = file.strip_suffix(PACKAGE_IMPL_FILE)?.strip_suffix('/')?;
    (!dir.is_empty()).then(|| format!("{dir}.py"))
}

/// The sentence of `text` around the first whole occurrence of `path`: from the previous line
/// break or sentence end to the next. This is the one line of a task's OBJECTIVE that says what
/// it does with a file it does not own, quoted back to it.
// string_slice: `at`/`after` bound a `find_path` match; every other offset is a `find`/`rfind` hit
// moved past an ASCII needle (`\n`, `. `) or `text.len()` — char boundaries by construction.
#[allow(clippy::string_slice)]
fn sentence_naming(text: &str, path: &str) -> Option<String> {
    let at = find_path(text, path)?;
    let head = &text[..at];
    let start = [
        head.rfind('\n').map(|i| i + 1),
        head.rfind(". ").map(|i| i + 2),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);
    let after = at + path.len();
    let tail = &text[after..];
    let end = [tail.find('\n'), tail.find(". ").map(|i| i + 1)]
        .into_iter()
        .flatten()
        .min()
        .map_or(text.len(), |i| after + i);
    Some(text[start..end].trim().to_string())
}

/// The slash-bearing source paths an answer writes — `files_from_objective`'s rule (a source
/// extension `FINDING_PATH_EXTS` names) without its backtick requirement, because an answer
/// writes "So app/webhooks.py may safely call…" bare. Prose punctuation is split off; URLs,
/// route paths (leading `/`) and templates (`<module>`, `*`) are not files; a bare basename
/// (`request.md`, `ledger.db`, `index.html`) is not judged — the spec and the databases are bare
/// words in nearly every answer, and only a path written AS a path can be a stranger to the plan.
fn path_tokens(text: &str) -> Vec<String> {
    let split = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '|'
            )
    };
    let mut out: Vec<String> = Vec::new();
    for word in text.split(split) {
        let tok = word.strip_prefix("./").unwrap_or(word);
        let tok = tok.trim_matches(|c: char| matches!(c, '.' | '*' | '—' | '-'));
        if tok.is_empty()
            || !tok.contains('/')
            || tok.starts_with('/')
            || tok.contains("://")
            || tok.contains(['<', '>', '*', '{'])
        {
            continue;
        }
        let lower = tok.to_lowercase();
        if !FINDING_PATH_EXTS.iter().any(|e| lower.ends_with(e)) {
            continue;
        }
        if !out.iter().any(|t| t == tok) {
            out.push(tok.to_string());
        }
    }
    out
}

/// One plan file with its owner and the module form it may have been written as.
struct Owned<'a> {
    file: &'a str,
    module_form: Option<String>,
    task: &'a str,
    slice: &'a str,
}

impl Owned<'_> {
    /// The form `text` writes this file as — the plan's path, or its pre-repair module form.
    fn written_in(&self, text: &str) -> Option<&str> {
        if find_path(text, self.file).is_some() {
            return Some(self.file);
        }
        self.module_form
            .as_deref()
            .filter(|m| find_path(text, m).is_some())
    }

    fn mentions_in(&self, text: &str) -> usize {
        path_positions(text, self.file).len()
            + self
                .module_form
                .as_deref()
                .map_or(0, |m| path_positions(text, m).len())
    }
}

enum Arm {
    /// This task owns the file the answer names: the asker builds against it.
    OwnedHere,
    /// The ASKER owns the file and this task's objective names it — that sentence, and how many
    /// places the objective names the file (1 → "the only sentence", else "the first of N").
    ImplementedByAsker { sentence: String, mentions: usize },
}

impl Arm {
    fn as_str(&self) -> &'static str {
        match self {
            Arm::OwnedHere => "owned_here",
            Arm::ImplementedByAsker { .. } => "implemented_by_asker",
        }
    }
}

/// The routed block goes ABOVE the decisions partition when the brief carries one, so
/// `decisions::brief_decisions_block`'s header-to-tail cut (what a repair shard is handed as
/// "decisions") never swallows another slice's answers — 2a D11's lesson, kept structurally.
// string_slice: `at` is a `find` hit or `description.len()`.
#[allow(clippy::string_slice)]
pub(super) fn insert_above_decisions(description: &str, block: &str) -> String {
    let at = [SETTLED_DECISIONS_HEADER, OPEN_DECISIONS_HEADER]
        .iter()
        .filter_map(|h| description.find(&format!("\n\n{h}")))
        .min()
        .unwrap_or(description.len());
    format!("{}{block}{}", &description[..at], &description[at..])
}

/// Route every answered research row that names a plan file into the OTHER tasks it concerns
/// (module doc), with the plan's owner stated per item. Runs on the REPAIRED plan string at both
/// doors of `plan_slices_to_dag` — the finalized synthesised / synthesis-failed plan and the
/// finalized DAG-invalid flat plan — and returns it byte-identical when nothing routes. Ownership
/// is the plan's `files`; the asker is a SLICE, so the match is on each task's `slice` (its `id`
/// when synthesis named none, exactly as `splice_briefs` keys the brief), never on synthesis's
/// id spelling.
pub(super) fn route_cross_slice_answers(
    plan_json: String,
    briefs: &[SliceBrief],
    research: &[ResearchRow],
    events: &dyn EventSink,
) -> String {
    let mut plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            // Not a substitution: the same string reaches `Dag::from_planner_json`, whose refusal
            // is the loud `synthesis_fallback`; this event says the routing saw it first.
            events.write_value(serde_json::json!({
                "event": "research_answer_routing_skipped",
                "error": e.to_string(),
            }));
            return plan_json;
        }
    };
    // The analysis borrows the plan's strings; it ends before the descriptions are rewritten.
    let rendered: BTreeMap<String, String> = {
        let Some(tasks) = plan.get("subtasks").and_then(|t| t.as_array()) else {
            events.write_value(serde_json::json!({
                "event": "research_answer_routing_skipped",
                "error": "the plan has no `subtasks` array",
            }));
            return plan_json;
        };
        // THE PLAN'S OWNERSHIP: every file with its task and slice, first claimant wins (the
        // rule `repair_shared_files` already applied); and the tasks a brief exists for, with
        // the opener's objective — the text whose sentences name a collaborator's file.
        let mut owned: Vec<Owned> = Vec::new();
        let mut candidates: Vec<(&str, &str, &str)> = Vec::new();
        for t in tasks {
            let Some(id) = t.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let slice = t
                .get("slice")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(id);
            for f in t
                .get("files")
                .and_then(|f| f.as_array())
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
            {
                if !owned.iter().any(|o| o.file == f) {
                    owned.push(Owned {
                        file: f,
                        module_form: module_form(f),
                        task: id,
                        slice,
                    });
                }
            }
            if let Some(b) = briefs.iter().find(|b| b.id == slice) {
                candidates.push((id, slice, b.objective.as_str()));
            }
        }
        let mut items: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for row in research {
            if row.status != RESEARCH_ANSWERED
                || row.question.trim().is_empty()
                || row.slice == DECISION_SLICE
            {
                continue;
            }
            let named: Vec<(&Owned, &str)> = owned
                .iter()
                .filter_map(|o| o.written_in(&row.answer).map(|w| (o, w)))
                .collect();
            let strangers: Vec<String> = path_tokens(&row.answer)
                .into_iter()
                .filter(|t| {
                    !owned.iter().any(|o| {
                        o.file == t.as_str() || o.module_form.as_deref() == Some(t.as_str())
                    })
                })
                .collect();
            if !strangers.is_empty() {
                events.write_value(serde_json::json!({
                    "event": "research_answer_unowned",
                    "from_slice": row.slice,
                    "q_index": row.q_index,
                    "names": strangers,
                }));
            }
            if named.is_empty() {
                continue;
            }
            for (task, slice, objective) in &candidates {
                if *slice == row.slice {
                    continue;
                }
                let mut lines: Vec<String> = Vec::new();
                for (o, written) in &named {
                    let arm = if o.task == *task {
                        // QUALITY FLOOR (the refuter on d1b0e4f3b): a bare basename (`README.md`,
                        // `DECISIONS.md`) is written in passing by answers about anything —
                        // r6h shipped console-page a mid-sentence fragment as "deliver that
                        // surface". It routes to its owner only when the owner's own objective
                        // names it too, or the path carries a directory; the miss is named.
                        if !o.file.contains('/') && o.written_in(objective).is_none() {
                            events.write_value(serde_json::json!({
                                "event": "research_answer_unrouted",
                                "from_slice": row.slice,
                                "q_index": row.q_index,
                                "to_task": task,
                                "file": o.file,
                                "reason": "passing_mention",
                            }));
                            continue;
                        }
                        Arm::OwnedHere
                    } else if o.slice == row.slice {
                        let Some(form) = o.written_in(objective) else {
                            continue;
                        };
                        let Some(sentence) = sentence_naming(objective, form) else {
                            continue;
                        };
                        Arm::ImplementedByAsker {
                            sentence,
                            mentions: o.mentions_in(objective),
                        }
                    } else {
                        continue;
                    };
                    events.write_value(serde_json::json!({
                        "event": "research_answer_routed",
                        "from_slice": row.slice,
                        "to_task": task,
                        "q_index": row.q_index,
                        "matched": "file",
                        "value": o.file,
                        "written": written,
                        "owner": o.task,
                        "arm": arm.as_str(),
                    }));
                    let written_note = if *written == o.file {
                        String::new()
                    } else {
                        format!(" (written {written} before the plan repair)")
                    };
                    lines.push(match &arm {
                        Arm::OwnedHere => format!(
                            "  YOURS: {}{written_note} is yours in the plan; {}'s answer names it \
                             and builds against what it says — deliver that surface exactly as \
                             written there.",
                            o.file, row.slice
                        ),
                        Arm::ImplementedByAsker { sentence, mentions } => format!(
                            "  NOT YOURS: {}{written_note} is {}'s in the plan and the answer \
                             names it — {} implements this, not you. Your objective names it \
                             here: \"{sentence}\" — {}.",
                            o.file,
                            o.task,
                            o.task,
                            if *mentions == 1 {
                                "the only sentence of your objective that names it".to_string()
                            } else {
                                format!("the first of {mentions} places your objective names it")
                            }
                        ),
                    });
                }
                if lines.is_empty() {
                    continue;
                }
                items.entry(*task).or_default().push(format!(
                    "- {}'s research q{}: {}\n{}\n  A: {}\n  FULL ANSWER: .swarm/ledger/{}",
                    row.slice,
                    row.q_index,
                    row.question.trim(),
                    lines.join("\n"),
                    row.answer.trim().replace('\n', "\n     "),
                    research_mini_name(&row.slice, row.q_index)
                ));
            }
        }
        if items.is_empty() {
            return plan_json;
        }
        items
            .into_iter()
            .map(|(task, items)| {
                let n = items.len();
                let block = format!(
                    "\n\n{ROUTED_ANSWERS_HEADER} — {n} answer{} another slice's lane settled that \
                     name a file of yours, or name the asker's own file where your objective \
                     names it; the OWNER is the plan's and is stated per item (routed by the \
                     engine — do not implement what another task owns, and do not re-derive who \
                     owns it):\n{}",
                    if n == 1 { "" } else { "s" },
                    items.join("\n")
                );
                (task.to_string(), block)
            })
            .collect()
    };
    if let Some(tasks) = plan.get_mut("subtasks").and_then(|t| t.as_array_mut()) {
        for t in tasks.iter_mut() {
            let Some(block) = t
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|id| rendered.get(id))
            else {
                continue;
            };
            let description = t
                .get("description")
                .and_then(|d| d.as_str())
                .map(|d| insert_above_decisions(d, block))
                .unwrap_or_else(|| block.trim_start().to_string());
            t["description"] = serde_json::Value::from(description);
        }
    }
    plan.to_string()
}

/// One task per slice, deps stripped, plus the sink. Always validates.
///
/// EACH TASK OWNS THE FILES ITS OWN BRIEF DECLARES. This hardcoded `"files": []`, so on either fallback
/// path — synthesis failed, or the synthesised plan will not load as a DAG — every task owned nothing:
/// the scheduler had no file ownership to serialise on, `smoke_all_files` was empty, the decomposition
/// counters reported the whole plan as `tasks_owning_nothing`, and `require_advertised_entry_files`
/// degenerated (its last-resort pick is the first task owning anything, and none did), so the
/// package-entry guarantee added after two runs shipped packages with no `__main__.py` did not run at all.
/// `SliceBrief.files` is populated by `files_from_objective` precisely so ownership is not invented.
///
/// FIRST CLAIMANT WINS, so ownership stays disjoint: two objectives declaring the same path is the
/// expected case (the synthesised path measures the same collision as `shared_files` in the
/// decomposition flags), and a plan where two tasks own one file is a plan the scheduler must
/// serialise rather than parallelise.
pub(super) fn flat_plan_from_briefs(briefs: &[SliceBrief], lang: TargetLang, spec: &str) -> String {
    // A slice whose objective declared no files (the `slice_files_unnamed` case) would make an
    // owns-nothing task, and an owns-nothing task is REMOVED by the plan repair's rule (a) — the
    // fallback once shed every task that way in its own test. A conventional one-module-per-slice
    // path keeps every fallback task buildable.
    let ext = match lang {
        TargetLang::Python => "py",
        TargetLang::TypeScript => "ts",
        TargetLang::Rust => "rs",
        TargetLang::Go => "go",
        TargetLang::Other => "py",
    };
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tasks: Vec<serde_json::Value> = briefs
        .iter()
        .map(|b| {
            let mut files: Vec<String> = b
                .files
                .iter()
                .filter(|f| claimed.insert((*f).clone()))
                .cloned()
                .collect();
            if files.is_empty() {
                let conventional = format!("{}.{ext}", b.id.replace('-', "_"));
                if claimed.insert(conventional.clone()) {
                    files.push(conventional);
                }
            }
            serde_json::json!({
                "id": b.id,
                "slice": b.id,
                "difficulty": "hard",
                "files": files,
                "depends_on": [],
                "description": b.brief,
            })
        })
        .collect();
    tasks.push(serde_json::json!({
        "id": goose_swarm::SINK_ID,
        "difficulty": "hard",
        "files": [],
        "depends_on": briefs.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
        // Finding 1: the fallback plan's sink row reaches the judge and the review — built
        // from the spec's advertised surface, never the banned template.
        "description": plan_sink_description(spec, lang),
    }));
    serde_json::json!({ "subtasks": tasks }).to_string()
}

#[cfg(test)]
mod tests {
    use super::super::SwarmEvent;
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

    impl ValueSink {
        fn named(&self, event: &str) -> Vec<serde_json::Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e["event"] == event)
                .cloned()
                .collect()
        }
    }

    // r6h's real rows (`.swarm/ledger/research-webhooks-workflow-q5.json`, `-q6.json`), verbatim.
    const Q5_QUESTION: &str = "What do the v3 docs prescribe for POST /v3/webhooks registration — payload, challenge timing, secret return, idempotent re-registration?";
    const Q5_ANSWER: &str = "From v3 docs §8 \"Registration — POST /v3/webhooks\" (fetched from http://127.0.0.1:8850/v3/docs): (1) PAYLOAD: JSON body exactly `{\"url\": \"http://127.0.0.1:<port>/your/endpoint\"}` — for this app the URL is frozen by request.md §4 as `http://127.0.0.1:<ledger-port>/api/webhooks/meridian`, so the body is `{\"url\": \"http://127.0.0.1:<ledger-port>/api/webhooks/meridian\"}`. (2) CHALLENGE TIMING: \"Before accepting, Meridian POSTs an UNSIGNED challenge to your URL: {\\\"type\\\": \\\"webhook.verify\\\", \\\"challenge\\\": \\\"<hex>\\\"}. Answer 200 {\\\"challenge\\\": \\\"<the same hex>\\\"} within 10 seconds — your endpoint must already be listening when you register.\" That is why request.md orders registration AFTER ledgerd is bound and listening; the challenge arrives during/just after the registration call, unsigned, and (per request.md) increments no counter. (3) SECRET RETURN: on success the registration response is `{\"id\": \"wh_…\", \"secret\": \"whsec_…\"}` — persist id+secret to disk under --db-dir so a restart can verify deliveries immediately; the secret is the HMAC key for every later `Meridian-Signature`. (4) IDEMPOTENT RE-REGISTRATION: docs verbatim: \"Registration is idempotent by URL: re-registering (e.g. after a restart) returns the same id and secret.\" So app/webhooks.py may safely call POST /v3/webhooks on every startup (retrying until the vendor is reachable, per request.md) even when a persisted secret exists — retries cannot create duplicate registrations; if the response differs from persisted values, trust the returned pair (docs guarantee they are the same).";
    const Q6_QUESTION: &str = "What do the v3 docs say about recognizing and completing transaction groups (txn id/part/of, stage until complete)?";
    const Q6_ANSWER: &str = "From v3 docs §8 \"Deliveries\" — Transaction groups: event envelope shape is `{\"id\": \"evt_0001\", \"type\": ..., \"created_at\": \"...\", \"txn\": null | {\"id\": \"txn_9\", \"part\": 1, \"of\": 2}, \"data\": <payment or reversal resource>}`. Docs verbatim rule: \"**Transaction groups**: events carrying `txn` with matching `id` form one atomic group of `of` parts (the refund pair: `payment.updated` to `refunded` + `reversal.created`). Stage parts until the group is complete, then apply them in ONE local transaction. No read of your store may ever observe half a group.\" Consequences for app/webhooks.py: (a) group key is `txn.id`; completeness = having all `of` distinct parts (`part` values 1..of); (b) dedupe on event id BEFORE staging — request.md §4 says group parts \"may arrive in either order, race other traffic, and be duplicated like any delivery\", so a duplicate part must not count twice toward completeness nor be applied twice; (c) when the group completes, apply both changes plus BOTH ledger events (`payment.updated` for the refund flip + `reversal.created`) inside ONE local transaction via app/ledger.py's single-transaction helper, so no API read can ever see the refunded payment without its reversal (graded per request.md consistency rules \"Group atomicity\"); (d) a group that never completes simply stays staged — never partially apply. Non-txn events (`txn: null`) are applied immediately as normal.";

    // r6h's two objectives EXACTLY as the opener wrote them (`.swarm/plan.json`, pre-repair: the
    // module `app/ledgerd.py` that `finalize_plan_before_dag` rewrote into `app/ledgerd/impl.py`).
    // `SliceBrief.objective` is this text at routing time; the plan's `files` are the repaired ones.
    const LEDGERD_CORE_OBJECTIVE: &str = "Build the ledgerd service and the `app` package boot contract. Owns: app/__init__.py, app/__main__.py, app/ledgerd.py, app/db.py, app/sync.py, app/ledger.py, app/relay.py, app/api.py, README.md (no other slice touches these). Implements: the three boot commands (`python -m app` boots both services as a convenience wrapper; `python -m app.ledgerd` alone), 127.0.0.1-only bind, listening within 10 s of start on fresh boot and every restart, idempotent schema init of ledger.db under --db-dir, clean resume with no duplicate application. Vendor sync walk per the docs: fixed 64-per-page cursor pagination (192 pages at fixture count), resume after a dropped connection, single Retry-After retry on 500, cursor restart on 410 cursor_expired, ETag/If-None-Match + X-Collection-Generation mismatch rule, ≤10 s timeout on every vendor request, upserts that compare version and never regress a row, plus GET /v3/reversals so reversal truth survives webhook outages; unprompted first sync once listening (retry at least every 5 s while the vendor is down, degraded state visible, keep serving local data). Event ledger: exactly one event per applied state change, seq strictly increasing and contiguous from 1. Outbox: the five crossing types written in the SAME SQLite transaction as their state change; background relay delivering batches of ≤50 ascending seq to POST /notify/events with backoff capped at 2 s, at-least-once, marked delivered only on 200, resuming from durable rows after restart, never running inside a request handler. Read API on one structured error envelope: GET /api/health (in-memory webhook counters), GET /api/payments list+detail (flattened counterparty keys, status/currency filters, instant-based sort, filtered total), GET /api/summary (per-currency only, no cross-currency totals), GET /api/buckets (gapless Berlin days × 4 statuses, day-major, zeros included), POST /api/sync (502 vendor_unavailable when down; all reads keep answering inside budgets during a sync — threaded stdlib server, never park readers behind vendor waits), POST /api/payments/<id>/note (If-Match dance: 412 → re-fetch + retry once, second 412 → 409 conflict, never write without If-Match), GET /api/events (bearer token, any role), GET /api/outbox/status, GET /api/notifications proxied to notifierd (502 notifier_unreachable when down), GET /api/viz/records (columnar, server-computed Europe/Berlin `day`, order created_at instant ASC then id ASC), GET /api/stream SSE hub (concurrent subscribers, per-connection batch numbers starting at 1 and monotonically increasing, one message per committed payment-state change, initial dataset never replayed). Serves web/ statically at / and /web/* with correct content types. Interface: app/ledger.py exposes a single-transaction helper (state change + ledger event + outbox row in one commit) and the SSE fan-out that app/webhooks.py and app/drafts.py (owned by webhooks-workflow) call; app/auth.py (owned by webhooks-workflow) supplies token/role checks for /api/events; app/ledgerd.py mounts the ROUTES tables exported by those two modules. README.md documents the exact install-nothing run commands.";
    const WEBHOOKS_OBJECTIVE: &str = "Build the vendor-facing webhook side and the maker/checker/admin approval workflow inside ledgerd. Owns: app/webhooks.py, app/drafts.py, app/auth.py (no other slice touches these). app/auth.py loads the tokens JSON ({\"maker\",\"checker\",\"admin\"} 32-hex) from --tokens-file and exposes bearer-token→role resolution used by the drafts routes and /api/events: missing/unknown token → 401 unauthorized, known token wrong role → 403 forbidden, admin reads everything and writes nothing. app/webhooks.py: POST /api/webhooks/meridian — on startup AFTER ledgerd is bound, register the URL with the vendor as the docs prescribe (idempotent by URL, retried until reachable, secret persisted so a restart re-registers cleanly); answer the unsigned challenge handshake 200 {\"challenge\": same hex} without touching any counter; then verify every delivery's Meridian-Signature FIRST against the RAW body bytes (missing/wrong → 401 bad_signature, state untouched, rejected+1); apply valid events idempotently in version order (event id already processed or version ≤ stored → ignored+1, state untouched; else apply + ledger event + applied+1), respond 200 {\"received\": true} in all three cases, count every arrival in received, answer within 3 s, never trigger a sync or vendor call from the handler; transaction groups (txn id/part/of) staged until complete then applied in ONE local transaction via app/ledger.py's single-transaction helper so no read ever observes a half group. app/drafts.py: POST /api/drafts (maker|checker), POST /api/drafts/<id>/submit (maker|checker), POST /api/drafts/<id>/approve (checker), POST /api/drafts/<id>/reject (checker), GET /api/drafts?state= (any role) exactly per the frozen table; four-eyes — approve/reject attempted with the SAME token that submitted → 403 approval_forbidden, state untouched; validation with the frozen field-error codes and dot paths with [index]; D2 decision settled here: rejected drafts are TERMINAL (the frozen machine has no edge out of rejected) — implement it, and it is documented in DECISIONS.md by the console-page slice. The SEND: on approve, after committing approved + draft.approved, POST /v3/payments with an Idempotency-Key generated and stored WITH the draft before the first attempt; on 2xx append payment.sent (outbox-crossing); any retry after crash/timeout/stall reuses the stored key — exactly one vendor payment per approved draft ever; SIGKILL immediately after a submit's or approve's 200 (including mid-send) must resume with state intact, outbox preserved, and the send finished under the same key. Every state change goes through app/ledger.py's single-transaction helper so ledger event + outbox row commit atomically. Both modules export ROUTES tables that app/ledgerd.py (owned by ledgerd-core) mounts.";
    const INTERFACE_SENTENCE: &str = "Interface: app/ledger.py exposes a single-transaction helper (state change + ledger event + outbox row in one commit) and the SSE fan-out that app/webhooks.py and app/drafts.py (owned by webhooks-workflow) call; app/auth.py (owned by webhooks-workflow) supplies token/role checks for /api/events; app/ledgerd.py mounts the ROUTES tables exported by those two modules.";
    const MOUNTS_SENTENCE: &str =
        "Both modules export ROUTES tables that app/ledgerd.py (owned by ledgerd-core) mounts.";

    fn brief(id: &str, objective: &str, brief: &str) -> SliceBrief {
        SliceBrief {
            id: id.to_string(),
            title: id.to_string(),
            objective: objective.to_string(),
            brief: brief.to_string(),
            files: Vec::new(),
            settled: String::new(),
        }
    }

    fn answered(slice: &str, q_index: usize, question: &str, answer: &str) -> ResearchRow {
        ResearchRow {
            slice: slice.to_string(),
            q_index,
            question: question.to_string(),
            status: RESEARCH_ANSWERED.to_string(),
            answer: answer.to_string(),
            reason: None,
            detail: None,
            raised: Vec::new(),
            model: "m".to_string(),
            secs: 1,
            kind: "external".to_string(),
            cite: String::new(),
            batch: 1,
        }
    }

    /// r6h's plan-loaded.json ownership — POST-repair, what the routing sees after
    /// `finalize_plan_before_dag`: `app/ledgerd/impl.py` (the rewritten module), `app/__main__.py`
    /// moved to the engine's skeleton (no brief), the sink owning nothing. ledgerd-core's brief
    /// carries a decisions block so the insertion point is exercised.
    fn r6h_plan(ledgerd_brief: &str) -> String {
        serde_json::json!({"subtasks": [
            {"id": "skeleton", "files": ["app/__main__.py", "app/ledgerd/__main__.py", "app/ledgerd/__init__.py"],
             "depends_on": [], "description": "WALKING SKELETON — assembled by the engine: app/webhooks.py mounts later."},
            {"id": "ledgerd-core", "slice": "ledgerd-core",
             "files": ["app/__init__.py", "app/ledgerd/impl.py", "app/db.py", "app/sync.py", "app/ledger.py", "app/relay.py", "app/api.py", "README.md"],
             "depends_on": ["skeleton"], "description": ledgerd_brief},
            {"id": "webhooks-workflow", "slice": "webhooks-workflow",
             "files": ["app/webhooks.py", "app/drafts.py", "app/auth.py"],
             "depends_on": ["ledgerd-core", "skeleton"], "description": WEBHOOKS_OBJECTIVE},
            {"id": goose_swarm::SINK_ID, "files": [], "depends_on": ["ledgerd-core", "webhooks-workflow", "skeleton"],
             "description": "The end-to-end join: boot the whole program."}
        ]})
        .to_string()
    }

    fn description_of(plan: &str, id: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(plan).unwrap();
        v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .and_then(|t| t["description"].as_str())
            .unwrap()
            .to_string()
    }

    /// THE r6h CASE: webhooks-workflow's q5 names `app/webhooks.py` (its own file); ledgerd-core's
    /// real objective names that file once, in its Interface sentence, so the answer lands in
    /// ledgerd-core's description with webhooks-workflow named as the owner and that sentence
    /// quoted — above its decisions block, below its own answers. q6 names BOTH `app/ledger.py`
    /// (ledgerd-core's: YOURS) and `app/webhooks.py` (NOT YOURS) in one item. The asker's own
    /// description, the skeleton's and the sink's are untouched; one `research_answer_routed` per
    /// matched file, `written` equal to the plan path when no repair renamed it; nothing unowned.
    #[test]
    fn r6h_q5_registration_answer_reaches_ledgerd_core_with_webhooks_workflow_named_as_owner() {
        let ledgerd_brief = format!(
            "{LEDGERD_CORE_OBJECTIVE}\n\nANSWERS SETTLED AT PLAN TIME — this slice's research lane \
             derived these:\nQ: [design] health shape\nA: {{\"webhook\": {{\"registered\": <bool>}}}}\
             \n\n{SETTLED_DECISIONS_HEADER} BY RESEARCH that name this slice:\n- D2 …"
        );
        let plan = r6h_plan(&ledgerd_brief);
        let briefs = vec![
            brief("ledgerd-core", LEDGERD_CORE_OBJECTIVE, &ledgerd_brief),
            brief("webhooks-workflow", WEBHOOKS_OBJECTIVE, WEBHOOKS_OBJECTIVE),
        ];
        let rows = vec![
            answered("webhooks-workflow", 5, Q5_QUESTION, Q5_ANSWER),
            answered("webhooks-workflow", 6, Q6_QUESTION, Q6_ANSWER),
        ];
        let sink = ValueSink::default();
        let routed = route_cross_slice_answers(plan.clone(), &briefs, &rows, &sink);
        let core = description_of(&routed, "ledgerd-core");
        let header_at = core.find(ROUTED_ANSWERS_HEADER).expect("the block lands");
        assert!(
            header_at < core.find(SETTLED_DECISIONS_HEADER).unwrap()
                && header_at > core.find("ANSWERS SETTLED AT PLAN TIME").unwrap(),
            "above the decisions partition, below the slice's own answers:\n{core}"
        );
        assert!(
            core.contains(&format!(
                "- webhooks-workflow's research q5: {Q5_QUESTION}\n  NOT YOURS: app/webhooks.py \
                 is webhooks-workflow's in the plan and the answer names it — webhooks-workflow \
                 implements this, not you. Your objective names it here: \"{INTERFACE_SENTENCE}\" \
                 — the only sentence of your objective that names it.\n  A: {Q5_ANSWER}\n  FULL \
                 ANSWER: .swarm/ledger/research-webhooks-workflow-q5.json"
            )),
            "{core}"
        );
        assert!(
            core.contains("- webhooks-workflow's research q6: ")
                && core.contains(
                    "  YOURS: app/ledger.py is yours in the plan; webhooks-workflow's answer \
                     names it and builds against what it says — deliver that surface exactly as \
                     written there.\n  NOT YOURS: app/webhooks.py is webhooks-workflow's in the \
                     plan"
                ),
            "q6 names both files, one item, two ownership lines:\n{core}"
        );
        assert!(core.contains(&format!(
            "{ROUTED_ANSWERS_HEADER} — 2 answers another slice's"
        )));
        assert_eq!(
            description_of(&routed, "webhooks-workflow"),
            WEBHOOKS_OBJECTIVE,
            "the asker is not routed to"
        );
        assert!(!description_of(&routed, "skeleton").contains(ROUTED_ANSWERS_HEADER));
        assert!(!description_of(&routed, goose_swarm::SINK_ID).contains(ROUTED_ANSWERS_HEADER));
        let routed_events = sink.named("research_answer_routed");
        assert_eq!(routed_events.len(), 3, "{routed_events:?}");
        assert_eq!(
            routed_events[0],
            serde_json::json!({"event": "research_answer_routed", "from_slice": "webhooks-workflow",
                "to_task": "ledgerd-core", "q_index": 5, "matched": "file", "value": "app/webhooks.py",
                "written": "app/webhooks.py", "owner": "webhooks-workflow", "arm": "implemented_by_asker"})
        );
        assert_eq!(routed_events[1]["value"], "app/ledger.py");
        assert_eq!(routed_events[1]["arm"], "owned_here");
        assert_eq!(routed_events[1]["owner"], "ledgerd-core");
        assert_eq!(routed_events[2]["value"], "app/webhooks.py");
        assert!(sink.named("research_answer_unowned").is_empty());
        assert!(sink.named("research_answer_unrouted").is_empty());
    }

    /// ORDER (the refuter on d1b0e4f3b): the routing runs AFTER the repairs, so the plan owns
    /// `app/ledgerd/impl.py` while ledgerd-core's answer and webhooks-workflow's objective both
    /// still write the module form `app/ledgerd.py` the opener used. The line carries the REPAIRED
    /// path, names the written form, and quotes webhooks-workflow's own mounting sentence; the
    /// module form is not a stranger; the two files webhooks-workflow owns ride as YOURS.
    #[test]
    fn a_repaired_package_path_is_carried_where_the_answer_wrote_the_module_form() {
        let plan = r6h_plan(LEDGERD_CORE_OBJECTIVE);
        let briefs = vec![
            brief(
                "ledgerd-core",
                LEDGERD_CORE_OBJECTIVE,
                LEDGERD_CORE_OBJECTIVE,
            ),
            brief("webhooks-workflow", WEBHOOKS_OBJECTIVE, WEBHOOKS_OBJECTIVE),
        ];
        let rows = vec![answered(
            "ledgerd-core",
            3,
            "Where are the webhook and drafts routes mounted, and when?",
            "app/ledgerd.py mounts the ROUTES tables that app/webhooks.py and app/drafts.py \
             export; the mount happens at boot, after the bind and before the first sync.",
        )];
        let sink = ValueSink::default();
        let routed = route_cross_slice_answers(plan, &briefs, &rows, &sink);
        let hooks = description_of(&routed, "webhooks-workflow");
        assert!(
            hooks.contains(&format!(
                "  NOT YOURS: app/ledgerd/impl.py (written app/ledgerd.py before the plan repair) \
                 is ledgerd-core's in the plan and the answer names it — ledgerd-core implements \
                 this, not you. Your objective names it here: \"{MOUNTS_SENTENCE}\" — the only \
                 sentence of your objective that names it."
            )),
            "{hooks}"
        );
        assert!(
            !hooks.contains("app/ledgerd.py is ledgerd-core's"),
            "never the stale pre-repair path as the plan's:\n{hooks}"
        );
        assert!(hooks.contains("  YOURS: app/webhooks.py is yours in the plan"));
        assert!(hooks.contains("  YOURS: app/drafts.py is yours in the plan"));
        let ev = sink.named("research_answer_routed");
        assert_eq!(ev.len(), 3, "{ev:?}");
        assert_eq!(ev[0]["value"], "app/ledgerd/impl.py");
        assert_eq!(ev[0]["written"], "app/ledgerd.py");
        assert_eq!(ev[0]["arm"], "implemented_by_asker");
        assert!(sink.named("research_answer_unowned").is_empty());
        assert!(!description_of(&routed, "ledgerd-core").contains(ROUTED_ANSWERS_HEADER));
    }

    /// QUALITY FLOOR + LOUD MISSES: a bare basename another task owns (r6h: `DECISIONS.md`,
    /// console-page's) written in passing routes NOWHERE when the owner's objective does not name
    /// it — `research_answer_unrouted{passing_mention}`, the plan byte-identical; a slash-written
    /// source path no plan file owns is `research_answer_unowned`; the decisions lane's row is the
    /// decisions partition's, never routed here.
    #[test]
    fn a_passing_basename_is_unrouted_and_a_stranger_path_is_named_unowned() {
        let console = "Build the console page as a product, not a debug view. Owns: web/app.js \
                       (page behavior) — the decisions document is written last.";
        let notifier = "Build notifierd as a standalone idempotent consumer. Owns: \
                        app/notifierd/impl.py only.";
        let plan = serde_json::json!({"subtasks": [
            {"id": "console-page", "slice": "console-page", "files": ["web/app.js", "DECISIONS.md"],
             "depends_on": [], "description": console},
            {"id": "notifierd", "slice": "notifierd", "files": ["app/notifierd/impl.py"],
             "depends_on": [], "description": notifier},
        ]})
        .to_string();
        let briefs = vec![
            brief("console-page", console, console),
            brief("notifierd", notifier, notifier),
        ];
        let rows = vec![
            answered(
                "notifierd",
                1,
                "What is the contract between the services?",
                "The run commands, the two database files, the `web/` files and DECISIONS.md are \
                 the contract.",
            ),
            answered(
                "notifierd",
                2,
                "Where does registration live?",
                "Put boot-time registration in app/registration.py (new), read request.md §4; the \
                 vendor URL is http://127.0.0.1:8850/v3/webhooks.",
            ),
            answered(
                DECISION_SLICE,
                0,
                "D2?",
                "Rejected drafts are terminal; DECISIONS.md records it.",
            ),
        ];
        let sink = ValueSink::default();
        let out = route_cross_slice_answers(plan.clone(), &briefs, &rows, &sink);
        assert_eq!(out, plan, "nothing routed => the plan string is untouched");
        assert!(sink.named("research_answer_routed").is_empty());
        assert_eq!(
            sink.named("research_answer_unrouted"),
            vec![
                serde_json::json!({"event": "research_answer_unrouted", "from_slice": "notifierd",
                "q_index": 1, "to_task": "console-page", "file": "DECISIONS.md",
                "reason": "passing_mention"})
            ]
        );
        assert_eq!(
            sink.named("research_answer_unowned"),
            vec![serde_json::json!({"event": "research_answer_unowned",
                "from_slice": "notifierd", "q_index": 2, "names": ["app/registration.py"]})],
            "the stranger is named; request.md (bare) and the URL are not paths"
        );
    }

    /// Whole-path occurrence: a longer path that ends or starts with the plan file is not it;
    /// prose punctuation around it is; the module form exists only for a package's impl file.
    #[test]
    fn find_path_matches_whole_paths_only() {
        assert!(find_path("call `app/api.py`, then", "app/api.py").is_some());
        assert!(find_path("Consequences for app/webhooks.py: (a)", "app/webhooks.py").is_some());
        assert!(find_path("via app/ledger.py's helper", "app/ledger.py").is_some());
        assert!(find_path("in xapp/api.py", "app/api.py").is_none());
        assert!(find_path("in pkg/app/api.py", "app/api.py").is_none());
        assert!(find_path("app/api.pyc and app/api.py/x", "app/api.py").is_none());
        assert_eq!(
            path_positions(LEDGERD_CORE_OBJECTIVE, "app/webhooks.py").len(),
            1
        );
        assert_eq!(
            sentence_naming(LEDGERD_CORE_OBJECTIVE, "app/webhooks.py").as_deref(),
            Some(INTERFACE_SENTENCE)
        );
        assert_eq!(
            sentence_naming(WEBHOOKS_OBJECTIVE, "app/ledgerd.py").as_deref(),
            Some(MOUNTS_SENTENCE)
        );
        assert_eq!(
            module_form("app/ledgerd/impl.py").as_deref(),
            Some("app/ledgerd.py")
        );
        assert_eq!(module_form("app/ledger.py"), None);
        assert_eq!(module_form("impl.py"), None);
        assert_eq!(
            path_tokens("see ./app/x.py, web/viz.js; not /web/app.js, http://h/a/b.py, app/<m>.py, request.md or txn id/part/of"),
            vec!["app/x.py".to_string(), "web/viz.js".to_string()]
        );
    }
}
