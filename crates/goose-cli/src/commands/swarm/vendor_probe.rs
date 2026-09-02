//! The vendor probe (P1-8 / P1-12 / VA-105): the vendor's docs page WHOLE and one live page of
//! each GET it advertises, carried into the planner's and every worker's `doc_facts` with every
//! cut and every failed fetch SAID.
//!
//! r6h measured the silent form this module closes. The P1-8 excerpt cut the docs page at 6,000
//! chars mid-bullet ("- Validation errors ") and lost §7 Reversals, §8 Webhooks (Registration,
//! Deliveries) and §9 Operational notes — 3,098 of 9,098 chars — for the planner and every
//! worker, while the block's header called the fragment "the vendor's REAL responses… Use these
//! literals EXACTLY" and the `vendor_probe` event reported only the post-cut `bytes` (18,440).
//! `ledgerd-core` diagnosed it on its own ("the injected probe only showed sections 1-6
//! (truncated)") and spent two `curl … | sed -n '/## 7/,$p'` turns at 06:14 and 06:16 recovering
//! what the engine had fetched and thrown away. `/v3/reversals` (516,203 B) and `/v3/payments`
//! (14,973 B) were cut the same way, unmarked, and the `/v3/payments?cursor=1` 400 was named
//! nowhere (`fetched: 2` of 3).
//!
//! The rule now: the docs page is the CONTRACT the run is built against and rides whole; an
//! endpoint body is a SAMPLE of the shape and is cut after a whole JSON object at the body
//! budget, with a CUT marker at the cut carrying the full body's facts and the exact recovery
//! command; a GET the probe could not fetch is listed with the vendor's own answer; and the
//! header says what the block IS.

use std::path::Path;

use super::probeable_get_path;

/// The VENDOR the spec tells the builder to integrate against: (docs_url, base_url, api_key).
/// Parsed from the spec's own idiom — "documentation is at `URL`", "Base URL `URL`",
/// "API key `KEY`" — so this is spec-derived, never benchmark-specific; a spec that names no
/// vendor yields Nones and every consumer stays inert. Pure/testable.
pub(super) fn spec_vendor(spec: &str) -> (Option<String>, Option<String>, Option<String>) {
    let grab = |pat: &str| {
        regex::Regex::new(pat)
            .ok()
            .and_then(|re| re.captures(spec))
            .map(|c| c[1].to_string())
    };
    (
        grab(r"documentation is at\s+`(https?://[^`]+)`"),
        grab(r"[Bb]ase URL\s+`(https?://[^`]+)`"),
        grab(r"API key\s+`([^`]+)`"),
    )
}

/// P1-8: every GET path a vendor's DOCS BODY advertises, deduped, made probeable (templated
/// COUNT values filled, param'd paths excluded — `probeable_get_path`'s rules). The docs page is
/// the vendor's own text, not the spec, so this is the plain `GET /path` idiom rather than the
/// spec's markdown-table surface.
///
/// VA-126: a templated query value that is not a count (`?cursor=<next>` — an opaque token only
/// the vendor can mint) is NOT filled and the path is not requested: the literal 1
/// `probeable_get_path` fills is valid for every count/offset/page parameter and a guess for
/// anything else, and r6h's blind `/v3/payments?cursor=1` 400'd `bad_cursor` on every run. The
/// path rides back as `PaginationSkipped` so the caller SAYS the absence
/// (`vendor_probe_pagination_skipped`). Reading the parameter's first-page form off the docs is a
/// later, measured step — the golden run never saw a page-2 body, so none is added now.
fn vendor_docs_get_paths(docs_text: &str) -> (Vec<String>, Vec<PaginationSkipped>) {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let mut skipped: Vec<PaginationSkipped> = Vec::new();
    if let Ok(re) = regex::Regex::new(r"GET\s+(/[A-Za-z0-9_./{}<>:?&=-]*)") {
        for c in re.captures_iter(docs_text) {
            let raw = &c[1];
            if let Some(param) = opaque_query_template(raw) {
                if !skipped.iter().any(|s| s.url == raw) {
                    skipped.push(PaginationSkipped {
                        url: raw.to_string(),
                        param,
                    });
                }
                continue;
            }
            if let Some(p) = probeable_get_path(raw) {
                if seen.insert(p.clone()) {
                    out.push(p);
                }
            }
        }
    }
    (out, skipped)
}

/// The first query parameter of `path` whose templated value is not a COUNT. A placeholder names
/// its own kind: `<int>`, `{n}`, `<number>`, `<count>` are counts (the literal 1 is valid for
/// them); `<next>`, `<cursor>`, `<opaque>`, `<string>` are tokens no literal can stand in for.
/// `None` when every templated value is a count or the path has no query.
fn opaque_query_template(path: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if !v.contains(['{', '<', ':']) {
            return None;
        }
        let kind = v
            .trim_matches(|c| matches!(c, '{' | '}' | '<' | '>' | ':'))
            .to_ascii_lowercase();
        if matches!(
            kind.as_str(),
            "int" | "integer" | "n" | "num" | "number" | "count"
        ) {
            None
        } else {
            Some(k.to_string())
        }
    })
}

