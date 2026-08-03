# RESUME — parked 2026-08-03 ~18:40 local, at Mihai's request

Everything below is on disk. Nothing needs to be re-derived.

## How to restart (in this order)

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
rm STOP                      # the sweep refuses to start while this exists
./loop.sh boundary           # parks the run tree, REBUILDS, verifies markers — see "owed" below
./loop.sh status
```

`loop.sh boundary` is the correct entry point, **not** `start`: the running binary predates seven
committed engine changes, and the boundary is the only thing that rebuilds. It also resets
`goalstate`'s binary-scoped sample — expected, not a bug.

## What is stopped

- **Sweep supervisor** — killed, and `STOP` written so it cannot take a new unit on its own.
- **Engine** (`goose swarm run`) — killed. `.swarm/pause` was written first; the scheduler honours it
  but the COMPLETE phase does not, so the kill was required.
- **promptbench** arms — killed.
- **LM Studio — NOT TOUCHED.** No model loaded, unloaded or re-aliased. `lms ps` shows 0 GENERATING;
  the fleet is simply receiving no work.

## The one number that matters, and its exact status

`goalstate` reads **test-author 5 completed / 0 failed, p = 0.157 — NOT significant.**

`swarm-3node-r1` holds **five more test-authors, all `status=done`** (`test-store`, `test-api`,
`test-meridian`, `test-api-edge`, `test-meridian-edge`). Ten clean completions against the historical
31% rate would be **p = 0.0245** and would move the row.

**They do not count and must not be counted.** `goalstate` requires `run_finished`, and r1 was killed
in its COMPLETE phase before emitting it. That gate is correct — an unfinished run can still dispatch
another test-author — and I am not loosening it to rescue my own result. The run reached 19
dispatched / 18 done / 0 FAILED with the sink complete; the data is on disk under
`runs/nodeloop/swarm-3node-r1/` if a later decision is made about it, but as it stands **the
registered test is UNRESOLVED, not passed.**

⚠ The confound stands regardless: the 31% baseline (13/42) is from an **older build era**, so this is
a before/after across builds, **not a randomised A/B**. The clean n=3 `baseline` cells are still owed.

## Seven engine changes committed, NONE yet verified on the wire

| # | commit | what |
|---|---|---|
| 1 | `e6064a428` | `kind_prompt` default ON — a test-author was told *"read AT MOST the ONE file you will edit"*, i.e. not to read the source module whose signatures it must assert. `#[serde(default)]` → `default_true`. |
| 2 | `e6064a428` | `dep_signatures` default ON — dependency bodies were injected truncated mid-token (3 of 4 blocks cut, one failing `ast.parse`). |
| 3 | `6efc6956c` | samplers matched to the model's own GGUF: `top_k 20`, `top_p 0.95`, `min_p 0.0`. Backup `~/.config/goose/config.yaml.bak-samplers`. |
| 4 | `7da0b6f84` | `force_write_tool` — **default OFF and pinned OFF by test**; measurement rejected it. |
| 5 | `f0a230d93` | every build stamps its own commit sha; `build_sha` read the literal `"dev"` on every run this campaign ever produced. |
| 6 | `d9394ebda` | **act-now nudge, default ON** — one line, last position, only when the worker owns files and none exist. |
| 7 | `5714f98e5` | repair routes to the **fastest enabled** node; it was pinned to `devices.first()` and never called `pick_device`, so every repair went to gabee (weight 1) while the workhorse (3) idled. |

**FIRST JOB AFTER THE BOUNDARY — verify on the wire, do not assume (the F213 trap):**
- `levers_resolved` shows `kind_prompt: true`, `dep_signatures: true`, `act_now_nudge: true`
- `levers_resolved.build_sha` is a **real sha**, not `"dev"`
- an `llm_request` payload carries `top_k` / `top_p` / `min_p`
- a worker dispatch contains **"Your next message must be a TOOL CALL"**
- a repair dispatch names the **workhorse**

Any one absent ⇒ that change shipped nothing, and it gets said plainly.

## Bench state (`promptbench.py`, replays real archived decision points, ~2 min/sample)

    baseline    n=42  refused 23.8%  wrote-first 23.8%   5 of 9 cases refused
    declared    n=39  refused 10.3%  wrote-first 33.3%   2 of 6 cases
    nudge       n=12  refused  0.0%  wrote-first 66.7%   0 of 4 cases
    toolchoice  n=18  refused  5.6%  wrote-first 33.3%   1 of 6 cases
    forcewrite  n=27  ALL HTTP 400 — the named tool_choice form is rejected by the server

**Hard limit to remember: the 9 test-author cases are only 3 TASKS from ONE SPEC.** No claim about
test-authors in general is available from this bench. Paired variant comparison is still valid.
Widening requires runs of a **different spec** — reps cannot do it.

## Next after verification

The interleaved node curve — `baseline-n3-r0, baseline-n1-r0, baseline-n3-r1, baseline-n1-r1, …` —
is already at the front of `backlog()`. That is **goal one**, and it yields a matched pair after every
two units instead of after six.
