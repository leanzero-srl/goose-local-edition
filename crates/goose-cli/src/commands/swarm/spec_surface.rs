//! THE SPEC'S ADVERTISED SURFACE: the endpoint-table rows a request documents, each tagged with
//! the service whose section it sits in. ONE parser feeds every consumer — the GET and POST
//! probers, the unprobed disclosure, the plan repair's unassigned-endpoint rule, the smoke-fix
//! brief and the sink's angle — so a second table reader cannot drift from the first the way the
//! inline decomposition counters once did. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases); moved verbatim from swarm.rs with
//! its tests, then amended for r6c's measured defect (the expected-shape column was a fixed
//! index, and §5's fourth column made that the ROLE). The WHY of every rule stays in its own
//! comment.

use std::collections::BTreeSet;

/// Pure, and deliberately narrow: it returns METHOD PATH -> EXPECTED triples only, never spec prose,
/// so a build order cannot survive extraction into a read-only shard's prompt. An empty result means
/// the caller emits today's string byte-for-byte.
pub(super) fn spec_advertised_surface(spec: &str) -> Vec<String> {
    let SpecSurface { primary, rows } = spec_surface_rows(spec);
    rows.into_iter()
        .filter(|(service, _)| primary.is_none() || *service == primary)
        .map(|(_, row)| row)
        .collect()
}

/// Every endpoint-table row in the spec, each tagged with the service whose section it sits in, plus
/// the primary service's name. `spec_advertised_surface` is the primary's rows and nothing else; the
/// plan repair's rule (d) needs the OTHER services' rows too, addressed to their own entry owners, and a
/// second table parser would drift from this one the way the inline decomposition counters did.
pub(super) struct SpecSurface {
    pub(super) primary: Option<String>,
    pub(super) rows: Vec<(Option<String>, String)>,
}