/// An advertised GET the probe did not request because its query template is an opaque token
/// (VA-126). `url` is the path as the docs wrote it, `param` the parameter that stopped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PaginationSkipped {
    pub(super) url: String,
    pub(super) param: String,
}

/// What the vendor probe measured. `ok` is the DOCS fetch outcome — the probe's one load-bearing
/// page; `block` is empty exactly when nothing usable came back, and an empty block injected into
/// `doc_facts` is a no-op, never a failure.
pub(super) struct VendorProbeOutcome {
    pub(super) ok: bool,
    pub(super) block: String,
    pub(super) endpoints: Vec<String>,
    pub(super) fetched: usize,
    pub(super) bytes: usize,
    pub(super) error: String,
    /// P1-12: the vendor's OWN row truth, read off page 1 of its advertised GETs — the `total`
    /// field when the body documents one, and the first collection array's length. The GATE's
    /// `sync_rows` row compares the app's own row count against these; they are persisted to
    /// `.swarm/vendor-probe.json` because a number that lives only in an event cannot be read
    /// by a later gate.
    pub(super) vendor_total: Option<i64>,
    pub(super) page1_rows: Option<i64>,
    /// VA-105: the docs page as fetched and as carried, in chars. Equal by construction — the
    /// page is the contract and rides whole — and BOTH ride the `vendor_probe` event so a reader
    /// sees wholeness as a number instead of inferring it from the block's post-assembly `bytes`
    /// (r6h's 18,440 said nothing about the 3,098 chars that were gone).
    pub(super) docs_chars: usize,
    pub(super) docs_kept: usize,
    /// VA-105: every endpoint body cut before it rode the brief — one `vendor_probe_truncated`
    /// event each, and a CUT marker in the block at the cut.
    pub(super) cuts: Vec<ProbeCut>,
    /// VA-105: every advertised GET whose blind probe came back without a usable body — one
    /// `vendor_probe_fetch_failed` event each, and a "(not fetched: …)" line in the block where
    /// the page would have been.
    pub(super) fetch_failures: Vec<ProbeFetchFailure>,
    /// VA-126: every advertised GET whose query template is an opaque token — never requested,
    /// one `vendor_probe_pagination_skipped` event each, no line in the block.
    pub(super) pagination_skipped: Vec<PaginationSkipped>,
    /// When the pages were fetched (RFC 3339, seconds) — the header names it so a worker knows
    /// the block is a snapshot, not the live vendor.
    pub(super) fetched_at: String,
}

/// One endpoint body the probe cut: the full body's size, what was kept, and whether the cut
/// landed after a whole JSON object (the marker's wording depends on it).
pub(super) struct ProbeCut {
    pub(super) url: String,
    pub(super) chars: usize,
    pub(super) kept: usize,
    pub(super) at_object_boundary: bool,
}

pub(super) struct ProbeFetchFailure {
    pub(super) url: String,
    pub(super) error: String,
}

/// P1-12, pure: the two row-evidence numbers one JSON body can carry — (`total` field, first
/// collection array's length over the names this bench family uses). Both None for a body that
/// is not JSON or carries neither: the caller then abstains rather than inventing a zero.
fn json_rows_and_total(body: &str) -> (Option<i64>, Option<i64>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
        return (None, None);
    };
    let total = v.get("total").and_then(|t| t.as_i64());
    let rows = [
        "data",
        "items",
        "rows",
        "events",
        "processed",
        "notifications",
    ]
    .iter()
    .find_map(|k| v.get(*k).and_then(|a| a.as_array()).map(|a| a.len() as i64));
    (total, rows)
}

/// P1-12, pure: one number for "how many rows does this body prove" — the documented `total`
/// outranks a page length (a page is bounded by `limit`; the total is the collection).
pub(super) fn json_rows_evidence(body: &str) -> Option<i64> {
    let (total, rows) = json_rows_and_total(body);
    total.or(rows)
}

/// VA-105, pure: the top-level keys of a JSON object body, SORTED — the one fact about a cut
/// body's SHAPE the excerpt cannot lose, rendered the same whatever map order serde_json builds
/// (goose-cli declares no `preserve_order`; a transitive dependency may). Empty for anything that
/// is not a JSON object (an array, a scalar, HTML), and the marker says so instead of listing
/// nothing.
fn json_top_level_keys(body: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(body.trim()) {
        Ok(serde_json::Value::Object(m)) => {
            let mut keys: Vec<String> = m.keys().cloned().collect();
            keys.sort();
            keys
        }
        _ => Vec::new(),
    }
}

/// VA-105, pure: the top-level key whose value is the body's row array — the largest array, ties
/// by name — so the header names where THIS vendor's rows live instead of a baked example. None
/// when no top-level value is an array (the header then carries no rows clause at all).
fn json_row_array_key(body: &str) -> Option<String> {
    let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(body.trim())
    else {
        return None;
    };
    let mut arrays: Vec<(usize, String)> = m
        .iter()
        .filter_map(|(k, v)| v.as_array().map(|a| (a.len(), k.clone())))
        .collect();
    arrays.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    arrays.into_iter().next().map(|(_, k)| k)
}

