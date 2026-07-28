# swarm-bench — the brief

_Written 2026-07-28 after a failed attempt. Read the failure section first; it is the most useful
part of this document._

## What this is for

A continuous instrument that measures **our swarm** — its phases, on our archetypes — so we can tune
levers against evidence and publish comparisons worth reading. It is a tuning instrument first and a
leaderboard second. If those two ever conflict, the instrument wins.

Not a generic agent leaderboard. That already exists (`evals/harbor` → Terminal-Bench 2.0, 89 real
tasks, goose at 50.6% stock / 57.3% code-mode against opencode 52.8% and pi 47.2%). Use harbor when
the question is "how does goose compare to other harnesses". Use this when the question is
**"which lever made our swarm better, and where in the run did it help"**.

---

## READ THIS FIRST — how the last attempt failed

An entire build was thrown away. Every failure below is cheap to repeat and expensive to discover.

1. **Vendor-authored toy tasks are not a benchmark.** Six synthetic exercises written in an afternoon
   by the same person publishing the results. Nobody adopts that. Real corpora are independent
   (SWE-bench: 500 real issues screened by 93 engineers) or at minimum use *our own* established
   archetypes rather than freshly invented ones.
2. **Everything scored 100%.** Frontier models cleared every fixture at every difficulty. A card
   reading `100.0 / 100.0 / 100.0` ranks nobody. **Calibrate difficulty against a baseline BEFORE
   spending fleet time** — if the reference tier does not land in roughly 40–70%, the fixture is not
   ready and no amount of statistics fixes it.
3. **A grader can fail in both directions.** After fixing saturation, one missing subcommand
   cascaded a mostly-correct build to 0/44 because the contract suite called `init` in shared setup.
   **Tests must be independent; no shared setup step may be able to zero a suite.**
4. **The comparison was rigged by accident.** Cloud models ran single-agent while the local fleet ran
   the swarm — a harness comparison wearing a model comparison's clothes. 75 of 77 episodes were
   built that way before anyone noticed.
5. **Effort went to plumbing instead of the product.** Days of statistical rigour — Wilson intervals,
   bootstrap, drift, tamper detection — wrapped around tasks that measured nothing. Rigour is
   worthless applied to the wrong question. **Get one real, discriminating measurement before
   building any infrastructure around it.**

---

## What already exists and must be reused

| Asset | Where | Why it matters |
|---|---|---|
| **Archetypes** | `harness/generator.py: ARCHETYPES` | `heavy-spec`, `minimal-spec`, `continue-existing` — our taxonomy, already validated over a campaign |
| **`hidden_requirements`** | same | The swarm never sees them. Grading against what a competent engineer would INFER is where spread comes from |
| **Phase timings** | `run_finished.phases` | `research_min`, `planning_min`, `execute_min`, `gates_min`, `total_min` |
| **Levers** | `swarm.rs` `levers_resolved` (~94) | The independent variable. This is the whole point |
| **Campaign harness** | `~/goose-builds/loop-state` | One lever per arm, same spec, LEDGER.tsv |
| **Execution graders** | `evals/agent-board/probes/` | Grade outside the workspace, restore protected files, tamper detection — sound, reusable |
| **Cloud-through-our-engine** | `evals/agent-board/runner/swarm_profile.py` + LiteLLM gateway | Proven: Claude runs through the real swarm. 7/7 tasks, 0 retries, 295s |
| **Real cross-harness numbers** | `evals/harbor` | Terminal-Bench 2.0, already comparable to published results |

---

## The measurement

**Unit:** one swarm run = (archetype × spec × lever-profile × fleet). Never a single agent call.

**Score the PHASES, not just the outcome.** The outcome alone cannot tell you which lever to turn.

| Phase | Question | Signal |
|---|---|---|
| research | did it find what it needed? | `research_completed.findings`, MCP tools actually called vs `expected_mcp`, `research_min` |
| planning | is the plan the right shape? | `plan_loaded.task_count` vs band, DAG validity, `plan_confidence`, replans |
| execute | did the fleet do the work? | task completion, non-transient retries, occupancy, `cross_module_drift` |
| gates | did the gates catch anything real? | `review.new_findings`, `pre_review.had_findings`, `complete_fix_wave` |
| delivery | did it finish and tell the truth? | `run_finished` present, false-green vs the execution grader |

**Outcome grading, in priority order.** Deterministic checks first, then `hidden_requirements`
(the spread axis), then functional probe. **Never** `complete_result.passed` or model prose — those
are only the *claim* side of honesty. Crashes and timeouts score 0 and stay in the denominator.

**Comparisons the design must support:**
- lever profile A vs B, same archetype, same fleet (**the primary one — this is the tuning loop**)
- node count 1 / 2 / 3
- local fleet vs Claude *through the same engine* (never local-swarm vs cloud-single-agent)

---

## Build order — one real measurement before any infrastructure

1. **Calibrate.** Take ONE archetype, run the reference tier, confirm it lands 40–70%. If not, fix
   the task. **No further work until a real spread exists.** This is the step the last attempt skipped.
2. **One lever, end to end.** Same archetype and spec, lever on vs off, enough reps to clear the noise
   floor. Produce one defensible sentence: *"lever X moved phase Y by Z, ±band."*
3. Only then generalise: more archetypes, phase scoring, the card, the site export.

## Done means

A number that changes a decision. Concretely: *"turning lever X on raises execute-phase completion
from A to B (±band, n reps), and costs C minutes"* — reproducible, with the raw event streams kept.

Nothing ships that cannot survive someone re-running it.

## Standing rules

- Assume the scorer is wrong until the corpus says otherwise. Green on the first run means the test
  is weak.
- Before trusting any zero, run a positive control on the same object.
- Report actual numbers, including what was skipped.
- Commit every iteration; keep the raw JSONL.
- State confidence honestly, and say plainly when a metric cannot be computed rather than imputing it.
