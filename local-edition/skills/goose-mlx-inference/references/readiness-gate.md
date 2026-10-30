# MODEL READINESS GATE (full text)

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Contents
- MODEL READINESS GATE

## MODEL READINESS GATE — use every model to the fullest on our machines (MANDATORY; owner 2026-10-29)
Run this BEFORE any local benchmark (Gauntlet / Forge) AND before putting a model into use (a new default, a swarm
role, a published profile, an article). Owner: *"always check and ensure that the context is being taken care of
before doing a local benchmark or preparing a model for usage ... use this same process we did now to always ensure
we're using our models with our machinery to the fullest"*.
WHY (2026-10-27/29, each step cost a 3-6 h run or a public number):
- MiMo ran at goose's default 128k although the split had been proven to 275k (0.5894 vs cloud 0.7909).
- An 8 GB "headroom" invented by an agent was reported to the owner as his rule.
- Qwen3.8-Flash-Next ran the Gauntlet BLIND. The build (`rapid-mlx/...-4bit`) said "text-only, vision tower not enabled",
  the engine pinned its alias text-only, and goose forced `--text-only`. It was published at 0.0065, then hidden.
- That build was also the leanest quant in existence: only the router gates were at 8-bit.
- Nobody had checked that the open model reaches 1M with YaRN while our engine ignored rope_scaling.
Write the answers into the run's NOTES.md / parity file. A step that cannot pass is a CAVEAT next to the score, never silence.

