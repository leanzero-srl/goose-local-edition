---
name: goose-mlx-inference
description: Operate and evolve goose Local Edition's in-house MLX inference engine (the supervised sidecar living NEXT TO LM Studio, never replacing it). Use when working on branch goose/mlx-inferencing, running the engine bake-off, mounting/benching MLX models, patching the engine fork, touching crates/goose-sidecar or the MLX desktop window, or when an experiment/finding about local MLX inference needs recording.
---

# goose-mlx-inference

## The campaign in one breath
Add an in-house open-source MLX engine that goose supervises as a sidecar — beside LM Studio,
which stays permanently. It supersedes only `crates/goose-local-inference` (the FFI path). Primary
workload: the SWARM (N concurrent long-context tool-calling agents). Plan of record:
`~/.claude/plans/i-want-you-to-enumerated-hejlsberg.md`. Campaign state: `local-edition/mlx/NOW.md`.

## Hard rules (Mihai's, verbatim intent)
- **Git identity on this repo is ALWAYS leanzero: `leanzero.srl <office@leanzero.net>`** (repo-local
  `git config`; set on 2026-08-30, matches the other LeanZero repos). Never the client or
  personal identities.
- LM Studio / LM Link / `lms` surfaces are NEVER removed or reconfigured. The engine is additive.
- Tests use **qwen3.5-9b 4-bit only, freshly downloaded through our own path**. The fleet's 27B is
  off-limits for mounting.
- Before ANY model mount or bench: `python3 local-edition/mlx/gates.py mount --model-path <dir> --port <p>`
  — exit 1 = BLOCKED, stop. Session start: `gates.py snapshot`; session end: `gates.py verify-fleet`.
