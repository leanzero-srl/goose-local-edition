# Settings > Forge: what to bake, what to keep, what to delete

The panel exposes **76 controls**. The goal is to bake what works into the default behaviour, keep only the
levers that genuinely go both ways (each with an honest PRO/CON), and delete the rest.

This document is the disposition and the evidence behind it. It is deliberately conservative about what
counts as "proven", because the measurement history here is worse than it looks.

## Read this first: how little is actually measured

A 2026-07-22 audit of the run corpus and the campaign ledger found:

- **The lever campaign never ran.** 28 arms were queued; `LEDGER.tsv` contains none of them.
- **Zero single-lever arms were ever completed.** 42 instrumented runs contain no clean contrast.
- **Eleven levers move as one block** — they are only ever true together in the `allon*` arms, so nothing
  in that block is individually attributable.
- **The harness silently refused 48 real levers** (fixed 2026-07-22). Arms that asked for them ran as
  controls, so a missing LEDGER row means "could not be armed", *not* "no effect".
- **43 config fields never appeared in `levers_resolved`** (fixed 2026-07-22), so no run could prove what
  it used.

So: do not read "no evidence" as "no effect". Most of this panel is unmeasured, not disproven.

---

## 1. BAKE — remove from the UI, make it how goose works

### Proven by measurement
| Lever | Evidence |
|---|---|
| `grounded_research_only` | Strongest evidence in the campaign — mechanism confirmed on 2 runs, 2 binaries, plus a code read |
| `no_tools_means_ask` | Best-evidenced lever; the only one with a genuine before/after on the same spec |
| `stream_decode_retry` | A real mid-stream body drop caught and recovered; observed again in arms 2/4/5 |
| `straggler_stop`, `straggler_stop_degrade` | 4+ fires, both directions proven, zero panics |
| `backbone_skip_confident` | 29-run survey plus a same-spec before/after |

### Structurally correct — "off" is indefensible, not merely worse
| Lever | Why it cannot sensibly be off |
|---|---|
| `require_tests` | An empty suite is not a passing suite. Off, "nothing was checked" and "everything passed" are the same value, and deleting a failing test becomes a way to go green. Corpus replay: 3 trees were green with zero tests, 0 spurious reds. |
| `contract_validate` | It now DROPS a contract stub that does not parse. Freezing prose as an "interface" is how a worker ends up writing against nothing (measured: h1-treat-2 shipped with every command broken). |
| `smoke`, `contracts`, `complete`, `split` | Force-set on every desktop run for months. They were never real choices, and their config keys were inert. Now defaults; parity with headless depends on them. |

### Already baked this session
`omni_judge` (the only supervisor that can watch a `verify::` task), plus the four parity gates above.

---

## 2. KEEP as levers — genuine two-sided trade-offs

Each needs its PRO and CON stated in the hint, so a user knows what they are buying and paying.

| Lever | PRO — what it buys | CON — what it costs |
|---|---|---|
| `ask_floor` / `ask_max_q` | goose asks instead of inventing your product. Measured: a run found 5 open decisions on the spec's own "do NOT guess" list. | Interrupts the build and waits on you. Set to 0 for unattended runs. |
| `fan_verify` | Per-module checks fan across the fleet instead of one serial sink. | Adds N tasks. The sink is still 47% of node-busy time, so the wall-clock win is UNPROVEN. |
| `parallel_tests` | Tests start the moment their module lands, overlapping the build. | More subtasks on a slow fleet; unmeasured. |
| `relax_contracted_deps` | Independent modules build simultaneously instead of chaining. | Relies on contracts being sound. If a contract is wrong, the modules diverge in parallel. |
| `best_of_n_skeletons` | More drafts, better plan structure. | Each draft is a full planning call — directly buys quality with minutes. |
| `sink_max_turns` | The final check can actually finish. Measured: 5 of 9 sinks never reached a verdict; 3 died exactly on the worker cap. | Each step costs ~1 min on a local model. The main quality/speed dial. |
| `worker_timeout_secs`, `planner_timeout_secs`, `progress_watchdog_secs`, `context_cap` | Stop a wedged or spiralling step. | Too tight cuts healthy work; a killed planner call has no retry path. |
| `temperature`, `top_p`, `top_k`, `min_p`, `repeat_penalty`, `draft_temp` | Model tuning for your specific local weights. | Wrong values degrade every call. Blank = model default. |
| `research_planning`, `research_scouts`, `scout_*`, `max_research_questions` | Grounds the plan in looked-up fact. | Costs a phase; with no lookup tools it is the model's own knowledge, not research. |

---

## 3. DELETE

| Lever | Why |
|---|---|
| `unwired_demotes_verified` | **MEASURED AND HURT.** The only lever with an explicit DO-NOT-SHIP verdict, backed by a reproduced false positive on a good app. |
| `review_repro`, `repro_demotes_verified` | Already deleted (`cd739001c`). Their implementation lived inside the dead `review_fanout` gate, so the toggles reported on something that could not happen. |

Dead engine features with no UI toggle were removed separately: `browser_verify`, `review_fanout`,
`review_verify`, `review_fix`, `review_fix_parallel` — ~1,636 lines that could never execute.
`sink_review` is verified dead and still pending removal (needs an AST-aware edit; see plan B3a).

---

## 4. NOT LEVERS — move out of the tuning panel

These are fleet identity and machine configuration, not behaviour choices, and they crowd out the controls
that are: `planner_model`, `planner_also_works`, `planner_weight`, `homogeneous_models`,
`allow_model_load`, per-node weights, `ai_session_name`, `no_log`, run-panel detail.

---

## Recommended shape

- **Tier 1 (default view, ~8 controls):** the live fleet, planner model, "ask when uncertain" + its two
  questions, research mode, run-panel detail.
- **Tier 2 (Advanced, collapsed):** the two-sided levers in §2, each with its PRO/CON.
- **Tier 3 (Experimental, collapsed + labelled unmeasured):** `fan_verify`, `parallel_tests`,
  `relax_contracted_deps`, `dep_signatures`, `scoped_contracts`, `split_fat` — mechanism verified, effect
  not.
- **Gone:** §1 (baked) and §3 (deleted).

That is 76 → ~8 by default, with everything still reachable for someone tuning.

## Before baking anything else

Two things must be true or a "bake" is just a guess with extra steps:

1. The lever appears in `levers_resolved` (so a run can prove it was used) — 35 were added 2026-07-22.
2. The harness can arm it (so a campaign can measure it) — 48 were added 2026-07-22.

Both now hold, which means **the first genuinely attributable single-lever arms are only now possible.**
Everything in §1 marked "proven" rests on evidence gathered before that, and should be re-confirmed as the
campaign produces clean arms.
