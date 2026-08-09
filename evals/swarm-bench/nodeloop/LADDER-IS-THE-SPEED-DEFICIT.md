# The three-node speed deficit is the confidence ladder, and nothing else

**Status: the mechanism is closed. The fix is NOT landed, and the last gate is a rebuild, not a
measurement.** Everything below is deduped (F651 found my own corpus double-counting 8 of 42 runs;
the numbers here are the corrected ones).

## The finding, in one table

Minutes, baseline arm, non-void, >=30 min, **deduped** by `(cell, finished_at)` — n=9 one-node
against 13 three-node:

| phase | 1-node | 3-node | diff | SE | SE units |
|---|---|---|---|---|---|
| **WALL** | 98.1 | 102.3 | +4.1 | 10.9 | +0.38 |
| prefix | 20.9 | 32.7 | **+11.8** | 3.9 | **+2.99** |
| &nbsp;&nbsp;research | 7.9 | 6.9 | −1.0 | 1.2 | −0.83 |
| &nbsp;&nbsp;**planning** | 13.0 | 25.8 | **+12.8** | 4.0 | **+3.23** |
| **EXECUTE** | 77.2 | 69.6 | −7.7 | 9.9 | −0.78 |

**Planning at 3.23 SE is the only quantity this campaign has produced that clears noise.** Every
other headline sits under one standard error — quality +0.0484 (0.84), wall (0.38), research (0.83),
research grounding (0.94).

**REPLICATED 2026-08-09 (F653) on five more cells** — corpus 9v13 -> 11v15, same conclusions, and the
ladder cost landed at **24.9 min**, a third independent estimate. Meanwhile the arm gap moved AGAINST
three nodes: speed is now **+10.0 min at 1.04 SE**, the first time this campaign's deficit has
cleared one standard error, and the equal-n tier table (8 vs 8) reads 1-node 0.731 against 3-node
0.698.


## ⭐ THE ENGINE ALREADY KNEW: 7 OF 7 LADDERS WERE UNNECESSARY (F685, 2026-08-09)

**`plan_convergence` carries a shadow counterfactual nobody had read.** `would_skip_ladder` answers,
for free on every run with the lever OFF, *"would enabling `diverse_plan` have skipped the redraft
ladder?"* — swarm.rs:9368 built it deliberately, and warns that the event and the enforce branch must
evaluate ONE predicate or "the shadow would then be confidently wrong, which is worse than absent
because it gets believed."

**Among runs that laddered, it reads TRUE in 7 of 7 — 100%.** Present in all 16 distinct runs
(deduped by `run_id`), so this is the whole corpus, not one build.

⚠️ **BASE-RATE CONTROL (F687) — READ THIS BEFORE QUOTING THE 7/7.** The predicate is not vacuous:
it is FALSE on 3 of 16 runs, so 7/7 among ladderers carries information. **But the three FALSE runs
are exactly the three with agreement = 100**, where there is no ladder to skip — and `struct_conv`
reads **93, 95 or 100 in all sixteen runs against a `struct_stop` of 80**. The threshold is never
remotely near the data (minimum margin 13 points). So the honest statement is NOT "the engine
identified these particular ladders as unnecessary" but **"the structural signal clears its bar in
every run ever measured, so enabling `diverse_plan` would skip essentially EVERY ladder"** — a
materially different lever, closer to *turn the ladder off* than to *turn off the unnecessary ones*.
**That sharpens the quality risk rather than softening it:** a lever that cannot tell a wasteful
ladder from a needed one makes F651's laddering-scores-higher signal governing, not a footnote.

**A fully traced instance, live on 2026-08-09:**

| at | event |
|---|---|
| +15.0m | agreement 81; **`struct_conv` 93 clears `struct_stop` 80**; `would_skip_ladder` TRUE, `enforced` FALSE |
| +30.8m | `confidence_retarget` fires, `binding_signal: agreement` |
| +35.5m | redraft's drafts agree 100 — but the resulting **plan scores 52** |
| +40.8m | `stall_stop`: "1 round failed to beat the best confidence (81)" |
| +44.5m | `plan_loaded` ships **agreement 81 — the exact plan held at +15.0m** |

**The ladder cost 13.7 minutes and changed nothing.** The monotonic-best guard worked perfectly; it
could not give the time back.