- Sidecar ports: never 1234/11434. Models live in ONE configurable `models_dir` (default `~/.goose/models`).
- Engine changes in `swarm.rs` go through the `swarm-surgeon` agent (see `.claude/agents/`); the six
  swarm invariants and development gates apply unchanged. The knob-turning skill's "never touch
  crates/goose/**" rule does NOT block this branch's sanctioned feature work — but swarm gates do run.

## The engine (verdict 2026-08-31, evidence in experiments.jsonl — 8 scored runs)
**Rapid-MLX**, forked at **github.com/leanzero-srl/Rapid-MLX** (local clone ~/Projects/Rapid-MLX,
`upstream` remote → raullenchai/Rapid-MLX). Won on sustained-N=8 stability (rapid TTFT improved
across runs 8.0→7.3s; omlx degraded 7.7→13.1→11.7s non-recovering), working hybrid-aware prefix
cache (hit −26% TTFT, fidelity held), lower RSS (4.4 vs 6.5 GB). Both engines: fidelity 1.0, zero
errors — the June DeltaNet prefix-cache footgun (omlx #825/mlx-lm #980) is dead in CURRENT
versions, but every new engine version re-runs the bench `prefix_probe` before adoption.
- Pinned launch (proven): `uvx --from git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.4-lz.1 rapid-mlx serve <models_dir>/<model> --port 8090 --served-model-name <id> --enable-prefix-cache --max-concurrent-requests 8`
- Upstream draw runbook: in ~/Projects/Rapid-MLX: `git fetch upstream && git merge --ff-only upstream/main && git push origin main --tags`, then tag the head `vX.Y.Z-lz.N` and push the tag. New pin = bump `ENGINE_LAUNCHER` in crates/goose-sidecar/src/engine.rs AND append the OLD launcher to `SUPERSEDED_ENGINE_LAUNCHERS` there — `EngineSettings::migrate_launcher` moves every persisted default-following config to the new pin on load (2026-09-05; before it, existing installs ran the first pin forever). Prove it before adopting: `uvx --from git+…@<tag> rapid-mlx serve --help` shows the flags, `/v1/status` still has num_running+num_waiting, and the ignored live test `cargo test -p goose-sidecar -- --ignored live_mount_of_the_real_engine` with GOOSE_SIDECAR_LIVE_MODELS_DIR/MODEL_ID set mounts and unmounts clean. Last draw: 2026-09-05, v0.13.1 → v0.13.4-lz.1 (upstream head, 89 commits), live mount 13.3 s. The crate picks its TLS backend from the consumer, so the HF live tests (`hf::tests::live_*`) need `--features rustls-tls` when run as `-p goose-sidecar` alone — without it every one fails in 0.00 s with "invalid URL, scheme is not http" (not a network fault; curl to huggingface.co answers 200).
- Facts that void old assumptions: serves arbitrary local model dirs directly; TTL exists (`--resident-model-idle-ttl`); presence/frequency penalty per-request IS plumbed (frequency proven to bite at temp 0; presence same path, flat penalty needs margin); accepts `max_completion_tokens` (no remap entry needed); auto-config picks hermes+qwen3 for dense qwen3.5 and documents why dense DeltaNet must not take the hybrid scheduler path.
- oMLX default is thinking ON — if it is ever re-benched, disable via `chat_template_kwargs {enable_thinking:false}` first or the numbers are 2x-wall unfair.

## Where everything lives (built 2026-08-31)
- `crates/goose-sidecar` — supervisor (spawn/health/restart+breaker/per-pid kill), `fit.rs` (THE fit rule —
  MemoryGate is deleted; gates.py G1 still carries the retired 8 GiB/10% floor), `hf.rs` (MLX HF search `filter=mlx`, snapshot downloads with
  .part/resume/cancel), `engine.rs` (MlxEngineManager: stopped/mounting/running/failed,
  restart_required = argv diff). Global manager consumed by the ACP layer.
- ACP: 11 methods `_goose/unstable/mlxEngine/*` (crates/goose/src/acp/server/mlx_engine.rs,
  DTOs in goose-sdk-types custom_requests.rs); settings persist under config key `mlx_engine`.
- Desktop: "MLX Engine" nav view `ui/desktop/src/components/mlx/MlxEngineView.tsx` +
  `src/acp/mlx-engine.ts`; capability-gated on `mlxEngine`; old Local Inference settings UI removed
  (ModelSettingsPanel kept — ModelsBottomBar imports it).
- Swarm boundary: `crates/goose-cli/src/commands/swarm_engine.rs` — SwarmEngine trait,
  LmStudioEngine (verbatim), Engines registry with per-engine unservable partition; sidecar
  registration is the open step C (six decision points commented at their sites).

## The distributed engine (2026-09-24 — one model split across Macs; isolated from the single engine)
- Code: `crates/goose-sidecar/src/distributed/` (own manager `distributed::global_manager()`, own port, own config key
  `mlx_distributed`); ACP `_goose/unstable/mlxEngine/distributed{Status,Preflight,Start,Stop,ConfigUpdate}` in
  `crates/goose/src/acp/server/mlx_distributed.rs`; capability `mlxDistributed`. engine.rs/Sidecar unchanged (visibility only);
  the single engine's tests/argv goldens pass unchanged (99 → 99 present, lib 74 → 120 with the 46 new).
- One engine owns a Mac: `distributedStart` → refusal code `singleEngineMounted` while the single engine is mounted (UI
  offers "unmount and continue"); `mount` → `distributedEngineActive` while distributed owns the Mac. Never a silent unmount.
- Runner by `model_type`: qwen3_5 → `mlx_lm.server` tensor split via goose's own rank launcher + embedded `rank_wrapper.py`
  (caps as RAM ratios, /v1/models = the goose id only, /goose/progress, /goose/admission); qwen4_exp → the fork pinned
  BY COMMIT (provision.rs `PIPELINE_FORK_COMMIT`, env proof prints mlx, mlx_lm and the installed commit from
  direct_url.json): preflight reads `pipeline_qwen4 plan --json` (bytes, budget, verdict verbatim, batch 2, a derived
  context walks the planner's ceiling to its fixed point), launch runs `pipeline_rank.py` → the fork's
  `pipeline_qwen4_serve.serve()` with the approved `--split` (2026-09-24).
- Switch: `align_omlx_host_env` points OMLX_HOST at the distributed base URL while it owns the Mac (goosed-owned only), back
  to the single port after stop. The swarm router's `probe_mlx` targets it while THIS process runs it; goose-cli swarm
  lanes (swarm_engine.rs) do not.
- Proven LIVE 2026-09-24: the ratio hang rule on a mid-stream SIGSTOP (`hang_ratio_only`), the stream-cut rule, and the
  watchdog on real ballast pressure (WARN/CRITICAL reserves raised via `watchdog_warn_ratio`/`watchdog_critical_ratio`).
  Never gate on `kern.memorystatus_level` — it did not move with 18 GiB of ballast.
- Liveness = rank-0 step counter OR every rank's CPU time advancing; hang = silent > 10 × running median (≥3 samples);
  ps stat `T` = frozen at once. Watchdog per poll: kernel pressure WARN or available < 5% RAM → admission 503; CRITICAL →
  verified stop, never restarted. Restart breaker = the single Sidecar's (3 per 600 s, backoff 1 s → 30 s).
- Live tests + measured numbers: mlx-jaccl-cluster skill, section "goose's DISTRIBUTED engine".
- SERVED ID (2026-09-24, 3.0.26 defect): `engine::served_model_id(settings, model_id)` applies
  `mlx_engine.served_model_name` ONLY when `model_id == mlx_engine.model_id` (the alias names ONE model;
  AddNodeDialog / `goose swarm` write the pair). Before: the alias applied to ANY model, so a Flash split
  answered `/v1/models` as `mihai-qwen3.8-27b-atlassian-q8-mlx` and the 27B's swarm node routed to Flash.
  Now Flash serves `rapid-mlx/Qwen3.8-Flash-Next-4bit` (tile AND placement-card start, measured) while a
  27B split keeps the alias. The router's thinking-profile tie (`profile_template_kwargs`) follows: a
  non-alias served id IS its HF dir. Every distributed start (tile, placement card, section) goes through
  ONE backend entry, `on_mlx_engine_distributed_start` — the served id is never computed in the UI.
- LIVE READ OF A SPLIT (2026-09-24): the desktop reads rank 0's `<baseUrl>/v1/status` with the SINGLE
  engine's parser (`parseMlxLiveStatus`, now + `prefilledTokens`/`promptTps`) — the tile via
  MlxEngineView.refreshLive (source switches reset the rates), the tray via main's MlxEngineMonitor
  (`distributedBaseUrl` from the renderer's report, snapshot `engine: 'distributed'`); `runPhase(state,
  admission, activity)` → reading blue / writing green / queued orange; held/failed/loading win.
  MEASURED two-Mac Flash (M4 Max rank 0 + M3 Ultra over Link/JACCL), 7,226-token prompt: prefill rows
  2,048 → 4,096 → 6,144 of 7,226 at 302 → 458 tok/s, first token at ~15 s, 531 tok/s final prompt rate,
  writing 20.6 → 19.6 tok/s; 27B tensor split: 4,694 tokens at 412 → 431 tok/s, writing 16.1 → 12.8.
  `prefilled_tokens` stays 0 until the runner's first 2,048-token chunk (~5 s) — the row reads 0%.
- TRAP: a second 27B beside the owner's (the engine live test) fits the gate (need 30.6, budget 45.1)
  but macOS pages it out mid-load (resident 26.1 → 10.5 GiB) and the >0.9 resident assertion fails —
  environmental; run that test with the owner's engine unmounted.
- PREFILL PEAK (Q-104, 2026-09-26, measured): MLX 0.32.2's `mx.fast.scaled_dot_product_attention` has
  NO fused prefill kernel for head_dim 256 (both Qwen 3.x models) — a chunk materializes rows × query
  heads × chunk × context scores at bf16 (probe: 0.96-1.06× that product; head_dim 128 → ~0, the
  negative control). 27B tensor rank (12 heads): one row, chunk 2,048 at 262,144 tokens = 12.9 GB — the
  old plan's overhead ratio granted 3.5 GB. mlx_lm pads every batch row to the longest (E2E #2's
  summaries rode the agent's ~50k width) and its batch ops transiently hold up to 2.16× the padded KV
  (merge 1.5-1.6×, extend ≤1.96×, partial split 2.1×; its split deep-copies EVERY row, 2.29× for a lone
  row). Fix (worktree branch, tag `mlxLmServerPrefill`): plan.rs charges `workspace_bytes` = one row's
  chunk at the full context (chunk = the rank's headroom in KV steps, ≤ 2,048, ≥ 256, every rank the
  smallest); rank_prefill.py shrinks each step's chunk to that workspace for its rows × width; rank 0
  HOLDS a request while 2.2 × the joined batch's padded KV exceeds the KV charge (idle always admits);
  the prompt cache yields to the projected charge every step; a split that moves every row copies
  nothing (rank_batch.py). PIPELINE: the fork's workspace models `batch × tokens × context × 3` for
  attention but its prefill takes the DENSE path (QSA sparse routes are env opt-in) — Flash layer 3
  measured 5.03 GB at width 32k, chunk 2,048 (fork: 0.20 GB); preflight now adds the scores and
  passes `--prefill-step` (the largest chunk every stage fits) to plan + serve, lowering a derived
  context the 256 chunk cannot fit. Measure with real mlx_lm on a TRUNCATED view (config
  num_hidden_layers=N + symlinked shards; sharded_load is strict=False) — the last full-attention
  layer's SDPA is dead code in a prefill, so a 4-layer 27B view shows no scores at all.
- TRAP (2026-09-26): this MacBook's GPU wedged — processes stuck in exit (`?E`) on
  IOSurfaceSharedEvent waits, a 1024² matmul never finished — while two 27B loads and the single engine
  competed at kernel WARN/CRITICAL. A test that needs only cache ops runs on `mx.cpu` so it can never
  hang on the GPU.
- TRAP (2026-09-26, Q-113 — shipped in 3.0.44, every tensor split died at startup): `import mlx_lm.generate as g` binds the re-exported `generate` FUNCTION (mlx_lm/__init__.py), so `g.PromptProcessingBatch` raised AttributeError in the rank prelude. The tests passed because they stubbed mlx_lm or used `from mlx_lm.generate import …` (which resolves the module). RULE: every rank-program line that runs without a model — imports, hasattr checks, monkeypatch targets, signatures — gets a test that runs the SHIPPED text against the REAL provisioned venv (`the_wrapper_prelude_binds_real_mlx_lm_modules` is the pattern; skip loudly when the venv is absent). Bind submodules with `importlib.import_module`. And a split fix is not proven until a real split STARTS on the installed build — Q-104 was merged with no live launch.

## The Swarm provider and the provider surface (2026-09-05, owner's rule)
- **Only the defined providers exist in the local edition:** Goose Swarm (`swarm`) plus the swarm's four cloud
  families by REGISTRY id (aws_bedrock, zai, google, custom_deepseek). One allow-list, `LOCAL_EDITION_PROVIDER_IDS` in
  `ui/desktop/src/components/leanzero-swarm/cloudProviders.ts`, derived from CLOUD_PROVIDERS so it cannot drift; applied to the
  model picker, onboarding, /configure-providers and the hub's Cloud Providers tab. omlx/lmstudio stay REGISTERED in Rust (the
  sidecar path needs omlx) but are never offered; an active omlx/lmstudio provider is migrated once, loudly, to swarm. The edition
  defaults to LOCAL with nothing persisted — this fork IS Goose Swarm.
- **`swarm` has two model ids:** `swarm` = CHAT — the turn is served by an idle node of the configured pool through
  `crates/goose/src/providers/swarm_router.rs` (process-wide idle guard: pool re-read every turn; capacity = instances or the
  sidecar admission cap `goose_sidecar::engine::MAX_CONCURRENT_REQUESTS`; free = cap − max(leases, live in-flight); sticky per
  conversation, else most-free, else QUEUE with no timeout; zero servable → a named error; admission 503 → next node; the permit
  rides the stream; context limit = the pool's minimum window). `swarm-build` = the brief → `goose swarm run` (the run panel path,
  unchanged). A mesh PEER's sidecar is not a node (loopback-bound; the Link mlx proxy is the only door) — LM Link fans LM Studio
  models across machines. Never add a clock to the queue (gate 5); never let a probe failure read as "idle".

## KV-cache compression (2026-09-24 — per model, OFF by default = no flag)
- Profile `kv_cache` (wire `kvCache: "int8"|"int4"`, absent = off) → `--kv-cache-dtype int8|int4`. goose_sidecar::kv_cache
  prices a token from config.json: full-attention layers × 2 × kv_heads × head_dim × act bytes; quantized = bits/8 per element
  + one scale + one bias per group (engine group 64, 32 for head_dim 96, none for 80 → goose refuses the mount). 27B:
  16/64 layers carry KV, 65,536 / 34,816 / 18,432 B per token. modelsList `kvCache`/`kvCacheError`, `kvCacheMeasurement` from
  the model folder's `goose-kv-cache.json` (written by `kv_quant_compare.py --record`). UI row: MlxKvCacheFields.tsx.
- Engine: lz.2 REFUSED int8 on every qwen3_5 ("the loaded model is incompatible: ArraysCache" — the live-cache probe
  rejected the GatedDeltaNet state caches). lz.3 (fork 999d43ea6, branch lz/kv-quant-hybrid) admits exact ArraysCache as
  fixed state and prices the compressed cache in D-METAL-CAP. Log line that proves it engaged: `[kv-cache] hybrid partial
  quantization: 16/64 full-attention layers use int8; 48 recurrent-state ... stay unquantized`; `/metrics`
  `rapid_mlx_kv_cache_dtype{dtype="int8"} 1`. qwen4_exp (Flash) still refuses (Qwen4ExpStateCache subclass + CacheList/QSA).
- Distributed: NEITHER runner can hold a quantized KV (mlx_lm.server BatchGenerator has no quantized batch cache; the pipeline
  fork builds KVCache + QSAIndexCache) and neither reads model profiles → planner/preflight stay bf16; the UI row says so.
- MEASURING (evals/mlx-engine-bench/kv_quant.py + kv_quant_compare.py): one FRESH engine per config on :8093 with
  `RAPID_MLX_PREFIX_CACHE_AUTOLOAD=0` (never read/overwrite the owner's persisted cache); quality phase WITHOUT MTP, memory/decode
  WITH MTP. Compare configs on the SAME nonce (`--nonce`) — a different first-line nonce alone moves greedy output (bf16 vs bf16
  with another nonce: 45% agreement to first divergence, 5/13 identical), so cross-nonce numbers measure perturbation, not KV.
- MEASURED 2026-09-24 (results/2026-09-24-kv-quant/README.md): 128k Metal peak 67.3 → 57.7 (int8) → 54.1 GB (int4);
  decode at 32k 17.3 → 12.0 (−31%) → 15.4 t/s (−11%); same-prompt greedy vs bf16: bf16 rerun 13/13 identical, int8 8/13
  (divergences only at exact ties, margin 0.0–0.25 nats), int4 4/13 (margins to 0.625); buried facts at 31k/35k/110k found by
  all; agent tool names unchanged. Verdict: default stays OFF; int8 is the opt-in for memory-bound long contexts.
- TRAPS (each cost time): (1) through lz.3 a `logprobs:true` request on an MTP-mounted engine ABORTS the whole process
  ("There is no Stream(gpu, 1) in current thread", helpers.py `_extract_token_logprob` on a lazy array) — FIXED in
  v0.14.3-lz.4 (fork ce15b39ff, see "MTP logprobs" below); an engine still on lz.2/lz.3 (check `ps` for the tag) must
  never get logprobs. (2) `uvx --from "…@ file:///worktree"` caches the built wheel by pyproject mtime: .py edits after the first build are
  NOT picked up — grep the archive (`~/.cache/uv/archive-v0/*/…/rapid_mlx`) for your change before trusting a run. (3) a second
  27B beside the owner's is a memory-gate BLOCK (43.4 GiB needed, ~31–42 available): unmount his via the tray menu, measure,
  remount via the tray. (4) other agents share the Mac — node/cargo at 100% CPU during a speed phase contaminates tok/s; check
  `ps -r` and say so.

## MTP logprobs — the lz.4 fix (2026-09-24)
- Mechanism: the speculative paths (vendored MTP, suffix, DSpark) yield LAZY rows (`lps[i]` views) made on the engine's
  mlx-step thread stream; the route thread's `np.array` evaluated them inside the buffer protocol, where an MLX throw is a
  libc++ `terminate`. Measured in isolation: `mx.eval`/`.item()`/`.tolist()` on such a row RAISE (catchable);
  `np.array`/`np.asarray`/`memoryview` ABORT. Upstream 0.15.1 still carries it (no upstream fix/issue).
- Fix (fork ce15b39ff, branch lz/mtp-logprobs, tag v0.14.3-lz.4): scheduler `_materialize_response_logprobs` =
  one `mx.async_eval` per step on the step thread (a blocking `mx.eval` cost 900→620 tok/s on a tiny model; async is in
  noise); `_extract_token_logprob` `mx.eval`s first so a residual failure fails ONE request with a named RuntimeError.
  Tests: tests/test_logprobs_cross_thread.py (subprocess-isolated, abort as negative control).
- Repro WITHOUT the 27B: a tiny random qwen3_5 + mtp.safetensors (hidden 256, 4 layers, the 27B's tokenizer copied)
  mounts with MTP active in seconds; zeroing `model.norm` and `mtp.norm` makes every draft accepted (19/19) — the only
  cheap way to exercise the accept path. Run fork code with `PYTHONPATH=<worktree> <uv-env python> -m rapid_mlx.cli serve`
  (sidesteps trap (2)). `cp -R` of a model dir the engine had open produced a 0-byte model.safetensors — rewrite via
  mx.load/save instead.
- Live proof: the ignored `live_mount_of_the_real_engine…` test now sends a logprobs request and asserts 200-with-entries
  (or a 400 naming logprobs) AND the same pid still serving — 27B with MTP active: 200, 8 entries, unmount clean.

## Warm vs cold, and what a census actually measures — Q-108 (2026-09-26, Studio, lz.7)
Suspicion was "the hybrid prefix cache restores state that is not a cold prefill, so a warm engine loses its
place". REFUTED, deterministically. Tools: `warm-cold/` next to this file.
- Method (reuse it for every new pin): capture a real goose session's request bodies with `proxy.py`
  (listen 18572 → tunnel 18571), then `replay.py` the same requests at temperature 0 with top-5 logprobs on
  fresh engines per arm (`studio_serve.sh <label> <cache on|off> <mtp on|off>`, then `POST /v1/cache/clear`,
  because a fresh engine is NOT cold: it LOADS the persisted prefix cache from
  ~/.cache/rapid-mlx/prefix_cache/<model> at boot), and `compare.py ref test` → first divergent token,
  both sides' top-2 margin, max |Δlogprob| before it.
- Result (21-turn census, 4.1k→8.6k-token prompts): cache on vs off 22/25 identical (MTP on) and 24/25 (MTP off);
  a SECOND session on the warm engine vs cold 18/21; MTP on vs off 21/25. EVERY divergence is an exact bf16 tie
  (logprobs are quantised to 1/8: one side's top-2 margin 0.0, the other 0.125) on a harmless token
  (" per"/" the", "="/"=path"); max |Δlogprob| before any divergence 0.125 = one quantum. The cache resumes at
  2048-stride checkpoints (4096, 6144, 8192) and restores bit-for-bit up to that quantum.
- Cross-session reuse is nil anyway: the working directory sits in the first user message at token ~1756,
  below the first 2048 checkpoint, so session 2's first request is a MISS.
- What really derails a census (read the words, 60 samples/arm of turn 2 at goose's sampling — goose-cli
  sends no temperature, so the checkpoint's generation_config applies: 1.0 / 0.95 / 20): the model opens with a
  FALSE claim about the session ("I need to go back and redo step 2 since I skipped it", "I need to continue
  from where I was interrupted", "Step 1 failed because the `docs` directory did not exist") in 12/60 fresh,
  9/60 warm, 12/60 fresh without MTP, 6/60 on a second fresh engine — no warm/MTP effect. Temperature only
  halves it (0.7/0.8: 4/60, 0.6: 6/60, 0.3: 3/60). The cause is the REQUEST SHAPE: the census binary
  (target/debug/goose of 2026-09-25 21:14) predates Q-94 and posts `<turn-context>` as its own trailing USER
  turn; joined to the tool result (the app's shape since 41fff6704) or dropped, it is 0/60 at turn 2 and 0/60
  at turn 3, and direct calls rise 23–25/60 → 45–51/60. A census must run a goose built from the current tree
  (`strings <bin> | grep -c rapid_mlx_transient_tail_on_tool` ≥ 1) or it measures a shape the app no longer sends.
- The ledger's "fresh clean, warm failed" split was confounded: g1 (clean) and g2 (failed) BOTH ran after the
  same 10 replays on their engines; with ~16% per early turn, one derailment then feeds itself through history.
- End to end with goose built from main (joined shape), one lz.7 engine, three censuses in a row: 59 calls,
  0 failed, 0 false-state texts, no "attached"/"redo" spiral. Not perfect: n1 (the FRESH one) skipped step 20
  and still answered "All 20 steps succeeded."; n3 ran step 6 before step 4; n2 was 20/20 in order.
- Census harness traps: `--provider openai` never probes `rapid_mlx_transient_tail` (only `omlx` is in
  PROVIDERS_FRONTING_RAPID_MLX), so no tail is marked and every turn resumes from a 2048-stride checkpoint
  (re-prefilling 600–1,800 tokens) instead of the message boundary. Run it as `--provider omlx` to measure the app.
- Residual, not fixed here: 1–6/60 replies open with `!` junk in plain text (`! [Image: …]`, `![](https://…)`) —
  the text-side twin of Q-85's tool-argument `!`; the lz.7 guard covers only the tool-call skeleton.

## Placement planner + "Measure speed" (2026-09-24 — design local-edition/mlx/DESIGN-PLACEMENT.md, phases 1–2)
- Code: `goose_sidecar::placement` (chip, model, predict, planner, store, bench); ACP `mlxEngine/{placementPlan,measureSpeed,
  speedHistory}` in `crates/goose/src/acp/server/mlx_placement.rs` (capability `mlxPlacement`); chat-turn recorder
  `providers/mlx_speed.rs` (one wrap in swarm_router for MlxSidecar/MlxRemote); UI `PlacementCard.tsx` + picker badges.
- FACTS: chip = `sysctl -n hw.model machdep.cpu.brand_string` + `ioreg -rc AGXAccelerator -d1 | grep gpu-core-count` (20 ms;
  system_profiler 400 ms). Bandwidth ONLY from Apple's spec pages (table in chip.rs, URL per row; base M1 has none → gap).
  This Mac's GPU ceiling = Metal `recommendedMaxWorkingSetSize` via the `metal` crate (== MLX's figure, measured). Peers
  answer the discover script's `@@chip`/`@@gpu` (managed env's mlx) — a peer goose older than bf03d52c1 is a named gap
  "update goose there", NEVER a guess; so the Link peer's chip needs the new goose installed THERE.
