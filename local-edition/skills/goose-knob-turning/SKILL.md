---
name: goose-knob-turning
description: Diagnose, tune, and improve goose-local-edition's local-AI swarm (LM Studio / LM Link), and safely ingest upstream block/goose changes. Use when a `goose swarm` run is slow, a device is starved, "Model is unloaded" appears, MCP/tool calls misbehave, context blows up mid-run, the user reports a swarm bug, or the fork needs to pull upstream. Tune ONLY the local-AI / LM-Studio surface — never upstream core.
---

# goose-local-edition: knob-turning & upstream-ingest

The fork lives at `~/Projects/goose` (branch `local-edition`, remote `origin` = leanzero-srl/goose-local-edition, `upstream` = block/goose). The swarm is `goose swarm run`/`goose swarm pool`; the scheduler is the `crates/goose-swarm` crate; the dispatcher + CLI is `crates/goose-cli/src/commands/swarm.rs`. The self-driving test harness is `evals/swarm-gym/`.

## HARD RULE — scope
Tune ONLY the local-AI / LM-Studio surface: the `goose-swarm` crate, `swarm.rs`, the swarm config, and LM Studio placement. NEVER change upstream goose core to "fix" a swarm issue. A change touching `crates/goose/**` (outside the per-turn-compaction hook), `crates/goose-providers/**` (outside the context cap), or any non-swarm file is OUT OF SCOPE — surface it instead.

## The knobs (and only these)
Pool config lives under the `swarm:` key in `~/.config/goose/config.yaml` (round-trips via `goose swarm pool`):
- **pool device weight** — max concurrent tasks on a device (`goose swarm pool weight <id> <n>`). Raise to send more work to a device; the planner's preferred-model routing sends a task to the device whose `model_id` matches, so a device only gets overflow work unless tasks prefer its model.
- **pool device instances** — copies of a model to load (default 1; `pool add <id> <model> <weight> <instances>`). Pre-warm is idempotent — it never stacks duplicates.
- **pool enable/disable** — `goose swarm pool enable|disable <id>`.
- **planner_model** — the smart planner (default `qwen/qwen3.6-27b`). Set via the `pool` menu.
- **worker_max_turns** (SwarmConfig, default 40) — raise if workers hit the cap before finishing.
- **max_attempts** (SwarmConfig, default 3) — raise for flaky LM Link.
- **GOOSE_LOCAL_CONTEXT_CAP** (env) — effective context window cap; per-turn proactive compaction triggers off it mid-run.
- **GOOSE_MAX_BACKGROUND_TASKS** (env) — goose's background-task cap (the swarm uses its own per-device semaphores, so this rarely matters).
- **MCP worker extensions** — `goose swarm run --mcp <name>` or `SwarmConfig.worker_extensions` (`context7` | `web-search` | `doc-processor`); secrets come from env (`CONTEXT7_API_KEY`, `WEBSEARCH_BEARER`/`SERPER_KEY`/`GITHUB_TOKEN`, `DOCPROC_BEARER`), never config.

## Where every log lives + how to read it
- **Structured per-run event log (JSONL):** `<run-cwd>/.swarm/run-<id>.jsonl` — events: run_started (pool/planner/endpoint), plan_loaded (full DAG + raw planner JSON), task_dispatched, task_completed (device/model/attempts/elapsed/session_id/tool_calls incl. is_mcp/ok), task_retry, run_finished (enriched RunReport). This is the first thing to read.
- **Machine report:** `goose swarm run … --output-format json` prints the enriched RunReport (done/failed/results/dispatched_per_device/`tasks[]`/`per_device`) to stdout (progress goes to stderr).
- **Full per-task trace:** each task's `session_id` (in the log/report) resolves in the goose session store `~/.local/share/goose/sessions/sessions.db` → `messages` table (every tool request/response). Workers are Hidden sessions; look up by id.
- **Goose CLI logs:** `~/.local/state/goose/logs/cli/<date>/*.log` (JSONL tracing); LLM req/resp+tokens in `~/.local/state/goose/logs/llm_request.*.jsonl`.
- **Fleet state:** `lms ps` (loaded models + GENERATING), `lms link status`, `curl -s http://localhost:1234/v1/models`.

