# Experiments log — durable, reproducible record

> Purpose: capture every experiment + result so nothing is lost to context compaction.
> Append new entries at the bottom with date + command + result.

## Environment (verified 2026-06-25, MacBook `Mihai-Macbook-2`)
- `lms` at `~/.lmstudio/bin/lms`; LM Studio.app installed; OpenAI server runs on `:1234`.
- Goose CLI 1.38.0 installed at `~/.local/bin/goose` via `download_cli.sh`. From-source build (v1.39) at `~/Projects/goose/target/release/goose`.
- Toolchain: cargo 1.95 + rustup (repo pins Rust **1.92** via `rust-toolchain.toml`); node v22.22.0; git. Missing: `gh`.
- **LM Link LIVE across 3 nodes** (`lms link status`): this device + `WorksMacStudio.lan` (connected) + `Mac.lan` (connected).
- Models placed (`lms ls`): `qwen/qwen3.6-27b` → WorksMacStudio.lan; `qwen/qwen3.6-35b-a3b` → Mac.lan (auto-loads). Local `curl :1234/v1/models` lists ALL fleet models by id.
- `workhorse` SSH alias (`192.168.8.220`) timed out — DHCP IP moved; LM Link still reaches it. Re-discover via `arp -a | grep -i worksmacstudio`.

## Models (verified via web research)
- **Qwen3.6-27B**: dense, hybrid attention `3×(Gated DeltaNet linear) + 1×(Gated Attention)`, 64 layers, 262K/1M ctx, Apache-2.0. Smartest for coding (beats 35B on every bench). LM Studio MLX 4-bit ≈16GB. Released 2026-04-22.
- **Qwen3.6-35B-A3B**: MoE 35B total / ~3B active (256 experts, ~9 active), same 3:1 hybrid (40 layers), 262K/1M ctx, Apache-2.0. 3–4× faster, lower quality. MLX 4-bit ≈20GB. Released 2026-04-15.
- Both are HYBRIDS → no pure-attention model in scope → single quality-first context strategy.

## SPIKE — GO/NO-GO: hybrid prefix-cache vs tool-calling
The documented risk: on Gated-DeltaNet hybrids, a prefix-cache HIT can make the model lose tool-calling and emit plain text (stale recurrent state; omlx #825, mlx-lm #980).

**Test:** `scratchpad/toolcall_loop.py` — multi-turn, append-only (growing prefix → later turns are cache HITS), each turn requires a tool. Direct OpenAI calls to `:1234` (over LM Link).
- Direct single tool call, 35B: `get_weather({"city":"Paris"})`, `finish_reason=tool_calls`. ✅
- **35B-A3B, 12 turns: 12/12 correct tool calls.** Decode dropped 4.2s→~1.5s after turn 0 (cache active). ✅
- **27B, 12 turns: 12/12 correct tool calls.** Turn 0 = 16.2s (JIT-load on workhorse), then ~4.5s/turn. ✅

**VERDICT: GO.** LM Studio's hybrid-aware MLX engine does NOT exhibit the tool-calling-loss footgun on either Qwen3.6 model. (LM Studio's engine saves/restores KV at 256-token boundaries and handles Qwen3.5/3.6 — this is WHY LM Studio is the right engine vs raw mlx-lm/omlx.)

## Goose wiring
- Provider id is **`lmstudio`** (NOT `lm_studio`). Env `LMSTUDIO_HOST` (default `http://localhost:1234`), `LMSTUDIO_API_KEY` (dummy ok). `goose run` reads model from `~/.config/goose/config.yaml` (env `GOOSE_MODEL` not honored by `run`).
- Goose 1.38 smoke: `goose run --no-session -t "...PONG"` → `PONG` on 35B. ✅
- Goose multi-tool agent loop (developer extension, 35B): created files, ran a test, PASS. Tool-calling held across the loop. ✅
- QUIRK: Goose's developer extension resolves write/shell paths against an internal project dir, not always the shell CWD. Pin subagent working dirs explicitly (matters for git-worktree isolation). The subagent `TaskConfig.parent_working_dir` is the hook.

