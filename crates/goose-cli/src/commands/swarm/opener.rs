//! THE OPENER'S CONTRACT: the slice and reply shapes the OPEN call returns (`OpenSlice`,
//! `OpenOutput`), the JSON schema it is held to (`open_schema`), and the parse-time
//! qualification of its open decisions (`OpenOutputRaw::qualify`). Sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases).
//!
//! THE OPENER SLICES; THE LANES RESEARCH (VA-089). A slice is an id, a title, an objective (its
//! owned files declared inside it), a weight and the spec sections it owns — nothing else. The
//! per-slice QUESTIONS the opener used to write are gone from the contract. MEASURED on four
//! runs (r6e/r6f/r6g/r6h): OPEN was one lane on one node for 46 / 71 / 61 / ~66 minutes while two
//! nodes idled, and the emitted bytes were NOT the cost — r6h's opener emitted cite-only lookups
//! and still took ~66 minutes because the single lane REASONED per question (169k chars of
//! "What do request.md:44-46 fix for database file ownership and cross-service isolation?" …
//! "What do request.md:75-79 fix for vendor pagination …", each verified against the spec text)
//! — while r6g's RESEARCH answered 7 questions on 4 parallel lanes in 15 minutes. The question
//! work now happens where the parallelism is: each research lane (one per slice, always —
//! `research.rs`) reads its slice's sections and DERIVES its own design/external questions, then
//! answers them in the same session. A legacy emit that still carries `questions` is not
//! refused (a refusal re-streams the whole emit): the entries are dropped and named once per
//! slice (`research_question_ignored{slice, count}`).
//!
//! WHY the decision gate (r6d first tick, run swarm-20260901-035310576, seq 91-92): the opener
//! listed three "open decisions" as bare sentences with `options: []` — an HTTP-framework choice
//! the request settles ("standard library only"), a token-entry question, and "D1/D2/D3 ... must
//! be decided and shipped in DECISIONS.md, not deferred to a human" — an instruction to itself.
//! All three went to ASK (`low_confidence_ask`, questions 3), the 5s window folded with "no
//! answers arrived; the fleet idled for the whole window", and each then cost a research lane.
//! A decision is a QUESTION with at least two concrete options and the request's words that
//! leave it open; anything else is measured, named (`decision_self_resolved`, MILD — never a
//! stop) and kept out of ASK and the fan.

use super::EventSink;

/// One semantic slice of the request, as the run consumes it. Built from `OpenSliceRaw` by
/// `OpenOutputRaw::qualify`, never deserialised directly.
#[derive(Clone, Debug)]
pub(crate) struct OpenSlice {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) objective: String,
    /// The opener's OWN estimate of how much work this slice is, 1-5. Not truth — a model estimate,
    /// used only to notice a lopsided split and ask for one more cut. Independent machines pick these
    /// up in parallel, so a slice twice the size of its siblings is a node idling while one grinds.
    pub(super) weight: u32,
    /// OPEN-1: the spec section HEADINGS this slice owns, claimed by the opener against the
    /// orientation index. The engine splices each claimed section's full text into the slice's
    /// brief verbatim, and its research lane reads the same text to derive its questions. Empty
    /// on a small spec (orientation not armed) — everything then behaves exactly as before this
    /// field existed.
    pub(super) sections: Vec<String>,
}

/// One slice as the opener EMITTED it. `questions` is read ONLY to measure a model still
/// writing the retired per-slice questions (VA-089); the entries are never kept — a research
/// lane derives its own from the slice's sections.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct OpenSliceRaw {
    pub(super) id: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) objective: String,
    #[serde(default)]
    pub(super) weight: u32,
    #[serde(default)]
    pub(super) sections: Vec<String>,
    #[serde(default)]
    pub(super) questions: Vec<serde_json::Value>,
}

/// The opener's reply as the run consumes it: `open_decisions` holds only QUALIFIED decisions
/// (a question, two or more options, the request's words that leave it open — rendered as one
/// line, which is the decision's identity through ASK, the fan and the briefs). Built from
/// `OpenOutputRaw::qualify`, never deserialised directly.
#[derive(Clone, Debug, Default)]
pub(crate) struct OpenOutput {
    pub(super) slices: Vec<OpenSlice>,
    pub(super) open_decisions: Vec<OpenDecision>,
}

