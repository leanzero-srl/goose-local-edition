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

## F2 — SUPERSEDED 2026-08-01 10:15 — node count IS a variable now

Mihai gave each host's loaded model a distinct identifier (`mihai-` / `workhorse-` / `gabee-`).
Everything below about the collapse was true of the fleet as it stood and is left intact, because
it explains why every earlier node-scaling number in this project is void. What is no longer true
is the conclusion that it could never be measured.

Re-proven after the change, by the same probe that condemned the old fleet: three concurrent calls,
one per identifier, put **all three instances into `generating` at once** (7.7 / 10.0 / 13.4 s —
one call's latency each, not serialized). And the engine's own `run_started.pool` for
`baseline-n3-r0` now lists **three devices**, one per host:

```
mac-gabee-...              model=gabee-...      weight=1
local-mihai-...            model=mihai-...      weight=1
worksmacstudio-workhorse-  model=workhorse-...  weight=1
```

The loop now varies node count as its primary dimension (3, 1, 2 on baseline, then the
dispatch-quality arms at 3), and voids any unit whose actual pool differs from the one it asked for.

**One comparability warning:** the three hosts are not identical. `gabee` runs **Q6_K at ctx
193792** while `mihai` and `workhorse` run **Q8_0 at ctx 262144**. So a 3-node run includes a
lower-quant node that a 1-node run does not, and `pick_device` will route to it. Any node-count
delta is a delta of "this fleet", not of "N identical workers" — worth stating before a number
gets read as more general than it is.

### The original finding, kept because it voids the old numbers

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

## F5 — I almost published "every finding was refuted" off a broken harness

Adversarial round 1 returned `survivors: []` with the note *"every finding was refuted — report
that plainly, do not soften it"*. That was **my bug, not a result.** The workflow's inner
`parallel([...])` was passed promises instead of thunks, so all 34 verification calls failed and
`votes` was empty for every finding — and my survival test was `votes.length > 0 && kills === 0`,
which turns "nothing was checked" into "it did not survive".

Four finder agents had in fact raised **38 findings**, none of which was ever tested. Recovering
them needed a second correction: my journal parser read `label`, but the field is `key`, so the
first recovery attempt also printed zero. Two blind instruments in a row, both reporting a
confident zero.

This is the project's standing law firing twice in five minutes: **a zero is usually a broken
instrument, not an empty world** — and before any zero licenses a conclusion, prove the query can
see the thing at all. The `24 agents completed` count is what refused the first zero, and the raw
journal shape is what refused the second.

Round 1 now dedupes by site before verifying, because three lenses independently raised the 75s
detail budget and three raised the missing event. Independent rediscovery is recorded as
corroboration rather than spent as three separate verifications.

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