1. **Is this the right build?** (research first; record a row in the topic's RESEARCH-LEDGER)
   - Provenance: who converted it, from which upstream repo, and on what date. Pin the HF commit.
   - Alternatives: list every MLX build (official, mlx-community, Jundot/oMLX, unsloth, lmstudio-community). Compare
     downloads/likes, the quant recipe (are the sensitive layers at 8-bit or bf16: router/gates, shared experts,
     linear-attention/GDN projections, embed, lm_head, MTP head), imatrix calibration, and reported problems. Read the
     card's own "Status" line: "experimental", "text-only" and "not enabled" are red flags.
   - Tokenizer, chat_template and generation_config must be byte-identical to the official repo. Sample a few norm
     tensors against the official bf16 (catches the "+1 folded twice" class of bug).
   - Verify every file's sha256 against HF after the download.
   - Pick the best build that keeps everything below. Delete the loser by exact path (the drive is not a backup).
2. **Does it keep every capability the model has, and do WE serve them?**
   - From config.json + the safetensors index: modalities (vision_config / audio / video plus their tower tensors),
     MTP head (mtp.safetensors or in-shard), PLE/n-gram tables, thinking.
   - Then prove each one END TO END on our engine, not from the card:
     - `/v1/models` advertises vision under the served name;
     - a real image gets a correct description;
     - the MTP and prompt-lookup counters move in `/v1/status`;
     - PLE offload shows in Metal active memory.
   - `--text-only` is a FAIL for any model with a tower, unless the engine truly cannot serve it on the fast lane. Then
     fix the engine (lz.16-ple.2+ shows the pattern: TEXT_LANE_VISION_FAMILIES).
3. **Context: the largest the model AND the machine can actually hold.**
   - Native `max_position_embeddings`, plus any rope_scaling / YaRN in the official card. Does our engine implement it
     (grep the model file)? Cloud Qwen3.8-Flash = 1M via YaRN. Open weights are 262k native.
     Flash-Next YaRN: fork tag v0.14.3-lz.16-ple.5+, opt-in `--qwen4-rope-scaling yarn` (env
     RAPID_MLX_QWEN4_ROPE_SCALING; `yarn:<f>`, JSON, `off`). Without it the engine still ADMITS >262k (plain
     extrapolation; 300k needle passed both ways), so "it accepted the prompt" proves nothing. Evidence
     /Volumes/AI-workhorse/experiments/2026-10-29-qwen4-yarn/NOTES.md (2026-10-29).
     IN GOOSE (flash-pld-bench 707900523d, 2026-10-29): model profile key `rope_scaling: yarn` -> argv
     `--qwen4-rope-scaling yarn` (qwen4_exp only, typo/other family fails the mount; rides the ACP DTO and the
     desktop drafts, no form row). Pair it with `context_limit` + `GOOSE_CONTEXT_LIMIT` 409,600 on the mule.
     Live check: /v1/models `context_window` 1,048,576 (max_model_len still prints 262,144).
   - FIXED (2026-10-30, fork v0.14.3-lz.18): Flash-Next (qwen4_exp) prompt lookup was GREEDY-ONLY
     (enabled_under_sampling=False, env disable-only), so at temperature 1.0 (every Gauntlet/agent run) n-gram drafted
     NOTHING (0 proposals). lz.18 sets it True: the verify path is exact speculative sampling for a point-mass copy
     (accept p(d), residual = p minus d), proven lossless by tests/test_qwen4_pld_sampled.py. Measured temp 1.0
     code_edit 8k on workmule: 99.4% accepted, 1.6x decode, 16/16 exact. Engines <= lz.17 / ple.6 still draft nothing
     when sampled. Disable the sampled route only: RAPID_MLX_MTP_PROMPT_LOOKUP_SAMPLED=0.
     Evidence: /Volumes/AI-workhorse/experiments/2026-10-30-qwen4-pld-sampled/NOTES.md.
   - TRAP (2026-10-29): the MEMORY GUARD stays on its PRIOR (192 B/(chunk*ctx)) for a whole agent run: agent prefills
     are short cache-hit increments, never the >=25%-span pairs its fit needs. On the mule the prior caps Flash at
     262,144 at idle and REFUSED 241,320 mid-Gauntlet (goose compacted there, 409,600 configured). FIX before pressing
     Run: on the freshly mounted engine send one cold ~135k-token prefill (max_tokens 1; scratchpad cal400.py style,
     random words ~3.4 tokens/word) -> /v1/status memory_guard.peak_model.source "calibrated" (a ~5-7 B/unit); then
     a cold ~390k prefill admits (peak 96.5 GB). Calibration is process-local: an engine restart loses it.
     SUPERSEDED by v0.14.3-lz.16-ple.6 (fork 46a7848bb, 2026-10-30): no cal400 step needed any more. Hit steps enter the
     fit (copy measured), flat full-chunk groups vote a=0, a turn that fits alone re-prefills cold instead of being
     refused (cold_requeues). STORE-CEILING FIX (2026-10-30): a mid-turn boundary copy counted as OPTIONAL memory (machine
     ceiling = physical - "others" - 8 %; on the idle mule "others" reads ~28 GB, mostly kernel wired the engine itself
     causes, so 96.9-97.8 GB vs device 103.9) because "the turn end hands the boundary over" -- but
     _memguard_keep_boundary_state cannot keep qwen4_exp's layout, so NOTHING was stored and the hit froze at 200,785
     (re-prefill 14-24k/turn, TTFT 88-162 s). Now a non-keepable own-prompt-prefix copy plans under the device ceiling
     and evicts the own older resume point it supersedes (_memguard_boundary_keepable; test_memory_guard_store_ceiling.py).
     Replay (runs/2026-10-29-flash-vision/reclaim/rep7): 0 skips/refusals to 378k, hits advance to 371k, peak 103.78 GB;
     chunks 2048 only to ~200k (then 480-790, 256 floor past 289k); past ~371k cold requeue every turn (1,560 s) ->
     Flash on the mule: GOOSE_CONTEXT_LIMIT 360,000. The "others" over-count is still OPEN (it only lowers optional
     memory now: the prefix budget).
     RESULT 2026-10-30: Gauntlet sb-7.2 on ple.6 at 360k: 0 refusals/skips/cold requeues, hits advance past 200k,
     goose compacted at ~290k; 0.3927 (brun-25b5557e) vs cloud 0.6846; ended on the 150-call budget.
   - The cloud's real need: peak prompt_tokens in the cloud run's telemetry (Gauntlet sb-7.2 peaked at 322,851).
   - KV arithmetic from config.json: full-attention layers × KV heads × (k_dim+v_dim) × bytes. Examples:
     Flash-Next 24.6 KB/token; MiMo 23 KB/token. Linear/SWA layers are constant.
   - Measure, don't predict. Run a needle + doc_quote at stepped lengths (e.g. 128k / 262k / 300k / 400k / 512k) on
     the target Mac. Record TTFT, peak Metal GB and swap per step; stop a step before swap grows.
   - Operating point = the largest step whose MEASURED peak stays clear of the Metal ceiling. Ceilings: workmule
     ~115 GB, workhorse ~77 GB, laptop ~115 GB, Gabee ~48 GB. Example 2026-10-29: Flash 400k peaked 96 GB on the mule
     = production; 512k at 114.7 GB = "fits, at the ceiling", not used for a 5 h run.
   - MEASURE WITH THE APP'S EXACT ARGV: prefix cache on, `--hybrid-cache-entries 24` (the app passes NO
     `--cache-memory-percent` on a Guarded pin: engine_profile.rs; the guard derives the budget),
     `--max-concurrent-requests 8`. Run a multi-turn session that GROWS to the target, not one cold request. Then set
     GOOSE_CONTEXT_LIMIT to what the memory guard actually admits. Why, 2026-10-29: the YaRN proof
     (`--disable-prefix-cache`, cold requests) showed 400k at 96 GB. In the Gauntlet the same model refused 241,320
     tokens ("needs 104.8 GB, this Mac can hold 103.9 GB"; footprint 82 GB with the prefix cache), and goose compacted
     at 59 % of the 409,600 it had been given. model-limits.json said `provider_default_unverified`.
   - Every margin names its origin (owner rule vs mine).
   - Never leave goose's default context (128k) on a model proven for more.