- MODEL: active bytes/token = dense + routed×k/E from safetensors headers (lookups = embed/`embedding` tables; mtp + vision
  excluded). 27B 26.47 GiB active; Flash 3.53 of 95.71 GiB, largest layer 31.18 GiB (the PLE layer).
- FORMULA (predict.rs, every factor fitted to OUR runs at startup): t = c + bytes/BW_eff, c from llama.cpp #4167 (M3U 5.2,
  M4M 4.3 ms), BW_eff least squares on the 27B+32B single runs → M3U 706, M4M 449 GB/s (worst residual 5.1%); all-sum
  JACCL 0.28 / ring 0.55 ms (the naive 21 µs formula was 2× off); MoE class 0.17 of dense from the Flash pipeline run,
  range up to 0.51 (published MoE share); rapid-mlx batch gain ×1.785 at 8 (experiments.jsonl). The SINGLE engine's
  Rapid-MLX/MTP gain over the mlx_lm formula is recalibrated PER MODEL from goose's own measurements
  (`single_engine_factor`). Estimates are labelled with a range; any measured (model, placement, backend, bucket) wins.
- RULE: fit = `fit::judge(need, NodeMemoryFacts)` — budget min(avail − RAM × AVAILABLE_MARGIN_RATIO 9.3%, Metal ceiling),
  warn inside DERIVED_CONTEXT_MARGIN_RATIO 2% of RAM. ONE function for the mount gate, the planner's single fit
  (`planner::single_engine_need`: weights + KV at the chat benchmark's 2,304 tokens), plan::budget_bytes (tensor), the
  preflight and the desktop (`mlxEngine/status {fitModelId}` → `mountFit`; mountCost's TS copy is deleted). A mounted model's
  footprint counts as available to the gate, as the planner counts it (9c170176f). Tensor via
  plan.rs arithmetic (JACCL only, even divisor), pipeline via the fork's `plan --json` (`run_fork_planner`, fixed-point
  walk) or a labelled aggregate estimate; goal pick with tie (overlapping ranges) → fewer Macs; `best` vs `bestAvailable`.
- STORE: `<data dir>/mlx-speed-measurements.jsonl` (`Paths::in_data_dir`), one record per benchmark workload / chat turn;
  bad lines are listed (`storeErrors`), never skipped. Chat turns record decode + TTFT; prefill only with cached-token usage.
- BENCH: `bench::Workload::Chat` ≈1.9k prompt tokens (bench.py vocabulary, 1.118 tok/word measured) + 256 greedy tokens,
  nonce at token 0; `LongDocument` ≈30k. Token-counted, no clock. Refused unless that placement is RUNNING.
- LIVE recipe (backend): `GOOSE_PATH_ROOT=/tmp/gplace target/debug/goose serve --port 18790 --dangerously-unauthenticated`
  with a seeded config (models_dir → ~/.goose/models, mlx_distributed peer `ssh: workhorse` so the NEW probe runs there),
  then `node <scratchpad>/live/acp.mjs ws://127.0.0.1:18790/acp _goose/unstable/mlxEngine/placementPlan '{"goal":"chat"}'`
  — 5.4 s with both Macs + the fork planner. TRAP: never benchmark while the KV agent's :8093 decode phase runs (GPU/CPU
  contention poisons both numbers); cargo/vitest during it contaminates their tok/s too.
