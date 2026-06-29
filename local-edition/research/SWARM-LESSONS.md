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
