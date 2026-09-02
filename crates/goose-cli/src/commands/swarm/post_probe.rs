//! The advertised-POST probe's verdict vocabulary: what calling a mutating endpoint TWICE proves
//! (`repeated_post_verdict`), the curl status split it reads, and the evidence a finding carries
//! about the request the gate actually sent. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases); the enum, both functions and their
//! test moved verbatim from swarm.rs.
//!
//! VA-088 — THE RENDER CHECK (DESIGN-REPAIR-V2 §3), the gate's other post-boot probe, lives here
//! too: a GENERAL browser check derived from the spec's own identifiers, never from a benchmark's
//! names. `spec_render_surface` reads every element id the spec names by element syntax
//! (`dom_contract::dom_ids_in_line` — `<canvas id="viz3d">`, `#viz-labels`) and every member of
//! a global the spec advertises (`window.X` → the `m(` lines of the `window.X = {` literal it
//! writes, plus any `X.m(` it cites). One headless Chromium page (`--use-angle=gl`; the
//! playwright the scorer already uses, resolved from the render probe's own directory) then
//! answers: is each advertised id in the live DOM, is each advertised member `typeof
//! 'function'`, how many `pageerror`s fired while loading, and does every `<canvas>` hold a
//! context AND show more than one colour in a screenshot clipped to it. Each answer is a
//! `FindingSource::RenderCheck*` finding worded from the spec line that advertised the fact
//! (`render_check_verdict`), or `verified`. No browser is a LOUD `render_check_unavailable{reason}`
//! (`RenderCheck::event`), never a silent pass; a fact the probe could not measure is named in
//! `unmeasured`, never counted either way. sb-7's `product_probe_v3.mjs` (`viz3d`, `vs7dbg`
//! literals) is the tier-specific probe this check replaces the need for — the engine reads
//! the spec, the scorer keeps its own script.

use super::dom_contract::dom_ids_in_line;
use super::findings::FindingSource;

/// What calling an advertised mutating endpoint TWICE proves about idempotency.
///
/// WHY THIS EXISTS. `run_spec_contract` issues only bare GETs, and `spec_unprobed_advertised`
/// merely NAMES the POSTs it skips. So every requirement that lives behind a POST is invisible to
/// the engine's own contract gate, and the fix loop never sees it.
///
/// MEASURED, and this is the whole reason: across the four best 3-node cells on the current binary,
/// `vendor_conditional` (mean 0.25) and `resync_conditional_ratio` (mean 0.25) are **44% of ALL
/// remaining weighted score loss** — the single largest block on the board. Both are the spec's own
/// sentence "the tool is run repeatedly against the same database; a second sync must be cheap and
/// must not duplicate rows". One cell's detail reads "13 requests carried If-None-Match, 0 answered
/// 304": the app KNOWS to send the header and still re-downloads everything. Another scores 1.00
/// ("3 requests carried If-None-Match, 3 answered 304"), so it is achievable and unreliable — 3 of
/// 4 cells fail it and nothing in the engine ever told them.
///
/// The verdict is decidable from the RESPONSE BODIES ALONE — no visibility into vendor traffic is
/// needed — which is what makes it cheap enough to run in the contract gate.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum RepeatedPost {
    Idempotent,
    Duplicates(String),
    /// Correct on rows and WASTEFUL on the wire: inserted nothing, but re-downloaded the collection
    /// it already had.
    ///
    /// ⚠️ THIS ARM EXISTS BECAUSE THE FIRST VERSION OF THIS FUNCTION SCORED THE FAILING APP AS A
    /// PASS. It returned `Idempotent` on `inserted == 0` with a flat `total` — which is EXACTLY the
    /// signature of the defect: `vendor_conditional` fails while rows stay correct, because the app
    /// re-fetches every page and then upserts them to no effect. The spec asks for two things, "a
    /// second sync must be CHEAP **and** must not duplicate rows", and the check only tested the
    /// second. A gate that passes the thing it was built to catch is worse than no gate.
    NotCheap(String),
    /// The FIRST call did no work, so a second call that also does nothing proves nothing.
    ///
    /// ⚠️ THIS IS THE SAME MISTAKE AS `NotCheap`, ONE LEVEL DOWN, AND IT SHIPPED. The measured
    /// signature is `sync_completeness 0/247 payments after one sync` with `second sync inserted=0
    /// total=0`: an app whose sync brings back NOTHING. Every arm above reads that as healthy —
    /// `inserted` is not > 0, `total` does not grow, `fetched` is 0 so the cheapness branch returns
    /// `Idempotent` — and `Idempotent` increments `verified`, the counter whose entire purpose is to
    /// let a consumer tell a real pass from "checked nothing". **Nothing happened twice is not
    /// idempotency; it is an empty app.** Being inconclusive, this never blames the app for a vendor
    /// that legitimately has no rows.
    Vacuous(String),
    /// Either body is not a JSON OBJECT — the one shape the spec documents for every endpoint.
    /// FAIL-OPEN on idempotency: says nothing about duplication.
    NotJson,
    /// Both bodies ARE JSON objects and neither carries a field that speaks to idempotency
    /// (`fetched`/`inserted`/`total`). Says nothing about duplication; what the pair DOES prove
    /// is decided by the caller from the HTTP status and the documented keys. r6c: this and
    /// `NotJson` were ONE arm (`Unreadable`), and the caller's text for it — "could not be read
    /// as JSON on either probe" / "does not carry the documented field(s)" — described five
    /// findings whose bodies were well-formed JSON 401 envelopes (`{"error": {"code":
    /// "unauthorized", …}}`, replayed against the archived tree); four lanes reported FIXED on
    /// them and the gate re-filed the same words every round.
    NoIdempotencyField,
}

/// Split `curl -w "\n%{http_code}"` output into (body, code). Pure, so the parse can be pinned.
///
/// The body is whatever precedes the final line, because a JSON body may itself contain newlines
/// and taking the FIRST line would truncate any pretty-printed response into invalid JSON — which
/// `repeated_post_verdict` would then read as Unreadable, converting a decidable case into an
/// abstention. A missing or non-numeric trailing line yields code 0, which is below every threshold
/// and so can never manufacture a finding.
pub(super) fn split_curl_status(out: &str) -> (&str, u16) {
    match out.rsplit_once('\n') {
        Some((body, tail)) => (body, tail.trim().parse().unwrap_or(0)),
        None => (out, 0),
    }
}

