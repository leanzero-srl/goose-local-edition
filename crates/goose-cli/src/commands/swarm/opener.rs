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

use super::orientation::{request_file_label, spec_sections, top_level};
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

/// VA-075: the length past which a `fact` is NAMED (`fact_overlong`, a WARNING at parse) — it
/// bounds NOTHING: the fact is kept verbatim, the plan is accepted, no lane or turn depends on
/// it. It is not in the schema: the refuter measured r6e's verified emit at 20 of 21 facts over
/// 200 chars (246…909; "Defaults: yaw = 30, pitch = 40, distance = 260. Clamps: pitch [5, 85]…"
/// keeps the literals exact as the rule demands), so a `maxLength` would have REFUSED a good plan
/// and forced the whole emit to re-stream — invariant 3's anti-pattern. WHY the rule: r6f's opener wrote ~1,100
/// characters of request.md:547-565 into ONE spec_lookup fact ("<canvas id='viz3d'>, context
/// webgl or webgl2 created {antialias:false, alpha:false}, on the MAIN thread — no
/// OffscreenCanvas, no Worker…") and its 33,818-completion-token reply sat 16m43s at 0 bytes
/// while the arguments streamed (r6e's reply: 41.5 KB; r6f's ≈ 1.5×). The owning brief already
/// splices each claimed section's FULL text and every worker holds the request file at the
/// cited line, so a pasted passage is the plan written twice.
pub(super) const FACT_MAX_CHARS: usize = 200;

