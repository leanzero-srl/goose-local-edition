
---

## F126 — those "retries" are the JUDGE killing workers, and 79% of its interventions are one word: SPIN

F124 measured that hard test tasks retry at 60%. It did not ask WHAT a retry is, and the answer
changes the meaning of the number.

**59 of 67 re-dispatches (88%) were preceded, within 180 s, by a `judge_verdict` carrying a hint on
that exact task.** Only 8 had none. A "retry" is overwhelmingly not the engine failing a worker —
it is the idle-node judge stopping one and re-dispatching it with a SUPERVISOR NOTE. swarm.rs:19776
is explicit: *"If the idle-model judge killed a prior attempt, lead with its corrective hint."*

### The taxonomy of all 73 interventions across 17 runs

| n | intervention | test / other |
|---|---|---|
| **40** | "your owned file(s) are written but unchanged for minutes while you keep running — you are stuck re-reading or re-verifying" | 28 / 12 |
| **11** | "you have produced no file yet. STOP reading/deliberating — WRITE your owned file(s) now" | 6 test / 5 sink |
| **7** | "produced no file yet and have taken no action at all — you are deliberating instead of building" | 5 / 2 |
| 14 | **specific code defects**, one each | 13 / 1 |
| 1 | finalize-spin salvaged | 0 / 1 |

**58 of 73 (79%) are process interventions — SPIN.** Three generic messages, and the dominant one by
far is *wrote the file, then kept running without changing it*.

**Test tasks take 52 of 73 interventions (71%)** while being 46 of 239 dispatched tasks (19%). On the
spin messages specifically, 39 of 58.

### The hypothesis this points at — and it is a hypothesis, not a measurement

The OFF-path stopping rule every worker receives (swarm.rs:19627) is *"STOP WHEN GREEN. The MOMENT
your file's tests pass, call final_output and finish."* **A test author has no such moment.** It is
writing tests against a module somebody else built, which may be broken — F75 records the repair tail
never going green, 13/13. Green never arrives, the stop condition never fires, and the worker keeps
running against a file it has already written. That is precisely the 40x signature.

`kind_prompt`'s ON-path rule for this kind (swarm.rs:19619) is *"STOP WHEN YOUR TESTS RUN. Run it
once to prove it collects and executes — a test file with a SyntaxError or a bad import is worse than
none. Then finish; do not chase coverage."* **That is a stop condition a test author can actually
reach.** So the lever's text matches the measured failure mode at the mechanism level, which is a far
stronger case than F124's "60% retry, rules are mismatched" — and it is exactly what lesson 8 asks
for before spending a unit. It remains a hypothesis until the arm runs; the readout is unchanged.

### The judge's other 19% is excellent, and that is worth saying plainly

The 14 specific findings are real engineering: `EXPECTED_SORTED_IDS` has the wrong order because
pay_005 at +01:00 converts to 07:00Z; `SOL_SOCKET` used in `free_port()` but not imported; a
module-level `req()` helper referencing a pytest fixture only valid inside test functions;
`HTTPServer(("127.0.0.1", 0), None)` failing because it requires a handler class; a worker mocking
`requests.request` when the spec says stdlib-only; a worker testing its own `make_parser()` replica
instead of the real CLI. This is the strongest evidence yet for F123's surviving claim that the judge
does real work — 14 genuine defects, not 5.

### One reading of my own I checked and dropped

The attempt-duration histogram put 32 of ~103 gaps in a single 60 s bucket (420-479 s) and I was
ready to call it a timeout constant. The raw values spread 420.1 -> 488.6 s across seven different
task ids including `integrate-verify`, `cli` and `frontend` — a cluster, not a constant. And
`worker_timeout_secs` cannot be the cause anyway: swarm.rs:19770 says it is **idle-based, not
wall-clock** — it aborts only when NO agent event arrives, so a slow-but-progressing worker is never
killed by it. A 60 s bucket is wide enough to manufacture a spike out of a broad cluster; the fix was
to print the raw values, which took one command.

## F164 — 93% of every failure this campaign has ever recorded is one dispatch kind

Across **12 finished 3-node runs**, by kind:

    implementer     65 completed     0 failed      0%
    test-author     42 completed    13 failed     31%
    verify/sink     97 completed     1 failed      1%

Per task, which is the authoritative view (the kind-classifier put `integrate-verify` in the
implementer column on the one run where it was dispatched WITH owned files — the totals are right,
the label was not):

    test-meridian          ran 10   FAILED 6   60%
    test-api               ran 10   FAILED 5   50%
    test-api-edge-cases    ran  2   FAILED 1   50%
    test-api-server        ran  1   FAILED 1  100%
    integrate-verify       ran 12   FAILED 1    8%

**Fourteen failures, five distinct tasks, and thirteen of them are test-authors. No implementer has
ever failed** — not once in 65 completions, across every engine build this campaign has run. The
single non-test failure is the sink.

A 31% failure rate against 0% is not a spread I need more replicates to believe; n is 42 and 65.

**This unifies the night.** Every independent thread converged on the same population without my
looking for it:

    F156  test-authors carry 22,511-char prompts, 2.3x the implementer's 9,860
    F159  3 of 3 judge interventions hit test-authors, 0 hit implementers; 3.3x the dry reasoning;
          the `test_meridian.py` author gets `## API of` for ALL FIVE modules (265 lines of BODY
          against 35 of signature, six private methods) when its declared dep is `meridian` alone
    F163  the stall detector cannot see them, because flat counters cannot distinguish frozen from
          writing
    F164  they are 93% of all failures, at 31% versus 0%

The swarm does not have a general reliability problem. It has ONE broken dispatch kind, and every
measurement I have taken tonight from a different direction has landed on it.

**And `test-meridian` is failing AGAIN right now** — re-dispatched 3x on r0 and not converging, which
would make it 7 of 11. F143 recorded this exact task killed 3x to FAILED on an earlier build; it was
read then as an instance and it is a *pattern*: the same task, the same way, on 60% of the runs it
appears in.

**Priority consequence.** `scoped_contracts` (F159) is aimed precisely at this population and has
never run once. Its arm is queued at reps=3 with the readout on test-authors only. After the baseline
reaches n=3 it is the first arm that should execute — ahead of `sink_review`, `split_inherit_spec`
and everything else in the queue, because every other arm is tuning a population that does not fail.

CAVEAT, stated: these 12 runs span several engine builds, so this is not a controlled comparison. It
is stronger than one for this purpose — the concentration survived every build change, which is what
a structural defect looks like and what a build-specific artefact does not.
