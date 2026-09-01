//! THE FAN CUT's routing (C2): a slice question that IS one of the opener's open decisions rides
//! the decisions lane — decided ONCE — and a question an already-landed mini answers is COVERED
//! by that mini instead of re-asked. Sibling module under the incremental-split law.
//!
//! WHY (r6d, archive local-sb7-swarm-r6d-KILLED-no-chance-vs-r5-fan-38q-research-165m-…):
//! D1 was decided three times — `__open_decisions__-q2` (the opener's "D1/D2/D3 … must be
//! decided" line, 19.2 min), `web-page-q1` ("D1: does the brush survive a streamed mutation",
//! dispatched) and inside `web-page-q3` (the brush interface) — and `web-page-q0` re-asked the
//! opener's own token-entry decision word for word (7.9 min). `drafts-workflow-q4` and
//! `ledger-api-q5` both asked what request.md:218 states about `/api/events` (8.1 + 21.5 min).
//!
//! THE MATCH IS STRICT, AND MILD: three rules, each explainable in one line, and a question that
//! matches none DISPATCHES — when in doubt, research it. Calibrated on r6d's 29 real texts
//! (`same_question` tests): the one true lexical pair (web-page-q0 vs the tokens decision)
//! scores Jaccard 0.57 with 13 shared content words; the best FALSE pair (ledger-api-q1 vs
//! web-page-q5, "sort / filters / keys") scores 0.20 with 3. The floors sit at 0.5 and 6.
//! drafts-q4 vs api-q5 score 0.10 — they are the same question by MEANING, not by words, and
//! the rule that catches them is the shared cite (C1 makes both cited facts anyway).

use std::collections::BTreeSet;

use super::opener::{OpenOutput, QuestionKind};
use super::research::{ResearchQuestion, ResearchRow, RESEARCH_ANSWERED};
use super::EventSink;

/// Content-word Jaccard floor and shared-word floor for the stem rule — r6d's true pair 0.57/13,
/// best false pair 0.20/3 (module doc).
const STEM_JACCARD_FLOOR: f64 = 0.5;
const STEM_SHARED_FLOOR: usize = 6;
/// The corroboration a shared CITE needs before it dedups (VA-030 D10-6): half the stem floor
/// in shared content words. Cite equality alone read two different failed lookups that "put
/// what you searched in cite" as one question, two design questions citing one heading as one,
/// and a design question citing request.md:148 beside the api-q1 fact row as answered by it —
/// `covering_mini` then COPIED the other's answer. A cite-only pair is named
/// (`research_question_cite_only`) and dispatched.
const CITE_SHARED_FLOOR: usize = STEM_SHARED_FLOOR / 2;

/// The request's own decision ids as they appear in questions — `D1`, `D2:`, `D1/D2/D3` — the
/// spec's "## D1 / ## D2 / ## D3" vocabulary (request.md §9). Uppercase D followed by digits,
/// not inside a longer word (`D1` yes, `ID1` no, `D1x` no).
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

/// Function words that carry no question identity. A linguistic constant, not a tuning knob.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "for", "and", "or", "is", "are", "be", "it", "its",
    "this", "that", "what", "which", "how", "does", "do", "any", "at", "as", "by", "with", "from",
    "into", "per", "vs", "so", "must", "can", "if", "than", "then", "they", "their", "there",
    "these", "those", "when", "where", "who", "whom", "will", "would", "should", "not", "no",
    "yes", "all", "each", "every", "one", "two", "three", "via", "over", "under", "about", "after",
    "before", "between", "while", "both", "either", "neither", "exactly", "exact", "also", "only",
];

/// The words a question's identity rests on: lowercased runs of `[a-z0-9_/#.-]` (so `/api/events`
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

fn normalize_cite(cite: &str) -> String {
    cite.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Are two questions the SAME question? `Some(rule)` names the one rule that matched — `cite`
/// (both cite the same request line/heading AND share at least `CITE_SHARED_FLOOR` content
/// words — the words corroborate the line), `decision_id` (both name the same D-id), `stem`
/// (content-word overlap past both floors) — or `None`: dispatch it. Order is by strength;
/// the rule name rides the `research_question_covered` event so the tick can read WHY. A pair
/// that shares only its cite is `cite_only` — named, never deduped.
pub(super) fn same_question(
    a_text: &str,
    a_cite: &str,
    b_text: &str,
    b_cite: &str,
) -> Option<&'static str> {
    let (wa, wb) = (content_words(a_text), content_words(b_text));
    let shared = wa.intersection(&wb).count();
    if same_cite(a_cite, b_cite) && shared >= CITE_SHARED_FLOOR {
        return Some("cite");
    }
    if !decision_ids(a_text).is_disjoint(&decision_ids(b_text)) {
        return Some("decision_id");
    }
    let union = wa.union(&wb).count();
    if union > 0
        && shared >= STEM_SHARED_FLOOR
        && (shared as f64) / (union as f64) >= STEM_JACCARD_FLOOR
    {
        return Some("stem");
    }
    None
}

