//! Endpoint-literal attribution: which FILE an unassigned gate finding belongs to, by evidence.
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). `endpoint_literal_of` and
//! `attribute_gate_finding` moved verbatim from swarm.rs, except the possessive-apostrophe cut in
//! `clean` (r5: `/api/drafts's` kept its apostrophe, the tree grep hit zero files, and 6 of 8
//! round-0 findings misrouted to the entry file).

/// P1-3, half one: the ENDPOINT LITERAL a gate finding names, if any. The deterministic gate's
/// own emitters write `GET <path> returned <code>` / `POST <path> …`, so the token after an HTTP
/// verb is the highest-confidence literal; a backticked `/…` token is the fallback for prose
/// findings. A bare `/` is deliberately None — grepping a tree for "/" matches every file, which
/// is attribution-shaped noise, and the entry-file fallback answers that case honestly instead.
fn endpoint_literal_of(finding: &str) -> Option<String> {
    // r5 (run swarm-20260830-083847650): the gate's own templates write `POST {path}'s response
    // …`, so the raw token is `/api/drafts's`. `trim_matches` only trims at token ENDS — the
    // trailing `s` is alphanumeric, so the apostrophe survived, the tree grep hit zero files,
    // and the entry-file fallback misrouted 6 of 8 round-0 findings. CUT at the first
    // disallowed character instead (after trimming any disallowed lead): `/api/drafts's` →
    // `/api/drafts`, `/api/payments/<id>/note's` → `/api/payments/`.
    let clean = |t: &str| {
        t.trim_start_matches(|c: char| !(c.is_ascii_alphanumeric() || "/_-.".contains(c)))
            .split(|c: char| !(c.is_ascii_alphanumeric() || "/_-.".contains(c)))
            .next()
            .unwrap_or("")
            .to_string()
    };
    let mut toks = finding.split_whitespace().peekable();
    while let Some(t) = toks.next() {
        let verb = t.trim_matches(|c: char| !c.is_ascii_alphabetic());
        if matches!(verb, "GET" | "POST" | "PUT" | "DELETE" | "PATCH") {
            if let Some(path) = toks.peek() {
                let p = clean(path);
                if p.starts_with('/') && p.len() > 1 {
                    return Some(p);
                }
            }
        }
    }
    finding
        .split('`')
        .skip(1)
        .step_by(2)
        .map(clean)
        .find(|t| t.starts_with('/') && t.len() > 1)
}

