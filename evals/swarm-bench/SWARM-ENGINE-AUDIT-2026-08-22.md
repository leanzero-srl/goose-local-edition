# Goose Local Swarm Engine Audit — 2026-08-22

Status: research in progress. This document records evidence before any implementation. SB7, the active Qwen3.8 run, LM Studio state, configuration, and executable source remain untouched.

## Audit standard

Each conclusion must connect four things:

1. the exact engine path;
2. an event, activity digest, session trace, or completed benchmark artifact;
3. the user-visible time/quality consequence;
4. a falsifiable treatment that can be tested without changing SB7 or its scorer.

Official defaults are controls, not optimization evidence. Model-card advice, community reports, inference-runtime behavior, research literature, and this fleet's own traces are kept distinct until they agree.

## Slice 1 — active Qwen3.8 planning/detail tail

Evidence source:

- active log: `runs/sb7-fleet38/swarm-3node-r2/runs/sb7-fleet38/swarm-3node-r2/run.jsonl`
- activity digests: `runs/sb7-fleet38/swarm-3node-r2/.swarm/activity/`
- request telemetry: `runs/sb7-fleet38/swarm-3node-r2/.swarm/telemetry.jsonl`
- engine: `crates/goose-cli/src/commands/swarm.rs`, especially `detail_plan`, `run_agent`, and `fanout_over_fleet_straggler`

Observed without signalling or mutating the run:

- Research finished in about 11 minutes.
- Round-one skeleton drafting took 2,741 seconds and returned two drafts from three requested.
- Despite 97% structural convergence, the backbone round ran and took another 5,545 seconds.
- The detail fan then created 27 detail work items. Twenty-six had completed when inspected.
- `detail-api-server-api` was the remaining barrier. Its live digest exceeded 200,000 reasoning characters, had made four shell calls, and had not produced its final task specification.
- Gabee was idle. Mihai was generating the long detail call. Workhorse was generating short semantic omni-judge inspections roughly every five minutes.
- The reasoning tail was still advancing through endpoint design rather than obviously repeating. Token volume alone therefore does not establish a loop.

### Critical discovery: task detailers are implementing before the plan exists

`detail_plan` tells each agent to produce only brief specification prose and explicitly says not to write code. It nevertheless invokes the normal Goose agent loop with its standard tool surface and, in uncapped mode, up to 60 turns.

The live detail activity proves that multiple detailers ignored the role boundary and wrote into the real build tree before `plan_loaded`:

- `detail-app-entry` wrote `app/__init__.py`, `app/cli.py`, `app/__main__.py`, and both service entry packages, then ran the app repeatedly.
- `detail-events-outbox` wrote `app/ledgerd/events.py`, `app/ledgerd/outbox.py`, and a temporary test.
- `detail-vendor-client` wrote `app/vendor_client.py`.
- test detailers wrote `tests/test_drafts.py`, `tests/test_notifierd.py`, and `tests/test_webhooks.py`.
- documentation detailers wrote `README.md` and `DECISIONS.md`.

This is not useful planning fan-out. It is an uncommitted implementation wave with no scheduler ownership, no write-set isolation, no dependency admission, and no final task graph. Later workers may overwrite it, while other detailers read the prematurely created files and expand their own work. It directly explains both the excessive planning time and why task creation can drift into 30–50k-token generations.

### Current hypothesis to test

Task creation needs its own constrained execution mode rather than another generic Goose coding session:

- immutable requirement slice and research evidence as input;
- read-only artifact snapshot only when the task genuinely depends on an existing repository;
- no write/shell capability for ordinary detail generation;
- a typed, machine-validated task-spec result rather than unconstrained prose;
- progressive materialization, so a completed and validated task can enter contracts/execution without waiting for an unrelated detail straggler;
- semantic judge supervision that can redirect a detailer toward its missing spec fields without replacing it with a fixed token or wall-clock cap.

This remains a hypothesis until the detail prompts, final outputs, historical task scores, and judge traces are compared.

## Pending slices

- semantic judge interventions and nudge value;
- task graph creation, requirement coverage, dependency shape, and split behavior;
- inner Goose agent loop, tool surface, context/compaction, streaming, and cancellation;
- research and canonical planning workflow;
- build scheduling and integration;
- repair causal structure and promotion;
- LM Studio request/load/runtime behavior;
- Qwen3.8 sampling and reasoning-effort field evidence;
- applicable upstream `block/goose` changes;
- benchmark design and autonomous supervision.