fn same_cite(a_cite: &str, b_cite: &str) -> bool {
    let (ac, bc) = (normalize_cite(a_cite), normalize_cite(b_cite));
    !ac.is_empty() && ac == bc
}

/// The pair `same_question` refuses on the words alone: the same non-empty cite, no rule
/// fired. Named so the tick can see how often the cite matched and the words did not.
pub(super) fn cite_only(a_text: &str, a_cite: &str, b_text: &str, b_cite: &str) -> bool {
    same_cite(a_cite, b_cite) && same_question(a_text, a_cite, b_text, b_cite).is_none()
}

/// The question half of a rendered decision line (`opener::render_open_decision` joins
/// `question — options: …`): the text the stem rule compares against.
fn decision_question(line: &str) -> &str {
    line.split(" — options: ").next().unwrap_or(line)
}

/// C2(a): a slice question that IS an open decision is routed to it — `OpenQuestion.decision`
/// set to the decision's index — and never dispatched on its slice's lane; the decision's own
/// settlement (the user's answer, or the ONE decisions-lane row) reaches the slice through the
/// decisions partition every brief carries, and `briefs_from_slices` points the question at it.
/// A cited fact is never routed (it is settled). The id rule applies to every kind; the stem
/// rule only to a design/unkinded question — a spec lookup that happens to share words with a
/// decision is still a lookup. Returns how many were routed. One `research_question_covered`
/// per routed question, `by: decision`, with the rule.
pub(super) fn route_questions_to_decisions(
    opened: &mut OpenOutput,
    events: &dyn EventSink,
) -> usize {
    let decisions: Vec<(usize, String)> = opened
        .open_decisions
        .iter()
        .enumerate()
        .map(|(i, d)| (i, d.line.clone()))
        .collect();
    if decisions.is_empty() {
        return 0;
    }
    let mut routed = 0;
    for sl in &mut opened.slices {
        for (q_index, q) in sl.questions.iter_mut().enumerate() {
            if q.is_cited_fact() || q.decision.is_some() {
                continue;
            }
            let stem_allowed = matches!(q.kind, QuestionKind::Design | QuestionKind::Unkinded);
            let hit = decisions.iter().find_map(|(i, line)| {
                let dq = decision_question(line);
                match same_question(&q.text, &q.cite, dq, "") {
                    Some("stem") if !stem_allowed => None,
                    Some(rule) => Some((*i, rule)),
                    None => None,
                }
            });
            if let Some((i, rule)) = hit {
                q.decision = Some(i);
                routed += 1;
                events.write_value(serde_json::json!({
                    "event": "research_question_covered",
                    "slice": sl.id,
                    "q_index": q_index,
                    "question": q.text.chars().take(200).collect::<String>(),
                    "by": "decision",
                    "decision": i,
                    "rule": rule,
                }));
            }
        }
    }
    routed
}

/// C2(b): the already-landed mini that answers `q`, if one does — read at the lane's DISPATCH
/// (the minis on disk right then, resumed or landed by earlier lanes of any slice), never at fan
/// start. Only an ANSWERED row covers; a row never covers its own question; the rule that
/// matched rides back for the event. A row that is itself a covered copy resolves to the mini
/// it copied, so `by_mini` always names an original.
pub(super) fn covering_mini<'a>(
    q: &ResearchQuestion,
    landed: &'a [ResearchRow],
    events: &dyn EventSink,
) -> Option<(&'a ResearchRow, &'static str)> {
    let eligible = |r: &&ResearchRow| {
        r.status == RESEARCH_ANSWERED
            && !r.answer.trim().is_empty()
            && !(r.slice == q.slice && r.q_index == q.q_index)
    };
    let covered = landed.iter().filter(eligible).find_map(|r| {
        same_question(&q.question, &q.cite, &r.question, &r.cite).map(|rule| (r, rule))
    });
    if covered.is_none() {
        // D10-6: the cite matched and the words did not — dispatched, and said out loud.
        for r in landed.iter().filter(eligible) {
            if cite_only(&q.question, &q.cite, &r.question, &r.cite) {
                events.write_value(serde_json::json!({
                    "event": "research_question_cite_only",
                    "a": {"slice": q.slice, "q_index": q.q_index,
                          "question": q.question.chars().take(200).collect::<String>()},
                    "b": {"slice": r.slice, "q_index": r.q_index,
                          "question": r.question.chars().take(200).collect::<String>()},
                    "cite": q.cite,
                }));
            }
        }
    }
    covered
}

