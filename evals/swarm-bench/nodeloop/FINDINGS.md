# nodeloop — findings ledger

Chat is compacted away; this file is not. Every conclusion here is derived from an engine event
log, a source line, or a probe against the live fleet — never from a model's self-report.

---

## F1 — The 46-point "noise" is the detailer failing open (2026-08-01)

Three runs, identical config, identical 1-device pool, same spec:

| run | tasks whose spec stayed the architect's one-liner | build | tier C (vendor) |
|---|---|---|---|
| `swarm-1node-r0` | **2** of 14 — `meridian`, `test-api` | **44.2%** | 14.3% |
| `swarm-2node-r0` | **1** of 16 — `api` | **86.7%** | 100.0% |
| `swarm-3node-r0` | **0** of 14 | **90.0%** | 85.7% |

The architect emits a description of "ONE short line" (`swarm.rs:11494`). The detailer expands it
into an implementation-ready spec, one call per device, under a hard 75 s cap
(`swarm.rs:12353`); on timeout/empty/error the engine silently keeps the one-liner (the
`_ => brief` fallback) and **emits no event**.

Per-module trace, which is stronger than the correlation: in the 44.2% run `meridian` — the
vendor client tier C exists to grade — was dispatched with **95 characters**. In the 90% run the
same module received **2,464 characters** with the full class signature block, exact endpoints and
required headers. Tier C moved 14.3% → 85.7%.

Commit `dfd67e84d`.

## F2 — Node count is not a variable on this fleet, and cannot be made one from the engine

Three hosts (Local, WorksMacStudio.lan, Mac.lan) all serve one identifier. Two probes against the
fleet exactly as it is:

- the host-qualified instance name `<deviceId>:<path>` that `lms ps --json` reports as
  `indexedModelIdentifier` is **rejected** by the endpoint: `HTTP 400 Invalid model identifier`
- three concurrent calls on the shared identifier were **all served by one host**; the other two
  never left idle for the duration

`reconcile_pool_with_fleet` (`swarm.rs:2043-2049`) builds one worker per DISTINCT identifier, so it
correctly reports a 1-device pool — the collapse is LM Studio's, not goose's. Every swarm run on
disk confirms it: `run_started.pool` has a single entry and every dispatch went to it, so
`swarm-1node`, `swarm-2node` and `swarm-3node` were three replicates of the same configuration.

**Consequence:** a sweep over `GOOSE_SWARM_MAX_NODES` measures nothing, and the earlier
"more nodes make it worse" table compared a configuration with itself. The loop measures dispatch
quality instead.

**Not yet fixed, and worth fixing:** the engine collapses three resident instances into one device
*silently*. It should say so — two thirds of a fleet going unused is not a detail.

## F3 — Most workers are given another job's rules

From `nodeloop/dispatch_audit.py` over the same three runs:

- **72–80% of dispatches** receive the implementer prompt while not being implementers
  (`kind_prompt` defaults OFF, `swarm.rs:18032`)
- only **20–24%** are actually implementers; **39–47%** own nothing (the `verify::<M>` and
  `verify-e2e::N` fan)
- **3–5 dispatches per run** own a `test_*.py` and are told *"NEVER read the project's TEST files"*
  (`swarm.rs:18100`) — the file they must produce is the file they may not open
- **5 dispatches per run** get `context_slice_len == 0`: no dependency context at all

Earlier work estimated this at 59.9%; on this spec it is higher, because `fan_verify`/`fan_e2e`
generate many owns-nothing tasks.

## F4 — Two grader flaws, caught by controls before anything was believed

`dispatch_audit.py` first docked every owns-nothing verifier for not naming files it is forbidden
to write, then graded verifiers on "names symbols" when what pins a verifier down is the command
it must run. Both were fabricated deductions of exactly the kind this project's standing law
predicts ("a grader's bugs invent defects rather than excuse them"). The self-test now grades per
task kind and asserts both score directions, the vacuous-truth case, and an empty run.

---

## Open, in flight

- `nodeloop/loop.sh` is running arms `baseline → kind_prompt → scoped_contracts → doc_prefetch`
  at n≥3, raising n rather than stopping when the backlog drains.
- Adversarial round 1 (detail fan-out) is auditing the 75 s cap, the silence, the fallback itself,
  and whether the same shape exists elsewhere (judge-side splits produce a **43-character** task
  statement, `scheduler.rs:57`).

## Standing rules for this loop

- The LM Studio fleet is READ-ONLY. If the engine cannot use what is there, fix the engine.
- Only a deterministic engine event may confer or retract a verdict.
- An arm that moves the build score by less than the replicate spread has said nothing.
- Run the built app before believing any score.
