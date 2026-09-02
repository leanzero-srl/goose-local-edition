//! Finding provenance and severity, plus the file-group cluster that consumes findings.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). `FileGroup`, `normalize_rel_path`,
//! `extract_file_from_finding`, `finding_fingerprint`, `dedupe_findings_exact`,
//! `engine_critical` and `group_findings_by_file` moved VERBATIM from swarm.rs with their
//! tests; the provenance/severity vocabulary is new (Mihai, 2026-08-30: "I remember explicitly
//! asking for priorities. How will the nodes know which ones to fix or focus on?").
//!
//! SEVERITY DERIVES FROM PROVENANCE — which engine check AUTHORED the finding — never from
//! matching the finding's text. The r5 receipt for why text needles cannot carry priority: the
//! render gate's console finding (4 errors, a ReferenceError severing the whole scene — the
//! biggest scoring surface) classed as a MINOR known bug while six response-shape contract
//! findings got equal standing, and the fix shard's brief listed its findings unordered. Every
//! authoring site names itself at the moment it pushes; the text is transport, not evidence.
//!
//! MILD: severity ORDERS work and reporting — the wave fans the severest file-group first and
//! every shard brief says what leads and why. It never gates, refuses or aborts. The green claim
//! (`FindingProvenance::partition_criticals`, VA-006 / DESIGN-REPAIR-V2 §4) is the
//! `engine_critical` wording PLUS the browser probe's app-unusable findings — no rows in a real
//! browser, an uncaught exception in the advertised page's boot path — and `passed` further
//! requires no render-class finding among the known active bugs. The sync_rows finding stays
//! repairable-never-blocking per its pinned test even though its authoring check ranks CRITICAL
//! for ordering: it is not a render probe.
//!
//! THE CHECK (REPAIR v2, VA-087): every sourced finding derives a `FindingCheck` — a key that
//! names the same check across gate runs and, when the finding carries it, the gate's own replay
//! command. The repair wave hands the command to the shard as its FIRST action and promotes a
//! shard only when the gate re-run on its merged preview no longer fails that key (and fails no
//! key it did not fail before). An untagged finding has no check: `finding_unverifiable`.

use super::TestRunVerdict;

/// GOOSE_SWARM_COMPLETE_PARALLEL: a group of verify findings that all name the SAME file, so exactly one
/// fix agent ever writes that file (same-file failures serialize by construction).
pub(super) struct FileGroup {
    pub(super) file: String,
    pub(super) findings: Vec<String>,
}

/// Pull the fix-target source file out of a deterministic pytest/tooling finding. Findings are built from
/// `tail_lines` of real pytest/`-m` output (not model text), so the `path.py:N: in ...` and
/// `File "path.py", line N` shapes are stable. Prefers a NON-test source frame (the thing to fix); falls
/// back to the last file seen. Returns None when the finding names no code file (e.g. a missing entry point).
/// Normalize a finding-extracted file path so different spellings of the SAME file (`./x`, `x`, `a//b`,
/// backslashes) collapse to ONE canonical relative string. LOAD-BEARING for GOOSE_SWARM_COMPLETE_PARALLEL:
/// two spellings must NOT become two file-groups -> two shards -> two promotes to the same real dst -> a
/// torn write. Pure + unit-tested.
/// EVERY EXTENSION A DEFECT MAY NAME. One list, because there were two and they disagreed.
///
/// `paths_in` (the RATE reply parser) and `extract_file_from_finding`'s `is_code` (the TEST/verdict text
/// parser) answer the SAME question — "is this token a file this app is made of?" — and answered it
/// differently. `is_code` knew six extensions, so a defect naming `cmd/app/main.go`, `App.tsx`,
/// `Foo.java` or `Note.swift` in backticks — the exact shape the angle prompt DEMANDS — extracted
/// nothing, fell to `unassigned`, and the round degraded to the whole-tree race. A Go or Swift app could
/// not shard a single defect.
///
/// Short extensions (.c, .h) are admitted deliberately: every caller resolves its result against the
/// run's own file list, so a stray `self.c` token can never become a fix target.
pub(super) const FINDING_PATH_EXTS: &[&str] = &[
    ".py", ".pyi", ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".go", ".java", ".kt",
    ".kts", ".rb", ".swift", ".c", ".cc", ".cpp", ".cxx", ".h", ".hpp", ".cs", ".php", ".scala",
    ".ex", ".exs", ".dart", ".lua", ".sh", ".html", ".htm", ".css", ".scss", ".vue", ".svelte",
    ".sql", ".json", ".toml", ".yaml", ".yml", ".md",
];

/// The SOURCE subset. A defect may name `config.yaml` or `README.md` and that is a real path worth
/// reading, but it must never outrank the source file in the same finding: broadening the list above
/// without this made "`app/main.go` mis-parses the flag, see `config.yaml`" aim its fix shard at the
/// config file, because the last path taken wins.
pub(super) const FINDING_SOURCE_EXTS: &[&str] = &[
    ".py", ".pyi", ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".go", ".java", ".kt",
    ".kts", ".rb", ".swift", ".c", ".cc", ".cpp", ".cxx", ".h", ".hpp", ".cs", ".php", ".scala",
    ".ex", ".exs", ".dart", ".lua", ".sh", ".html", ".htm", ".css", ".scss", ".vue", ".svelte",
    ".sql",
];