#[cfg(test)]
mod tests {
    use super::super::opener::{OpenDecision, OpenQuestion, OpenSlice};
    use super::super::research::{research_mini_name, ORIGIN_COVERED_PREFIX};
    use super::super::{NullSink, SwarmEvent};
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

    // r6d's texts, verbatim from research_dispatched / low_confidence_ask.
    const WEB_Q0: &str = "How does the browser obtain the three bearer tokens for drafts endpoints — the spec defines no auth UI; is it a prompt, a config step, or do we ship a small token entry in the page?";
    const DEC_TOKENS: &str = "How the browser obtains the three bearer tokens for drafts endpoints: the spec defines no auth UI, so token entry (prompt field, config, or hardcoded dev tokens in the page) must be chosen by a human.";
    const DEC_D123: &str = "D1/D2/D3 are deliberately left open by the spec but are ASSIGNED decisions — they must be decided and shipped in DECISIONS.md (## D1 brush vs streamed mutation, ## D2 rejected-draft terminality, ## D3 pre-sync table state), not deferred to a human.";
    const WEB_Q1: &str = "D1: does the brush survive a streamed mutation of a brushed record (stay brushed vs drop out) — decide and document under ## D1?";
    const WEB_Q2: &str = "D3: empty-with-progress vs loading state before first sync — decide and document under ## D3?";
    const DRAFTS_Q4: &str = "Do maker/checker see /api/events at all (auth applies to the endpoint; is there any role-based filtering of event visibility)?";
    const API_Q5: &str = "Does GET /api/events require a token of ANY known role, and does admin's read-everything include the full event history from seq 1?";
    const API_Q1: &str = "Which sort keys does sort=<k> accept and in what direction(s); which status/currency values do the filters accept?";
    const WEB_Q5: &str = "Which filter and sort controls does the table expose, and which query keys do they map to?";

    /// The three rules on r6d's pairs: the token question IS the token decision (stem 0.57/13);
    /// web-q1 names D1, which the D1/D2/D3 line also names (id); drafts-q4 and api-q5 are the
    /// same question by meaning only — no rule fires on their words (0.10) but the shared cite
    /// does; api-q1 vs a sort/filter question shares 3 words (0.20) and stays two questions.
    #[test]
    fn same_question_fires_on_cite_id_or_a_high_stem_and_nothing_else() {
        assert_eq!(same_question(WEB_Q0, "", DEC_TOKENS, ""), Some("stem"));
        assert_eq!(same_question(WEB_Q1, "", DEC_D123, ""), Some("decision_id"));
        assert_eq!(same_question(WEB_Q2, "", DEC_D123, ""), Some("decision_id"));
        assert_eq!(
            same_question(WEB_Q1, "", WEB_Q2, ""),
            None,
            "D1 and D3 are different decisions"
        );
        assert_eq!(same_question(DRAFTS_Q4, "", API_Q5, ""), None);
        // D10-6: the same line cited by two questions that share 2 content words (/api/events,
        // event) is CITE-ONLY — dispatched and named, never deduped on the line alone.
        assert_eq!(
            same_question(DRAFTS_Q4, "request.md:218", API_Q5, "request.md:218"),
            None
        );
        assert!(cite_only(
            DRAFTS_Q4,
            "request.md:218",
            API_Q5,
            "request.md:218"
        ));
        assert!(
            !cite_only(DRAFTS_Q4, "", API_Q5, ""),
            "no cite, nothing to corroborate"
        );
        // The same line PLUS the words: a rephrasing of api-q1 sharing sort / keys /
        // status/currency / accept (4 >= the floor of 3) dedups by cite.
        assert_eq!(
            same_question(
                API_Q1,
                "request.md:148",
                "Which sort keys and status/currency filter values does the table's query accept?",
                "request.md:148"
            ),
            Some("cite")
        );
        assert_eq!(
            same_question(DRAFTS_Q4, "request.md:218", API_Q5, "request.md:51"),
            None,
            "different cites, different words: two questions"
        );
        assert_eq!(same_question(API_Q1, "", WEB_Q5, ""), None);
        assert_eq!(
            same_question("", "", "", ""),
            None,
            "two empty questions are not the same question"
        );
        assert_eq!(
            decision_ids("D1/D2/D3 and ## D2, but ID1 and D1x are not"),
            [1, 2, 3].into_iter().collect()
        );
        let w = content_words(DRAFTS_Q4);
        assert!(w.contains("/api/events") && w.contains("role-based") && !w.contains("the"));
    }