pub(super) fn repeated_post_verdict(first: &str, second: &str) -> RepeatedPost {
    let (Ok(a), Ok(b)) = (
        serde_json::from_str::<serde_json::Value>(first),
        serde_json::from_str::<serde_json::Value>(second),
    ) else {
        return RepeatedPost::NotJson;
    };
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return RepeatedPost::NotJson;
    };
    // DID THE FIRST CALL DO ANYTHING AT ALL? Every arm below compares the second call to the first,
    // so a first call that fetched nothing, inserted nothing and left an empty collection makes all
    // of them vacuous. Some(true) = work is evidenced, Some(false) = the fields are present and all
    // zero, None = no counter is present so this says nothing either way.
    let worked = |o: &serde_json::Map<String, serde_json::Value>| -> Option<bool> {
        let mut seen = false;
        for k in ["fetched", "inserted", "total"] {
            if let Some(n) = o.get(k).and_then(|v| v.as_u64()) {
                seen = true;
                if n > 0 {
                    return Some(true);
                }
            }
        }
        seen.then_some(false)
    };
    if worked(a) == Some(false) {
        return RepeatedPost::Vacuous(
            "the first sync fetched nothing, inserted nothing and left the collection empty, so a \
             second call that also does nothing establishes no idempotency"
                .to_string(),
        );
    }
    // A second identical call must insert NOTHING.
    if let Some(ins) = b.get("inserted").and_then(|v| v.as_u64()) {
        if ins > 0 {
            return RepeatedPost::Duplicates(format!("the second call inserted {ins} more row(s)"));
        }
    }
    // ...and must not grow the collection.
    if let (Some(t1), Some(t2)) = (
        a.get("total").and_then(|v| v.as_u64()),
        b.get("total").and_then(|v| v.as_u64()),
    ) {
        if t2 != t1 {
            return RepeatedPost::Duplicates(format!("total went {t1} -> {t2} on a repeat call"));
        }
    }
    // THE CHEAPNESS HALF. Rows being correct is necessary and not sufficient: an app that re-pulls
    // every page and upserts it changes nothing and still burns the quota the spec is protecting.
    // `fetched` is the documented field that distinguishes them, so when both bodies carry it the
    // verdict is decidable; when either lacks it, fail open rather than guess.
    if let (Some(f1), Some(f2)) = (
        a.get("fetched").and_then(|v| v.as_u64()),
        b.get("fetched").and_then(|v| v.as_u64()),
    ) {
        if f1 > 0 && f2 >= f1 {
            // F797: 4 of 4 recent builds fail this and the repair loop has never cracked it from
            // the bare symptom — the finding now CARRIES the named fix (repair-directed, the same
            // measured pattern as smoke_fix_description's root-cause ask): conditional requests
            // keyed per page, not one ETag replayed against every page.
            return RepeatedPost::NotCheap(format!(
                "the second sync re-fetched {f2} row(s) it already had. FIX: make the client send \
                 If-None-Match per page — store each page's ETag keyed by (path, offset, limit) \
                 from the first sync and replay THAT page's ETag on the matching request; treat \
                 304 as 'page unchanged, keep local rows'. One ETag replayed on every page never \
                 matches and re-fetches everything"
            ));
        }
        return RepeatedPost::Idempotent;
    }
    if a.get("total").and_then(|v| v.as_u64()).is_some()
        || b.get("inserted").and_then(|v| v.as_u64()).is_some()
    {
        // Rows proven correct, cheapness UNPROVEN because the app advertises no `fetched`. Say so
        // rather than banking a pass the evidence does not support.
        return RepeatedPost::NoIdempotencyField;
    }
    RepeatedPost::NoIdempotencyField
}

/// The body as a JSON OBJECT, or None — the one parse the probe's arms share.
pub(super) fn json_object(body: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    serde_json::from_str::<serde_json::Value>(body.trim())
        .ok()?
        .as_object()
        .cloned()
}

/// Is `key` present ANYWHERE in the value — top level or nested? The spec's documented keys
/// are read from the RESPONSE side of ONE table cell by a `"key":` regex (`spec_documented_keys`,
/// S6), which flattens a nested shape: sb-7's `GET /notify/notifications` row documents
/// `{"data": [{"id", "event_seq", "kind", "message", "at"}...], "total"}`, so `id`..`at` are
/// documented keys that a CORRECT response carries one level down inside `data`. (The
/// `/api/drafts` row's `counterparty: {name, country}` is its REQUEST body; since S6 that row
/// documents no response key at all.) A top-level-only check would file "does not carry `id`"
/// against a handler that does.
fn json_has_key(v: &serde_json::Value, key: &str) -> bool {
    match v {
        serde_json::Value::Object(o) => {
            o.contains_key(key) || o.values().any(|x| json_has_key(x, key))
        }
        serde_json::Value::Array(a) => a.iter().any(|x| json_has_key(x, key)),
        _ => false,
    }
}

/// Which of the spec's documented keys the body does NOT carry (anywhere), in documented
/// order. A body that is not a JSON object is missing every key — the caller has already
/// separated that case (`NotJson`) and words it as such.
pub(super) fn missing_documented_keys(body: &str, documented: &[String]) -> Vec<String> {
    let parsed =
        serde_json::from_str::<serde_json::Value>(body.trim()).unwrap_or(serde_json::Value::Null);
    documented
        .iter()
        .filter(|k| !json_has_key(&parsed, k))
        .cloned()
        .collect()
}

/// The first `max` chars of a response body on ONE line, for a finding: whitespace runs
/// collapse to one space so a pretty-printed envelope reads as a sentence; `«»` delimit it so
/// the body's own quotes and backticks cannot end the quote early. An empty body says so.
fn body_snippet(body: &str, max: usize) -> String {
    let one_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.is_empty() {
        return "empty body".to_string();
    }
    let n = one_line.chars().count();
    if n > max {
        let head: String = one_line.chars().take(max).collect();
        format!("«{head}…» ({n} chars)")
    } else {
        format!("«{one_line}»")
    }
}

/// THE EVIDENCE a POST-probe finding carries, so a repair shard reproduces the GATE'S request
/// instead of guessing at it. r6c: the gate's `curl -s -w '\n%{http_code}' -X POST -m 20 <url>`
/// sends NO body and NO headers, and the finding said only "could not be read as JSON on
/// either probe"; four lanes booted the tree, tried "8 realistic variants" with tokens and
/// bodies, saw JSON every time, reported FIXED, and the gate re-filed the identical words —
/// they never learned the probe was a bare unauthenticated POST answered by a 401 envelope.
/// Each probe's status and body head are stated; a status of 0 is curl's "no HTTP response
/// within the budget" — boot latency or a hang, a different defect class from a non-JSON body,
/// and a shard must be able to tell them apart. `budget_secs` is the probe's own curl `-m`
/// (POST_PROBE_SECS, quoted as data — it bounds the app under test, never a model).
pub(super) fn post_probe_evidence(
    path: &str,
    budget_secs: u64,
    probes: &[(u16, &str)],
    boot: &str,
    port: u16,
) -> String {
    let mut out = format!(
        "PROBE EVIDENCE — request as sent: `POST {path}` with NO body and NO headers (bare \
         `curl -X POST`, {budget_secs}s budget)"
    );
    for (i, (code, body)) in probes.iter().enumerate() {
        if *code == 0 {
            out.push_str(&format!(
                "; probe {}: no HTTP response within {budget_secs}s",
                i + 1
            ));
        } else {
            out.push_str(&format!(
                "; probe {}: HTTP {code}, body {}",
                i + 1,
                body_snippet(body, 200)
            ));
        }
    }
    out.push('.');
    // S5d (iii): the gate's own boot argv and a copy-paste replay — so the shard reproduces
    // the GATE'S request against the GATE'S app, never "8 realistic variants" of its own.
    out.push_str(&format!(
        " REPLAY IT: boot exactly as the gate did — `{boot}` — then `curl -s -w '\\n%{{http_code}}' \
         -X POST -m {budget_secs} http://127.0.0.1:{port}{path}`; a NOT REAL verdict must quote \
         that command's status and body."
    ));
    out
}

