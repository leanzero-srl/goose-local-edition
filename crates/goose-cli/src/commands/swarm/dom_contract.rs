//! VA-109: DOM-contract routing — a spec section that names a DOM element whose home is the
//! plan's `.html` file reaches the task that OWNS that file.
//!
//! THE DEFECT (r6h, live, 07:18): `console-page` owned `web/index.html, web/styles.css,
//! web/app.js, DECISIONS.md`; `viz-engine` owned `web/viz.js`. The spec requires
//! `<canvas id="viz3d">` (request.md:547, «Rendering — bounded draw calls, demand rendering») and
//! `#viz-labels` (request.md:663, «Screen-space labels — deterministic collision culling») — both
//! sections viz-engine's. console-page's 26,297-char brief carried `viz3d` only as the
//! `window.viz3d.*` brush API (4 hits): `<canvas` 0 hits, `viz-labels` 0 hits; viz-engine's brief
//! carried `<canvas id='viz3d'>` ×2 and `#viz-labels` ×2. console-page wrote `web/index.html`
//! (4,528 B) with NO canvas and NO `#viz-labels`; `initViz()` looked for `#viz3d`, returned false
//! → "3D unavailable" → the graded `vs7dbg` field never drew. The same class as VA-104 (a fact
//! landing only in the slice that owns the section), for SPEC SECTIONS that name DOM ELEMENTS
//! whose home is a file another task owns.
//!
//! THE MECHANISM, from the plan's and the spec's facts only — no word list anywhere: after
//! `finalize_plan_before_dag` and the cross-slice answer routing, every task's OWN spec sections
//! (the opener's claims matched by `heading_key`; an unclaimed section belongs to its nearest
//! claimed ancestor, the document's own nesting) are read line by line for DOM ids by ELEMENT
//! SYNTAX (`dom_ids_in_line`): an `id="X"` / `id='X'` attribute — inside a `<tag …>` the whole tag
//! is the token — or `#X` inside a backtick code span. A heading's `#`, an issue-style `#7`, an
//! entity's `&#39;`, a URL fragment's `docs#x` and a hex colour `#B91C1C` (request.md:416) all
//! fail the SYNTAX (`#` must open the selector, `X` must be an identifier that is not a 3/4/6/8
//! hex digit run), never a list. Every id named in a section of a task that owns NO `.html` file
//! routes to the task(s) that do — as ONE block per receiving task, each id with the citing task,
//! the section title, `request.md:LINE` and that spec line VERBATIM; an id the receiver's own
//! sections already name is skipped (no noise). Ids only: a class (`viz-label`, request.md:663) is
//! created by the script that renders the element, not provided by the page. The mirror — the
//! page's sections naming an id another task's script reads — had 0 measured cases on r6h and is
//! not built (gate 9). A prose `#name` outside backticks is not read either: both r6h tokens are
//! backticked, and an unbackticked `#` in prose is an issue number more often than a selector.
//!
//! Events: `dom_contract_routed{from_task, to_task, files, ids, cites}` per citing→receiving pair;
//! `dom_contract_unowned{ids, cites}` when ids were found and no task owns an `.html` file (loud,
//! MILD — the plan proceeds); `dom_contract_skipped{error}` on an unparseable plan.
//!
//! `finalize_and_route` is the ONE door's tail — repairs → answer routing → DOM contract — and
//! both doors of `plan_slices_to_dag` call it, so a third routing joins in one place and the two
//! doors cannot drift (gate 6). It lives here because VA-109 is the routing whose arrival made
//! two hand-copied door tails one too many.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::answer_routing::{insert_above_decisions, route_cross_slice_answers};
use super::orientation::{heading_key, spec_sections, SpecSection};
use super::research::ResearchRow;
use super::{EventSink, OpenOutput, SliceBrief, TargetLang};

pub(super) const DOM_CONTRACT_HEADER: &str =
    "MUST PROVIDE — DOM ids other tasks' spec sections name";

/// Everything the door's tail reads beside the plan string: the spec and the decision-gate bit
/// (`finalize_plan_before_dag`), the briefs and research (VA-104), the opener's section claims
/// (VA-109), the sink. Built once per planning pass, handed to both doors.
pub(super) struct PlanDoor<'a> {
    pub(super) spec: &'a str,
    pub(super) every_decision_settled: bool,
    /// VA-060: the run's language — rule (c) is Python-only and says so on any other run.
    pub(super) lang: TargetLang,
    pub(super) briefs: &'a [SliceBrief],
    pub(super) research: &'a [ResearchRow],
    pub(super) opened: &'a OpenOutput,
    pub(super) sink: &'a Arc<dyn EventSink>,
}

