
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