/// One DOM id the spec advertises, with the line that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpecElement {
    pub(super) id: String,
    /// As the spec wrote it: `<canvas id="viz3d">`, `#viz-labels`.
    pub(super) written: String,
    pub(super) line_no: usize,
}

/// One member of a global the spec advertises: `window.vs7dbg.frames()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpecApiMember {
    pub(super) object: String,
    pub(super) member: String,
    pub(super) line_no: usize,
}

/// The spec's render-side identifiers (module doc, VA-088).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct RenderSurface {
    pub(super) elements: Vec<SpecElement>,
    pub(super) members: Vec<SpecApiMember>,
}

fn push_member(out: &mut RenderSurface, object: &str, member: &str, line_no: usize) {
    if !out
        .members
        .iter()
        .any(|m| m.object == object && m.member == member)
    {
        out.members.push(SpecApiMember {
            object: object.to_string(),
            member: member.to_string(),
            line_no,
        });
    }
}

/// Every element id and advertised-global member the spec names, first cite wins. Pure.
pub(super) fn spec_render_surface(spec: &str) -> RenderSurface {
    let mut out = RenderSurface::default();
    for (k, line) in spec.lines().enumerate() {
        for t in dom_ids_in_line(line) {
            if !out.elements.iter().any(|e| e.id == t.id) {
                out.elements.push(SpecElement {
                    id: t.id,
                    written: t.written,
                    line_no: k + 1,
                });
            }
        }
    }
    let (Ok(global), Ok(literal_member)) = (
        regex::Regex::new(r"window\.([A-Za-z_$][\w$]*)"),
        regex::Regex::new(r"^\s*([A-Za-z_$][\w$]*)\s*\("),
    ) else {
        return out;
    };
    let mut objects: Vec<String> = Vec::new();
    for c in global.captures_iter(spec) {
        let name = c[1].to_string();
        if !objects.contains(&name) {
            objects.push(name);
        }
    }
    for object in objects {
        let (Ok(member_call), Ok(literal_open)) = (
            regex::Regex::new(&format!(
                r"\b{}\.([A-Za-z_$][\w$]*)\s*\(",
                regex::escape(&object)
            )),
            regex::Regex::new(&format!(r"window\.{}\s*=\s*\{{", regex::escape(&object))),
        ) else {
            continue;
        };
        let mut in_literal = false;
        for (k, line) in spec.lines().enumerate() {
            for c in member_call.captures_iter(line) {
                push_member(&mut out, &object, &c[1], k + 1);
            }
            if in_literal {
                if line.trim_start().starts_with('}') {
                    in_literal = false;
                } else if let Some(c) = literal_member.captures(line) {
                    push_member(&mut out, &object, &c[1], k + 1);
                }
            } else if literal_open.is_match(line) {
                in_literal = true;
            }
        }
    }
    out
}

/// The replay facts every render-check finding carries in its GATE COMMAND sentence.
pub(super) struct RenderReplay<'a> {
    pub(super) boot_line: &'a str,
    pub(super) command: &'a str,
}

/// What the render check measured, or exactly why it could not.
#[derive(Debug, Default)]
pub(super) struct RenderCheck {
    pub(super) ran: bool,
    /// Why the check did not run — never empty while `ran` is false.
    pub(super) reason: String,
    pub(super) findings: Vec<(FindingSource, String)>,
    /// Advertised facts the browser affirmed: an id present, a member a function, zero page
    /// errors, a canvas that drew.
    pub(super) verified: usize,
    /// Facts the probe could not measure or did not report — named, never counted either way.
    pub(super) unmeasured: Vec<String>,
    pub(super) elements: usize,
    pub(super) elements_missing: Vec<String>,
    pub(super) members: usize,
    pub(super) members_missing: Vec<String>,
    pub(super) page_errors: usize,
    pub(super) canvases: usize,
    pub(super) canvases_blank: Vec<String>,
}

impl RenderCheck {
    fn unavailable(reason: String) -> RenderCheck {
        RenderCheck {
            ran: false,
            reason,
            ..Default::default()
        }
    }

    /// One line for the gate's status field: what ran, in numbers, or why not.
    pub(super) fn status(&self) -> String {
        if !self.ran {
            return format!("unavailable ({})", self.reason);
        }
        format!(
            "ran (elements {} missing {}, members {} missing {}, page_errors {}, canvases {} blank {}, unmeasured {})",
            self.elements,
            self.elements_missing.len(),
            self.members,
            self.members_missing.len(),
            self.page_errors,
            self.canvases,
            self.canvases_blank.len(),
            self.unmeasured.len()
        )
    }

    /// The event the caller writes: `render_check{…}` with every count, or the loud
    /// `render_check_unavailable{reason}` — never a silent pass.
    pub(super) fn event(&self, round: usize) -> serde_json::Value {
        if !self.ran {
            return serde_json::json!({
                "event": "render_check_unavailable",
                "round": round,
                "reason": self.reason,
            });
        }
        serde_json::json!({
            "event": "render_check",
            "round": round,
            "elements": self.elements,
            "elements_missing": self.elements_missing,
            "members": self.members,
            "members_missing": self.members_missing,
            "page_errors": self.page_errors,
            "canvases": self.canvases,
            "canvases_blank": self.canvases_blank,
            "unmeasured": self.unmeasured,
            "verified": self.verified,
            "findings": self.findings.len(),
        })
    }
}

/// The probe prints exactly one JSON object; a diagnostic line before it must not hide it.
fn last_json_object(stdout: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .or_else(|| {
            stdout
                .lines()
                .rev()
                .find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        })
}

fn gate_command(replay: &RenderReplay<'_>) -> String {
    format!(
        "GATE COMMAND (boot exactly as the gate did — `{}` — then run it yourself; it prints \
         idPresent, memberType, pageErrors and canvases): `{}`.",
        replay.boot_line, replay.command
    )
}