4. **Speed levers on, and proven lossless:** MTP, prompt lookup (n-gram), PLE offload, prefix cache.
   - A/B each lever with greedy, at 8k and at the operating context. code_edit must be byte-identical. A lever that
     changes greedy output, or does not speed anything up, stays OFF and gets a ledger line (see MiMo MTP on the split,
     dropped 2026-10-29).
   - Streamed output must equal the non-streamed bytes (the lz.16 whitespace bug corrupted every streamed file write
     before 2026-10-29).
5. **Machinery:** pick the Mac or split from the measured peak in step 3 (node-probe headroom; Studios first). One Goose
   app per Mac. The model copy sits on that Mac's internal disk or on the HARD NFS mount.
   **Memory tunables (engine >= v0.14.3-lz.20; set on purpose, record in parity, verify via /v1/status):**
   a. Device spec check FIRST: `sysctl hw.memsize iogpu.wired_limit_mb`; the MLX working set
      (`/v1/status memory_budget.device_working_set_bytes`); `memory_pressure`. Fleet (2026-10-30): mule/laptop
      137.44 GB physical, working set 115.45 GB; workhorse 103.08 / 83.49; Gabee 68.72 / 55.66; wired limit 0 everywhere.
   b. Cap layers, outermost first: physical RAM -> iogpu working set (wired limit) -> engine cap (auto 0.90..0.97 of the
      working set from weights + context plan, or --metal-limit-gb) -> floor room = physical - others - floor -> guard
      margin. `memory_budget.step_binding` names the layer that binds. Flag RAM the plan leaves unused.
   c. What the floor counts as "others" (`memory_budget.machine`): wired + anonymous - purgeable + compressor, less the
      engine's Metal memory. NOT the file cache (file-backed pages, incl. the qwen4 PLE shards the offload memmaps):
      macOS frees it. The engine's own memory outside Metal (Python, token buffers) IS counted
      (`others_engine_non_metal_bytes`), because it is really held. Mule mid-Gauntlet 2026-10-30: used 119.5 GB =
      wired 102.0 + anonymous 16.5 + compressor 1.4, file cache 19.5 GB left out, others 22 GB.
   d. The tunables (Settings > LeanZero MLX > Memory; config key `mlx_engine.memory.<key>` -> engine flag):
      | key | flag | default |
      |---|---|---|
      | metal_limit_gb | --metal-limit-gb | blank = auto (context-aware) |
      | memory_floor_gb | --memory-floor-gb | round(5% x physical GB): mule 7, workhorse 5, Gabee 3 |
      | prefix_cache_gb | --cache-memory-mb (GB x 1e9 / 2^20) | engine auto |
      | prefill_step_size | --prefill-step-size | engine 2048 |
      | max_context_tokens | --max-model-len | profile context_limit, else none |
      | guard_margin_gb | --memory-guard-margin-gb | auto 2% of ceiling |
      | max_concurrent_requests | --max-concurrent-requests | goose 8 |
      | auto_utilization_floor / _ceiling | --auto-utilization-floor / -ceiling | 0.90 / 0.97 |
      | auto_headroom_fraction | --auto-headroom-fraction | 0.08 |
      | system_reserve_fraction | --system-reserve-fraction | 0.08 |
      | (macOS) | `sudo sysctl iogpu.wired_limit_mb=N` via the tab's button (admin password; optional LaunchDaemon) | 0 |
      lz.20-only flags are sent only when ENGINE_LAUNCHER pins >= v0.14.3-lz.20; a set one on an older pin fails the
      mount by name.
   e. Procedure: read the spec (a); set EVERY tunable on purpose (blank = the default, chosen knowingly); record the
      values in the parity file (`python3 ~/goose-builds/loop-state/memory_tunables_gate.py emit --engine-port <p>`,
      15 rows; bench launchers exit 6 without them); after mount verify `/v1/status memory_budget.tunables`,
      `limit_bytes`, `effective_ceiling_bytes`, `max_context_at_ceiling` match what was set.
6. **Benchmark only:** print the parity table vs the cloud entrant (context, vision, thinking actually sent, sampling,
   tool parser, output cap ≥ the cloud's longest reply, timeouts) and get every row PASS before pressing Run. See
   goose-benchmark-iteration "LOCAL-MODEL RUNS". After publishing, re-open the run page and check that the caveats are
   on it.
