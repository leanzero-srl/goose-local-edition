# M5 — No Idle Models (design)

## Goal
A node should NEVER sleep mid-run. The judge already runs on an idle node, but when there's no worker that
needs judging, the node goes quiet. Fill every gap with USEFUL work that improves quality.

## Idle-node work, in priority order (the scheduler picks the first that applies)
The hook is the scheduler's idle-node step (where `pick_judge_target` already selects an idle device +
an in-flight worker to review). Generalize it to `pick_idle_work(idle_device) -> IdleJob`:

1. **JUDGE** an in-flight worker not judged in the last ~N s (EXISTS today). Highest value — catches a
   stuck/over-reading/drifting worker live.
2. **CORRECTNESS PRE-REVIEW of a just-completed task** (NEW, highest-leverage). When a task finished but
   integrate-verify hasn't consumed it yet, an idle node RUNS its tests + exercises its primary feature on
   a golden input and checks the OUTPUT is plausibly correct (not just "no crash"). It writes findings to
   `.swarm/prereview/<task>.json`. This directly attacks the #1 finding ("tests pass but the code is wrong"):
   spend otherwise-idle compute catching the broken-default-path / crash-on-primary / duplicate-impl bugs
   BEFORE integrate-verify, and inject those findings into integrate-verify's context so it fixes them.
3. **RESEARCH for a PENDING task** (NEW). For a not-yet-dispatched task whose deps are nearly ready, gather
   the API/usage facts its worker will need (context7 / web) into `.swarm/research/<task>.md`, injected when
   it dispatches — so the worker starts informed instead of guessing (guessing → divergent duplicate impls).
4. **CONFIDENCE ASSESSMENT** (ties to M6). Rate the current plan/run confidence; if low, flag what's shaky.

## Why this is the right shape
- It reuses the existing idle-node + LLM-on-idle machinery (low new surface).
- #2 turns wasted idle time into the exact quality gate the deep review proved is missing.
- Every job WRITES an artifact that a later stage consumes — no make-work.

## Implementation (incremental, flag confidence)
1. Scheduler: generalize the idle-node selection so when no judge target exists it returns a different
   IdleJob (pre-review of a completed-but-unconsumed task; else research of a soon-ready pending task).
   **Confidence: MEDIUM** (scheduler change, but additive + gated).
2. goose-cli: implement the pre-review job (LLM on idle device: run tests + golden-input check + write
   `.swarm/prereview/<task>.json`). **Confidence: MEDIUM-HIGH** (it's a constrained LLM task + file write).
3. integrate-verify: inject any `.swarm/prereview/*.json` findings into its prompt so it fixes them.
   **Confidence: HIGH** (prompt context injection, already done for deps).
4. Later: research job (#3), confidence job (#4 → M6).
Gate behind config `idle_work_enabled` (default on). Build/test/commit per increment.

## Observability
Emit an IdleJob event per assignment so we can later prove utilization ("0 idle-node-seconds wasted").
The user explicitly wants to SEE nodes never sleeping.
