//! Attribution: which FILE a gate finding belongs to, by evidence — the endpoint-literal grep
//! for findings that name no path, the render gate's served-page derivation, and the repair
//! ledger's HANDOFFS (a prior lane's own routing). Sibling module under the incremental-split
//! law (development_gates::swarm_rs_line_count_only_decreases). `endpoint_literal_of` (since
//! renamed `endpoint_literal_forms_of`) and `attribute_gate_finding` moved verbatim from
//! swarm.rs, except the possessive-apostrophe cut in `clean` (r5: `/api/drafts's` kept its
//! apostrophe, the tree grep hit zero files, and 6 of 8 round-0 findings misrouted to the entry
//! file).
//!
//! r6c (gate A, "repair must OWN every finding"), replayed to the item against the archived
//! tree: (1) `/api/drafts/<id>/submit` grepped ZERO files because the route table spells
//! `{id}` and app/drafts.py routes by SEGMENTS (`parts[1:3] == ["api", "drafts"]`), so the
//! drafts handler was never a shard and its round-1 fix died at promotion — placeholder forms
//! are normalized across conventions and a path segment's namesake module becomes a claim;
//! (2) `POST /api/drafts`'s RESPONSE-shape finding was won by web/app.js (three fetch()
//! literals) over the route table — a server-response finding ranks server source above web
//! assets; (3) the render gate's "ZERO rows after a successful sync" named no file at all and
//! rode `critical_unassigned` every round — the emitter now derives the page and its
//! row-building script from what the server actually served (`render_sources`); (4) the lane
//! told to hand a fix off by name did so ("HANDOFF — Files touched: `app/drafts.py`") and the
//! handoff was a dead letter — it is persisted (`parse_handoffs`) and consumed next round
//! (`handoffs_from_rollup`), ahead of the grep.

use super::findings::{FileGroup, FindingProvenance};
use std::collections::HashMap;

/// A route segment that stands for a value, in any of the conventions a tree may use:
/// `<id>` / `<int:id>` (werkzeug/flask), `{id}` (openapi, fastapi, the sb-7 route table),
/// `:id` (express, rails). Returns the bare NAME so the same route can be re-spelled in each.
/// Derived from the segment's own delimiters — never a list of names.
fn placeholder_name(seg: &str) -> Option<String> {
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if let Some(inner) = seg.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        let name = inner.rsplit(':').next().unwrap_or(inner);
        return ident(name).then(|| name.to_string());
    }
    if let Some(inner) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let name = inner.split(':').next().unwrap_or(inner);
        return ident(name).then(|| name.to_string());
    }
    if let Some(name) = seg.strip_prefix(':') {
        return ident(name).then(|| name.to_string());
    }
    None
}