/// The browser's JSON against the spec's identifiers → findings worded from the spec line that
/// advertised each fact (module doc). Pure, so r6h's tree pins it without a browser.
pub(super) fn render_check_verdict(
    surface: &RenderSurface,
    stdout: &str,
    replay: &RenderReplay<'_>,
) -> RenderCheck {
    let Some(v) = last_json_object(stdout) else {
        return RenderCheck::unavailable("the check printed no JSON object".to_string());
    };
    if v.get("ran").and_then(|x| x.as_bool()) != Some(true) {
        let reason = v
            .get("reason")
            .and_then(|x| x.as_str())
            .unwrap_or("the check reported ran:false without a reason");
        return RenderCheck::unavailable(reason.to_string());
    }
    let command = gate_command(replay);
    let mut out = RenderCheck {
        ran: true,
        elements: surface.elements.len(),
        members: surface.members.len(),
        ..Default::default()
    };
    for e in &surface.elements {
        match v
            .pointer(&format!("/idPresent/{}", e.id))
            .and_then(|x| x.as_bool())
        {
            Some(true) => out.verified += 1,
            Some(false) => {
                out.elements_missing.push(e.id.clone());
                out.findings.push((
                    FindingSource::RenderCheckElement,
                    format!(
                        "the served page has NO element with id `{}` while the spec advertises \
                         it as `{}` (request.md:{}) — the scripts that implement that section \
                         look it up when they load and find nothing. Put the element in the \
                         page's static markup, or create it before any script reads it. \
                         {command}",
                        e.id, e.written, e.line_no
                    ),
                ));
            }
            None => out.unmeasured.push(format!("#{} (not reported)", e.id)),
        }
    }
    for m in &surface.members {
        let path = format!("{}.{}", m.object, m.member);
        match v
            .pointer(&format!("/memberType/{path}"))
            .and_then(|x| x.as_str())
        {
            Some("function") => out.verified += 1,
            Some(other) => {
                out.members_missing.push(path.clone());
                out.findings.push((
                    FindingSource::RenderCheckApi,
                    format!(
                        "the spec advertises `window.{path}()` (request.md:{}) and in the served \
                         page `typeof window.{path}` is `{other}` — the debug surface the spec \
                         grades is incomplete. Define it as a synchronous function on \
                         `window.{}`. {command}",
                        m.line_no, m.object
                    ),
                ));
            }
            None => out.unmeasured.push(format!("window.{path} (not reported)")),
        }
    }
    match v.get("pageErrors").and_then(|x| x.as_array()) {
        Some(errors) => {
            out.page_errors = errors.len();
            match errors.first().and_then(|x| x.as_str()) {
                Some(first) => out.findings.push((
                    FindingSource::RenderCheckPageError,
                    format!(
                        "the served page throws an UNCAUGHT exception while loading ({} \
                         pageerror(s); first: {first}) — every statement after it never ran, so \
                         whatever that script was to build is missing. Fix the exception at its \
                         own source line. {command}",
                        errors.len()
                    ),
                )),
                None => out.verified += 1,
            }
        }
        None => out
            .unmeasured
            .push("pageerror count (not reported)".to_string()),
    }
    let webgl_available = v.get("webglAvailable").and_then(|x| x.as_bool());
    match v.get("canvases").and_then(|x| x.as_array()) {
        Some(canvases) => {
            out.canvases = canvases.len();
            for c in canvases {
                let id = c.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let index = c.get("index").and_then(|x| x.as_u64()).unwrap_or(0);
                let name = if id.is_empty() {
                    format!("canvas #{index}")
                } else {
                    format!("<canvas id=\"{id}\">")
                };
                let size = match (
                    c.get("width").and_then(|x| x.as_f64()),
                    c.get("height").and_then(|x| x.as_f64()),
                ) {
                    (Some(w), Some(h)) => format!("{}×{} px", w.round(), h.round()),
                    _ => "size unreported".to_string(),
                };
                match c.get("context").and_then(|x| x.as_str()) {
                    None if webgl_available == Some(false) => out.unmeasured.push(format!(
                        "{name}: the check's browser has no WebGL, its context is unmeasured"
                    )),
                    None => {
                        out.canvases_blank.push(name.clone());
                        out.findings.push((
                            FindingSource::RenderCheckCanvas,
                            format!(
                                "the page's {name} ({size}) never gets a rendering context in a \
                                 real browser (no getContext call succeeded on it) — the scene \
                                 the spec describes cannot draw. Create the context on this \
                                 canvas when the page loads and draw into it. {command}"
                            ),
                        ));
                    }
                    Some(kind) => match c.get("distinctColors").and_then(|x| x.as_u64()) {
                        Some(n) if n > 1 => out.verified += 1,
                        Some(_) => {
                            out.canvases_blank.push(name.clone());
                            out.findings.push((
                                FindingSource::RenderCheckCanvas,
                                format!(
                                    "the page's {name} holds a `{kind}` context but a screenshot \
                                     clipped to it ({size}) is ONE flat colour — nothing was drawn \
                                     to it in a real browser. Draw the scene the spec describes \
                                     and verify with a screenshot that shows more than one \
                                     colour. {command}"
                                ),
                            ));
                        }
                        None => out.unmeasured.push(format!(
                            "{name}: `{kind}` context, screenshot colours unmeasured"
                        )),
                    },
                }
            }
        }
        None => out.unmeasured.push("canvases (not reported)".to_string()),
    }
    out
}

/// Run the check against the app at `url` (module doc): the spec's identifiers → one headless
/// page → the verdict. `node` and `probe_path` are the render gate's own
/// (GOOSE_SWARM_RENDER_NODE / GOOSE_SWARM_RENDER_PROBE): the probe's directory is where the
/// scorer's playwright resolves. `budget_secs` is the gate's transport budget for the browser
/// process — the same number the gate hands its other probes, quoted as data. The script is
/// written beside the system temp dir under one fixed name so the GATE COMMAND replays. Called
/// from swarm.rs's render gate (`run_spec_contract`, after the tier probe's scenarios, VA-149).
pub(super) async fn render_check(
    spec: &str,
    url: &str,
    node: &str,
    probe_path: Option<&str>,
    budget_secs: u64,
    boot_line: &str,
) -> RenderCheck {
    let surface = spec_render_surface(spec);
    let script = std::env::temp_dir().join("goose-render-check.mjs");
    if let Err(e) = std::fs::write(&script, RENDER_CHECK_SCRIPT) {
        return RenderCheck::unavailable(format!(
            "could not write the check script to {}: {e}",
            script.display()
        ));
    }
    let probe_dir = probe_path
        .and_then(|p| std::path::Path::new(p).parent())
        .map(|d| d.display().to_string());
    let args = serde_json::json!({
        "url": url,
        "ids": surface.elements.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        "members": surface
            .members
            .iter()
            .map(|m| serde_json::json!({"object": m.object, "member": m.member}))
            .collect::<Vec<_>>(),
        "probeDir": probe_dir,
    })
    .to_string();
    let command = format!("{node} {} '{args}'", script.display());
    let mut cmd = tokio::process::Command::new(node);
    cmd.arg(&script).arg(&args);
    match super::smoke_output(cmd, budget_secs).await {
        Some(out) => render_check_verdict(
            &surface,
            &String::from_utf8_lossy(&out.stdout),
            &RenderReplay {
                boot_line,
                command: &command,
            },
        ),
        None => RenderCheck::unavailable(format!(
            "`{node}` did not complete within {budget_secs}s or could not be spawned"
        )),
    }
}

/// The browser side, one ESM file run by node with one JSON argument
/// (`{url, ids, members, probeDir}`); prints exactly ONE JSON object. Playwright resolves from
/// the render probe's directory first (the scorer's install), then the usual node roots — the
/// same ladder `product_probe_v3.mjs` climbs. Every failure is `{ran:false, reason}`.
const RENDER_CHECK_SCRIPT: &str = r##"// goose render check (VA-088): the spec's identifiers -> one headless Chromium page -> one JSON line.
import { createRequire } from 'node:module';
import { execSync } from 'node:child_process';
import { dirname, join } from 'node:path';

const args = JSON.parse(process.argv[2] || '{}');
const emit = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const text = (e) => String((e && e.message) || e);