/// One QUALIFIED open decision: `line` is its rendered identity everywhere downstream (the ASK
/// question text, the user's `Q:` match, the fan's question, the briefs); `options` are the two
/// or more concrete choices the opener named, carried STRUCTURED so the ASK payload
/// (`clarify-questions.json`, `low_confidence_ask`, the desktop clarify card) offers them as
/// one-click answers — r6d seq 91 shipped `options: []` on every question while the choices sat
/// inside the rendered line (r6e E10).
#[derive(Clone, Debug)]
pub(crate) struct OpenDecision {
    pub(super) line: String,
    pub(super) options: Vec<String>,
}

impl OpenOutput {
    /// The decisions' rendered lines, for the consumers whose identity IS the line
    /// (`still_open_after_user`, `partition_decisions`, the ask event's not-asked diff).
    pub(super) fn decision_lines(&self) -> Vec<String> {
        self.open_decisions.iter().map(|d| d.line.clone()).collect()
    }
}

/// The opener's contract, deliberately small: no files FIELD (owned files are declared inside the
/// objective text — synthesis infers each task's paths from its slice's objective), no deps, no
/// task ids, no requirement map, and — since VA-089 — no questions: the research lanes derive
/// those from the sections each slice claims. An open decision is an OBJECT — `question`,
/// `options` (two or more), `cite` — never a bare sentence: r6d's opener emitted three strings
/// with no options, one of them an instruction to itself, and the ask window was spent on them.
pub(super) fn open_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["slices"],
        "properties": {
            "slices": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "title", "objective", "weight"],
                    "properties": {
                        "id": {"type": "string"},
                        "title": {"type": "string"},
                        "objective": {"type": "string"},
                        "weight": {"type": "integer"},
                        "sections": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "open_decisions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["question", "options"],
                    "properties": {
                        "question": {"type": "string"},
                        "options": {"type": "array", "items": {"type": "string"}},
                        "cite": {"type": "string"}
                    }
                }
            }
        }
    })
}

/// One open decision as the opener EMITTED it — the schema's object, or a bare string from a
/// model that ignored the schema (r6d's shape), or anything else (kept parseable so one odd
/// entry cannot fail the whole opener).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum OpenDecisionRaw {
    Framed {
        #[serde(default)]
        question: String,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        cite: String,
    },
    Bare(String),
    Other(serde_json::Value),
}

/// The opener's reply as it ARRIVES — slices and decisions raw. `qualify` is the one door to
/// `OpenOutput`.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub(crate) struct OpenOutputRaw {
    #[serde(default)]
    pub(super) slices: Vec<OpenSliceRaw>,
    #[serde(default)]
    pub(super) open_decisions: Vec<OpenDecisionRaw>,
}

impl OpenOutputRaw {
    pub(super) fn qualify(self, events: &dyn EventSink) -> OpenOutput {
        OpenOutput {
            slices: qualify_slices(self.slices, events),
            open_decisions: qualify_open_decisions(self.open_decisions, events),
        }
    }
}

/// VA-089: the contract has no per-slice questions — a research lane derives its own from the
/// slice's sections. A model that writes them anyway spent its serial emit on work the parallel
/// lanes now do; the entries are dropped and named ONCE per slice with their count, never
/// refused (a refusal re-streams the whole emit) and never carried.
fn qualify_slices(raw: Vec<OpenSliceRaw>, events: &dyn EventSink) -> Vec<OpenSlice> {
    raw.into_iter()
        .map(|sl| {
            if !sl.questions.is_empty() {
                events.write_value(serde_json::json!({
                    "event": "research_question_ignored",
                    "slice": sl.id,
                    "count": sl.questions.len(),
                }));
            }
            OpenSlice {
                id: sl.id,
                title: sl.title,
                objective: sl.objective,
                weight: sl.weight,
                sections: sl.sections,
            }
        })
        .collect()
}

