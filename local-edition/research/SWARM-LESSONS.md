# SWARM LESSONS — the swarm's growing memory (M7)

Seeded from the deep qualitative reviews (qwen+qwopus apps). Each lesson = a concrete failure mode the
swarm produced + the STRATEGY to prevent it. This file GROWS every run: a new failure → a new lesson →
fed back into the architect/worker/judge prompts. The deepest finding: **passing tests routinely hid a
critical bug — "it runs" and "tests pass" are NOT "it is correct."** So the strategies below push the
swarm from "produces output" toward "produces CORRECT output, verified."

## Lessons (failure mode → strategy)
1. **Broken DEFAULT path while a correct impl sits unused** (chaos-fern: default CLI used corrupted
   Barnsley params; correct ifs_core.py unused). → STRATEGY: integrate-verify (and the idle-node review)
   must EXERCISE THE PRIMARY/DEFAULT path and check the OUTPUT is plausibly correct, not just non-crashing.
2. **Duplicate implementations that drift** (two IFS impls; Tarjan twice). → STRATEGY: M2 shipped — factor
   shared logic into one module, import don't re-implement. Judge flags two subtasks coding the same thing.
3. **Primary command crashes at runtime** (fsdrift `snapshot` TypeError from a layer param mismatch). →
   STRATEGY: integrate-verify must RUN the primary command end-to-end with real args, not just import modules.
4. **Advertised feature crashes on realistic input** (byte-oracle `--recurse` on nested dirs). → STRATEGY:
   tests must cover ADVERTISED flags with realistic inputs (nested dirs, edge cases), never only flat/happy.
5. **Components exist but nothing wires them** (logfunnel: lexer/parser/stages but no dispatcher). →
   STRATEGY: architect includes an explicit wiring/entry subtask; integrate-verify runs the WHOLE pipeline.
6. **Tests pass but never check VALUE correctness** (tongue-id tests check only that "en" appears, not the
   score; chaos-fern never checks params). → STRATEGY: require at least one test that asserts a KNOWN
   input → KNOWN correct output (golden value), not just "output contains X" / "no exception".
7. **Spec drift** (logfunnel spec'd Rust → built Python; fsdrift advertised --exclude/--follow-symlinks
   unimplemented). → STRATEGY: judge's spec_drift verdict + integrate-verify checks output matches the spec.
8. **No runnable entry point** (antic-turmite: no __main__/CLI). → STRATEGY: architect CLI-entry rule +
   integrate-verify confirms `python3 -m <pkg>` actually runs.

## Strategy backlog (how the swarm SPLITS / ASSESSES / EXECUTES — to evolve)
- SPLIT (M3): too-big task → judge splits into smaller file-partitioned children.
- ASSESS: idle node (M5) pre-reviews completed tasks for the bugs above BEFORE integrate-verify, so a
  defect is caught while a node is free instead of shipping. Judge gains a "correctness-verify" pass that
  runs the primary feature on a known input.
- CONFIDENCE (M6): architect/worker rate honest confidence; low confidence → research to raise it.
- EXECUTE: small modular files (M2 shipped), reuse over duplicate, stop-when-green but verify-correctness.

## How this feeds back
Each cycle, the loop appends new lessons here from fresh qualitative reviews, and periodically distills the
top recurring ones into the architect/worker/judge prompt text (a real prompt edit, built+tested+committed).
That is the "organic, self-improving" mechanism: mistakes become permanent strategy.

## Distilled into prompts (M7 feedback closing)
- 2026-06-28: Lesson #1 (broken DEFAULT path while tests pass) -> integrate-verify prompt now requires
  exercising the PRIMARY/default command on a known input and CHECKING the output is correct, not just
  that it starts. (commit pending) This is a mistake-becomes-permanent-strategy instance.