function loadPlaywright(probeDir) {
  const attempts = [];
  const tryFrom = (label, anchor) => {
    try { return createRequire(anchor)('playwright'); } catch (e) { attempts.push(label + ': ' + text(e)); return null; }
  };
  let pw = probeDir ? tryFrom('probe dir ' + probeDir, join(probeDir, '__probe__.js')) : null;
  if (!pw) pw = tryFrom('local', import.meta.url);
  if (!pw) {
    try { pw = tryFrom('npm root -g', join(execSync('npm root -g', { encoding: 'utf8' }).trim(), '__probe__.js')); }
    catch (e) { attempts.push('npm root -g: ' + text(e)); }
  }
  if (!pw) pw = tryFrom('execPath', join(dirname(process.execPath), '..', 'lib', 'node_modules', '__probe__.js'));
  if (!pw) throw new Error('cannot resolve playwright: ' + attempts.join(' | '));
  return pw;
}

// Records which context kind each canvas actually obtained, without creating one ourselves.
const contextInstrument = () => {
  const orig = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, ...rest) {
    const ctx = orig.call(this, type, ...rest);
    if (ctx) this.__gooseContextKind = String(type);
    return ctx;
  };
};

let browser = null;
try {
  let pw;
  try { pw = loadPlaywright(args.probeDir); } catch (e) { emit({ ran: false, reason: text(e) }); process.exit(0); }
  try {
    browser = await pw.chromium.launch({ headless: true, args: ['--use-angle=gl'] });
  } catch (e) { emit({ ran: false, reason: 'chromium launch failed: ' + text(e) }); process.exit(0); }
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await context.addInitScript(contextInstrument);
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(text(e)));
  try { await page.goto(args.url, { waitUntil: 'load' }); }
  catch (e) { emit({ ran: false, reason: 'goto ' + args.url + ' failed: ' + text(e) }); process.exit(0); }
  // A polling app never idles; the two frames below still let its first draw land.
  try { await page.waitForLoadState('networkidle'); } catch (_) { /* measured on the frames below */ }
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))));
  const dom = await page.evaluate(({ ids, members }) => {
    const idPresent = {};
    for (const id of ids) idPresent[id] = !!document.getElementById(id);
    const memberType = {};
    for (const m of members) {
      let t = 'undefined';
      try { const o = window[m.object]; t = o == null ? 'undefined' : typeof o[m.member]; } catch (_) { t = 'undefined'; }
      memberType[m.object + '.' + m.member] = t;
    }
    const canvases = Array.from(document.querySelectorAll('canvas')).map((c, index) => {
      const r = c.getBoundingClientRect();
      return { id: c.id || '', index, context: c.__gooseContextKind || null,
               x: r.left + window.scrollX, y: r.top + window.scrollY, width: r.width, height: r.height };
    });
    let webglAvailable = false;
    try {
      const probe = document.createElement('canvas');
      webglAvailable = !!(probe.getContext('webgl2') || probe.getContext('webgl'));
    } catch (_) { webglAvailable = false; }
    return { idPresent, memberType, canvases, webglAvailable };
  }, { ids: args.ids || [], members: args.members || [] });
  for (const c of dom.canvases) {
    if (!(c.width >= 1 && c.height >= 1)) { c.distinctColors = 0; continue; }
    const clip = { x: Math.max(0, c.x), y: Math.max(0, c.y),
                   width: Math.max(1, Math.floor(c.width)), height: Math.max(1, Math.floor(c.height)) };
    try {
      const png = await page.screenshot({ clip, fullPage: true, type: 'png' });
      c.distinctColors = await page.evaluate(async (b64) => {
        const img = new Image();
        img.src = 'data:image/png;base64,' + b64;
        await img.decode();
        const cv = document.createElement('canvas');
        cv.width = img.width; cv.height = img.height;
        const ctx = cv.getContext('2d');
        ctx.drawImage(img, 0, 0);
        const d = ctx.getImageData(0, 0, cv.width, cv.height).data;
        const seen = new Set();
        for (let i = 0; i < d.length; i += 4) seen.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
        return seen.size;
      }, png.toString('base64'));
    } catch (e) { c.distinctColors = null; c.screenshotError = text(e); }
  }
  emit({ ran: true, pageErrors, ...dom });
} catch (e) {
  emit({ ran: false, reason: 'check crashed: ' + text(e) });
} finally {
  if (browser) await browser.close().catch(() => {});
}
"##;

#[cfg(test)]
mod tests {
    use super::super::findings::{check_key, FindingSeverity};
    use super::super::spec_post_endpoints;
    use super::*;

    /// r6h's request file verbatim (VA-109's fixture): the canvas at 547, the labels container
    /// at 663, console-page's own ids at 381–446, `window.vs7dbg = {` at 723–732.
    const R6H_REQUEST: &str = include_str!("testdata/va109/request.md");
    /// r6h's FINAL `web/index.html` (the shipped tree the round-1 gate measured, 135 lines).
    const R6H_FINAL_INDEX: &str = include_str!("testdata/va088/index.html");
    /// r6h's `web/index.html` before REPAIR (`.swarm/prefix-tree`, VA-114's fixture).
    const R6H_PREFIX_INDEX: &str = include_str!("testdata/va114/prefix-index.html");