/// The parse-time gate: an entry with fewer than two concrete options is not a decision — the
/// request or the opener already decides it — so it is named in a `decision_self_resolved`
/// event (the text verbatim, so the tick can read what was dropped) and goes neither to ASK
/// nor to the fan. A qualified decision renders as ONE line — question, options, citation —
/// which is its identity everywhere downstream (`Q:` lines, the fan's question, the briefs).
pub(super) fn qualify_open_decisions(
    raw: Vec<OpenDecisionRaw>,
    events: &dyn EventSink,
) -> Vec<OpenDecision> {
    let mut out = Vec::new();
    for entry in raw {
        // `miss` is the schema-miss shape when the entry was not the framed object.
        let (question, options, cite, miss) = match entry {
            OpenDecisionRaw::Framed {
                question,
                options,
                cite,
            } => (question, options, cite, None),
            OpenDecisionRaw::Bare(q) => (q, Vec::new(), String::new(), Some("bare_string")),
            OpenDecisionRaw::Other(v) => (
                v.to_string(),
                Vec::new(),
                String::new(),
                Some("other_value"),
            ),
        };
        let question = question.split_whitespace().collect::<Vec<_>>().join(" ");
        let options: Vec<String> = options
            .iter()
            .map(|o| o.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|o| !o.is_empty())
            .collect();
        if options.len() < 2 {
            // THE SHAPE NAMES WHOSE MISS IT IS (r6e refuter E13): a bare string or a stray value
            // is the MODEL ignoring the schema — a question may be hiding in it — while a framed
            // entry with fewer than two options is the opener saying the request decides it.
            // Both are dropped from ASK and the fan (the downgrade is the same); the tick must
            // not read the first as "self-resolved".
            let shape = match (miss, options.len()) {
                (Some(m), _) => m,
                (None, 0) => "framed_no_options",
                (None, _) => "framed_one_option",
            };
            let reason = if miss.is_none() {
                "fewer than two options — the request or the opener already decides it; not \
                 asked, not researched"
            } else {
                "schema miss — not a {question, options, cite} object, so no options exist to \
                 ask; not asked, not researched (a model miss, not a resolved decision)"
            };
            events.write_value(serde_json::json!({
                "event": "decision_self_resolved",
                "question": question,
                "options": options,
                "shape": shape,
                "reason": reason,
            }));
            continue;
        }
        out.push(OpenDecision {
            line: render_open_decision(&question, &options, cite.trim()),
            options,
        });
    }
    out
}

