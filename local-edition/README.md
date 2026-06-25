# goose-local-edition

A CLI coding agent for **local AI on Apple Silicon**, built as a thin additive fork of
[Goose](https://github.com/aaif-goose/goose) with **LM Studio** + **LM Link** as the inference fabric.

Three pillars:
1. **Always-plan + multi-device swarm** — a planner decomposes work into a typed DAG of subtasks, and a
   weighted work-queue scheduler dispatches them in parallel across your local machines via **LM Link**
   (one OpenAI endpoint that routes by model-id to whichever device holds the model).
2. **Local-model support** — first-class targeting of **Qwen3.6-27B** (dense planner) and
   **Qwen3.6-35B-A3B** (MoE worker), with safe handling of their hybrid (Gated-DeltaNet) tool-calling.
3. **Quality-first context** — keep only meaningful context; `GOOSE_LOCAL_CONTEXT_CAP` caps the effective
   window and per-turn proactive compaction keeps long sessions lean.

## The swarm (first-class command)
```bash
goose swarm run "Build lru.py, greet.py, slug.py as independent modules, then integrate and test them."
goose swarm pool                 # interactive menu: add/remove devices, set weight + instances, enable/disable, probe
goose swarm pool show
```
The planner (27B) emits the DAG; the scheduler runs it across the device pool with **per-device weights**,
**pull-based work-stealing**, a **locking queue** (a task's files are held while it runs; dependents unlock
when prerequisites finish), **shared-context** pass-down, and a final **integrate-verify** task that tests
the result. A live view (`▸ run … → device` / `✓ … (Ns)`) shows the parallelism. See `docs/USAGE.md`.

## Status (2026-06-25) — feature-complete + fleet-validated
- Scheduler crate `crates/goose-swarm` (model-agnostic, 7/7 concurrency unit tests, clippy-clean).
- `goose swarm run` + `goose swarm pool` wired into the CLI (only upstream-tracked edit: one `cli.rs` arm).
- **2-device and 3-device runs verified**: 5 tasks concurrent at peak across mac+macbook+workhorse, weighted,
  work-stealing, dep-gating; the swarm's own generated test suites **pass** (21/21, 8/8, all-pass).
- Per-device **instances** + idempotent pre-warm (no duplicate model instances); pre-warm + retry on
  "Model is unloaded"; per-turn compaction (cap bites mid-run). Full log: `docs/EXPERIMENTS.md`.

## Fleet (LM Link-linked; pool is configurable via `goose swarm pool`)
| Node | Chip / RAM | Default role | Model |
|---|---|---|---|
| MacBook `Mihai-Macbook-2` | M4 Max / 128GB | control + worker | `…holo3…` (35B) |
| `WorksMacStudio.lan` | M3 Ultra | planner + worker | `qwen/qwen3.6-27b` + `qwopus…35b-a3b` |
| `Mac.lan` 192.168.8.222 | M3 Max / 64GB | worker | `qwen/qwen3.6-35b-a3b` |

## Layout
- `crates/goose-swarm/` — the scheduler crate (DAG, weights, work-stealing, locking, shared context).
- `crates/goose-cli/src/commands/swarm.rs` — the `goose swarm` command + GooseAgentDispatcher.
- `recipes/` — the original recipe-based swarm PoC (`goose run --recipe recipes/swarm_v3.yaml`), now superseded by `goose swarm`.
- `scripts/preload-swarm.sh` — pre-warm the pool. `skills/lm-link-setup` — fleet setup skill.
- `config/` — example Goose config. `docs/` — USAGE, SCHEDULER, ARCHITECTURE, DECISIONS, CONTEXT, EXPERIMENTS, INTERNALS-NEXTPHASE.

> Additive fork: the scheduler lives in its own crate (auto-joined via `crates/*`); the whole feature
> touches one line of upstream-tracked code, so we keep pulling upstream patches.
