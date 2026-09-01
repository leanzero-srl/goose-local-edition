//! THE OPENER'S CONTRACT: the slice and reply shapes the OPEN call returns (`OpenSlice`,
//! `OpenOutput`), the JSON schema it is held to (`open_schema`), and the parse-time
//! qualification of its open decisions (`OpenOutputRaw::qualify`). Sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases): the shapes and
//! the schema moved verbatim from swarm.rs (visibility only), paying for the decision contract
//! and its parse-time gate in the same commit.
//!
//! WHY the gate (r6d first tick, run swarm-20260901-035310576, seq 91-92): the opener listed
//! three "open decisions" as bare sentences with `options: []` — an HTTP-framework choice the
//! request settles ("standard library only"), a token-entry question, and "D1/D2/D3 ... must be
//! decided and shipped in DECISIONS.md, not deferred to a human" — an instruction to itself.
//! All three went to ASK (`low_confidence_ask`, questions 3), the 5s window folded with "no
//! answers arrived; the fleet idled for the whole window", and each then cost a research lane
//! (`research_planned.per_slice.__open_decisions__: 3`). A decision is a QUESTION with at least
//! two concrete options and the request's words that leave it open; anything else is measured,
//! named (`decision_self_resolved`, MILD — never a stop) and kept out of ASK and the fan.
//!
//! THE QUESTION CONTRACT (the fan cut, r6d): a slice question is an OBJECT with a `kind` —
//! `spec_lookup` | `design` | `external` — and, for a lookup the opener settled by reading the
//! request, the `fact` and its `cite`. r6d dispatched 27 questions over 165 minutes on 3 nodes
//! and 13 of them were answerable by one grep of request.md (ledger-api-q1 "which sort keys does
//! sort accept" — request.md:148 lists the four; drafts-workflow-q0 "the tokens-file shape" —
//! request.md:51 shows it), each costing a 15-minute lane. A cited fact is not a question: the
//! engine renders it into the brief as a SPEC FACT and no lane runs. A question that arrives in
//! the old bare-string shape, or with a kind the contract does not name, is UNKINDED — it is
//! dispatched exactly as before (the contract miss costs nothing but a lane) and named by
//! `research_question_unkinded` so the miss is visible.

use super::EventSink;

/// One semantic slice of the request, as the opener sees it.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct OpenSlice {
    pub(super) id: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) objective: String,
    /// The slice's questions, in the opener's order — the position is `q_index`, the identity
    /// the ledger mini, the activity key and the brief partition share.
    #[serde(default)]
    pub(super) questions: Vec<OpenQuestion>,
    /// The opener's OWN estimate of how much work this slice is, 1-5. Not truth — a model estimate,
    /// used only to notice a lopsided split and ask for one more cut. Independent machines pick these
    /// up in parallel, so a slice twice the size of its siblings is a node idling while one grinds.
    #[serde(default)]
    pub(super) weight: u32,
    /// OPEN-1: the spec section HEADINGS this slice owns, claimed by the opener against the
    /// orientation index. The engine splices each claimed section's full text into the slice's
    /// brief verbatim. Empty on a small spec (orientation not armed) — everything then behaves
    /// exactly as before this field existed.
    #[serde(default)]
    pub(super) sections: Vec<String>,
}

/// What kind of question the opener says it is. `Unkinded` is not a kind the opener may choose:
/// it is the parse-time reading of the old bare-string shape or of a `kind` the contract does not
/// name, kept dispatchable (treated as `design`) and counted by `research_question_unkinded`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuestionKind {
    /// The request's own text answers it. With a `cite` AND a `fact` it is a SPEC FACT — no lane
    /// runs; without a fact the opener searched and did not find it, and a lane looks.
    SpecLookup,
    /// The request leaves it open; a builder must choose a convention.
    Design,
    /// Needs the vendor's documentation or another source outside the request.
    External,
    Unkinded,
}

impl QuestionKind {
    /// Lenient on decoration (case, `-`/` ` for `_`), strict on vocabulary: only the three names
    /// the schema enumerates resolve; anything else is `Unkinded`, never a guess at what the
    /// model meant.
    fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().replace(['-', ' '], "_").as_str() {
            "spec_lookup" => Self::SpecLookup,
            "design" => Self::Design,
            "external" => Self::External,
            _ => Self::Unkinded,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SpecLookup => "spec_lookup",
            Self::Design => "design",
            Self::External => "external",
            Self::Unkinded => "unkinded",
        }
    }
}

