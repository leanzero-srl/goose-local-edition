//! The advertised-POST probe's verdict vocabulary: what calling a mutating endpoint TWICE proves
//! (`repeated_post_verdict`), the curl status split it reads, and the evidence a finding carries
//! about the request the gate actually sent. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases); the enum, both functions and their
//! test moved verbatim from swarm.rs.

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

#[cfg(test)]
mod tests {
    use super::super::spec_post_endpoints;
    use super::*;

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
}
