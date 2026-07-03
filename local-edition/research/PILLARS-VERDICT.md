# GOOSE_SWARM_GOALS (pillars) — verdict (2026-07-03)

## What shipped + gated (local-edition)
- **Part 1 distill + Part 3 inject** (faf733ecc): at plan time the swarm distills 3-7 app PILLARS (acceptance criteria + interface/invariant shape) from spec+research+plan -> `.swarm/pillars.json` + a pillars event, and injects a rendered pillars block FIRST into every worker prompt so modules cohere through context compaction. Mirrors the CONTRACTS OnceLock pattern; default OFF -> off-path byte-identical.
- **Part 5 judge-on-goals** (c9cdd8feb): the judge + pre_review read the pillars and ground their existing SPEC_DRIFT/ISSUE verdicts in the concrete criteria (conservative, HIGH-confidence only).

## The rate A/B — 3x3, same natural spendlog spec, GOOSE_SWARM_GOALS on vs off
`report_budget_ok` = the built app exposes `report budget` (NOT `budget report`) and it flags OVER, verified by RUNNING. `pytest_pass` = the app's own suite is green. `pillars_count` = pillars distilled (ON only).

| metric | OFF (n=3) | ON (n=3) |
|---|---|---|
| report_budget_ok (interface held) | 2/3 | **3/3** |
| over_flagged | 2/3 | 3/3 |
| pytest_pass | 3/3 | 2/3 |
| pillars (mean) | 0 | 6.7 (6,7,7) |

Per-run: OFF = [held, DRIFTED, held]; ON = [held+pytest-red, held+green, held+green].

## Verdict (honest)
- **The mechanism is proven.** Distill reliably produces 6-7 load-bearing pillars every run (capturing the report-budget interface verbatim + the shared-store invariant + money precision). Injection reaches every worker. The judge is goal-aware. All gated (build+clippy -D warnings+tests) and default-OFF.
- **Interface adherence is directionally better with pillars: ON 3/3 vs OFF 2/3.** OFF drifted the interface once (built `budget report`); ON held it in all three. That is exactly what pillars target, and the direction matches the mechanism. BUT at **N=3 this is within noise** — the gap is a single run (3/3 vs 2/3), not a significant effect. Honest: the direction is positive and mechanism-consistent, but proving a rate improvement needs more reps.
- **Pillars are NOT a general correctness fix: pytest ON 2/3 vs OFF 3/3.** One ON run held the interface but shipped a failing test suite (on-1). Pillars anchor the GOAL/interface, not every unit test. This is the exact gap that motivates the separate GOOSE_SWARM_COMPLETE feature (verify-by-running + never-ship-red).
- BONUS: the sink-cap (Option B) fired in 2 of the 6 runs — more real-run validation of that earlier fix (and it surfaced the idle-only limitation now fixed by the hard-ceiling follow-up).

## Net
Pillars are a real, working, shippable feature: modules now share a distilled north star and the judge holds it, and interface adherence trends up (3/3 vs 2/3). The aggregate rate effect is directionally positive but not significant at N=3; a larger campaign (or folding pillar `check` into GOOSE_SWARM_COMPLETE's deterministic gate) is how you turn "trends up" into "provably up". Kept default OFF pending that evidence.

Caveats: N=3 per arm, single spec (spendlog). This A/B used the SAME natural spec for both arms (clean isolation, unlike the earlier ultra-explicit-spec confound).