fn render_open_decision(question: &str, options: &[String], cite: &str) -> String {
    let mut line = format!("{question} — options: {}", options.join(" | "));
    if !cite.is_empty() {
        let cite = cite.split_whitespace().collect::<Vec<_>>().join(" ");
        line.push_str(&format!(" (the request leaves it open: {cite})"));
    }
    line
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

    /// r6d's three entries, verbatim (low_confidence_ask, seq 91), in the shapes the new schema
    /// yields: the two that are questions with options qualify; the third — an instruction the
    /// opener wrote to itself, no options — is named in `decision_self_resolved` and dropped.
    /// The bare-string shape r6d actually emitted is the same downgrade.
    #[test]
    fn an_entry_without_two_options_is_named_and_kept_out_of_ask() {
        let http = "HTTP framework for ledgerd/notifierd: the spec fixes behavior (endpoints, envelopes, \
                    10s boot, p95 <150ms under load) but not the server — stdlib http.server (threaded) \
                    vs Flask vs FastAPI; this choice affects concurrency under the graded load.";
        let tokens = "How the browser obtains the three bearer tokens for drafts endpoints: the spec \
                      defines no auth UI, so token entry (prompt field, config, or hardcoded dev tokens \
                      in the page) must be chosen by a human.";
        let d123 = "D1/D2/D3 are deliberately left open by the spec but are ASSIGNED decisions — they \
                    must be decided and shipped in DECISIONS.md (## D1 brush vs streamed mutation, ## D2 \
                    rejected-draft terminality, ## D3 pre-sync table state), not deferred to a human.";
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [],
            "open_decisions": [
                {"question": http, "options": ["stdlib http.server (threaded)", "Flask", "FastAPI"],
                 "cite": "p95 <150ms under load"},
                {"question": tokens, "options": ["prompt field", "config", "hardcoded dev tokens in the page"]},
                {"question": d123, "options": []},
            ]
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        assert_eq!(out.open_decisions.len(), 2, "{:?}", out.open_decisions);
        assert!(
            out.open_decisions[0]
                .line
                .starts_with("HTTP framework for ledgerd/notifierd:"),
            "{:?}",
            out.open_decisions[0]
        );
        assert!(
            out.open_decisions[0]
                .line
                .contains("— options: stdlib http.server (threaded) | Flask | FastAPI"),
            "{:?}",
            out.open_decisions[0]
        );
        assert!(
            out.open_decisions[0]
                .line
                .ends_with("(the request leaves it open: p95 <150ms under load)"),
            "{:?}",
            out.open_decisions[0]
        );
        assert!(
            !out.open_decisions[1].line.contains("leaves it open"),
            "no cite, no clause"
        );
        // E10: the options ride STRUCTURED beside the line, in the opener's order.
        assert_eq!(
            out.open_decisions[0].options,
            vec!["stdlib http.server (threaded)", "Flask", "FastAPI"]
        );
        assert_eq!(
            out.open_decisions[1].options,
            vec!["prompt field", "config", "hardcoded dev tokens in the page"]
        );
        assert_eq!(out.decision_lines()[0], out.open_decisions[0].line);
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "decision_self_resolved");
        assert_eq!(events[0]["question"], d123);
        assert_eq!(events[0]["options"], serde_json::json!([]));
        assert_eq!(events[0]["shape"], "framed_no_options");
        assert!(events[0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("fewer than two options"));

        // The shape r6d actually emitted — bare strings, no options anywhere — is three downgrades
        // and an empty ASK; one option is still not a choice; an odd value cannot fail the parse.
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [],
            "open_decisions": [http, tokens, d123, {"question": "one?", "options": ["only"]}, 7]
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        assert!(out.open_decisions.is_empty(), "{:?}", out.open_decisions);
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 5);
        // E13: the shape says whose miss it is — three schema misses, one real one-option entry,
        // one stray value — so the tick's DECISIONS SELF-RESOLVED row cannot launder a model
        // miss as a resolved decision.
        let shapes: Vec<&str> = events
            .iter()
            .map(|e| e["shape"].as_str().unwrap())
            .collect();
        assert_eq!(
            shapes,
            vec![
                "bare_string",
                "bare_string",
                "bare_string",
                "framed_one_option",
                "other_value"
            ]
        );
        assert!(events[0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("schema miss"));
        assert!(events[3]["reason"]
            .as_str()
            .unwrap()
            .starts_with("fewer than two options"));
    }

    /// The schema holds the opener to the object shape: `question` and `options` are required.
    #[test]
    fn the_open_schema_requires_options_on_every_decision() {
        let schema = open_schema();
        let item = &schema["properties"]["open_decisions"]["items"];
        assert_eq!(item["type"], "object");
        assert_eq!(item["required"], serde_json::json!(["question", "options"]));
        assert_eq!(item["properties"]["options"]["type"], "array");
    }

    /// VA-089: the opener emits SLICES ONLY — the schema names no `questions` and requires none;
    /// a slice parses with id, title, objective, weight and sections and nothing else is kept.
    /// r6h's opener shape (per-slice `questions` objects with kind and cite) still parses — the
    /// entries are dropped and named ONCE per slice with their count
    /// (`research_question_ignored`), never refused; a slice without them names nothing.
    #[test]
    fn the_opener_emits_slices_only_and_legacy_questions_are_dropped_and_named() {
        let schema = open_schema();
        let slice = &schema["properties"]["slices"]["items"];
        assert_eq!(
            slice["required"],
            serde_json::json!(["id", "title", "objective", "weight"])
        );
        assert!(
            slice["properties"].get("questions").is_none(),
            "no per-slice questions in the contract: {slice}"
        );
        assert!(slice["properties"].get("sections").is_some());
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [
                {"id": "ledger-api", "title": "the api", "objective": "Owns `app/ledgerd/api.py`.",
                 "weight": 3, "sections": ["Endpoints", "Errors"],
                 "questions": [
                    {"question": "What do request.md:131-142 fix for the /api/health shape?",
                     "kind": "spec_lookup", "cite": "request.md:131-142"},
                    {"question": "Which journal mode for ledger.db?", "kind": "design",
                     "cite": "request.md:77 'SQLite'; grep -n -i 'wal' → no match"},
                    "a bare string"
                 ]},
                {"id": "web-page", "title": "the page", "objective": "Owns `web/app.js`.",
                 "weight": 2, "sections": ["Web console"]}
            ],
            "open_decisions": []
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        assert_eq!(out.slices.len(), 2);
        assert_eq!(out.slices[0].id, "ledger-api");
        assert_eq!(out.slices[0].sections, vec!["Endpoints", "Errors"]);
        assert_eq!(out.slices[0].weight, 3);
        assert_eq!(out.slices[1].objective, "Owns `web/app.js`.");
        let events = sink.0.lock().unwrap();
        assert_eq!(
            *events,
            vec![serde_json::json!({
                "event": "research_question_ignored",
                "slice": "ledger-api",
                "count": 3,
            })],
            "one event for the slice that carried legacy questions, none for the one that did not"
        );
        // A slice without an id is not a slice: the whole reply fails to parse, as before.
        assert!(serde_json::from_value::<OpenOutputRaw>(serde_json::json!({
            "slices": [{"title": "no id"}]
        }))
        .is_err());
    }
}