/// One slice question as the run consumes it. `text` is the question verbatim (whitespace
/// squashed to one line — the identity the mini, the brief and the dedup all read); `cite` is the
/// request line or heading the opener read (`request.md:148`, or a heading), empty when it named
/// none; `fact` is the answer in the request's words, empty unless the opener found one.
#[derive(Clone, Debug)]
pub(crate) struct OpenQuestion {
    pub(crate) text: String,
    pub(crate) kind: QuestionKind,
    pub(crate) cite: String,
    pub(crate) fact: String,
}

impl OpenQuestion {
    /// A SPEC FACT: a lookup the opener settled by reading the request — both the fact and where
    /// it read it. Either half missing and this is a question again: a fact without a cite is an
    /// uncheckable claim, a cite without a fact is a search that found nothing.
    pub(crate) fn is_cited_fact(&self) -> bool {
        self.kind == QuestionKind::SpecLookup && !self.cite.is_empty() && !self.fact.is_empty()
    }
}

/// Test fixtures and the pre-contract call sites build a plain question; it is a `design`
/// question (dispatched), never `Unkinded` — the unkinded reading belongs to the deserializer
/// alone, so a `research_question_unkinded` event always means the MODEL missed the contract.
impl From<&str> for OpenQuestion {
    fn from(text: &str) -> Self {
        Self {
            text: squash(text),
            kind: QuestionKind::Design,
            cite: String::new(),
            fact: String::new(),
        }
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One question as the opener EMITTED it — the schema's object, a bare string from a model that
/// ignored the schema (every pre-cut opener's shape), or anything else (kept parseable so one odd
/// entry cannot fail the whole opener — the `OpenDecisionRaw` lesson).
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum OpenQuestionRaw {
    Framed {
        #[serde(default)]
        question: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        cite: String,
        #[serde(default)]
        fact: String,
    },
    Bare(String),
    Other(serde_json::Value),
}

impl<'de> serde::Deserialize<'de> for OpenQuestion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match OpenQuestionRaw::deserialize(d)? {
            OpenQuestionRaw::Framed {
                question,
                kind,
                cite,
                fact,
            } => OpenQuestion {
                text: squash(&question),
                kind: QuestionKind::parse(&kind),
                cite: squash(&cite),
                fact: fact.trim().to_string(),
            },
            OpenQuestionRaw::Bare(q) => OpenQuestion {
                text: squash(&q),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                fact: String::new(),
            },
            OpenQuestionRaw::Other(v) => OpenQuestion {
                text: squash(&v.to_string()),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                fact: String::new(),
            },
        })
    }
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
/// task ids, no requirement map. An open decision is an OBJECT — `question`, `options` (two or
/// more), `cite` — never a bare sentence: r6d's opener emitted three strings with no options,
/// one of them an instruction to itself, and the ask window was spent on them.
pub(super) fn open_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["slices"],
        "properties": {
            "slices": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "title", "objective", "questions", "weight"],
                    "properties": {
                        "id": {"type": "string"},
                        "title": {"type": "string"},
                        "objective": {"type": "string"},
                        "questions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["question", "kind"],
                                "properties": {
                                    "question": {"type": "string"},
                                    "kind": {"type": "string", "enum": ["spec_lookup", "design", "external"]},
                                    "cite": {"type": "string"},
                                    "fact": {"type": "string"}
                                }
                            }
                        },
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

/// The opener's reply as it ARRIVES — slices as-is, decisions raw. `qualify` is the one door to
/// `OpenOutput`.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub(crate) struct OpenOutputRaw {
    #[serde(default)]
    pub(super) slices: Vec<OpenSlice>,
    #[serde(default)]
    pub(super) open_decisions: Vec<OpenDecisionRaw>,
}

impl OpenOutputRaw {
    pub(super) fn qualify(self, events: &dyn EventSink) -> OpenOutput {
        OpenOutput {
            slices: qualify_slice_questions(self.slices, events),
            open_decisions: qualify_open_decisions(self.open_decisions, events),
        }
    }
}

