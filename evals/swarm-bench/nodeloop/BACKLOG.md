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