    /// THE 44%-OF-REMAINING-LOSS CHECK, and every way it must refuse to fire.
    ///
    /// Measured across the four best 3-node cells on the current binary: `vendor_conditional` and
    /// `resync_conditional_ratio` together are 44% of ALL remaining weighted score loss, and both
    /// are the spec's own "a second sync must be cheap and must not duplicate rows". The engine
    /// never checked it because `run_spec_contract` issues only bare GETs.
    ///
    /// The FAIL-OPEN rows matter more than the positive one. This is the first WRITE the contract
    /// gate ever issues, and a false finding against a freshly built app is the most expensive
    /// mistake available here — so anything it cannot decide from the body must be Unreadable,
    /// never Duplicates.
    /// IT HAD NO `#[test]`, so it never ran — clippy reported it as a plain never-used function among
    /// eighty others and it was invisible. A test that does not run is not a test.
    #[test]
    fn a_repeated_post_is_only_a_finding_when_the_body_actually_proves_duplication() {
        let sync = r#"{"fetched":247,"inserted":247,"total":247}"#;
        assert_eq!(
            repeated_post_verdict(sync, r#"{"fetched":0,"inserted":0,"total":247}"#),
            RepeatedPost::Idempotent,
            "a CHEAP second sync re-fetches nothing, inserts nothing, and leaves the total alone"
        );
        // ⚠️ THE REGRESSION THAT MATTERS. The first version of this function returned Idempotent
        // here — on the exact signature of the defect it exists to catch. Rows are correct
        // (inserted 0, total flat) and the app still re-pulled all 247 pages.
        match repeated_post_verdict(sync, r#"{"fetched":247,"inserted":0,"total":247}"#) {
            // correct rows do NOT excuse re-downloading the collection — the spec asks for both
            RepeatedPost::NotCheap(f) => {
                assert!(f.starts_with("the second sync re-fetched 247 row(s) it already had"));
                // F797: the finding is repair-directed — the per-page ETag fix rides it.
                assert!(
                    f.contains("(path, offset, limit)"),
                    "the named fix rides the finding"
                );
            }
            other => panic!("expected NotCheap, got {other:?}"),
        }
        assert_eq!(
            repeated_post_verdict(sync, r#"{"fetched":247,"inserted":247,"total":494}"#),
            RepeatedPost::Duplicates("the second call inserted 247 more row(s)".into()),
            "THE DEFECT: re-syncing duplicates the collection"
        );
        assert_eq!(
            repeated_post_verdict(
                r#"{"fetched":247,"total":247}"#,
                r#"{"fetched":247,"total":248}"#
            ),
            RepeatedPost::Duplicates("total went 247 -> 248 on a repeat call".into()),
            "a growing total is duplication even with no `inserted` field"
        );
        // ⚠️ THE SECOND REGRESSION OF THE SAME SHAPE, MEASURED ON REAL CELLS. Three cells of build
        // 1786340680 scored `sync_completeness 0/247 payments after one sync` with `resync_idempotent
        // second sync inserted=0 total=0`. Before this arm every branch above read that as healthy
        // and the cheapness branch returned Idempotent — which increments `verified`, the counter
        // that exists so a consumer can tell a real pass from having checked nothing. An app that
        // syncs zero rows was being affirmatively verified as idempotent.
        let empty = r#"{"fetched":0,"inserted":0,"total":0}"#;
        assert!(
            matches!(
                repeated_post_verdict(empty, empty),
                RepeatedPost::Vacuous(_)
            ),
            "nothing happening twice is an empty app, not idempotency"
        );
        assert!(
            matches!(
                repeated_post_verdict(r#"{"inserted":0,"total":0}"#, r#"{"inserted":0,"total":0}"#),
                RepeatedPost::Vacuous(_)
            ),
            "no `fetched` field does not rescue it — an empty collection decides nothing"
        );
        // ...and the guard must not swallow a REAL pass: work on the first call still decides.
        assert_eq!(
            repeated_post_verdict(sync, r#"{"fetched":0,"inserted":0,"total":247}"#),
            RepeatedPost::Idempotent,
            "NEGATIVE CONTROL: a populated collection is still judged, not called vacuous"
        );
        // FAIL-OPEN — none of these may produce a finding.
        for (a, b, want, why) in [
            (
                "not json",
                "{}",
                RepeatedPost::NotJson,
                "a non-JSON body decides nothing",
            ),
            (
                r#"{"ok":true}"#,
                r#"{"ok":true}"#,
                RepeatedPost::NoIdempotencyField,
                "no idempotency-bearing field",
            ),
            (
                "[1,2]",
                "[1,2]",
                RepeatedPost::NotJson,
                "a JSON array is not an object",
            ),
            (
                r#"{"inserted":0}"#,
                "oops",
                RepeatedPost::NotJson,
                "one unreadable side is enough to abstain",
            ),
        ] {
            assert_eq!(repeated_post_verdict(a, b), want, "{why}");
        }
        // THE STATUS SPLIT, both directions. A body containing newlines must survive intact —
        // taking the first line instead of everything-before-the-last truncates pretty-printed JSON
        // into an Unreadable abstention, turning a decidable case into silence.
        assert_eq!(
            split_curl_status("{\"inserted\":0}\n200"),
            ("{\"inserted\":0}", 200)
        );
        assert_eq!(
            split_curl_status("{\n  \"error\": \"bad_cursor\"\n}\n500"),
            ("{\n  \"error\": \"bad_cursor\"\n}", 500),
            "a multi-line body must not be truncated by the status split"
        );
        assert_eq!(
            split_curl_status("no trailing status").1,
            0,
            "a missing status is 0, which is below every threshold and cannot manufacture a finding"
        );
        // And the endpoint extractor must find the POST the whole check depends on.
        let spec = "| Method | Path | Response |\n|---|---|---|\n\
                    | `GET` | `/api/health` | `{}` |\n| `POST` | `/api/sync` | `{\"inserted\": 0}` |\n";
        assert_eq!(spec_post_endpoints(spec), vec!["/api/sync".to_string()]);
        assert!(
            spec_post_endpoints("| `GET` | `/api/health` | `{}` |").is_empty(),
            "a GET-only spec advertises nothing to probe, so the gate stays silent"
        );
    }

    /// GAP 3 (r6c): the Unreadable arm split in two, and the evidence a finding carries. The
    /// five r6c "could not be read as JSON" / "does not carry the documented field(s)" findings
    /// were bare unauthenticated POSTs answered by JSON 401 envelopes — replayed against the
    /// archived tree: `POST /api/drafts` → 401 `{"error": {"code": "unauthorized", "message":
    /// "missing or unknown bearer token"}}`, `/api/webhooks/meridian` → 401 `bad_signature`,
    /// `/api/payments/<id>/note` → 400 `note is required`. None was non-JSON.
    #[test]
    fn probe_evidence_names_the_bare_request_each_status_and_a_body_head() {
        let env =
            r#"{"error": {"code": "unauthorized", "message": "missing or unknown bearer token"}}"#;
        let boot = "cd <tree> && PYTHONPATH=src python3 -m app --db-dir /tmp/x --ledger-port 8741";
        let ev = post_probe_evidence("/api/drafts", 20, &[(401, env), (401, env)], boot, 8741);
        assert!(
            ev.starts_with(
                "PROBE EVIDENCE — request as sent: `POST /api/drafts` with NO body and NO headers"
            ),
            "{ev}"
        );
        assert!(ev.contains("20s budget"), "{ev}");
        assert!(
            ev.contains("probe 1: HTTP 401, body «{\"error\": {\"code\": \"unauthorized\""),
            "{ev}"
        );
        assert!(ev.contains("probe 2: HTTP 401"), "{ev}");
        // S5d (iii): the gate's boot argv and a copy-paste replay ride every probe finding.
        assert!(ev.contains("REPLAY IT: boot exactly as the gate did — `cd <tree> && PYTHONPATH=src python3 -m app --db-dir /tmp/x --ledger-port 8741`"), "{ev}");
        assert!(
            ev.contains(
                "curl -s -w '\\n%{http_code}' -X POST -m 20 http://127.0.0.1:8741/api/drafts"
            ),
            "{ev}"
        );
        // A silent second probe is a DIFFERENT class (boot latency / hang), worded as such.
        let ev2 = post_probe_evidence(
            "/api/sync",
            20,
            &[(200, "{\"total\": 3}"), (0, "")],
            boot,
            8741,
        );
        assert!(
            ev2.contains("probe 2: no HTTP response within 20s"),
            "{ev2}"
        );
        // A long pretty-printed body reads on one line, capped, with its true length.
        let long = format!("{{\n  \"rows\": \"{}\"\n}}", "x".repeat(400));
        let ev3 = post_probe_evidence("/api/x", 20, &[(200, &long)], boot, 8741);
        assert!(!ev3.contains('\n'), "{ev3}");
        assert!(
            ev3.contains("…» (4"),
            "the snippet states the full length: {ev3}"
        );
        assert!(!ev3.contains("empty body"));
        assert!(post_probe_evidence("/api/x", 20, &[(204, "")], boot, 8741).contains("empty body"));
    }

    /// The documented keys are checked ANYWHERE in the body, because the spec's key regex
    /// flattens nested RESPONSE shapes: sb-7's `GET /notify/notifications` row documents
    /// `{"data": [{"id", "event_seq", "kind", "message", "at"}...], "total"}`, so `id`..`at` sit
    /// one level down inside `data`. The 401 envelope misses all seven; a correct page misses none.
    #[test]
    fn missing_documented_keys_reads_nested_shapes_honestly() {
        let documented: Vec<String> = ["data", "id", "event_seq", "kind", "message", "at", "total"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let page = r#"{"data": [{"id": "n_1", "event_seq": 7, "kind": "draft.created",
                      "message": "draft d_1 created", "at": "2026-09-01T00:00:00Z"}], "total": 1}"#;
        assert!(missing_documented_keys(page, &documented).is_empty());
        let env = r#"{"error": {"code": "unauthorized"}}"#;
        assert_eq!(missing_documented_keys(env, &documented), documented);
        assert_eq!(
            missing_documented_keys("not json", &documented),
            documented,
            "a non-object misses everything; the caller words that case as NotJson"
        );
        assert!(json_object("[1]").is_none());
        assert!(json_object(" {\"a\": 1} ").is_some());
    }

    /// The browser's JSON for a tree whose html defines `ids` statically: presence read from the
    /// markup by hand (`id="…"`), the eight members functions, no page error, one drawn canvas.
    fn probe_json(html: &str, surface: &RenderSurface, member_override: &[(&str, &str)]) -> String {
        let mut id_present = serde_json::Map::new();
        for e in &surface.elements {
            id_present.insert(
                e.id.clone(),
                serde_json::Value::Bool(html.contains(&format!("id=\"{}\"", e.id))),
            );
        }
        let mut member_type = serde_json::Map::new();
        for m in &surface.members {
            let key = format!("{}.{}", m.object, m.member);
            let ty = member_override
                .iter()
                .find(|(k, _)| *k == key)
                .map_or("function", |(_, t)| *t);
            member_type.insert(key, serde_json::Value::from(ty));
        }
        serde_json::json!({
            "ran": true,
            "idPresent": id_present,
            "memberType": member_type,
            "pageErrors": [],
            "canvases": [{"id": "viz3d", "index": 0, "context": "webgl2",
                          "x": 24, "y": 612, "width": 1232, "height": 540, "distinctColors": 4096}],
            "webglAvailable": true,
        })
        .to_string()
    }

    const REPLAY: RenderReplay<'static> = RenderReplay {
        boot_line: "cd <tree> && python3 -m app --ledger-port 8080",
        command: "node /tmp/goose-render-check.mjs '{\"url\":\"http://127.0.0.1:8080\"}'",
    };

    /// THE SURFACE IS THE SPEC'S: r6h's request names 21 element ids by element syntax (the
    /// canvas as `<canvas id="viz3d">` at 547, `#viz-labels` at 663, `#app-header` at 381, the
    /// brush counter at 677 — never the four hex colours of 416) and one advertised global,
    /// `window.vs7dbg`, whose eight members are the `m(` lines of the literal at 724–731.
    /// Nothing here knows sb-7; a spec without `window.X` advertises no member.
    #[test]
    fn the_render_surface_is_read_from_the_specs_own_identifiers() {
        let s = spec_render_surface(R6H_REQUEST);
        let ids: Vec<&str> = s.elements.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), 21, "{ids:?}");
        for (id, line_no, written) in [
            ("app-header", 381, "#app-header"),
            ("viz3d", 547, "<canvas id=\"viz3d\">"),
            ("viz-labels", 663, "#viz-labels"),
            ("brush-count", 677, "#brush-count"),
        ] {
            let e = s.elements.iter().find(|e| e.id == id).expect(id);
            assert_eq!((e.line_no, e.written.as_str()), (line_no, written), "{id}");
        }
        assert!(
            !ids.iter().any(|i| i.chars().all(|c| c.is_ascii_hexdigit())),
            "{ids:?}"
        );
        assert_eq!(
            s.members
                .iter()
                .map(|m| (m.object.as_str(), m.member.as_str(), m.line_no))
                .collect::<Vec<_>>(),
            vec![
                ("vs7dbg", "layout", 724),
                ("vs7dbg", "sceneDigest", 725),
                ("vs7dbg", "camera", 726),
                ("vs7dbg", "setCamera", 727),
                ("vs7dbg", "pick", 728),
                ("vs7dbg", "pickPixel", 729),
                ("vs7dbg", "brush", 730),
                ("vs7dbg", "frames", 731),
            ]
        );
        let plain = spec_render_surface(
            "## UI\n\nA table in `#rows` and a button `#sync`; window.location.reload() after.\n",
        );
        assert_eq!(
            plain
                .elements
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["rows", "sync"]
        );
        assert_eq!(
            plain
                .members
                .iter()
                .map(|m| (m.object.as_str(), m.member.as_str()))
                .collect::<Vec<_>>(),
            vec![("location", "reload")],
            "a cited `X.m(` is a member; a global with no member advertises nothing"
        );
    }

    /// LEAST IMPACT (VA-088): on r6h's FINAL tree the derived check yields the SAME render-class
    /// verdict the shipped probe did — `complete_result.render_class_known_bugs = 0`, round 1's
    /// `render_gate: ran (rows=50, console_errors=0)`. Every one of the 21 advertised ids is in
    /// the final `web/index.html` (read by hand from the archive: `id="…"` for each), the eight
    /// members are functions (`_vs7Facade`, viz.js:43), the score's `webgl_on_viz3d = True` and
    /// `draw_calls = True` are the canvas facts — so zero findings and 31 verified. On the
    /// PRE-repair tree the same check names the three ids the page lacked (`viz-empty` 445,
    /// `viz-error` 446, `viz-labels` 663), each worded from its spec line — the round-0 gate saw
    /// only the one a script looked up literally.
    #[test]
    fn r6h_final_tree_yields_no_render_finding_and_the_prefix_tree_names_its_missing_ids() {
        let surface = spec_render_surface(R6H_REQUEST);
        let final_tree = render_check_verdict(
            &surface,
            &probe_json(R6H_FINAL_INDEX, &surface, &[]),
            &REPLAY,
        );
        assert!(final_tree.ran);
        assert!(final_tree.findings.is_empty(), "{:?}", final_tree.findings);
        assert_eq!(final_tree.verified, 21 + 8 + 1 + 1);
        assert!(
            final_tree.unmeasured.is_empty(),
            "{:?}",
            final_tree.unmeasured
        );
        assert_eq!(
            final_tree.event(1),
            serde_json::json!({"event": "render_check", "round": 1, "elements": 21,
                "elements_missing": [], "members": 8, "members_missing": [], "page_errors": 0,
                "canvases": 1, "canvases_blank": [], "unmeasured": [], "verified": 31, "findings": 0})
        );
        assert_eq!(
            final_tree.status(),
            "ran (elements 21 missing 0, members 8 missing 0, page_errors 0, canvases 1 blank 0, unmeasured 0)"
        );
        let prefix = render_check_verdict(
            &surface,
            &probe_json(R6H_PREFIX_INDEX, &surface, &[]),
            &REPLAY,
        );
        assert_eq!(
            prefix.elements_missing,
            vec!["viz-empty", "viz-error", "viz-labels"]
        );
        assert_eq!(prefix.findings.len(), 3);
        assert!(prefix
            .findings
            .iter()
            .all(|(s, _)| *s == FindingSource::RenderCheckElement));
        let (_, labels) = &prefix.findings[2];
        assert!(
            labels.starts_with(
                "the served page has NO element with id `viz-labels` while the spec advertises it \
                 as `#viz-labels` (request.md:663) — the scripts that implement that section look \
                 it up when they load and find nothing."
            ),
            "{labels}"
        );
        assert!(
            labels.ends_with(
                "GATE COMMAND (boot exactly as the gate did — `cd <tree> && python3 -m app \
                 --ledger-port 8080` — then run it yourself; it prints idPresent, memberType, \
                 pageErrors and canvases): `node /tmp/goose-render-check.mjs \
                 '{\"url\":\"http://127.0.0.1:8080\"}'`."
            ),
            "{labels}"
        );
        assert_eq!(
            check_key(FindingSource::RenderCheckElement, labels),
            "render check | the served page has no element with id `viz-labels` while the spec \
             advertises it as `#viz-labels`"
        );
        assert_eq!(prefix.verified, 18 + 8 + 1 + 1);
    }

    /// The other three classes, each worded from the spec and classed by provenance: a member
    /// that is not a function (HIGH, `window.vs7dbg.frames` → `undefined`), an uncaught page
    /// error (CRITICAL, render-class — r5's ReferenceError read from the browser's own event),
    /// a canvas with a context and ONE colour (CRITICAL). A canvas with no context in a browser
    /// that has no WebGL is UNMEASURED, never a finding; a fact the JSON omits is unmeasured
    /// too. No JSON, or `ran:false`, is the loud `render_check_unavailable{reason}`.
    #[test]
    fn a_missing_member_a_page_error_and_a_blank_canvas_are_named_and_classed() {
        let surface = spec_render_surface(R6H_REQUEST);
        let mut v: serde_json::Value = serde_json::from_str(&probe_json(
            R6H_FINAL_INDEX,
            &surface,
            &[("vs7dbg.frames", "undefined")],
        ))
        .unwrap();
        v["pageErrors"] =
            serde_json::json!(["ReferenceError: onBrushChangeTracked is not defined"]);
        v["canvases"][0]["distinctColors"] = serde_json::json!(1);
        let check = render_check_verdict(&surface, &v.to_string(), &REPLAY);
        let sources: Vec<FindingSource> = check.findings.iter().map(|(s, _)| *s).collect();
        assert_eq!(
            sources,
            vec![
                FindingSource::RenderCheckApi,
                FindingSource::RenderCheckPageError,
                FindingSource::RenderCheckCanvas
            ]
        );
        assert!(check.findings[0].1.starts_with(
            "the spec advertises `window.vs7dbg.frames()` (request.md:731) and in the served page \
             `typeof window.vs7dbg.frames` is `undefined`"
        ), "{}", check.findings[0].1);
        assert!(check.findings[1].1.starts_with(
            "the served page throws an UNCAUGHT exception while loading (1 pageerror(s); first: \
             ReferenceError: onBrushChangeTracked is not defined)"
        ), "{}", check.findings[1].1);
        assert!(check.findings[2].1.starts_with(
            "the page's <canvas id=\"viz3d\"> holds a `webgl2` context but a screenshot clipped to \
             it (1232×540 px) is ONE flat colour"
        ), "{}", check.findings[2].1);
        assert_eq!(check.members_missing, vec!["vs7dbg.frames"]);
        assert_eq!(check.page_errors, 1);
        assert_eq!(check.canvases_blank, vec!["<canvas id=\"viz3d\">"]);
        assert_eq!(check.verified, 21 + 7);
        for (s, want) in [
            (FindingSource::RenderCheckApi, FindingSeverity::High),
            (
                FindingSource::RenderCheckPageError,
                FindingSeverity::Critical,
            ),
            (FindingSource::RenderCheckCanvas, FindingSeverity::Critical),
            (FindingSource::RenderCheckElement, FindingSeverity::High),
        ] {
            assert_eq!(s.severity(), want, "{s:?}");
            assert!(
                s.is_render_probe(),
                "{s:?} blocks `passed` like the render gate's classes"
            );
            assert_eq!(s.probe(), "render check");
        }
        assert_eq!(
            check_key(FindingSource::RenderCheckPageError, &check.findings[1].1),
            "render check | the served page throws an uncaught exception while loading"
        );
        // No context, no WebGL in the checking browser: said, not blamed.
        v["pageErrors"] = serde_json::json!([]);
        v["webglAvailable"] = serde_json::json!(false);
        v["canvases"][0]["context"] = serde_json::Value::Null;
        let check = render_check_verdict(&surface, &v.to_string(), &REPLAY);
        assert!(check
            .findings
            .iter()
            .all(|(s, _)| *s == FindingSource::RenderCheckApi));
        assert_eq!(
            check.unmeasured,
            vec!["<canvas id=\"viz3d\">: the check's browser has no WebGL, its context is unmeasured"]
        );
        // The same canvas with WebGL available and no context IS the finding.
        v["webglAvailable"] = serde_json::json!(true);
        let check = render_check_verdict(&surface, &v.to_string(), &REPLAY);
        assert!(check
            .findings
            .iter()
            .any(|(s, t)| *s == FindingSource::RenderCheckCanvas
                && t.starts_with(
                    "the page's <canvas id=\"viz3d\"> (1232×540 px) never gets a rendering context"
                )));
        // Omitted facts are unmeasured, never verified and never findings.
        let sparse = render_check_verdict(&surface, r#"{"ran": true}"#, &REPLAY);
        assert!(sparse.findings.is_empty() && sparse.verified == 0);
        assert_eq!(sparse.unmeasured.len(), 21 + 8 + 2);
        // No browser: loud, with the reason the script gave.
        let off = render_check_verdict(
            &surface,
            "diagnostic on stderr-ish line\n{\"ran\": false, \"reason\": \"cannot resolve playwright: local: x | npm-root-g: y\"}\n",
            &REPLAY,
        );
        assert!(!off.ran);
        assert_eq!(
            off.event(0),
            serde_json::json!({"event": "render_check_unavailable", "round": 0,
                "reason": "cannot resolve playwright: local: x | npm-root-g: y"})
        );
        assert_eq!(
            off.status(),
            "unavailable (cannot resolve playwright: local: x | npm-root-g: y)"
        );
        let none = render_check_verdict(&surface, "not json at all", &REPLAY);
        assert_eq!(none.reason, "the check printed no JSON object");
        let bare = render_check_verdict(&surface, r#"{"ran": false}"#, &REPLAY);
        assert_eq!(bare.reason, "the check reported ran:false without a reason");
        // The script the runner writes names the sole GL flag and nothing of any benchmark.
        assert!(RENDER_CHECK_SCRIPT.contains("args: ['--use-angle=gl']"));
        for literal in ["viz3d", "vs7dbg", "12288", "12,288"] {
            assert!(
                !RENDER_CHECK_SCRIPT.contains(literal),
                "{literal} is a benchmark's name"
            );
        }
    }
}