/// The finalized plan, routed: `finalize_plan_before_dag` (pin → skeleton → repairs → entries),
/// then VA-104's cross-slice answers, then VA-109's DOM contract — the order every door walks.
/// `source` tags the `plan_repaired` event exactly as before ("plan" / "dag_fallback").
pub(super) fn finalize_and_route(plan_json: String, source: &str, door: &PlanDoor<'_>) -> String {
    let plan_json = super::finalize_plan_before_dag(
        plan_json,
        door.spec,
        door.every_decision_settled,
        door.lang,
        door.sink,
        source,
    );
    let events = door.sink.as_ref();
    let plan_json = route_cross_slice_answers(plan_json, door.briefs, door.research, events);
    route_dom_contract(plan_json, door.opened, door.spec, events)
}

/// `index.html` for `web/index.html` — the name a spec section writes when it names a page.
fn basename(file: &str) -> &str {
    file.rsplit('/').next().unwrap_or(file)
}

/// One DOM id a spec line names, as the line wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IdToken {
    pub(super) id: String,
    /// The token exactly as written: `<canvas id="viz3d">`, `id='x'`, `#viz-labels`.
    pub(super) written: String,
    at: usize,
}

/// The byte length of the identifier `[A-Za-z_][A-Za-z0-9_-]*` at the start of `s` (0: none).
fn ident_len(s: &str) -> usize {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return 0,
    }
    chars
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
        .map_or(s.len(), |(i, _)| i)
}

/// `#059669`, `#B91C1C`, `#fff`: a CSS colour is a 3/4/6/8-digit hex run, never an element.
fn is_hex_colour(s: &str) -> bool {
    matches!(s.len(), 3 | 4 | 6 | 8) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Every DOM id `line` names by element syntax, in the order written (module doc).
// string_slice: every index is a `find`/`match_indices` hit, that hit moved past ASCII syntax
// (`<`, `>`, `id=`, a quote, `#`), or an `ident_len` (a `char_indices` offset or `s.len()`) —
// char boundaries by construction.
#[allow(clippy::string_slice)]
pub(super) fn dom_ids_in_line(line: &str) -> Vec<IdToken> {
    let mut out: Vec<IdToken> = Vec::new();
    let mut tag_spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(off) = line[i..].find('<') {
        let start = i + off;
        let rest = &line[start + 1..];
        let close = rest
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .and_then(|_| rest.find('>'));
        match close {
            Some(close) => {
                let end = start + 1 + close + 1;
                tag_spans.push((start, end));
                i = end;
            }
            None => i = start + 1,
        }
    }
    for (at, _) in line.match_indices("id=") {
        if line[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '#'))
        {
            continue;
        }
        let after = &line[at + 3..];
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            continue;
        };
        let value = &after[1..];
        let len = ident_len(value);
        if len == 0 || !value[len..].starts_with(quote) {
            continue;
        }
        let id = &value[..len];
        let tag = tag_spans.iter().find(|(s, e)| *s <= at && at < *e);
        out.push(IdToken {
            id: id.to_string(),
            written: match tag {
                Some((s, e)) => line[*s..*e].to_string(),
                None => format!("id={quote}{id}{quote}"),
            },
            at: tag.map_or(at, |(s, _)| *s),
        });
    }
    let mut offset = 0;
    for (n, span) in line.split('`').enumerate() {
        if n % 2 == 1 {
            for (h, _) in span.match_indices('#') {
                let opens = span[..h].chars().next_back().is_none_or(|c| {
                    !(c.is_ascii_alphanumeric() || matches!(c, '_' | '&' | '#' | '-' | '.'))
                });
                let len = ident_len(&span[h + 1..]);
                if !opens || len == 0 || is_hex_colour(&span[h + 1..h + 1 + len]) {
                    continue;
                }
                out.push(IdToken {
                    id: span[h + 1..h + 1 + len].to_string(),
                    written: span[h..h + 1 + len].to_string(),
                    at: offset + h,
                });
            }
        }
        offset += span.len() + 1;
    }
    out.sort_by_key(|t| t.at);
    out
}

/// One plan task as the routing reads it.
struct TaskRow<'a> {
    id: &'a str,
    /// Its `.html` files — the pages whose markup DOM ids live in.
    html: Vec<&'a str>,
}

