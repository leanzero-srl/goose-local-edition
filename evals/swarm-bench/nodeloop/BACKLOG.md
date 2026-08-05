# Backlog

Ideas accepted but not started. Each one records the implementation leads found when it was filed,
so picking it up does not begin with a search.

---

## B1 — a plan-approval gate, and an honest answer to "how many nodes will this plan actually use"

**Filed 2026-08-01 by Mihai.** Two halves, and they belong together because the second is what makes
the first worth stopping for.

**Half one — present the plan and let the user accept it,** the way Claude Code does, somewhere
between research and contracts. Today the run decomposes and dispatches without ever showing anyone
the decomposition; the operator sees the plan only after the fleet has committed to it.

**Half two — the swarm says how many nodes the plan can actually use.** Mihai's framing: it should
recommend whether the plan genuinely uses all 3 nodes, or whether only 1 or 2 are enough and the rest
would be noise. This is the honest version of goal one — not "more nodes are better" but "here is what
this particular plan can absorb."

### Why this is close to buildable

**The approval channel already exists.** The ask-floor path writes `.swarm/clarify-questions.json`,
emits `low_confidence_ask`, and BLOCK-polls `.swarm/clarify-answers.json` up to a timeout
(`swarm.rs:19588-19606`). A harness or the desktop panel answers as the human. A plan-approval gate
is the same handshake with the plan as the payload — it does not need a new transport, a new UI
primitive, or a new blocking mechanism.

**The node number already exists, but only after the fact.** `occupancy.py` computes
`max_useful_nodes = total_work / critical_path` from observed task durations
(`occupancy.py:216`) — it reported **3.64 against a pool of 3** on the last baseline, meaning that
plan could have used more nodes than the fleet had. At plan time the durations are not known, but the
DAG is, so the structural bound is available: critical-path LENGTH in tasks versus total task count,
and the maximum antichain width (how many tasks are ever simultaneously ready). Difficulty labels
give a crude weight if a better estimate is wanted.

The honest presentation is a RANGE with its basis stated, not a single confident number — the
post-hoc figure and the structural bound must be reported in the same units so they can be compared
after the run, which is also how the estimator gets validated instead of trusted.

### What it must not become

- It must not block a headless run. The clarify path already handles this (a timeout with a
  documented default), and the same rule applies: an unattended sweep must never wedge waiting for a
  human who left.
- The recommendation is advisory. It must never silently shrink the pool — that would make every
  node-count measurement a measurement of the estimator instead of the swarm.
- The estimate must be emitted as a deterministic event so it can be scored against what the run
  actually achieved. An estimator nobody grades will drift.

### Prior art in this repo to reuse

- `swarm.rs:19588` — the clarify question/answer handshake and its block-poll.
- `low_confidence_ask` — the event shape for "the run wants a human".
- `plan_loaded` — already carries the full DAG (ids, deps, owned_files), which is the estimator's
  entire input.
- `occupancy.py:216` — the after-the-fact figure the estimate will be graded against.

---

## ARM: `sink_lean_prefill=1` — the two-sided lever aimed at the highest-leverage cell

**Why this arm and not another.** F385 showed the pre-registered design needs 51 matched pairs at the
observed gap, and F386 showed that removing the single sink-stall cell moves that to 11. The sink
running to its cap is therefore the one defect worth attacking. `sink_lean_prefill` (`swarm.rs:2456`,
default OFF, never measured) drops the frozen-contract bundle from the sink's prompt specifically so
the slowest task finishes before the cap.

**Why it is NOT a default flip.** It deletes the agreed-contract reference from the only task that
reconciles cross-module interfaces. `frozen_interfaces_block` calls itself the reference for "the #1
cause of passing-unit-tests but a broken end-to-end integration", and a SHAPE check is exactly what
the capped run lost (`sync_shape` 1.00 → 0.00). Running the app proves a mismatch EXISTS; the bundle
is what says which side is wrong. The two sides pull on the same metric in opposite directions.

**The falsifier is two-sided and must be read as a pair — neither half alone decides it:**

| side | signal | verdict |
|---|---|---|
| speed | sink wall-clock vs `cap_secs` from `sink_capped` (F386 now reports the EFFECTIVE ceiling) | finishing under the cap where the OFF arm capped ⇒ the PRO is real |
| correctness | `sync_shape` and the cross-module checks in `verdict.json` | any tier-A integration check dropping vs the OFF arm ⇒ the CON is real, REVERT |

**It wins only if it takes the first without the second.** Faster-and-broken is the outcome this lever
is most likely to produce, and it is the one a wall-clock-only reading would score as a win.

⚠️ **Cost is NOT settleable at n=1** (F382: e2e varies 2277.5/425.7/1391.6 across identical cells).
The SPEED half needs ≥3 replicates per arm compared as medians. The CORRECTNESS half is a per-run
deterministic check and is readable sooner — so read correctness first, and stop if it drops.