/// P1-12: the vendor row truth the RUN persisted at BUILD start (`.swarm/vendor-probe.json`,
/// written beside the vendor_probe event). None when the file is absent or carries no number —
/// an offline replay of an older tree, or a spec with no vendor.
pub(super) fn read_vendor_probe_rows(root: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(root.join(".swarm").join("vendor-probe.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("vendor_total")
        .and_then(|t| t.as_i64())
        .or_else(|| v.get("page1_rows").and_then(|t| t.as_i64()))
}

/// P1-8's page budget, unchanged in value and applied to ENDPOINT BODIES ONLY since VA-105. A
/// page of rows is a SAMPLE of the vendor's shape — r6h's `/v3/reversals` page is 516,203 B and
/// no brief can carry it — so the excerpt keeps whole JSON objects up to this many chars and
/// SAYS the cut. This bounds prompt text, never model work. The docs page is the contract the
/// run is built against and is never cut: r6h lost §7–§9 (Reversals, Webhooks, Operational
/// notes) to this very constant, and the planner and every worker built against a fragment
/// their header called complete. VA-126: this is the REFERENCE value on the 262,144 window; the
/// live budget is `budgets::ShownBudgets::vendor_body_chars` (scaled from the fleet's probed
/// window) and every caller passes it as `body_chars`.
pub(super) const VENDOR_PROBE_BODY_CHARS: usize = 6_000; // measured: r6h value on the 262,144 reference window (r6h-golden-0.4616)

/// VA-105, pure: an endpoint body's excerpt — the whole body when it fits `body_chars`, else the
/// longest prefix under the budget that ends after a whole JSON object (`},` / `}]` / `}` — a row
/// close, never mid-key; a body with no such boundary under the budget is cut raw and the marker
/// says so). The kept text is a SAMPLE and is not valid JSON on its own; the marker names that.
// string_slice: `budget_end` is a `char_indices` offset or `t.len()`; `cut_at` is an `rfind` hit
// moved past its ASCII `}` — char boundaries by construction.
#[allow(clippy::string_slice)]
pub(super) fn excerpt_body(url: &str, body: &str, body_chars: usize) -> (String, Option<ProbeCut>) {
    let t = body.trim();
    let chars = t.chars().count();
    if chars <= body_chars {
        return (t.to_string(), None);
    }
    let budget_end = t.char_indices().nth(body_chars).map_or(t.len(), |(i, _)| i);
    let head = &t[..budget_end];
    // Priority, not position: a row separator (`}, {`) beats a close-bracket beats a nested
    // object's `},` beats any `}` — so a page of rows is cut between ROWS, never after a row's
    // nested `counterparty` object.
    let boundary = ["}, {", "},{", "}]", "},", "}"]
        .iter()
        .find_map(|sep| head.rfind(*sep))
        .map(|i| i + 1);
    let cut_at = boundary.unwrap_or(budget_end);
    let kept = head[..cut_at].to_string();
    let cut = ProbeCut {
        url: url.to_string(),
        chars,
        kept: kept.chars().count(),
        at_object_boundary: boundary.is_some(),
    };
    (kept, Some(cut))
}

/// VA-105: the marker that stands at a body's cut — the facts the cut hid (the FULL body's
/// top-level keys, its `total`, its row count) and the exact command that fetches the rest. A
/// worker that reads the marker knows what the whole body proves without re-deriving it, and a
/// worker that needs more knows the one call to make.
pub(super) fn body_cut_marker(cut: &ProbeCut, api_key: Option<&str>, body: &str) -> String {
    let (total, rows) = json_rows_and_total(body);
    let keys = json_top_level_keys(body);
    let shape = if keys.is_empty() {
        "The FULL body is not a JSON object".to_string()
    } else {
        format!("The FULL body's top-level keys: {}", keys.join(", "))
    };
    let mut facts = Vec::new();
    if let Some(t) = total {
        facts.push(format!("total={t}"));
    }
    if let Some(r) = rows {
        facts.push(format!("{r} rows in its collection array"));
    }
    let facts = if facts.is_empty() {
        "no `total` field and no collection array under a name the probe knows".to_string()
    } else {
        facts.join("; ")
    };
    let boundary = if cut.at_object_boundary {
        "ending after a whole JSON object"
    } else {
        "cut mid-text — no JSON object boundary under the budget"
    };
    // No key in the spec means no header on the recovery command — the honest empty.
    let auth = match api_key {
        Some(k) => format!(" -H 'Authorization: Bearer {k}'"),
        None => String::new(),
    };
    format!(
        "[CUT by the probe: the first {} of {} chars, {boundary}. {shape}; {facts}. The rest is one \
         call away: `curl -s{auth} {}` ({} chars — pipe through `head -c` to what you need).]",
        cut.kept, cut.chars, cut.url, cut.chars
    )
}

/// P1-8: fetch the vendor's docs page and ONE page of each GET it advertises, so every worker's
/// `doc_facts` carries the vendor's REAL key names before a line of sync code is written.
///
/// The r2 root critical: vendor_sync.py issued ZERO list requests in sync #1 → 0/12288 payments,
/// with 31 vacuous legs under it — the builder guessed the vendor's shape (`items`) instead of
/// reading it (`data`, `amount_minor`). One real response body in the prompt is the cheapest
/// possible correction.
///
/// Transport bounds only: a CONNECT timeout (a refused or silent-to-connect vendor answers
/// promptly) and NO read window — II-7 deleted the read cut class, and this fetch inherits that
/// rule. The docs page rides whole; an endpoint body is excerpted AFTER it arrives
/// (`excerpt_body`), which bounds the prompt, not the transport, and every cut is said. Every
/// failure is an outcome (`ok:false`, empty block; a failed endpoint page is listed, not
/// dropped), never an error path: a spec whose vendor is down still builds exactly as it would
/// have.
pub(super) async fn probe_vendor(
    docs_url: &str,
    base_url: &str,
    api_key: Option<&str>,
    body_chars: usize,
) -> VendorProbeOutcome {
    let fetched_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let dead = |error: String, fetched_at: String| VendorProbeOutcome {
        ok: false,
        block: String::new(),
        endpoints: Vec::new(),
        fetched: 0,
        bytes: 0,
        error,
        vendor_total: None,
        page1_rows: None,
        docs_chars: 0,
        docs_kept: 0,
        cuts: Vec::new(),
        fetch_failures: Vec::new(),
        pagination_skipped: Vec::new(),
        fetched_at,
    };
    // A client that cannot be built (a TLS backend that fails to initialise) is a measured
    // `ok:false` with its reason, not a default client that panics on first use.
    let client = match reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return dead(format!("http client: {e}"), fetched_at),
    };
    let fetch = |url: String| {
        let client = client.clone();
        let auth = api_key.map(|k| format!("Bearer {k}"));
        async move {
            let mut req = client.get(&url);
            if let Some(a) = auth {
                req = req.header("Authorization", a);
            }
            match req.send().await {
                Ok(r) => {
                    let status = r.status().as_u16();
                    match r.text().await {
                        Ok(t) if (200..300).contains(&status) && !t.trim().is_empty() => Ok(t),
                        Ok(_) if (200..300).contains(&status) => {
                            Err(format!("status {status}, empty body"))
                        }
                        // The vendor's own words ride the failure (r6h: `{"error": "bad_cursor"}`),
                        // clipped so an HTML error page cannot become the block.
                        Ok(t) => Err(format!(
                            "status {status}: {}",
                            t.trim().chars().take(200).collect::<String>()
                        )),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
    };
    let docs_text = match fetch(docs_url.to_string()).await {
        Ok(t) => t,
        Err(e) => return dead(e, fetched_at),
    };
    // As served, untrimmed: `docs_chars` is the page's own size (r6h: 9,098), and the block
    // carries exactly those chars — kept == chars is the wholeness a reader can check.
    let docs_chars = docs_text.chars().count();
    let (endpoints, pagination_skipped) = vendor_docs_get_paths(&docs_text);
    let mut pages: Vec<String> = vec![format!("### GET {docs_url}\n{docs_text}")];
    let mut fetched = 0usize;
    let mut vendor_total: Option<i64> = None;
    let mut page1_rows: Option<i64> = None;
    let mut cuts: Vec<ProbeCut> = Vec::new();
    let mut fetch_failures: Vec<ProbeFetchFailure> = Vec::new();
    let mut row_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for path in &endpoints {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        match fetch(url.clone()).await {
            Ok(body) => {
                // P1-12: read the row truth off the FULL body before it is excerpted for the prompt.
                let (t, r) = json_rows_and_total(&body);
                vendor_total = vendor_total.max(t);
                page1_rows = page1_rows.max(r);
                if let Some(k) = json_row_array_key(&body) {
                    row_keys.insert(k);
                }
                let (kept, cut) = excerpt_body(&url, &body, body_chars);
                match cut {
                    Some(cut) => {
                        let marker = body_cut_marker(&cut, api_key, &body);
                        pages.push(format!("### GET {url}\n{kept}\n{marker}"));
                        cuts.push(cut);
                    }
                    None => pages.push(format!("### GET {url}\n{kept}")),
                }
                fetched += 1;
            }
            Err(error) => {
                pages.push(format!(
                    "### GET {url}\n(not fetched: {error} — the probe's blind request; the docs \
                     above say how this endpoint is called)"
                ));
                fetch_failures.push(ProbeFetchFailure { url, error });
            }
        }
    }
    let bodies = if cuts.is_empty() {
        ", whole".to_string()
    } else {
        format!(
            ", cut after a whole JSON object where a CUT marker says so ({} of {fetched} cut; \
             each marker carries the full body's keys, total and row count and the command that \
             fetches the rest)",
            cuts.len()
        )
    };
    let failures = if fetch_failures.is_empty() {
        String::new()
    } else {
        format!(
            " {} advertised GET(s) came back without a body and are listed with the vendor's \
             answer instead of a page.",
            fetch_failures.len()
        )
    };
    // Where THIS vendor's rows live, read off the fetched bodies — never a baked example (`items`
    // for `data` was sb-7's fact and wrong advice for a vendor whose key IS `items`). No fetched
    // body with a row array → no clause.
    let rows_note = if row_keys.is_empty() {
        String::new()
    } else {
        format!(
            " The rows of the fetched pages live under {} — that key, not a guessed one.",
            row_keys
                .iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(" / ")
        )
    };
    let block = format!(
        "## Vendor probe — the vendor's live responses, fetched by the engine at {fetched_at}\n\
         The docs page below is complete ({docs_chars} chars, as served). Each endpoint page is \
         one live response body{bodies}.{failures} The key names and body shapes are the vendor's \
         own — use these literals exactly, never a guessed name.{rows_note}\n\n{}",
        pages.join("\n\n")
    );
    let bytes = block.len();
    VendorProbeOutcome {
        ok: true,
        block,
        endpoints,
        fetched,
        bytes,
        error: String::new(),
        vendor_total,
        page1_rows,
        docs_chars,
        docs_kept: docs_chars,
        cuts,
        fetch_failures,
        pagination_skipped,
        fetched_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// r6h's `/v3/docs` exactly as the live vendor served it (127.0.0.1:8850, 2026-09-02):
    /// 9,150 B / 9,098 chars, nine `##` sections. The old 6,000-char excerpt cut it inside §6.
    const R6H_V3_DOCS: &str = include_str!("testdata/va105/v3_docs.md");

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-vendor-probe-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A one-thread HTTP fixture answering `n` requests by path, each with `Connection: close`.
    fn serve(
        n: usize,
        respond: fn(&str) -> (u16, String),
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut s, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let read = std::io::Read::read(&mut s, &mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..read]).to_string();
                let (status, body) = respond(&req);
                let reason = if status == 200 { "OK" } else { "Bad Request" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = std::io::Write::write_all(&mut s, resp.as_bytes());
            }
        });
        (format!("http://{addr}"), server)
    }

    fn payments_page() -> String {
        let rows: Vec<String> = (0..200)
            .map(|i| {
                format!(
                    r#"{{"id": "pay_{i:05}", "amount_minor": {}, "currency": "USD", "status": "pending", "counterparty": {{"name": "Aurora Freight", "country": "DE"}}}}"#,
                    1000 + i
                )
            })
            .collect();
        format!(
            r#"{{"data": [{}], "total": 12288, "limit": 200}}"#,
            rows.join(", ")
        )
    }

    #[test]
    fn spec_vendor_parses_the_spec_idiom_and_stays_inert_without_it() {
        // The exact idiom both spec-build.md versions use — the F825 vendor-truth chain
        // starts here, so the parse is pinned against silent drift.
        let spec = "The Meridian API documentation is at `http://127.0.0.1:8935/v1/docs`. \
                    Read it before you start. Base URL `http://127.0.0.1:8935`,\n\
                    API key `sk_test_meridian`.";
        let (docs, base, key) = spec_vendor(spec);
        assert_eq!(docs.as_deref(), Some("http://127.0.0.1:8935/v1/docs"));
        assert_eq!(base.as_deref(), Some("http://127.0.0.1:8935"));
        assert_eq!(key.as_deref(), Some("sk_test_meridian"));

        // A spec naming no vendor must yield Nones — the whole chain stays inert, and the
        // Vacuous arm keeps its original never-blame-the-app behavior.
        let (d2, b2, k2) = spec_vendor("Build a todo app. No vendor here.");
        assert!(d2.is_none() && b2.is_none() && k2.is_none());
    }

    /// P1-8 isolation fixture: a local vendor serving `/v3/docs` + `/v3/payments`. The probe must
    /// deliver BOTH real literals (`data`, `amount_minor`) into the doc_facts block — the two
    /// names r2's sync guessed wrong on its way to 0/12288 — and a dead vendor must come back
    /// promptly as a measured `ok:false`, never a failure or a wait.
    #[tokio::test]
    async fn the_vendor_probe_delivers_real_body_literals_and_measures_a_dead_vendor() {
        let (base, server) = serve(2, |req| {
            if req.starts_with("GET /v3/docs") {
                (
                    200,
                    "Meridian vendor API.\nGET /v3/payments returns one page of payments."
                        .to_string(),
                )
            } else {
                (
                    200,
                    r#"{"data":[{"id":"p_1","amount_minor":1250,"currency":"EUR"}],"total":12288,"limit":100,"offset":0}"#
                        .to_string(),
                )
            }
        });
        let probe = probe_vendor(
            &format!("{base}/v3/docs"),
            &base,
            Some("sk_test"),
            VENDOR_PROBE_BODY_CHARS,
        )
        .await;
        server.join().unwrap();
        assert!(probe.ok, "a live vendor probes ok: {}", probe.error);
        assert_eq!(probe.endpoints, vec!["/v3/payments".to_string()]);
        assert_eq!(probe.fetched, 1, "one page of the one advertised GET");
        assert!(
            probe.block.contains(r#""data""#),
            "the vendor's real collection key must reach doc_facts verbatim: {}",
            probe.block
        );
        assert!(
            probe.block.contains("amount_minor"),
            "the vendor's real field literal must reach doc_facts verbatim: {}",
            probe.block
        );
        assert!(probe.cuts.is_empty() && probe.fetch_failures.is_empty());
        assert!(
            probe.block.contains("one live response body, whole."),
            "{}",
            probe.block
        );

        // The dead-vendor case: a port nothing listens on refuses the connect, and the probe
        // returns a MEASUREMENT — ok:false, empty block — promptly, on the transport's connect
        // path alone. (The elapsed assertion is a test measuring a test fixture, not a cap on
        // model work: no model is anywhere near this code.)
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_addr = dead.local_addr().unwrap();
        drop(dead);
        let started = std::time::Instant::now();
        let probe = probe_vendor(
            &format!("http://{dead_addr}/v3/docs"),
            &format!("http://{dead_addr}"),
            None,
            VENDOR_PROBE_BODY_CHARS,
        )
        .await;
        assert!(!probe.ok);
        assert!(probe.block.is_empty(), "{}", probe.block);
        assert!(!probe.error.is_empty());
        assert_eq!((probe.docs_chars, probe.docs_kept), (0, 0));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "connect-refused must answer promptly, not wait on a read"
        );
    }

    /// VA-105, the r6h case end to end: the vendor serves r6h's real docs page and the GETs it
    /// advertises answer as the live vendor did — a payments page over the body budget, a small
    /// reversals page. The block must carry every section the old cut lost (§7, §8
    /// Registration/Deliveries, §9), say the docs page is complete, and stand a CUT marker where
    /// the payments page was cut — with the full body's facts and the recovery curl. VA-126: the
    /// third advertised GET, `/v3/payments?cursor=<next>`, is NOT requested — r6h's blind
    /// `?cursor=1` 400'd `bad_cursor` — and rides back as `pagination_skipped`; the fixture's 400
    /// arm stays as the tripwire that would fire if the probe ever guessed again (the server
    /// answers exactly three requests, so a fourth would also hang the join).
    #[tokio::test]
    async fn r6h_docs_page_rides_whole_and_every_cut_and_failed_fetch_is_said() {
        assert_eq!(
            R6H_V3_DOCS.chars().count(),
            9_098,
            "the fixture is r6h's live page"
        );
        let old_excerpt: String = R6H_V3_DOCS.chars().take(6_000).collect();
        assert!(
            !old_excerpt.contains("## 7. Reversals"),
            "the measured defect: the P1-8 cut lost §7 onward"
        );

        let (base, server) = serve(3, |req| {
            if req.starts_with("GET /v3/docs") {
                (200, R6H_V3_DOCS.to_string())
            } else if req.starts_with("GET /v3/payments?") {
                (400, r#"{"error": "bad_cursor"}"#.to_string())
            } else if req.starts_with("GET /v3/payments") {
                (200, payments_page())
            } else {
                (
                    200,
                    r#"{"data": [{"id": "rev_00000", "payment_id": "pay_00001", "amount_minor": 3760, "currency": "EUR"}], "total": 1}"#
                        .to_string(),
                )
            }
        });
        let probe = probe_vendor(
            &format!("{base}/v3/docs"),
            &base,
            Some("sk_test"),
            VENDOR_PROBE_BODY_CHARS,
        )
        .await;
        server.join().unwrap();
        assert!(probe.ok, "{}", probe.error);

        // The contract rides whole: the sections the cut lost are in the block, and the event
        // numbers say so without inference.
        for section in [
            "## 6. Creating payments",
            "- Validation errors",
            "## 7. Reversals — `GET /v3/reversals`",
            "## 8. Webhooks",
            "### Registration — `POST /v3/webhooks`",
            "### Deliveries",
            "## 9. Operational notes",
        ] {
            assert!(probe.block.contains(section), "lost again: {section}");
        }
        assert_eq!((probe.docs_chars, probe.docs_kept), (9_098, 9_098));
        assert!(
            probe
                .block
                .contains("The docs page below is complete (9098 chars, as served)."),
            "{}",
            probe.block.lines().take(3).collect::<Vec<_>>().join("\n")
        );
        assert!(
            !probe.block.contains("REAL responses") && !probe.block.contains("just now"),
            "the overclaiming header is gone"
        );

        // VA-126: r6h's event listed three GETs and fetched two; the opaque `?cursor=<next>` is
        // now never requested — two advertised, two fetched, the skip said by name.
        assert_eq!(
            probe.endpoints,
            vec!["/v3/payments".to_string(), "/v3/reversals".to_string()],
            "{:?}",
            probe.endpoints
        );
        assert_eq!(probe.fetched, 2, "both probeable GETs fetched");
        assert_eq!(
            probe.pagination_skipped,
            vec![PaginationSkipped {
                url: "/v3/payments?cursor=<next>".to_string(),
                param: "cursor".to_string(),
            }]
        );
        assert!(
            probe.fetch_failures.is_empty(),
            "no blind cursor request, so no 400: {:?}",
            probe
                .fetch_failures
                .iter()
                .map(|f| &f.url)
                .collect::<Vec<_>>()
        );
        // The docs page itself still names `?cursor=<next>` and `400 bad_cursor` (it rides
        // whole); what must be absent is any PAGE or failure line for the un-requested GET.
        assert!(
            !probe.block.contains("(not fetched:")
                && !probe
                    .block
                    .contains(&format!("### GET {base}/v3/payments?cursor")),
            "nothing about the un-requested page reaches the brief: {}",
            probe.block
        );
        assert!(!probe.block.contains("came back without a body"));

        // The payments page is over budget: cut after a whole row, marked with the full body's
        // facts and the exact recovery command; the reversals page is whole.
        assert_eq!(probe.cuts.len(), 1, "one body over the budget");
        let cut = &probe.cuts[0];
        assert!(cut.url.ends_with("/v3/payments"));
        assert_eq!(cut.chars, payments_page().chars().count());
        assert!(cut.kept <= VENDOR_PROBE_BODY_CHARS && cut.at_object_boundary);
        assert_eq!(
            (cut.chars, cut.kept),
            (28_840, 5_912),
            "the 200-row page is 28,840 chars; the last row boundary under 6,000 ends at 5,912"
        );
        assert!(
            probe.block.contains(
                "[CUT by the probe: the first 5912 of 28840 chars, ending after a whole JSON \
                 object. The FULL body's top-level keys: data, limit, total; total=12288; 200 rows \
                 in its collection array."
            ),
            "{}",
            probe.block
        );
        assert!(
            probe.block.contains(
                "The rows of the fetched pages live under `data` — that key, not a guessed one."
            ),
            "the rows clause is THIS vendor's key, read off its bodies: {}",
            probe.block.lines().take(3).collect::<Vec<_>>().join("\n")
        );
        assert!(
            !probe.block.contains("`items` for `data`"),
            "no baked example"
        );
        assert!(
            probe.block.contains(&format!(
                "`curl -s -H 'Authorization: Bearer sk_test' {base}/v3/payments`"
            )),
            "{}",
            probe.block
        );
        assert!(probe.block.contains("1 of 2 cut"), "{}", probe.block);
        assert_eq!(
            probe.vendor_total,
            Some(12288),
            "row truth read off the FULL body"
        );
        assert_eq!(probe.page1_rows, Some(200));
        assert!(
            probe.block.contains(r#""id": "rev_00000""#),
            "the small page rides whole"
        );
    }

    /// VA-126: a count template is filled with the literal 1 (the Q2 measured fix — sb-6's
    /// payments row was being dropped for the `<` in `limit=<int>`); an opaque template is not
    /// requested and is returned as skipped, once per path; a bare path advertised beside its
    /// templated form is probed once. The r6h docs page yields exactly r6h's two probeable GETs
    /// and the one skip.
    #[test]
    fn opaque_query_templates_are_skipped_and_count_templates_are_filled() {
        let (paths, skipped) = vendor_docs_get_paths(
            "GET /api/payments?limit=<int>&offset=<int>\nGET /api/items?cursor=<next>\n\
             GET /api/items?cursor=<next>\nGET /api/items/{id}\nGET /api/health\n\
             GET /api/after?since={n}\nGET /api/walk?page=<opaque>",
        );
        assert_eq!(
            paths,
            vec![
                "/api/payments?limit=1&offset=1".to_string(),
                "/api/health".to_string(),
                "/api/after?since=1".to_string(),
            ]
        );
        assert_eq!(
            skipped,
            vec![
                PaginationSkipped {
                    url: "/api/items?cursor=<next>".to_string(),
                    param: "cursor".to_string(),
                },
                PaginationSkipped {
                    url: "/api/walk?page=<opaque>".to_string(),
                    param: "page".to_string(),
                },
            ]
        );
        assert_eq!(opaque_query_template("/x?limit=25"), None);
        assert_eq!(opaque_query_template("/x"), None);
        assert_eq!(
            opaque_query_template("/x?after=:token").as_deref(),
            Some("after")
        );

        let (paths, skipped) = vendor_docs_get_paths(R6H_V3_DOCS);
        assert_eq!(
            paths,
            vec!["/v3/payments".to_string(), "/v3/reversals".to_string()]
        );
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].url, "/v3/payments?cursor=<next>");
        assert_eq!(skipped[0].param, "cursor");
    }

    /// VA-105, the excerpt alone: a body under the budget is untouched; one over it is cut after
    /// the last whole row under the budget (the kept text ends on a row's `}`, never inside a
    /// nested object or a key); a non-JSON body over the budget is cut raw and the marker says
    /// both that and that the body is not a JSON object.
    #[test]
    fn a_body_over_budget_is_cut_after_a_whole_object_and_the_marker_carries_its_facts() {
        let small = r#"{"data": [{"id": 1}], "total": 1}"#;
        let (kept, cut) = excerpt_body("http://v/x", small, VENDOR_PROBE_BODY_CHARS);
        assert_eq!(kept, small);
        assert!(cut.is_none());

        let page = payments_page();
        let (kept, cut) = excerpt_body("http://v/v3/payments", &page, VENDOR_PROBE_BODY_CHARS);
        let cut = cut.expect("over budget");
        assert!(
            kept.ends_with('}'),
            "{}",
            kept.chars()
                .skip(kept.chars().count().saturating_sub(80))
                .collect::<String>()
        );
        assert!(
            kept.ends_with(r#""country": "DE"}}"#),
            "the cut lands after a whole ROW (its nested counterparty closed too): {}",
            kept.chars()
                .skip(kept.chars().count().saturating_sub(80))
                .collect::<String>()
        );
        assert!(cut.kept <= VENDOR_PROBE_BODY_CHARS);
        assert_eq!(cut.kept, kept.chars().count());
        assert_eq!(cut.chars, page.chars().count());
        assert!(cut.at_object_boundary);
        assert_eq!((cut.chars, cut.kept), (28_840, 5_912));
        assert_eq!(
            kept.matches(r#""id""#).count(),
            41,
            "41 whole rows of 200 fit under the budget"
        );
        assert_eq!(json_row_array_key(&page).as_deref(), Some("data"));
        assert_eq!(json_row_array_key("<html>"), None);
        assert_eq!(json_row_array_key(r#"{"ok": true}"#), None);
        let marker = body_cut_marker(&cut, None, &page);
        assert!(marker.contains("top-level keys: data, limit, total; total=12288; 200 rows"));
        assert!(
            marker.contains("`curl -s http://v/v3/payments`"),
            "{marker}"
        );

        let html = format!("<html>{}</html>", "x".repeat(VENDOR_PROBE_BODY_CHARS + 50));
        let (kept, cut) = excerpt_body("http://v/docs", &html, VENDOR_PROBE_BODY_CHARS);
        let cut = cut.expect("over budget");
        assert_eq!(kept.chars().count(), VENDOR_PROBE_BODY_CHARS);
        assert!(!cut.at_object_boundary);
        let marker = body_cut_marker(&cut, Some("k"), &html);
        assert!(marker.contains("cut mid-text — no JSON object boundary under the budget"));
        assert!(marker.contains("The FULL body is not a JSON object"));
        assert!(marker.contains("-H 'Authorization: Bearer k'"));
    }

    /// P1-8: the docs-body GET parser — dedupe, template filling, param'd-path exclusion.
    #[test]
    fn vendor_docs_get_paths_dedupes_fills_templates_and_excludes_param_routes() {
        let docs = "GET /v3/payments returns a page. GET /v3/payments?limit=<int>&offset=<int> \
                    pages it. GET /v3/payments/{id} returns one. GET /v3/reversals lists reversals. \
                    GET /v3/payments is idempotent.";
        let (got, skipped) = vendor_docs_get_paths(docs);
        assert!(
            skipped.is_empty(),
            "count templates are never skipped: {skipped:?}"
        );
        assert!(got.contains(&"/v3/payments".to_string()), "{got:?}");
        assert!(
            got.contains(&"/v3/payments?limit=1&offset=1".to_string()),
            "a templated query is filled, not dropped: {got:?}"
        );
        assert!(got.contains(&"/v3/reversals".to_string()), "{got:?}");
        assert!(
            !got.iter().any(|p| p.contains("{id}")),
            "a param'd PATH cannot be probed blind: {got:?}"
        );
        assert_eq!(
            got.iter().filter(|p| *p == "/v3/payments").count(),
            1,
            "deduped: {got:?}"
        );
    }

    /// P1-12: the row-evidence readers are pinned — `total` outranks a page length (a page is
    /// bounded by `limit`; the total is the collection), a collection array counts when no total
    /// is documented, and a body that is not JSON or carries neither ABSTAINS (None), never
    /// invents a zero — a zero here becomes a repair finding, so an invented one would send a
    /// fixer at working code.
    #[test]
    fn row_evidence_reads_total_over_page_and_abstains_on_neither() {
        assert_eq!(
            json_rows_and_total(r#"{"data": [1, 2, 3], "total": 12288, "limit": 64}"#),
            (Some(12288), Some(3))
        );
        assert_eq!(
            json_rows_evidence(r#"{"data": [1, 2], "total": 12288}"#),
            Some(12288)
        );
        assert_eq!(json_rows_evidence(r#"{"events": [1]}"#), Some(1));
        assert_eq!(json_rows_evidence(r#"{"data": []}"#), Some(0));
        assert_eq!(json_rows_evidence(r#"{"status": "ok"}"#), None);
        assert_eq!(json_rows_evidence("<html>not json</html>"), None);
        // the persisted probe file round-trips through the gate-side reader
        let dir = tmp("vendor-probe");
        assert_eq!(read_vendor_probe_rows(&dir), None, "absent file abstains");
        std::fs::create_dir_all(dir.join(".swarm")).unwrap();
        std::fs::write(
            dir.join(".swarm/vendor-probe.json"),
            r#"{"ok": true, "vendor_total": 12288, "page1_rows": 64}"#,
        )
        .unwrap();
        assert_eq!(read_vendor_probe_rows(&dir), Some(12288));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