/// P1-3, half one: the ENDPOINT LITERAL FORMS a gate finding names, most specific first. The
/// deterministic gate's own emitters write `GET <path> returned <code>` / `POST <path> …`, so
/// the token after an HTTP verb is the highest-confidence literal; a backticked `/…` token is
/// the fallback for prose findings. A bare `/` is deliberately absent — grepping a tree for "/"
/// matches every file, which is attribution-shaped noise, and the entry-file fallback answers
/// that case honestly instead.
///
/// FORMS, deduped, in this order: the VERBATIM literal (r5: the gate probes placeholder routes
/// verbatim — `POST /api/payments/<id>/note's response …` — and a route table may hold that
/// exact string in code); then, when the route carries a placeholder, the same route re-spelled
/// in EACH convention (`<id>`, `{id}`, `:id` — r6c: the gate's `<id>` grepped zero files while
/// app/ledgerd/__init__.py's table spelled `{id}`, so every drafts finding fell to the prefix
/// form and pooled three routes' hits into the dispatcher); last the PREFIX up to the first
/// placeholder (`/api/payments/`), the old cut, for a route no file spells in any convention.
/// The caller tries them in order and falls through only when a form greps zero files.
fn endpoint_literal_forms_of(finding: &str) -> Vec<String> {
    // r5: the gate's own templates write `POST {path}'s response …`, so the raw token is
    // `/api/drafts's`. `trim_matches` only trims at token ENDS — the trailing `s` is
    // alphanumeric, so the apostrophe survived, the tree grep hit zero files, and the
    // entry-file fallback misrouted 6 of 8 round-0 findings. CUT at the first disallowed
    // character instead (after trimming any disallowed lead): `/api/drafts's` → `/api/drafts`,
    // `/api/payments/<id>/note's` → `/api/payments/<id>/note` (verbatim) / `/api/payments/`
    // (prefix). The verbatim set admits every placeholder delimiter; a trailing `:` is prose
    // punctuation, never part of a path.
    let form = |verbatim: bool| -> Option<String> {
        let ok = |c: char| {
            c.is_ascii_alphanumeric() || "/_-.".contains(c) || (verbatim && "<>{}:".contains(c))
        };
        let clean = |t: &str| {
            t.trim_start_matches(|c: char| !ok(c))
                .split(|c: char| !ok(c))
                .next()
                .unwrap_or("")
                .trim_end_matches(':')
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
    fn push(forms: &mut Vec<String>, s: String) {
        if s.len() > 1 && !forms.contains(&s) {
            forms.push(s);
        }
    }
    let mut forms: Vec<String> = Vec::new();
    if let Some(verbatim) = form(true) {
        let segs: Vec<&str> = verbatim.split('/').collect();
        let names: Vec<Option<String>> = segs.iter().map(|s| placeholder_name(s)).collect();
        push(&mut forms, verbatim.clone());
        if names.iter().any(Option::is_some) {
            let conventions: [fn(&str) -> String; 3] = [
                |n| format!("<{n}>"),
                |n| format!("{{{n}}}"),
                |n| format!(":{n}"),
            ];
            for wrap in conventions {
                let respelled: Vec<String> = segs
                    .iter()
                    .zip(&names)
                    .map(|(s, n)| match n {
                        Some(n) => wrap(n),
                        None => s.to_string(),
                    })
                    .collect();
                push(&mut forms, respelled.join("/"));
            }
        }
    }
    if let Some(prefix) = form(false) {
        push(&mut forms, prefix);
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

fn is_source_file(f: &str) -> bool {
    super::findings::FINDING_SOURCE_EXTS
        .iter()
        .any(|e| f.ends_with(e))
}

/// The one web-asset predicate (briefs::is_asset_owner), asked about a single file.
fn is_web_asset(f: &str) -> bool {
    super::briefs::is_asset_owner(std::slice::from_ref(&f.to_string()))
}

/// The module name a file answers to: its basename without extension, or — for a Python
/// package's `__init__.py` — the package directory's name.
fn module_stem(f: &str) -> &str {
    let base = f.rsplit('/').next().unwrap_or(f);
    let stem = base.split('.').next().unwrap_or(base);
    if stem == "__init__" {
        f.rsplit('/').nth(1).unwrap_or(stem)
    } else {
        stem
    }
}

/// SEGMENT-BASENAME claims for an endpoint finding: every source file whose module name equals
/// a non-placeholder segment of the route (`/api/drafts/<id>/submit` → `api`, `drafts`,
/// `submit` → app/api.py, app/drafts.py). REST convention makes the namesake module the
/// handler's usual home, and the claim is derived from THIS tree's files, never a table. r6c:
/// app/drafts.py routes by segments and never spells its own literal, so no grep could reach
/// it; as a runner-up claim it joins the shard, and an edit there PROMOTES. Test paths are
/// left out (a handler fix is not handed its tests); `exclude` is the winner. Order is
/// `all_files` order; `resolve_shard_ownership` keeps a path already owned by an earlier shard
/// with that shard (one door).
fn segment_basename_claims(literal: &str, all_files: &[String], exclude: &[&str]) -> Vec<String> {
    let segments: Vec<&str> = literal
        .split('/')
        .filter(|s| !s.is_empty() && placeholder_name(s).is_none())
        .collect();
    all_files
        .iter()
        .filter(|f| is_source_file(f) && !f.contains("test") && !exclude.contains(&f.as_str()))
        .filter(|f| segments.contains(&module_stem(f)))
        .cloned()
        .collect()
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
/// `server_side` is the finding's CLASS (FindingSource::is_server_response_probe): for a
/// finding about what a HANDLER answered, server-side source ranks above web assets whenever
/// both carry the literal — a page that CALLS the endpoint is not where a response-shape
/// defect lives (r6c F5: web/app.js's three fetch() literals out-counted the route table's two
/// and a frontend shard carried the server finding). MILD: a preference inside the ranking; a
/// web asset still wins when no server file greps at all.
///
/// Returns `(winner, runner_ups)`. The runner-ups are ownership CLAIMS, never the group: the
/// second-best grep candidate when it is a SOURCE file with at least one comment-stripped
/// boundary hit (a finding whose evidence reconciles across two files — route table vs handler
/// body — must let the shard own both, so whichever side the worker fixes can land: the
/// js/css↔html precedent at `shard_owned_files`), then the route's segment-basename modules.
/// Grouping stays by winner; only ownership may widen, and only through the caller's
/// `resolve_shard_ownership` claim pass.
pub(super) fn attribute_gate_finding_ranked(
    finding: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
    server_side: bool,
) -> Option<(String, Vec<String>)> {
    let literals = endpoint_literal_forms_of(finding);
    let claims = |winner: &str| -> Vec<String> {
        match literals.first() {
            Some(lit) => segment_basename_claims(lit, all_files, &[winner]),
            None => Vec::new(),
        }
    };
    for lit in &literals {
        // Forms are tried most-specific first: the VERBATIM placeholder route, its
        // re-spellings, then the prefix cut, falling through ONLY when a form greps zero
        // files' stripped source. Within each form the ordering and tiebreaks are unchanged.
        // (`<`, `{` and `:` after the prefix form are boundary chars below — a route table's
        // `/api/drafts/{id}/…` entries still boundary-count for `/api/drafts/`.)
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
        // (server class when preferred, stripped b, stripped raw, raw b, raw raw)
        type RankKey = (bool, usize, usize, usize, usize);
        // A DATA OR DOC FILE NEVER OUTRANKS SOURCE — extract_file_from_finding's take()-side
        // rule (swarm.rs, same wording), which the grep side never got: the WINNER slot had no
        // source-ext filter, only the runner-up did, so a README.md spelling an endpoint often
        // enough would take the shard and aim a code fix at documentation. `best`/`second` rank
        // SOURCE-ext candidates only; a non-source candidate wins only when NO source-ext
        // candidate greps a nonzero stripped count — FINDING_PATH_EXTS deliberately admits
        // .md/.json so a finding ABOUT those files stays attributable, and that case still
        // lands on them.
        let prefer_server = |f: &str| server_side && !is_web_asset(f);
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
                prefer_server(f),
                boundary_hits(&src),
                stripped_raw,
                boundary_hits(&full),
                full.matches(lit.as_str()).count(),
            );
            if !is_source_file(f) {
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
            let winner = all_files[i].clone();
            let mut runner_ups: Vec<String> = second
                .filter(|((_, sb, _, _, _), _)| *sb >= 1)
                .map(|(_, j)| all_files[j].clone())
                .filter(|f| is_source_file(f))
                .into_iter()
                .collect();
            for c in claims(&winner) {
                if c != winner && !runner_ups.contains(&c) {
                    runner_ups.push(c);
                }
            }
            return Some((winner, runner_ups));
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
    let winner = entries
        .iter()
        .find(|f| {
            f.rsplit('/')
                .nth(1)
                .map(|pkg| !pkg.is_empty() && finding.contains(pkg))
                .unwrap_or(false)
        })
        .or_else(|| entries.first())
        .map(|f| (*f).clone())?;
    // A route nothing spells still names its segments: the namesake modules ride the entry
    // shard as claims, so the handler that routes by segments can be fixed there and land.
    let runner_ups = claims(&winner)
        .into_iter()
        .filter(|c| *c != winner)
        .collect();
    Some((winner, runner_ups))
}

/// The winner alone, for tests that assert grouping without ownership or class.
#[cfg(test)]
fn attribute_gate_finding(
    finding: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    attribute_gate_finding_ranked(finding, all_files, read_source, false).map(|(w, _)| w)
}

/// What `attribute_findings` decided for one round.
pub(super) struct Attributed {
    /// One group per winner file, first-seen order — the round's shard set.
    pub(super) groups: Vec<FileGroup>,
    /// Findings nothing could place — the caller emits them as `known_bugs`.
    pub(super) known_bugs: Vec<String>,
    /// winner file → ownership claims (candidate co-ownership only — nothing is owned until
    /// `resolve_shard_ownership`'s claim pass).
    pub(super) runner_ups: HashMap<String, Vec<String>>,
    /// `(path, finding)` pairs where a PRIOR round's lane handed the finding to `path` — the
    /// caller emits each as `handoff_consumed`, so the routing is visible in the event stream.
    pub(super) handoffs_consumed: Vec<(String, String)>,
}

/// P1-3, the seam every repair path shares: `group_findings_by_file`, then a prior lane's
/// HANDOFF, then evidence-based attribution for what neither could place. Attributed findings
/// JOIN their file's shard (or open one); what remains is the KNOWN-BUGS list — the caller
/// emits it as an event and dispatches no whole-tree residue worker for it.
///
/// THE HANDOFF (r6c): the repair brief tells a shard that a fix belonging to a file it does
/// not own is HANDED OFF by name in its final message, and the app.js lane did exactly that
/// ("HANDOFF — Files touched: `app/drafts.py` only") — to nobody, because nothing persisted or
/// read it. `handoffs` is finding-text → paths from the ledger (`handoffs_from_rollup`). For a
/// finding the text itself names a file for, the handed path CO-OWNS (the finding's own file
/// stays the group); for a finding that names no file, the handed path IS the group — ahead
/// of the literal grep, because it is the previous lane's evidence-backed routing.
///
/// ALL runner-ups per winner file, not the first — r6c: a winner with SEVERAL findings for the
/// SAME endpoint (each its own literal form) can rank a DIFFERENT second-best file per finding,
/// and a single-slot map silently kept only the first.
pub(super) fn attribute_findings(
    findings: &[String],
    all_files: &[String],
    prov: &FindingProvenance,
    handoffs: &HashMap<String, Vec<String>>,
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Attributed {
    let (mut groups, unassigned) = super::findings::group_findings_by_file(findings, all_files);
    let mut known_bugs: Vec<String> = Vec::new();
    let mut runner_ups: HashMap<String, Vec<String>> = HashMap::new();
    let mut handoffs_consumed: Vec<(String, String)> = Vec::new();
    let handed = |f: &str| -> Vec<String> {
        handoffs
            .get(f)
            .into_iter()
            .flatten()
            .filter(|p| all_files.contains(p))
            .cloned()
            .collect()
    };
    fn claim(runner_ups: &mut HashMap<String, Vec<String>>, winner: &str, path: String) {
        if path == winner {
            return;
        }
        let slot = runner_ups.entry(winner.to_string()).or_default();
        if !slot.contains(&path) {
            slot.push(path);
        }
    }
    for g in &groups {
        for f in &g.findings {
            for p in handed(f) {
                if p != g.file {
                    handoffs_consumed.push((p.clone(), f.clone()));
                }
                claim(&mut runner_ups, &g.file, p);
            }
        }
    }
    for f in unassigned {
        let handed_paths = handed(&f);
        let placed = match handed_paths.split_first() {
            Some((first, rest)) => {
                for p in &handed_paths {
                    handoffs_consumed.push((p.clone(), f.clone()));
                }
                Some((first.clone(), rest.to_vec()))
            }
            None => {
                let server_side = prov
                    .source_of(&f)
                    .is_some_and(|s| s.is_server_response_probe());
                attribute_gate_finding_ranked(&f, all_files, read_source, server_side)
            }
        };
        match placed {
            Some((file, claims)) => {
                for c in claims {
                    claim(&mut runner_ups, &file, c);
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
    Attributed {
        groups,
        known_bugs,
        runner_ups,
        handoffs_consumed,
    }
}

/// F883/E5: the files a per-file fix shard OWNS. A pytest failure attributes to its TEST file —
/// the only path a `-q` summary names — but the defect is as often in the module under test,
/// which a shard owning only the test cannot land: its worker either fixes the module in a
/// shadow that promote discards, or takes the one route that CAN land — weakening the tests.
/// Owning BOTH keeps the fix landable either way. The module is added only when it resolves to
/// a planned non-test file AND no sibling group already owns it — the partition must stay
/// disjoint, because two shards writing one real file is the race this machinery exists to
/// prevent.
fn shard_owned_files(
    group_file: &str,
    all_files: &[String],
    taken: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut owned = vec![group_file.to_string()];
    let base = group_file.rsplit('/').next().unwrap_or(group_file);
    if let Some(stem) = base
        .strip_prefix("test_")
        .map(|r| r.trim_end_matches(".py"))
        .filter(|s| !s.is_empty())
    {
        let want = format!("{stem}.py");
        if let Some(module) = all_files.iter().find(|f| {
            let fb = f.rsplit('/').next().unwrap_or(f);
            fb == want && !fb.starts_with("test_")
        }) {
            if module.as_str() != group_file && !taken.contains(module.as_str()) {
                owned.push(module.clone());
            }
        }
    }
    // A SCRIPT'S FINDINGS RECONCILE AGAINST ITS MARKUP. MEASURED (run 10, round 0): the dom-id
    // scan attributed nine findings to app.js — each saying "either add the id to the HTML or fix
    // the reference" — and the shard owning only app.js took the natural half of that
    // instruction: seven tool calls, every missing id added to index.html, pytest collect green
    // in its shadow. Promote copies only owned files, so the grade-what-lands preview correctly
    // refused the byte-identical app.js — and the round DISCARDED a correct repair. The js/css
    // shard now owns the page markup too (when planned and unclaimed), so whichever side of the
    // reconciliation the worker picks can actually land.
    if base.ends_with(".js") || base.ends_with(".css") {
        let dir = group_file.strip_suffix(base).unwrap_or("");
        if let Some(html) = all_files
            .iter()
            .filter(|f| f.ends_with(".html"))
            .max_by_key(|f| f.starts_with(dir))
        {
            if html.as_str() != group_file && !taken.contains(html.as_str()) {
                owned.push(html.clone());
            }
        }
    }
    owned
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
    runner_ups: &HashMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    let mut taken: std::collections::HashSet<String> =
        groups.iter().map(|g| g.file.clone()).collect();
    groups
        .iter()
        .map(|g| {
            let mut owned = shard_owned_files(&g.file, all_files, &taken);
            // ALL of this winner's runner-ups claim, not the first — see attribute_findings.
            for ru in runner_ups.get(&g.file).into_iter().flatten() {
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

/// The wave's own backstop, not a substitute for attributing better: which of THIS round's
/// `unassigned` findings are provenance-CRITICAL (RenderGateRows, SyncAcquisition, BootProbe,
/// …) — the class that means the app is unusable — and therefore got NO shard this round.
/// `group_findings_by_file`/`attribute_gate_finding_ranked` can both legitimately decline (a
/// finding that genuinely names no file), so this never gates or refuses; it only makes the
/// silence LOUD (the fallback gate: a missing owner is a named absence-event, never quiet). r6c:
/// a render-gate critical rode `unassigned` for two full rounds with zero shards' logs ever
/// mentioning it — findings.rs's reverse-suffix resolve closes the specific URL-basename case
/// that caused it; this is the general backstop for whatever still slips through (a second
/// ambiguous basename, a check with no file-naming evidence at all).
pub(super) fn criticals_left_unassigned(
    prov: &FindingProvenance,
    unassigned: &[String],
) -> Vec<String> {
    unassigned
        .iter()
        .filter(|f| prov.severity_label(f) == "critical")
        .cloned()
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

/// One line of a shard's HANDOFF that names a tree path the shard did not own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Handoff {
    pub(super) path: String,
    pub(super) symbol: Option<String>,
    pub(super) note: String,
}

/// PERSIST half of the handoff (r6c): the shard's final message, from its first handoff line
/// ("HANDOFF", "hand off", "handed off" — the brief's own words) to the end, yields one entry
/// per EXISTING tree path named there that the shard did not own, with the first backticked
/// identifier on that line as the symbol (a following "Symbols changed: `x`" line completes an
/// entry that had none) and the line itself as the note. Lenient by design — the reporter is
/// a weak model, not a serializer — and bounded by the tree: a path not in `all_files` is
/// never a handoff, so a hallucinated file cannot become a claim. The r6c round-1 app.js
/// message parses to exactly {path: app/drafts.py, symbol: _draft_obj}.
pub(super) fn parse_handoffs(output: &str, all_files: &[String], owned: &[String]) -> Vec<Handoff> {
    let lines: Vec<&str> = output.lines().collect();
    let Some(start) = lines.iter().position(|l| {
        let low = l.to_lowercase();
        ["handoff", "hand off", "hand-off", "handed off"]
            .iter()
            .any(|k| low.contains(k))
    }) else {
        return Vec::new();
    };
    let ident = |c: &str| {
        let mut chars = c.chars();
        matches!(chars.next(), Some(ch) if ch.is_alphabetic() || ch == '_')
            && chars.all(|ch| ch.is_alphanumeric() || "_.:".contains(ch))
    };
    let mut out: Vec<Handoff> = Vec::new();
    for line in &lines[start..] {
        let paths: Vec<String> = line
            .split(|c: char| c.is_whitespace() || "`'\"(),;:*".contains(c))
            .map(|t| {
                t.trim_matches(|c: char| !(c.is_alphanumeric() || "/_.-".contains(c)))
                    .trim_end_matches('.')
            })
            .filter(|t| t.contains('/') || t.contains('.'))
            .filter(|t| all_files.iter().any(|a| a == t) && !owned.iter().any(|o| o == t))
            .map(str::to_string)
            .collect();
        // The symbol is named BEFORE its path ("`serve_stream` in `app/api.py`"); backticks
        // after the path are the lane's asides ("(`web/app.js` untouched — sends nested
        // `counterparty`)"), so a path line's symbol comes from the text ahead of its first
        // handed path, and a following "Symbols changed: `x`" line completes an entry without one.
        let cutoff = paths
            .iter()
            .filter_map(|p| line.find(p.as_str()))
            .min()
            .unwrap_or(line.len());
        let symbol = line
            .get(..cutoff)
            .unwrap_or("")
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .find(|c| !c.contains('/') && ident(c) && !all_files.iter().any(|a| a == c))
            .map(str::to_string);
        if paths.is_empty() {
            if let (Some(sym), Some(last)) = (symbol, out.last_mut()) {
                if last.symbol.is_none() {
                    last.symbol = Some(sym);
                }
            }
            continue;
        }
        for path in paths {
            if out.iter().any(|h| h.path == path) {
                continue;
            }
            out.push(Handoff {
                path,
                symbol: symbol.clone(),
                note: line.trim().chars().take(300).collect(),
            });
        }
    }
    out
}

/// CONSUME half of the handoff: finding-text → handed paths, from the roll-up's repair rows
/// (`/repair/rounds[]`, each a shard's mini with `findings_assigned` and `handoffs`). Newest
/// round first, so the latest lane's routing leads; every finding the row was assigned inherits
/// the row's handoffs (lenient — the lane rarely numbers them). Only paths in `all_files`
/// survive. A finding that closed since simply never appears in the next round's texts, so a
/// stale handoff is inert by construction.
pub(super) fn handoffs_from_rollup(
    rollup: Option<&serde_json::Value>,
    all_files: &[String],
) -> HashMap<String, Vec<String>> {
    // No roll-up, or one with no repair rows yet (round 0), is honestly empty: nothing was
    // handed off because no lane has run — no event, no substitution.
    let mut rows: Vec<&serde_json::Value> = match rollup
        .and_then(|r| r.pointer("/repair/rounds"))
        .and_then(|a| a.as_array())
    {
        Some(a) => a.iter().collect(),
        None => Vec::new(),
    };
    rows.sort_by_key(|r| std::cmp::Reverse(r.get("round").and_then(|x| x.as_u64()).unwrap_or(0)));
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let paths: Vec<String> = row
            .get("handoffs")
            .and_then(|h| h.as_array())
            .into_iter()
            .flatten()
            .filter_map(|h| h.get("path").and_then(|p| p.as_str()))
            .filter(|p| all_files.iter().any(|a| a == p))
            .map(str::to_string)
            .collect();
        if paths.is_empty() {
            continue;
        }
        for f in row
            .get("findings_assigned")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str())
        {
            let slot = out.entry(f.to_string()).or_default();
            for p in &paths {
                if !slot.contains(p) {
                    slot.push(p.clone());
                }
            }
        }
    }
    out
}

/// What the render gate measured, named as files: the page the server actually served at `/`
/// and the scripts it loaded, resolved against the tree.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RenderSources {
    /// The `src` values exactly as the served page wrote them, document order.
    pub(super) loaded: Vec<String>,
    /// Those that resolved to a tree file (exact, or a UNIQUE reverse-suffix match — the same
    /// rule extract_file_from_finding applies to a URL basename), document order.
    pub(super) scripts: Vec<String>,
    /// The loaded script that builds table rows (most tbody/tr/td construction sites in its
    /// comment-stripped source; ties to document order) — None when none does.
    pub(super) renderer: Option<String>,
    /// The tree's html that is the served page: the one html containing every loaded `src`,
    /// else the tree's only html.
    pub(super) page: Option<String>,
}

impl RenderSources {
    /// The attribution-list suffix the render findings end with — ` (in \`renderer\`,
    /// \`page\`, …)` — the exact trailing shape `extract_file_from_finding` parses, FIRST entry
    /// first: for a zero-rows finding the row-building script is the first owner and the page
    /// the runner-up (the js↔html pairing at `shard_owned_files` then owns both). Empty when
    /// nothing resolved: the finding stays unowned and `critical_unassigned` says so — never a
    /// substituted name.
    pub(super) fn attribution_suffix(&self) -> String {
        let mut list: Vec<&String> = Vec::new();
        if let Some(r) = &self.renderer {
            list.push(r);
        }
        if let Some(p) = &self.page {
            if !list.contains(&p) {
                list.push(p);
            }
        }
        for s in &self.scripts {
            if !list.contains(&s) {
                list.push(s);
            }
        }
        if list.is_empty() {
            return String::new();
        }
        format!(
            " (in {})",
            list.iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// The measured facts in prose, ahead of the suffix — what the served page loaded and
    /// which script builds rows — so the shard reads the derivation, not just its result.
    pub(super) fn evidence_sentence(&self) -> String {
        if self.loaded.is_empty() {
            return "The served page loaded no scripts.".to_string();
        }
        let loaded = self
            .loaded
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(", ");
        match &self.renderer {
            Some(r) => format!(
                "The served page loaded {loaded}; `{r}` is the script that builds the table rows."
            ),
            None => format!(
                "The served page loaded {loaded}; none of them builds table rows (no tbody/tr \
                 construction found)."
            ),
        }
    }
}

/// `<script … src="…">` values in document order, from the html the server actually served
/// (a `src=` must follow whitespace, so `data-src=` is not one). Byte offsets come from the
/// ASCII-lowercased copy, which preserves them.
fn script_srcs(html: &str) -> Vec<String> {
    let low = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = low.get(from..).and_then(|rest| rest.find("<script")) {
        let tag_start = from + i;
        let Some(tag_low) = low
            .get(tag_start..)
            .and_then(|rest| rest.find('>').and_then(|e| rest.get(..e)))
        else {
            break;
        };
        let Some(tag) = html.get(tag_start..tag_start + tag_low.len()) else {
            break;
        };
        let src_at = tag_low.match_indices("src=").find(|(j, _)| {
            tag_low
                .get(..*j)
                .and_then(|before| before.chars().next_back())
                .map(char::is_whitespace)
                .unwrap_or(false)
        });
        if let Some((j, _)) = src_at {
            if let Some(rest) = tag.get(j + 4..) {
                let val = match rest.chars().next() {
                    Some(q @ ('"' | '\'')) => {
                        rest.get(1..).and_then(|r| r.split(q).next()).unwrap_or("")
                    }
                    _ => rest
                        .split(|c: char| c.is_whitespace() || c == '>')
                        .next()
                        .unwrap_or(""),
                };
                if !val.is_empty() {
                    out.push(val.to_string());
                }
            }
        }
        from = tag_start + tag_low.len();
    }
    out
}

/// GAP 1 (r6c): the render gate's source, DERIVED from what the server served — never a
/// hardcoded page or script name. The probe reports only console-error sources (0eb7a09ea);
/// a zero-rows finding with no console error has none, so the gate reads the served `/`
/// itself: its script tags, resolved to tree files, scored for table-row construction. Pure
/// over (html, files, reader) so a fixture drives it and the archived r6c tree replays it
/// read-only.
pub(super) fn render_sources(
    served_html: &str,
    all_files: &[String],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> RenderSources {
    let loaded = script_srcs(served_html);
    let resolve = |src: &str| -> Option<String> {
        let path = src.split(['?', '#']).next().unwrap_or(src);
        let path = match path.find("://") {
            Some(i) => path
                .get(i + 3..)
                .and_then(|host_and_path| {
                    host_and_path.find('/').and_then(|j| host_and_path.get(j..))
                })
                .unwrap_or(""),
            None => path,
        };
        let rel = path.trim_start_matches('/');
        if rel.is_empty() {
            return None;
        }
        if let Some(f) = all_files.iter().find(|f| f.as_str() == rel) {
            return Some(f.clone());
        }
        let reverse: Vec<&String> = all_files
            .iter()
            .filter(|f| f.ends_with(&format!("/{rel}")))
            .collect();
        (reverse.len() == 1).then(|| reverse[0].clone())
    };
    let mut scripts: Vec<String> = Vec::new();
    for f in loaded.iter().filter_map(|s| resolve(s)) {
        if !scripts.contains(&f) {
            scripts.push(f);
        }
    }
    // Markup (`<tr`), DOM API (`insertRow`) and DOM-built tables that name the tag as a string
    // literal (r6c's app.js: `el("tr", …)` through one createElement helper — no `<tr` anywhere
    // in code, only in a comment the stripper removes).
    let row_markers = [
        "tbody",
        "<tr",
        "<td",
        "insertrow",
        "insertcell",
        "\"tr\"",
        "'tr'",
        "\"td\"",
        "'td'",
    ];
    let row_sites = |f: &str| -> usize {
        match read_source(f) {
            Some(src) => {
                let low = strip_comments_for_evidence(&src, f).to_lowercase();
                row_markers.iter().map(|m| low.matches(m).count()).sum()
            }
            None => 0,
        }
    };
    let mut renderer: Option<(usize, String)> = None;
    for f in &scripts {
        let n = row_sites(f);
        if n > 0 && renderer.as_ref().map(|(best, _)| n > *best).unwrap_or(true) {
            renderer = Some((n, f.clone()));
        }
    }
    let htmls: Vec<&String> = all_files
        .iter()
        .filter(|f| f.ends_with(".html") || f.ends_with(".htm"))
        .collect();
    let containing: Vec<&String> = htmls
        .iter()
        .copied()
        .filter(|f| match read_source(f) {
            Some(h) => loaded.iter().all(|s| h.contains(s.as_str())),
            None => false,
        })
        .collect();
    let page = if containing.len() == 1 {
        Some(containing[0].clone())
    } else if htmls.len() == 1 {
        Some(htmls[0].clone())
    } else {
        None
    };
    RenderSources {
        loaded,
        scripts,
        renderer: renderer.map(|(_, f)| f),
        page,
    }
}

/// Every file under `root` with a known finding extension, tree-relative with `/`, sorted —
/// the file list the render gate resolves served script URLs against (it runs inside
/// `run_spec_contract`, which has the tree root and no plan). Same skip list and depth as
/// `collect_py_files`.
pub(super) fn tree_files(root: &std::path::Path) -> Vec<String> {
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        ".venv",
        ".swarm",
        "__pycache__",
    ];
    fn walk(dir: &std::path::Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth == 0 || name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                walk(&p, depth - 1, out);
            } else if super::findings::FINDING_PATH_EXTS
                .iter()
                .any(|e| name.ends_with(e))
            {
                out.push(p);
            }
        }
    }
    let mut paths = Vec::new();
    walk(root, 6, &mut paths);
    let mut rel: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .collect();
    rel.sort();
    rel
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
        // A placeholder route yields the verbatim literal (the shape r5's route table holds in
        // real code) first, the other conventions' spellings next (r6c: the table spelled
        // `{id}`), its prefix cut last.
        assert_eq!(
            endpoint_literal_forms_of(
                "POST /api/payments/<id>/note's response does not carry the documented field(s) `ok`"
            ),
            vec![
                "/api/payments/<id>/note".to_string(),
                "/api/payments/{id}/note".to_string(),
                "/api/payments/:id/note".to_string(),
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
        let Attributed {
            groups,
            known_bugs: known,
            ..
        } = attribute_findings(
            &findings,
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        );
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
            attribute_gate_finding_ranked(f1, &all, &read, false),
            Some((
                "app/sync.py".to_string(),
                vec!["app/httpapi.py".to_string()]
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
            attribute_gate_finding_ranked(f4, &all, &read, false),
            Some((
                "app/ledgerd/__init__.py".to_string(),
                vec!["app/httpapi.py".to_string()]
            ))
        );
        // UNCLAIMED runner-up (F4 alone): the shard owns winner AND runner-up.
        let Attributed {
            groups,
            known_bugs: known,
            runner_ups,
            ..
        } = attribute_findings(
            &[f4.to_string()],
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        );
        assert!(known.is_empty());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file, "app/ledgerd/__init__.py");
        assert_eq!(
            runner_ups
                .get("app/ledgerd/__init__.py")
                .map(|v| v.as_slice()),
            Some(["app/httpapi.py".to_string()].as_slice())
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
        let Attributed {
            groups: groups2,
            runner_ups: runner_ups2,
            ..
        } = attribute_findings(
            &[f4.to_string(), f3.to_string()],
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        );
        let files: Vec<&str> = groups2.iter().map(|g| g.file.as_str()).collect();
        assert_eq!(files, ["app/ledgerd/__init__.py", "app/httpapi.py"]);
        let owned2 = resolve_shard_ownership(&groups2, &all, &runner_ups2);
        assert_eq!(owned2[0], vec!["app/ledgerd/__init__.py".to_string()]);
        assert_eq!(owned2[1], vec!["app/httpapi.py".to_string()]);
    }

    /// r6c ownership fix: one winner file (e.g. a client that calls several endpoints) can rank
    /// a DIFFERENT second-best SERVER file per finding. A single-slot runner-up map kept only
    /// the FIRST, so a shard's `owned_files` stopped growing after the first — the real fix a
    /// worker made to the SECOND endpoint's server file landed in its shadow and never rode the
    /// promote copy (a verified fix dying at promotion: `changed_samples` nonzero, `promoted:
    /// false`, because the fix lived outside `owned_files`). `resolve_shard_ownership` must
    /// claim EVERY runner-up a winner's findings produced, not the first.
    #[test]
    fn a_winner_with_two_findings_claims_both_runner_ups_not_just_the_first() {
        let groups = vec![FileGroup {
            file: "web/app.js".to_string(),
            findings: vec!["finding A".to_string(), "finding B".to_string()],
        }];
        let all = vec![
            "web/app.js".to_string(),
            "app/ledgerd/__init__.py".to_string(),
            "app/drafts.py".to_string(),
        ];
        let mut runner_ups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        runner_ups.insert(
            "web/app.js".to_string(),
            vec![
                "app/ledgerd/__init__.py".to_string(),
                "app/drafts.py".to_string(),
            ],
        );
        let owned = resolve_shard_ownership(&groups, &all, &runner_ups);
        assert_eq!(
            owned,
            vec![vec![
                "web/app.js".to_string(),
                "app/ledgerd/__init__.py".to_string(),
                "app/drafts.py".to_string(),
            ]],
            "every runner-up must be claimed, not just the first"
        );
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

    /// r6c: the backstop that makes an unowned critical LOUD instead of silent. A provenance-
    /// critical finding left in `unassigned` after both attribution passes must be reported; a
    /// medium left there (the expected, non-blocking residue class) must not.
    #[test]
    fn criticals_left_unassigned_reports_only_the_critical_class() {
        use super::super::findings::{FindingProvenance, FindingSource};
        let critical =
            "the served page renders NO data rows in a real browser (in `viz.js`)".to_string();
        let medium = "POST /api/drafts's response is missing field `amount_minor`".to_string();
        let mut prov = FindingProvenance::default();
        prov.tag(
            FindingSource::RenderGateRows,
            std::slice::from_ref(&critical),
        );
        prov.tag(
            FindingSource::EndpointContractProbe,
            std::slice::from_ref(&medium),
        );
        let unassigned = vec![critical.clone(), medium];
        assert_eq!(
            criticals_left_unassigned(&prov, &unassigned),
            vec![critical],
            "only the provenance-critical finding is a stuck-critical event"
        );
        assert!(criticals_left_unassigned(&prov, &[]).is_empty());
    }

    /// THE ROUND LOOP'S SHAPE, pinned. r6c's operator narrative was "viz.js got RETIRED from
    /// redispatch because its shard promoted last round" — the walk found no such mechanism:
    /// `attribute_findings` is a PURE function of THIS round's own findings, called fresh every
    /// round on `verdict.findings` from a fresh gate run, with no promoted-file exclusion list
    /// anywhere in the call chain. This test pins that shape directly: round 1's OWN findings
    /// (not round 0's) decide round 1's shard set — a file whose finding SURVIVED into round
    /// 1's fresh verdict gets a shard again regardless of what promoted last round; a file whose
    /// finding is simply ABSENT from round 1 (fixed, or never existed) gets none, and that is
    /// the ONLY reason it drops out — never a memory of its own prior promotion.
    #[test]
    fn a_rounds_shard_set_is_its_own_findings_never_a_memory_of_prior_promotion() {
        let all = vec!["app/drafts.py".to_string(), "web/viz.js".to_string()];
        let read = |_: &str| -> Option<String> { Some(String::new()) };

        // Round 0: both files have open findings and (per the r6c narrative) both get shards.
        let round0 = vec![
            "`app/drafts.py` raises KeyError on an empty JSON body".to_string(),
            "console error (in `web/viz.js`)".to_string(),
        ];
        let g0 = attribute_findings(
            &round0,
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        )
        .groups;
        assert_eq!(g0.len(), 2, "round 0 shards both files");

        // Round 1's OWN fresh gate run: drafts.py's finding is GONE (round 0 fixed it — a
        // genuinely cleared file); viz.js's finding SURVIVED (round 0's fix did not close it).
        let round1 = vec!["console error (in `web/viz.js`)".to_string()];
        let Attributed {
            groups: g1,
            known_bugs: unassigned1,
            ..
        } = attribute_findings(
            &round1,
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        );
        assert_eq!(
            g1.iter().map(|g| g.file.as_str()).collect::<Vec<_>>(),
            vec!["web/viz.js"],
            "the surviving critical's file reappears in round 1's set — nothing retires it"
        );
        assert!(
            unassigned1.is_empty(),
            "drafts.py is simply absent from round 1's own findings, not excluded by a promotion memory"
        );
    }

    /// GAP 2(a), r6c F6 verbatim: the gate probes `<id>`, the route table spells `{id}`, and
    /// app/drafts.py routes by segments — under the old two forms the finding fell to the
    /// prefix cut and pooled three routes' hits into the dispatcher.
    #[test]
    fn a_placeholder_route_is_respelled_in_every_convention_before_the_prefix_cut() {
        let f6 = "POST /api/drafts/<id>/submit's response could not be read as a JSON object on \
                  either probe — the spec documents a JSON response for every endpoint.";
        assert_eq!(
            endpoint_literal_forms_of(f6),
            vec![
                "/api/drafts/<id>/submit".to_string(),
                "/api/drafts/{id}/submit".to_string(),
                "/api/drafts/:id/submit".to_string(),
                "/api/drafts/".to_string(),
            ]
        );
        // Flask converters and express colons normalize to the same bare name.
        let flask = endpoint_literal_forms_of("GET /users/<int:user_id>/posts returned 404");
        assert_eq!(flask[0], "/users/<int:user_id>/posts", "verbatim first");
        assert_eq!(
            &flask[1..],
            [
                "/users/<user_id>/posts",
                "/users/{user_id}/posts",
                "/users/:user_id/posts",
                "/users/"
            ],
            "the converter is stripped in every re-spelling"
        );
        let express = endpoint_literal_forms_of(
            "the advertised `/v1/items/:itemId` endpoint answers 500 under load",
        );
        assert_eq!(express[0], "/v1/items/:itemId");
        assert!(
            express.contains(&"/v1/items/{itemId}".to_string()),
            "{express:?}"
        );
        assert!(express.contains(&"/v1/items/".to_string()), "{express:?}");
        // The r6c tree's shapes: only the `{id}` spelling exists in code → the route table wins
        // the group, and the segment namesake (app/drafts.py, which spells no literal) rides
        // as a claim.
        let all = vec![
            "app/ledgerd/__init__.py".to_string(),
            "web/app.js".to_string(),
            "app/drafts.py".to_string(),
        ];
        let read = |f: &str| -> Option<String> {
            match f {
                "app/ledgerd/__init__.py" => Some(
                    "    (\"POST\", \"/api/drafts\"),\n\
                     \x20   (\"POST\", \"/api/drafts/{id}/submit\"),\n\
                     \x20   (\"POST\", \"/api/drafts/{id}/approve\"),\n"
                        .into(),
                ),
                "web/app.js" => Some(
                    "draftsFetch(\"/api/drafts/\" + encodeURIComponent(d.id) + \"/\" + action, \"POST\", {});\n"
                        .into(),
                ),
                "app/drafts.py" => Some(
                    "        if method == \"POST\" and len(parts) == 5 and parts[1:3] == [\"api\", \"drafts\"]:\n"
                        .into(),
                ),
                _ => None,
            }
        };
        let (winner, claims) = attribute_gate_finding_ranked(f6, &all, &read, true).unwrap();
        assert_eq!(
            winner, "app/ledgerd/__init__.py",
            "the {{id}} form reaches the route table instead of pooling prefix hits"
        );
        assert!(
            claims.contains(&"app/drafts.py".to_string()),
            "the segment namesake rides as a claim: {claims:?}"
        );
    }

    /// GAP 2(b): segment-basename claims are derived from THIS tree's files — a module (or
    /// package) named like a route segment — never from a table, and never a test path. They
    /// ride the entry-file fallback too, so a route nothing spells still reaches its handler.
    #[test]
    fn segment_namesake_modules_become_runner_up_claims_never_tests() {
        let all: Vec<String> = [
            "app/__main__.py",
            "app/api.py",
            "app/drafts.py",
            "app/drafts/__init__.py",
            "tests/test_drafts.py",
            "app/webhooks.py",
            "web/app.js",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            segment_basename_claims("/api/drafts/<id>/submit", &all, &["app/api.py"]),
            vec![
                "app/drafts.py".to_string(),
                "app/drafts/__init__.py".to_string()
            ]
        );
        assert!(segment_basename_claims("/api/webhooks/meridian", &all, &[])
            .iter()
            .any(|c| c == "app/webhooks.py"));
        assert!(segment_basename_claims("/health", &all, &[]).is_empty());
        let read = |_: &str| -> Option<String> { Some(String::new()) };
        let (winner, claims) = attribute_gate_finding_ranked(
            "POST /api/drafts/<id>/reject's response could not be read as a JSON object",
            &all,
            &read,
            true,
        )
        .unwrap();
        assert_eq!(winner, "app/__main__.py");
        assert!(
            claims.contains(&"app/api.py".to_string())
                && claims.contains(&"app/drafts.py".to_string()),
            "{claims:?}"
        );
    }

    /// GAP 2 addendum, r6c F5 verbatim: `POST /api/drafts`'s RESPONSE-shape finding was won by
    /// web/app.js (three fetch() literals) over the route table (two rows), so a FRONTEND shard
    /// carried a server-side defect and its edits to app/drafts.py died at promotion. For a
    /// server-response finding (provenance class), server source ranks above web assets; the
    /// class is derived from the authoring check, and without it the counts decide as before.
    #[test]
    fn a_server_response_finding_ranks_server_source_above_the_calling_page() {
        let f5 = "POST /api/drafts's response does not carry the documented field(s) \
                  `amount_minor`, `currency` — the spec's endpoint table names them for exactly \
                  this endpoint.";
        let all = vec![
            "app/ledgerd/__init__.py".to_string(),
            "web/app.js".to_string(),
            "app/drafts.py".to_string(),
        ];
        let read = |f: &str| -> Option<String> {
            match f {
                "app/ledgerd/__init__.py" => Some(
                    "    (\"POST\", \"/api/drafts\"),\n\
                     \x20   (\"POST\", \"/api/drafts/{id}/submit\"),\n\
                     \x20   (\"GET\", \"/api/drafts\"),\n"
                        .into(),
                ),
                "web/app.js" => Some(
                    "draftsFetch(\"/api/drafts\", \"GET\");\n\
                     draftsFetch(\"/api/drafts\", \"GET\");\n\
                     draftsFetch(\"/api/drafts\", \"POST\", payload);\n"
                        .into(),
                ),
                "app/drafts.py" => {
                    Some("\"\"\"Drafts (POST /api/drafts). Roles: maker/checker.\"\"\"\n".into())
                }
                _ => None,
            }
        };
        let (winner, claims) = attribute_gate_finding_ranked(f5, &all, &read, true).unwrap();
        assert_eq!(winner, "app/ledgerd/__init__.py");
        assert!(claims.contains(&"app/drafts.py".to_string()), "{claims:?}");
        let (untyped, _) = attribute_gate_finding_ranked(f5, &all, &read, false).unwrap();
        assert_eq!(
            untyped, "web/app.js",
            "no class: the counts decide as before"
        );
        let mut prov = FindingProvenance::default();
        prov.tag(
            super::super::findings::FindingSource::EndpointContractProbe,
            &[f5.to_string()],
        );
        let a = attribute_findings(&[f5.to_string()], &all, &prov, &HashMap::new(), &read);
        assert_eq!(a.groups[0].file, "app/ledgerd/__init__.py");
        let owned = resolve_shard_ownership(&a.groups, &all, &a.runner_ups);
        assert!(
            owned[0].contains(&"app/drafts.py".to_string()),
            "an edit to the handler PROMOTES: {owned:?}"
        );
    }

    /// The HANDOFF, persist half — r6c's round-1 app.js final message, verbatim excerpt: the
    /// lane did exactly what the brief asked and it reached nobody.
    #[test]
    fn a_lanes_handoff_parses_to_the_unowned_path_and_its_symbol() {
        let all = vec![
            "web/app.js".to_string(),
            "web/index.html".to_string(),
            "app/drafts.py".to_string(),
            "app/api.py".to_string(),
        ];
        let owned = vec!["web/app.js".to_string(), "web/index.html".to_string()];
        let msg = "**FINDING 1: FIXED** — booted `python3 -m app.ledgerd --port 8931 --tokens-file \
                   /tmp/lgr_tokens.json` from the edited tree and ran the finding's own probe.\n\n\
                   **Root cause & fix** (server-side, so the edit landed in `app/drafts.py`, not my \
                   two web files):\n\
                   1. `_draft_obj` returned `name`/`country` only nested inside `counterparty`.\n\n\
                   **HANDOFF**\n\
                   - Files touched: `app/drafts.py` only (`web/app.js`, `web/index.html` untouched \
                   — frontend already sends nested `counterparty`, which still works).\n\
                   - Symbols changed: `_draft_obj` (+ top-level `name`, `country` keys), \
                   `_validate_create` (flat alias).\n\
                   - Nothing remains open.\n";
        let h = parse_handoffs(msg, &all, &owned);
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!(h[0].path, "app/drafts.py");
        assert_eq!(h[0].symbol.as_deref(), Some("_draft_obj"));
        assert!(h[0].note.starts_with("- Files touched"), "{}", h[0].note);
        // No handoff section, a path outside the tree, an owned path: nothing.
        assert!(parse_handoffs("FINDING 1: FIXED — edited app/drafts.py", &all, &owned).is_empty());
        assert!(parse_handoffs("HANDOFF: fix `app/ghost.py`", &all, &owned).is_empty());
        assert!(parse_handoffs("HANDOFF: fix `web/app.js`", &all, &owned).is_empty());
        // A bare path with sentence punctuation, and a symbol named before its path.
        let h2 = parse_handoffs(
            "I handed off the rest: `serve_stream` in app/api.py must send the SSE headers.",
            &all,
            &owned,
        );
        assert_eq!(h2.len(), 1);
        assert_eq!(h2[0].path, "app/api.py");
        assert_eq!(h2[0].symbol.as_deref(), Some("serve_stream"));
    }

    /// The HANDOFF, consume half: a prior round's handed path routes a still-open finding — as
    /// the GROUP for a finding that names no file (ahead of the grep), as a CLAIM for one that
    /// does. A path outside the tree and a finding that closed are inert.
    #[test]
    fn a_prior_rounds_handoff_routes_a_still_open_finding_ahead_of_the_grep() {
        let f5 = "POST /api/drafts's response does not carry the documented field(s) \
                  `amount_minor` — the spec's endpoint table names them."
            .to_string();
        let f0 =
            "the served page renders NO data rows in a real browser (in `web/viz.js`)".to_string();
        let all = vec![
            "web/app.js".to_string(),
            "web/viz.js".to_string(),
            "app/drafts.py".to_string(),
            "app/ledgerd/__init__.py".to_string(),
        ];
        let rollup = serde_json::json!({"repair": {"rounds": [
            {"round": 0, "shard": "web/app.js", "findings_assigned": [f5],
             "handoffs": [{"path": "app/drafts.py", "symbol": "_draft_obj",
                           "note": "- Files touched: `app/drafts.py` only"}]},
            {"round": 0, "shard": "web/viz.js", "findings_assigned": [f0],
             "handoffs": [{"path": "web/app.js", "symbol": null, "note": "the row renderer is app.js"}]},
            {"round": 0, "shard": "app/ledgerd/__init__.py", "findings_assigned": ["gone finding"],
             "handoffs": [{"path": "app/ghost.py", "symbol": null, "note": "x"}]}
        ]}});
        let handoffs = handoffs_from_rollup(Some(&rollup), &all);
        assert_eq!(
            handoffs.get(&f5).map(|v| v.as_slice()),
            Some(["app/drafts.py".to_string()].as_slice())
        );
        assert!(
            !handoffs.contains_key("gone finding"),
            "a path outside the tree is never a handoff"
        );
        // The grep would put f5 on web/app.js (the only file spelling the literal); the
        // handoff wins the group.
        let read = |f: &str| -> Option<String> {
            (f == "web/app.js").then(|| "fetch(\"/api/drafts\")".to_string())
        };
        let a = attribute_findings(
            &[f5.clone(), f0.clone()],
            &all,
            &FindingProvenance::default(),
            &handoffs,
            &read,
        );
        let files: Vec<&str> = a.groups.iter().map(|g| g.file.as_str()).collect();
        assert_eq!(
            files,
            ["web/viz.js", "app/drafts.py"],
            "f0 keeps its named file; f5 is grouped under the handed path"
        );
        assert_eq!(
            a.runner_ups.get("web/viz.js").map(|v| v.as_slice()),
            Some(["web/app.js".to_string()].as_slice()),
            "a named finding's handoff co-owns"
        );
        assert!(a
            .handoffs_consumed
            .contains(&("app/drafts.py".to_string(), f5.clone())));
        assert!(a
            .handoffs_consumed
            .contains(&("web/app.js".to_string(), f0.clone())));
        let b = attribute_findings(
            std::slice::from_ref(&f5),
            &all,
            &FindingProvenance::default(),
            &HashMap::new(),
            &read,
        );
        assert_eq!(
            b.groups[0].file, "web/app.js",
            "no rollup: the grep as before"
        );
        assert!(b.handoffs_consumed.is_empty());
    }

    /// GAP 1: the render gate's source is DERIVED from the served page — its script tags,
    /// resolved to the tree, scored for row construction — never a hardcoded name. r6c's
    /// index.html loads viz.js BEFORE app.js, so document order alone would name the wrong
    /// file; content decides. Nothing resolvable → no suffix (a loud absence, never a name).
    #[test]
    fn the_render_gates_source_is_derived_from_the_served_page_and_attributes() {
        let all = vec![
            "app/ledgerd/__init__.py".to_string(),
            "web/app.js".to_string(),
            "web/index.html".to_string(),
            "web/viz.js".to_string(),
        ];
        let html =
            "<!doctype html>\n<html><head><link rel=\"stylesheet\" href=\"styles.css\"></head>\n\
                    <body><table id=\"payments\"><tbody id=\"rows\"></tbody></table>\n\
                    \x20 <script src=\"viz.js\"></script>\n  <script src=\"app.js\"></script>\n\
                    </body></html>\n";
        let read = |f: &str| -> Option<String> {
            match f {
                "web/index.html" => Some(html.to_string()),
                "web/app.js" => Some(
                    "// renders rows\nfunction render(rows) {\n  const tbody = document.querySelector('tbody');\n\
                     \x20 tbody.innerHTML = rows.map(r => `<tr><td>${r.id}</td></tr>`).join('');\n}\n"
                        .into(),
                ),
                "web/viz.js" => {
                    Some("const ctx = canvas.getContext('2d');\nctx.fillRect(0, 0, 10, 10);\n".into())
                }
                _ => None,
            }
        };
        let rs = render_sources(html, &all, &read);
        assert_eq!(rs.loaded, vec!["viz.js", "app.js"]);
        assert_eq!(rs.scripts, vec!["web/viz.js", "web/app.js"]);
        assert_eq!(
            rs.renderer.as_deref(),
            Some("web/app.js"),
            "content decides, not document order"
        );
        assert_eq!(rs.page.as_deref(), Some("web/index.html"));
        assert_eq!(
            rs.attribution_suffix(),
            " (in `web/app.js`, `web/index.html`, `web/viz.js`)"
        );
        let finding = format!(
            "after a SUCCESSFUL sync the page still renders ZERO rows — the backend acquired the \
             data. {}{}",
            rs.evidence_sentence(),
            rs.attribution_suffix()
        );
        assert_eq!(
            extract_file_from_finding(&finding, &all).as_deref(),
            Some("web/app.js")
        );
        // No script builds rows: the page leads the list.
        let no_rows = |f: &str| -> Option<String> {
            if f == "web/index.html" {
                Some(html.to_string())
            } else {
                Some("console.log(1)".to_string())
            }
        };
        let rs2 = render_sources(html, &all, &no_rows);
        assert_eq!(rs2.renderer, None);
        assert_eq!(
            rs2.attribution_suffix(),
            " (in `web/index.html`, `web/viz.js`, `web/app.js`)"
        );
        // A CDN script no tree file answers to, no html in the plan: no suffix at all.
        let rs3 = render_sources(
            "<script src=\"http://cdn.example/x/lib.js\"></script><script data-src=\"y.js\"></script>",
            &["app/main.py".to_string()],
            &|_| None,
        );
        assert_eq!(rs3.loaded, vec!["http://cdn.example/x/lib.js"]);
        assert!(rs3.scripts.is_empty() && rs3.page.is_none());
        assert_eq!(rs3.attribution_suffix(), "");
    }

    /// GATE A's TRACE, run by the REAL code against the ARCHIVED r6c tree read-only (machine-
    /// local; skips loudly when absent): round 0's nine findings verbatim, F1 carrying the
    /// suffix the emitter now derives from the tree's own served page. Every finding must be
    /// OWNED, F1 by the row-building script, and the shard carrying the drafts findings must
    /// own app/drafts.py — the file the round-1 lane actually fixed and lost at promotion.
    /// Prints the shard table (`--nocapture`) for the commit's trace.
    #[test]
    fn r6c_round0_findings_are_all_owned_against_the_archived_tree_read_only() {
        let root = std::path::Path::new(
            "/Users/mihaiperdum/goose-builds/local-sb7-swarm-r6c-FINISHED-0.1420-passed-with-2-unowned-criticals-build-608m",
        );
        if !root.join("app/drafts.py").exists() {
            eprintln!(
                "SKIP: archived r6c tree not on this machine ({})",
                root.display()
            );
            return;
        }
        let all: Vec<String> = [
            "DECISIONS.md",
            "README.md",
            "app/__main__.py",
            "app/api.py",
            "app/auth.py",
            "app/db.py",
            "app/drafts.py",
            "app/ledger.py",
            "app/ledgerd/__init__.py",
            "app/ledgerd/__main__.py",
            "app/ledgerd/impl.py",
            "app/notifierd/__init__.py",
            "app/notifierd/__main__.py",
            "app/notifierd/impl.py",
            "app/notify_store.py",
            "app/outbox.py",
            "app/sync.py",
            "app/webhooks.py",
            "web/app.js",
            "web/index.html",
            "web/styles.css",
            "web/viz.js",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
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
        let served = read("web/index.html").expect("the archive's page");
        let rs = render_sources(&served, &all, &read);
        assert_eq!(rs.loaded, vec!["viz.js", "app.js"], "{rs:?}");
        assert_eq!(rs.renderer.as_deref(), Some("web/app.js"), "{rs:?}");
        assert_eq!(rs.page.as_deref(), Some("web/index.html"));
        use super::super::findings::FindingSource as S;
        let f = |s: &str| s.to_string();
        let findings = vec![
            (S::RenderGateRows, f("the served page renders NO data rows in a real browser — the API works but the frontend shows a user nothing. First console error: TypeError: Illegal invocation. Open web/index.html end to end: the page must fetch the documented endpoints and render the rows, and every fetch failure must surface a visible state, not a blank page. (in `viz.js`)")),
            (S::RenderGateRows, format!("after a SUCCESSFUL sync the page still renders ZERO rows — the backend acquired the data (the API returns it) but the frontend never displays it, so the user sees an empty table forever. After the sync completes, re-fetch the payments endpoint and RENDER the returned rows into the table, and update the last-synced/count readouts from that same response. {}{}", rs.evidence_sentence(), rs.attribution_suffix())),
            (S::DomIdScan, f("web/viz.js:533 references DOM id `viz-labels` which NO html file in the app defines — getElementById returns null there and the page throws at runtime (the rendered-nothing class). Either add the id to the HTML or fix the reference to an id that exists.")),
            (S::EndpointContractProbe, f("POST /api/payments/<id>/note's response does not carry the documented field(s) `id`, `note`, `version` — the spec's endpoint table names them for exactly this endpoint. Return them from this handler; without them the endpoint's contract cannot be verified by anyone, including this gate.")),
            (S::EndpointContractProbe, f("POST /api/webhooks/meridian's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.")),
            (S::EndpointContractProbe, f("POST /api/drafts's response does not carry the documented field(s) `amount_minor`, `currency`, `counterparty`, `name`, `country`, `note` — the spec's endpoint table names them for exactly this endpoint. Return them from this handler; without them the endpoint's contract cannot be verified by anyone, including this gate.")),
            (S::EndpointContractProbe, f("POST /api/drafts/<id>/submit's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.")),
            (S::EndpointContractProbe, f("POST /api/drafts/<id>/approve's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.")),
            (S::EndpointContractProbe, f("POST /api/drafts/<id>/reject's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.")),
        ];
        let mut prov = FindingProvenance::default();
        for (s, t) in &findings {
            prov.tag(*s, std::slice::from_ref(t));
        }
        let texts: Vec<String> = findings.iter().map(|(_, t)| t.clone()).collect();
        let a = attribute_findings(&texts, &all, &prov, &HashMap::new(), &read);
        let mut groups = a.groups;
        for g in &mut groups {
            prov.sort_findings(&mut g.findings);
        }
        prov.sort_groups(&mut groups);
        let owned = resolve_shard_ownership(&groups, &all, &a.runner_ups);
        eprintln!(
            "r6c round-0 TRACE — {} shard(s), {} known bug(s):",
            groups.len(),
            a.known_bugs.len()
        );
        for (g, o) in groups.iter().zip(&owned) {
            let idx: Vec<usize> = g
                .findings
                .iter()
                .map(|f| texts.iter().position(|t| t == f).unwrap())
                .collect();
            eprintln!("  shard {} owns {:?} findings F{:?}", g.file, o, idx);
        }
        assert!(a.known_bugs.is_empty(), "unowned: {:?}", a.known_bugs);
        assert!(criticals_left_unassigned(&prov, &a.known_bugs).is_empty());
        let shard_of = |i: usize| {
            groups
                .iter()
                .zip(&owned)
                .find(|(g, _)| g.findings.contains(&texts[i]))
                .map(|(g, o)| (g.file.clone(), o.clone()))
                .unwrap()
        };
        assert_eq!(shard_of(0).0, "web/viz.js");
        assert_eq!(
            shard_of(1).0,
            "web/app.js",
            "F1 is owned by the row-building script"
        );
        for i in 5..=8 {
            let (file, o) = shard_of(i);
            assert!(
                o.contains(&"app/drafts.py".to_string()),
                "F{i} on shard {file} must own app/drafts.py: {o:?}"
            );
        }
        for (f, t0) in before {
            let t1 = std::fs::metadata(root.join(&f))
                .and_then(|m| m.modified())
                .unwrap();
            assert_eq!(t0, t1, "attribution modified the archived tree: {f}");
        }
    }

    /// F885 (run 10, round 0, watched live): the app.js shard's worker added every missing DOM
    /// id to index.html — a file it did not own — so grade-what-lands refused the byte-identical
    /// app.js and a CORRECT repair was discarded. A js/css shard owns its page markup too.
    #[test]
    fn a_script_shard_owns_the_markup_it_must_reconcile_with() {
        let files: Vec<String> = [
            "vendorsync/web/app.js",
            "vendorsync/web/index.html",
            "vendorsync/web/styles.css",
            "vendorsync/api.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let taken: std::collections::HashSet<String> =
            ["vendorsync/web/app.js".to_string()].into_iter().collect();
        assert_eq!(
            shard_owned_files("vendorsync/web/app.js", &files, &taken),
            vec![
                "vendorsync/web/app.js".to_string(),
                "vendorsync/web/index.html".to_string()
            ]
        );
        // If a sibling group already owns the html, the partition stays disjoint.
        let taken2: std::collections::HashSet<String> = [
            "vendorsync/web/app.js".to_string(),
            "vendorsync/web/index.html".to_string(),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            shard_owned_files("vendorsync/web/app.js", &files, &taken2),
            vec!["vendorsync/web/app.js".to_string()]
        );
    }
}