## Diagnose → fix → re-test (symptom → knob)
- **"Model is unloaded" / connection retries** (task_retry events, per_device.retries > 0) → pre-warm (the run does this; or `lms load <model> -y --ttl 3600`) + raise `max_attempts`.
- **A device got 0 tasks** (per_device.dispatched missing a device; cluster `starved`) → confirm its model is loaded (`lms ps`), raise its weight, or give more independent subtasks; remember preferred-model routing concentrates work on the device whose model the planner names.
- **Context blows up mid-run** → set `GOOSE_LOCAL_CONTEXT_CAP` (e.g. 16000–24000); per-turn compaction will trigger. Too low ⇒ a compaction loop (cap must exceed system+tools baseline ~5K).
- **Worker hits max-turns before finishing** (task output truncated / no final_output) → raise `worker_max_turns`.
- **A hard subtask ran on a weak model** → the planner labels hard tasks `qwen/qwen3.6-27b`, but 27B is the planner, not a worker, so it falls back to a 35B. Add 27B to the worker pool (`pool add wh-27b qwen/qwen3.6-27b 1`) for true hard-task routing.
- **No MCP calls when expected** (is_mcp tool calls absent) → confirm `--mcp <name>` + the secret env var is set; a failed extension connection is logged non-fatally.

## Running the harness (the verifier + the upstream gate)
```bash
cd ~/Projects/goose/evals/swarm-gym && . .venv/bin/activate
python -m harness once --archetype heavy-spec --turns 3       # one vibing session (build → vibe follow-ups)
python -m harness once --archetype minimal-spec               # tests gap-filling
python -m harness once --archetype continue-existing          # extends a kept, previously-green app
python -m harness loop --n 6 --tweak                          # campaign; A/B knob tweaks on cluster issues
python -m harness report                                      # ledger summary / trend
# Brain: export ANTHROPIC_API_KEY for the Claude judge, or SWARMGYM_PROVIDER=local for the Qwen 27B judge.
```
Each session keeps its app in `apps/<slug>/` and artifacts in `runs/<id>/report.html`. Read the report + the per-turn `runs/<id>/turn-*.json` to see what the swarm did and the verdict's findings.

## Upstream-ingest runbook (pull block/goose without breaking our pillars)
```bash
cd ~/Projects/goose && git fetch upstream && git merge upstream/main   # (or a release tag)
```
Conflicts can only appear in the **rebase-risk files** (everything else is a separate crate / our own files):
`crates/goose-cli/src/cli.rs` (the Command::Swarm arm), `crates/goose-cli/src/commands/mod.rs`, `crates/goose-cli/Cargo.toml`, `crates/goose/src/agents/agent.rs` (per-turn compaction ~the `conversation.extend` hook), `crates/goose-providers/src/openai.rs` (GOOSE_LOCAL_CONTEXT_CAP), `crates/goose/src/context_mgmt/mod.rs` (the cap in `check_if_compaction_needed`). Resolve additively (keep both upstream's change and our addition). Then GATE:
```bash
source bin/activate-hermit
cargo build -p goose-cli && cargo clippy -p goose-cli --all-targets -- -D warnings && cargo test -p goose-swarm
# then a smoke swarm run, and the harness as the behavioral gate:
cd evals/swarm-gym && . .venv/bin/activate && python -m harness loop --n 3   # diff trend vs last green baseline
```
Only push `origin local-edition` once the build + clippy + goose-swarm tests + a smoke run + the harness all pass.

## The dumb-human bug path
When the user reports "X is broken" in plain terms: (1) find the latest `<cwd>/.swarm/run-*.jsonl` (or ask which run); (2) read run_finished + the failing task_completed/task_retry; (3) open that task's `session_id` trace in the session DB to see the exact tool calls + errors; (4) inspect the built files in the workspace; (5) map to a knob (table above) or, if it's a real code defect in our surface, fix `goose-swarm`/`swarm.rs` (never hand-edit a generated app to mask a swarm defect); (6) re-run the same task and confirm.

## Changelog (append learnings)
- 2026-06-25: Created. Pillars: scheduler crate + swarm.rs + structured JSONL/observability + MCP worker extensions + per-turn compaction. Known: preferred-model routing concentrates easy tasks on the 35B device whose model the planner names; add 27B as a worker for hard-task routing. Qwen3.6 is a reasoning model — give the harness brain ≥16K max_tokens (thinking lands in reasoning_content).