/// One id a task's section named, with where.
struct Mention<'a> {
    from: usize,
    token: IdToken,
    section: &'a str,
    line_no: usize,
    line: &'a str,
    body: &'a str,
}

/// Each section's claimant tasks: the tasks whose slice claimed the heading (`heading_key`), or
/// the nearest claimed ancestor's when the section itself was not claimed.
fn claimants(sections: &[SpecSection], claims: &[(usize, Vec<String>)]) -> Vec<Vec<usize>> {
    let direct: Vec<Vec<usize>> = sections
        .iter()
        .map(|s| {
            let key = heading_key(&s.heading);
            claims
                .iter()
                .filter(|(_, headings)| headings.iter().any(|h| heading_key(h) == key))
                .map(|(t, _)| *t)
                .collect()
        })
        .collect();
    (0..sections.len())
        .map(|i| {
            let mut j = i;
            loop {
                if !direct[j].is_empty() || sections[j].heading.is_empty() {
                    return direct[j].clone();
                }
                let level = sections[j].level;
                match (0..j)
                    .rev()
                    .find(|p| !sections[*p].heading.is_empty() && sections[*p].level < level)
                {
                    Some(p) => j = p,
                    None => return Vec::new(),
                }
            }
        })
        .collect()
}

fn cite(line_no: usize) -> String {
    format!("request.md:{line_no}")
}

