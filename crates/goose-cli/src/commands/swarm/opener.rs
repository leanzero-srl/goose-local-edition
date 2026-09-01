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
//! THE QUESTION CONTRACT (the fan cut, r6d; VA-095): a slice question is an OBJECT with a `kind`
//! — `spec_lookup` | `design` | `external` — and a `cite`; for a lookup the request settles, the
//! cite IS the answer (`request.md:A-B`, `SpecCite`) and the engine renders those lines — the
//! opener writes no fact text. r6d dispatched 27 questions over 165 minutes on 3 nodes
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
    /// The request's own text answers it. With a line-range `cite` (`SpecCite`) it is a SPEC
    /// FACT — the engine renders the cited lines, no lane runs; with any other cite (the grep
    /// that found nothing) the opener searched and did not find it, and a lane looks.
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

/// VA-095: a `spec_lookup` cite as a LINE RANGE of the request file — `<label>:A` or
/// `<label>:A-B` (`request_file_label`, so `request.md:148`, `request.md:547-565`), 1-based and
/// inclusive, the form the orientation index carries and the rule asks for. This is the whole
/// fact: the engine renders those lines of the request verbatim (`render`), the opener writes no
/// answer text. WHY: r6g's opener emitted 80 `spec_lookup` facts WITH text (62 over 200 chars,
/// max 471) in a 61-minute reply on one node while two idled — research's work moved into the
/// serial opener lane — and every fact duplicated the section text the owning brief already
/// splices. A cite that names no line of the file (a grep command, a heading, another file) is
/// not a fact location; `parse` says so with `None` and the question rides a lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpecCite {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// The leading decimal number of `s` and what follows it; `None` when `s` does not start with a
/// digit (or the digits overflow).
fn leading_number(s: &str) -> (Option<usize>, &str) {
    let n = s.chars().take_while(char::is_ascii_digit).count();
    (s[..n].parse::<usize>().ok(), &s[n..])
}

impl SpecCite {
    /// Lenient on what surrounds the range (a path prefix, a heading after it), strict on the
    /// range itself: the label, a colon, digits, optionally `-` and digits; `start >= 1` and
    /// `end >= start`, else `None` — never a guess at which lines the opener meant.
    pub(crate) fn parse(cite: &str) -> Option<Self> {
        let marker = format!("{}:", request_file_label());
        let rest = &cite[cite.find(&marker)? + marker.len()..];
        let (start, after) = leading_number(rest);
        let start = start?;
        let end = match after.strip_prefix('-').map(leading_number) {
            Some((Some(end), _)) => end,
            _ => start,
        };
        (start >= 1 && end >= start).then_some(Self { start, end })
    }

    /// Lines cited, inclusive.
    pub(crate) fn span(self) -> usize {
        self.end - self.start + 1
    }

    /// The cited lines of `spec`, verbatim — each line's trailing whitespace cut, blank lines
    /// dropped, nothing else touched: the RANGE is the trim. No sentence is selected by code,
    /// because the words that settle a question (a table's literal values, a default) are the
    /// ones the question never names, so any overlap heuristic would drop exactly them (an
    /// invention by omission); the rule tells the opener to cite the line(s), and
    /// `spec_fact_rendered.lines` shows when it cited a section instead. `Err` names why the
    /// range is not a fact location: past the file's last line (`out_of_range`), across two
    /// sections of the document's own structure (`spans_sections` — a fact lives at a line or
    /// inside one section; a region is not a fact), or blank (`blank_range`).
    pub(crate) fn render(self, spec: &str) -> Result<String, &'static str> {
        let lines: Vec<&str> = spec.lines().collect();
        if self.start == 0 || self.end > lines.len() {
            return Err("out_of_range");
        }
        let sections = spec_sections(spec);
        let section_of = |line: usize| {
            sections
                .iter()
                .position(|s| (s.line_start..=s.line_end).contains(&line))
        };
        // Both lines exist, so both sit in a section (the index is contiguous over the file);
        // a hole in the index would be the engine's defect, not the opener's, and refuses nothing.
        if let (Some(a), Some(b)) = (section_of(self.start), section_of(self.end)) {
            if a != b {
                return Err("spans_sections");
            }
        }
        let text = lines[self.start - 1..self.end]
            .iter()
            .map(|l| l.trim_end())
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return Err("blank_range");
        }
        Ok(text)
    }
}