/// P1-3, half two: attribute one UNASSIGNED gate finding to a file by EVIDENCE, never to a
/// whole-tree residue worker. (1) grep the tree for the endpoint literal the finding names —
/// the file that mentions `/api/payments` is the file that serves it or was supposed to; the
/// file with the most occurrences wins, ties to `all_files` order. (2) Else the service's ENTRY
/// file (`…/__main__.py`, preferring one whose package the finding names): an endpoint nothing
/// implements is a defect of the file that binds the port. (3) Else None — the caller ships the
/// finding as a KNOWN BUG event. WHY: r0's residue worker owned all eight files and took 115 of
/// a 138-minute wave (83%) while the four file-attributed shards finished in 24 minutes, all
/// promoted; r2's sink burned ~130 min on the same whole-tree shape. `read_source` is injected
/// so the fixture drives this against an archived tree READ-ONLY.
pub(super) fn attribute_gate_finding(
    finding: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let literal = endpoint_literal_of(finding);
    if let Some(lit) = &literal {
        // A DECLARED route outranks a CALL to it: `"/api/payments":` in the dispatcher is the
        // literal as a complete token, `"/api/payments?limit=100"` in the page is the literal
        // mid-URL. Boundary hits (next char ends the path) are counted first; raw substring
        // counts only break a no-boundary tie. Most hits wins; ties go to all_files order.
        let boundary_hits = |src: &str| {
            src.match_indices(lit.as_str())
                .filter(|(i, _)| {
                    src.get(i + lit.len()..)
                        .and_then(|rest| rest.chars().next())
                        .map(|c| !(c.is_ascii_alphanumeric() || "/_-.?=&%".contains(c)))
                        .unwrap_or(true)
                })
                .count()
        };
        let mut best: Option<(usize, usize, usize)> = None; // (boundary, raw, index)
        for (i, f) in all_files.iter().enumerate() {
            let Some(src) = read_source(f) else { continue };
            let raw = src.matches(lit.as_str()).count();
            if raw == 0 {
                continue;
            }
            let b = boundary_hits(&src);
            if best.map(|(bb, br, _)| (b, raw) > (bb, br)).unwrap_or(true) {
                best = Some((b, raw, i));
            }
        }
        if let Some((_, _, i)) = best {
            return Some(all_files[i].clone());
        }
    }
    // The entry-file fallback answers ENDPOINT- and BOOT-shaped findings only: an advertised
    // route nothing on disk mentions, or the gate's own boot-probe strings. Arbitrary prose must
    // NOT land on the entry file — that would re-create the residue worker one file at a time.
    let boot_shaped = {
        let l = finding.to_lowercase();
        [
            "python -m",
            "python3 -m",
            "never bound",
            "does not start",
            "did not start",
            "entry",
        ]
        .iter()
        .any(|m| l.contains(m))
    };
    if literal.is_none() && !boot_shaped {
        return None;
    }
    let entries: Vec<&String> = all_files
        .iter()
        .filter(|f| *f == "__main__.py" || f.ends_with("/__main__.py"))
        .collect();
    entries
        .iter()
        .find(|f| {
            f.rsplit('/')
                .nth(1)
                .map(|pkg| !pkg.is_empty() && finding.contains(pkg))
                .unwrap_or(false)
        })
        .or_else(|| entries.first())
        .map(|f| (*f).clone())
}