fn normalize_rel_path(p: &str) -> String {
    p.replace('\\', "/")
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn extract_file_from_finding(finding: &str, all_files: &[String]) -> Option<String> {
    // F862 forensics: .js/.html/.css were missing, so every FRONTEND finding (render-no-rows,
    // DOM TypeError) fell out of per-file fix scoping and collapsed into the unscoped join.
    // The same hole F862 found for .js/.html/.css was still open for Go, Java, Kotlin, Swift, Ruby,
    // C/C++ and .tsx — see FINDING_PATH_EXTS, which this now shares with `paths_in`.
    let is_code = |p: &str| {
        !p.is_empty() && !p.contains(' ') && FINDING_PATH_EXTS.iter().any(|e| p.ends_with(e))
    };
    let mut last: Option<String> = None;
    // TWO SOURCE SLOTS, BECAUSE THE TWO FINDING SHAPES ORDER THEIR PATHS OPPOSITELY. A traceback
    // lists the failing frame LAST and routinely opens on a stdlib frame, so among frames the last
    // wins. An authored finding is written subject-FIRST — r0's TEST defect D5, "Frontend not served
    // ... (in `app/ledgerd.py`, `web/index.html`)", names the server that must serve the page and
    // then the page — so among named paths the first wins. One slot with last-wins sharded D5 to
    // web/index.html, a file that was never wrong, which is the defect the module branch below had
    // already fixed for its own shape ("FIRST match wins").
    let mut frame_src: Option<String> = None;
    let mut named_src: Option<String> = None;
    let mut take = |p: &str, named: bool| {
        // Normalize so two spellings of one file become ONE group (partition invariant).
        let norm = normalize_rel_path(p);
        // A DATA OR DOC FILE NEVER OUTRANKS SOURCE. The shared list admits .json/.yaml/.md so a defect
        // that lives in one is readable at all; without this the LAST path won, and "`app/main.go`
        // mis-parses the flag, see `config.yaml`" aimed its fix shard at the config file.
        let code = FINDING_SOURCE_EXTS.iter().any(|e| norm.ends_with(e));
        if code || last.is_none() {
            last = Some(norm.clone());
        }
        if code && !p.contains("test") && !p.contains("conftest") {
            if named {
                named_src.get_or_insert(norm);
            } else {
                frame_src = Some(norm);
            }
        }
    };
    // THE ATTRIBUTION LIST OUTRANKS THE PROSE. `parse_observed_defects` re-emits the tester's FILES
    // line as a trailing "(in `a`, `b`)" — the author's own answer to "which file", subject-first.
    // Paths in the sentence before it are context: D5 says "despite `web/index.html` existing"
    // three clauses before its list names `app/ledgerd.py` first, so first-wins over the whole
    // sentence would still have aimed the shard at the page instead of the server.
    if let Some((_, list)) = finding
        .trim_end()
        .strip_suffix(')')
        .and_then(|f| f.rsplit_once(" (in `"))
    {
        for chunk in format!("`{list}").split('`').skip(1).step_by(2) {
            let c = chunk.trim();
            if is_code(c) {
                take(c, true);
            }
        }
    }
    for raw in finding.lines() {
        let line = raw.trim();
        let cand: Option<&str> = if let Some((_, rest)) = line.split_once("File \"") {
            rest.split('"').next()
        } else {
            line.split(':').next().map(|t| t.trim())
        };
        if let Some(p) = cand.map(|p| p.trim()) {
            if is_code(p) {
                take(p, false);
            }
        }
        // A PATH IN BACKTICKS, anywhere in the sentence. The line-leading forms above only match a
        // pytest traceback, which is the shape this was written for and the only one it ever saw.
        // Engine-authored findings put the path mid-sentence in backticks —
        // "planned deliverable `vendorsync/store.py` is MISSING", "its deliverable
        // `tests/test_meridian.py` IS written" — and every one of them fell through to `unassigned`.
        for chunk in line.split('`').skip(1).step_by(2) {
            let c = chunk.trim();
            if is_code(c) {
                take(c, true);
            }
        }
        // A PYTEST NODE ID, anywhere in the line. `pytest -q` names the failing file ONLY as
        // `FAILED tests/test_api.py::TestX::test_y - msg` / `ERROR tests/test_api.py::test_z` —
        // a status word first, then the path fused to the test path with `::`. The line-leading
        // split above sees "ERROR tests/test_api.py" (embedded space) and rejects it. MEASURED
        // (run 9, round 0): a finding carrying FIFTEEN such lines attributed to NOTHING, so the
        // round raced whole-tree twins — the 0-for-12 path — instead of handing tests/test_api.py
        // to a focused fixer.
        for tok in line.split_whitespace() {
            if let Some((head, _)) = tok.split_once("::") {
                let h = head.trim();
                if is_code(h) {
                    take(h, true);
                }
            }
        }
    }
    // RESOLVE AGAINST THE FILES THIS RUN OWNS. A pytest traceback names ABSOLUTE paths, and the first
    // frame is routinely CPython's own stdlib —
    // `/opt/homebrew/.../python3.14/threading.py` appeared as the first `File "..."` line in the very
    // first real finding this engine emitted. Unfiltered, that becomes a FileGroup, and
    // `complete_parallel` dispatches a fix shard that owns it: an agent sent to repair CPython, or to
    // write an absolute path into a shadow tree that promotes by relative path.
    //
    // Latent until now because the fan never fired — five of six finding shapes resolved to nothing
    // (F41). Fixing the extractor made this reachable, which is the whole reason "treat 'I fixed
    // that' as a hypothesis" is a rule.
    //
    // With an EMPTY file list there is nothing to resolve against, so the old behaviour stands — that
    // is the unit-test path; every real call site passes the run's planned files.
    if let Some(f) = named_src.clone().or(frame_src).or(last) {
        if all_files.is_empty() {
            return Some(f);
        }
        // Exact, then longest-suffix: a traceback's absolute path ends with the repo-relative one.
        if all_files.iter().any(|a| normalize_rel_path(a) == f) {
            return Some(f);
        }
        if let Some(owned) = all_files
            .iter()
            .map(|a| normalize_rel_path(a))
            .filter(|a| f.ends_with(&format!("/{a}")) || f.ends_with(a.as_str()))
            .max_by_key(|a| a.len())
        {
            return Some(owned);
        }
        // THE REVERSE DIRECTION: the candidate can also be the SHORTER string. r6c: a render
        // probe's console-error attribution is the URL PATH the browser actually fetched the
        // script from — `parsed.pathname`, verbatim (product_probe_v3.mjs `urlToRelPath`) — and
        // a static route commonly serves a subdirectory at the URL root (`static_url_path=''`
        // maps disk `web/viz.js` to `/viz.js`), so the candidate here is `viz.js` while the
        // owned path is `web/viz.js`. The forward rule above never matches that pair (the owned
        // path, not the candidate, is longer), so the finding fell through to `unassigned` and a
        // CRITICAL render finding shipped known-but-unowned for the run's whole life. Resolve
        // only when the match is UNIQUE — two files sharing a basename (`app/__init__.py`,
        // `web/__init__.py`) must stay unresolved rather than guess (the fallback gate).
        let reverse: Vec<String> = all_files
            .iter()
            .map(|a| normalize_rel_path(a))
            .filter(|a| a.ends_with(&format!("/{f}")))
            .collect();
        if reverse.len() == 1 {
            return Some(reverse.into_iter().next().expect("len == 1"));
        }
        // Named a real file, but not one this run owns (stdlib, site-packages, a sibling checkout),
        // or an ambiguous basename. Fall through to the module resolver rather than aiming a fix
        // shard outside the app or guessing between two files.
    }
    // A DOTTED MODULE resolved against the files this run actually planned. The AST review names a
    // module, never a path — "function 'log_message' in module 'vendorsync.api' is a STUB" — and it
    // produced a finding in 3 of 3 measured runs, as did cross-module drift, which names two. Both
    // went to the serial path, so the fan they should have driven had almost nothing to fan.
    //
    // Resolved against `all_files` rather than by string-munging, so a module can only ever map to a
    // file this run really owns — an invented path would send a fix shard at nothing.
    let words: Vec<&str> = finding
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_'))
        .filter(|w| w.contains('.') && !w.is_empty())
        .collect();
    // FIRST match wins. These findings are written subject-first — "function X in module `A` is a
    // STUB", "module `A` reads a field `B` does not define" — so the first module named is the one to
    // repair. Taking the last aimed the drift fix at the module that was CORRECT.
    for w in words {
        let as_path = w.replace('.', "/");
        for f in all_files {
            let norm = normalize_rel_path(f);
            let stem = norm
                .strip_suffix(".py")
                .or_else(|| norm.strip_suffix(".rs"))
                .or_else(|| norm.strip_suffix(".ts"))
                .unwrap_or(&norm);
            if stem == as_path {
                return Some(norm);
            }
        }
    }
    None
}

/// Dedup + group findings by the file they name so each file becomes ONE fix agent (writes partitioned,
/// same-file findings serialized). Returns (groups in first-seen order, unassigned findings that name no
/// file) — the unassigned bucket gets a single serial fallback fix so a file-less finding is not dropped.
/// The one string that decides whether two findings are the SAME defect.
///
/// The angle prefix is stripped — `[bad-input] X` and `[primary-journey] X` are one defect, not two —
/// then every non-alphanumeric run collapses to one space and the body lowercases. Deliberately a
/// WHOLE-BODY fingerprint: matching on a first line would fold two different pytest findings that
/// happen to share a banner, and a FALSE MERGE hides a real defect, which is the one outcome worse than
/// the duplicate this exists to remove.
fn finding_fingerprint(f: &str) -> String {
    let body = match f.strip_prefix('[') {
        Some(rest) => match rest.split_once("] ") {
            Some((_, tail)) => tail,
            None => f,
        },
        None => f,
    };
    body.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fold findings with equal fingerprints, keeping ONE.
///
/// The finding COUNT is the ruler for three decisions at once: it gates green, it fires the stall exit,
/// and it seeds the per-check baseline the wave grades a shard's preview against (`TreeGrade`). One
/// duplicate is charged three times — and in the SAME round it makes the app harder to call green and
/// the shard easier to promote.
///
/// MEASURED (round 0 of a live run): "Empty limit parameter silently falls back to default" was reported
/// by two testers and BOTH survived, because TEST prefixes every defect with `[{angle}] ` and the merge
/// is a `contains` — an exact compare on strings made distinct one line earlier.
///
/// It only ever REMOVES, never rewrites, so every survivor is the exact string the fix fan already knows
/// how to attribute. An ENGINE finding (absent from `model_observed`) always displaces a model-observed
/// twin whichever order they arrived in, because `engine_fatal` forces the engine's own wording CRITICAL
/// and keeping a tester's paraphrase would silently un-force it.
/// The gate's own criticals (P1-9, replacing the deleted RATE call): a finding is CRITICAL when
/// the ENGINE'S OWN WORDING says the app does not run, build or answer — strings written by
/// run_smoke_gate, run_spec_contract and the deliverable stat, never by a model. Everything else
/// ships as a known active bug (still repaired by the wave, never hidden). Pure, and pinned by a
/// truth table over the REAL finding strings the gate emits, so a reworded probe message that
/// silently demotes a dead app to "minor" fails a test instead of a run.
pub(super) fn engine_critical(f: &str) -> bool {
    let l = f.to_lowercase();
    l.contains("does not start")
        || l.contains("never bound")
        || l.contains("does not run at all")
        || l.contains("did not build")
        || l.contains("exited non-zero")
        || l.contains("no such command")
        || l.contains("runtime crash")
        || l.contains("missing deliverable")
        || l.contains("is missing or empty")
        || l.contains("never returns")
        || l.contains("nothing at all once it")
}

/// The planned-deliverable stat's finding, ONE template for the round ruler and the wave's
/// grader (two copies once disagreed by a word and the keys they grade by would have too).
pub(super) fn missing_deliverable_finding(file: &str) -> String {
    format!(
        "planned deliverable `{file}` is MISSING or EMPTY — a task was marked done without \
         writing it. Create it (the simplest version that satisfies the spec) so the app is \
         complete and runnable."
    )
}

/// GOOSE_SWARM_REQUIRE_TESTS: an app that ships NO executable tests must not read as GREEN.
///
/// `interpret_pytest_run` already distinguishes `NoTests` (exit 5) from `Pass` (exit 0), but only `Failures`
/// pushed a finding — so "nothing was checked" was indistinguishable from "everything passed". The review-fix
/// gate already gets this right (it requires `TestRunVerdict::Pass`, never merely-empty findings); this brings
/// the completion gate to the same standard.
///
/// `on == false` returns None on every input => no finding is ever pushed => byte-identical. Pure so the
/// distinction is unit-testable without a python3 on the box.
///
/// PROVENANCE (VA-136, the root cause of VA-134's r6h row): this finding carries its OWN source,
/// `FindingSource::RequireTests` (class MEDIUM — the class the green partition already gave it:
/// no `engine_critical` wording, not a render probe), tagged by `tag_require_tests` at the smoke
/// gate BEFORE the gate's batch tag. Until VA-136 it rode that batch tag
/// (`FindingSource::SmokeGate`, CRITICAL), so its label read `critical` on a minor;
/// `verdict_severity_mismatches` is the instrument that made that loud, and it stays.
pub(super) fn require_tests_finding(verdict: &TestRunVerdict, on: bool) -> Option<String> {
    if !on {
        return None;
    }
    matches!(verdict, TestRunVerdict::NoTests).then(|| {
        "the app ships NO executable tests (`pytest -q` collected 0) — an empty suite is not a passing \
         suite. Write real tests that assert the spec's concrete expected values, and never delete or \
         skip a failing test to go green."
            .to_string()
    })
}

/// VA-136: the no-executable-tests finding's own tag, called at the smoke gate AHEAD of the
/// gate's batch tag — `FindingProvenance::tag` is first-writer-wins (`or_insert`), so the tag
/// that lands first owns the text and a later batch tag cannot relabel it. Keyed on the
/// finding's exact text, the registry's own transport key: tagged only when the gate actually
/// pushed it (the lever on and `TestRunVerdict::NoTests`, the one verdict that authors it).
pub(super) fn tag_require_tests(prov: &mut FindingProvenance, findings: &[String]) {
    if let Some(text) =
        require_tests_finding(&TestRunVerdict::NoTests, true).filter(|t| findings.contains(t))
    {
        prov.tag(FindingSource::RequireTests, std::slice::from_ref(&text));
    }
}

/// Does a browser console line name an UNCAUGHT JS EXCEPTION — `ReferenceError: x is not
/// defined`, `TypeError: Illegal invocation`, `Uncaught SyntaxError …` — rather than a resource
/// failure (`Failed to load resource: net::ERR_EMPTY_RESPONSE`) or an app's own console.error
/// text? The classes are the LANGUAGE'S: a capitalized token ending in `Error` directly followed
/// by `:`, or the `Uncaught` prefix Chromium prints for a pageerror. Derived from the line the
/// probe recorded, so the render gate can name the authoring branch (`RenderGateException`) at
/// the moment it pushes — provenance, not a needle on the finding afterwards.
pub(super) fn console_error_is_exception(line: &str) -> bool {
    line.split_whitespace().any(|t| {
        let t = t.trim_start_matches(['(', '[', '"', '\'']);
        t == "Uncaught"
            || t.strip_suffix(':').is_some_and(|name| {
                name.ends_with("Error")
                    && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            })
    })
}

/// The CHECK behind a finding — what the shard re-runs FIRST (DESIGN-REPAIR-V2 §1) and what the
/// gate re-runs on the merged preview to decide promotion (§2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FindingCheck {
    /// The check's identity ACROSS gate runs: the PROBE that ran (`FindingSource::probe` — the
    /// render gate's console is one check whether its first error is an uncaught exception or a
    /// resource failure) plus the finding template's own first clause, parenthesized spans
    /// dropped and digit runs normalized. The gate re-run on a preview boots the app on a fresh
    /// port, re-numbers a shifted line and quotes whichever console error is first NOW — none
    /// of that is a different check. What stays is the subject the template names outside
    /// parentheses: the endpoint, the DOM id, the module, the deliverable path.
    pub(super) key: String,
    /// The check as the gate states it in the finding's own words — its `GATE COMMAND` /
    /// `REPLAY IT` sentence, or the leading backticked command — verbatim. None when the
    /// authoring check is not a command a shard can run by hand (a static scan, a deliverable
    /// stat): the brief then names the check and says the engine re-runs it on the edit.
    pub(super) command: Option<String>,
}

/// The identity of a check across gate runs: the PROBE that ran (`FindingSource::probe`, never
/// the severity-branch label) plus the finding's first clause. Parenthesized spans go (the
/// quoted exemplar, the `(in \`file\`)` attribution, `(exit 1)`), digit runs become `#` (ports,
/// line numbers, counts), and the key is the first clause — the engine's templates separate the
/// claim from its elaboration with ` — `, a sentence end or a newline. Lowercased and
/// whitespace-collapsed.
pub(super) fn check_key(source: FindingSource, text: &str) -> String {
    let mut depth = 0usize;
    let mut in_digits = false;
    let mut flat = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '(' {
            depth += 1;
            in_digits = false;
            continue;
        }
        if c == ')' {
            depth = depth.saturating_sub(1);
            in_digits = false;
            continue;
        }
        if depth > 0 {
            continue;
        }
        if c.is_ascii_digit() {
            if !in_digits {
                flat.push('#');
                in_digits = true;
            }
            continue;
        }
        in_digits = false;
        flat.push(c);
    }
    let mut head = flat.as_str();
    for sep in [" — ", "\n", ". ", ": "] {
        if let Some((h, _)) = head.split_once(sep) {
            head = h;
        }
    }
    let norm = head
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    format!("{} | {}", source.probe(), norm)
}

/// The gate's own replay, quoted from the finding: the sentence from its `GATE COMMAND` or
/// `REPLAY IT:` marker (both written by the engine's probes) to the end, minus the trailing
/// attribution suffix `extract_file_from_finding` parses; else the command a finding OPENS with
/// (`` `pytest -q` failed … ``, `` `python3 -m app --help` failed … ``). None for a finding that
/// carries no command — said as such, never invented.
pub(super) fn check_command(text: &str) -> Option<String> {
    for marker in ["GATE COMMAND", "REPLAY IT:"] {
        // `find` offsets are char boundaries; `get` says so — `None` falls through as an absent marker does.
        if let Some(rest) = text.find(marker).and_then(|i| text.get(i..)) {
            let rest = rest.rsplit_once(" (in `").map(|(a, _)| a).unwrap_or(rest);
            return Some(rest.trim().trim_end_matches('.').to_string());
        }
    }
    let mut parts = text.splitn(3, '`');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(""), Some(cmd), Some(_)) if !cmd.trim().is_empty() => Some(cmd.trim().to_string()),
        _ => None,
    }
}

