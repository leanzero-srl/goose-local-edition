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

use super::EventSink;

/// One semantic slice of the request, as the opener sees it.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct OpenSlice {
    pub(super) id: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) objective: String,
    #[serde(default)]
    pub(super) questions: Vec<String>,
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

/// The opener's reply as the run consumes it: `open_decisions` holds only QUALIFIED decisions
/// (a question, two or more options, the request's words that leave it open — rendered as one
/// line, which is the decision's identity through ASK, the fan and the briefs). Built from
/// `OpenOutputRaw::qualify`, never deserialised directly.
#[derive(Clone, Debug, Default)]
pub(crate) struct OpenOutput {
    pub(super) slices: Vec<OpenSlice>,
    pub(super) open_decisions: Vec<String>,
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
                        "questions": {"type": "array", "items": {"type": "string"}},
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
            slices: self.slices,
            open_decisions: qualify_open_decisions(self.open_decisions, events),
        }
    }
}

/// The parse-time gate: an entry with fewer than two concrete options is not a decision — the
/// request or the opener already decides it — so it is named in a `decision_self_resolved`
/// event (the text verbatim, so the tick can read what was dropped) and goes neither to ASK
/// nor to the fan. A qualified decision renders as ONE line — question, options, citation —
/// which is its identity everywhere downstream (`Q:` lines, the fan's question, the briefs).
pub(super) fn qualify_open_decisions(
    raw: Vec<OpenDecisionRaw>,
    events: &dyn EventSink,
) -> Vec<String> {
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
        out.push(render_open_decision(&question, &options, cite.trim()));
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
            out.open_decisions[0].starts_with("HTTP framework for ledgerd/notifierd:"),
            "{}",
            out.open_decisions[0]
        );
        assert!(
            out.open_decisions[0]
                .contains("— options: stdlib http.server (threaded) | Flask | FastAPI"),
            "{}",
            out.open_decisions[0]
        );
        assert!(
            out.open_decisions[0].ends_with("(the request leaves it open: p95 <150ms under load)"),
            "{}",
            out.open_decisions[0]
        );
        assert!(
            !out.open_decisions[1].contains("leaves it open"),
            "no cite, no clause"
        );
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
}