/// The render probe's own attribution: `/consoleErrors/sources/0` — an array parallel to
/// `texts`, each a server-relative path like `web/viz.js`, `""` when the browser could not name
/// a source. An ABSENT key is an old probe: None, finding text unchanged — degrade gracefully,
/// never an error. r5 F8: the console finding named NO file, so the ONE product-killing bug
/// (ReferenceError: onBrushChangeTracked is not defined, web/viz.js:1124) parked as known_bugs
/// while six contract nits got fix shards.
pub(super) fn console_error_source(v: &serde_json::Value) -> Option<&str> {
    v.pointer("/consoleErrors/sources/0")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::super::{attribute_findings, extract_file_from_finding};
    use super::*;

    /// P1-3: the endpoint literal a gate finding names, straight from the gate's own emitter
    /// shapes. A bare `/` is None on purpose — grepping a tree for "/" hits every file, and the
    /// entry-file fallback answers that case honestly.
    #[test]
    fn the_endpoint_literal_comes_from_the_verb_or_backticks_never_bare_slash() {
        assert_eq!(
            endpoint_literal_of(
                "GET /api/payments returned 404 — the spec advertises this endpoint but the app does not implement it"
            )
            .as_deref(),
            Some("/api/payments")
        );
        assert_eq!(
            endpoint_literal_of("POST /api/sync did not complete twice").as_deref(),
            Some("/api/sync")
        );
        assert_eq!(
            endpoint_literal_of("the advertised `/api/health` endpoint answers 500").as_deref(),
            Some("/api/health")
        );
        assert_eq!(endpoint_literal_of("GET / returned 404"), None);
        assert_eq!(endpoint_literal_of("no route named anywhere"), None);
        // r5: the gate's own possessive templates (`POST {path}'s response …`) — the literal is
        // cut at the apostrophe, never carried into the tree grep.
        assert_eq!(
            endpoint_literal_of(
                "POST /api/drafts's response does not carry the documented field(s) `amount_minor`, `currency`"
            )
            .as_deref(),
            Some("/api/drafts")
        );
        assert_eq!(
            endpoint_literal_of(
                "POST /api/webhooks/meridian's response could not be read as JSON on either probe"
            )
            .as_deref(),
            Some("/api/webhooks/meridian")
        );
        assert_eq!(
            endpoint_literal_of(
                "POST /api/payments/<id>/note's response does not carry the documented field(s) `ok`"
            )
            .as_deref(),
            Some("/api/payments/")
        );
    }

    /// P1-3: attribution by EVIDENCE. (1) the file whose source carries the endpoint literal;
    /// (2) else the service's entry file, preferring the package the finding names; (3) else
    /// None — the caller ships it as a known bug.
    #[test]
    fn an_unassigned_finding_attributes_by_grep_then_entry_file_then_known_bug() {
        let all = vec![
            "vendorsync/web/index.html".to_string(),
            "vendorsync/api.py".to_string(),
            "vendorsync/__main__.py".to_string(),
        ];
        let read = |f: &str| -> Option<String> {
            match f {
                "vendorsync/api.py" => Some(
                    "if path == \"/api/payments\": ...\nif path == \"/api/payments\": ...".into(),
                ),
                "vendorsync/web/index.html" => Some("fetch(\"/api/payments?limit=100\")".into()),
                _ => Some(String::new()),
            }
        };
        // Most occurrences of the literal wins (the server names its route more than the page
        // that calls it); ties go to all_files order.
        assert_eq!(
            attribute_gate_finding("GET /api/payments returned 404", &all, &read).as_deref(),
            Some("vendorsync/api.py")
        );
        // No literal -> the entry file, and the package the finding names picks between entries.
        assert_eq!(
            attribute_gate_finding(
                "spec-contract: `python3 -m vendorsync` never bound port 8850 within 4s",
                &all,
                &read
            )
            .as_deref(),
            Some("vendorsync/__main__.py")
        );
        // Nothing greps, no entry file in the plan -> None: a known bug, not a residue task.
        let no_entry = vec!["a.py".to_string()];
        assert_eq!(
            attribute_gate_finding("something nobody can place", &no_entry, &|_| None),
            None
        );
        // r5 VERBATIM (run swarm-20260830-083847650, REPAIR round 0, F4): the possessive
        // template. Under the old end-trimming clean the literal stayed `/api/drafts's`, the
        // grep hit zero files, and the finding fell to the entry file — 6 of 8 round-0 findings
        // misrouted to app/__main__.py while app/httpapi.py held the route.
        let r5_all = vec!["app/__main__.py".to_string(), "app/httpapi.py".to_string()];
        let r5_read = |f: &str| -> Option<String> {
            match f {
                "app/httpapi.py" => Some("if path == \"/api/drafts\": ...".into()),
                _ => Some(String::new()),
            }
        };
        assert_eq!(
            attribute_gate_finding(
                "POST /api/drafts's response does not carry the documented field(s) \
                 `amount_minor`, `currency` — the spec's endpoint table names them for exactly \
                 this endpoint.",
                &r5_all,
                &r5_read
            )
            .as_deref(),
            Some("app/httpapi.py"),
            "the possessive apostrophe must never reach the tree grep"
        );
        // And the shared seam merges an attributed finding into its file's existing group.
        let findings = vec![
            "vendorsync/api.py:12: wrong key".to_string(),
            "GET /api/payments returned 404".to_string(),
            "cosmic ray".to_string(),
        ];
        let (groups, known) = attribute_findings(&findings, &all, &read);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file, "vendorsync/api.py");
        assert_eq!(
            groups[0].findings.len(),
            2,
            "the 404 joined its file's shard"
        );
        assert_eq!(known, ["cosmic ray"]);
    }

    /// P1-3 fixture: r0's real finding shapes against the ARCHIVED r0 tree, read-only. The tree
    /// (evals/swarm-bench/runs/build/swarm-3node-r0 — the vendorsync app) is machine-local and
    /// gitignored, so the test SKIPS loudly when it is absent rather than asserting on nothing.
    #[test]
    fn r0s_real_findings_attribute_against_the_archived_tree_read_only() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evals/swarm-bench/runs/build/swarm-3node-r0");
        if !root.join("vendorsync/api.py").exists() {
            eprintln!(
                "SKIP: archived r0 tree not on this machine ({})",
                root.display()
            );
            return;
        }
        let all = vec![
            "vendorsync/web/index.html".to_string(),
            "vendorsync/api.py".to_string(),
            "vendorsync/store.py".to_string(),
            "vendorsync/meridian.py".to_string(),
            "vendorsync/__main__.py".to_string(),
        ];
        let before: Vec<(String, std::time::SystemTime)> = all
            .iter()
            .filter_map(|f| {
                std::fs::metadata(root.join(f))
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| (f.clone(), t))
            })
            .collect();
        let read = |f: &str| std::fs::read_to_string(root.join(f)).ok();
        // The gate's own emitter shape for an unimplemented advertised endpoint: the literal
        // lives in api.py (the route dispatcher), so the finding shards there.
        assert_eq!(
            attribute_gate_finding(
                "GET /api/payments returned 404 — the spec advertises this endpoint but the app does not implement it",
                &all,
                &read
            )
            .as_deref(),
            Some("vendorsync/api.py")
        );
        // The boot-shaped finding names the package, not a path: the entry file owns it.
        assert_eq!(
            attribute_gate_finding(
                "spec-contract: `python3 -m vendorsync` never bound port 8081 within 4s — the advertised entrypoint did not start a server",
                &all,
                &read
            )
            .as_deref(),
            Some("vendorsync/__main__.py")
        );
        // READ-ONLY: attribution must not touch the archive.
        for (f, t0) in before {
            let t1 = std::fs::metadata(root.join(&f))
                .and_then(|m| m.modified())
                .unwrap();
            assert_eq!(t0, t1, "attribution modified the archived tree: {f}");
        }
    }

    /// r5 F8: the render gate's console finding named NO file by construction, so the ONE
    /// product-killing bug of the round (ReferenceError: onBrushChangeTracked is not defined —
    /// web/viz.js:1124) parked as known_bugs while six contract nits got fix shards. With the
    /// probe's `sources` the branch appends the attribution-list suffix ` (in \`{src}\`)` as the
    /// FINAL characters — the exact shape `extract_file_from_finding` parses — and the finding
    /// shards to the named file (.js is in FINDING_PATH_EXTS since the F862 broadening).
    #[test]
    fn a_console_finding_with_a_source_suffix_attributes_to_that_file() {
        let files = vec!["web/viz.js".to_string(), "app/__main__.py".to_string()];
        let finding = "the page renders but the browser console carries 1 error(s) in normal use \
                       (first: ReferenceError: onBrushChangeTracked is not defined) — fix the JS \
                       errors; users hit them as broken interactions. (in `web/viz.js`)";
        assert_eq!(
            extract_file_from_finding(finding, &files).as_deref(),
            Some("web/viz.js")
        );
        // The probe contract: sources parallel to texts; "" and an absent key both mean unknown.
        let v = serde_json::json!({"consoleErrors": {"count": 1,
            "texts": ["ReferenceError: onBrushChangeTracked is not defined"],
            "sources": ["web/viz.js"]}});
        assert_eq!(console_error_source(&v), Some("web/viz.js"));
        let empty =
            serde_json::json!({"consoleErrors": {"count": 1, "texts": ["x"], "sources": [""]}});
        assert_eq!(console_error_source(&empty), None);
        let old_probe = serde_json::json!({"consoleErrors": {"count": 1, "texts": ["x"]}});
        assert_eq!(
            console_error_source(&old_probe),
            None,
            "absent means old probe, never an error"
        );
    }
}