- LIVE UI (packaged app, isolated GOOSE_PATH_ROOT + GOOSE_USER_DATA_DIR, CDP 9461): a shell-launched window
  reads `document.visibilityState == "hidden"` (screen locked / occluded) and the Engine view's status poll never starts —
  the hero sat on "Mounting" for 7 min while goose said running. Drive it by overriding visibility over CDP
  (`Object.defineProperty(document,"visibilityState",{get:()=>"visible"}); document.dispatchEvent(new Event("visibilitychange"))`).
  MEASURED 27B single on the M4 Max (rapid-mlx lz.3, MTP, "Windows App" at 110–235% CPU beside it): 1,962 + 256 tokens,
  cold 165.6 / warm 226.6 tok/s reading, 16.7 tok/s writing both runs.
- RUN IS A SWITCH (2026-09-25, 47c31c32e): Run on a way stops the way of THIS model that runs now (local unmount / peer
  remoteSingleStop(unmount) / split followed until it no longer owns the Mac), each returns after the engine exits, then
  starts. WHY: 3.0.29 started a second 31 GB copy on this Mac beside the Studio's; the route kept chat on the Studio, so the
  copy sat idle. The plan credits the peer copy's memory to this model's placements: the peer engine's `/v1/status` through
  the relay → `metal.active_memory_gb + cache_memory_gb` (DECIMAL GB, Rapid-MLX `/1e9`; measured 40.17 + 13.02 on the
  Studio's idle 27B) added to that node's available in `plan_raw`, before goose's fit AND the fork planner. A running split's
  ranks are still NOT credited (their per-rank footprint is unread) — a split → single switch can read "short" until then.
- LIVE TRAP (harness): my Run-button finder returned -1 and `.nth(-1)` clicked the LAST Run on the page — it started the
  27B on this Mac. Always `process.exit` when the index is < 0; never feed a -1 into nth().

## Thinking controls (2026-09-23 — per model, OFF by default = send nothing)
- WHY: Rapid-MLX turns thinking OFF on every tool-bearing request (`service/helpers.py`
  `maybe_auto_disable_thinking_for_tools`) unless the request pins `chat_template_kwargs.enable_thinking`,
  top-level `reasoning_effort`/`reasoning_max_tokens`, or `tool_choice:"none"`. NOTE: `chat_template_kwargs.reasoning_effort`
  alone does NOT stand the auto-disable down — under Auto with tools the effort level is inert (Qwen3.8 only reads it
  while thinking is on).
- Capability record: `goose_sidecar::thinking::model_thinking_capabilities(dir)` parses the tool-use template ONCE per
  source (minijinja `unstable_machinery` AST, a port of rapid-mlx `detect_native_reasoning_effort_levels`). ORACLE for any
  change: run the engine's own function on the fixture — `~/.cache/uv/archive-v0/<hash>/bin/python -c "from
  rapid_mlx.utils.chat_template import detect_native_reasoning_effort_levels as d; print(d(open(F).read()))"`.
  Qwen3.8 = switch enable_thinking, levels xhigh/medium/low, default xhigh, preserve_thinking.
- Profile: `ModelProfile.thinking` (None=auto | on | off), `.reasoning_effort` (None=template default); no argv effect.
  Wire: profile `thinking`/`reasoningEffort`, modelsList `thinking{thinkingSwitch,effortLevels,defaultEffort,
  preserveThinking,budgetForcible}` / `thinkingError`.
- Request seam: `swarm_router::route_stream`, MlxSidecar nodes only, via `request_params.chat_template_kwargs`;
  captured per SESSION (SwarmProvider field) so a mid-session edit never voids the prefix cache. The bare `omlx`
  provider and goose-cli swarm lanes do NOT get it. Defaults = byte-identical request (test
  `default_choices_leave_the_mlx_request_byte_identical`).

## Live state: the tile, the tray, and "who is using it" (2026-09-23)
- Rapid-MLX `/v1/status` cannot tell clients apart and has NO per-request prefill progress
  (`scheduler.get_running_requests_info`: prompt_tokens, cached_tokens, ttft_s, tokens_per_second only). The honest
  reading rate is `(prompt_tokens - cached_tokens) / ttft_s` of the newest generating request (oMLX's "prefill speed";
  ttft runs from arrival, so it is conservative). `generation_tps` and `prompt_tps` are STICKY aggregates and
  prompt_tps counts cached tokens as read — never display either. Code: `mlxLiveStats.ts` measuredPrefillTps/LastRates.
- WHO: goose registers in-flight work at its two doors — the swarm router's lease on the mlx-sidecar node (session id
  from the reply loop's task-local) and the OpenAI-compatible turn — in `providers/mlx_serving.rs`, served at
  `GET /mlx-engine/serving` on goose serve's API routes (X-Secret-Key). Anything else on the engine (a `goose swarm run`
  child, another app, a linked peer) is COUNTED as unattributed, never named.