⚠️ **THIS DOES NOT AUTHORISE FLIPPING THE DEFAULT.** `would_skip_ladder` says the ladder was
structurally *unnecessary*, not that the build would have been as *good*. Skipping ships a
lower-confidence plan, and F651 has laddering runs at 0.7490 against 0.7140 (0.39 SE) — the author's
original hold. **Quality is the gate: 13.7 min for 0.02 of score is a LOSS.** The action is to RUN
the `diverse_plan` arm, which is already queued and has never executed.

## ⚠️ TWO CORRECTIONS TO THIS DOCUMENT'S OWN FRAMING (2026-08-09)

- **"Three nodes is losing" was never supported.** On 27 non-void rows: wall **+8.11 ± 10.04 (0.81
  SE)**, score **−0.0038 ± 0.0716 (0.05 SE)**. Both under one SE. The honest claim is **NOT
  MEASURABLE**. This retracts the earlier "+10.0 min at 1.04 SE — the first time the deficit cleared
  one SE": one additional row put it back under, and a difference that crosses and re-crosses on a
  single row was never a finding.
- **The 3-node wall of 102.1 min is stale** — it is **105.80** on the current 16 rows.
- **The pool-invariant fix reaches at most 20% of runs (F683).** It is dead whenever fewer than three
  drafts return, and the fleet requests three and gets **2.4** on average — because
  `collect_drafts_with_straggler_stop` *deliberately* aborts the lone last-place draft once two valid
  ones land (F684, `straggler_aborted`). That is a designed speed trade, not a defect, and it costs
  nothing measurable (0.20 SE, Fisher p=1.000).

## The planning tax is entirely the ladder

Within three-node runs only, split by whether a confidence-ladder round fired:

| group | n | planning |
|---|---|---|
| 3-node, laddered | 6 | **39.5 min** |
| 3-node, no ladder | 7 | **14.0 min** |
| 1-node (never ladders) | 9 | **13.0 min** |

- Laddering costs **+25.4 min** — and F610 independently priced a single ladder round at **25.0 min
  (6.73 SE)** by a completely different route.
- **A three-node run that never ladders plans at 14.0 min against one-node's 13.0 — a gap of +1.1.**
  There is no second cause. Nothing else about having three nodes slows planning down.

## Why it is a three-node event

**One node drafts exactly ONE skeleton** — `skeleton_drafts.requested` is 1 in 22 of 22 one-node runs
(F659). Cross-draft agreement is therefore undefined, and the retarget site fires only *"when
AGREEMENT is the binding signal"* — `binding_signal` explicitly accepts a missing agreement
(swarm.rs:6735). **So the ladder cannot fire at one node**, and no convergence is even measured:
`plan_convergence` appears in ZERO of 24 one-node logs (F658). Confirmed in the data six separate
times: **every one-node run reports `redraft_rounds` zero.**

⚠️ **CORRECTED (F660).** This section previously credited F607's `idle_capacity() >= 2` proof. That
proof governs the DYNAMIC REPLANNER, not the confidence ladder — a neighbouring mechanism. F607 is
still correct and still what makes the replan-injection findings attributable (F618/F621/F622/F635);
it was simply the wrong citation here. No measured number changes.

And F612 measured *why* it fires: **4 of 5 laddering rounds had `agreement_best2` clearing the floor
while `agreement_conf` did not** — i.e. the round was bought by pool size, not by real disagreement.
The engine author's own comment on that: *"the invariant measure is applied only AFTER the ladder has
been triggered, never at the round-1 decision point where the pool-size penalty is what triggers it.
That is backwards."*

## The author's hold, and its discharge

They wrote, and held the fix on it:

> *Emitted rather than acted on: the cells that laddered scored 0.9343/0.7147/0.8157 against
> 0.6030/0.6695 for the two that did not, so the ladder may be buying the quality and a silent flip
> here could spend that. **Measure first.***

**Measured.** Within three nodes, laddering scores **0.7490 (n=6) against 0.7197 (n=7) — +0.0293 at
0.31 SE**, under the 0.05 bar, **and the median runs the other way** (0.7192 vs 0.7426). Their
five-cell 0.185 does not replicate.

**So the objection they named is not supported by the data they asked for.** The ladder costs 25.4
minutes a round and buys 0.0293 of score at a third of a standard error.

## What the engine ALREADY does about the ladder — read this before proposing anything (F657)

Two mechanisms exist that I did not know about until I grepped, and both blunt the obvious follow-up
ideas. **Do not propose either of them again.**

- **The best round is kept, not the last.** `swarm.rs:24853`: *"Monotonic best (retarget only):
  remember the highest-confidence plan so a re-draft that happens to diverge can never ship worse
  than the best already measured."* So a run whose rounds read 83 then 60 **ships the 83 plan**. A
  per-round `agreement_conf` sequence is a measurement, NOT what the run lives with — I briefly
  believed otherwise and the source refuted it.
