# swarm-gym — self-driving test harness for goose-local-edition

An AI **brain** invents coding tasks, drives them through `goose swarm` across multiple turns — a real
**vibing session** (build → react → amend → fix, like a real user) — then **verifies** what was
produced from the logs + artifacts and can **tweak** the swarm's knobs to make it better. It is also
the gate we run after pulling upstream `block/goose` changes.

## Setup
```bash
cd evals/swarm-gym
python3 -m venv .venv && . .venv/bin/activate && pip install -r requirements.txt
# Brain: export ANTHROPIC_API_KEY for the default Claude judge,
#        OR set brain.provider: local in config.yaml to use Qwen 27B via LM Studio (self-contained).
# MCP secrets (only if config.yaml swarm.mcp is non-empty): export CONTEXT7_API_KEY / WEBSEARCH_BEARER / ...
```

## Two run modes
The harness has two explicit, ledger-tagged modes (every session records `mode`):

- **`benchmark` (locked / reproducible)** — replays the **FROZEN** `SUITE` in `harness/bench.py`
  (crud / compute / txn ×N) through graduated tiers (smoke → light → medium → high → extreme),
  tagged `--variant` (e.g. mlx / gguf), graded deterministically by RUNNING the built app. The
  prompts + golden checks are byte-identical across runs on purpose — that is the reproducibility
  contract that lets variants pair (do NOT edit them; new coverage = a new suite). Emits
  `runs/BENCHMARK.md` + `benchmark-runs.csv` + `benchmark-summary.csv`.
- **`exploratory` (open-ended / monitored)** — the brain invents new prompts across archetypes and
  vibes follow-ups; an active monitor scans per-device dispatch each session and surfaces
  **starved / idle / underused nodes** (`monitor.py`), optionally auto-tuning scope-guarded pool
  knobs via the tweaker (`--tweak`). **No API key:** set `SWARMGYM_PROVIDER=operator` and Claude
  Code (the operator driving the session) IS the brain via a file handshake — the harness writes
  `runs/operator/req-*.json`, the operator answers with `runs/operator/resp-*.txt`.

```bash
# BENCHMARK (locked, reproducible, variant-tagged)
python -m harness bench --tier medium --variant mlx          # replay the frozen SUITE, 5×3, tagged mlx
python -m harness bench-report                                # head-to-head MLX vs GGUF (+ writes BENCHMARK.md)
python -m harness bench-csv                                   # export the CSVs for downstream authoring

# EXPLORATORY (open-ended, actively monitored; operator = brain, no key)
SWARMGYM_PROVIDER=operator python -m harness explore --n 6 --tweak   # generated sessions + idle-node monitor + auto-tune
python -m harness once --archetype heavy-spec --turns 3      # one heavily-specced new app + follow-ups
python -m harness once --archetype minimal-spec              # terse one-liner; judge scores gap-filling
python -m harness once --archetype continue-existing         # extend a kept, previously-green app
python -m harness loop --n 6 --tweak                         # cycle archetypes; A/B knob tweaks on cluster issues
python -m harness report                                     # ledger summary
```

## What a session does (one episode = one vibing session)
1. `open` — the brain (persona-seeded) invents the opener for the archetype + its requirements + deterministic checks.
2. `run` — `goose swarm run --output-format json` in `apps/<slug>/` (kept on disk).
3. `collect` — joins the JSON result + the `.swarm/run-<id>.jsonl` event log + per-task session traces (by logged `session_id`) + the built files.
4. `verify` — deterministic checks (build/run/tests/files/tool_called) + cluster (per-device distribution, starved devices, retries, MCP calls) + AI judge (requirement coverage, code quality, subtle bugs).
5. `next_move` — the brain reacts like a real user and sends the next turn (feature / fix / tests / refactor / mcp-feature) on the **evolving** codebase; repeat to the turn budget or until satisfied.
6. Optional `tweak` — on a systemic cluster finding, propose a scoped `KnobDelta` (swarm knobs ONLY), apply, re-run, A/B record.

## Conventions
- **Secrets** come from the environment / a local `.env`, never from any committed file.
- **Kept on disk, gitignored:** `apps/<slug>/` (the amendment substrate), `runs/<id>/` (artifacts + `report.html`), `ledger/` (the results ledger). Committed: the harness code, `config.yaml`, this README.
- **Knob-turning is local-AI only.** The `tweaker` scope-guard rejects any change outside the swarm knobs (pool weight/instances/enabled, planner_model, worker_max_turns, max_attempts, GOOSE_LOCAL_CONTEXT_CAP, GOOSE_MAX_BACKGROUND_TASKS).

See `../../local-edition/docs/USAGE.md` for the swarm itself and the `goose-knob-turning` skill for the diagnose→fix→re-test + upstream-ingest runbooks.