## Empirical confirmation (controlled A/B, chaos-fern, 2026-06-28)
On the SAME spec, qwen's ONLY real defect was lessons #1+#2 TOGETHER: a duplicate impl (correct ifs_core.py
UNUSED) + the wired default (builtins.py) carrying CORRUPTED Barnsley params -> malformed fern. qwopus wrote
ONE correct ifs.py and rendered a real fern. So #1 (verify the default-path OUTPUT) and #2 (no duplicate
impls) are empirically the HIGHEST-value lessons — both already shipped (M7-distill + M2). The controlled
test validates the direction: these exact swarm changes would have caught qwen's bug.

## New lesson (controlled A/B, antic-turmite, 2026-06-29)
9. **Built-but-unwired headline feature** (antic-turmite: highway DETECTOR is correct but `run` never calls
   it / never prints the period the spec demanded). Reinforces #5. → STRATEGY: integrate-verify + M5 idle
   pre-review must exercise the SPEC'S HEADLINE deliverable through the default command and confirm it is
   actually surfaced, not merely that some module implements it. Also: slow TEST tasks stalled the tail and
   forced a cut before integrate-verify — M3 task-splitting on the test tasks would prevent that.

- 2026-06-29: Lesson #9 (built-but-unwired headline feature, antic-turmite) -> integrate-verify
  prompt now requires confirming the spec HEADLINE deliverable is REACHABLE through the default command,
  not merely that a module implements it.

## New lesson (controlled A/B, logfunnel, 2026-06-29)
10. **qwopus STALLS on a too-big task and plain re-dispatch can't recover it** (logfunnel stages-renderer:
    zero writes 6+ min, judge re-dispatched 2x, run never produced a dispatcher/CLI -> cut). This is the
    STRONGEST evidence FOR M3 task-splitting: a too-big PRODUCING-then-stuck task needs SPLITTING into
    smaller file-partitioned children, not just re-dispatch. Validates M3's existence; M4 should prove split
    fires on exactly this kind of task. Also: a hard, heavily-decomposed app is where qwopus's lead vanishes.

## New lesson (controlled A/B, fsdrift, 2026-06-29)
11. **Cross-module CONTRACT drift hidden by isolation-only tests** (fsdrift: snapshot writes ISO mtime, diff
    parses float -> pipeline CRASHES; 45 tests pass because each module is tested alone). The single most
    important confirmation of the deepest finding: unit tests that never run the END-TO-END pipeline LIE.
    -> STRATEGY (already targeted): integrate-verify MUST run the real multi-module pipeline (snapshot THEN
    diff), and M5 idle pre-review should exercise the integrated feature, not trust green unit tests. Also
    reinforces a shared-format/contract subtask so two modules agree on the manifest schema.

## Controlled A/B FINAL (2026-06-29): qwopus > qwen on SAME 5 apps (3W-2D-0L; means 5.8/5.6/7.6/5.6 vs
3.0/4.2/4.4/3.6). qwopus wins clean cohesive apps decisively; DRAWS on big multi-module apps — its failure
mode there (stall on too-big task, cross-module contract drift hidden by isolation tests, unwired entry) is
exactly M3/M5/M7's target. The disjoint-app confound is removed: qwopus is genuinely better, and the swarm's
next quality gains are in the multi-module-integration regime.

## New lesson (live, v8 A1-1, 2026-06-29) — fan-out must respect per-device weight
12. **Planning-phase fan-outs over-dispatched weight-1 nodes** (user observed +1 QUEUED on all 3 nodes):
    the parallel PLAN-detailing spawned every subtask spec at once round-robin (idx % num_devices), so 6
    subtasks on 3 nodes = 2 concurrent per node; LM Studio ran one and QUEUED one (the queued details
    finished at 75s vs 36-43s for the first wave). EXECUTE was already correct (the scheduler honors
    per-device weight); only the planning fan-out ignored it. -> STRATEGY (shipped 5f7fa599a):
    fanout_over_fleet, a work-stealing helper capping in-flight to <=1 call per device, routed through the
    detailer; a weight-1 node never has a second request queued behind the first. READ-THE-LOGS-FIRST held
    again — the .swarm jsonl + progress.log named the detailing phase precisely (no guessing from the LM
    Studio screenshot). Scouts / best-of-N / research-questions share the idiom and will adopt the helper
    next; they only over-dispatch in the rarer items > nodes case (lenses/questions = 4 on a 3-node fleet).

