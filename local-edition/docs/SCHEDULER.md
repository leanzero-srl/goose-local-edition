# Swarm scheduler — architecture & roadmap

Chosen via a 3-architecture design + adversarial judge workflow (2026-06-25). Satisfies the user's 5
requirements: CLI menu, per-device weights, pull-based work-stealing, locking DAG queue, shared context.

## Architecture
`goose-swarm`: an **in-process weighted DAG scheduler** in a **fork-only crate** (`crates/goose-swarm/`),
model-agnostic via a `TaskDispatcher` trait. The 27B planner recipe emits the typed DAG (proven); the
scheduler owns queue/locking/weights/work-stealing/shared-context; the real dispatcher INLINES Goose's
PUBLIC Agent drive sequence (no private `summon`/`run_subagent_task`), so there is ZERO core change except
a `Command::Swarm` arm in `goose-cli/src/cli.rs` (the crate auto-joins the workspace via the `crates/*` glob).
Drives one shared `lmstudio` provider; per-device weighted concurrency via our OWN semaphores (NOT bound by
`GOOSE_MAX_BACKGROUND_TASKS=5`). Rejected: subprocess-per-task (B) — `JsonOutput` has no `final_output`, would
scrape stdout. Kept B as a documented escape hatch.

## Data model
Pool config — additive `swarm:` key in `~/.config/goose/config.yaml` (round-trips with `goose configure`):
```
swarm:
  endpoint: http://localhost:1234
  planner_model: qwen/qwen3.6-27b
  devices:
    - { id: workhorse, model_id: qwopus3.6-35b-a3b-v1-mtp, weight: 3, enabled: true }
    - { id: mac,       model_id: qwen/qwen3.6-35b-a3b,      weight: 2, enabled: true }
    - { id: macbook,   model_id: qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx, weight: 2, enabled: true }
```
INVARIANT: `model_id` unique across enabled devices (LM Link routes by id alone) — asserted at load.
DAG (from planner JSON `subtasks[{id,description,difficulty,model,depends_on,files}]`): `Dag{tasks, dependents,
ready: BinaryHeap}`, `Node{spec, indegree_remaining, state, attempts, result}`, `TaskSpec{id, description,
difficulty, preferred_model, owned_files, deps}`. Ready rank = (criticality, -fan_out). Load guards: reject
cycles, unknown deps, and file-overlap among parallel-eligible tasks.
Weight = max in-flight tasks routed to that device concurrently (coarse capacity proxy; EWMA auto-tune is M4).

## Roadmap
_Status (2026-06-25): M1.0 ✅ (7/7 mock tests, clippy clean) · M1.1 ✅ · M1.2 ✅ fleet-validated (`goose swarm`: weighting {mac:3,macbook:1}, dep-gating, shared-context). Next: M1.3 pre-warm+retry, M2 pool menu, M3 worktree+reducer._

- **M1.0 (GATE, medium):** scaffold `goose-swarm` + MockDispatcher concurrency tests — no double-claim,
  dep-gating, weighting (~Nx), re-dispatch on transient, file-overlap hold. GREEN before any device.
- **M1.1 (medium):** `GooseAgentDispatcher` in goose-cli — inline the public drive sequence
  (`providers::init::create("lmstudio")` → `TaskConfig::new` → `Agent::with_config` → `update_provider`
  → `apply_recipe_components` → `override_system_prompt` → `reply` stream), extract `recipe__final_output`
  by public tool name; strict rule: no valid final_output when a schema was set ⇒ Failed + re-dispatch.
- **M1.2 (medium):** `goose swarm run "<prompt>"` — planner(27B)→DAG→scheduler over a hard-coded 2-device
  pool→context-slice pass-down→append-only `context.json`→per-task report. Wire `Command::Swarm` in cli.rs.
  Acceptance: the lru/greet/slug task across the fleet (no double-claim, dep-gating, ~2x weighting, kill-model re-dispatch).
- **M1.3 (low):** mandatory pre-warm (preload-swarm.sh / lms load) at run start; on "Model is unloaded",
  lms-load-then-retry before re-dispatch. Document + assert the model-id-uniqueness invariant.
- **M2.0 (low):** `goose swarm pool` cliclack menu (view/add/remove, set-weight, enable/disable, Probe via
  curl :1234/v1/models + lms ps). Custom dialogs/selects only, solid status colors, no native chrome, no left-rail.
- **M2.1 (low):** crash-resume — `state.json` checkpoint per transition; on restart, Claimed-with-dead-owner → Pending.
- **M3.0 (high, core):** git-worktree write-isolation per write-subtask (task #8) — see INTERNALS-NEXTPHASE.md.
- **M3.1 (high, core):** reducer/coordinator (task #9) + optional per-turn proactive compaction (agent.rs:2620, verified safe).
- **M4.0 (deferred):** EWMA-of-completion-time weight auto-tuning.

## Risks (confidence, not effort)
1. LOWEST confidence — the concurrency core (double-claim race, deadlock between file-overlap hold and
   dep-wait, lost ready-heap wakeup). Mitigation = the M1.0 mock test suite is a hard GATE.
2. LOWER — `recipe__final_output` extraction across imperfect exit paths (max-turns before final_output,
   partial JSON). Rule: invalid/missing ⇒ Failed + re-dispatch; needs dedicated tests.
3. CANNOT be fixed by the scheduler — LM Link "Model is unloaded" JIT race ⇒ mandatory pre-warm + bounded re-dispatch.
4. MEDIUM — weight=max-in-flight is coarse (one :1234 endpoint may serialize); EWMA auto-tune deferred to M4.