- MAIN owns one loop (`utils/mlxEngineMonitor.ts`, the view's 2 s cadence) that runs only while the engine answers or goose
  says "mounting"; woken by every local ACP `mlxEngineStatus` read (reported via IPC `mlx-engine-report`), tray creation
  and the tray menu opening. The tray section is a pure model (`utils/mlxTray.ts`); Mount/Unmount run in a window's
  renderer (`hooks/useMlxTrayActions.ts`) because main has no ACP client.
- Tile colours while RUNNING: slate idle / accent reading / ok writing / slate when activity is unknown.
- TILE WORDS (owner, 2026-09-25, 3.0.31–3.0.33): the headline IS the activity (Idle / Reading prompt / Writing / Queued) —
  `servingEngine().wordText`, shared with the other tabs' badge; "Running" only when up with no live read. Only MEASURED
  figures are drawn (no dashes, no "nothing written yet"); the rate trace only while writing; cache-saved only when > 0.
  He reads the tile against the tray ("Remote · Idle"): the two must never disagree.
- RESTORE RACE (3.0.31): route_status asks the proxy first and the peer's status only on failure — an engine that becomes
  ready between the two reads looked "running but not serving". Fixed: re-ask the proxy after the peer says running
  (`route_models`). A failed restore line clears itself once its model serves (`settleRestoreLine`); Run it re-plans when
  what serves changes. Harness trap: `restore.mjs` breaks on "Running" — the headline no longer says it; key on Idle.
- TRAP (2026-09-24, 3.0.28 → fixed 6e287d4b9): main's `net.fetch` rides `session.defaultSession`, whose
  `onBeforeSendHeaders` hook (upstream goose) stamped `Origin: http://localhost:5173` on EVERY request — main's too. The
  Link relay refuses any Origin-bearing request (403), so a remote single's tile read "Rates unavailable over LeanZero
  Link — engine returned 403" while chat worked (goosed's reqwest sends no Origin). Now the stamp applies only when
  `details.webContentsId` is set (`utils/rendererOrigin.ts`). To see what main really sends: run a python echo server on
  127.0.0.1:8777 and call `window.electron.mlxLiveStatus('http://127.0.0.1:8777/x')` over CDP 9333.
- Restore on relaunch (3.0.28): the serving intent is saved only from 3.0.28 on — an app upgraded FROM an older build
  comes back with nothing to restore. With the Studio engine still up, relaunch reattaches in <1 poll.

## Mount refusal, load progress, reading vs writing (2026-09-24 — 9c170176f, df7bdfff1, d8767618b)
- A gate Block is `engine::MountRefused` (still Err for Rust callers); ACP `mlxEngine/mount` answers
  `{refusal: {fit, alternative, badge, alternativeError}}` — NOT an RPC error (the old error + status.gateMessage
  was the owner's two banners). alternative = `planner::alternative_to_this_mac` (Flash on the M4 Max → the pipeline).
  Remote single reads a peer's refusal as `peerMountFailed`.
- Badge `needsBothMacs{needs}`: with NO distributed setup on this Mac, placementPlan measures the Link peers; a peer
  whose distributed-node switch is off answers no Discover → its memory + `gpuCeilingBytes` come from its
  mlxEngine/status over the mesh and the split carries `split_refusal` ("allow this Mac to serve …"). The workhorse's
  3.0.25 "Too big" was the planner seeing no peer at all (peers came only from the saved setup).
- Single load: `status.load {phase makingRoom|starting|loading|warming, residentBytes, weightsBytes}` — RSS of the
  largest process in the engine tree (StartupWatch); MEASURED warm 27B: loading 0.30 → 30.05 GiB of 30.5, 13.8 s to
  running. Phase words are Rapid-MLX's stderr ("Loading model/MLLM", "Warming up") — stdout is discarded by the sidecar.
- Rank load: node `loadPhase` (loading until RANK_CAPS, warming until READY) + `plannedWeightsGb`; activeMemoryGb is
  the numerator (MLX active; mx.eval releases the GIL — measured 21.9 of 31.0 GB mid-eval). The tensor wrapper's
  RANK_MEM reporter now starts before the load. Peer: `hosting.{phase, loadedBytes, plannedWeightBytes}` on BOTH
  distributedStatus and mlxEngine/status; its `state` said "serving" at group join (before any weight) — fixed.
  Layers-loaded is NOT measurable (the fork evals a stage in one mx.eval).
- Rank 0 `/v1/status` = Rapid-MLX shape + `prefilled_tokens`, `prompt_tokens_per_second` (rank_live.py builds it;
  rank_wrapper.py wraps ResponseGenerator.generate; pipeline_rank.py extends the fork's _Job/run_batch/_step/_build_app —
  no fork change). TRAP: mlx_lm's first prompt-progress report comes after its first chunks (30 s on a 15k prompt) —
  prefill is stamped when the context comes back. TRAP: the fork's _Job dataclass has no __post_init__, so a subclass's
  __post_init__ never runs (500 on /v1/status) — extend __init__. HARNESS: 2 local ring ranks (ring_hosts 127.0.0.1:55xx,
  distinct ports) for the 27B tensor; the Flash pipeline via a test-only `load_stage(layer_limit=4)` shim.
- UNMOUNT STOPS A LOADING ENGINE (Q-112, 2026-09-26). Before: `unmount()` on `Mounting` flipped the state and
  returned at once; the start task loaded on to ready HOLDING the Mac's load lock, then shut the engine down.
  Measured on 3.0.44: Studio got `unmount` 05:53:49.106, "sidecar ready" 05:53:51.315 — the MacBook's split
  preflight between them read the lock held and refused. Now: `SidecarConfig.start_cancel` (`StartCancel`) ends
  `await_ready` — terminate (SIGTERM pid → grace → proven-group SIGKILL), release_port, `StartCancelled` — and
  `unmount` waits the start task's `ended` watch (sent AFTER the lock is dropped). A mount overtaken while its
  gate judged → `MountStopped` (unmount counter + `judging` RwLock). Run it's switch to the SPLIT also awaits
  `dropRoute().settled` (the peer's unmount answered) — the 3.0.44 split start began before the Studio even got
  the unmount. A load the split does NOT own refuses with code `modelLoading`: "<Mac> is loading <model> right
  now…", pid/port/elapsed only in `detail`; the lock record carries `model=` (single engine AND ranks).

## Available memory on macOS (2026-09-23 — the measure under the mount gate and the page)
- `memory::measure()` on macOS = `host_statistics64(HOST_VM_INFO64)`: (free_count − speculative_count) +
  external_page_count + purgeable_count, × page size — Activity Monitor's physical minus (app + wired + compressed).
  `reclaimableCacheGb` = external + purgeable. Linux keeps sysinfo `MemAvailable`. A failed probe is `memoryError`,
  and mount refuses on it (`measure()?`).
- NEVER sysinfo on macOS: `free + inactive + purgeable − compressor` read 0.0–3.6 GiB while 41 GiB was available
  (a 41.8 GiB compressor subtracted, active file cache dropped) — the "0.0 GB free" orange page. NEVER
  `kern.memorystatus_level` (memory_pressure's %): 12 GiB of held anon memory left it at 46–47% while the chosen
  figure fell 41.4 → 28.0. raw `free_count` INCLUDES speculative (vm_stat subtracts it). Probe: vm_stat + a
  16 KiB-page C loop over host_statistics64; cache load = `cat` of already-downloaded shards, never a download.
- local-edition/mlx/gates.py G1 still sums vm_stat free + inactive + speculative + purgeable — NOT parity (counts
  inactive anon, drops active file cache).

## Releasing a notarized macOS build (2026-09-05 — every release is notarized, one command)
- `just release-notarized <version>` (bump from ui/desktop/package.json's current version; the own-version floor is 2.0.0). It sources
  `~/.leanzero/apple/notary.env`, unlocks the dedicated `goose-signing` keychain, builds, signs with the Developer ID (Mihai Perdum,
  ZZ8MTZ6NRZ), notarizes + staples the app and the DMG, verifies with `spctl`, and rebuilds the auto-update zip + manifest. Run it under
  hermit with the corepack pnpm shim FIRST on PATH; the DMG lands in ui/desktop/out/make/. Then commit the package.json bump.
- New Mac: `bash ui/desktop/scripts/bootstrap-signing.sh` (pulls the bundle from `workhorse`, creates the keychain, proves signing and
  the Apple login). Full setup notes: ui/desktop/NOTARIZATION.md → "LeanZero setup".
- Never put the bundle in git; never use the login keychain for the identity (it prompts for the user's password).
- Then `just publish-release <version> [notes.md]` — GitHub release (DMG + Goose.zip + latest-mac.yml, --latest). gh is logged in as leanzero-srl on the workhorse; token in `~/.leanzero/github/token.env` (never in git). First notarized release: v2.0.3 (2026-09-05).

## The evolution loop (this is the durable memory — update as you go)
1. Every experiment/change lands as one row in `local-edition/mlx/experiments.jsonl`
   (`void_reason` string when not a pass, never a bare fail) AND a dated entry in `LEDGER.md`
   (newest first: Did / Learned-and-it-changed-the-design / gate defects found).
2. A loss becomes a RULE change: new gate in `gates.py` (with BLOCK+ALLOW self-test in
   `gates_selftest.py` — `python3 gates_selftest.py` must pass) or a line here. Never note-and-ignore.
3. Findings triage ON ARRIVAL into `QUEUED-FIXES.md`: IMPLEMENT / DROP / SCHEDULED(condition).
   Report the ratio, not the count.
4. Solutions evaluated-but-unpicked go to `TRUMP-CARDS.md` with the ONE idea worth stealing.
   Improvement hunts fan ONE agent per card — brief = repo + idea + our exact pain point, nothing else.
5. `NOW.md` changes in the SAME commit as any thread change. After a compaction: read NOW.md first,
   then LEDGER.md head, then `git log --oneline -10`; never resume from a summary.

## Fan protocol (Claude subagents on this workstream)
Fan → reduce in code → adversarially verify → synthesize. A brief carries ONLY: goal, exact files,
constraints, verification. No campaign history — sharp and fast beats informed and diluted.

### Verifying the menu-bar tray live (2026-09-24, 3.0.17)
Screen capture of the menu bar fails from this harness ("could not create image from rect"); read the tray through Accessibility instead:
`osascript -e 'tell application "System Events" to tell process "Goose Swarm" to get title of menu bar items of menu bar 2'` (title) and click `menu bar item 1 of menu bar 2` then read `name of menu items of menu 1 of …` (menu; press key code 53 to close). Menu items can be CLICKED the same way ("Mount <model>", "Unmount the MLX engine") — a real user path, no CDP needed.
Measured on 3.0.17: title '' when off (by design) → "Mounting" → "Idle" → "Reading 3.0k" (prompt 3,032 tok) → 18.0–22.7 tok/s → "Idle"; the mounted menu lists model, last run (wrote 21.5 tok/s, read 238 tok/s), cache hits, served count, uptime, GPU memory.
The Thunderbolt copy UI renders NOTHING unless Link is signed in and a peer is on the mesh (deliberate: single-Mac installs unchanged). Link sign-in is an email code — owner-only.

- 2026-09-26 (quality loop, E2E runs): three engine-level facts worth keeping.
  (1) Q-85 — the 27B, after the last `</parameter>\n` of a tool call, greedily writes `}`/`]`/`!` (~5% on `</`),
      MTP or not; Rapid-MLX's qwen3_coder_xml parser kept it as argument text (51% of calls failed on the Studio in
      E2E #2b; mlx_lm on the split drops it). Fix = constrain decoding to what the chat template allows at three
      points in a tool call (fork lz/xml-param-residue → lz.6). `qwen3_xml` in this fork is the JSON parser — wrong wire.
  (2) Q-103 — lz.5 admits ONE running request for its whole life (MTP verifier), prefill included: a 1-token request
      waits 256–428 s behind 17k prompts. Fork lz/fair-prefill keeps admission open until a request decodes alone.
  (3) Q-106 — several 27B engines loading at once under memory pressure (swap 47.5/49 GB) WEDGED the MacBook GPU
      (Metal hangs uninterruptibly; reboot only). One engine load at a time per Mac.
- 2026-09-26 Q-85 CLOSED: Rapid-MLX v0.14.3-lz.7 (eb9ed506d = lz.6 + XML tool-call skeleton guard) is pinned. Proof on the
  Studio: guard off 3/3 junk on the replay and 17/46 in a warm census → guard on 0 junk in 153 calls. The model escaped the
  format at FOUR different points across rounds (after `</parameter>\n`, right after `</parameter>`, after `</function>` →
  an open call and 28 looping calls); the guard now pins all of them. Fork tags so far: lz.5 (32768 cap fix), lz.6
  (transient tail may end a tool message), lz.7 (the guard). Never create a tag another agent already pushed.