pub(super) fn spec_surface_rows(spec: &str) -> SpecSurface {
    // A cell is unwrapped ONLY when it is one whole code span (`GET`, `/api/health`,
    // `{"status": "ok"}`): it starts and ends with a backtick and holds no other. A MIXED cell —
    // prose around one or more spans — is kept verbatim: `trim_matches` used to strip its outer
    // backticks and leave the inner ones dangling, so sb-7's §5 row rendered as
    // `EXPECT create from `{...}` → `draft.created` (one backtick short) and the §6 row
    // `` `{"events": [...]}` → `{"accepted": ...}` `` lost both ends — an unbalanced shape handed
    // to the builder as the response contract (r6e refuter E12).
    let unwrap_cell = |c: &str| {
        let c = c.trim();
        match c.strip_prefix('`').and_then(|r| r.strip_suffix('`')) {
            Some(inner) if !inner.contains('`') => inner.trim().to_string(),
            _ => c.to_string(),
        }
    };
    // THE PATH IS THE FIRST BACKTICKED TOKEN when the cell opens with a backtick, else the first
    // whitespace token. sb-7's row `| `GET` | `/` + `web/*` | …` stripped by `unwrap_cell` to
    // "/` + `web/*"; every consumer then took its first whitespace token, "/`", and the gate curled
    // an endpoint that exists nowhere — r0's "GET /` returned 404". The REAL `/` was never emitted
    // at all, so the frontend row went unprobed while its phantom twin blocked green.
    // A TRAILING ELLIPSIS IS THE WRITER'S "AND BENEATH", NOT PART OF THE PATH (VA-044 F1): sb-7's
    // Endpoints table points at §5 with `| `POST/GET` | `/api/drafts...` | section 5 |`; the row
    // advertises `/api/drafts`, and `/api/drafts...` is a route nobody serves — the same phantom
    // class as "/`". Stripped HERE, once: `spec_get_endpoints` (swarm.rs) trims prose punctuation
    // on its own and would have hidden it from the GET prober, while `spec_post_endpoints`,
    // `research::advertised_paths` (rules a and d) and the plan repair read the path as emitted.
    let path_cell = |c: &str| -> String {
        let c = c.trim();
        let path = match c.strip_prefix('`') {
            Some(rest) => rest.split('`').next().unwrap_or("").trim(),
            None => c.split_whitespace().next().unwrap_or(""),
        };
        path.trim_end_matches(['.', '…']).to_string()
    };
    // ONE SERVICE'S SURFACE, NOT THE WHOLE DOCUMENT'S. sb-7 documents two services: §3 `ledgerd`,
    // whose table the gate boots and probes, and §6 `notifierd`, whose /notify/* and /health rows
    // are served on notifierd's OWN port. Every consumer of `spec_advertised_surface` — the GET
    // prober, the POST prober, the unprobed disclosure, the tester angle and the fix worker —
    // addresses ONE port, so notifierd's rows probed on ledgerd's port were three of r0's phantom
    // 404s. The nearest heading's backticked name is a row's service; the FIRST endpoint table's
    // service is the primary; a row named for another service is tagged with that name and the
    // primary-only consumer drops it. Only headings AT THAT LEVEL OR DEEPER denote services: sb-7's
    // title is "# Build `app`", and letting an unnamed section inherit from it made §5's drafts rows
    // (`### 5. The approval workflow`, no name) belong to `app` rather than to ledgerd and dropped
    // them. Ancestors above the first table's named heading are the document, not a service; an
    // unnamed sibling section is the primary's.
    // The notifier port IS known to the gate (`spec_run_argv_v2` fills it second), so a future
    // prober can address those rows there; today nothing does, and probing them there is a lie.
    // THE EXPECTED-SHAPE COLUMN IS NAMED BY THE HEADER, NEVER FIXED BY INDEX. sb-7's §3 and §6
    // tables are `Method | Path | Response`, where cells[2] IS the response; §5 is
    // `Method | Path | Role | Effect + ledger event`, where cells[2] is the ROLE. r6c's skeleton
    // brief (plan_loaded tasks[7]) read "POST /api/drafts -> EXPECT maker or checker" and
    // "GET /api/drafts?state= -> EXPECT any role" — a permission handed to the worker as the
    // response shape — and `smoke_fix_description`'s WHAT THIS APP ADVERTISES said the same. The
    // header is the row before a separator; its cell named response/returns/shape/effect picks the
    // column, and a table whose header names none reads the last cell.
    let is_separator = |cells: &[&str]| {
        cells.iter().all(|c| {
            let t = c.trim();
            !t.is_empty() && t.chars().all(|ch| ch == '-' || ch == ':')
        })
    };
    let shape_column = |header: &[&str]| -> Option<usize> {
        ["response", "returns", "shape", "effect"]
            .iter()
            .find_map(|name| {
                header
                    .iter()
                    .position(|c| c.to_ascii_lowercase().contains(name))
            })
    };
    let mut header_cells: Vec<&str> = Vec::new();
    let mut expected_col: Option<usize> = None;
    let mut service_by_level: [Option<String>; 7] = Default::default();
    let mut first_row_seen = false;
    let mut primary: Option<(usize, String)> = None;
    let mut in_fence = false;
    let mut rows = Vec::new();
    for line in spec.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence && line.starts_with('#') {
            let level = line.chars().take_while(|c| *c == '#').count();
            if level <= 6 && line.get(level..).is_some_and(|rest| rest.starts_with(' ')) {
                for slot in service_by_level.iter_mut().skip(level) {
                    *slot = None;
                }
                service_by_level[level] = heading_service_name(line);
            }
            continue;
        }
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').collect();
        if cells.len() < 3 {
            continue;
        }
        if is_separator(&cells) {
            expected_col = shape_column(&header_cells);
            continue;
        }
        // ONE ROW PER METHOD when the cell names several (VA-044 F1): sb-7's Endpoints table
        // writes its drafts pointer as `| `POST/GET` | `/api/drafts...` | section 5 |`, and an
        // exact-method test classed that row as a HEADER — so the Endpoints section advertised
        // no `/api/drafts` at all: the consumer routing's rule (a) (`research::advertised_paths`
        // reads each SECTION's own rows) could not carry Endpoints to a slice that names the
        // drafts path, and the GET prober never saw the bare `GET /api/drafts`. The cell is a
        // method cell only when EVERY `/`-separated alternative is a method (the same split
        // `spec_prose_documented_keys` applies); a mixed cell is still a header.
        let method_cell = unwrap_cell(cells[0]).to_uppercase();
        let methods: Vec<&str> = method_cell
            .split('/')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .collect();
        if methods.is_empty()
            || !methods
                .iter()
                .all(|m| matches!(*m, "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"))
        {
            header_cells = cells; // a header row, or a table that is not an endpoint table
            continue;
        }
        let path = path_cell(cells[1]);
        if !path.starts_with('/') {
            continue;
        }
        if !first_row_seen {
            first_row_seen = true;
            primary = service_by_level
                .iter()
                .enumerate()
                .rev()
                .find_map(|(level, s)| s.as_ref().map(|s| (level, s.clone())));
        }
        let service = primary.as_ref().map(|(level, name)| {
            service_by_level[*level..]
                .iter()
                .rev()
                .find_map(|s| s.clone())
                .unwrap_or_else(|| name.clone())
        });
        let expected_cell = expected_col
            .filter(|i| *i < cells.len())
            .unwrap_or(cells.len() - 1);
        let expected = unwrap_cell(cells[expected_cell]);
        for method in methods {
            rows.push((
                service.clone(),
                if expected.is_empty() {
                    format!("{method} {path}")
                } else {
                    format!("{method} {path} -> EXPECT {expected}")
                },
            ));
        }
    }
    SpecSurface {
        primary: primary.map(|(_, name)| name),
        rows,
    }
}