## Build from source
- `git clone --depth 1 https://github.com/block/goose.git ~/Projects/goose` (redirects to aaif-goose/goose, v1.39).
- `cd ~/Projects/goose && source bin/activate-hermit && cargo build --release -p goose-cli` → `target/release/goose` (243M), **4m33s** on M4 Max. ✅
- From-source binary validated against LM Link (created + cat'd a file via shell tool). ✅

## Recipe / sub-recipe mechanism (verified in source v1.39, file:line)
- `Recipe` (`crates/goose/src/recipe/mod.rs`): `settings{goose_provider,goose_model,temperature,max_turns}`, `response{json_schema}`, `sub_recipes[]`, `retry`, `parameters`, `extensions`.
- Sub-recipes are dispatched via the **Summon** extension's tools `delegate(source, instructions, async)` and `load(source)` — NOT as named tools.
- Per-subagent provider/model: `summon.rs resolve_provider()` / `resolve_model_config()` pull from delegate() param → sub-recipe `settings.goose_provider/goose_model` → `GOOSE_SUBAGENT_*` env → parent. A fresh `Arc<dyn Provider>` + `ModelConfig` is built and applied via `subagent_handler.rs update_provider()`. → **device routing is pure config.**
- Parallelism: `delegate(async:true)` → `tokio::spawn`, capped by `GOOSE_MAX_BACKGROUND_TASKS` (default 5). Sync delegate is sequential.

## SWARM PoC — multi-device, end-to-end (PASSED 2026-06-25)
Recipes: `recipes/swarm.yaml` (planner, `goose_model: qwen/qwen3.6-27b`) + `recipes/subrecipes/worker.yaml` (`goose_model: qwen/qwen3.6-35b-a3b`).
```bash
LMSTUDIO_HOST=http://localhost:1234 LMSTUDIO_API_KEY=lm-studio GOOSE_MAX_BACKGROUND_TASKS=5 \
  goose run --recipe recipes/swarm.yaml -n swarm-poc --max-turns 50
```
- During run, `lms ps` showed: `qwen/qwen3.6-27b` **GENERATING on WorksMacStudio.lan** (planner) + `qwen/qwen3.6-35b-a3b` **on Mac.lan** (workers). Live cross-device routing confirmed.
- Planner fired **3 `delegate(source:worker, async:true)`** → 3 parallel subagents (`subagent:6/7/8`) wrote `strutils.py`, `mathutils.py`, `dateutils.py` (correct code).
- Planner integrated `main.py` + `test_all.py`, ran `python3 test_all.py` → **"All tests passed!"**; `main.py` → `HI! / 5 / 6 / True`.
- 100% pure recipe config; zero Goose core changes.

## TYPED PLAN (task 6) — PASSED 2026-06-25
Recipe `recipes/planner.yaml`: 27B planner with `response.json_schema` (object{subtasks[], integration}), no tools (final_output only). Run on a 4-module CLI-todo task.
- Emitted **schema-valid JSON** via `final_output`: 5 subtasks, each with id/description/difficulty/model/depends_on/files + an integration step. Log: `docs/runs/planner.log`.
- Correctly derived the dependency DAG (task-model foundational; storage + scheduler independent/parallel; cli + tests dependent) and non-overlapping file ownership.
- Note: planner tagged all subtasks "easy" (→35B), incl. the stable multi-key sort (fair — trivial with Python's stable sort). The difficulty/model fields populate correctly; classification is model judgment.

## DIFFICULTY-AWARE SWARM v2 (task 7) — PASSED 2026-06-25
Recipe `recipes/swarm_v2.yaml` + `subrecipes/worker_generic.yaml`. Task: LRU cache (hard) + greet + slugify (easy).
- Planner (27B) classified & routed live: `lru.py` → `delegate(model=qwen/qwen3.6-27b)`; `greet.py`, `slug.py` → `delegate(model=qwen/qwen3.6-35b-a3b)`; dispatched async in parallel. `lms ps` showed BOTH 27B (workhorse) + 35B (mac.lan) generating.
- 27B worker produced a correct dict + doubly-linked-list LRU (sentinels, O(1) get/put, eviction). 35B workers produced greet/slug.
- Planner integrated `test_all.py` (14 tests incl. 2 LRU eviction-order). First run: 1 failure (`test_slugify_special_chars`) → planner FIXED it → re-ran → **14/14 PASS**. Demonstrates a verify→fix loop on top of the swarm.
- Files: `docs/runs/v2_out/{lru,greet,slug,test_all}.py`. Log: `docs/runs/swarm_v2.log`.
- NOTE: true **3-way fan-out** (same model on 3 devices) not yet exercised — 35B lives only on mac.lan; would need loading 35B on workhorse + macbook too (manual LM Studio step). The routing/dispatch mechanism is proven regardless.

## CONTEXT runtime test #1 (task 10) — 2026-06-25 — PARTIAL (key finding)
Goal: force compaction under a capped window and confirm tool-calling survives the re-prefill on the 35B hybrid.
Setup: `goose run` on 35B, `GOOSE_CONTEXT_LIMIT=12000`, `GOOSE_AUTO_COMPACT_THRESHOLD=0.5`, a 10-planet write+read+summary task (22 tool calls). Log: `docs/runs/context_test.log`, files: `docs/runs/ctx_out/`.
- Task COMPLETED correctly: 10 files written + read back + `summary.txt`; **tool-calling intact across all 22 calls**.
- BUT compaction did NOT fire. Goose CLI log (`~/.local/state/goose/logs/cli/...`) shows the session `total_tokens=8189`, `message_count=59`, and **no `Performing message compaction`** line.
- ROOT CAUSE: `GOOSE_CONTEXT_LIMIT` is applied via `with_default_context_limit` (a FALLBACK), but the lmstudio provider's `get_context_limit` (`goose-providers/src/openai.rs:529`) returns the EXPLICIT `model_config.context_limit` (set ~200K by the canonical catalog / probe). So the 12K cap was ignored → 8189/200000 ≈ 0.04 ≪ 0.5 → no trigger. Tool-pair summarization also didn't fire (22 calls < ~25 cutoff at a 200K limit).
- CONCLUSION: compaction-safety on hybrids is **NOT yet validated** at runtime. To cap the effective window (central to the quality-first strategy) we must set the EXPLICIT `context_limit`, not the default. Options: (a) override the canonical limit for qwen3.6, or (b) small additive provider change — treat `GOOSE_CONTEXT_LIMIT` as a hard ceiling = `min(configured, probed)`. This is the first context-pillar change that touches the fork's core. NEXT: implement the cap, re-run, confirm tool-calling survives a real compaction.

## CONTEXT runtime test #2 (task 10) — COMPACTION-SAFETY VALIDATED 2026-06-25
To force compaction conclusively, set `GOOSE_AUTO_COMPACT_THRESHOLD=0.0001` on a trivial task (create `a.txt` + read back), 35B. Dir `docs/runs/ctx_out3/`.
- Session store (`~/.local/share/goose/sessions/sessions.db-wal`) shows **4× "context was compacted"** + the run's goose log shows a compaction event → compaction FIRED repeatedly.
- The task COMPLETED correctly through those compactions (`a.txt` created, read back "apple", confirmed) → **tool-calling SURVIVES compaction on the Qwen3.6 hybrid.** ✅ Last unproven pillar-2 risk closed (stress-tested through 4 compactions).
- Corollary: the threshold env IS honored. Normal sessions don't compact because the effective `context_limit` is very large (~1M YaRN — local model ids are NOT in Goose's canonical catalog, so it probes the model's max, not the 200K shown by `lms ps`). 0.03 didn't fire at 12.7K because 0.03×~1M ≈ 30K > 12.7K.
- CONFIRMS: to realize the quality-first LEAN window (and make compaction trigger at a sane size), we must CAP the effective `context_limit`. That cap is the right next implementation (small provider/model_config change).

## FORK WIRED + first core change (2026-06-25)
- Fork: **leanzero-srl/goose-local-edition**. Local repo (`~/Projects/goose`): `origin`=fork via **SSH** (`git@github.com`, key `id_ed25519_github` → `ssh -T git@github.com` auths as `leanzero-srl`; HTTPS push has no creds so SSH is the path), `upstream`=block/goose (unshallowed, tags). Branch **`local-edition` committed + PUSHED**. Product folded into `local-edition/` (recipes, skills, docs, config).
- First core change — effective-window cap, env **`GOOSE_LOCAL_CONTEXT_CAP`** (additive; no-op when unset/0). Applied in BOTH `goose-providers/src/openai.rs::get_context_limit` (inherent method on OpenAiProvider) AND `goose/src/context_mgmt/mod.rs::check_if_compaction_needed` (via `Config::get_param` — the proven env path).
- VERIFIED via a temporary debug print: the cap WORKS — the check saw `context_limit=3000` under `GOOSE_LOCAL_CONTEXT_CAP=3000` (`cap_env=Some("3000")`). The real limitation is CADENCE: this proactive check runs ONCE per `goose run`, at reply-START (conversation = just the initial user msg, `current_tokens=23`; `session.usage.total_tokens` is None there so it estimates from messages only), NOT per tool-call turn. So proactive auto-compaction never fires mid-run regardless of cap/threshold; only REACTIVE compaction on a true model-window `ContextLengthExceeded` (`agent.rs:2363`) fires in-loop. → FOLLOW-UP: add a PER-TURN proactive check in the reply loop so the capped lean window actually drives mid-run compaction. Compaction-safety itself is validated separately (test #2). Debug print removed after diagnosis.
- Compaction-SAFETY itself was already validated (test #2 / captest3): tool-calling survives compaction on the hybrid. The cap is about *triggering* compaction at a lean size.

## 3-WAY FAN-OUT over LM Link (the headline) — VALIDATED 2026-06-25
Recipe `local-edition/recipes/swarm_v3.yaml`: planner 27B@workhorse fans 4 independent modules across THREE physical devices, each addressed by a DISTINCT model id on the one `:1234` endpoint:
  - casing.py, rot13.py → `qwen/qwen3.6-35b-a3b` @ mac.lan
  - palindrome.py → `qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx` @ macbook
  - vowels.py → `qwopus3.6-35b-a3b-v1-mtp` @ workhorse
- FIRST run (v3): mac.lan + macbook workers succeeded, but the workhorse worker hit **"Server error: Model is unloaded"** — JIT-loading the 38GB qwopus on workhorse WHILE it ran the 27B planner failed. 2/3 delivered; task incomplete. (Files: docs/runs/v3_out/.)
- FIX: PRE-LOAD all worker models — `local-edition/scripts/preload-swarm.sh` (`lms load <id> -y --ttl 3600`). VERIFIED workhorse holds BOTH the 27B planner (29.5GB) + qwopus-35B worker (38.7GB) at once (~68GB; 27B loaded in 9s on top of qwopus).
- RE-RUN (v3b, pre-warmed): all 3 devices delivered their modules in parallel; planner integrated `test_all.py`, self-corrected the test twice (verify→fix), → **"All tests passed!"** (4/4 modules). Files: docs/runs/v3_out2/. Log: docs/runs/swarm_v3b.log.
- OPERATIONAL LESSON: pre-warm worker models before a swarm; remote JIT-load during active generation is unreliable. FOLLOW-UP: have the swarm pre-load its device pool and/or retry on "Model is unloaded".

## SCHEDULER M1.1 + M1.2 — `goose swarm` runs the fleet (VALIDATED 2026-06-25)
- M1.1 `GooseAgentDispatcher` (`crates/goose-cli/src/commands/swarm.rs`): inlines the PUBLIC Agent drive sequence — `providers::create("lmstudio")` → `AgentConfig::new` → `Agent::with_config` → `create_session` → `update_provider(ModelConfig per device)` → add developer extension → `apply_recipe_components` → `override_system_prompt` → drain `reply` stream; captures the `recipe__final_output` ToolRequest argument FROM the stream (no private `final_output_tool` access). Maps errors → Transient (re-dispatch) vs Terminal.
- M1.2 `goose swarm "<prompt>"` (`cli.rs` Command::Swarm — the ONLY upstream-tracked edit; crate auto-joins via crates/*): planner(27B) → typed plan JSON → `Dag::from_planner_json` → `Scheduler::run` over the pool → report. **Compiled clean first try; clippy -D warnings clean.**
- FLEET RUN (text task: 3 planet paragraphs + a summary depending on all 3; pool mac(w2)+macbook(w1), planner 27B@workhorse): plan = 4 subtasks. **All 4 done; dispatched_per_device {mac:3, macbook:1}** — the weight-2 device did 3x the weight-1 device's work; the summary ran AFTER its 3 deps (dep-gating) and combined their outputs (shared-context pass-down). Log: `docs/runs/swarm_m12.log`.
- NOTE: `qwopus3.6-35b-a3b`@workhorse failed to PRE-LOAD ("LM Link connection closed" while workhorse held the 27B) → ran on the 2 healthy loaded workers (mac.lan + macbook). Reinforces M1.3 (pre-warm + retry) and M2 (pool menu). The scheduler's re-dispatch would have steered around it had it been in the pool.

## SCHEDULER M2.0 + M1.3 + reducer + live view — full code swarm VERIFIED (2026-06-25)
- M2.0 `goose swarm pool`: cliclack interactive menu + non-interactive subcommands (show/add/rm/weight/enable/disable/probe), persisted under the `swarm` key in config.yaml; `goose swarm run` reads the pool from config. clippy -D clean. Pool CRUD ops validated.
- M1.3: pre-warm planner + enabled workers (`lms load`) at run start; dispatcher re-warms on a transient "Model is unloaded"/connection error before the scheduler re-dispatches.
- REDUCER (task #9) via the plan: the planner now ALWAYS adds a final `integrate-verify` subtask (depends on all others; integrates + writes/RUNS tests).
- **FULL CODE SWARM VERIFIED**: task = lru/greet/slug (independent) + integrate-verify; pool mac(w2)+macbook(w1), planner 27B@workhorse. Result: all 4 done; `dispatched {mac:3, macbook:1}`; **all 4 files (lru/greet/slug/test_all.py) in the ONE shared working dir** (file-sharing across agents confirmed); the swarm-produced `test_all.py` (82 lines, 21 tests) **PASSES 21/0** → the swarm produced working, verified code end-to-end. Log: `docs/runs/swarm_code.log`.
- Live concurrency VIEW added to `goose swarm run`: each task prints `▸ run <task> → <device>` on start + `✓ <task> (Ns)` on finish — concurrent dispatch is now visible in the CLI. (Explains the "only one running" perception: LM Studio collapses 2 same-device requests into ONE `GENERATING` row, and short workers finish staggered; the planning phase is single-model.)
- NOTE: 27B is the planner, not in the worker pool, so `integrate-verify`'s preferred-27B gracefully falls back to a 35B worker. Add 27B to the pool (or a dedicated hard-worker device) for true hard-task routing.

## PER-TURN COMPACTION (task #10) — WORKS 2026-06-25
Added a per-turn proactive compaction check in the `agent.rs` reply loop, right after `conversation.extend` (the adversarially-verified safe spot), guarded by `did_recovery_compact_this_iteration` + cancel + `exit_chat`; mirrors the reactive block but continues (no break). It calls `check_if_compaction_needed` which honors `GOOSE_LOCAL_CONTEXT_CAP`. Before, the proactive check ran ONLY once at reply-start, so the cap never bit mid-run.
RUNTIME TEST: `goose run` with `GOOSE_LOCAL_CONTEXT_CAP=12000`, threshold 0.6, a 10-file growing task → 10 files created; **"Context near the cap — compacting to stay lean..."** printed; goose log shows compaction FIRED mid-run (≥1); NO loop; task completed (tool-calling survived). The cap now drives lean-context compaction during a session. clippy -D clean. Both single-agent `goose run` and swarm workers (agent.reply) benefit.

## 3-DEVICE SWARM via `goose swarm run` — VERIFIED 2026-06-25
Pool: mac(qwen35b, w2) + macbook(holo3, w1) + workhorse(qwopus, w2); planner 27B@workhorse. Task: 6 independent utility modules + integrate-verify.
- Live view: **5 tasks dispatched concurrently across all 3 devices** at start (mac 2, macbook 1, workhorse 2 — honoring weights); **work-stealing** (macbook pulled temp-convert right after gcd finished); **dep-gating** (integrate-verify only after all 6 modules done).
- All 7 done; `dispatched {mac:3, workhorse:2, macbook:2}`; all 6 modules + `test_utils.py` in the shared cwd; the swarm-produced test suite → **OVERALL: ALL PASS**. Definitive 3-machine parallelism + verified output. Log: `docs/runs/swarm_3dev.log`.

## Status: the swarm is feature-complete + validated
Done: scheduler core (7/7 mock tests) · GooseAgentDispatcher · `goose swarm run` · `goose swarm pool` menu (weights + instances) · idempotent pre-warm (no duplicate instances) · reducer (integrate-verify) · per-turn compaction (cap bites mid-run) · live concurrency view. Fleet-validated 2-device + 3-device, output passes its own tests.
DEFERRED (task #8, git-worktree write-isolation): assessed LOWER value / HIGHER risk right now — the scheduler's file-overlap locks + the planner's non-overlapping file ownership already prevent concurrent-edit corruption, and the shared-dir model is what makes integrate-verify see all modules (worktrees would isolate files and BREAK that flow unless a merge step is added). Recommend deferring until there's a concrete need for tasks that genuinely edit overlapping files. Other future: M2.1 crash-resume, EWMA weight auto-tune, add 27B as a hard-task worker for difficulty routing.

## Open items / next
- Difficulty-aware routing (planner tags hard→27B / bulk→35B) + typed plan (`response.json_schema`).
- True 3-way fan-out: same model-id on multiple devices → LM Link "Preferred Device" picks one; need to verify how to target a specific device (distinct aliases per device, or delegate-level addressing).
- Context condenser (pillar 3): quality-first hybrid strategy.
