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

`replan_has_enough_dag_left` and the trigger's `idle_capacity() >= 2` term make a ladder
**arithmetically impossible at one node** (F607, proven from source). Confirmed in the data six
separate times: **every one-node run in the corpus reports `redraft_rounds` zero.**

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
+12.8/3.23 SE) · F652 (the dedup bug is bounded to ad-hoc scripts; every shipped instrument is clean).