/// The backticked service a section heading names — `### 3. `ledgerd` — …` is `ledgerd`,
/// `### 3. `vendorsync/api.py` — the HTTP backend` is `vendorsync/api.py`. None for an unnamed
/// heading (`#### Endpoints`) and for a backticked PATH (`### GET `/api/health``), which names an
/// endpoint rather than the thing serving it.
fn heading_service_name(heading: &str) -> Option<String> {
    let name = heading.split('`').nth(1)?.trim();
    (!name.is_empty() && !name.starts_with('/') && !name.contains(char::is_whitespace))
        .then(|| name.to_string())
}

/// Advertised endpoints that MUTATE, as bare paths. `spec_advertised_surface` returns display
/// strings ("POST /api/sync -> EXPECT {...}"); this returns the paths a prober can actually call.
pub(super) fn spec_post_endpoints(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    for adv in spec_advertised_surface(spec) {
        let mut it = adv.split_whitespace();
        let (Some(method), Some(path)) = (it.next(), it.next()) else {
            continue;
        };
        if method == "POST" && path.starts_with('/') && !out.contains(&path.to_string()) {
            out.push(path.to_string());
        }
    }
    out
}

/// THE ONE boundary scanner behind `path_token_named` and `resource_word_named`: `needle`
/// occurs in `text` with no `is_token_byte` byte on either side. Needles are ASCII (paths and
/// path segments), so byte offsets around them are char offsets. An empty needle names nothing.
fn token_named(needle: &str, text: &str, is_token_byte: impl Fn(u8) -> bool) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    text.match_indices(needle).any(|(at, _)| {
        let before_ok = at == 0 || !is_token_byte(bytes[at - 1]);
        let end = at + needle.len();
        let after_ok = end >= bytes.len() || !is_token_byte(bytes[end]);
        before_ok && after_ok
    })
}

/// Does `text` name `path` as a whole path token? Bounded on both sides so notifierd's
/// `/health` is not found inside ledgerd's `/api/health`, and `/api/drafts` is not found inside
/// `/api/drafts/<id>/submit` — that longer row has its own path. A byte before or after that is
/// not part of a path token (alphanumeric, `_`, `/`, `.`, `-`) is a boundary. Shared by the
/// consumer routing (`research::consumed_spec_sections`, rule a), the plan repair's cross-owner
/// mention rule and the prose-shape label match below.
pub(super) fn path_token_named(path: &str, text: &str) -> bool {
    token_named(path, text, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'/' | b'.' | b'-')
    })
}

/// Does `text` use `word` as a WORD — case-folded, bounded by anything that is not a word byte
/// (alphanumeric or `_`)? The consumer routing's rule (d) predicate (VA-032): sb-7's §7 writes
/// the drafts panel as `#draft-form`, `#draft-list`, "the drafts call", "the draft's state" —
/// never the path `/api/drafts` — so `path_token_named` (rule a) finds nothing there while the
/// prose names the resource in every sentence. `-`, `.` and `/` are boundaries here where they
/// are path bytes above: `draft` is a word in `#draft-form` and in `draft.created`, and is not
/// a word in `drafted`. `word` is a path segment (ASCII); a non-ASCII text folds only its ASCII
/// letters, which is all a segment can match.
pub(super) fn resource_word_named(word: &str, text: &str) -> bool {
    token_named(
        &word.to_ascii_lowercase(),
        &text.to_ascii_lowercase(),
        |b| b.is_ascii_alphanumeric() || b == b'_',
    )
}

/// The MOUNT PREFIXES of a request's advertised routes — derived from the routes themselves,
/// never a list: a leading segment that leads more than one distinct advertised base path is
/// where routes are mounted, not what any of them is about (sb-7: `api` leads twelve, `notify`
/// three; `/health` leads only itself and is its own resource). A resource word is read AFTER
/// these (`resource_words`).
pub(super) fn mount_prefixes(bases: &[String]) -> BTreeSet<String> {
    let distinct: BTreeSet<&str> = bases.iter().map(String::as_str).collect();
    let mut leads: std::collections::BTreeMap<&str, usize> = Default::default();
    for base in distinct {
        if let Some(first) = base.split('/').find(|s| !s.is_empty()) {
            *leads.entry(first).or_insert(0) += 1;
        }
    }
    leads
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(seg, _)| seg.to_string())
        .collect()
}

