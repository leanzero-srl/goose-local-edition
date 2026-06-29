# Research — helping WEAK LOCAL models (qwopus) deliver better in goose-swarm

Method: 6 research fronts, each web-search-grounded (2025–2026 techniques) AND code-grounded (read the actual
goose-swarm), every recommendation skeptic-vetted twice for "feasible in THIS codebase" + "genuinely helps a
weak 27B local model" (not cloud-agent cargo-culting). 23 recommendations, 14 vetted-strong. Full digest:
scratchpad/RESEARCH-RAW.md.

Anchored in tonight's controlled A/B: qwopus WINS clean cohesive apps, DRAWS on big multi-module apps. The
draw failures were exactly four: (1) a too-big task STALLED (one node doing all the work), (2) cross-module
CONTRACT DRIFT hidden by isolation-only unit tests (fsdrift: snapshot wrote ISO mtime, diff parsed float),
(3) BUILT-BUT-UNWIRED entry points (logfunnel had no dispatcher; antic's highway detector unwired), (4) NO
end-to-end run (verbal "PASS" with the binary never invoked). Every top recommendation targets one of these.

## The strongest signal: 4 independent fronts converged on the SAME idea
When R1, R3, R4 and R6 — researching different questions — all land on the same mechanism, it's real:

### TIER 1 (highest confidence, build first)
1. **Deterministic end-to-end SMOKE GATE (run the real entry point in Rust, not by prompt).** After
   integrate-verify, the HARNESS itself (not the weak model) runs `pytest --collect-only -q` (imports every
   module+target across the project → surfaces cross-module ImportError that per-module tests miss) and, for
   a CLI, derives the package and runs `python3 -m <pkg> --help` asserting exit 0; on failure, fire ONE
   corrective fix re-dispatch whose description IS the captured traceback. **Why local:** verbal self-report
   ("I ran it, PASS") is the least trustworthy signal from a 27B model; `--collect-only` + exit-checked
   `--help` are ground-truth oracles needing zero model intelligence, and they fire precisely on the
   multi-module apps where qwopus only drew. Converged from R1/R3/R4/R6. **Where:** run_swarm tail after
   `scheduler.run`, reuse the ProcCommand pattern (swarm.rs ~922). **Flag:** `GOOSE_SWARM_SMOKE` (off;
   self-disables when no Python entry/tests). Effort M.

2. **Contract-first interface injection (kill cross-module drift structurally).** The planner emits, per
   module, signature-only stubs (function/class signatures + docstrings, no bodies) BEFORE the EXECUTE
   phase; the full set of *sibling* stubs is injected into every parallel worker's prompt. **Why local:**
   weak models can't hold a consistent cross-file contract in their head across independent parallel
   workers — fsdrift's snapshot↔diff mismatch is exactly this. A frozen, injected interface makes the
   contract ground-truth instead of each worker re-inventing it. Converged from R3/R4/R6. **Where:** a new
   pre-EXECUTE planning step in parallel_plan (swarm.rs) + the worker layout_block injection. **Flag:**
   `GOOSE_SWARM_CONTRACTS` (off). Effort M.

3. **Pre-done checklist gate + traceback-into-retry.** At completion, before "done" is accepted, run the
   EXISTING deterministic `py_syntax_error` (ast.parse — NOT py_compile, which the code deliberately avoids
   to skip __pycache__ pollution) on each owned .py; on failure return a Transient with the error. CRITICAL
   enabling fix the vet found: scheduler.rs's Transient arm (482–534) currently DROPS the reason — it never
   writes `prior_hints`, so retries "repeat the same mistake" (acknowledged in swarm.rs:3221). Thread the
   real syntax/import traceback into `prior_hints` (scoped to CONTENT failures, not infra transients) so the
   next attempt gets a targeted fix. **Why local:** a weak model re-rolls blindly without the error; the
   traceback turns a blind retry into a guided one — a bigger win for weak models than strong. **Where:**
   swarm.rs completion guard (~3424) + scheduler.rs Transient arm. **Flag:** `GOOSE_SWARM_DONE_GATE` (off).
   Effort M. (Optional nested `_IMPORTS` sub-flag for the import-resolution probe — riskier, layout-dependent.)

### TIER 2 — the "dynamic review workflow" you asked about (emulating Claude Code)
4. **End-of-run adversarial Review→Verify→Fix PHASE as successive Scheduler DAGs.** This is literally what we
   just did by hand (the audit that found 11 real bugs). Implement it INSIDE goose-swarm: after
   integrate-verify, spawn N review sub-agents across dimensions on the produced app → skeptic-verify each
   finding → dispatch the survivors as file-scoped FIX subtasks back through the existing scheduler →
   re-verify → loop until clean or a budget. **Feasibility: YES, and cleanly — the Scheduler IS the
   orchestrator; a review pass is just another Dag.** That is the answer to "can we implement something like
   Claude Code?": the deterministic-orchestration layer already exists (Scheduler + Dag + Judge); a review
   workflow is a new phase on top, not a rewrite. **Where:** new phase in run_swarm after integrate-verify,
   reusing Scheduler::run with a review-DAG. **Flag:** `GOOSE_SWARM_REVIEW` (off). Effort L.
5. **Deterministic cross-module/wiring reviewer (Python AST, NO model) as the cheap floor under #4.** Parse
   the produced tree with ast, build the symbol/import graph, and flag: an import of a symbol no module
   defines (drift), a module defining a public entry never imported by the CLI (unwired), a CLI subcommand in
   the spec with no handler. Emits single-file fix subtasks. **Why local:** deterministic, model-free, catches
   the #2/#3 failure classes for free; the LLM reviewers (#4) then handle semantic correctness. **Where:**
   new module invoked in the review phase. **Flag:** shares `GOOSE_SWARM_REVIEW`. Effort M.

### TIER 3 — swarm dynamics (your priorities)
6. **Confidence-gated START (your HARD rule: NEVER start a low-confidence task).** On top of the M6 meter:
   if plan/subtask confidence < floor, do NOT dispatch — enter a bounded research-and-refine loop (Context7
   + web), re-draft, re-measure, repeat until confidence ≥ floor (or max rounds, then surface loudly). **Why
   local:** weak models confidently start vague tasks and thrash; forcing research-until-confident front-loads
   the cheap fix. **Where:** gate in parallel_plan / before scheduler dispatch, using M6's agreement+verbalized
   score. **Flag:** `GOOSE_SWARM_CONFIDENCE_GATE` + a `plan_confidence_floor` config. Effort M.
7. **Load-aware adaptive splitting (no lone node hogging).** Two parts: (a) PLAN-TIME pre-split of oversized
   subtasks (files-owned / est-LOC over a threshold) — cheaper than waiting ~900s for reactive M3; (b)
   RUNTIME: when one node carries a lone long task while others idle, the judge splits it FURTHER (relax
   M3's split-once cap to a small bounded depth, load-aware). **Where:** plan-time check in parallel_plan +
   is_split_candidate/apply_split in judge.rs/scheduler.rs. **Flag:** `GOOSE_SWARM_SPLIT` (extend existing).
   Effort L, risk med — flag honestly + cap depth to avoid runaway.
8. **M7 cross-run lessons, distilled DETERMINISTICALLY from signals already captured (judge verdicts, smoke
   failures, review findings) and injected into the NEXT run's planner.** Closes the learning loop without a
   model deciding what to remember. **Where:** persist a lessons file keyed by failure-signal; inject at
   plan time. **Flag:** `GOOSE_SWARM_LESSONS`. Effort M.
9. (minor) **Adaptive judge thresholds, WIDEN-ONLY** from observed fleet speed (slow fleet → longer
   patience), never narrow (so it can't get trigger-happy). Effort M, low risk.

### TIER 4 — other local-model boosters (beyond dynamic workflows)
10. **Deterministic Context7 doc pre-fetch + injection.** Don't make the weak worker DECIDE to retrieve API
    docs (it usually doesn't) — the planner detects external libs in a subtask and pre-fetches the real API
    via the wired Context7 MCP, injecting it into the worker prompt. **Why local:** removes a decision weak
    models fumble + prevents hallucinated APIs. **Flag:** `GOOSE_SWARM_DOC_PREFETCH`. Effort M.
11. **Grammar / JSON-schema-constrained decoding for ALL structured outputs via LM Studio `response_format`.**
    The planner/judge already use a schema; extend schema-enforcement to every structured tool call so a weak
    model physically cannot emit malformed JSON. **Note:** vet rated this lower because the key paths already
    use it — treat as "audit all structured calls use response_format," not a big build. Effort S.

### What is Claude-Code-like and worth porting (direct answer)
- **Deterministic multi-agent orchestration** → already have it (Scheduler/Dag). The review phase (#4) is the
  missing Claude-Code-style "workflow."
- **Sub-agents with adversarial verification** → #4/#5 (we proved it tonight on our own code).
- **Plan mode / confidence-before-acting** → #6 (never start low-confidence).
- **Structured tool use** → #11 (schema-constrained).
- **A "skill"/checklist the agent must satisfy before done** → #3 (done-gate).
NOT worth porting wholesale: Claude-Code's cloud-model assumptions (long-context single-agent reasoning) —
weak local models need MORE deterministic scaffolding and SMALLER asks, the opposite direction.

## Playwright out-of-the-box (your specific interest) — honest read
Vet rated full OOTB browser-drive as **high-risk / high-effort** (it's a real new capability in core). Phased
recommendation:
- **NOW (low risk):** use the **MCP Playwright** tools (already available this session) INSIDE the review
  phase (#4) — for a web/UI app, navigate, click, assert, screenshot; failures become fix subtasks. This
  gets the value immediately without core changes.
- **STRETCH (the OOTB build you want):** a built-in (non-MCP) verify step that detects web-vs-CLI, starts the
  app, drives it headless via Playwright, asserts + screenshots, feeds failures back. Higher effort/risk;
  do it AFTER the MCP version proves the workflow. Both gated by `GOOSE_SWARM_BROWSER_VERIFY`.
The non-web/CLI path is the smoke gate (#1) — already Tier 1.

## Feature-flagging the whole local-edition (your requirement) — architecture
The vet's nuance: **goose-swarm is already a SEPARATE crate** (de-facto isolation — cloud paths never touch
it). The only bleed into core crates is a handful of hooks (context_mgmt cap, openai provider
`GOOSE_LOCAL_CONTEXT_CAP`, agent.rs per-turn compaction). Clean architecture:
- One top-level **`goose-local` Cargo feature** that compiles in the swarm command + gates the few core-crate
  hooks behind `#[cfg(feature = "goose-local")]`; OFF by default → cloud builds are byte-identical.
- One **runtime switch** (provider-is-local detection OR a config `local_edition: true`) so even in a
  combined build the local-edition behavior only activates for local models.
- The **`GOOSE_SWARM_*` env flags** already in place (JUDGE/SPLIT/PREREVIEW/SMOKE/…) stay as per-feature
  runtime gates under that umbrella.
Effort M; do this BEFORE proposing upstream so block/goose can ingest without risking cloud regressions.

## Recommended build order for the next session (confidence-flagged)
1. Smoke gate (#1) — HIGH conf, biggest single win. 2. Contract injection (#2) — HIGH, kills the drift class.
3. Done-gate + traceback-into-retry (#3) — HIGH (the prior_hints fix is independently valuable).
4. Confidence-gated start (#6) — MED-HIGH, your hard rule. 5. Review phase #4 + AST reviewer #5 — MED (the
big one; the dynamic workflow). 6. Doc pre-fetch (#10), load-aware split (#7), lessons (#8) — MED.
7. Playwright: MCP-in-review now, OOTB later. 8. Feature-flag umbrella (#R6) — do alongside, not last.
All behind flags, all default-OFF, all built+clippy+tested+committed on local-edition.