/// The parse-time reading of every slice question. An entry with no text is nothing to research
/// and nothing to brief — dropped, named (`research_question_empty`, with the opener's own
/// position so the transcript can be checked), so the surviving positions ARE the q_indexes
/// every downstream identity uses. An unkinded entry (bare string, unknown `kind`) survives
/// unchanged and dispatches as a design question — the miss costs a lane, never an answer — and
/// is named by `research_question_unkinded` with its words, so the contract miss is visible on
/// the tick instead of silently paid.
fn qualify_slice_questions(mut slices: Vec<OpenSlice>, events: &dyn EventSink) -> Vec<OpenSlice> {
    for sl in &mut slices {
        let raw = std::mem::take(&mut sl.questions);
        for (position, q) in raw.into_iter().enumerate() {
            if q.text.is_empty() {
                events.write_value(serde_json::json!({
                    "event": "research_question_empty",
                    "slice": sl.id,
                    "position": position,
                }));
                continue;
            }
            if q.kind == QuestionKind::Unkinded {
                events.write_value(serde_json::json!({
                    "event": "research_question_unkinded",
                    "slice": sl.id,
                    "q_index": sl.questions.len(),
                    "question": q.text.chars().take(200).collect::<String>(),
                }));
            }
            sl.questions.push(q);
        }
    }
    slices
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

    /// THE QUESTION CONTRACT on r6d's own questions. The schema demands `question` + `kind`
    /// (three names); the parse reads the framed shape into kind/cite/fact, reads r6d's actual
    /// shape (bare strings) as UNKINDED — dispatched as design, each named once with its words —
    /// and drops an empty entry by name so the surviving positions are the q_indexes.
    #[test]
    fn a_question_arrives_kinded_and_the_old_bare_shape_is_named_unkinded() {
        let schema = open_schema();
        let q = &schema["properties"]["slices"]["items"]["properties"]["questions"]["items"];
        assert_eq!(q["type"], "object");
        assert_eq!(q["required"], serde_json::json!(["question", "kind"]));
        assert_eq!(
            q["properties"]["kind"]["enum"],
            serde_json::json!(["spec_lookup", "design", "external"])
        );

        // r6d ledger-api-q1 as a cited fact (request.md:148 lists the four sort keys), r6d
        // ledger-core-q1 as an external question, notifierd-q2 as a design one, one lookup the
        // opener searched for and did not find, one bare string (r6d's real shape), one stray
        // value, one empty object.
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [{
                "id": "ledger-api", "title": "t", "objective": "o", "weight": 3,
                "questions": [
                    {"question": "Which sort keys does sort=<k> accept and in what direction(s)?",
                     "kind": "spec_lookup", "cite": "request.md:148",
                     "fact": "`sort` is one of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`; default `created_at` (ascending by INSTANT)."},
                    {"question": "What cursor state from the vendor's paginated list is persisted across a dropped connection?",
                     "kind": "External"},
                    {"question": "What durability/locking strategy for notify.db (WAL? single writer?)",
                     "kind": "design"},
                    {"question": "Which header verifies signed webhooks?", "kind": "spec-lookup",
                     "cite": "grep -n -i signature request.md"},
                    "Static hosting: which content types (html/css/js/ico) and any cache headers?",
                    7,
                    {}
                ]
            }]
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        let qs = &out.slices[0].questions;
        assert_eq!(qs.len(), 6, "the empty object is dropped: {qs:?}");
        assert_eq!(qs[0].kind, QuestionKind::SpecLookup);
        assert!(qs[0].is_cited_fact());
        assert_eq!(qs[0].cite, "request.md:148");
        assert!(qs[0].fact.starts_with("`sort` is one of"));
        assert_eq!(qs[1].kind, QuestionKind::External, "case folds");
        assert_eq!(qs[2].kind, QuestionKind::Design);
        assert_eq!(
            qs[3].kind,
            QuestionKind::SpecLookup,
            "dash folds to underscore"
        );
        assert!(
            !qs[3].is_cited_fact(),
            "a lookup that found nothing is a question again, not a fact"
        );
        assert_eq!(qs[4].kind, QuestionKind::Unkinded);
        assert!(qs[4].text.starts_with("Static hosting"));
        assert_eq!(qs[5].kind, QuestionKind::Unkinded);
        assert_eq!(qs[5].text, "7");
        let events = sink.0.lock().unwrap();
        let names: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "research_question_unkinded",
                "research_question_unkinded",
                "research_question_empty"
            ],
            "{events:?}"
        );
        assert_eq!(events[0]["slice"], "ledger-api");
        assert_eq!(
            events[0]["q_index"], 4,
            "the q_index AFTER the drop, the ledger's identity"
        );
        assert!(events[0]["question"]
            .as_str()
            .unwrap()
            .starts_with("Static hosting"));
        assert_eq!(events[1]["q_index"], 5);
        assert_eq!(
            events[2]["position"], 6,
            "the opener's own position of the empty entry"
        );
        // A fixture question is a design question: never counted as a model's contract miss.
        let plain = OpenQuestion::from("which port");
        assert_eq!(plain.kind, QuestionKind::Design);
        assert!(!plain.is_cited_fact());
    }
}