/// One slice question as the run consumes it. `text` is the question verbatim (whitespace
/// squashed to one line — the identity the mini, the brief and the dedup all read); `cite` is the
/// request line or heading the opener read (`request.md:148`, or a heading), empty when it named
/// none; `fact` is the answer as ONE sentence (`FACT_MAX_CHARS`), empty unless the opener found one.
#[derive(Clone, Debug)]
pub(crate) struct OpenQuestion {
    pub(crate) text: String,
    pub(crate) kind: QuestionKind,
    pub(crate) cite: String,
    pub(crate) fact: String,
    /// C2(a): the index of the opener's open decision this question IS, set by
    /// `research_plan::route_questions_to_decisions` after ASK — never by the opener. Routed
    /// questions ride no slice lane; the decision settles once and the brief points at it.
    pub(crate) decision: Option<usize>,
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
            decision: None,
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
                decision: None,
            },
            OpenQuestionRaw::Bare(q) => OpenQuestion {
                text: squash(&q),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                fact: String::new(),
                decision: None,
            },
            OpenQuestionRaw::Other(v) => OpenQuestion {
                text: squash(&v.to_string()),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                fact: String::new(),
                decision: None,
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

/// VA-078 (VA-060's generality receipt): the three example shapes in the QUESTIONS rule are
/// drawn from THIS request's own lines by code — the first table row, the first rule-stating
/// line, the first top-level section — never from another project's words. Before, the rule
/// carried sb-7's "sort is one of created_at…" and "410 cursor_expired" into every run's opener
/// prompt. A request with no line of a kind says so beside a placeholder shape (no row is ever
/// invented).
struct RequestExemplars {
    table_row: Option<TableRow>,
    prose: Option<ProseLine>,
    /// The first section at the document's top level (`orientation::top_level`), with its line
    /// range — the range-cite form the index now carries.
    section: Option<(String, usize, usize)>,
}

/// The first data row of the first markdown table: its line, the header cells above the
/// separator, and its own cells.
struct TableRow {
    line: usize,
    header: Vec<String>,
    cells: Vec<String>,
}

/// The first prose line that states a rule (a normative word), else the first prose line at
/// all; `normative` says which.
struct ProseLine {
    line: usize,
    text: String,
    normative: bool,
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

fn is_table_line(t: &str) -> bool {
    t.starts_with('|') && t.chars().skip(1).any(|c| c == '|')
}

fn is_table_separator(t: &str) -> bool {
    is_table_line(t)
        && table_cells(t)
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')))
}

fn is_heading_line(t: &str) -> bool {
    let hashes = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ')
}

/// A rule in prose: a line carrying a normative word — general English, no project vocabulary.
fn states_a_rule(t: &str) -> bool {
    t.split(|c: char| !c.is_alphanumeric()).any(|w| {
        matches!(
            w.to_ascii_lowercase().as_str(),
            "must" | "never" | "always" | "required" | "shall" | "only" | "exactly" | "cannot"
        )
    })
}

fn request_exemplars(spec: &str) -> RequestExemplars {
    let lines: Vec<&str> = spec.lines().collect();
    let mut in_fence = false;
    let mut pending_header: Option<Vec<String>> = None;
    let mut table_row: Option<TableRow> = None;
    let mut first_prose: Option<ProseLine> = None;
    let mut rule: Option<ProseLine> = None;
    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || t.is_empty() || is_heading_line(t) {
            continue;
        }
        if is_table_line(t) {
            if table_row.is_some() {
                continue;
            }
            if is_table_separator(t) {
                // GFM: a table is a header row, a separator, then rows — a separator with no
                // header line above it is not a table.
                if let Some(h) = idx
                    .checked_sub(1)
                    .map(|h| lines[h].trim())
                    .filter(|h| is_table_line(h) && !is_table_separator(h))
                {
                    pending_header = Some(table_cells(h));
                }
            } else if let Some(header) = pending_header.take() {
                table_row = Some(TableRow {
                    line: idx + 1,
                    header,
                    cells: table_cells(t),
                });
            }
            continue;
        }
        let normative = states_a_rule(t);
        if first_prose.is_none() {
            first_prose = Some(ProseLine {
                line: idx + 1,
                text: t.to_string(),
                normative,
            });
        }
        if rule.is_none() && normative {
            rule = Some(ProseLine {
                line: idx + 1,
                text: t.to_string(),
                normative,
            });
        }
    }
    let sections = spec_sections(spec);
    let top = top_level(&sections);
    let section = sections
        .iter()
        .filter(|s| !s.heading.is_empty())
        .find(|s| top.is_none_or(|l| s.level == l))
        .map(|s| (s.heading.clone(), s.line_start, s.line_end));
    RequestExemplars {
        table_row,
        prose: rule.or(first_prose),
        section,
    }
}

/// A quoted line inside an example is an illustration of the cite form — cut at a character
/// boundary to `max` chars INCLUDING the ellipsis (a cut fact must still pass `FACT_MAX_CHARS`);
/// the full line is one `sed` away.
fn quote_cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut cut: String = s.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

/// The request's text as the rule saw it: read back from the persisted file, or a NAMED absence
/// the examples state instead of quoting (gate 1 — never a substituted row).
enum RequestText {
    Read(RequestExemplars),
    Absent(String),
}

/// The three example lines of the QUESTIONS rule, one per kind, each ending in a JSON object
/// that satisfies `open_schema` (the tests parse them): spec_lookup on the first table row
/// (its cells ARE the fact), design on the first rule-stating line (the closest words + the grep
/// that found nothing), external on the first top-level section (the range cite from the index).
fn example_lines(text: &RequestText, label: &str) -> [String; 3] {
    let (row, prose, section, absent) = match text {
        RequestText::Read(ex) => (
            ex.table_row.as_ref(),
            ex.prose.as_ref(),
            ex.section.as_ref(),
            None,
        ),
        RequestText::Absent(why) => (None, None, None, Some(why.as_str())),
    };
    let none_shown = |missing: &str| match absent {
        Some(why) => format!("Example ({why}, so no line of it is quoted; {missing}): "),
        None => format!("Example (this request has no {missing}): "),
    };
    let lookup = match row {
        Some(r) => {
            // `split` always yields at least one cell, so the key exists; empty only when the
            // row's first cell is literally empty — quoted as such.
            let key = r.cells.first().map_or("", String::as_str);
            let fact = r
                .header
                .iter()
                .zip(&r.cells)
                .map(|(h, c)| format!("{h}: {c}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "Example, on this request's own first table row ({label}:{}): {}",
                r.line,
                serde_json::json!({
                    "question": format!("What does the row at {label}:{} fix for `{key}`?", r.line),
                    "kind": "spec_lookup",
                    "cite": format!("{label}:{}", r.line),
                    "fact": quote_cut(&fact, FACT_MAX_CHARS),
                })
            )
        }
        None => format!(
            "{}{}",
            none_shown("markdown table, so no row is shown; the shape holds for any line that holds a value"),
            serde_json::json!({
                "question": "<what the request settles, asked as a question>",
                "kind": "spec_lookup",
                "cite": format!("{label}:<N>"),
                "fact": format!("<one sentence in your words, at most {FACT_MAX_CHARS} characters>"),
            })
        ),
    };
    let design = match prose {
        Some(p) => format!(
            "Example, on this request's first {} ({label}:{}): {}",
            if p.normative {
                "rule-stating line"
            } else {
                "prose line"
            },
            p.line,
            serde_json::json!({
                "question": "<the convention this line leaves to the builder — name it>",
                "kind": "design",
                "cite": format!(
                    "{label}:{} '{}'; grep -n -i '<term>' → no match",
                    p.line,
                    quote_cut(&p.text, 140)
                ),
            })
        ),
        None => format!(
            "{}{}",
            none_shown("prose line to quote"),
            serde_json::json!({
                "question": "<the convention the request leaves to the builder — name it>",
                "kind": "design",
                "cite": format!("{label}:<N> '<the closest words you read>'; grep -n -i '<term>' → no match"),
            })
        ),
    };
    let external = match section {
        Some((heading, a, b)) => format!(
            "Example, on this request's first section ({label}:{a}-{b} `{heading}`): {}",
            serde_json::json!({
                "question": "<what the outside source must settle that the request only points at>",
                "kind": "external",
                "cite": format!(
                    "{label}:{a}-{b} `{heading}` — quote its sentence that defers to the source, and the line that names the source"
                ),
            })
        ),
        None => format!(
            "{}{}",
            none_shown("heading to quote"),
            serde_json::json!({
                "question": "<what the outside source must settle that the request only points at>",
                "kind": "external",
                "cite": format!("{label}:<A-B> '<the words that defer to the source>'; the source is named at {label}:<N>"),
            })
        ),
    };
    [lookup, design, external]
}

/// The opener's contract, deliberately small: no files FIELD (owned files are declared inside the
/// objective text — synthesis infers each task's paths from its slice's objective), no deps, no
/// task ids, no requirement map. An open decision is an OBJECT — `question`, `options` (two or
/// more), `cite` — never a bare sentence: r6d's opener emitted three strings with no options,
/// one of them an instruction to itself, and the ask window was spent on them.
/// D10-8: the QUESTIONS rule the opener reads right after the SOURCES block — the contract the
/// schema enforces (`cite` required on every kind, non-empty), with the three shapes shown on
/// THIS request's own lines (VA-078, `request_exemplars`, read back from the persisted file —
/// the same bytes the opener will grep, by that file's line numbers), and the order of
/// operations: RUN the grep first. `request_path` is the persisted request file (the SOURCES
/// block names it too); when persisting failed the rule names the absence instead of a path that
/// is not there (gate 1), and the examples say so instead of quoting.
pub(super) fn opener_questions_rule(request_path: Option<&std::path::Path>) -> String {
    let path = request_path.map_or_else(
        || {
            "the request file (NOT persisted this run — see SOURCES; cite the heading you read)"
                .to_string()
        },
        |p| p.display().to_string(),
    );
    let label = request_file_label();
    let text = match request_path {
        None => RequestText::Absent("the request file was NOT persisted this run".to_string()),
        Some(p) => match std::fs::read_to_string(p) {
            Ok(spec) => RequestText::Read(request_exemplars(&spec)),
            Err(e) => RequestText::Absent(format!(
                "the request file could not be read back for examples: {e}"
            )),
        },
    };
    let [lookup_example, design_example, external_example] = example_lines(&text, &label);
    format!(
        "\n\nQUESTIONS. A question is an OBJECT {{question, kind, cite, fact}}. EVERY kind carries a \
         cite; the schema rejects a question with an empty cite. Before you write any question, RUN \
         (do not describe) a grep against the request file named under SOURCES: `grep -n -i '<term>' \
         {path}` then `sed -n 'A,Bp'`. Never print the whole file. When the request arrives as its \
         ORIENTATION INDEX, every entry carries its section's line range (`{label}:A-B`): take \
         cites FROM the index, and read a section (`sed -n 'A,Bp'`) only when a fact needs its \
         words — the index IS the heading-to-line map, so never rebuild one by hand and never \
         re-read a range you have already read. Then:\n\
         — If a line ANSWERS it, it is not a question: kind spec_lookup, cite {label}:<N>, fact = \
         ONE sentence in your own words, at most {FACT_MAX_CHARS} characters (a longer one is kept \
         but named on the event log), saying what the line settles for a builder, literal values \
         kept exact; no \
         lane runs. The fact is NOT the passage: the owning brief carries each claimed section's \
         FULL text and every builder holds the request file at the cited line, so a pasted passage \
         is the plan written twice — MEASURED: one opener pasted ~1,100 characters of a section \
         into a single fact and its reply sat 16 minutes at zero bytes while the arguments \
         streamed. {lookup_example}\n\
         — If the request is SILENT and a builder must choose: kind design, cite = the closest lines \
         you read AND the grep that found nothing. {design_example}\n\
         — If it needs the vendor's documentation: kind external, cite = the request line that defers \
         to it. {external_example}\n\
         A spec_lookup with an empty fact is allowed ONLY with the literal grep command you ran and \
         its 'no match' in cite — a cite you did not run is a false citation. A question that IS one \
         of your open_decisions, or names a decision the request itself assigns to the builder, goes \
         under open_decisions only. MEASURED: a previous opener wrote a question asking for a \
         response shape 'in full text' while the request held that shape three lines from the cite \
         it gave; 'in full text' means grep it now, not ask."
    )
}

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
                                "required": ["question", "kind", "cite"],
                                "properties": {
                                    "question": {"type": "string"},
                                    "kind": {"type": "string", "enum": ["spec_lookup", "design", "external"]},
                                    "cite": {"type": "string", "minLength": 1},
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
            } else if q.cite.trim().is_empty() {
                // D10-8: the schema requires a non-empty cite on EVERY kind; a kinded question
                // that arrives without one came through the text fallthrough (prose, a fence)
                // past the validator. Named, kept, dispatched — a grep the opener did not run is
                // a grep a lane now runs.
                events.write_value(serde_json::json!({
                    "event": "research_question_uncited",
                    "slice": sl.id,
                    "q_index": sl.questions.len(),
                    "kind": q.kind.as_str(),
                    "question": q.text.chars().take(200).collect::<String>(),
                }));
            }
            // VA-075: an overlong fact is a WARNING with its size, never a refusal — the fact rides
            // verbatim, the plan is accepted. r6e's good plan carried 20 such facts (246…909 chars).
            let fact_chars = q.fact.chars().count();
            if fact_chars > FACT_MAX_CHARS {
                events.write_value(serde_json::json!({
                    "event": "fact_overlong",
                    "slice": sl.id,
                    "q_index": sl.questions.len(),
                    "question": q.text.chars().take(200).collect::<String>(),
                    "chars": fact_chars,
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
        assert_eq!(
            q["required"],
            serde_json::json!(["question", "kind", "cite"]),
            "D10-8: the VALIDATOR refuses a bare question — never a retry count"
        );
        assert_eq!(q["properties"]["cite"]["minLength"], 1);
        // VA-075 (refuted refuser): the schema bounds a fact's length NOWHERE — r6e's good plan
        // had 20 of 21 facts over 200 chars; an overlong fact is named, never refused.
        assert!(q["properties"]["fact"].get("maxLength").is_none());
        assert_eq!(
            q["properties"]["kind"]["enum"],
            serde_json::json!(["spec_lookup", "design", "external"])
        );
        // The rule's three example objects satisfy the schema's shape: every kind, a non-empty cite.
        let rule = opener_questions_rule(Some(std::path::Path::new("/run/.swarm/request.md")));
        assert!(rule.contains("grep -n -i '<term>' /run/.swarm/request.md"));
        let mut examples = 0;
        for line in rule.lines() {
            let Some(start) = line.find("{\"question\"") else {
                continue;
            };
            let obj: serde_json::Value =
                serde_json::from_str(line.get(start..).unwrap_or("").trim_end())
                    .expect("the example is valid JSON");
            let kind = obj["kind"].as_str().unwrap();
            assert!(
                ["spec_lookup", "design", "external"].contains(&kind),
                "{obj}"
            );
            assert!(!obj["cite"].as_str().unwrap().is_empty(), "{obj}");
            if kind == "spec_lookup" {
                assert!(!obj["fact"].as_str().unwrap().is_empty(), "{obj}");
            }
            examples += 1;
        }
        assert_eq!(examples, 3, "one example per kind");
        assert!(
            rule.contains(&format!("at most {FACT_MAX_CHARS} characters"))
                && rule.contains("The fact is NOT the passage"),
            "the rule states the fact's length and WHY a passage is a duplicate"
        );
        assert!(
            opener_questions_rule(None).contains("NOT persisted this run"),
            "a missing request file is named, never pointed at"
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
                "research_question_uncited",
                "research_question_uncited",
                "research_question_unkinded",
                "research_question_unkinded",
                "research_question_empty"
            ],
            "the two kinded-but-uncited entries (External, design) are named first: {events:?}"
        );
        assert_eq!(events[0]["kind"], "external");
        assert_eq!(events[0]["q_index"], 1);
        assert_eq!(events[1]["kind"], "design");
        let events: Vec<serde_json::Value> = events
            .iter()
            .filter(|e| e["event"] != "research_question_uncited")
            .cloned()
            .collect();
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

    fn rule_examples(rule: &str) -> Vec<serde_json::Value> {
        rule.lines()
            .filter_map(|l| l.find("{\"question\"").and_then(|s| l.get(s..)))
            .map(|j| serde_json::from_str(j.trim_end()).expect("the example is valid JSON"))
            .collect()
    }

    /// VA-078: the rule's three examples quote THIS request — its first table row (the cells are
    /// the fact, cut to the schema's length), its first rule-stating line (the closest words +
    /// the grep that found nothing) and its first top-level section (the range cite the index
    /// carries) — by that file's line numbers; a request without a table says so instead of
    /// inventing a row; fenced code is not prose; an absent or unreadable file is named. No
    /// sb-7 literal survives in the general prompt.
    #[test]
    fn the_rule_examples_quote_this_request_never_another_projects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.md");
        std::fs::write(
            &path,
            "# Title\nintro\n\n## Endpoints\n| Method | Path | Purpose |\n|---|---|---|\n\
             | GET | /api/items | list items |\n\n## Rules\nThe server MUST answer within 2 seconds.\n",
        )
        .unwrap();
        let rule = opener_questions_rule(Some(&path));
        let ex = rule_examples(&rule);
        assert_eq!(ex.len(), 3, "{rule}");
        assert_eq!(ex[0]["kind"], "spec_lookup");
        assert_eq!(ex[0]["cite"], "request.md:7");
        assert_eq!(
            ex[0]["fact"],
            "Method: GET; Path: /api/items; Purpose: list items"
        );
        assert_eq!(
            ex[0]["question"],
            "What does the row at request.md:7 fix for `GET`?"
        );
        assert!(
            rule.contains("on this request's own first table row (request.md:7)"),
            "{rule}"
        );
        assert_eq!(ex[1]["kind"], "design");
        assert!(
            ex[1]["cite"].as_str().unwrap().starts_with(
                "request.md:10 'The server MUST answer within 2 seconds.'; grep -n -i '<term>'"
            ),
            "{}",
            ex[1]["cite"]
        );
        assert!(
            rule.contains("first rule-stating line (request.md:10)"),
            "{rule}"
        );
        assert_eq!(ex[2]["kind"], "external");
        assert!(
            ex[2]["cite"]
                .as_str()
                .unwrap()
                .starts_with("request.md:4-8 `Endpoints`"),
            "the first TOP-LEVEL section, not the title: {}",
            ex[2]["cite"]
        );
        for foreign in [
            "created_at",
            "cursor_expired",
            "D1/D2/D3",
            "Health response shape",
            "events.py",
        ] {
            assert!(
                !rule.contains(foreign),
                "sb-7's words left the prompt: {foreign}"
            );
        }

        // No table: the example SAYS so; the design example falls back to the first prose line
        // and says which; the external example still has a section.
        let plain = dir.path().join("plain.md");
        std::fs::write(
            &plain,
            "# Title\n\n## One\nUse the standard library.\n\n## Two\nmore\n",
        )
        .unwrap();
        let rule = opener_questions_rule(Some(&plain));
        let ex = rule_examples(&rule);
        assert_eq!(ex.len(), 3);
        assert!(
            rule.contains("this request has no markdown table, so no row is shown"),
            "{rule}"
        );
        assert_eq!(ex[0]["cite"], "request.md:<N>");
        assert!(rule.contains("first prose line (request.md:4)"), "{rule}");
        assert!(ex[2]["cite"]
            .as_str()
            .unwrap()
            .starts_with("request.md:3-5 `One`"));

        // Fenced code is neither a table nor prose; a fact cut to the schema's length still passes.
        let fenced = dir.path().join("fenced.md");
        std::fs::write(
            &fenced,
            "# T\n```\n| a | b |\n|---|---|\n| 1 | 2 |\nx must y\n```\nplain\n",
        )
        .unwrap();
        let rule = opener_questions_rule(Some(&fenced));
        assert!(rule.contains("no markdown table"), "{rule}");
        assert!(rule.contains("first prose line (request.md:8)"), "{rule}");
        let long_row = format!(
            "# T\n\n## S\n| k | v |\n|---|---|\n| key | {} |\n",
            "x".repeat(400)
        );
        let long = dir.path().join("long.md");
        std::fs::write(&long, long_row).unwrap();
        let ex = rule_examples(&opener_questions_rule(Some(&long)));
        let fact = ex[0]["fact"].as_str().unwrap();
        assert_eq!(fact.chars().count(), FACT_MAX_CHARS);
        assert!(fact.ends_with('…'));

        // Absent (not persisted) or unreadable: named in the examples, never quoted from.
        let rule = opener_questions_rule(None);
        assert!(
            rule.contains(
                "the request file was NOT persisted this run, so no line of it is quoted"
            ),
            "{rule}"
        );
        assert_eq!(rule_examples(&rule).len(), 3);
        let rule = opener_questions_rule(Some(std::path::Path::new("/nonexistent/request.md")));
        assert!(
            rule.contains("could not be read back for examples"),
            "{rule}"
        );
        assert_eq!(rule_examples(&rule).len(), 3);
    }

    /// VA-075 as the refuter corrected it: r6e's verified emit had 20 of 21 facts over 200
    /// chars (246…909) and was a GOOD plan — a schema `maxLength` would have refused it and
    /// re-streamed the whole emit. An overlong fact parses, rides verbatim, still counts as a
    /// cited fact, and is NAMED once (`fact_overlong{slice, q_index, question, chars}`); a
    /// one-sentence fact is silent.
    #[test]
    fn an_overlong_fact_is_kept_verbatim_and_named_never_refused() {
        let defaults: String =
            "Defaults: yaw = 30, pitch = 40, distance = 260. Clamps: pitch [5, 85]"
                .chars()
                .chain("; distance [120, 900]".repeat(9).chars())
                .take(246)
                .collect();
        assert_eq!(
            defaults.chars().count(),
            246,
            "r6e's shortest overlong fact"
        );
        let nine_hundred_nine = "x".repeat(909);
        let nine_hundred = "y".repeat(900);
        let short = "`sort` is one of `created_at`, `-created_at`; default `created_at`.";
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [{
                "id": "web-viz", "title": "t", "objective": "o", "weight": 3,
                "questions": [
                    {"question": "Camera defaults and clamps?", "kind": "spec_lookup",
                     "cite": "request.md:616-617", "fact": defaults},
                    {"question": "Longest r6e fact?", "kind": "spec_lookup",
                     "cite": "request.md:547-565", "fact": nine_hundred_nine},
                    {"question": "A 900-char paste?", "kind": "spec_lookup",
                     "cite": "request.md:1", "fact": nine_hundred},
                    {"question": "Sort keys?", "kind": "spec_lookup",
                     "cite": "request.md:148", "fact": short}
                ]
            }]
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        let qs = &out.slices[0].questions;
        assert_eq!(qs.len(), 4, "nothing is refused or dropped");
        assert_eq!(qs[0].fact, defaults);
        assert_eq!(qs[1].fact, nine_hundred_nine);
        assert_eq!(qs[2].fact, nine_hundred);
        assert!(
            qs.iter().all(OpenQuestion::is_cited_fact),
            "still facts, no lane runs"
        );
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 3, "{events:?}");
        for (e, (q_index, chars)) in events.iter().zip([(0, 246), (1, 909), (2, 900)]) {
            assert_eq!(e["event"], "fact_overlong");
            assert_eq!(e["slice"], "web-viz");
            assert_eq!(e["q_index"], q_index);
            assert_eq!(e["chars"], chars);
        }
        assert_eq!(events[0]["question"], "Camera defaults and clamps?");
        assert!(
            open_schema()["properties"]["slices"]["items"]["properties"]["questions"]["items"]
                ["properties"]["fact"]
                .get("maxLength")
                .is_none(),
            "the schema never bounds a fact"
        );
    }
}
