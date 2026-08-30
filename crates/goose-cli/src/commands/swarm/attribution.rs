//! Endpoint-literal attribution: which FILE an unassigned gate finding belongs to, by evidence.
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). `endpoint_literal_of` (since renamed
//! `endpoint_literal_forms_of` — it now derives the verbatim placeholder form beside the prefix
//! cut) and `attribute_gate_finding` moved verbatim from swarm.rs, except the
//! possessive-apostrophe cut in `clean` (r5: `/api/drafts's` kept its apostrophe, the tree grep
//! hit zero files, and 6 of 8 round-0 findings misrouted to the entry file).

use super::findings::FileGroup;

/// P1-3, half one: the ENDPOINT LITERAL FORMS a gate finding names, most specific first. The
/// deterministic gate's own emitters write `GET <path> returned <code>` / `POST <path> …`, so
/// the token after an HTTP verb is the highest-confidence literal; a backticked `/…` token is
/// the fallback for prose findings. A bare `/` is deliberately absent — grepping a tree for "/"
/// matches every file, which is attribution-shaped noise, and the entry-file fallback answers
/// that case honestly instead.
///
/// TWO forms per finding, deduped, verbatim first (r5, run swarm-20260830-083847650: the
/// placeholder routes). The gate probes placeholder routes verbatim — `POST
/// /api/payments/<id>/note's response …` — and the r5 tree holds that literal IN CODE
/// (app/ledgerd/__init__.py's route table, `("POST", "/api/payments/<id>/note")`). Cutting at
/// `<` reduced every such finding to its prefix (`/api/payments/`), which structurally favors
/// whichever file mentions the prefix most — F5-F7's `/api/drafts/` even pooled THREE different
/// routes' hits into one count. So: the VERBATIM form keeps `<>` and cuts only at the
/// apostrophe class (any non-path, non-placeholder char); the PREFIX form is the old cut at
/// `<`. The caller tries verbatim first and falls back when it greps zero files.
fn endpoint_literal_forms_of(finding: &str) -> Vec<String> {
    // r5: the gate's own templates write `POST {path}'s response …`, so the raw token is
    // `/api/drafts's`. `trim_matches` only trims at token ENDS — the trailing `s` is
    // alphanumeric, so the apostrophe survived, the tree grep hit zero files, and the
    // entry-file fallback misrouted 6 of 8 round-0 findings. CUT at the first disallowed
    // character instead (after trimming any disallowed lead): `/api/drafts's` → `/api/drafts`,
    // `/api/payments/<id>/note's` → `/api/payments/<id>/note` (verbatim) / `/api/payments/`
    // (prefix).
    let form = |verbatim: bool| -> Option<String> {
        let ok = |c: char| {
            c.is_ascii_alphanumeric() || "/_-.".contains(c) || (verbatim && "<>".contains(c))
        };
        let clean = |t: &str| {
            t.trim_start_matches(|c: char| !ok(c))
                .split(|c: char| !ok(c))
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
    };
    let mut forms: Vec<String> = Vec::new();
    for lit in [form(true), form(false)].into_iter().flatten() {
        if !forms.contains(&lit) {
            forms.push(lit);
        }
    }
    forms
}

/// Comment text never counts as attribution EVIDENCE (r5, run swarm-20260830-083847650, REPAIR
/// round 0: web/app.js's only `/api/drafts` hits — 2 boundary, 3 raw — sat in its doc-comment
/// endpoint inventory at lines 41-42 and came ONE RANK from winning F4's shard over the file
/// that declares the route). `.py`: `#` to end-of-line; the js/ts family: `//` to EOL and
/// `/* */` blocks. A cheap scanner, deliberately: a `#` or `//` inside a string literal
/// (`"http://…"`) mildly over-strips the rest of that line, which is acceptable for EVIDENCE
/// COUNTING — the cost is a slightly lower count inside a ranking, never a refusal and never a
/// lost finding. Python docstrings survive (they are string literals — and r5's F1 fix, the one
/// promotion of round 0, rode a docstring mention in app/sync.py). Block comments become one
/// space so stripping can never JOIN two halves of a line into a match that was not there.
/// Other extensions pass through unchanged.
fn strip_comments_for_evidence(src: &str, file: &str) -> String {
    if file.ends_with(".py") {
        src.lines()
            .map(|l| l.split('#').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    } else if [".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"]
        .iter()
        .any(|e| file.ends_with(e))
    {
        let mut out = String::with_capacity(src.len());
        let mut it = src.chars().peekable();
        while let Some(c) = it.next() {
            if c == '/' && it.peek() == Some(&'/') {
                for d in it.by_ref() {
                    if d == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            } else if c == '/' && it.peek() == Some(&'*') {
                it.next();
                let mut prev = ' ';
                for d in it.by_ref() {
                    if prev == '*' && d == '/' {
                        break;
                    }
                    prev = d;
                }
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out
    } else {
        src.to_string()
    }
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
///
/// Returns `(winner, runner_up)`. The RUNNER-UP is the second-best candidate by the same
/// ordering, surfaced only when it is a SOURCE file with at least one comment-stripped boundary
/// hit — a finding whose evidence reconciles across two files (route table vs handler body) must
/// let the shard own both, so whichever side the worker fixes can land (the js/css↔html
/// reconciliation precedent at `shard_owned_files`). Grouping stays by winner; only ownership
/// may widen, and only through the caller's `resolve_shard_ownership` claim pass.
pub(super) fn attribute_gate_finding_ranked(
    finding: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Option<(String, Option<String>)> {
    let literals = endpoint_literal_forms_of(finding);
    for lit in &literals {
        // Forms are tried most-specific first: the VERBATIM placeholder route (`/api/payments/
        // <id>/note` — real code in r5's route table) before its prefix cut, falling through
        // ONLY when a form greps zero files' stripped source. Within each form the ordering and
        // tiebreaks are unchanged. (`<` after the prefix form is a boundary char below — a
        // route table's `/api/drafts/<id>/…` entries still boundary-count for `/api/drafts/`.)
        //
        // A DECLARED route outranks a CALL to it: `"/api/payments":` in the dispatcher is the
        // literal as a complete token, `"/api/payments?limit=100"` in the page is the literal
        // mid-URL. Boundary hits (next char ends the path) are counted first; raw substring
        // counts only break a no-boundary tie. Most hits wins; ties go to all_files order.
        // Counts are taken over COMMENT-STRIPPED source (a file whose only mentions are
        // comments exits the candidate set entirely — r5's web/app.js); an exact stripped tie
        // falls back to the UNSTRIPPED counts before file order, because r5's F1 proved a
        // docstring/comment mention can still be the honest discriminator between files that
        // each carry one code hit (app/sync.py, the round's one landed fix, ties app/httpapi.py
        // and app/ledgerd/__init__.py at (1,1) stripped and wins only on the raw counts).
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
        type RankKey = (usize, usize, usize, usize); // (stripped b, stripped raw, raw b, raw raw)
                                                     // A DATA OR DOC FILE NEVER OUTRANKS SOURCE — extract_file_from_finding's take()-side rule
                                                     // (swarm.rs, same wording), which the grep side never got: the WINNER slot had no
                                                     // source-ext filter, only the runner-up did, so a README.md spelling an endpoint often
                                                     // enough would take the shard and aim a code fix at documentation. `best`/`second` now
                                                     // rank SOURCE-ext candidates only (existing RankKey ordering unchanged within the class);
                                                     // a non-source candidate wins only when NO source-ext candidate greps a nonzero stripped
                                                     // count — FINDING_PATH_EXTS deliberately admits .md/.json so a finding ABOUT those files
                                                     // stays attributable, and that case still lands on them.
        let is_source = |f: &str| super::FINDING_SOURCE_EXTS.iter().any(|e| f.ends_with(e));
        let mut best: Option<(RankKey, usize)> = None;
        let mut second: Option<(RankKey, usize)> = None;
        let mut best_other: Option<(RankKey, usize)> = None;
        for (i, f) in all_files.iter().enumerate() {
            let Some(full) = read_source(f) else { continue };
            let src = strip_comments_for_evidence(&full, f);
            let stripped_raw = src.matches(lit.as_str()).count();
            if stripped_raw == 0 {
                continue;
            }
            let key: RankKey = (
                boundary_hits(&src),
                stripped_raw,
                boundary_hits(&full),
                full.matches(lit.as_str()).count(),
            );
            if !is_source(f) {
                if best_other.map(|(bk, _)| key > bk).unwrap_or(true) {
                    best_other = Some((key, i));
                }
                continue;
            }
            if best.map(|(bk, _)| key > bk).unwrap_or(true) {
                second = best;
                best = Some((key, i));
            } else if second.map(|(sk, _)| key > sk).unwrap_or(true) {
                second = Some((key, i));
            }
        }
        if let Some((_, i)) = best.or(best_other) {
            let runner_up = second
                .filter(|((sb, _, _, _), _)| *sb >= 1)
                .map(|(_, j)| all_files[j].clone())
                .filter(|f| super::FINDING_SOURCE_EXTS.iter().any(|e| f.ends_with(e)));
            return Some((all_files[i].clone(), runner_up));
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
    if literals.is_empty() && !boot_shaped {
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
        .map(|f| ((*f).clone(), None))
}

/// The winner alone, for tests that assert grouping without ownership.
#[cfg(test)]
fn attribute_gate_finding(
    finding: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    attribute_gate_finding_ranked(finding, all_files, read_source).map(|(w, _)| w)
}

/// P1-3, the seam every repair path shares: `group_findings_by_file`, then evidence-based
/// attribution for what it could not place. Attributed findings JOIN their file's shard (or open
/// one); what remains is the KNOWN-BUGS list — the caller emits it as an event and dispatches
/// no whole-tree residue worker for it. The third return is the winner→runner-up map (first
/// attributed finding with a runner-up sets its group's entry): candidate co-ownership only —
/// nothing is owned until `resolve_shard_ownership`'s claim pass, so grouping is unchanged.
pub(super) fn attribute_findings(
    findings: &[String],
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> (
    Vec<FileGroup>,
    Vec<String>,
    std::collections::HashMap<String, String>,
) {
    let (mut groups, unassigned) = super::findings::group_findings_by_file(findings, all_files);
    let mut known_bugs: Vec<String> = Vec::new();
    let mut runner_ups: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for f in unassigned {
        match attribute_gate_finding_ranked(&f, all_files, read_source) {
            Some((file, ru)) => {
                if let Some(ru) = ru {
                    runner_ups.entry(file.clone()).or_insert(ru);
                }
                match groups.iter_mut().find(|g| g.file == file) {
                    Some(g) => g.findings.push(f),
                    None => groups.push(FileGroup {
                        file,
                        findings: vec![f],
                    }),
                }
            }
            None => known_bugs.push(f),
        }
    }
    (groups, known_bugs, runner_ups)
}

/// Fix-1 seam, the CLAIM pass: each shard's owned files, resolved SEQUENTIALLY in group order
/// BEFORE the concurrent fan. `shard_owned_files`' pairings (test↔module, js/css↔html) and the
/// attribution runner-up claim into `taken` as they land, so the partition the promotes copy
/// stays DISJOINT by construction — first shard claims, later shards stay narrower (two
/// promotes must never touch one real dst). This also closes the latent pairing race: two .js
/// groups in one directory could each pair the same .html when every closure consulted only the
/// group-file set. MILD throughout — ownership only ever widens; nothing is refused.
pub(super) fn resolve_shard_ownership(
    groups: &[FileGroup],
    all_files: &[String],
    runner_ups: &std::collections::HashMap<String, String>,
) -> Vec<Vec<String>> {
    let mut taken: std::collections::HashSet<String> =
        groups.iter().map(|g| g.file.clone()).collect();
    groups
        .iter()
        .map(|g| {
            let mut owned = super::shard_owned_files(&g.file, all_files, &taken);
            if let Some(ru) = runner_ups.get(&g.file) {
                if !taken.contains(ru) && !owned.contains(ru) {
                    owned.push(ru.clone());
                }
            }
            for f in owned.iter().skip(1) {
                taken.insert(f.clone());
            }
            owned
        })
        .collect()
}

/// The render probe's own attribution: `/consoleErrors/sources` — an array parallel to
/// `texts`, each a server-relative path like `web/viz.js`, `""` when the browser could not name
/// a source. Scans the first THREE pairs (the probe records at most three texts) and returns the
/// first index whose text AND source are both non-empty, as `(i, text, source)` — the honest
/// PAIR, so a caller can never attach error 2's file to error 1's text (a first error with `""`
/// source must not suppress an attributable error 2 behind it). An ABSENT key is an old probe:
/// None, finding text unchanged — degrade gracefully, never an error. r5 F8: the console finding
/// named NO file, so the ONE product-killing bug (ReferenceError: onBrushChangeTracked is not
/// defined, web/viz.js:1124) parked as known_bugs while six contract nits got fix shards.
pub(super) fn console_error_source(v: &serde_json::Value) -> Option<(usize, &str, &str)> {
    (0..3).find_map(|i| {
        let text = v.pointer(&format!("/consoleErrors/texts/{i}"))?.as_str()?;
        let src = v
            .pointer(&format!("/consoleErrors/sources/{i}"))?
            .as_str()?;
        (!text.is_empty() && !src.is_empty()).then_some((i, text, src))
    })
}

#[cfg(test)]
mod tests {
    use super::super::findings::extract_file_from_finding;
    use super::*;

    /// P1-3: the endpoint literal a gate finding names, straight from the gate's own emitter
    /// shapes. A bare `/` yields no forms on purpose — grepping a tree for "/" hits every file,
    /// and the entry-file fallback answers that case honestly.
    #[test]
    fn the_endpoint_literal_comes_from_the_verb_or_backticks_never_bare_slash() {
        // A finding without a placeholder has ONE form (verbatim == prefix, deduped).
        let one = |s: &str| {
            let v = endpoint_literal_forms_of(s);
            assert!(v.len() <= 1, "expected at most one form for {s:?}: {v:?}");
            v.into_iter().next()
        };
        assert_eq!(
            one(
                "GET /api/payments returned 404 — the spec advertises this endpoint but the app does not implement it"
            )
            .as_deref(),
            Some("/api/payments")
        );
        assert_eq!(
            one("POST /api/sync did not complete twice").as_deref(),
            Some("/api/sync")
        );
        assert_eq!(
            one("the advertised `/api/health` endpoint answers 500").as_deref(),
            Some("/api/health")
        );
        assert_eq!(one("GET / returned 404"), None);
        assert_eq!(one("no route named anywhere"), None);
        // r5: the gate's own possessive templates (`POST {path}'s response …`) — the literal is
        // cut at the apostrophe, never carried into the tree grep.
        assert_eq!(
            one(
                "POST /api/drafts's response does not carry the documented field(s) `amount_minor`, `currency`"
            )
            .as_deref(),
            Some("/api/drafts")
        );
        assert_eq!(
            one("POST /api/webhooks/meridian's response could not be read as JSON on either probe")
                .as_deref(),
            Some("/api/webhooks/meridian")
        );
        // A placeholder route yields TWO forms: the verbatim literal (the shape r5's route
        // table holds in real code) first, its prefix cut second.
        assert_eq!(
            endpoint_literal_forms_of(
                "POST /api/payments/<id>/note's response does not carry the documented field(s) `ok`"
            ),
            vec![
                "/api/payments/<id>/note".to_string(),
                "/api/payments/".to_string()
            ]
        );
    }

    /// Fix 2, the r5 live-tree shape (run swarm-20260830-083847650, REPAIR round 0, F2):
    /// `POST /api/payments/<id>/note's response …` — the verbatim placeholder literal exists IN
    /// CODE (app/ledgerd/__init__.py:63, `("POST", "/api/payments/<id>/note")`), so the
    /// verbatim form greps the route table directly instead of pooling prefix hits across
    /// unrelated routes. The prefix form stays as the fallback for a placeholder route no file
    /// spells out verbatim.
    #[test]
    fn a_placeholder_route_greps_verbatim_before_the_prefix_cut() {
        let f2 = "POST /api/payments/<id>/note's response does not carry the documented \
                  field(s) `id`, `note`, `version` — the spec's endpoint table names them for \
                  exactly this endpoint.";
        let all = vec![
            "app/ledgerd/__init__.py".to_string(),
            "web/app.js".to_string(),
        ];
        // The live tree's shapes verbatim: the route table declares the literal in code;
        // app.js holds it only in its doc comment (stripped — never evidence).
        let read = |f: &str| -> Option<String> {
            match f {
                "app/ledgerd/__init__.py" => Some(
                    "    (\"GET\", \"/api/payments/<id>\"),\n\
                     \x20   (\"POST\", \"/api/payments/<id>/note\"),\n"
                        .into(),
                ),
                "web/app.js" => {
                    Some("/*\n * POST /api/payments/<id>/note — body {note, version}\n */\n".into())
                }
                _ => None,
            }
        };
        assert_eq!(
            attribute_gate_finding(f2, &all, &read).as_deref(),
            Some("app/ledgerd/__init__.py"),
            "the verbatim literal lands on the route table that declares it"
        );
        // The structural half the prefix cut got wrong: a CALLER spelling the prefix many
        // times out-counts the declaring route table under the prefix form; the verbatim form
        // pins the declaration. (On the live r5 tree no file does this — every round-0 winner
        // is unchanged, so this half ships as a NET.)
        let read2 = |f: &str| -> Option<String> {
            match f {
                "app/ledgerd/__init__.py" => {
                    Some("    (\"POST\", \"/api/payments/<id>/note\"),\n".into())
                }
                "web/app.js" => Some(
                    "fetch(\"/api/payments/\" + id + \"/note\");\n\
                     fetch(\"/api/payments/\" + id + \"/note\", opts);\n\
                     fetch(\"/api/payments/\" + id);\n"
                        .into(),
                ),
                _ => None,
            }
        };
        assert_eq!(
            attribute_gate_finding(f2, &all, &read2).as_deref(),
            Some("app/ledgerd/__init__.py")
        );
        // And the fallback: a placeholder route NO file spells verbatim still attributes by
        // its prefix instead of dying to the entry-file arm.
        let read3 = |f: &str| -> Option<String> {
            (f == "web/app.js").then(|| "fetch(\"/api/payments/\" + id + \"/note\");\n".to_string())
        };
        assert_eq!(
            attribute_gate_finding(f2, &all, &read3).as_deref(),
            Some("web/app.js"),
            "zero verbatim greps fall back to the prefix form"
        );
    }

    /// A DATA OR DOC FILE NEVER OUTRANKS SOURCE — the grep side of the rule
    /// `extract_file_from_finding` has carried on the take() side since the FINDING_PATH_EXTS
    /// broadening. The r5 manifest ships README.md and DECISIONS.md beside the code (measured:
    /// both in the run's task_owns), so a README spelling an endpoint often enough used to take
    /// the winner slot and aim a code fix shard at documentation.
    #[test]
    fn a_doc_file_never_outgreps_source_for_the_winner_slot() {
        let all = vec!["README.md".to_string(), "app/httpapi.py".to_string()];
        // README out-greps the handler 3 boundary hits to 1 — the handler still wins.
        let read = |f: &str| -> Option<String> {
            match f {
                "README.md" => Some(
                    "## API\nPOST /api/sync\nPOST /api/sync twice is cheap\ncurl -X POST /api/sync\n"
                        .into(),
                ),
                "app/httpapi.py" => Some("        if path == \"/api/sync\":\n".into()),
                _ => None,
            }
        };
        assert_eq!(
            attribute_gate_finding("POST /api/sync is not CHEAP on a repeat run", &all, &read)
                .as_deref(),
            Some("app/httpapi.py"),
            "a non-source file must not WIN attribution while any source candidate greps"
        );
        // A finding whose ONLY greppable candidate is the doc file still lands on it —
        // FINDING_PATH_EXTS admits .md/.json precisely so findings ABOUT those files stay
        // attributable, and this arm keeps that alive.
        let read_docs_only = |f: &str| -> Option<String> {
            (f == "README.md").then(|| "POST /api/sync — documented here only\n".to_string())
        };
        assert_eq!(
            attribute_gate_finding(
                "POST /api/sync is not CHEAP on a repeat run",
                &all,
                &read_docs_only
            )
            .as_deref(),
            Some("README.md"),
            "no source candidate greps: the doc file may win, never silently drop to known bugs"
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
        let (groups, known, _) = attribute_findings(&findings, &all, &read);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file, "vendorsync/api.py");
        assert_eq!(
            groups[0].findings.len(),
            2,
            "the 404 joined its file's shard"
        );
        assert_eq!(known, ["cosmic ray"]);
    }

    /// Fix 2, the r5 receipt VERBATIM (run swarm-20260830-083847650, REPAIR round 0, F4):
    /// web/app.js's only `/api/drafts` mentions are its doc-comment endpoint inventory (lines
    /// 41-42) — 2 boundary + 3 raw, ONE RANK from beating app/httpapi.py's (2,2). Comment
    /// stripping drops it to 0 and it exits the candidate set entirely; a tree holding ONLY the
    /// comment-carrying file attributes nowhere (known bug), never to the comment.
    #[test]
    fn comment_hits_never_count_as_attribution_evidence() {
        let f4 = "POST /api/drafts's response does not carry the documented field(s) \
                  `amount_minor`, `currency`, `counterparty`, `name`, `country`, `note` — the \
                  spec's endpoint table names them for exactly this endpoint.";
        let app_js = "/* ============================================================================\n\
                      \x20*   Drafts — every call carries Authorization: Bearer <#role-token>:\n\
                      \x20*     POST /api/drafts {amount_minor, currency, counterparty:{name,country},\n\
                      \x20*     note}; GET /api/drafts; POST /api/drafts/<id>/{submit,approve,reject}.\n\
                      \x20* ============================================================================\n\
                      \x20*/\n(function () {\n  \"use strict\";\n})();\n";
        let only_comments = vec!["web/app.js".to_string()];
        let read_js = |f: &str| (f == "web/app.js").then(|| app_js.to_string());
        assert_eq!(
            attribute_gate_finding(f4, &only_comments, &read_js),
            None,
            "a file whose only mentions are comments must exit the candidate set"
        );
        // And the r5 F1 shape proves the tiebreak half: after stripping, app/sync.py's `#`
        // comment hit dies but its DOCSTRING hit survives, tying all three files at (1,1)
        // stripped — the raw counts (sync 2-2 vs 1-1) must keep the winner at app/sync.py, the
        // file round 0's one landed promotion actually fixed, instead of file order handing F1
        // to app/httpapi.py.
        let f1 = "POST /api/sync is not CHEAP on a repeat run — the second sync re-fetched \
                  12290 row(s) it already had.";
        let all = vec![
            "app/httpapi.py".to_string(),
            "app/ledgerd/__init__.py".to_string(),
            "app/sync.py".to_string(),
        ];
        let read = |f: &str| -> Option<String> {
            match f {
                "app/sync.py" => Some(
                    "_SYNC_LOCK = threading.Lock()   # one walk at a time (POST /api/sync + boot loop)\n\
                     def start_boot_sync():\n\
                     \x20   \"\"\"Kick the boot walk.\n\
                     \x20   once it returns. Later syncs are on-demand (POST /api/sync).\"\"\"\n"
                        .into(),
                ),
                "app/httpapi.py" => Some("            if path == \"/api/sync\":\n".into()),
                "app/ledgerd/__init__.py" => Some("    (\"POST\", \"/api/sync\"),\n".into()),
                _ => None,
            }
        };
        assert_eq!(
            attribute_gate_finding_ranked(f1, &all, &read),
            Some((
                "app/sync.py".to_string(),
                Some("app/httpapi.py".to_string())
            )),
            "an exact stripped tie falls back to the unstripped counts before file order"
        );
    }

    /// Fix 1, r5's F4 verbatim: the finding's evidence reconciles across the route table
    /// (app/ledgerd/__init__.py, 5 raw — the winner and the group) and the handler file
    /// (app/httpapi.py, the runner-up) where the response-field fix actually belongs. The shard
    /// owns BOTH when the runner-up is unclaimed, so whichever side the worker fixes can land;
    /// a runner-up already claimed (the r5 round-1 shape: httpapi.py is F3's own group) leaves
    /// the shard single-file — the promoted partition stays disjoint.
    #[test]
    fn an_endpoint_shard_owns_winner_and_unclaimed_runner_up() {
        let f4 = "POST /api/drafts's response does not carry the documented field(s) \
                  `amount_minor`, `currency`, `counterparty`, `name`, `country`, `note` — the \
                  spec's endpoint table names them for exactly this endpoint.";
        let f3 = "POST /api/webhooks/meridian's response could not be read as JSON on either \
                  probe — the spec documents a JSON response for every endpoint.";
        let all = vec![
            "app/httpapi.py".to_string(),
            "app/ledgerd/__init__.py".to_string(),
            "web/app.js".to_string(),
        ];
        let read = |f: &str| -> Option<String> {
            match f {
                "app/ledgerd/__init__.py" => Some(
                    "    (\"POST\", \"/api/drafts\"),\n\
                     \x20   (\"POST\", \"/api/drafts/<id>/submit\"),\n\
                     \x20   (\"POST\", \"/api/drafts/<id>/approve\"),\n\
                     \x20   (\"POST\", \"/api/drafts/<id>/reject\"),\n\
                     \x20   (\"GET\", \"/api/drafts\"),\n\
                     \x20   (\"POST\", \"/api/webhooks/meridian\"),\n"
                        .into(),
                ),
                "app/httpapi.py" => Some(
                    "            if path == \"/api/drafts\":\n\
                     \x20           if path == \"/api/drafts\":\n\
                     \x20           if path == \"/api/webhooks/meridian\":\n\
                     \x20           if path == \"/api/webhooks/meridian\":\n"
                        .into(),
                ),
                // The r5 doc comment: raw hits that must not resurrect app.js as a runner-up.
                "web/app.js" => Some(
                    "/*\n * POST /api/drafts {amount_minor,\n * note}; GET /api/drafts; POST /api/drafts/<id>/x.\n */\n"
                        .into(),
                ),
                _ => None,
            }
        };
        assert_eq!(
            attribute_gate_finding_ranked(f4, &all, &read),
            Some((
                "app/ledgerd/__init__.py".to_string(),
                Some("app/httpapi.py".to_string())
            ))
        );
        // UNCLAIMED runner-up (F4 alone): the shard owns winner AND runner-up.
        let (groups, known, runner_ups) = attribute_findings(&[f4.to_string()], &all, &read);
        assert!(known.is_empty());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file, "app/ledgerd/__init__.py");
        assert_eq!(
            runner_ups
                .get("app/ledgerd/__init__.py")
                .map(|s| s.as_str()),
            Some("app/httpapi.py")
        );
        let owned = resolve_shard_ownership(&groups, &all, &runner_ups);
        assert_eq!(
            owned,
            vec![vec![
                "app/ledgerd/__init__.py".to_string(),
                "app/httpapi.py".to_string()
            ]],
            "the shard owns both, so the handler-body fix can land"
        );
        // TAKEN runner-up (F4 + F3, the r5 round-1 shape): httpapi.py is F3's own group, so the
        // ledgerd shard stays single-file — first shard claims, later shards don't.
        let (groups2, _, runner_ups2) =
            attribute_findings(&[f4.to_string(), f3.to_string()], &all, &read);
        let files: Vec<&str> = groups2.iter().map(|g| g.file.as_str()).collect();
        assert_eq!(files, ["app/ledgerd/__init__.py", "app/httpapi.py"]);
        let owned2 = resolve_shard_ownership(&groups2, &all, &runner_ups2);
        assert_eq!(owned2[0], vec!["app/ledgerd/__init__.py".to_string()]);
        assert_eq!(owned2[1], vec!["app/httpapi.py".to_string()]);
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
        assert_eq!(
            console_error_source(&v),
            Some((
                0,
                "ReferenceError: onBrushChangeTracked is not defined",
                "web/viz.js"
            ))
        );
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

    /// The suffix scans all three PAIRS honestly: a first error the browser could not source
    /// (`""`) must not suppress an attributable error behind it, and the pair returned is the
    /// pair — error 2's file can never ride error 1's text. All-empty sources: None, so the
    /// finding text stays exactly as before.
    #[test]
    fn the_console_suffix_scans_all_three_pairs_honestly() {
        let v = serde_json::json!({"consoleErrors": {"count": 3,
            "texts": ["Uncaught SyntaxError: unexpected token",
                      "ReferenceError: drawBrush is not defined",
                      "TypeError: x is undefined"],
            "sources": ["", "web/viz.js", ""]}});
        assert_eq!(
            console_error_source(&v),
            Some((1, "ReferenceError: drawBrush is not defined", "web/viz.js")),
            "the exemplar must be texts/1 — the text web/viz.js actually produced"
        );
        let none = serde_json::json!({"consoleErrors": {"count": 2,
            "texts": ["a", "b"], "sources": ["", ""]}});
        assert_eq!(console_error_source(&none), None);
    }
}