/// Route every DOM id a task's spec sections name into the description of the task that owns the
/// page (module doc). Byte-identical when nothing routes.
pub(super) fn route_dom_contract(
    plan_json: String,
    opened: &OpenOutput,
    spec: &str,
    events: &dyn EventSink,
) -> String {
    let mut plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            // Not a substitution: the same string reaches `Dag::from_planner_json`, whose refusal
            // is the loud `synthesis_fallback`; this event says the routing saw it first.
            events.write_value(serde_json::json!({
                "event": "dom_contract_skipped",
                "error": e.to_string(),
            }));
            return plan_json;
        }
    };
    let sections = spec_sections(spec);
    let rendered: BTreeMap<String, String> = {
        let Some(tasks) = plan.get("subtasks").and_then(|t| t.as_array()) else {
            events.write_value(serde_json::json!({
                "event": "dom_contract_skipped",
                "error": "the plan has no `subtasks` array",
            }));
            return plan_json;
        };
        let mut rows: Vec<TaskRow> = Vec::new();
        let mut claims: Vec<(usize, Vec<String>)> = Vec::new();
        for t in tasks {
            let Some(id) = t.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let slice = t
                .get("slice")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(id);
            let html: Vec<&str> = t
                .get("files")
                .and_then(|f| f.as_array())
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
                .filter(|f| {
                    let lower = f.to_lowercase();
                    lower.ends_with(".html") || lower.ends_with(".htm")
                })
                .collect();
            if let Some(sl) = opened.slices.iter().find(|sl| sl.id == slice) {
                claims.push((rows.len(), sl.sections.clone()));
            }
            rows.push(TaskRow { id, html });
        }
        let owners: Vec<usize> = (0..rows.len())
            .filter(|i| !rows[*i].html.is_empty())
            .collect();
        let mut mentions: Vec<Mention> = Vec::new();
        let mut own_ids: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (i, claimed_by) in claimants(&sections, &claims).iter().enumerate() {
            let s = &sections[i];
            let first_body_line = if s.heading.is_empty() {
                s.line_start
            } else {
                s.line_start + 1
            };
            for (k, line) in s.body.lines().enumerate() {
                for token in dom_ids_in_line(line) {
                    for from in claimed_by {
                        own_ids.entry(*from).or_default().push(token.id.clone());
                        mentions.push(Mention {
                            from: *from,
                            token: token.clone(),
                            section: &s.heading,
                            line_no: first_body_line + k,
                            line,
                            body: &s.body,
                        });
                    }
                }
            }
        }
        if mentions.is_empty() {
            return plan_json;
        }
        if owners.is_empty() {
            let mut ids: Vec<&str> = Vec::new();
            for m in &mentions {
                if !ids.contains(&m.token.id.as_str()) {
                    ids.push(&m.token.id);
                }
            }
            events.write_value(serde_json::json!({
                "event": "dom_contract_unowned",
                "ids": ids,
                "cites": mentions.iter().map(|m| cite(m.line_no)).collect::<Vec<_>>(),
            }));
            return plan_json;
        }
        // (receiver → (id → (written, first cite's section/line/text, every cite))), plus the
        // receiver's targeted pages and per citing task the ids and cites for the event.
        struct Item<'a> {
            written: String,
            from: &'a str,
            section: &'a str,
            line_no: usize,
            line: &'a str,
            also: Vec<(String, &'a str)>,
        }
        let mut items: BTreeMap<usize, Vec<(String, Item)>> = BTreeMap::new();
        let mut pages: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
        let mut routed: BTreeMap<(usize, usize), (Vec<String>, Vec<String>)> = BTreeMap::new();
        for m in &mentions {
            if !rows[m.from].html.is_empty() {
                continue;
            }
            let named: Vec<usize> = owners
                .iter()
                .copied()
                .filter(|o| rows[*o].html.iter().any(|f| m.body.contains(basename(f))))
                .collect();
            let targets = if named.is_empty() { &owners } else { &named };
            for to in targets {
                if own_ids.get(to).is_some_and(|ids| ids.contains(&m.token.id)) {
                    continue;
                }
                let page = pages.entry(*to).or_default();
                for f in &rows[*to].html {
                    let targeted = named.is_empty() || m.body.contains(basename(f));
                    if targeted && !page.contains(f) {
                        page.push(f);
                    }
                }
                let list = items.entry(*to).or_default();
                match list.iter_mut().find(|(id, _)| *id == m.token.id) {
                    Some((_, item)) => item.also.push((cite(m.line_no), m.section)),
                    None => list.push((
                        m.token.id.clone(),
                        Item {
                            written: m.token.written.clone(),
                            from: rows[m.from].id,
                            section: m.section,
                            line_no: m.line_no,
                            line: m.line,
                            also: Vec::new(),
                        },
                    )),
                }
                let (ids, cites) = routed.entry((m.from, *to)).or_default();
                if !ids.contains(&m.token.id) {
                    ids.push(m.token.id.clone());
                }
                cites.push(cite(m.line_no));
            }
        }
        for ((from, to), (ids, cites)) in &routed {
            events.write_value(serde_json::json!({
                "event": "dom_contract_routed",
                "from_task": rows[*from].id,
                "to_task": rows[*to].id,
                "files": pages[to],
                "ids": ids,
                "cites": cites,
            }));
        }
        items
            .into_iter()
            .map(|(to, list)| {
                let files = pages[&to].join(", ");
                let lines: Vec<String> = list
                    .iter()
                    .map(|(_, it)| {
                        let also = if it.also.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "; also {}",
                                it.also
                                    .iter()
                                    .map(|(c, s)| format!("{c} («{s}»)"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };
                        format!(
                            "- `{}` — named by {}'s section «{}», {}: \"{}\"{also}",
                            it.written,
                            it.from,
                            it.section,
                            cite(it.line_no),
                            it.line.trim()
                        )
                    })
                    .collect();
                let block = format!(
                    "\n\nYOUR {files} {DOM_CONTRACT_HEADER} — the section is theirs, the element's \
                     home is your page; their scripts look these up by id when they load, so each \
                     exists in your static markup (routed by the engine from the spec lines below — \
                     provide the element, do not implement what their sections describe):\n{}",
                    lines.join("\n")
                );
                (rows[to].id.to_string(), block)
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

#[cfg(test)]
mod tests {
    use super::super::decisions::SETTLED_DECISIONS_HEADER;
    use super::super::opener::OpenSlice;
    use super::super::SwarmEvent;
    use super::*;
    use std::sync::Mutex;

    /// r6h's request file verbatim (`.swarm/request.md`, 873 lines): line 547 is the canvas,
    /// 663 the labels container, 416 the four hex colours, 381–446 console-page's own ids.
    const R6H_REQUEST: &str = include_str!("testdata/va109/request.md");
    const RENDERING: &str = "Rendering — bounded draw calls, demand rendering";
    const LABELS: &str = "Screen-space labels — deterministic collision culling";
    const WEB: &str = "7. `web/` — the frontend";
    const FIELD: &str = "8. The 3D field — 12,288 instances, five mechanisms";

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

    fn slice(id: &str, sections: &[&str]) -> OpenSlice {
        OpenSlice {
            id: id.to_string(),
            title: id.to_string(),
            objective: String::new(),
            weight: 3,
            sections: sections.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn opened(slices: Vec<OpenSlice>) -> OpenOutput {
        OpenOutput {
            slices,
            open_decisions: Vec::new(),
        }
    }

    // r6h's plan-loaded.json ownership for the two web tasks, verbatim; console-page's brief
    // carries a decisions block so the insertion point is exercised.
    const VIZ_BRIEF: &str = "Build web/viz.js: the 3D field, window.viz3d.* brush API, vs7dbg.";

    fn r6h_plan(console_files: &[&str]) -> String {
        let console_brief = format!(
            "Build the console page as a product, not a debug view. Owns: web/index.html \
             (structure only), web/styles.css (all styling), web/app.js (page behavior: table, \
             filters, sync, notes, workflow, notifications), DECISIONS.md (no other slice touches \
             these).\n\n{SETTLED_DECISIONS_HEADER} BY RESEARCH that name this slice:\n- D2 rejected \
             drafts are terminal."
        );
        serde_json::json!({"subtasks": [
            {"id": "ledgerd-core", "slice": "ledgerd-core", "files": ["app/ledgerd/impl.py", "app/api.py"],
             "depends_on": [], "description": "ledgerd"},
            {"id": "console-page", "slice": "console-page", "files": console_files,
             "depends_on": ["ledgerd-core"], "description": console_brief},
            {"id": "viz-engine", "slice": "viz-engine", "files": ["web/viz.js"],
             "depends_on": ["ledgerd-core"], "description": VIZ_BRIEF},
            {"id": goose_swarm::SINK_ID, "files": [], "depends_on": ["console-page", "viz-engine"],
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

    /// THE r6h CASE: viz-engine's two sections name `<canvas id="viz3d">` (547) and `#viz-labels`
    /// (663); console-page owns the only `.html`, so its brief gains ONE block carrying both, each
    /// with the citing task, the section title, the line cite and the spec line verbatim — above
    /// its decisions block. console-page's OWN ids (`#app-header`, `#viz-empty`, request.md:381,
    /// 445) do not ride: the citing task owns the page. viz-engine's brief is byte-identical; one
    /// `dom_contract_routed`; nothing unowned.
    // string_slice: `block_at` is a `find` hit.
    #[allow(clippy::string_slice)]
    #[test]
    fn r6h_canvas_and_labels_container_reach_the_index_html_owner() {
        let plan = r6h_plan(&[
            "web/index.html",
            "web/styles.css",
            "web/app.js",
            "DECISIONS.md",
        ]);
        let opened = opened(vec![
            slice("console-page", &[WEB]),
            slice("viz-engine", &[RENDERING, LABELS]),
        ]);
        let sink = ValueSink::default();
        let routed = route_dom_contract(plan, &opened, R6H_REQUEST, &sink);
        let console = description_of(&routed, "console-page");
        let block_at = console
            .find("YOUR web/index.html MUST PROVIDE — DOM ids other tasks' spec sections name")
            .expect("the block lands");
        assert!(
            block_at < console.find(SETTLED_DECISIONS_HEADER).unwrap(),
            "above the decisions partition:\n{console}"
        );
        assert!(
            console.contains(
                "- `<canvas id=\"viz3d\">` — named by viz-engine's section «Rendering — bounded \
                 draw calls, demand rendering», request.md:547: \"- `<canvas id=\"viz3d\">`, \
                 context `webgl` or `webgl2` created\"\n\
                 - `#viz-labels` — named by viz-engine's section «Screen-space labels — \
                 deterministic collision culling», request.md:663: \"- **Geometry:** each label \
                 is a DOM element in `#viz-labels` (absolutely positioned over the\""
            ),
            "{console}"
        );
        for own in [
            "app-header",
            "viz-empty",
            "viz-error",
            "draft-form",
            "brush-count",
        ] {
            assert!(
                !console[block_at..].contains(own),
                "{own} is console-page's own (or unclaimed) — never routed to it:\n{console}"
            );
        }
        assert_eq!(description_of(&routed, "viz-engine"), VIZ_BRIEF);
        assert_eq!(
            sink.named("dom_contract_routed"),
            vec![
                serde_json::json!({"event": "dom_contract_routed", "from_task": "viz-engine",
                "to_task": "console-page", "files": ["web/index.html"],
                "ids": ["viz3d", "viz-labels"], "cites": ["request.md:547", "request.md:663"]})
            ]
        );
        assert!(sink.named("dom_contract_unowned").is_empty());
        assert!(sink.named("dom_contract_skipped").is_empty());
    }

    /// The document's own nesting: viz-engine claims §8 and none of its `####` children — the
    /// children belong to the nearest claimed ancestor, so the canvas, the labels container AND
    /// §8's brush counter (`#brush-count`, request.md:677) route; console-page's §7 claim never
    /// reaches into §8.
    #[test]
    fn an_unclaimed_child_section_belongs_to_its_nearest_claimed_ancestor() {
        let plan = r6h_plan(&["web/index.html", "web/app.js"]);
        let opened = opened(vec![
            slice("console-page", &[WEB]),
            slice("viz-engine", &[FIELD]),
        ]);
        let sink = ValueSink::default();
        let routed = route_dom_contract(plan, &opened, R6H_REQUEST, &sink);
        let ev = sink.named("dom_contract_routed");
        assert_eq!(ev.len(), 1, "{ev:?}");
        let ids: Vec<&str> = ev[0]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&"viz3d") && ids.contains(&"viz-labels") && ids.contains(&"brush-count"),
            "{ids:?}"
        );
        assert!(description_of(&routed, "console-page").contains(
            "- `#brush-count` — named by viz-engine's section «The linked brush — table ⇄ \
             instances», request.md:677:"
        ));
    }

    /// LOUD, MILD: no task owns a page, so the ids viz-engine's sections name are reported as
    /// `dom_contract_unowned` with their cites and the plan is byte-identical.
    #[test]
    fn ids_with_no_page_owner_are_named_unowned_and_the_plan_is_untouched() {
        let plan = r6h_plan(&["web/app.js"]);
        let opened = opened(vec![slice("viz-engine", &[RENDERING, LABELS])]);
        let sink = ValueSink::default();
        let out = route_dom_contract(plan.clone(), &opened, R6H_REQUEST, &sink);
        assert_eq!(out, plan);
        assert_eq!(
            sink.named("dom_contract_unowned"),
            vec![serde_json::json!({"event": "dom_contract_unowned",
                "ids": ["viz3d", "viz-labels"], "cites": ["request.md:547", "request.md:663"]})]
        );
        assert!(sink.named("dom_contract_routed").is_empty());
    }

    /// An unparseable plan is named, never rewritten; a plan whose sections name no id is
    /// byte-identical with no event.
    #[test]
    fn unparseable_plan_is_skipped_loudly_and_a_silent_spec_routes_nothing() {
        let sink = ValueSink::default();
        let out = route_dom_contract("{not json".to_string(), &opened(vec![]), R6H_REQUEST, &sink);
        assert_eq!(out, "{not json");
        assert_eq!(sink.named("dom_contract_skipped").len(), 1);
        let plan = r6h_plan(&["web/index.html"]);
        let sink = ValueSink::default();
        let out = route_dom_contract(plan.clone(), &opened(vec![slice("viz-engine", &[RENDERING])]), "# Title\n\n## Rendering — bounded draw calls, demand rendering\n\nDraw at most 8 calls; see issue #7 and `## 7. web/`.\n", &sink);
        assert_eq!(out, plan);
        assert!(sink.0.lock().unwrap().is_empty());
    }

    /// THE TOKEN RULE, on one line: a heading's `#`, `#7`, `&#39;`, the hex colours of
    /// request.md:416, a fragment `docs#x` and `data-id=` are not ids; `<canvas id='viz3d'>`
    /// carries the whole tag, a bare `id="x-1"` its attribute, `#viz-labels` the selector.
    #[test]
    fn dom_ids_are_read_by_element_syntax_only() {
        let line = "## 7. web/ — see #7, `#B91C1C` `#059669` `#fff`, `&#39;`, `docs#anchor`, \
                    `## 7. web/`, data-id=\"9\", <canvas id='viz3d' class=\"x\">, attribute \
                    id=\"x-1\" and `#viz-labels`; `div#viz3d` is not read";
        let got: Vec<(String, String)> = dom_ids_in_line(line)
            .into_iter()
            .map(|t| (t.id, t.written))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "viz3d".to_string(),
                    "<canvas id='viz3d' class=\"x\">".to_string()
                ),
                ("x-1".to_string(), "id=\"x-1\"".to_string()),
                ("viz-labels".to_string(), "#viz-labels".to_string()),
            ]
        );
        assert_eq!(
            dom_ids_in_line(R6H_REQUEST.lines().nth(546).unwrap())[0].written,
            "<canvas id=\"viz3d\">"
        );
        assert_eq!(
            dom_ids_in_line(R6H_REQUEST.lines().nth(415).unwrap()),
            Vec::<IdToken>::new(),
            "request.md:416's four status colours"
        );
    }
}