pub(super) fn dedupe_findings_exact(
    findings: &[String],
    model_observed: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut keep: Vec<String> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for f in findings {
        let fp = finding_fingerprint(f);
        if fp.is_empty() {
            keep.push(f.clone());
            continue;
        }
        match at.get(&fp) {
            None => {
                at.insert(fp, keep.len());
                keep.push(f.clone());
            }
            Some(&i) => {
                if model_observed.contains(&keep[i]) && !model_observed.contains(f) {
                    keep[i] = f.clone();
                }
            }
        }
    }
    keep
}

pub(super) fn group_findings_by_file(
    findings: &[String],
    all_files: &[String],
) -> (Vec<FileGroup>, Vec<String>) {
    let mut order: Vec<String> = Vec::new();
    let mut by_file: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut unassigned: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for f in findings {
        if !seen.insert(f.clone()) {
            continue;
        }
        match extract_file_from_finding(f, all_files) {
            Some(file) => {
                if !by_file.contains_key(&file) {
                    order.push(file.clone());
                }
                by_file.entry(file).or_default().push(f.clone());
            }
            None => unassigned.push(f.clone()),
        }
    }
    let groups = order
        .into_iter()
        .map(|file| {
            let findings = by_file.remove(&file).unwrap_or_default();
            FileGroup { file, findings }
        })
        .collect();
    (groups, unassigned)
}

/// The ordering vocabulary, most severe first. Ranks order work and reporting only — no tier
/// gates anything (the green claim stays with `engine_critical` above).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum FindingSeverity {
    /// An engine product check proved the app unusable: never bound, holds zero rows against
    /// vendor truth, the primary journey is dead in a real browser.
    Critical,
    /// Feature-severing evidence: a console error with an attributed source file, a guaranteed
    /// runtime null/AttributeError, a public API returning a fraction of the collection.
    High,
    /// Contract/shape findings: a documented response field missing, idempotency/ETag
    /// behavior, a latent no-timeout call.
    Medium,
    /// Cosmetic/advisory: the page works but ships unstyled.
    Low,
}

impl FindingSeverity {
    pub(super) fn label(self) -> &'static str {
        match self {
            FindingSeverity::Critical => "critical",
            FindingSeverity::High => "high",
            FindingSeverity::Medium => "medium",
            FindingSeverity::Low => "low",
        }
    }
}

/// WHICH ENGINE CHECK authored a finding. One variant per authoring check (or per authoring
/// branch where one check writes two different classes — the POST probe's hang/dark findings
/// are a different class from its shape findings). Severity is a pure function of this enum,
/// so the SAME text from two different checks ranks differently and two different texts from
/// one check rank the same — provenance, never text matching.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FindingSource {
    /// run_spec_contract's entry spawn: an advertised invocation never bound its port.
    BootProbe,
    /// run_smoke_gate: the build/test/entry oracle (did not build, entry exited non-zero,
    /// failing tests).
    SmokeGate,
    /// run_smoke_gate under GOOSE_SWARM_REQUIRE_TESTS: the app ships NO executable tests
    /// (`pytest -q` collected 0). Its own source since VA-136: riding the smoke gate's batch tag
    /// labelled it `critical` while the green partition ships it as a minor — r6h's one
    /// `verdict_severity_mismatch` row (VA-134).
    RequireTests,
    /// A planned task FAILED and the smoke gate was blind — a deterministic engine event.
    FailedTask,
    /// The planned-deliverable stat: a source deliverable is missing/empty on disk.
    MissingDeliverable,
    /// The POST probe's dead branches: an advertised endpoint hangs past the spec's own
    /// budget, or reads go dark once the app holds rows.
    EndpointDeadProbe,
    /// Row-acquisition truth: the app holds zero rows while the vendor's collection does not
    /// (sync_rows, acquired-but-not-persisted, the fire-and-forget sync).
    SyncAcquisition,
    /// The render gate's journey rows: no rows in a real browser, no working sync control,
    /// zero rows after a successful sync.
    RenderGateRows,
    /// The restart-durability probe: never binds again on its own database, or loses rows.
    RestartDurability,
    /// The render gate's console: JS errors in normal use that are NOT exceptions (a resource
    /// that failed to load, an app's own console.error).
    RenderGateConsole,
    /// The render gate's console: an UNCAUGHT EXCEPTION (`ReferenceError`, `TypeError`, …) during
    /// the advertised page's boot/probe — the whole script after it never ran (r5: one
    /// `ReferenceError: onBrushChangeTracked is not defined` severed the entire 3D scene and
    /// shipped as a MINOR known bug; VA-006 ruled this class CRITICAL by construction).
    RenderGateException,
    /// The client public-API paging probe: a direct caller gets a fraction of the collection.
    ClientApiPaging,
    /// The cross-module drift scan: a module reads a field a sibling never defines.
    CrossModuleDrift,
    /// The dom-id contract scan: js references an id no html defines.
    DomIdScan,
    /// The endpoint contract probe: response shape/behavior against the spec's own table
    /// (missing documented fields, unreadable JSON, idempotency, ETag cheapness, 4xx/5xx).
    EndpointContractProbe,
    /// The aggregate-truth probe: recomputed aggregates disagree with the app's own endpoints.
    AggregateTruth,
    /// The http-timeout scan: an outbound call with no timeout (latent block-forever).
    HttpTimeoutScan,
    /// The render gate's styling conjunction: the page renders with browser-default styling.
    RenderGateStyling,
    /// The css-coherence scan: stylesheet vocabulary and markup disagree.
    CssCoherenceScan,
}

impl FindingSource {
    /// The checks that measured the SERVER'S ANSWER over HTTP — a response shape, a status, a
    /// hang, a row count read back through the app's own endpoints. A finding of this class
    /// describes what a HANDLER returned, so attribution ranks server-side source above web
    /// assets for it (r6c F5: `POST /api/drafts`'s response-shape finding was won by web/app.js
    /// — three fetch() literals — over the route table, so a FRONTEND shard carried a
    /// server-side defect and its two correct edits to app/drafts.py died at promotion). The
    /// browser-side checks (render, console, dom-id, css) are deliberately absent: their
    /// defects live in the assets.
    pub(super) fn is_server_response_probe(self) -> bool {
        matches!(
            self,
            FindingSource::BootProbe
                | FindingSource::EndpointDeadProbe
                | FindingSource::SyncAcquisition
                | FindingSource::RestartDurability
                | FindingSource::EndpointContractProbe
                | FindingSource::AggregateTruth
        )
    }

    /// RENDER-CLASS: the browser probe's findings about whether a spec-advertised surface WORKS
    /// in a real browser — rows rendered, exceptions thrown, console errors hit. `passed`
    /// (VA-006) requires none of these among the known active bugs; the probe's styling
    /// conjunction is its cosmetic reading and stays advisory (Low), so it is not in this class.
    pub(super) fn is_render_probe(self) -> bool {
        matches!(
            self,
            FindingSource::RenderGateRows
                | FindingSource::RenderGateConsole
                | FindingSource::RenderGateException
        )
    }

    /// THE DERIVATION TABLE. Class rules: app-unusable product checks are CRITICAL;
    /// feature-severing evidence is HIGH; contract/shape is MEDIUM (the spec's test contract
    /// too — an empty suite verifies nothing, but the app runs: `RequireTests`, the class the
    /// green partition already gives it); cosmetic is LOW.
    pub(super) fn severity(self) -> FindingSeverity {
        match self {
            FindingSource::BootProbe
            | FindingSource::SmokeGate
            | FindingSource::FailedTask
            | FindingSource::MissingDeliverable
            | FindingSource::EndpointDeadProbe
            | FindingSource::SyncAcquisition
            | FindingSource::RenderGateRows
            | FindingSource::RenderGateException
            | FindingSource::RestartDurability => FindingSeverity::Critical,
            FindingSource::RenderGateConsole
            | FindingSource::ClientApiPaging
            | FindingSource::CrossModuleDrift
            | FindingSource::DomIdScan => FindingSeverity::High,
            FindingSource::EndpointContractProbe
            | FindingSource::AggregateTruth
            | FindingSource::HttpTimeoutScan
            | FindingSource::RequireTests => FindingSeverity::Medium,
            FindingSource::RenderGateStyling | FindingSource::CssCoherenceScan => {
                FindingSeverity::Low
            }
        }
    }

    /// The CHECK that ran — the identity `check_key` is built on. Where one check writes two
    /// classes (the render gate's console: an uncaught exception vs. any other console error;
    /// the POST probe: a hang/dark read vs. a response shape), both classes name the ONE probe:
    /// a shard that turns `ReferenceError: x is not defined` into a resource-load error fixed
    /// the exception and left the console check failing — the same check, fewer or equal
    /// failures — never a check that "passed on the tree and fails on the preview" (VA-098:
    /// keyed on `label()`, that edit read as `preview_regressed`).
    pub(super) fn probe(self) -> &'static str {
        match self {
            FindingSource::BootProbe => "boot probe",
            FindingSource::SmokeGate | FindingSource::RequireTests => "smoke gate",
            FindingSource::FailedTask => "failed planned task",
            FindingSource::MissingDeliverable => "planned-deliverable stat",
            FindingSource::EndpointDeadProbe | FindingSource::EndpointContractProbe => {
                "endpoint probe"
            }
            FindingSource::SyncAcquisition => "sync acquisition probe",
            FindingSource::RenderGateRows => "render gate rows",
            FindingSource::RestartDurability => "restart durability probe",
            FindingSource::RenderGateConsole | FindingSource::RenderGateException => {
                "render gate console"
            }
            FindingSource::ClientApiPaging => "client public-API paging probe",
            FindingSource::CrossModuleDrift => "cross-module drift scan",
            FindingSource::DomIdScan => "dom-id contract scan",
            FindingSource::AggregateTruth => "aggregate-truth probe",
            FindingSource::HttpTimeoutScan => "http timeout scan",
            FindingSource::RenderGateStyling => "render gate styling",
            FindingSource::CssCoherenceScan => "css coherence scan",
        }
    }

    /// The authoring check's name as events and briefs print it.
    pub(super) fn label(self) -> &'static str {
        match self {
            FindingSource::BootProbe => "boot probe (advertised entry never bound)",
            FindingSource::SmokeGate => "smoke gate (build/test/entry oracle)",
            FindingSource::RequireTests => "smoke gate (no executable tests)",
            FindingSource::FailedTask => "failed planned task (engine event)",
            FindingSource::MissingDeliverable => "planned-deliverable stat (missing/empty)",
            FindingSource::EndpointDeadProbe => "endpoint dead probe (hang / dark reads)",
            FindingSource::SyncAcquisition => "sync acquisition probe (zero rows vs vendor)",
            FindingSource::RenderGateRows => "render gate rows (journey dead in a browser)",
            FindingSource::RestartDurability => "restart durability probe",
            FindingSource::RenderGateConsole => "render gate console (browser JS errors)",
            FindingSource::RenderGateException => {
                "render gate exception (uncaught JS exception in the page's boot path)"
            }
            FindingSource::ClientApiPaging => "client public-API paging probe",
            FindingSource::CrossModuleDrift => "cross-module drift scan",
            FindingSource::DomIdScan => "dom-id contract scan",
            FindingSource::EndpointContractProbe => "endpoint contract probe (response shape)",
            FindingSource::AggregateTruth => "aggregate-truth probe",
            FindingSource::HttpTimeoutScan => "http timeout scan",
            FindingSource::RenderGateStyling => "render gate styling (page unstyled)",
            FindingSource::CssCoherenceScan => "css coherence scan",
        }
    }
}

/// The per-round provenance registry: exact finding text → the check that authored it. The
/// text is only the TRANSPORT key (dedupe removes, attribution clones, nothing rewrites), so
/// an exact-identity lookup carries the authorship through the pipeline without any pattern
/// matching on content. An untagged finding is a LOUD named absence — "unsourced" — reported
/// as itself and ordered LAST (it cannot claim priority it cannot prove), never silently
/// defaulted into a tier (the fallback gate).
#[derive(Default)]
pub(super) struct FindingProvenance {
    by_text: std::collections::HashMap<String, FindingSource>,
}