## Observation (live, v8 A1-2 spreadsheet, 2026-06-29) — judge may over-kill HARD slow tasks (WATCH)
NOT yet a confirmed lesson — one data point. A1-2's formula-parser (a genuinely hard 385-line formula
parser/evaluator) was re-dispatched 3x and drew ~67 judge_verdict events on a 2-tasks-done run; the kills
read as the idle-judge's "over_reading / produced no file yet" verdict firing while the 27B was legitimately
reasoning for minutes before its first tool call (reasoning models think long on hard tasks). Confirmed by
reading the logs that this is NOT a v8-feature bug: zero task_retry/ContentRetry events and formula_parser.py
PARSES (DONE_GATE correctly silent); the file IS produced, just slowly across re-dispatches. The scheduler
already excludes judge kills from the transient-retry budget so it is bounded, but it wastes work. IF this
recurs on the A2 multi-module runs -> candidate tuning: raise the judge's min-age/over-reading patience for
HARD-difficulty tasks (a hard task legitimately reads + reasons longer before producing), or gate the
over_reading verdict on elapsed-vs-difficulty. Gather evidence across A2 before touching the judge.
-> LIKELY RESOLVED (2nd data point, A1-3 task-scheduler): A1-3 had only 12 judge verdicts, ZERO
over_reading kills, ZERO re-dispatches — a calm judge. The A1-2 over-kills were a SYMPTOM of the detailer
filename drift: the worker wrote formula_parser.py not the owned parser.py, so at the owned path the judge
saw "no file produced" and fired over_reading/"produced no file yet" — i.e. the over-kill was DOWNSTREAM of
the drift bug (lesson 13), not an independent judge defect. With no drift (A1-3 wrote all 8 owned files
correctly) the judge stayed quiet. So the detailer fix (7e81b3b6a) probably also fixes this over-kill class.
Do NOT tune the judge; keep watching A2 to confirm.

## New lesson (live, v8 A1-2 spreadsheet FAIL, 2026-06-29)
13. **A HARD LYNCHPIN task that exhausts its attempts CASCADES the whole run.** A1-2's formula-parser —
    which formula-evaluator, cli-entry, tests, and integrate-verify ALL depend on — failed after 3
    attempts, so fail_descendants tanked 5/7 subtasks; only the two leaf data modules survived. No
    runnable app. SMOKE caught it deterministically ("no python3 -m entry point — unrunnable").
    TRUE ROOT CAUSE (found by reading the run, corrects an earlier max-turns guess): the DETAILER wrote
    a detailed spec saying "File owned: formula_parser.py" while the skeleton's owned_files was
    [spreadsheet/parser.py] — the detailer was NEVER TOLD the owned files, so it invented a contradicting
    filename. The worker followed the SPEC (formula_parser.py), so the assigned parser.py was never
    written; the hallucinated-completion guard (owned file missing -> Transient) failed the task EVERY
    attempt -> exhausted -> fail_descendants cascaded 5/7. (The 385-line formula_parser.py parsed fine, so
    DONE_GATE correctly stayed silent — this was a wrong-FILENAME failure, not a syntax/quality one; the
    judge over_reading kills were secondary.) -> PRIMARY FIX SHIPPED (7e81b3b6a): thread each subtask's
    owned_files into the detailer prompt + instruct it to use those EXACT paths verbatim (never invent/
    rename). Same class as the earlier DispatchRequest-gains-owned_files worker fix. Validate on the next
    multi-module run: detailer spec uses the assigned paths, no missing-owned-file exhaustion.
    -> SECONDARY mitigation: GOOSE_SWARM_CONTRACTS still helps decouple a genuinely-rocky lynchpin (frozen
    interface lets dependents build even if the lynchpin task is shaky) — validate on A2. NB the
    SMOKE-autofix patches WIRING (could add a __main__) but cannot conjure an absent module.
    META-LESSON: read the logs to the ACTUAL file on disk vs the planned owned_files before blaming the
    model's reasoning — a filename mismatch masquerades as a hard-task failure.