/// One slice question as the run consumes it. `text` is the question verbatim (whitespace
/// squashed to one line — the identity the mini, the brief and the dedup all read); `cite` is the
/// request line or heading the opener read (`request.md:148`, or a heading), empty when it named
/// none. There is no fact text (VA-095): a lookup's answer is its cited lines, rendered by code.
#[derive(Clone, Debug)]
pub(crate) struct OpenQuestion {
    pub(crate) text: String,
    pub(crate) kind: QuestionKind,
    pub(crate) cite: String,
    /// VA-095: the size of a `fact` the model wrote against the contract, kept ONLY so
    /// `qualify_slice_questions` can name it once (`research_question_fact_ignored`); the text
    /// itself is dropped at parse and rides nowhere.
    pub(crate) ignored_fact_chars: usize,
    /// C2(a): the index of the opener's open decision this question IS, set by
    /// `research_plan::route_questions_to_decisions` after ASK — never by the opener. Routed
    /// questions ride no slice lane; the decision settles once and the brief points at it.
    pub(crate) decision: Option<usize>,
}

impl OpenQuestion {
    /// A SPEC FACT: a lookup whose cite names a line range of the request file (`SpecCite`) —
    /// the engine renders those lines, no lane runs. A `spec_lookup` whose cite is anything else
    /// (the grep that found nothing, a heading) is a question again and rides a lane. Whether the
    /// range is IN the file is settled at the fan (`research::land_spec_fact`), where the request
    /// text is.
    pub(crate) fn is_cited_fact(&self) -> bool {
        self.kind == QuestionKind::SpecLookup && SpecCite::parse(&self.cite).is_some()
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
            ignored_fact_chars: 0,
            decision: None,
        }
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One question as the opener EMITTED it — the schema's object, a bare string from a model that
/// ignored the schema (every pre-cut opener's shape), or anything else (kept parseable so one odd
/// entry cannot fail the whole opener — the `OpenDecisionRaw` lesson). `fact` is read only to
/// MEASURE a model that still writes one (VA-095); it is never kept.
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
                ignored_fact_chars: fact.trim().chars().count(),
                decision: None,
            },
            OpenQuestionRaw::Bare(q) => OpenQuestion {
                text: squash(&q),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                ignored_fact_chars: 0,
                decision: None,
            },
            OpenQuestionRaw::Other(v) => OpenQuestion {
                text: squash(&v.to_string()),
                kind: QuestionKind::Unkinded,
                cite: String::new(),
                ignored_fact_chars: 0,
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

/// The first data row of the first markdown table (a header row, a separator, then rows): its
/// line and its cells.
struct TableRow {
    line: usize,
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
    let mut header_seen = false;
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
                header_seen = idx
                    .checked_sub(1)
                    .map(|h| lines[h].trim())
                    .is_some_and(|h| is_table_line(h) && !is_table_separator(h));
            } else if header_seen {
                table_row = Some(TableRow {
                    line: idx + 1,
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
/// boundary to `max` chars INCLUDING the ellipsis; the full line is one `sed` away.
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
/// that satisfies `open_schema` (the tests parse them): spec_lookup on the first table row (the
/// cite IS the fact — no text, VA-095), design on the first rule-stating line (the closest words
/// + the grep that found nothing), external on the first top-level section (the range cite from
/// the index).
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
            format!(
                "Example, on this request's own first table row ({label}:{}), which the engine \
                 renders verbatim as the fact: {}",
                r.line,
                serde_json::json!({
                    "question": format!("What does the row at {label}:{} fix for `{key}`?", r.line),
                    "kind": "spec_lookup",
                    "cite": format!("{label}:{}", r.line),
                })
            )
        }
        None => format!(
            "{}{}",
            none_shown("markdown table, so no row is shown; the shape holds for any line that holds a value"),
            serde_json::json!({
                "question": "<what the request settles, asked as a question>",
                "kind": "spec_lookup",
                "cite": format!("{label}:<N> or {label}:<A-B>, the line(s) that hold the value"),
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
        "\n\nQUESTIONS. A question is an OBJECT {{question, kind, cite}} — there is NO fact field. \
         EVERY kind carries a \
         cite; the schema rejects a question with an empty cite. Before you write any question, RUN \
         (do not describe) a grep against the request file named under SOURCES: `grep -n -i '<term>' \
         {path}` then `sed -n 'A,Bp'`. Never print the whole file. When the request arrives as its \
         ORIENTATION INDEX, every entry carries its section's line range (`{label}:A-B`): take \
         cites FROM the index, and read a section (`sed -n 'A,Bp'`) only when a fact needs its \
         words — the index IS the heading-to-line map, so never rebuild one by hand and never \
         re-read a range you have already read. Then:\n\
         — If a line ANSWERS it, it is not a question: kind spec_lookup, cite {label}:<N> or \
         {label}:<A-B> — the exact line(s) that hold the value, and NOTHING else: write no answer, \
         no sentence, no passage. The engine renders the cited lines verbatim into the owning brief \
         as the SPEC FACT and no lane runs; the cite IS the fact. The owning brief already carries \
         each claimed section's FULL text and every builder holds the request file at the cited \
         line, so any answer text you write is the plan written twice — MEASURED: one opener wrote \
         80 such facts (62 over 200 characters) into a 61-minute reply on one node while two nodes \
         idled. Cite the line(s), not the section: a section range taken from the index renders \
         the whole section. {lookup_example}\n\
         — If the request is SILENT and a builder must choose: kind design, cite = the closest lines \
         you read AND the grep that found nothing. {design_example}\n\
         — If it needs the vendor's documentation: kind external, cite = the request line that defers \
         to it. {external_example}\n\
         A spec_lookup whose cite is not a line range rides a lane — that is the one case you \
         searched and found nothing, and its cite is the literal grep command you ran and its 'no \
         match'; a cite you did not run is a false citation. A question that IS one \
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
                                    "cite": {"type": "string", "minLength": 1}
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
            // VA-095: the contract has no `fact` — a lookup's answer is its cited lines, rendered
            // by code at the fan. A model that writes one anyway spent its emit on text the brief
            // already holds; named once with its size (the text was dropped at parse).
            if q.ignored_fact_chars > 0 {
                events.write_value(serde_json::json!({
                    "event": "research_question_fact_ignored",
                    "slice": sl.id,
                    "q_index": sl.questions.len(),
                    "chars": q.ignored_fact_chars,
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
    /// (three names); the parse reads the framed shape into kind/cite, reads r6d's actual
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
        // VA-095: the schema names no `fact` — a lookup's answer is its cited lines.
        assert!(q["properties"].get("fact").is_none());
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
            assert!(
                obj.get("fact").is_none(),
                "no example carries fact text: {obj}"
            );
            examples += 1;
        }
        assert_eq!(examples, 3, "one example per kind");
        assert!(
            rule.contains("there is NO fact field")
                && rule.contains("the cite IS the fact")
                && rule.contains("Cite the line(s), not the section"),
            "the rule says a lookup carries its cite only, and WHY: {rule}"
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
                     "kind": "spec_lookup", "cite": "request.md:148"},
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
        assert_eq!(qs[0].ignored_fact_chars, 0);
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

    /// VA-078: the rule's three examples quote THIS request — its first table row (its cite is
    /// the fact, VA-095), its first rule-stating line (the closest words +
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
        assert!(
            ex[0].get("fact").is_none(),
            "the cite is the fact; the engine renders request.md:7: {}",
            ex[0]
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
        assert_eq!(
            ex[0]["cite"],
            "request.md:<N> or request.md:<A-B>, the line(s) that hold the value"
        );
        assert!(rule.contains("first prose line (request.md:4)"), "{rule}");
        assert!(ex[2]["cite"]
            .as_str()
            .unwrap()
            .starts_with("request.md:3-5 `One`"));

        // Fenced code is neither a table nor prose; a long row is cited, never quoted.
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
        assert_eq!(
            ex[0]["cite"], "request.md:6",
            "a long row is a cite, never a paste"
        );
        assert!(ex[0].get("fact").is_none());

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

    /// VA-095 on r6g's emit (80 `spec_lookup` facts with text, 62 over 200 chars, a 61-minute
    /// opener on one node): the contract carries NO fact — the cite is the fact. A `spec_lookup`
    /// whose cite is a line range of the request file is a cited fact by the cite alone; one
    /// whose cite is the grep that found nothing is a question; a `fact` a model writes anyway is
    /// dropped at parse and named ONCE with its size (`research_question_fact_ignored`), never
    /// carried — and the schema refuses nothing new (a refusal re-streams the whole emit).
    #[test]
    fn a_lookup_is_a_fact_by_its_cite_alone_and_a_written_fact_is_dropped_and_named() {
        let raw: OpenOutputRaw = serde_json::from_value(serde_json::json!({
            "slices": [{
                "id": "ledger-api", "title": "t", "objective": "o", "weight": 3,
                "questions": [
                    {"question": "Which sort keys does sort accept?", "kind": "spec_lookup",
                     "cite": "request.md:148"},
                    {"question": "Camera defaults and clamps?", "kind": "spec_lookup",
                     "cite": "request.md:616-617", "fact": "x".repeat(471)},
                    {"question": "Which header verifies signed webhooks?", "kind": "spec_lookup",
                     "cite": "grep -n -i signature request.md → no match"},
                    {"question": "Which convention for the tokens file?", "kind": "design",
                     "cite": "request.md:51 'tokens'; grep -n -i 'tokens file' → no match"}
                ]
            }]
        }))
        .unwrap();
        let sink = ValueSink::default();
        let out = raw.qualify(&sink);
        let qs = &out.slices[0].questions;
        assert_eq!(qs.len(), 4, "nothing is refused or dropped");
        assert!(qs[0].is_cited_fact());
        assert!(
            qs[1].is_cited_fact(),
            "the stray fact changes nothing about the cite"
        );
        assert_eq!(qs[1].ignored_fact_chars, 471);
        assert!(
            !qs[2].is_cited_fact(),
            "a grep cite is a search that found nothing"
        );
        assert!(
            !qs[3].is_cited_fact(),
            "a design question is never a fact, cite or not"
        );
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "research_question_fact_ignored");
        assert_eq!(events[0]["slice"], "ledger-api");
        assert_eq!(events[0]["q_index"], 1);
        assert_eq!(events[0]["chars"], 471);
        let schema = open_schema();
        let q = &schema["properties"]["slices"]["items"]["properties"]["questions"]["items"];
        assert!(q["properties"].get("fact").is_none());
        assert!(
            q.get("additionalProperties").is_none(),
            "a stray fact is measured, never refused"
        );
    }

    /// `SpecCite`: the range forms the index carries parse (a path prefix and a trailing heading
    /// are ignored); a grep command, a bare label, a zero line and an inverted range do not.
    /// `render` gives the cited lines verbatim (blank lines dropped, trailing whitespace cut) —
    /// a whole section renders whole, visible as `lines`, never selected by code — and names why
    /// a range is not a fact location: past the file, across two sections, blank.
    #[test]
    fn a_spec_cite_parses_a_line_range_and_renders_those_lines_verbatim() {
        assert_eq!(
            SpecCite::parse("request.md:148"),
            Some(SpecCite {
                start: 148,
                end: 148
            })
        );
        assert_eq!(
            SpecCite::parse("request.md:547-565"),
            Some(SpecCite {
                start: 547,
                end: 565
            })
        );
        assert_eq!(
            SpecCite::parse("/run/.swarm/request.md:4-8 `Endpoints`"),
            Some(SpecCite { start: 4, end: 8 })
        );
        assert_eq!(
            SpecCite::parse("request.md:12-"),
            Some(SpecCite { start: 12, end: 12 })
        );
        for not_a_range in [
            "grep -n -i signature request.md → no match",
            "request.md",
            "request.md:",
            "request.md:0",
            "request.md:10-4",
            "request.md:L148",
            "## Endpoints",
            "",
        ] {
            assert_eq!(SpecCite::parse(not_a_range), None, "{not_a_range:?}");
        }
        assert_eq!(SpecCite { start: 4, end: 8 }.span(), 5);

        let spec = "# Title\nintro\n\n## Endpoints\n| Method | Path |\n|---|---|\n\
                    | GET | /api/items |   \n\n## Rules\nThe server MUST answer within 2 seconds.\n";
        assert_eq!(
            SpecCite { start: 7, end: 7 }.render(spec).unwrap(),
            "| GET | /api/items |",
            "one line, trailing whitespace cut"
        );
        assert_eq!(
            SpecCite { start: 5, end: 8 }.render(spec).unwrap(),
            "| Method | Path |\n|---|---|\n| GET | /api/items |",
            "a range inside one section: verbatim lines, the blank line dropped"
        );
        assert_eq!(
            SpecCite { start: 4, end: 8 }.render(spec).unwrap(),
            "## Endpoints\n| Method | Path |\n|---|---|\n| GET | /api/items |",
            "a whole section renders whole"
        );
        assert_eq!(
            SpecCite { start: 10, end: 11 }.render(spec),
            Err("out_of_range")
        );
        assert_eq!(
            SpecCite {
                start: 200,
                end: 200
            }
            .render(spec),
            Err("out_of_range")
        );
        assert_eq!(
            SpecCite { start: 7, end: 10 }.render(spec),
            Err("spans_sections")
        );
        assert_eq!(
            SpecCite { start: 8, end: 8 }.render(spec),
            Err("blank_range")
        );
    }
}
