//! THE SPEC'S ADVERTISED SURFACE: the endpoint-table rows a request documents, each tagged with
//! the service whose section it sits in. ONE parser feeds every consumer — the GET and POST
//! probers, the unprobed disclosure, the plan repair's unassigned-endpoint rule, the smoke-fix
//! brief and the sink's angle — so a second table reader cannot drift from the first the way the
//! inline decomposition counters once did. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases); moved verbatim from swarm.rs with
//! its tests, then amended for r6c's measured defect (the expected-shape column was a fixed
//! index, and §5's fourth column made that the ROLE). The WHY of every rule stays in its own
//! comment.

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
    let unwrap_cell = |c: &str| c.trim().trim_matches('`').trim().to_string();
    // THE PATH IS THE FIRST BACKTICKED TOKEN when the cell opens with a backtick, else the first
    // whitespace token. sb-7's row `| `GET` | `/` + `web/*` | …` stripped by `unwrap_cell` to
    // "/` + `web/*"; every consumer then took its first whitespace token, "/`", and the gate curled
    // an endpoint that exists nowhere — r0's "GET /` returned 404". The REAL `/` was never emitted
    // at all, so the frontend row went unprobed while its phantom twin blocked green.
    let path_cell = |c: &str| -> String {
        let c = c.trim();
        match c.strip_prefix('`') {
            Some(rest) => rest.split('`').next().unwrap_or("").trim().to_string(),
            None => c.split_whitespace().next().unwrap_or("").to_string(),
        }
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
        let method = unwrap_cell(cells[0]).to_uppercase();
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        ) {
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
        rows.push((
            service,
            if expected.is_empty() {
                format!("{method} {path}")
            } else {
                format!("{method} {path} -> EXPECT {expected}")
            },
        ));
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
        assert!(
            rows[1].starts_with(
                "GET /api/drafts?state= -> EXPECT {\"data\": [...], \"total\": <int>}"
            ),
            "{:?}",
            rows[1]
        );
        assert!(!rows[1].contains("any role"), "{:?}", rows[1]);

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
        assert_eq!(drafts.len(), 5, "{drafts:?}");
        for r in &drafts {
            assert!(
                !r.ends_with("maker or checker")
                    && !r.ends_with("EXPECT checker")
                    && !r.ends_with("any role"),
                "{r}"
            );
        }
    }
}
