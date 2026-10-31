# Engine features: KV compression, MTP logprobs, warm vs cold, placement

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Contents
- KV-cache compression
- MTP logprobs
- Warm vs cold, and what a census actually measures
- Placement planner + "Measure speed"
- New chat, cold first token: what the cross-chat prefix cache actually reuses (Q-524)

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

## New chat, cold first token: what the cross-chat prefix cache reuses (Q-524, 2026-10-31, Gabee, Qwen3.5-9B, lz.19)
- goose's prefix is byte-identical across new chats: measured `LCP snapped to checkpoint: shared=21860 entry_len=34055
  resumed_at=16384 remaining=5917` (engine log, two chats in two different work dirs). Tools sorted (reply_parts.rs),
  extensions sorted, time/cwd live in the per-turn turn-context block, hints/project text at the very end. Since Q-524 every
  extension's folder-specific part (project skills, project agents/recipes, project memories, the $HOME note;
  McpClientTrait::get_project_instructions, MCP servers via the goose/projectInstructions fromByte capability)
  renders after ALL shared text, so a chat in another PROJECT also shares the bulk.
- Qwen3.5/3.8 templates render the TOOLS FIRST (~6k tokens), then goose's system text (~15.7k); token map tool:
  apply_chat_template + offset_mapping on a `llm_request.N.jsonl` (goose logs keep the last 10 requests only).
- The loss is the ENGINE's: a hybrid (GatedDeltaNet) entry is reusable only at a recorded recurrent checkpoint
  (rapid_mlx/hybrid_state_checkpoints.py: max 4, stride 2048, thinned by smallest gap, newest kept), so a 21.9k shared frame
  snaps to 16384 or 20480. And the LCP fetch (memory_cache.py ~2351) compares only the two sorted neighbours and snaps the
  longest-LCP entry, not the one with the deepest checkpoint. Fix lives in the fork: a checkpoint at the session frame
  (`_session_hint` frame, batched.py ~3705) and/or pick the deepest snap among equal-LCP candidates.
- First chat after a model's FIRST engine start is fully cold (`[cache_persist] no index ... nothing to load`): ~35 s for
  22k tokens on a 9B 4-bit on Gabee. The cache is persisted only at a graceful shutdown (server.py ~406) and autoloaded at boot.
