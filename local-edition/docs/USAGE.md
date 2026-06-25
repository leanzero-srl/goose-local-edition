# `goose swarm` — usage

A local multi-device swarm over LM Studio LM Link. The smart model plans; a weighted work-queue
scheduler dispatches subtasks across your device pool; results are integrated + verified.

## Pool (the device menu)
```bash
goose swarm pool                 # interactive menu: add / set-weight / enable-disable / remove /
                                 #   set-planner / probe / save
goose swarm pool show            # print the pool
goose swarm pool add <id> <model_id> [weight]   # add a device (weight defaults to 1)
goose swarm pool weight <id> <n> # set a device's weight (max concurrent tasks on it)
goose swarm pool enable <id>     # / disable <id>
goose swarm pool rm <id>
goose swarm pool probe           # lms ps + the endpoint's model ids (discover the live fleet)
```
The pool is persisted under the `swarm` key in `~/.config/goose/config.yaml`:
```yaml
swarm:
  endpoint: http://localhost:1234        # LM Link OpenAI endpoint
  planner_model: qwen/qwen3.6-27b        # the smart planner (dense)
  devices:
    - { id: mac,     model_id: qwen/qwen3.6-35b-a3b, weight: 2, enabled: true }
    - { id: macbook, model_id: qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx, weight: 1, enabled: true }
```
**Weight** = max concurrent tasks routed to that device (heterogeneous capacity → bigger device,
higher weight, more work). **model_id must be unique** across enabled devices (LM Link routes by id).

## Run
```bash
goose swarm run "Build lru.py (LRUCache), greet.py, slug.py as independent modules, then integrate and test them."
```
What happens:
1. The planner (27B) emits a typed DAG of subtasks (id, deps, files, difficulty, target model), plus a
   final `integrate-verify` subtask that depends on all others and runs/tests the result.
2. The scheduler pre-warms the pool, then dispatches: independent subtasks run in parallel across
   devices (weighted, work-stealing — a free device pulls the next ready task); a task is **locked**
   (its files held) while in flight so no two tasks edit the same file at once; a dependent task only
   becomes ready when its prerequisites finish, and gets their outputs as **shared context**.
3. Transient failures (e.g. "Model is unloaded") are re-warmed (`lms load`) and re-dispatched to a
   different device. The report shows done/failed + dispatched-per-device.

## Notes
- Pre-warm the pool first for best reliability: `local-edition/scripts/preload-swarm.sh` (the run also
  pre-warms automatically). Remote JIT-load during generation can be flaky.
- Architecture + roadmap: `docs/SCHEDULER.md`. Scheduler crate: `crates/goose-swarm` (model-agnostic,
  unit-tested). Dispatcher + CLI: `crates/goose-cli/src/commands/swarm.rs`.