/// Ordering rank: 0..=3 from the severity table, 4 for unsourced.
const UNSOURCED_RANK: u8 = 4;

/// The loud named absence `source_label` reports for an untagged finding. Shared with
/// fix_order_note, whose sentence must branch on it: "authored by the unsourced (no authoring
/// check recorded)" is not English, and that text reaches a model (the specificity gate).
const UNSOURCED_SOURCE: &str = "unsourced (no authoring check recorded)";

impl FindingProvenance {
    /// Author + push in one move: the site that writes the finding names itself.
    pub(super) fn push(&mut self, findings: &mut Vec<String>, source: FindingSource, text: String) {
        self.by_text.entry(text.clone()).or_insert(source);
        findings.push(text);
    }

    /// Tag a batch another function authored (the round loop's per-check extends).
    pub(super) fn tag(&mut self, source: FindingSource, texts: &[String]) {
        for t in texts {
            self.by_text.entry(t.clone()).or_insert(source);
        }
    }

    /// Merge a callee's registry (run_spec_contract returns its own). First writer wins,
    /// matching dedupe's keep-first.
    pub(super) fn absorb(&mut self, other: FindingProvenance) {
        for (t, s) in other.by_text {
            self.by_text.entry(t).or_insert(s);
        }
    }

    pub(super) fn severity_label(&self, text: &str) -> &'static str {
        match self.by_text.get(text) {
            Some(s) => s.severity().label(),
            None => "unsourced",
        }
    }

    pub(super) fn source_label(&self, text: &str) -> &'static str {
        match self.by_text.get(text) {
            Some(s) => s.label(),
            None => UNSOURCED_SOURCE,
        }
    }

    /// The authoring check itself, for callers that branch on the CLASS of a finding
    /// (attribution's server-side preference); None for an untagged finding — the caller
    /// treats that as "no preference", never as a class.
    pub(super) fn source_of(&self, text: &str) -> Option<FindingSource> {
        self.by_text.get(text).copied()
    }

    /// The check behind a finding (`FindingCheck`). None only for an untagged finding — no
    /// authoring check is recorded, so nothing can be re-run for it; the wave says
    /// `finding_unverifiable` and keeps the labelled count comparison for that finding alone.
    pub(super) fn check_of(&self, text: &str) -> Option<FindingCheck> {
        let source = self.source_of(text)?;
        Some(FindingCheck {
            key: check_key(source, text),
            command: check_command(text),
        })
    }

    /// THE GREEN PARTITION (VA-006, DESIGN-REPAIR-V2 §4). A finding is critical when the
    /// engine's own wording says the app does not run/build/answer (`engine_critical`), OR when
    /// the browser probe of a spec-advertised surface found the app unusable there — no rows in
    /// a real browser, an uncaught exception in the page's boot path. r5 shipped `passed:true`
    /// with `ReferenceError: onBrushChangeTracked is not defined` as a minor known bug; r6c with
    /// `TypeError: Illegal invocation` and an empty table as two "criticals" the partition never
    /// saw, because it read the TEXT class alone. Provenance decides the render half — never a
    /// literal like `viz3d` — and the sync_rows probe stays repairable-never-blocking (its
    /// pinned test): it is not a render probe.
    pub(super) fn is_critical(&self, text: &str) -> bool {
        engine_critical(text)
            || self
                .source_of(text)
                .is_some_and(|s| s.is_render_probe() && s.severity() == FindingSeverity::Critical)
    }

    /// The round's green partition, split once — the two Vecs are always consumed together
    /// (final_passed / known_active_bugs).
    pub(super) fn partition_criticals(&self, findings: &[String]) -> (Vec<String>, Vec<String>) {
        findings.iter().cloned().partition(|f| self.is_critical(f))
    }

    /// Render-class (`FindingSource::is_render_probe`): the findings `passed` may not ship as
    /// known active bugs.
    pub(super) fn is_render_class(&self, text: &str) -> bool {
        self.source_of(text).is_some_and(|s| s.is_render_probe())
    }

    /// VA-134: the verdict's two readings of one shipped bug, made LOUD where they disagree.
    /// `partition_criticals` (green) and `severity_label` (provenance) are different functions
    /// of the same finding: r6h shipped `passed: true` with ONE known active bug — the
    /// no-executable-tests finding — whose label read `critical` because it rides the smoke
    /// gate's batch tag. The label never blocks `passed` (the least-impact law: r6h is the
    /// golden run and its verdict stands); instead every known bug whose label is `critical`
    /// or `high` gets ONE `verdict_severity_mismatch{finding, partition, label, source}` row,
    /// and `complete_result` carries `mismatched: N` (present only when N > 0). `minors` is the
    /// partition's minor half in ship order; the rows come back in the same order.
    pub(super) fn verdict_severity_mismatches(&self, minors: &[String]) -> Vec<serde_json::Value> {
        minors
            .iter()
            .filter_map(|m| {
                let label = self.severity_label(m);
                if label != "critical" && label != "high" {
                    return None;
                }
                Some(serde_json::json!({
                    "event": "verdict_severity_mismatch",
                    "finding": m.chars().take(160).collect::<String>(),
                    "partition": "minor",
                    "label": label,
                    "source": self.source_label(m),
                }))
            })
            .collect()
    }

    fn rank(&self, text: &str) -> u8 {
        match self.by_text.get(text) {
            Some(s) => s.severity() as u8,
            None => UNSOURCED_RANK,
        }
    }

    /// Most-severe-first, STABLE — within a tier the detector order still decides, and the
    /// exact strings are never rewritten (finding_texts stays byte-identical per finding for
    /// its existing readers; only the order changes).
    pub(super) fn sort_findings(&self, findings: &mut [String]) {
        findings.sort_by_key(|f| self.rank(f));
    }

    /// The wave's dispatch order: max severity first, then more findings, stable. When
    /// file-groups exceed free nodes, fanout_over_fleet hands devices to the FIRST items, so
    /// this order IS which files the fleet repairs first.
    pub(super) fn sort_groups(&self, groups: &mut [FileGroup]) {
        groups.sort_by_key(|g| {
            let best = g
                .findings
                .iter()
                .map(|f| self.rank(f))
                .min()
                .unwrap_or(UNSOURCED_RANK);
            (best, usize::MAX - g.findings.len())
        });
    }

    /// The shard brief's fix-first instruction, assembled from THIS group's real facts: the
    /// numbered positions, the derived severity and the authoring check per contiguous run.
    /// `ordered` must already be most-severe-first (sort_findings). Ends with a blank line so
    /// it prepends cleanly to smoke_fix_description; no line starts `N. `, so
    /// parse_numbered_findings still finds the numbered block untouched.
    pub(super) fn fix_order_note(&self, ordered: &[String]) -> String {
        if ordered.is_empty() {
            return String::new();
        }
        let mut runs: Vec<(usize, usize, &'static str, &'static str)> = Vec::new();
        for (i, f) in ordered.iter().enumerate() {
            let sev = self.severity_label(f);
            let src = self.source_label(f);
            match runs.last_mut() {
                Some((_, end, s, c)) if *s == sev && *c == src => *end = i,
                _ => runs.push((i, i, sev, src)),
            }
        }
        let lines: Vec<String> = runs
            .iter()
            .map(|(a, b, sev, src)| {
                let subject = if a == b {
                    format!("finding {} is", a + 1)
                } else {
                    format!("findings {}-{} are", a + 1, b + 1)
                };
                let authorship = if *src == UNSOURCED_SOURCE {
                    "with no authoring check recorded".to_string()
                } else {
                    format!("authored by the {src}")
                };
                format!("- {subject} {} — {authorship}", sev.to_uppercase())
            })
            .collect();
        format!(
            "FIX IN THIS ORDER. The {} numbered finding(s) below are ordered most-severe-first \
             by the engine check that authored each:\n{}\nClose the earlier findings before \
             touching a later one — the severity is derived from which check measured it, and \
             the earliest findings are the ones that stop the app doing its job.\n\n",
            ordered.len(),
            lines.join("\n")
        )
    }
}

/// Shorten a long string by keeping its HEAD and its TAIL and eliding the middle. Char-based, so it
/// never splits a multi-byte character.
///
/// Head-only truncation keeps the part that says WHAT was checked and discards the part that says what
/// went WRONG, because diagnostics — tracebacks, pytest error banners, argparse dispatch tails — live at
/// the end. Two separate defects in swarm.rs came from that: a flat 2000-char head cut hid the dispatch
/// tail of every real entry point and fabricated "unwired/unreachable" findings, and a 400-char head cut
/// on `complete_verify.finding_texts` rendered a pytest collect failure as a list of SUCCESSFULLY
/// collected tests followed by `================` — the error banner beginning exactly where the
/// truncation ended. That finding was unreadable, and it was the only evidence for the round.
pub(super) fn elide_middle(s: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= head + tail {
        return s.to_string();
    }
    let h: String = chars[..head].iter().collect();
    let t: String = chars[chars.len() - tail..].iter().collect();
    format!("{h}\n\n... [middle elided — head + tail shown] ...\n\n{t}")
}

/// The numbered findings block out of a `smoke_fix_description` — the contiguous `1. …` `2. …`
/// run the shard prompt itself demands verdict lines against. Recovered from the description
/// because the dispatcher's epilogue (where the repair ledger is written) has the description in
/// hand but not the findings vec it was built from; round-tripping our own format is exact.
///
/// r6c: a finding's OWN text is routinely multi-line (a pytest tail-lines finding carries a
/// whole traceback), and the old parser BROKE on the first such continuation line — silently
/// truncating `findings_assigned[]`, the ledger's only durable record of what a shard owned,
/// even though the live PROMPT (built straight from the findings vec) still carried all of
/// them. The template's blank line after `{numbered}` is the real terminator; a non-"N. " line
/// before it is a continuation of the finding just pushed, never the end of the block.
pub(super) fn parse_numbered_findings(desc: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in desc.lines() {
        let t = line.trim();
        let want = format!("{}. ", out.len() + 1);
        if let Some(rest) = t.strip_prefix(&want) {
            out.push(rest.to_string());
        } else if !out.is_empty() {
            if t.is_empty() {
                break;
            }
            if let Some(last) = out.last_mut() {
                last.push('\n');
                last.push_str(line);
            }
        }
    }
    out
}