/// The WORDS a request's prose uses for the resource a route names — the first path segment
/// after the mount prefixes (`/api/drafts` → `drafts`, `/api/viz/records` → `viz`,
/// `/api/outbox/status` → `outbox`, `/notify/events` → `events`, `/health` → `health`; a path
/// made only of mount prefixes keeps its last segment), lowercased, with its English
/// singular/plural sibling (`drafts` ⇄ `draft`): the prose that describes the drafts panel says
/// both, and a route table never says either — it says the path. Template segments never
/// appear here because `research::advertised_paths` cuts a base path at its first `<`/`{`.
/// Derived per request from its own advertised routes (`mount_prefixes`); nothing is listed.
pub(super) fn resource_words(base: &str, mount: &BTreeSet<String>) -> Vec<String> {
    let segments: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    let Some(resource) = segments
        .iter()
        .find(|s| !mount.contains(**s))
        .or(segments.last())
    else {
        return Vec::new();
    };
    let word = resource.to_ascii_lowercase();
    let mut out = vec![word.clone()];
    match word.strip_suffix('s') {
        Some(singular) if singular.len() >= 3 && !singular.ends_with('s') => {
            out.push(singular.to_string());
        }
        Some(_) => {}
        None => out.push(format!("{word}s")),
    }
    out
}

/// VA-005: the response keys a spec documents for an endpoint OUTSIDE its table row — the
/// PROSE SHAPE. sb-7 writes three rows as `| GET | /api/health | shape below |` and puts the
/// shape after the table, under a bold label, in a fenced block:
///
/// ````text
/// **Health.**
///
/// ```json
/// {"status": "ok", "payments": <int>, "last_sync": <str or null>,
///  "webhook": {"registered": <bool>, ...}}
/// ```
/// ````
///
/// `spec_documented_keys` (swarm.rs) reads the row's own cell and found nothing for /api/health,
/// /api/summary and /api/buckets on r6c/r6d — three of ledgerd's four JSON reads went unchecked.
/// The rule, derived from the document: the section that advertises the row; inside it, a LABEL
/// line — one that names the path as a token, or whose decoration-stripped text IS the path's
/// last segment (`**Health.**` → `health`) — whose next non-empty line opens a fence; the keys
/// are the TOP-LEVEL keys of that fenced shape (depth 1 of its outermost object), because the
/// GET prober asserts documented keys at the top level (`v.get(k)`) and a flattened `registered`
/// or `currency` would be filed as missing against a correct response. Empty when the section
/// has no such label+fence — the caller then asserts nothing, exactly as for a prose cell. MILD.
/// Consumed by `spec_documented_keys` (swarm.rs) when the row's own cell documents no shape.
pub(super) fn spec_prose_documented_keys(spec: &str, method: &str, path: &str) -> Vec<String> {
    let base = path.split('?').next().unwrap_or(path);
    if !base.starts_with('/') {
        return Vec::new();
    }
    let segment = base
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_matches(|c| c == '<' || c == '>')
        .to_lowercase();
    let is_http_method = |m: &str| {
        matches!(
            m.trim().to_uppercase().as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        )
    };
    let lines: Vec<&str> = spec.lines().collect();
    let is_heading = |l: &str| {
        let t = l.trim_start();
        let n = t.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&n) && t.get(n..).is_some_and(|r| r.starts_with(' '))
    };
    // The section (heading to heading, fences respected) whose table advertises the row.
    let mut in_fence = false;
    let mut section_start = 0usize;
    let mut advertising_section: Option<(usize, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if is_heading(t) {
            if let Some((start, _)) = advertising_section {
                advertising_section = Some((start, i));
                break;
            }
            section_start = i;
            continue;
        }
        if advertising_section.is_some() || !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.split('|').map(str::trim).collect();
        let method_cell = cells
            .iter()
            .find(|c| !c.is_empty())
            .map(|c| c.trim_matches('`'));
        let names_a_method = method_cell.is_some_and(|c| c.split('/').any(is_http_method));
        let names_this_method = method_cell
            .is_some_and(|c| c.split('/').any(|m| m.trim().eq_ignore_ascii_case(method)));
        if names_a_method && !names_this_method {
            continue;
        }
        let advertises = cells.iter().any(|c| {
            let c = c.trim_matches('`');
            c.split(['?', '`', ' ']).next().unwrap_or("") == base
        });
        if advertises {
            advertising_section = Some((section_start, lines.len()));
        }
    }
    let Some((start, end)) = advertising_section else {
        return Vec::new();
    };
    let label_names_path = |t: &str| -> bool {
        if path_token_named(base, t) {
            return true;
        }
        let stripped: String = t
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`'))
            .collect::<String>()
            .trim()
            .trim_end_matches(['.', ':'])
            .to_lowercase();
        !segment.is_empty() && stripped == segment
    };
    let mut in_fence = false;
    let mut i = start;
    while i < end {
        let t = lines[i].trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            i += 1;
            continue;
        }
        if in_fence || t.is_empty() || t.starts_with('|') || is_heading(t) || !label_names_path(t) {
            i += 1;
            continue;
        }
        let Some(fence_at) = (i + 1..end).find(|j| !lines[*j].trim().is_empty()) else {
            break;
        };
        if !lines[fence_at].trim().starts_with("```") {
            i += 1;
            continue;
        }
        let body: String = lines[fence_at + 1..end]
            .iter()
            .take_while(|l| !l.trim().starts_with("```"))
            .map(|l| format!("{l}\n"))
            .collect();
        return top_level_keys(&body);
    }
    Vec::new()
}