## New lesson (validating GOOSE_SWARM_REVIEW, 2026-06-29) — runs + tests + human ALL missed an unwired dup
14. **A built-but-unwired DUPLICATE hides behind a passing run.** byte-oracle — an AB 9/9/9/9 "clean win" —
    actually has detector.py UNWIRED: cli.py RE-IMPLEMENTS the whole detection inline (220 lines, its own
    detect_type + magic signatures), so detector.py is imported only by tests; plus two dead src/ modules
    nobody imports. The manual AB review MISSED all of it because `python3 -m byte_oracle` RUNS correctly
    (via the inline duplicate) and the unit tests pass (they test detector.py directly). This is the
    deepest validation of the new model-free AST reviewer (a59d53edc): SMOKE (app runs) + unit tests (green)
    + a careful human (reviewed) ALL gave byte-oracle a clean bill, yet a deterministic import-graph pass
    instantly flagged the unwired duplicate + dead code. Combines lesson #2 (duplicate impls drift) + #5
    (built-but-unwired). -> STRATEGY: run GOOSE_SWARM_REVIEW=1 on every eval; it catches a class that
    running + testing + reading cannot. NB prior "clean win" verdicts may be inflated where an inline
    duplicate masks an unwired module — the deterministic graph is the only reliable check.

## New lesson (live, v8 A1-3 task-scheduler, 2026-06-29) — inline-duplicate-unwired pattern RECURS
15. **qwopus builds a dedicated module then INLINES its logic in the CLI, leaving the module UNWIRED — and
    it passes smoke + tests + human review.** A1-3 (CLI task scheduler) DONE all 8 subtasks, 0 failed,
    SMOKE PASS, 44 pytest green, rich 8-command CLI — a clean WIN by the old criteria. But the AST reviewer
    flagged sched.store + sched.runner unwired, and RUNNING it confirmed a REAL user-facing bug: sched add
    prints "Added task" but sched list (new process) shows NOTHING — tasks DON'T PERSIST, because cli.py
    builds an in-memory Schedule() and never calls store.py (unwired); cli.py run also inlines
    subprocess.run instead of runner.py. The 44 tests pass because they test store/runner IN ISOLATION; the
    CLI never calls them (lesson #11 via UNWIRING, not drift). 2nd app after byte-oracle with this exact
    pattern => RECURRING qwopus failure mode. SMOKE (runs) + green tests + 8-command --help ALL missed it;
    the deterministic AST unwired check was the ONLY catch. -> STRATEGY: (a) GOOSE_SWARM_REVIEW=1 every run.
    (b) BUILD an AST-finding fix-dispatch (mirror SMOKE-autofix): on an unwired finding fire ONE fix worker
    told to WIRE the module in (cli loads store on start, saves on mutation) — would have fixed A1-3. (c)
    CONTRACTS may PREVENT it (frozen store/runner interface injected => cli worker imports not re-invents);
    A2-1 (contracts ON) tests this. Combines #2 (duplicate impls) + #5/#11.

## New lesson (live, v8 A2-1 ledger WIN, 2026-06-29) — the full stack converts the multi-module DRAW->WIN
16. **The full v8 stack converts the multi-module DRAW class into a WIN.** A1-2 spreadsheet (no contracts,
    minimal spec) FAILED 2/0/5/2 via the detailer-drift cascade; A1-3 scheduler (no contracts) shipped
    broken 3/5/5/4 with an unwired store (tasks did not persist); A2-1 ledger (FULL stack: CONTRACTS +
    detailer-fix + smoke + review; detailed spec) is the FIRST multi-module app in v8 that WORKS end-to-end
    — RAN it: posts balanced entries, REJECTS unbalanced ("Unbalanced journal entry"), trial-balance correct
    (debits==credits), and PERSISTENCE ROUND-TRIPS across a fresh process (the exact test A1-3 failed).
    8/6/8/8 vs AB mean 5.8/5.6/7.6/5.6. VERIFIED the mechanisms fired (read the tree): the CONTRACTS phase
    injected frozen interfaces and ledger-core imported the EXACT frozen models API (no drift); the REVIEW
    event found 0 unwired; no surviving contract stubs. CAVEAT (honest): A2-1 was MAX-DETAIL vs A1's MINIMAL,
    so the detailed spec also contributes — a contracts-OFF same-spec run would isolate contracts. Still, the
    two draw-class failure mechanisms that sank A1-2/A1-3 (cross-module drift cascade; built-but-unwired) did
    NOT occur with the full stack — the strongest end-to-end evidence the v8 build targets the right thing.

## New lesson (live, v8 A2-2 log-DSL WIN, 2026-06-29) — 2nd DRAW->WIN; the logfunnel class is tamed
17. **The logfunnel STALL/no-dispatcher class is tamed (A2-2 log-pipeline DSL = WIN 8/8/8/8).** logfunnel in
    the AB STALLED — stages built with NO dispatcher to wire them (lesson #5). A2-2 (full v8 stack) WORKS,
    verified: RAN "filter ERROR | count" -> 2 and "filter ERROR | upper" -> ERROR X / ERROR Z (multi-stage,
    correctly wired); 44 tests; SMOKE pass; AST review 0 unwired. The architect planned an explicit
    runner-module (dispatcher); CONTRACTS froze the tokens/parser/stages interfaces so dependents wired
    against them. With A2-1 (ledger WIN), A2 = WIN/WIN — both hard multi-module max-detail apps work under
    the full stack, while the same regime FAILED (A1-2) / shipped broken (A1-3) without it. The v8 build
    hits its target consistently across two distinct draw classes (contract-drift cascade + no-dispatcher
    stall). The contract-stub-cleanup prompt fix also held (0 stray stubs this run).

## New lesson (live, v8 A2 = 3/3, 2026-06-29) — full stack converts the multi-module DRAW->WIN, confirmed x3
18. **A2 = 3-FOR-3: the full v8 stack converts the multi-module DRAW class into WINs across 3 distinct apps.**
    A2-1 ledger (contract-drift class) WIN 8/6/8/8; A2-2 log-DSL (logfunnel no-dispatcher class) WIN 8/8/8/8;
    A2-3 state-machine WIN 8/6/8/8 (RAN it: valid transition persists, invalid rejected with exit 1, graph
    validation). ALL on the full stack (CONTRACTS + detailer-fix + smoke + review + done_gate), each VERIFIED
    end-to-end (ran the app) + AST-review-clean (0 unwired) + 0 stray stubs. vs the multi-module A1 runs
    WITHOUT contracts: A1-2 FAIL 2/0/5/2 (drift cascade) + A1-3 broken 3/5/5/4 (store unwired). The v8 build
    does what it set out to: make a weak 27B fleet deliver WORKING multi-module apps. CAVEAT (repeated
    honestly): A2 is max-detail vs A1's minimal, so spec detail also contributes — the cleanest isolation is
    a contracts-OFF same-spec A2 run (good next experiment). But the failure MECHANISMS that sank A1-2/A1-3
    (drift, unwiring, stub-pollution) were each observed PREVENTED on A2.

## New lesson (live, v8 A3-1 chaos-fern --svg, 2026-06-29) — AMENDMENT re-architect + Playwright env wall
19. **AMENDMENT failure mode: the architect RE-ARCHITECTS an existing project instead of editing in place.**
    Asked to ADD a --svg flag to chaos-fern, the architect created NEW parallel modules (fern.py/
    render_ascii.py/export_svg.py) + rewired cli.py to them, ABANDONING the originals (renderer.py/ifs.py/
    chaos_game.py = unwired duplicates), broke test_cli.py collection, and the run THRASHED (cli-entry/
    tests-svg judge-killed + re-dispatched, no writes 150s, CUT at 34min). The --svg feature itself WORKED
    (correct 100k-element Barnsley SVG; ASCII default preserved), but as a messy rewrite. The AST reviewer
    CORRECTLY flagged the 3 abandoned originals as unwired — a real demo. Score 5/3/4/6. -> CANDIDATE FIXES
    (confirm recurrence on A3-2 first): (a) ARCHITECT amendment rule — when the manifest already lists
    project files, EDIT them at their EXACT paths; NEVER create a parallel renamed module (render_ascii.py
    beside renderer.py); A3-2 launched WITH an explicit edit-in-place instruction to test if that alone
    fixes it. (b) the WIRE-FIX mis-applies to amendments — it would WIRE the abandoned duplicates back in
    (wrong; they should be DELETED) — so wire-fix should be cautious on amendment runs. NB all the v8 WINS
    are GREENFIELD (A1-1, A2 x3); amendments are a separate, harder regime.
    PLAYWRIGHT LIMITATION (env): the MCP browser LAUNCHES (about:blank renders) but cannot render real
    content in this sandbox — file:// blocked, localhost HTTP network-isolated (127.0.0.1 times out), and
    data: URLs with SVG content time out (30s) then hang the backend. 6 attempts, 4 approaches. For SVG/web
    verification here, fall back to STRUCTURAL + ALGORITHMIC checks (valid SVG + the generating algorithm +
    a matching ASCII render) rather than a pixel screenshot.

## New lesson (live, v8 A3-2 byte-oracle --json, 2026-06-29) — amendment WRONG-PATH + FALSE-GREEN + wire-fix
20. **AMENDMENT failure mode #2 (distinct from #1 re-architecture): WRONG-PATH write + FALSE-GREEN tests.**
    With an explicit edit-in-place instruction (OLD binary), the worker did NOT parallel-rename (the
    byte_oracle package stayed intact) but wrote its --json `cli.py` to the CWD ROOT instead of editing
    `byte_oracle/cli.py` — so `python3 -m byte_oracle --json` -> "error: unrecognized arguments: --json"
    while the feature sits DEAD in a stray root file the package entry never imports. 135 pytest PASS (the
    test imports the stray file / json fn directly) and smoke passed (it only checks `--help` exit 0) — so
    BOTH green signals were FALSE; only RUNNING the real feature exposed it (VERIFY-don't-trust, again). The
    AST reviewer DID flag the stray 'cli' unwired. Root: the amendment subtask owned a bare `cli.py` (root),
    not the package-qualified `byte_oracle/cli.py` (my spec also said "edit the existing cli.py" unqualified).
    f9e89b782's "own the EXACT existing path" clause targets this; A3-3 (new binary, NO instruction) tests it.
    + **WIRE-FIX mis-applies on amendments:** it tried to wire the PRE-EXISTING intentional detector dup
    (byte-oracle ships detector.py unwired; cli inlines detection — lesson 14) AND the stray cli, ran 14 shell
    calls ~9min without resolving -> cut. CANDIDATE FIX (confirm on A3-3): AST review/wire-fix should only
    flag/fix modules NEWLY created THIS run (diff vs the amendment's original manifest), never pre-existing.
    NET: amendments are the hard regime — TWO distinct failure modes now (re-architecture A3-1; wrong-path
    A3-2); greenfield is solid (A1-1, A2 x3). The deterministic gates each PARTIALLY helped (AST flagged the
    stray) but none alone catches a wrong-path feature whose tests falsely pass.

## New lesson (live, v8 TS-1 first non-Python run, 2026-06-29) — de-Python chain validated end-to-end
21. **FIRST non-Python run (TS-1, TypeScript todo CLI) validates the de-Python work AND shows ALL of it is
    needed, not just the architect.** On the architect-only binary: the architect planned correct TypeScript
    (src/*.ts modules + vitest + tsconfig + package.json, ZERO .py) and the workers wrote real TS whose logic
    passes 30 vitest tests — strong proof the de-Python architect works. BUT (a) the CLI ENTRY crashed at
    runtime (`new URL(process.argv[1])` on a plain path -> Node ERR_INVALID_URL; FALSE-GREEN — the unit tests
    bypass the entry, exactly the A3-2 class), and (b) integrate-verify THRASHED 13x because the still-Python
    WORKER prompt told it to run `pytest` on a TS project. So the very gate meant to catch an entry crash (RUN
    the real entry) couldn't, and the bug shipped. LESSON: de-Pythoning the architect is necessary but NOT
    sufficient — the WORKER prompt + integrate-verify + gates must be language-aware too, or the verify layer
    is BLIND on non-Python and false-green escapes. The shipped fixes (worker 75682ae7c, planner 12cd6a744,
    smoke-skip dcf6a6b2e) target exactly this; RUST-1 (next, on the fixed binary) tests the full chain.
    Cross-language confirmation that VERIFY-don't-trust + RUN-the-real-entry is the load-bearing principle
    regardless of language — a green test suite never proves a non-Python CLI runs any more than a Python one.

## New lesson (live, v8 inquisitive-swarm handshake PROVEN, 2026-06-29) — confidence-gated USER questions
22. **INQUISITIVE SWARM validated LIVE — the harness-mediated Q&A closed loop works end-to-end.** The user's
    ask: with local models, the swarm should ASK the user more, gated by the confidence meter (he calls it
    "the single most important thing"), and — since this autonomous loop IMPERSONATES the human — the harness
    answers and feeds it back. Built GOOSE_SWARM_ASK_FLOOR (commit cf573d811, after an adversarial review that
    caught + fixed a proceed-on-default regression + a PTY-hang). LIVE PROOF (floor=100, vague spec "build a
    tool to process logs"): swarm computed plan confidence 82/100 (best-of-2 cross-draft agreement — the M6
    meter that was previously COMPUTED THEN DISCARDED, now the live trigger); the verbalized step returned
    EMPTY uncertainties so the GENERIC-FALLBACK question fired (review-fix: below floor ALWAYS asks, never
    proceed-on-default); it wrote .swarm/clarify-questions.json, emitted low_confidence_ask, and BLOCKED.
    The HARNESS (this loop, AS the human) wrote .swarm/clarify-answers.json with a concrete answer (parse
    syslog/JSON/nginx, never crash on a bad line, per-level summary + --json); within ~10s the swarm logged
    "clarifications received — re-planning", emitted low_confidence_answered, folded the Q&A into the planner
    findings, and RE-PLANNED — exactly ONCE (the `asked` flag bounds it), then proceeds to EXECUTE. The closed
    Q&A loop through the harness is real, not a side-channel. Detached detection = stdin&&stdout both TTY else
    file handshake (+ GOOSE_SWARM_ASK_FILE override) so a detached/eval run never hangs. Default-OFF
    (unset/0) so every existing run is byte-identical. Next: a dedicated question GENERATOR (real
    interrogatives from spec+plan, not split-the-uncertainty-string) + local-model-strength floor scaling.

## New lesson (live, 2026-06-29) — flaky early shared/types subtask = weak-model EXPLORE-not-WRITE (no clean fix)
23. **The recurring flaky early "shared/types" subtask is a weak-model EXPLORE-INSTEAD-OF-WRITE pattern, not a
    new bug.** Read-the-logs on ASK-TEST3 shared-types (owned dedup_csv/__init__.py + types.py, dispatched 3x,
    FAILED): activity = 7 tool_calls, ALL shell (cat/ls), ZERO writes — the worker explored instead of writing,
    claimed done, the guard caught the missing files, retried, repeated the same, exhausted. The worker prompt
    ALREADY says "WRITE FIRST, do NOT ls/cat"; the missing-files guard + the new guided retry (92f393495) both
    fire; yet the weak 27b still explores. Same class as A1-2 (minimal-spec flake). KEY MITIGATION already in
    the stack: CONTRACTS pre-freezes the type signatures as a stub the worker just writes, removing the "what
    types do I write?" ambiguity that triggers exploration — the A2 wins (CONTRACTS on) did NOT flake; the
    inquisitive ASK-TEST runs had CONTRACTS OFF and hit it. So this is a KNOWN no-contracts-regime flake, NOT a
    regression, with no clean additional swarm-side fix beyond what exists. Noted, NOT over-built. Practical:
    enable CONTRACTS for any multi-module run. The inquisitive feature itself is unaffected + fully validated.