/// The `FINDING n: FIXED / NOT FIXED / NOT REAL — detail` lines the shard prompt has demanded
/// since the numbered-verdict protocol shipped — and which NOTHING parsed until now, so a round
/// N+1 shard re-tried what round N had already reported. Order matters: `NOT FIXED` and `NOT
/// REAL` are tested before `FIXED` because both contain it. Tolerant of markdown litter
/// (leading `-`/`*`/`#`, `**bold**`) because the reporter is a weak model, not a serializer.
pub(super) fn parse_finding_verdicts(output: &str) -> Vec<(u32, &'static str, String)> {
    let mut out = Vec::new();
    for line in output.lines() {
        let t = line
            .trim()
            .trim_start_matches(['-', '*', '#', '>', ' '])
            .trim_start_matches("**");
        let Some(rest) = t
            .strip_prefix("FINDING ")
            .or_else(|| t.strip_prefix("Finding "))
        else {
            continue;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(n) = digits.parse::<u32>() else {
            continue;
        };
        let after = rest
            .get(digits.len()..)
            .unwrap_or("")
            .trim_start_matches("**")
            .trim_start_matches([':', ' '])
            .trim_start_matches("**")
            .trim();
        let upper = after.to_uppercase();
        let verdict = if upper.starts_with("NOT FIXED") {
            "NOT FIXED"
        } else if upper.starts_with("NOT REAL") {
            "NOT REAL"
        } else if upper.starts_with("FIXED") {
            "FIXED"
        } else {
            continue;
        };
        let detail = after
            .get(verdict.len()..)
            .unwrap_or("")
            .trim_start_matches(['*', ' ', '—', '-', ':'])
            .trim();
        out.push((n, verdict, super::tail_chars(detail, 300)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// The finding COUNT gates green, fires the stall exit, AND seeds the wave's per-check baseline
    /// (`TreeGrade`) — so one duplicate is charged three times, making the app harder to call green
    /// and the shard easier to promote in the same round.
    ///
    /// MEASURED: two testers reported "Empty limit parameter silently falls back to default" and both
    /// survived, because TEST prefixes every defect with `[{angle}] ` and the merge was a `contains`.
    fn the_same_defect_from_two_testers_counts_once() {
        let engine = "vendorsync/api.py:17 references DOM id `prev-btn`".to_string();
        let a = "[bad-input] Empty limit parameter silently falls back to default".to_string();
        let b =
            "[primary-journey] Empty limit parameter silently falls back to default".to_string();
        let model: std::collections::HashSet<String> = [a.clone(), b.clone()].into_iter().collect();

        let out = dedupe_findings_exact(&[a.clone(), engine.clone(), b.clone()], &model);
        assert_eq!(
            out.len(),
            2,
            "the angle prefix must not make one defect two: {out:?}"
        );
        assert!(out.contains(&engine), "an unrelated finding must survive");

        // AN ENGINE FINDING IS ALWAYS THE SURVIVOR, in either arrival order — engine_fatal forces its
        // wording CRITICAL and keeping a tester's paraphrase would silently un-force it.
        let measured = "the served page renders NO data rows".to_string();
        let paraphrase = "[primary-journey] the served page renders NO data rows".to_string();
        let only_model: std::collections::HashSet<String> =
            [paraphrase.clone()].into_iter().collect();
        for pair in [
            vec![paraphrase.clone(), measured.clone()],
            vec![measured.clone(), paraphrase.clone()],
        ] {
            let out = dedupe_findings_exact(&pair, &only_model);
            assert_eq!(
                out,
                vec![measured.clone()],
                "the engine string must survive"
            );
        }

        // Two DIFFERENT defects that share a file must both stand — a false merge hides a real defect,
        // which is worse than the duplicate this removes.
        let x = "[bad-input] `--db` pointing at a directory crashes".to_string();
        let y = "[bad-input] `--db` pointing at a non-SQLite file crashes".to_string();
        assert_eq!(dedupe_findings_exact(&[x, y], &model).len(), 2);

        // Nothing to fold leaves the list byte-identical.
        let solo = vec!["only one".to_string()];
        assert_eq!(dedupe_findings_exact(&solo, &model), solo);
    }

    #[test]
    fn extract_file_prefers_source_over_test_and_none_when_absent() {
        let f = "tests/test_cli.py:9: in test_add\n from spendlog.cli import main\n\
                 spendlog/cli.py:3: in <module>\n    import missing\nE   ModuleNotFoundError"
            .to_string();
        assert_eq!(
            extract_file_from_finding(&f, &[]).as_deref(),
            Some("spendlog/cli.py"),
            "a non-test source frame is the fix target"
        );
        assert_eq!(
            extract_file_from_finding("no python3 -m app entry point found", &[]),
            None
        );
        // `File "path", line N` shape (pytest full traceback / rust).
        assert_eq!(
            extract_file_from_finding("File \"app/core.py\", line 7, in run", &[]).as_deref(),
            Some("app/core.py")
        );
    }

    /// r0 (2026-08-29) TEST defect D5, verbatim: the tester's FILES line, re-emitted by
    /// `parse_observed_defects` as the trailing "(in ...)" list, names the SERVER first and the page
    /// second — the fix lives in ledgerd.py, which must serve the page. Last-wins sharded it to
    /// web/index.html; so would first-wins over the whole sentence, because the prose mentions the
    /// page ("despite `web/index.html` existing") before the list. The attribution list outranks
    /// the prose, and within it — and within any authored sentence — the first source path wins.
    #[test]
    fn an_authored_finding_shards_to_the_first_file_in_its_attribution_list() {
        let files: Vec<String> = [
            "app/__init__.py",
            "app/ledgerd.py",
            "app/notifierd.py",
            "web/index.html",
            "web/app.js",
            "tests/test_api.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let d5 = "[primary-journey] Frontend not served at all. `GET /` returns `{\"error\": \"Not \
                  found\"}` instead of the HTML console page. The app cannot be used as a payments \
                  console because there's no UI being served, despite `web/index.html` existing. \
                  Command: `curl -s http://127.0.0.1:18400/` (in `app/ledgerd.py`, `web/index.html`)";
        assert_eq!(
            extract_file_from_finding(d5, &files).as_deref(),
            Some("app/ledgerd.py"),
            "D5's fix lives in the server that must serve the page"
        );
        // The same shape with the attribution suffix spelled literally (the TEST fan that used
        // to author it is deleted; authored findings still arrive in this shape).
        assert_eq!(
            extract_file_from_finding(
                "Frontend not served at all, despite `web/index.html` existing (in `app/ledgerd.py`, `web/index.html`)",
                &files
            )
            .as_deref(),
            Some("app/ledgerd.py")
        );
        // A test file first in the list never outranks the source that follows it.
        assert_eq!(
            extract_file_from_finding(
                "/api/health 500s on an empty db (in `tests/test_api.py`, `app/ledgerd.py`)",
                &files
            )
            .as_deref(),
            Some("app/ledgerd.py")
        );
        // Without a list, an authored sentence is still subject-first.
        assert_eq!(
            extract_file_from_finding(
                "`app/ledgerd.py` returns the wrong shape; `app/notifierd.py` is correct",
                &files
            )
            .as_deref(),
            Some("app/ledgerd.py")
        );
        // THE DATA/DOC RULE STANDS: a source path anywhere beats a config file named first.
        assert_eq!(
            extract_file_from_finding(
                "see `config.yaml`: `app/ledgerd.py` mis-parses the flag",
                &files
            )
            .as_deref(),
            Some("app/ledgerd.py")
        );
    }

    /// A traceback orders its frames the OTHER way: the failing frame is LAST, and the first is
    /// routinely the test or CPython itself. First-wins there would send the shard to the caller,
    /// or to the stdlib — so frames keep last-wins while authored paths take first-wins.
    #[test]
    fn a_traceback_still_shards_to_its_last_owned_frame() {
        let files: Vec<String> = ["app/api.py", "app/store.py", "tests/test_api.py"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let tb = "`pytest -q` failed:\n \
                  File \"/opt/homebrew/Cellar/python@3.14/lib/python3.14/threading.py\", line 1024, in run\n \
                  File \"/Users/x/runs/unit/app/api.py\", line 40, in list_payments\n \
                  File \"/Users/x/runs/unit/app/store.py\", line 88, in query";
        assert_eq!(
            extract_file_from_finding(tb, &files).as_deref(),
            Some("app/store.py"),
            "the failing frame is the last one"
        );
        let short = "tests/test_api.py:9: in test_list\n    api.list_payments()\n\
                     app/api.py:40: in list_payments\n    store.query()\n\
                     app/store.py:88: in query\nE   sqlite3.OperationalError";
        assert_eq!(
            extract_file_from_finding(short, &files).as_deref(),
            Some("app/store.py")
        );
    }

    /// F881 REGRESSION (run 8, score 0.601): the repair round RACED whole-tree twins even though its
    /// first finding literally begins with a path. 0 of 3 twins promoted, as 0 of 9 had before it.
    /// These are the THREE finding strings run 8's own `complete_verify` emitted, and the file list
    /// its DAG owned. If attribution returns no group, the round can only race — so this test pins
    /// the routing INPUT, not the routing rule (which `prefer_shard_over_race` already pins).
    #[test]
    fn run8_findings_attribute_to_files_so_the_round_shards() {
        let files: Vec<String> = [
            "README.md",
            "test_api.py",
            "test_main.py",
            "test_meridian.py",
            "test_store.py",
            "test_web.py",
            "vendorsync/__init__.py",
            "vendorsync/__main__.py",
            "vendorsync/api.py",
            "vendorsync/meridian.py",
            "vendorsync/store.py",
            "vendorsync/web/app.js",
            "vendorsync/web/index.html",
            "vendorsync/web/styles.css",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let findings = vec![
            "vendorsync/web/app.js:132 references DOM id `sync-error` which NO html file in the app \
             defines — getElementById returns null there and the page throws at runtime (the \
             rendered-nothing class). Either add the id to the HTML or fix the reference to an id \
             that exists.".to_string(),
            "POST /api/sync is not CHEAP on a repeat run — the second sync re-fetched 247 row(s) it \
             already had. FIX: make the client send If-None-Match per page.".to_string(),
            "the page renders but the browser console carries 2 error(s) in normal use (first: \
             Failed to load resource: net::ERR_EMPTY_RESPONSE) — fix the JS errors; users hit them \
             as broken interactions.".to_string(),
        ];
        let (groups, unassigned) = group_findings_by_file(&findings, &files);
        eprintln!("GROUPS={} UNASSIGNED={}", groups.len(), unassigned.len());
        for g in &groups {
            eprintln!("  group {} <- {} finding(s)", g.file, g.findings.len());
        }
        let attributed: usize = groups.iter().map(|g| g.findings.len()).sum();
        assert!(
            attributed > 0 && !groups.is_empty(),
            "run 8's findings must attribute to per-file shards — the fan is the only wave now"
        );
    }

    /// F882 REGRESSION (run 9, round 0): TWO findings, both describing file-anchored work, BOTH
    /// attributed to nothing — so the round raced whole-tree twins (lifetime 0 of 12 promoted)
    /// instead of sharding. These are the REAL shapes from that round: (1) a `pytest -q` failure
    /// whose only file mentions are node-id summary lines with a status word in front; (2) the
    /// gate's own NotCheap finding, which described the client fix in full and named no file —
    /// now required to carry the client path the spec's module table derives.
    #[test]
    fn run9_round0_findings_attribute_and_shard() {
        let files: Vec<String> = [
            "vendorsync/api.py",
            "vendorsync/meridian.py",
            "vendorsync/store.py",
            "tests/test_api.py",
            "tests/test_meridian.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pytest_finding =
            "`pytest -q` failed — the generated tests exercise runtime paths that \
             `--help`/`--collect-only` never invoke:\n\
             ERROR tests/test_api.py::TestPayments::test_payments_item_keys - AttributeError\n\
             ERROR tests/test_api.py::TestSummarySync::test_summary_empty_db - AttributeError\n\
             19 failed, 33 passed, 16 warnings, 15 errors in 22.59s"
                .to_string();
        let notcheap_finding = "POST /api/sync is not CHEAP on a repeat run — the second sync \
             re-fetched 247 row(s) it already had. Key each page's ETag by the exact request that \
             produced it. The vendor client lives in `vendorsync/meridian.py` — fix it there."
            .to_string();
        assert_eq!(
            extract_file_from_finding(&pytest_finding, &files).as_deref(),
            Some("tests/test_api.py"),
            "a pytest -q node-id summary must resolve to its file"
        );
        assert_eq!(
            extract_file_from_finding(&notcheap_finding, &files).as_deref(),
            Some("vendorsync/meridian.py"),
            "a client-behaviour finding must resolve to the client file it now names"
        );
        let (groups, unassigned) =
            group_findings_by_file(&[pytest_finding, notcheap_finding], &files);
        assert_eq!(groups.len(), 2, "two files, two shards");
        assert!(unassigned.is_empty());
        let attributed: usize = groups.iter().map(|g| g.findings.len()).sum();
        assert_eq!(
            attributed, 2,
            "run 9's round attributes both findings to shards"
        );
    }

    #[test]
    fn group_findings_by_file_partitions_dedups_and_serializes() {
        let findings = vec![
            "tests/test_a.py:5: in test_x\n assert foo() == 1\nE   AssertionError".to_string(),
            "spendlog/cli.py:12: in cmd_add\n raise ValueError\nE   ValueError: boom".to_string(),
            "spendlog/cli.py:40: in cmd_budget\n x\nE   KeyError".to_string(), // SAME file
            "spendlog/cli.py:12: in cmd_add\n raise ValueError\nE   ValueError: boom".to_string(), // dup
            "no python3 -m spendlog entry point found".to_string(), // unassigned
        ];
        let (groups, unassigned) = group_findings_by_file(&findings, &[]);
        let cli = groups
            .iter()
            .find(|g| g.file == "spendlog/cli.py")
            .expect("cli.py group");
        // The two DISTINCT cli.py findings collapse into ONE group (same-file serialize); the dup is dropped.
        assert_eq!(cli.findings.len(), 2);
        assert!(groups.iter().any(|g| g.file == "tests/test_a.py"));
        assert_eq!(unassigned.len(), 1); // the file-less finding
                                         // Partition invariant: every group names a distinct file.
        let files: std::collections::HashSet<_> = groups.iter().map(|g| &g.file).collect();
        assert_eq!(files.len(), groups.len());
    }

    /// THE FAN'S PRECONDITION. `complete_parallel` groups findings by file and fans one shard per
    /// group; anything with no file falls to the single serial fix worker that owns 22-44% of the
    /// run. MEASURED by porting this extractor and running it on the finding strings three real runs
    /// actually produced: FIVE of six shapes returned None, including the only two that appeared in
    /// every run. The fan was well built and had almost nothing to fan — the same shape as the
    /// judge-side splitter that never fires and the e2e fan that did not partition.
    #[test]
    fn every_real_finding_shape_resolves_to_a_file_group() {
        let files = vec![
            "vendorsync/api.py".to_string(),
            "vendorsync/store.py".to_string(),
            "vendorsync/meridian.py".to_string(),
            "tests/test_meridian.py".to_string(),
        ];
        let cases: [(&str, &str); 5] = [
            // AST review — names a MODULE, never a path. Present in 3 of 3 runs.
            ("function 'log_message' in module 'vendorsync.api' is a STUB/UNIMPLEMENTED — implement it",
             "vendorsync/api.py"),
            // cross-module drift — names two modules; the READER is the one to fix.
            ("module 'vendorsync.api' reads field 'total' that 'vendorsync.store' does not define",
             "vendorsync/api.py"),
            // engine-authored, path in backticks mid-sentence.
            ("planned deliverable `vendorsync/store.py` is MISSING or EMPTY — create it",
             "vendorsync/store.py"),
            ("planned task `test-meridian` FAILED, but its deliverable `tests/test_meridian.py` IS \
              written. Its attempts were exhausted because the checks it runs DO NOT PASS.",
             "tests/test_meridian.py"),
            // the pytest traceback the extractor was originally written for must still work.
            ("`pytest -q` failed:\ntests/test_meridian.py:89: in test_x\nE   AssertionError",
             "tests/test_meridian.py"),
        ];
        for (finding, want) in cases {
            assert_eq!(
                extract_file_from_finding(finding, &files).as_deref(),
                Some(want),
                "finding did not resolve to its file: {finding:?}"
            );
        }
        // A finding that genuinely names no file must STILL be unassigned — the serial path is the
        // correct home for it, and inventing a file would aim a fix shard at nothing.
        assert_eq!(
            extract_file_from_finding(
                "GET /api/health returned 404 — the app does not implement it",
                &files
            ),
            None
        );

        // THE REAL PYTEST TRACEBACK, in the shape of the first finding this engine ever emitted. Its
        // FIRST frame is CPython's own threading.py and the app frame is ABSOLUTE. Neither may
        // escape: a stdlib path sends a fix shard to repair CPython, and an absolute path keys the
        // file-group differently from the rest of the engine and breaks the shadow tree's
        // promote-by-relative-path. Latent until F41 made the fan fire.
        let tb = "`pytest -q` failed:\n File \"/opt/homebrew/Cellar/python@3.14/lib/python3.14/threading.py\", line 1024, in run\n File \"/Users/x/runs/unit/vendorsync/meridian.py\", line 40, in serve";
        assert_eq!(
            extract_file_from_finding(tb, &files).as_deref(),
            Some("vendorsync/meridian.py"),
            "a traceback must resolve to the OWNED, repo-relative file — never the stdlib, never absolute"
        );

        // Naming only files this run does not own resolves to NOTHING; the serial fix path handles
        // it. Inventing an owner is worse than admitting there is none.
        let foreign =
            "File \"/opt/homebrew/Cellar/python@3.14/lib/python3.14/socket.py\", line 9, in x";
        assert_eq!(extract_file_from_finding(foreign, &files), None);
    }

    #[test]
    fn normalize_rel_path_collapses_spellings_to_one_group() {
        assert_eq!(normalize_rel_path("./pkg/x.py"), "pkg/x.py");
        assert_eq!(normalize_rel_path("pkg//x.py"), "pkg/x.py");
        assert_eq!(normalize_rel_path("pkg\\x.py"), "pkg/x.py");
        assert_eq!(normalize_rel_path("pkg/x.py"), "pkg/x.py");
        // Two DISTINCT findings that name the same file with different spellings must resolve to ONE
        // file-group — otherwise two shards would promote to the same real dst (a torn write).
        let findings = vec![
            "./pkg/cli.py:1: in a\nE   Err".to_string(),
            "pkg//cli.py:2: in b\nE   Err".to_string(),
        ];
        let (groups, _) = group_findings_by_file(&findings, &[]);
        assert_eq!(groups.len(), 1, "both spellings collapse to one group");
        assert_eq!(groups[0].file, "pkg/cli.py");
    }

    /// PRIORITIES (Mihai 2026-08-30): severity is a pure function of the AUTHORING CHECK.
    /// The same text under two different sources ranks differently, two different texts under
    /// one source rank the same, and an untagged text is the loud named absence "unsourced" —
    /// never a silent default tier.
    #[test]
    fn severity_derives_from_the_authoring_check_never_from_the_text() {
        let text = "the page renders but the browser console carries 4 error(s) in normal use \
                    (first: ReferenceError: onBrushChangeTracked is not defined) — fix the JS \
                    errors; users hit them as broken interactions. (in `web/viz.js`)"
            .to_string();
        let mut a = FindingProvenance::default();
        a.tag(
            FindingSource::RenderGateConsole,
            std::slice::from_ref(&text),
        );
        let mut b = FindingProvenance::default();
        b.tag(
            FindingSource::EndpointContractProbe,
            std::slice::from_ref(&text),
        );
        assert_eq!(a.severity_label(&text), "high");
        assert_eq!(
            b.severity_label(&text),
            "medium",
            "the SAME text under a different authoring check must rank differently"
        );
        let other = "POST /api/drafts's response does not carry the documented field(s) \
                     `amount_minor`, `currency`"
            .to_string();
        b.tag(
            FindingSource::EndpointContractProbe,
            std::slice::from_ref(&other),
        );
        assert_eq!(
            b.severity_label(&other),
            "medium",
            "two different texts under ONE authoring check must rank the same"
        );
        assert_eq!(a.severity_label("never tagged"), "unsourced");
        assert_eq!(
            a.source_label("never tagged"),
            "unsourced (no authoring check recorded)"
        );
    }

    /// The derivation table itself, pinned per class: app-unusable product checks CRITICAL,
    /// feature-severing HIGH, contract/shape MEDIUM, cosmetic LOW.
    #[test]
    fn the_derivation_table_maps_each_authoring_check_to_its_class() {
        use FindingSource::*;
        for s in [
            BootProbe,
            SmokeGate,
            FailedTask,
            MissingDeliverable,
            EndpointDeadProbe,
            SyncAcquisition,
            RenderGateRows,
            RenderGateException,
            RestartDurability,
        ] {
            assert_eq!(s.severity(), FindingSeverity::Critical, "{s:?}");
        }
        for s in [
            RenderGateConsole,
            ClientApiPaging,
            CrossModuleDrift,
            DomIdScan,
        ] {
            assert_eq!(s.severity(), FindingSeverity::High, "{s:?}");
        }
        for s in [EndpointContractProbe, AggregateTruth, HttpTimeoutScan] {
            assert_eq!(s.severity(), FindingSeverity::Medium, "{s:?}");
        }
        for s in [RenderGateStyling, CssCoherenceScan] {
            assert_eq!(s.severity(), FindingSeverity::Low, "{s:?}");
        }
    }

    /// The wave's dispatch order when file-groups exceed free nodes: fanout_over_fleet hands
    /// devices to the first items, so sort_groups must put the severest file first — max
    /// severity, then finding count, stable for ties.
    #[test]
    fn the_wave_fans_the_severest_file_group_first_when_groups_exceed_nodes() {
        let mut prov = FindingProvenance::default();
        let crit = "the advertised sync answered 2xx but the app's OWN list holds ZERO rows \
                    while the vendor's collection holds 1553"
            .to_string();
        let high = "the page renders but the browser console carries 4 error(s) (in `web/viz.js`)"
            .to_string();
        let med1 = "POST /api/drafts's response does not carry the documented field(s)".to_string();
        let med2 =
            "POST /api/webhooks's response could not be read as a JSON object on either probe"
                .to_string();
        let med3 = "POST /api/sync is not CHEAP on a repeat run".to_string();
        prov.tag(FindingSource::SyncAcquisition, std::slice::from_ref(&crit));
        prov.tag(
            FindingSource::RenderGateConsole,
            std::slice::from_ref(&high),
        );
        prov.tag(
            FindingSource::EndpointContractProbe,
            &[med1.clone(), med2.clone(), med3.clone()],
        );
        // Three groups over (say) two nodes: the MEDIUM-heavy group arrives first from
        // grouping order, and must dispatch LAST.
        let mut groups = vec![
            FileGroup {
                file: "app/__main__.py".to_string(),
                findings: vec![med1.clone(), med2.clone(), med3.clone()],
            },
            FileGroup {
                file: "web/viz.js".to_string(),
                findings: vec![high.clone()],
            },
            FileGroup {
                file: "app/sync.py".to_string(),
                findings: vec![crit.clone()],
            },
        ];
        prov.sort_groups(&mut groups);
        let order: Vec<&str> = groups.iter().map(|g| g.file.as_str()).collect();
        assert_eq!(
            order,
            ["app/sync.py", "web/viz.js", "app/__main__.py"],
            "critical file first, then high, then the medium pile"
        );
        // Equal severity: the group with MORE findings leads; equal both: stable.
        let mut tie = vec![
            FileGroup {
                file: "a.py".to_string(),
                findings: vec![med1.clone()],
            },
            FileGroup {
                file: "b.py".to_string(),
                findings: vec![med2.clone(), med3.clone()],
            },
        ];
        prov.sort_groups(&mut tie);
        assert_eq!(
            tie[0].file, "b.py",
            "same tier: more findings dispatches first"
        );
    }

    /// The shard brief's fix-first note is assembled from the round's REAL facts — r5's own
    /// round-0 shape: one render-console HIGH ahead of contract MEDIUMs, real positions, real
    /// authoring checks — and it must not collide with the numbered block the brief's own
    /// parser (parse_numbered_findings) round-trips.
    #[test]
    fn the_fix_first_note_carries_real_severities_sources_and_counts() {
        let mut prov = FindingProvenance::default();
        let console = "the page renders but the browser console carries 4 error(s) in normal \
                       use (first: ReferenceError: onBrushChangeTracked is not defined) (in \
                       `web/viz.js`)"
            .to_string();
        let shape1 = "POST /api/payments/<id>/note's response does not carry the documented \
                      field(s) `id`, `note`, `version`"
            .to_string();
        // The live emitter's shape since the r6c split: the verdict names which probe, and the
        // finding ends in the probe's own evidence (request line, each status, a body head).
        let shape2 = format!(
            "POST /api/webhooks/meridian's response could not be read as a JSON object on \
             either probe — the spec documents a JSON response for every endpoint. {}",
            "PROBE EVIDENCE — request as sent: `POST /api/webhooks/meridian` with NO body and NO headers (bare `curl -X POST`, 20s budget); probe 1: HTTP 401, body «{\"error\": {\"code\": \"bad_signature\"}}»; probe 2: HTTP 401, body «{\"error\": {\"code\": \"bad_signature\"}}»."
        );
        prov.tag(
            FindingSource::RenderGateConsole,
            std::slice::from_ref(&console),
        );
        prov.tag(
            FindingSource::EndpointContractProbe,
            &[shape1.clone(), shape2.clone()],
        );
        let mut findings = vec![shape1.clone(), console.clone(), shape2.clone()];
        prov.sort_findings(&mut findings);
        assert_eq!(findings[0], console, "the HIGH console finding leads");
        let note = prov.fix_order_note(&findings);
        assert!(note.starts_with("FIX IN THIS ORDER"), "{note}");
        assert!(
            note.contains("- finding 1 is HIGH — authored by the render gate console"),
            "{note}"
        );
        assert!(
            note.contains("- findings 2-3 are MEDIUM — authored by the endpoint contract probe"),
            "{note}"
        );
        assert!(note.ends_with("\n\n"), "prepends cleanly to the brief");
        for line in note.lines() {
            assert!(
                !line.trim_start().starts_with("1. "),
                "the note must never open a numbered block of its own: {line}"
            );
        }
        // The unsourced arm stays LOUD but must read as a sentence — the broken shape was
        // "authored by the unsourced (no authoring check recorded)" in a model-facing brief.
        let untagged =
            "GET /healthz answered 500 before any check registered authorship".to_string();
        let mut with_untagged = findings.clone();
        with_untagged.push(untagged);
        prov.sort_findings(&mut with_untagged);
        let note_untagged = prov.fix_order_note(&with_untagged);
        assert!(
            note_untagged.contains("- finding 4 is UNSOURCED — with no authoring check recorded"),
            "{note_untagged}"
        );
        assert!(
            !note_untagged.contains("authored by the unsourced"),
            "{note_untagged}"
        );
        assert_eq!(prov.fix_order_note(&[]), "", "no findings, no note");
    }

    /// Back-compat: ordering REORDERS, never rewrites — the multiset of exact strings is
    /// unchanged (finding_texts readers keep their texts), and an unsourced finding orders
    /// LAST, after every tagged tier.
    #[test]
    fn severity_ordering_reorders_never_rewrites_and_unsourced_goes_last() {
        let mut prov = FindingProvenance::default();
        let low = "the served page renders with browser-DEFAULT styling".to_string();
        let crit = "pkg did not build: SyntaxError".to_string();
        let untagged = "a finding no check claimed".to_string();
        prov.tag(FindingSource::RenderGateStyling, std::slice::from_ref(&low));
        prov.tag(FindingSource::SmokeGate, std::slice::from_ref(&crit));
        let mut findings = vec![untagged.clone(), low.clone(), crit.clone()];
        let before: std::collections::HashSet<String> = findings.iter().cloned().collect();
        prov.sort_findings(&mut findings);
        assert_eq!(findings, vec![crit, low, untagged.clone()]);
        let after: std::collections::HashSet<String> = findings.iter().cloned().collect();
        assert_eq!(before, after, "exact strings survive byte-identical");
        assert_eq!(prov.severity_label(&untagged), "unsourced");
    }

    /// The rule that makes racing N fix agents safe instead of merely triple the cost. Every case here is
    /// a shape the corpus actually produced: 13 of 13 archived repair rounds ended with findings
    /// outstanding, and the count ROSE in 3 of them under the current promote-on-agent-Ok behaviour.
    /// The diagnostic lives at the END. A head-only cut keeps "what was checked" and throws away "what
    /// went wrong", which is how a pytest collect FAILURE was recorded as a list of successfully
    /// collected tests: the error banner began one character past the old 400-char boundary.
    #[test]
    fn elision_keeps_the_end_of_a_finding_where_the_error_actually_is() {
        // Short input is returned verbatim — no marker, no loss.
        assert_eq!(elide_middle("short", 150, 650), "short");
        assert_eq!(elide_middle("", 10, 10), "");

        // The shape that motivated this: a banner of collected items, then the real error at the end.
        let out = format!(
            "pytest --collect-only errors:\n{}\n=== ERRORS ===\nImportError: cannot import name 'upsert_many'",
            (0..80).map(|i| format!("test_store.py::test_{i}")).collect::<Vec<_>>().join("\n")
        );
        let e = elide_middle(&out, 150, 650);
        assert!(
            e.contains("pytest --collect-only errors"),
            "the head must still name the check"
        );
        assert!(
            e.contains("ImportError: cannot import name 'upsert_many'"),
            "the ERROR must survive truncation — it is the only actionable part: {e}"
        );
        assert!(
            e.contains("middle elided"),
            "elision must be visible, never silent"
        );
        assert!(e.chars().count() < out.chars().count());

        // Multi-byte safety: char-based, so it can never split a character.
        let wide: String = "\u{1f9ea}".repeat(500);
        let w = elide_middle(&wide, 10, 10);
        assert!(w.starts_with(&"\u{1f9ea}".repeat(10)));
        assert!(w.ends_with(&"\u{1f9ea}".repeat(10)));
    }

    /// r6c REGRESSION: the render probe's console-error attribution names the URL PATH the
    /// browser served the script at (product_probe_v3.mjs `urlToRelPath` = `parsed.pathname`,
    /// verbatim), which a static route can serve SHORTER than its repo path — `web/viz.js` on
    /// disk answering at `/viz.js` when the app's static mount strips its directory. The forward
    /// suffix rule only matches when the CANDIDATE is longer than the owned path, so this
    /// finding attributed to NOTHING for two full rounds while a HIGH from a different check
    /// correctly named `web/viz.js`. The reverse match must resolve the unique case and must
    /// stay silent on an ambiguous one.
    #[test]
    fn a_url_derived_basename_shorter_than_its_owned_path_still_resolves_uniquely() {
        let files: Vec<String> = ["web/viz.js", "app/ledgerd.py", "web/index.html"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let finding = "the served page renders NO data rows in a real browser — the API works \
             but the frontend shows a user nothing. First console error: TypeError: Illegal \
             invocation at onBrushChange (viz.js:1124:5). Open web/index.html end to end: the \
             page must fetch the documented endpoints and render the rows, and every fetch \
             failure must surface a visible state, not a blank page. (in `viz.js`)";
        assert_eq!(
            extract_file_from_finding(finding, &files).as_deref(),
            Some("web/viz.js"),
            "a bare URL-served basename must resolve to its unique owned path"
        );

        // Two files share the basename: must NOT guess.
        let ambiguous: Vec<String> = ["web/viz.js", "legacy/viz.js"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            extract_file_from_finding("console error (in `viz.js`)", &ambiguous),
            None,
            "an ambiguous basename must stay unresolved rather than guess between two owners"
        );
    }

    /// II-2: the shard prompt's own demanded line format, parsed at last. `NOT FIXED`/`NOT REAL`
    /// must win over the `FIXED` they contain, markdown litter must not hide a verdict, and a
    /// line that is not a verdict must not become one.
    #[test]
    fn parse_finding_verdicts_reads_the_demanded_line_format() {
        let out = "I repaired what I could.\n\
                   FINDING 1: FIXED — ran `python3 -m pytest tests/test_ledger_core.py`, 26 passed\n\
                   - **FINDING 2: NOT FIXED** — tried rewriting the UPDATE to use version guards; \
                   test_concurrent_updates still fails with version 3 != 2\n\
                   FINDING 3: NOT REAL — GET /api/health returned 200 with {\"status\":\"ok\"}\n\
                   finding 4 was hard so I skipped it\n\
                   Finding 5: FIXED — curl output attached";
        let v = parse_finding_verdicts(out);
        assert_eq!(
            v.iter().map(|(n, s, _)| (*n, *s)).collect::<Vec<_>>(),
            vec![
                (1, "FIXED"),
                (2, "NOT FIXED"),
                (3, "NOT REAL"),
                (5, "FIXED")
            ],
            "the prose line about finding 4 is not a verdict"
        );
        assert!(
            v[1].2.contains("version guards"),
            "detail survives: {:?}",
            v[1].2
        );
        assert!(parse_finding_verdicts("all good, nothing to report").is_empty());
    }

    /// VA-006 / REPAIR v2 §4: the render gate names the EXCEPTION branch from the console line
    /// the probe recorded — the language's own classes, never a list of symbol names. r5's
    /// `ReferenceError` and r6c's `TypeError: Illegal invocation` are exceptions; a resource
    /// that failed to load and an app's own console.error text are not.
    #[test]
    fn a_console_exception_is_classified_by_the_language_never_by_a_name() {
        for line in [
            "ReferenceError: onBrushChangeTracked is not defined",
            "TypeError: Illegal invocation",
            "Uncaught SyntaxError: Unexpected token '<'",
            "TypeError: Cannot read properties of null (reading 'getContext')",
            "Error: WebGL2 unavailable",
        ] {
            assert!(console_error_is_exception(line), "{line}");
        }
        for line in [
            "Failed to load resource: net::ERR_EMPTY_RESPONSE",
            "GET http://127.0.0.1:8931/styles.css 404 (Not Found)",
            "Sync failed: HTTP 500",
            "",
        ] {
            assert!(!console_error_is_exception(line), "{line}");
        }
    }

    /// REPAIR v2 §2: a check's KEY is the same check across two gate runs — the preview boots on
    /// another port, r6c's DOM-id line moves when an edit lands above it, and the console's first
    /// error changes when the first one is fixed — while two different checks stay distinct: two
    /// DOM ids in one file, the same words under two authoring checks. An untagged finding has no
    /// check at all (the fallback gate: unverifiable is said, never guessed).
    #[test]
    fn a_checks_key_survives_ports_line_shifts_and_exemplar_changes() {
        let mut prov = FindingProvenance::default();
        let r5_console = |n: u32, first: &str, port: u32| {
            format!(
                "the page renders but the browser console carries {n} error(s) in normal use \
                 (first: {first}) — fix the JS errors; users hit them as broken interactions. \
                 GATE COMMAND (run it yourself; it prints consoleErrors.texts): `node \
                 /opt/probe.mjs load http://127.0.0.1:{port}`. (in `web/viz.js`)"
            )
        };
        let before = r5_console(
            4,
            "ReferenceError: onBrushChangeTracked is not defined",
            54321,
        );
        let after = r5_console(
            3,
            "TypeError: gl.vertexAttribDivisor is not a function",
            61002,
        );
        prov.tag(
            FindingSource::RenderGateException,
            &[before.clone(), after.clone()],
        );
        let kb = prov.check_of(&before).expect("sourced");
        let ka = prov.check_of(&after).expect("sourced");
        assert_eq!(kb.key, ka.key, "port, count and exemplar are not the check");
        assert_eq!(
            kb.key,
            "render gate console | the page renders but the browser console carries # error in \
             normal use"
        );
        // VA-098: the exception fixed, a resource-load error now first — the SAME console check
        // (fewer failures), never a check that passed on the tree and fails on the preview.
        assert_eq!(
            check_key(FindingSource::RenderGateException, &before),
            check_key(
                FindingSource::RenderGateConsole,
                &r5_console(1, "Failed to load resource: net::ERR_EMPTY_RESPONSE", 61003)
            ),
            "the console probe is one check across its two classes"
        );
        let dom = |line: u32, id: &str| {
            format!(
                "web/viz.js:{line} references DOM id `{id}` which NO html file in the app \
                 defines — getElementById returns null there and the page throws at runtime \
                 (the rendered-nothing class). Either add the id to the HTML or fix the \
                 reference to an id that exists."
            )
        };
        let d533 = dom(533, "viz-labels");
        let d540 = dom(540, "viz-labels");
        let legend = dom(533, "viz-legend");
        prov.tag(
            FindingSource::DomIdScan,
            &[d533.clone(), d540.clone(), legend.clone()],
        );
        assert_eq!(
            check_key(FindingSource::DomIdScan, &d533),
            check_key(FindingSource::DomIdScan, &d540),
            "a line shift is not a different check"
        );
        assert_ne!(
            check_key(FindingSource::DomIdScan, &d533),
            check_key(FindingSource::DomIdScan, &legend),
            "two DOM ids in one file are two checks"
        );
        let shared = "the served page renders NO data rows in a real browser — the API works \
                      but the frontend shows a user nothing. First console error: TypeError: \
                      Illegal invocation. (in `viz.js`)";
        assert_ne!(
            check_key(FindingSource::RenderGateRows, shared),
            check_key(FindingSource::EndpointContractProbe, shared),
            "the same words under two different probes are two checks"
        );
        assert_eq!(
            check_key(FindingSource::RenderGateRows, shared),
            "render gate rows | the served page renders no data rows in a real browser"
        );
        assert_eq!(
            check_key(
                FindingSource::BootProbe,
                "the app never bound port 8123 when started EXACTLY as its spec documents \
                 (`python3 -m app --db-dir X`), so it does not run at all. Check that the \
                 entrypoint BLOCKS while serving."
            ),
            "boot probe | the app never bound port # when started exactly as its spec documents \
             , so it does not run at all"
        );
        assert_eq!(prov.check_of("never tagged"), None, "unsourced = no check");
    }

    /// REPAIR v2 §1: the shard's first action is the gate's own replay, quoted from the finding
    /// — the render gate's `GATE COMMAND` sentence (attribution suffix stripped), the POST
    /// probe's `REPLAY IT:` sentence, the command a smoke finding opens with — and a finding
    /// that carries no command says so (None), never a substitute.
    #[test]
    fn a_checks_command_is_the_gates_own_replay_sentence() {
        let console = "the page renders but the browser console carries 4 error(s) in normal use \
                       (first: ReferenceError: onBrushChangeTracked is not defined) — fix the JS \
                       errors. GATE COMMAND (run it yourself; it prints consoleErrors.texts): \
                       `node /opt/probe.mjs load http://127.0.0.1:54321`. (in `web/viz.js`)";
        assert_eq!(
            check_command(console).as_deref(),
            Some(
                "GATE COMMAND (run it yourself; it prints consoleErrors.texts): `node \
                 /opt/probe.mjs load http://127.0.0.1:54321`"
            )
        );
        let post = "POST /api/webhooks/meridian's response could not be read as a JSON object on \
                    either probe — the spec documents a JSON response for every endpoint. PROBE \
                    EVIDENCE — request as sent: `POST /api/webhooks/meridian` with NO body and NO \
                    headers (bare `curl -X POST`, 20s budget); probe 1: HTTP 401, body «{}». \
                    REPLAY IT: boot exactly as the gate did — `cd <tree> && PYTHONPATH=src \
                    python3 -m app.ledgerd --db-dir D` — then `curl -s -w '\\n%{http_code}' -X \
                    POST -m 20 http://127.0.0.1:8931/api/webhooks/meridian`; a NOT REAL verdict \
                    must quote that command's status and body.";
        let cmd = check_command(post).expect("the REPLAY IT sentence");
        assert!(
            cmd.starts_with("REPLAY IT: boot exactly as the gate did"),
            "{cmd}"
        );
        assert!(cmd.contains("-X POST -m 20 http://127.0.0.1:8931/api/webhooks/meridian"));
        assert_eq!(
            check_command("`pytest -q` failed — the generated tests exercise runtime paths:\nFAILED tests/test_api.py::test_x")
                .as_deref(),
            Some("pytest -q")
        );
        assert_eq!(
            check_command("web/viz.js:533 references DOM id `viz-labels` which NO html file in the app defines — fix it"),
            None,
            "a static scan is not a command the shard can run by hand"
        );
    }

    /// VA-006 (DESIGN-REPAIR-V2 §4), the partition r5 and r6c shipped green through: a boot-path
    /// exception and a dead journey in a real browser are CRITICAL by provenance; a resource
    /// console error is render-class but not critical; styling is neither; the sync_rows probe
    /// keeps its pinned MILD standing (critical for ORDERING, never for the green claim); the
    /// endpoint contract stays a minor. r6c's round-0 nine, partitioned, put both render rows
    /// findings in the criticals — the two "known active bugs" its `passed:true` shipped over.
    #[test]
    fn boot_path_exceptions_are_critical_and_render_class_is_named() {
        let mut prov = FindingProvenance::default();
        let exception = "the page renders but the browser console carries 4 error(s) in normal \
                         use (first: ReferenceError: onBrushChangeTracked is not defined) — fix \
                         the JS errors. (in `web/viz.js`)"
            .to_string();
        let rows = "the served page renders NO data rows in a real browser — the API works but \
                    the frontend shows a user nothing. First console error: TypeError: Illegal \
                    invocation. (in `viz.js`)"
            .to_string();
        let resource = "the page renders but the browser console carries 1 error(s) in normal \
                        use (first: Failed to load resource: net::ERR_EMPTY_RESPONSE) — fix the \
                        JS errors."
            .to_string();
        let styling = "the served page renders with browser-DEFAULT styling — no stylesheet \
                       reached the browser."
            .to_string();
        let sync_rows = "sync_rows: the vendor's own collection holds 12288 row(s), the \
                         advertised sync (`POST /api/sync`) answered 200, and the app's OWN reads \
                         still report ZERO rows — the sync is not acquiring the data."
            .to_string();
        let contract = "POST /api/drafts's response does not carry the documented field(s) \
                        `amount_minor`, `currency` — the spec's endpoint table names them."
            .to_string();
        let never_bound = "the app never bound port 8123 when started EXACTLY as its spec \
                           documents (`python3 -m app`), so it does not run at all."
            .to_string();
        prov.tag(
            FindingSource::RenderGateException,
            std::slice::from_ref(&exception),
        );
        prov.tag(FindingSource::RenderGateRows, std::slice::from_ref(&rows));
        prov.tag(
            FindingSource::RenderGateConsole,
            std::slice::from_ref(&resource),
        );
        prov.tag(
            FindingSource::RenderGateStyling,
            std::slice::from_ref(&styling),
        );
        prov.tag(
            FindingSource::SyncAcquisition,
            std::slice::from_ref(&sync_rows),
        );
        prov.tag(
            FindingSource::EndpointContractProbe,
            std::slice::from_ref(&contract),
        );
        prov.tag(FindingSource::BootProbe, std::slice::from_ref(&never_bound));
        assert!(
            prov.is_critical(&exception),
            "a boot-path exception is critical"
        );
        assert!(
            prov.is_critical(&rows),
            "a dead journey in a browser is critical"
        );
        assert!(
            prov.is_critical(&never_bound),
            "engine_critical wording still counts"
        );
        assert!(
            !prov.is_critical(&resource) && prov.is_render_class(&resource),
            "a resource console error is render-class, not critical"
        );
        assert!(
            !prov.is_critical(&styling) && !prov.is_render_class(&styling),
            "styling stays advisory"
        );
        assert!(
            !prov.is_critical(&sync_rows) && !prov.is_render_class(&sync_rows),
            "sync_rows is repairable-never-blocking (P1-12 MILD, pinned)"
        );
        assert!(!prov.is_critical(&contract) && !prov.is_render_class(&contract));
        let all = vec![
            rows.clone(),
            exception.clone(),
            resource.clone(),
            styling.clone(),
            sync_rows,
            contract.clone(),
        ];
        let (criticals, minors) = prov.partition_criticals(&all);
        assert_eq!(criticals, vec![rows, exception]);
        assert_eq!(minors.len(), 4);
        let render_class_minors: Vec<&String> =
            minors.iter().filter(|m| prov.is_render_class(m)).collect();
        assert_eq!(
            render_class_minors,
            vec![&resource],
            "the one render-class minor is what blocks `passed`"
        );
        // An untagged text: neither critical nor render-class by provenance — only its own
        // wording can make it critical.
        assert!(!prov.is_render_class("never tagged"));
        assert!(prov.is_critical("the entry exited non-zero"));
    }

    #[test]
    fn require_tests_separates_an_empty_suite_from_a_passing_one() {
        // OFF: no input can produce a finding => byte-identical to the pre-lever gate.
        for v in [
            TestRunVerdict::NoTests,
            TestRunVerdict::Pass,
            TestRunVerdict::PytestMissing,
            TestRunVerdict::Failures("boom".to_string()),
        ] {
            assert_eq!(require_tests_finding(&v, false), None);
        }
        // ON: ONLY the "nothing was checked" verdict becomes a finding.
        assert!(require_tests_finding(&TestRunVerdict::NoTests, true).is_some());
        // A real pass stays green, and a real failure keeps its OWN existing finding (never double-reported).
        assert_eq!(require_tests_finding(&TestRunVerdict::Pass, true), None);
        assert_eq!(
            require_tests_finding(&TestRunVerdict::Failures("boom".to_string()), true),
            None
        );
        // A MISSING pytest is inconclusive, never a defect — the gate must not invent one.
        assert_eq!(
            require_tests_finding(&TestRunVerdict::PytestMissing, true),
            None
        );
    }

    /// VA-134 → VA-136, r6h's exact shape: the no-executable-tests finding tagged at the smoke
    /// gate — its own source FIRST, then the gate's batch tag. The partition still reads it as a
    /// minor (`passed` unchanged), its label now reads `medium` from its own source, and the
    /// mismatch rows are EMPTY (`mismatched` absent from complete_result). Nothing is tagged
    /// when the gate did not push the finding.
    #[test]
    fn the_require_tests_finding_carries_its_own_source_and_r6h_has_no_mismatch_rows() {
        let no_tests = require_tests_finding(&TestRunVerdict::NoTests, true).unwrap();
        let shape = "POST /api/drafts's response does not carry the documented field(s) \
                     `amount_minor`"
            .to_string();
        let gate_findings = vec![no_tests.clone()];
        let mut prov = FindingProvenance::default();
        tag_require_tests(&mut prov, &gate_findings);
        prov.tag(FindingSource::SmokeGate, &gate_findings);
        prov.tag(
            FindingSource::EndpointContractProbe,
            std::slice::from_ref(&shape),
        );
        assert_eq!(
            prov.source_of(&no_tests),
            Some(FindingSource::RequireTests),
            "its own source, not the batch tag's"
        );
        assert_eq!(prov.severity_label(&no_tests), "medium");
        assert_eq!(
            prov.source_label(&no_tests),
            "smoke gate (no executable tests)"
        );
        let findings = vec![no_tests.clone(), shape.clone()];
        let (criticals, minors) = prov.partition_criticals(&findings);
        assert!(
            criticals.is_empty(),
            "r6h: the partition reads both as minors"
        );
        assert_eq!(minors, findings);
        let passed = criticals.is_empty() && !minors.iter().any(|m| prov.is_render_class(m));
        assert!(passed, "r6h's verdict stands — unchanged by the label");
        assert!(
            prov.verdict_severity_mismatches(&minors).is_empty(),
            "label and partition agree: no row"
        );
        let mut none = FindingProvenance::default();
        tag_require_tests(&mut none, std::slice::from_ref(&shape));
        assert_eq!(none.source_of(&no_tests), None);
        assert_eq!(none.source_of(&shape), None);
    }

    /// The tag ORDER is the rule: `tag` keeps the first writer, so the batch tag landing first
    /// (the pre-VA-136 site) reproduces r6h's `critical`-labelled minor and its ONE mismatch
    /// row — the instrument keeps guarding the next unsourced finding.
    #[test]
    fn the_batch_tag_landing_first_reproduces_r6hs_mismatch_row() {
        let no_tests = require_tests_finding(&TestRunVerdict::NoTests, true).unwrap();
        let gate_findings = vec![no_tests.clone()];
        let mut prov = FindingProvenance::default();
        prov.tag(FindingSource::SmokeGate, &gate_findings);
        tag_require_tests(&mut prov, &gate_findings);
        assert_eq!(prov.source_of(&no_tests), Some(FindingSource::SmokeGate));
        let (criticals, minors) = prov.partition_criticals(&gate_findings);
        assert!(criticals.is_empty());
        let rows = prov.verdict_severity_mismatches(&minors);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["event"], "verdict_severity_mismatch");
        assert_eq!(rows[0]["partition"], "minor");
        assert_eq!(rows[0]["label"], "critical");
        assert_eq!(rows[0]["source"], "smoke gate (build/test/entry oracle)");
        assert_eq!(
            rows[0]["finding"],
            no_tests.chars().take(160).collect::<String>()
        );
    }
}