- **A stall guard stops a ladder that is not climbing**, default `true` (`swarm.rs:1250`), gated by
  `GOOSE_SWARM_RETARGET_STALL_GUARD`. Its comment: *"a redraft that failed to beat the best
  confidence already measured is evidence the ladder is not climbing, and every further rung costs a
  full planning pass across the fleet (**~20 min measured**) to ship a plan `best_plan` is already
  holding."*

**That ~20 min is a FOURTH independent estimate of the rung cost**, reached by the engine author from
different data, sitting beside 24.9 / 25.0 / 25.4.

**What the ladder does achieve, stated fairly (F656, n=6):** it reaches the 85 floor in only 2 of 6
runs, but measured first-round-to-best it gains **+11 points on average**. It is not useless. The
case for the pool-invariant fix rests on ladders that **should never have fired**, not on how badly
the ones that fire perform.

## What this does NOT establish — read before acting

- **Runs are not randomised into laddering.** The ladder fires precisely when the draft skeletons
  disagree, so laddering runs may be the ones facing a harder decomposition. This shows **where the
  time goes**, never that removing the ladder **returns** it.
- **The execute benefit is indicated, not proven.** Three nodes finish execute 7.7 min faster, but at
  0.78 SE. The asymmetry is the point: **the tax clears noise, the benefit does not.**
- **A causal quality answer is unaffordable.** F611 priced it at 183 cells per bucket against a
  corpus of ~50.
- **n is 6 against 7** on the decisive comparison. It clears the pre-registered minimum of 3 and
  nothing more.

## The change, and the one remaining gate

The fix is the engine author's own, already specified in `scorer-queue/README.md` entry 6: wire
`best_subset_agreement(k=2)` into the **round-1 decision point** rather than only into retarget, so a
growing pool cannot manufacture the pool penalty that triggers the ladder. `k=2` is what a one-node
fleet drafts, so it reports every fleet on the same footing and is a **no-op at one node**.

**The no-op is verified, and NOT for the reason first published (F658, explained by F659).** I first
argued it from value equality — *one node drafts 2, so `best_subset_agreement` falls through and
`best2 == conf1`*. **That was wrong on its own premise: `skeleton_drafts` shows one node requests
exactly ONE draft in 22 of 22 runs** (three nodes request 3 and get 2.54 back on average). I read
`base.max(devices.len())` and supplied a value for `base` I never checked. **The correct statement:
one node requests ONE draft, so no cross-draft agreement exists to measure, so no convergence round
occurs, so the block containing the change cannot execute.** Confirmed: across **24 one-node logs,
`skeleton_drafts` fires in 22 and `plan_convergence` fires in ZERO** — the block containing the change is never reached
at one node, so the guarantee is by non-execution rather than by value. Positive control: the same
predicate finds `pool_penalty > 0` in 19 of 45 three-node rounds (values to 34), so it can detect a
nonzero; it simply had nothing to look at.

**Not landed as of 2026-08-08 23:45, deliberately.** The measurement gates are all discharged; what
remains is that landing it needs a rebuild, and the sweep never leaves an idle window — so a rebuild
means killing a live cell. `rebuild-and-rotate.sh`'s own header records what that cost last time:
three rotations landed as 0.0563/0.0561/0.0561 with `void=False`, indistinguishable from genuinely
bad runs, and one **overwrote the campaign's best result, 0.9033**.

**Land it at the next natural cell boundary, not by interrupting one.**

## Provenance

F607 (ladder impossible at one node) · F610 (25.0 min/round, 6.73 SE) · F612 (4 of 5 rounds
pool-bought) · F649 (the deficit is a planning deficit) · F650 (the ladder is the whole planning tax)
· F651 (the hold discharged; and the dedup correction that shrank F649 from +15.9/4.63 SE to
+12.8/3.23 SE) · F652 (the dedup bug is bounded to ad-hoc scripts; every shipped instrument is clean)
· F653 (both halves replicated on 5 more cells; ladder cost 24.9) · F655 (a live pre-fix run laddering
on a round the fix would have prevented, conf 54 against a pool-invariant 88 and a verified 85 floor)
· F656 (the ladder reaches its floor in 2 of 6 runs but gains ~11 points at its best)
· F657 (my own follow-up hypothesis REFUTED by the source: best-plan retention and the stall guard
already exist).