/// The keys at depth 1 of a JSON-LIKE shape — the spec's notation (`<int>`, `<str or null>`,
/// `[...]`) is not JSON a parser accepts, so this walks braces and brackets outside string
/// literals and takes a string followed by `:` only inside the outermost object. Deduped, in
/// document order.
fn top_level_keys(shape: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    let chars: Vec<char> = shape.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
                if depth == 1 && next == Some(&':') && !keys.contains(&current) {
                    keys.push(current.clone());
                }
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' => {
                    in_string = true;
                    current.clear();
                }
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_advertised_surface_enumerates_the_real_endpoint_table() {
        // The real spec's shape, verbatim from evals/swarm-bench/spec-build.md.
        let spec = "\
## 3. `vendorsync/api.py`\n\n\
| Method | Path | Response |\n\
|---|---|---|\n\
| `GET` | `/api/health` | `{\"status\": \"ok\", \"payments\": <int>}` |\n\
| `GET` | `/api/summary` | `{\"count\": <int>, \"currency\": \"EUR\"}` |\n\
| `POST` | `/api/sync` | `{\"fetched\": <int>, \"inserted\": <int>}` |\n\n\
`limit` defaults to 25 and is capped at 100.\n";
        let items = spec_advertised_surface(spec);
        assert_eq!(
            items.len(),
            3,
            "three endpoint rows, header and separator skipped: {items:?}"
        );
        assert!(
            items[0].starts_with("GET /api/health -> EXPECT"),
            "{:?}",
            items[0]
        );
        assert!(
            items[2].starts_with("POST /api/sync -> EXPECT"),
            "{:?}",
            items[2]
        );
        // DETERMINISM is the whole point — the same spec must yield the same list, so three shards
        // partition one list instead of three they each invented.
        assert_eq!(items, spec_advertised_surface(spec));
        // PROSE MUST NOT SURVIVE. A read-only shard handed a build order is the failure this is
        // deliberately narrow to avoid.
        assert!(
            !items.iter().any(|i| i.contains("defaults to 25")),
            "{items:?}"
        );
        assert!(
            !items.iter().any(|i| i.contains("vendorsync/api.py")),
            "{items:?}"
        );

        // A spec with no endpoint table yields NOTHING, so the caller emits today's string
        // byte-for-byte and the change is inert rather than half-applied.
        assert!(spec_advertised_surface("# Build a CLI\n\nIt should be fast.\n").is_empty());
        assert!(spec_advertised_surface("").is_empty());
        // A markdown table that is not an endpoint table must not be mistaken for one.
        assert!(
            spec_advertised_surface("| Name | Type |\n|---|---|\n| `id` | `str` |\n").is_empty()
        );
    }

    /// `spec_surface_rows` tags every row with its service and `spec_advertised_surface` is its
    /// primary-only view — the notifierd rows the gate must not probe on ledgerd's port are still
    /// enumerated, under their own name, for the repair to address to notifierd's entry owner.
    #[test]
    fn spec_surface_rows_tags_every_service_and_the_primary_view_is_unchanged() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let SpecSurface { primary, rows } = spec_surface_rows(spec);
        assert_eq!(primary.as_deref(), Some("ledgerd"));
        let notifierd: Vec<&str> = rows
            .iter()
            .filter(|(s, _)| s.as_deref() == Some("notifierd"))
            .map(|(_, r)| r.as_str())
            .collect();
        assert_eq!(notifierd.len(), 4, "{notifierd:?}");
        assert!(notifierd[0].starts_with("POST /notify/events"));
        let primary_rows: Vec<String> = rows
            .iter()
            .filter(|(s, _)| *s == primary)
            .map(|(_, r)| r.clone())
            .collect();
        assert_eq!(primary_rows, spec_advertised_surface(spec));
        assert!(
            primary_rows
                .iter()
                .any(|r| r == "GET / -> EXPECT the frontend files, correct content types"),
            "{primary_rows:?}"
        );
        assert!(
            primary_rows
                .iter()
                .any(|r| r.starts_with("POST /api/drafts ")),
            "§5's unnamed section belongs to the primary"
        );
    }

    /// r6c's skeleton brief (plan_loaded tasks[7]) carried "POST /api/drafts -> EXPECT maker or
    /// checker" and "GET /api/drafts?state= -> EXPECT any role": §5's table is four columns
    /// (Method | Path | Role | Effect + ledger event) and cells[2] was read as the shape. The
    /// header names the column; a table whose header names none reads the last cell. The header
    /// and rows below are sb-7's, verbatim.
    #[test]
    fn the_expected_shape_column_is_named_by_the_header_not_fixed_by_index() {
        let five = "### 5. The approval workflow — maker, checker, admin\n\n\
            | Method | Path | Role | Effect + ledger event |\n\
            |---|---|---|---|\n\
            | `POST` | `/api/drafts` | maker or checker | create from `{\"amount_minor\": <int>, \"currency\": <str>, \"counterparty\": {\"name\": <str>, \"country\": <str>}, \"note\": <str>}` → `draft.created` |\n\
            | `GET` | `/api/drafts?state=` | any role | `{\"data\": [...], \"total\": <int>}`, filtered by state; unknown state = validation error |\n";
        let rows = spec_advertised_surface(five);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(
            rows[0].starts_with("POST /api/drafts -> EXPECT create from"),
            "{:?}",
            rows[0]
        );
        assert!(rows[0].contains("draft.created"), "{:?}", rows[0]);
        assert!(!rows[0].contains("maker or checker"), "{:?}", rows[0]);
        // E12: a MIXED cell keeps every backtick — the old strip left `draft.created` open.
        assert!(rows[0].ends_with("→ `draft.created`"), "{:?}", rows[0]);
        assert_eq!(rows[0].matches('`').count() % 2, 0, "{:?}", rows[0]);
        // E12: the mixed cell keeps its span intact (the old strip ate the leading backtick and
        // left the closing one dangling).
        assert!(
            rows[1].starts_with(
                "GET /api/drafts?state= -> EXPECT `{\"data\": [...], \"total\": <int>}`, filtered"
            ),
            "{:?}",
            rows[1]
        );
        assert_eq!(rows[1].matches('`').count(), 2, "{:?}", rows[1]);
        assert!(!rows[1].contains("any role"), "{:?}", rows[1]);

        // E12, the §6 shape: a cell that starts AND ends with a backtick but holds two spans is
        // not one code span — kept verbatim, both ends intact, backticks balanced.
        let six = "### 6. `notifierd`\n\n| Method | Path | Response |\n|---|---|---|\n\
                   | `POST` | `/notify/events` | `{\"events\": [...]}` → `{\"accepted\": [<seq>...], \"duplicate\": [<seq>...]}` |\n";
        let rows6 = spec_advertised_surface(six);
        assert_eq!(
            rows6,
            vec!["POST /notify/events -> EXPECT `{\"events\": [...]}` → `{\"accepted\": [<seq>...], \"duplicate\": [<seq>...]}`".to_string()]
        );
        assert_eq!(rows6[0].matches('`').count(), 4, "{:?}", rows6[0]);

        // Three columns: the header names `Response` at index 2, as before.
        let three = "| Method | Path | Response |\n|---|---|---|\n\
                     | `GET` | `/api/health` | `{\"status\": \"ok\"}` |\n";
        assert_eq!(
            spec_advertised_surface(three),
            vec!["GET /api/health -> EXPECT {\"status\": \"ok\"}".to_string()]
        );
        // A named column beats the last cell; a header naming none reads the last cell.
        let named_then_notes = "| Method | Path | Response | Notes |\n|---|---|---|---|\n\
                                | `GET` | `/api/x` | `{\"x\": 1}` | cached |\n";
        assert_eq!(
            spec_advertised_surface(named_then_notes),
            vec!["GET /api/x -> EXPECT {\"x\": 1}".to_string()]
        );
        let unnamed = "| Method | Path | Auth | Body |\n|---|---|---|---|\n\
                       | `GET` | `/api/y` | bearer | `{\"y\": 2}` |\n";
        assert_eq!(
            spec_advertised_surface(unnamed),
            vec!["GET /api/y -> EXPECT {\"y\": 2}".to_string()]
        );
        // Two tables back to back: the second header re-picks its own column.
        let both = format!("{named_then_notes}\n{five}");
        let rows = spec_advertised_surface(&both);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows[1].contains("draft.created"), "{:?}", rows[1]);

        // And on the whole real spec every drafts row carries the effect side, never the role.
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let SpecSurface { rows, .. } = spec_surface_rows(spec);
        let drafts: Vec<&str> = rows
            .iter()
            .filter(|(_, r)| r.contains("/api/drafts"))
            .map(|(_, r)| r.as_str())
            .collect();
        // §5's five rows plus the Endpoints table's `POST/GET` pointer, one row per method
        // (VA-044 F1) — those two end in the pointer's own cell, "section 5".
        assert_eq!(drafts.len(), 7, "{drafts:?}");
        for r in &drafts {
            assert!(
                !r.ends_with("maker or checker")
                    && !r.ends_with("EXPECT checker")
                    && !r.ends_with("any role"),
                "{r}"
            );
        }
    }

    /// VA-005 on the real sb-7 spec (the run's `.swarm/request.md` is this file): the three rows
    /// whose Response cell says "shape below" document their keys in a fenced block under a bold
    /// label. Top-level keys only — the GET prober asserts them with `v.get(k)`, so `registered`
    /// (inside `webhook`) or `currency` (inside `by_currency` items) must NOT be returned. A row
    /// with an inline shape, a label followed by prose (`**Sync.**`, `**Note.**`), a path with no
    /// row and notifierd's own inline rows all yield nothing — the caller then asserts nothing.
    #[test]
    fn prose_shapes_under_a_label_document_the_top_level_keys_of_shape_below_rows() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        assert_eq!(
            spec_prose_documented_keys(spec, "GET", "/api/health"),
            vec!["status", "payments", "last_sync", "webhook"]
        );
        assert_eq!(
            spec_prose_documented_keys(spec, "GET", "/api/summary"),
            vec![
                "count",
                "last_sync",
                "oldest",
                "newest",
                "by_currency",
                "reversals"
            ]
        );
        assert_eq!(
            spec_prose_documented_keys(spec, "GET", "/api/buckets"),
            vec!["timezone", "days", "statuses", "cells"]
        );
        for (method, path) in [
            ("GET", "/api/payments?limit=1&offset=1"),
            ("POST", "/api/sync"),
            ("POST", "/api/payments/<id>/note"),
            ("GET", "/api/outbox/status"),
            ("GET", "/health"),
            ("POST", "/notify/events"),
            ("GET", "/api/nowhere"),
            ("POST", "/api/health"),
        ] {
            assert!(
                spec_prose_documented_keys(spec, method, path).is_empty(),
                "{method} {path} documents no prose shape"
            );
        }
        // A label naming the PATH (not the segment) works too, the fence must be the next
        // non-empty line, and a fence that is not the shape's yields nothing.
        let doc = "## svc\n\n| Method | Path | Response |\n|---|---|---|\n\
                   | `GET` | `/api/x` | shape below |\n| `GET` | `/api/y` | shape below |\n\n\
                   The response of `GET /api/x`:\n\n```json\n{\"a\": 1, \"b\": {\"c\": 2}, \"d\": [{\"e\": 3}]}\n```\n\n\
                   **Y.**\n\nSome prose first.\n\n```json\n{\"z\": 1}\n```\n";
        assert_eq!(
            spec_prose_documented_keys(doc, "GET", "/api/x"),
            vec!["a", "b", "d"]
        );
        assert!(spec_prose_documented_keys(doc, "GET", "/api/y").is_empty());
        assert_eq!(
            top_level_keys("{\"k\": \"v:with:colons\", \"q\": [\"a\", {\"n\": 1}], \"k\": 2}"),
            vec!["k", "q"]
        );
        assert!(path_token_named("/api/x", "GET `/api/x` now"));
        assert!(!path_token_named("/health", "GET /api/health"));
        assert!(!path_token_named("/api/drafts", "/api/drafts/<id>/submit"));
        assert!(path_token_named("/api/drafts", "POST /api/drafts?state=x"));
        assert!(!path_token_named("", "anything"));
    }

    /// VA-032, rule (d)'s vocabulary, derived from sb-7's own advertised routes: `api` and
    /// `notify` are mount prefixes (each leads several base paths), `/health` is its own
    /// resource, the resource word is the first segment after the mount (`viz`, not `records`;
    /// `outbox`, not `status`), and each word carries its singular/plural sibling. The word
    /// predicate finds `draft` where the path predicate cannot (`#draft-form`, `draft.created`)
    /// and stops at word boundaries (`drafted`), case-folded.
    #[test]
    fn resource_words_derive_from_the_routes_and_match_as_words() {
        let bases: Vec<String> = [
            "/api/health",
            "/api/payments",
            "/api/summary",
            "/api/buckets",
            "/api/sync",
            "/api/webhooks/meridian",
            "/api/events",
            "/api/outbox/status",
            "/api/notifications",
            "/api/viz/records",
            "/api/stream",
            "/api/drafts",
            "/notify/events",
            "/health",
            "/notify/processed",
            "/notify/notifications",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mount = mount_prefixes(&bases);
        assert_eq!(
            mount.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["api", "notify"]
        );
        let words = |b: &str| resource_words(b, &mount);
        assert_eq!(words("/api/drafts"), vec!["drafts", "draft"]);
        assert_eq!(words("/api/viz/records"), vec!["viz", "vizs"]);
        assert_eq!(words("/api/outbox/status"), vec!["outbox", "outboxs"]);
        assert_eq!(words("/notify/events"), vec!["events", "event"]);
        assert_eq!(words("/health"), vec!["health", "healths"]);
        assert_eq!(
            words("/api/status"),
            vec!["status", "statu"],
            "an English-plural heuristic: the sibling of a singular that ends in `s` is a \
             non-word that matches nothing"
        );
        assert_eq!(
            words("/api/address"),
            vec!["address"],
            "a word ending in `ss` keeps no singular"
        );
        assert_eq!(
            words("/api"),
            vec!["api", "apis"],
            "a path made only of mount prefixes keeps its last segment"
        );
        assert!(words("/").is_empty());
        // A request whose routes share no leading segment has no mount: every first segment is
        // the resource.
        let flat: Vec<String> = ["/payments", "/drafts"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(mount_prefixes(&flat).is_empty());
        assert_eq!(
            resource_words("/payments", &mount_prefixes(&flat)),
            vec!["payments", "payment"]
        );

        let s7 = "**Drafts panel.** A token input (`#role-token`) — the bearer the page sends on \
                  every drafts call; a create form (`#draft-form`) ... enabled only when the \
                  action is legal for the draft's state.";
        assert!(resource_word_named("draft", s7));
        assert!(resource_word_named("drafts", s7));
        assert!(
            !path_token_named("/api/drafts", s7),
            "rule (a) has nothing to find here"
        );
        assert!(resource_word_named("draft", "emits `draft.created`"));
        assert!(!resource_word_named("draft", "the drafted amount"));
        assert!(
            !resource_word_named("draft", "draft_id"),
            "`_` is a word byte"
        );
        assert!(resource_word_named("health", "GET /api/health"));
        assert!(!resource_word_named("", "anything"));
    }

    /// VA-044 F1, on the real sb-7 spec: the Endpoints table's drafts pointer is written
    /// `| `POST/GET` | `/api/drafts...` | section 5 |` — a method cell naming two methods and a
    /// path with the writer's ellipsis. It used to be classed as a header row, so the Endpoints
    /// section advertised no `/api/drafts`. Now it is one row per method, the path is the base
    /// the ellipsis points beneath, and every other table's row count is exactly what it was:
    /// fourteen Endpoints rows before the pair, §5's five after it, notifierd's four.
    #[test]
    fn a_method_cell_naming_two_methods_is_one_row_per_method() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let SpecSurface { primary, rows } = spec_surface_rows(spec);
        assert_eq!(rows.len(), 25, "{rows:?}");
        let ledgerd: Vec<&str> = rows
            .iter()
            .filter(|(s, _)| *s == primary)
            .map(|(_, r)| r.as_str())
            .collect();
        assert_eq!(ledgerd.len(), 21, "{ledgerd:?}");
        assert!(
            ledgerd[0].starts_with("GET / -> EXPECT"),
            "{:?}",
            ledgerd[0]
        );
        assert!(
            ledgerd[13].starts_with("GET /api/stream -> EXPECT SSE, section 8"),
            "the fourteen single-method Endpoints rows come first: {:?}",
            ledgerd[13]
        );
        assert_eq!(ledgerd[14], "POST /api/drafts -> EXPECT section 5");
        assert_eq!(ledgerd[15], "GET /api/drafts -> EXPECT section 5");
        assert!(
            ledgerd[16].starts_with("POST /api/drafts -> EXPECT create from"),
            "§5's five rows follow, unchanged: {:?}",
            ledgerd[16]
        );
        assert!(
            ledgerd[20].starts_with("GET /api/drafts?state= -> EXPECT"),
            "{:?}",
            ledgerd[20]
        );
        assert_eq!(
            rows.iter()
                .filter(|(s, _)| s.as_deref() == Some("notifierd"))
                .count(),
            4
        );
        assert!(
            !rows.iter().any(|(_, r)| {
                r.split_whitespace()
                    .nth(1)
                    .is_some_and(|path| path.ends_with(['.', '…']))
            }),
            "the ellipsis is notation, never a path byte: {rows:?}"
        );
        // The mutating-path view dedups by path, so the pointer adds no second POST; the
        // Endpoints row comes first in the document, so `/api/drafts` now precedes its
        // templated siblings.
        let posts = spec_post_endpoints(spec);
        assert_eq!(
            posts.iter().filter(|p| *p == "/api/drafts").count(),
            1,
            "{posts:?}"
        );
        assert!(
            posts.iter().position(|p| p == "/api/drafts")
                < posts.iter().position(|p| p == "/api/drafts/<id>/submit"),
            "{posts:?}"
        );

        // The rule on its own: every alternative must be a method, whitespace around the
        // slash is tolerated, and a cell that mixes a method with a non-method is a header.
        let doc = "| Method | Path | Response |\n|---|---|---|\n\
                   | `POST/GET` | `/api/x...` | see below |\n\
                   | GET / HEAD | `/api/y` | `{\"y\": 1}` |\n\
                   | `GET/foo` | `/api/z` | never |\n";
        assert_eq!(
            spec_advertised_surface(doc),
            vec![
                "POST /api/x -> EXPECT see below".to_string(),
                "GET /api/x -> EXPECT see below".to_string(),
                "GET /api/y -> EXPECT {\"y\": 1}".to_string(),
                "HEAD /api/y -> EXPECT {\"y\": 1}".to_string(),
            ]
        );
    }
}