    /// C2(a) on r6d's web-page slice against the two decisions HEAD's qualifier keeps (the
    /// http-framework one and the token-entry one): web-q0 is routed to the token decision
    /// (stem), web-q1/web-q2 name D1/D3 that no QUALIFIED decision names and stay on the lane
    /// (dispatch when in doubt), a cited fact is never routed, and a spec lookup sharing words
    /// with a decision is still a lookup.
    #[test]
    fn a_slice_question_that_is_an_open_decision_is_routed_to_it_once() {
        let http = "HTTP framework for ledgerd/notifierd — options: stdlib http.server (threaded) | Flask | FastAPI (the request leaves it open: p95 <150ms under load)";
        let tokens = format!(
            "{DEC_TOKENS} — options: prompt field | config | hardcoded dev tokens in the page"
        );
        let mut lookup_like_tokens: OpenQuestion = OpenQuestion::from(WEB_Q0);
        lookup_like_tokens.kind = QuestionKind::SpecLookup;
        let mut opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "web-page".into(),
                title: "the page".into(),
                objective: "web/app.js".into(),
                questions: vec![
                    OpenQuestion::from(WEB_Q0),
                    OpenQuestion::from(WEB_Q1),
                    OpenQuestion::from(WEB_Q2),
                    lookup_like_tokens,
                    serde_json::from_value(serde_json::json!({
                        "question": WEB_Q0, "kind": "spec_lookup", "cite": "request.md:432",
                        "fact": "A token input (`#role-token`) — the bearer the page sends on every drafts call"
                    }))
                    .unwrap(),
                ],
                weight: 3,
                sections: Vec::new(),
            }],
            open_decisions: vec![
                OpenDecision {
                    line: http.into(),
                    options: vec!["stdlib".into(), "Flask".into(), "FastAPI".into()],
                },
                OpenDecision {
                    line: tokens,
                    options: vec!["prompt field".into(), "config".into()],
                },
            ],
        };
        let sink = ValueSink::default();
        assert_eq!(route_questions_to_decisions(&mut opened, &sink), 1);
        let qs = &opened.slices[0].questions;
        assert_eq!(qs[0].decision, Some(1), "web-q0 is the token decision");
        assert_eq!(qs[1].decision, None, "D1: no qualified decision names it");
        assert_eq!(qs[2].decision, None);
        assert_eq!(qs[3].decision, None, "a lookup is not routed by stem");
        assert_eq!(
            qs[4].decision, None,
            "a cited fact is settled, never routed"
        );
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["event"], "research_question_covered");
        assert_eq!(ev[0]["by"], "decision");
        assert_eq!(ev[0]["decision"], 1);
        assert_eq!(ev[0]["rule"], "stem");
        assert_eq!(ev[0]["slice"], "web-page");
        assert_eq!(ev[0]["q_index"], 0);
        // With the D1/D2/D3 line among the decisions (r6d's shape before the qualifier), the
        // id rule routes web-q1 and web-q2 to it — D1 decided ONCE, on the decisions lane.
        opened.open_decisions.push(OpenDecision {
            line: format!("{DEC_D123} — options: stay brushed | drop out"),
            options: vec!["stay brushed".into(), "drop out".into()],
        });
        let sink = ValueSink::default();
        assert_eq!(route_questions_to_decisions(&mut opened, &sink), 2);
        assert_eq!(opened.slices[0].questions[1].decision, Some(2));
        assert_eq!(opened.slices[0].questions[2].decision, Some(2));
        assert_eq!(
            opened.slices[0].questions[0].decision,
            Some(1),
            "an already-routed question is not re-routed"
        );
        // No decisions: nothing routes, nothing is emitted.
        opened.open_decisions.clear();
        for q in &mut opened.slices[0].questions {
            q.decision = None;
        }
        let sink = ValueSink::default();
        assert_eq!(route_questions_to_decisions(&mut opened, &sink), 0);
        assert!(sink.0.lock().unwrap().is_empty());
    }

    /// C2(b) on r6d's api-q1 fact (request.md:148) and a web question citing the same line with
    /// corroborating words: covered by cite — the covered row copies the answer, names the
    /// ORIGINAL mini in its origin, carries the FACT's cite (D10-7) and spends no seconds. r6d's
    /// drafts-q4 / api-q5 pair on request.md:218 (2 shared words) is CITE-ONLY under D10-6: not
    /// covered, dispatched, and named by `research_question_cite_only`. Without a cite it
    /// dispatches silently; an unanswered or empty row covers nothing; a row never covers
    /// itself; a covered copy resolves to the original.
    #[test]
    fn a_question_an_earlier_landed_mini_answers_is_covered_by_that_mini() {
        let api_q1 = ResearchQuestion::of(
            "ledger-api",
            1,
            &serde_json::from_value(serde_json::json!({
                "question": API_Q1, "kind": "spec_lookup", "cite": "request.md:148"
            }))
            .unwrap(),
        );
        let landed_fact = ResearchRow::spec_fact(
            &api_q1,
            "`sort` is one of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`.",
        );
        let mut cited_web: OpenQuestion = OpenQuestion::from(
            "Which sort keys and status/currency filter values does the table's query accept?",
        );
        cited_web.cite = "request.md:148".into();
        let qw = ResearchQuestion::of("web-page", 5, &cited_web);
        let sink = ValueSink::default();
        let (cover, rule) = covering_mini(&qw, std::slice::from_ref(&landed_fact), &sink)
            .expect("the shared cite, corroborated by the words, covers it");
        assert_eq!(rule, "cite");
        assert_eq!(cover.q_index, 1);
        assert!(
            sink.0.lock().unwrap().is_empty(),
            "a covered pair is not cite-only"
        );
        let row = ResearchRow::covered_by(&qw, cover, rule);
        assert_eq!(row.status, RESEARCH_ANSWERED);
        assert!(row.answer.starts_with("`sort` is one of"));
        assert_eq!(
            row.origin,
            format!(
                "{ORIGIN_COVERED_PREFIX}{}",
                research_mini_name("ledger-api", 1)
            )
        );
        assert_eq!(row.slice, "web-page");
        assert_eq!(row.q_index, 5);
        assert_eq!(
            row.cite, "request.md:148",
            "the fact's cite travels with its answer"
        );
        assert_eq!(row.secs, 0, "nothing was called");
        assert!(row.model.is_empty(), "no lane answered a fact");
        // A copy of a copy names the original.
        let again = ResearchRow::covered_by(&qw, &row, "cite");
        assert_eq!(again.origin, row.origin);

        // r6d's drafts-q4 / api-q5 on request.md:218: cite-only — dispatched and named.
        let api_q5 = ResearchQuestion::of(
            "ledger-api",
            5,
            &serde_json::from_value(serde_json::json!({
                "question": API_Q5, "kind": "spec_lookup", "cite": "request.md:218"
            }))
            .unwrap(),
        );
        let q5_fact = ResearchRow::spec_fact(
            &api_q5,
            "It requires a bearer token (any of the three roles).",
        );
        let mut cited_q4: OpenQuestion = OpenQuestion::from(DRAFTS_Q4);
        cited_q4.cite = "request.md:218".into();
        let q4 = ResearchQuestion::of("drafts-workflow", 4, &cited_q4);
        let sink = ValueSink::default();
        assert!(covering_mini(&q4, std::slice::from_ref(&q5_fact), &sink).is_none());
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "research_question_cite_only");
        assert_eq!(events[0]["a"]["slice"], "drafts-workflow");
        assert_eq!(events[0]["b"]["q_index"], 5);
        assert_eq!(events[0]["cite"], "request.md:218");
        drop(events);
        // Without the cite the words alone do not match: dispatch, and nothing to name.
        let bare_q4 = ResearchQuestion::of("drafts-workflow", 4, &OpenQuestion::from(DRAFTS_Q4));
        let quiet = ValueSink::default();
        assert!(covering_mini(&bare_q4, std::slice::from_ref(&q5_fact), &quiet).is_none());
        assert!(quiet.0.lock().unwrap().is_empty());
        // Its own row, an unanswered row and a blank answer never cover.
        let mut own = landed_fact.clone();
        own.slice = "web-page".into();
        own.q_index = 5;
        assert!(covering_mini(&qw, std::slice::from_ref(&own), &NullSink).is_none());
        let mut missed = landed_fact.clone();
        missed.status = "unanswered".into();
        assert!(covering_mini(&qw, std::slice::from_ref(&missed), &NullSink).is_none());
        let mut blank = landed_fact.clone();
        blank.answer = "  ".into();
        assert!(covering_mini(&qw, std::slice::from_ref(&blank), &NullSink).is_none());
    }
}
