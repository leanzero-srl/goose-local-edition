# CHANGELOG: dated record (newest entries at the end)

## Contents
- RC13 CONTROL, FIRST FULL LOCAL FORGE 2.0 BUILD WITH NO ENGINE INCIDENT (2026-10-17, Gabee 64 GB, Qwe
- COMPACTION V2 LIVE + LIVE WINDOW + V3 (2026-10-17, Gabee, Qwen3.8-27B Q8-base, engine RC13):
- LIQUID LFM2.5 24B-A2B LOCAL (2026-10-17, engine pin v0.14.3-lz.11 = lz.10 + fork c56728690, goose 48
- NEMOTRON-H LOCAL (NVIDIA-Nemotron-3.5-Super-VL-120B-A12B-MLX-4bit, 2026-10-18; fork lz/nemotron-h-me
- MUSE-GLIMMER-30B-8bit LOCAL (2026-10-18, Gabee, RC13 e3a92b555 standalone, --text-only): NO engine f
- XING4.0 LOCAL (XingChen-AGI/Xing4.0-29B-A4B, xing4_0, Apache-2.0, 31.2B total / ~4B active; 2026-10-
- 2026-10-18 (night, laptop session) — Xing 4.0 live in the STUDIO APP on an unreleased engine (pin-by
- 2026-10-18 (night) — GEMMA 4 PREFIX-CACHE MISS: 12 % reuse, two causes, fixed on lz/rotating-boundar
- 2026-10-18 23:2x audit — reading the engine's own counters during a live run (GET /v1/status only): 
- 2026-10-18 (late night) — MEMORY GUARD OVER-SIZED LATENT ATTENTION (MLA) 12.4x: Xing capped at 35,32
- 2026-10-19 (audit #3) — Gemma 4 still ~10 % reuse on 87e71b18: the NEXT-TURN check was too strict (f
- 2026-10-19 (night, audit #4) — read Gemma 4 speed from the engine log, not the call count. On Gabee 
- 2026-10-19 (01:30-02:30, fork) — D-METAL-CAP OVER-PRICED GEMMA 4 KV 8x AND MASKED ITS REFUSAL (RUN-I
- 2026-10-19 (03:40) — GRANITE 4.2 30B: stale granite[-_]?4 profile (RUN-ISSUES I-23). Engine b20ec1b7
- 2026-10-19 — the three Macs do NOT run the same goose. Gabee's app is a t12-line build that sends
- 2026-10-19 — Nex-N2.5-mini (qwen3_5_moe, abenzerps folder) gets NO parsers from auto-detect (RUN-ISS
- 2026-10-19 — NEMOTRON 3.5 BOLT (nemotron_h_puzzle) PORTED + the guard's KV over-prices (RUN-ISSUES I
- 2026-10-19 (audit #9) — THE ENGINE DOUBLED HISTORY THINK BLOCKS on a template that renders them on e
- 2026-10-19 — Providers > Models delete refused SSD-linked models ("refusing to delete '<id>': /Volum
- 2026-10-19 — RELEASE 3.0.119: engine pin v0.14.3-lz.12, overnight wrappers RETIRED. ENGINE_LAUNCHER 
- 2026-10-20 — ENGINE PIN v0.14.3-lz.13 on branch lz/backlog-1019 (not released). Tag object 3bab1d05 
- 2026-10-22 — RECURSIVE FLINT LOCAL (RecursiveSI/flint, flint / FlintForCausalLM, Apache-2.0, 39.8B t
- 2026-10-23 — DENSE 27B AT 4 tok/s ON THE LAPTOP = Q-447 ON THE SINGLE ENGINE (float32 promotion; for
- 2026-10-23 — RELEASE v0.14.3-lz.14 (fork lz/release-lz14 03fa3b38ff4bbf42642d1e3448b1fffce90c4b09 = 
- 2026-10-30 — LONG RUNS (>30 min) section added near the top (owner rule in CLAUDE.md): context proof
- 2026-10-29 — qwen4_exp VISION WIRED ON THE TEXT LANE: tag v0.14.3-lz.16-ple.2 (fork lz/qwen4-vision-
- 2026-10-29 — STREAMING WHITESPACE BUG FIXED + LONGCAT TOOL PARSER: tags v0.14.3-lz.16 (= lz.15 + fix
- MELLUM LOCAL (JetBrains/Mellum2.1-12B-A2.5B-Thinking, mellum, Apache-2.0, 12B total / ~2.5B active; 
- 2026-10-30 — skills audit restructure: SKILL.md became a ~130-line CORE card; every section moved ve


Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

- RC13 CONTROL, FIRST FULL LOCAL FORGE 2.0 BUILD WITH NO ENGINE INCIDENT (2026-10-17, Gabee 64 GB, Qwen3.8-27B Q8-base,
  engine e3a92b555): build 1 scored 0.1342. That is the best local Forge score so far (previous best was JetBrains-blend at 0.0938).
  Inner score 0.664, cut by a x0.6 critical (r6_secret_never_disclosed) and by reliability 0.337. CORRECTION 2026-10-17: that
  critical is a PROBE FALSE POSITIVE, not a model leak: forge2_probe.mjs shownSecret took the admin accountId (36 chars,
  first shown after the rotate) for the secret; the real secret occurs only in kvs secret/set and the rotate response.
  Without it the build is at least 0.2237 (details + fix design: goose-benchmark-iteration SKILL.md changelog 2026-10-17). 299 of 300 calls, 7.8 h, OOM 0, and the memory guard took no harmful action. Peak prompt 122k.
  Model-side trap seen: it spent about 1.4 h in `sleep 480` chains waiting for the simulator's quota hour to roll so it could re-test
  the backfill, and it read local time as UTC. Count that wait separately when comparing candidates' wall clock. The engine
  sits idle the whole time, so it is not serving cost.
- COMPACTION V2 LIVE + LIVE WINDOW + V3 (2026-10-17, Gabee, Qwen3.8-27B Q8-base, engine RC13):
  - The window probe read the memory guard's `max_model_len` ONCE per process. Reading it at app launch under load
    gave 117,760, while the engine later served 130-161k, so every compaction fired early. Fixed in df5d65d03
    (t12/combined, also on t12/local-bench-fidelity): a guard-derived window is re-read every 60 s
    (LIVE_WINDOW_REFRESH); a missed re-read keeps the last window and waits a refresh. Live trace: 117,760 -> 137,984
    -> 161,014 within one run.
  - V2 C1 (series 8795392e, at call 61, 105k -> 61k): the summary held every needed fact, yet the model re-researched
    for 29+ calls with nothing written. It cited the summary only at about call 78. Request order was [summary][cont]
    [working set 135k ch][task]. The six node_modules .d.ts it had read were "not shown", and those are the APIs it
    re-read. V3 (t12/compaction-v3 76dc2744f): working set BEFORE the summary; dependency files read whole fill
    the budget left after the chat's own files.
  - A ONE-CALL REPLAY CANNOT SEPARATE PROMPT ORDERS here: every sample's first action was the summary's own
    "next step" (research). The defect lives in the ~30 calls after that, so measure V3 live (calls and minutes from
    compaction to first write). Single-call timings vary 6x run to run (1,696 s vs 279 s for the same request).
  - The "turn N/M used" line already exists (moim.rs turn_budget_line, from 50% of max_turns). The model saw
    299/300 and still made a breaking last edit, so this is not a goose gap.
  - V3 MEASURED (series d1f23cea, 2 builds, frozen forge-2.0 scorer): 0.2576 / 0.1128, mean 0.185 vs the old-goose
    control 0.140; inner 0.708 / 0.771 vs 0.664. After every compaction the next call or the one after was a write
    (V2 C1 went 29+ calls without one). b2's low final score is two R6 security criticals in its 75-line web-trigger
    (an unsigned request still persisted the rate dose to KVS; a check-then-set eventId guard let a concurrent replay
    apply twice). That is model variance in one file, not compaction. b2 first read 0.2736 unpublishable because the
    scorer's probe crashed (RangeError on a spread; fixed d97ab1fab on fix/forge2-probe-spread) and the crash had
    hidden those criticals.
- LIQUID LFM2.5 24B-A2B LOCAL (2026-10-17, engine pin v0.14.3-lz.11 = lz.10 + fork c56728690, goose 48ef631b8, app 3.0.117):
  - Aliases match only hub names/paths, so a FOLDER mount of any LFM got no parser: calls came back as content
    `[shell(command='ls /tmp')]`. lz.11 detects `model_type lfm2*` + a `<|tool_call_start|>` template → tool `lfm`,
    reasoning `deepseek_r1_distill` (LFM2.5 opens `<think>` in EVERY generation prompt and has no enable_thinking
    switch; `qwen3` streamed the thought as content whenever R12-T1F auto-disabled thinking for tools).
  - TAG RULE: the fork's pipeline audit (tests/test_pipeline_single_line_ports_audit.py) accepts only
    `v<x>.<y>.<z>-lz.<N>` integers; a `lz.10.1` tag fails it. lz.11 is now LFM; the memory guard release takes lz.12.
    A pin bump also moves `PIPELINE_SINGLE_LINE_REVIEWED_THROUGH` with an n/a entry on the pipeline line (331d6231d).
  - PLANNER GAP: "Could not plan — config.json declares 64 experts but no tensor has a leading dimension of 64" for
    lfm2_moe (stacked layout); starting still works. Not fixed.
  - Config for a local Gauntlet: mlx_engine.model_id = <org>/<dir>, served_model_name null (the old alias would name
    the new model), OPENAI_HOST http://127.0.0.1:8090, provider_default_models.openai = the served id; edit with the
    app QUIT. The engine-glance pill appears once the engine mounts: loop-state tools/mainpage.mjs picks the largest page.
- NEMOTRON-H LOCAL (NVIDIA-Nemotron-3.5-Super-VL-120B-A12B-MLX-4bit, 2026-10-18; fork `lz/nemotron-h-metadata` 8af0af611
  on lz.11, port `lz/nemotron-h-metadata-rc13` 9828cffa8 on RC13 e3a92b555; NOT in any pin yet):
  - Symptom: goose's connection check said "no visible response". Streaming content read `<think>The user wants...OK` +
    `OK` with no reasoning split; with tools the call came back as raw `<tool_call><function=write>...` CONTENT, 0 parsed.
  - Cause: `detect_model_config(<folder>)` returned None (no profile at all). Its template shows the tool wire only as an
    EXAMPLE string inside the instructions and lists tools as `<function><name>` blocks, so
    `_template_uses_parameterized_xml_tools` is False; nemotron_h declares no linear attention; no alias names a local
    folder. With no reasoning parser, `strip_special_tokens` dropped the non-special `</think>` (id 13) and
    `set_thinking_model` (name contains "nemotron", no parser) prefixed a lone `<think>`. The OutputRouter's "Think tag
    format detected" log line is a red herring: only gemma4/harmony routers are allowlisted (`_OUTPUT_ROUTER_ALLOWLIST`).
    Renaming served_model_name changes nothing.
  - Fix (metadata fallback): nemotron_h -> hybrid, no spec decode, MoE when `n_routed_experts`; tool `nemotron` when
    the template has `<tool_call>` + `<function=` + `<parameter=`; reasoning `qwen3` when it primes `<think>` behind
    `enable_thinking` (the nemotron-3.5-lightning alias's choice). Boot log: `Auto-detected checkpoint metadata ... →
    tool_call_parser=nemotron, reasoning_parser=qwen3, is_hybrid=True`.
  - Measured live on the workhorse (standalone engine, lz.11 env + the worktree on PYTHONPATH, app argv, port 8196,
    weights 69.5 GB under the 83.5 GB Metal ceiling): streaming text "OK" clean; 3/3 write calls parsed without effort
    (2.6-3.1 s each) and 3/3 at reasoning_effort medium with 1,030-2,142 reasoning chars split out; non-streaming call
    parsed. Tests: 2 RED on lz.11, 354 passed with the fix.
  - Smoke tool: `smoke.py <port> <model> [--effort medium]` (streams like goose; TEXT + 3 write calls, flags leaked
    `<think>`/`<tool_call>` in content). Copy in the session scratchpad; it is also at /tmp/smoke.py on workhorse/Gabee.
  - Release step (workhorse session): tag the lz.11-line commit as the next `v0.14.3-lz.<N>` (or fold the RC13-line
    port into the memory-guard release), bump ENGINE_LAUNCHER + SUPERSEDED_ENGINE_LAUNCHERS + the pipeline
    `reviewed_through` n/a entry (see the LFM entry above), then remount Nemotron in the app and re-run the
    connection check.
- MUSE-GLIMMER-30B-8bit LOCAL (2026-10-18, Gabee, RC13 e3a92b555 standalone, `--text-only`): NO engine fix needed. Auto
  family detection `muse[-_]?glimmer` -> tool `muse`, reasoning `muse`; text-only lane forced. Streaming smoke: text
  "OK" with 288-364 reasoning chars split; at reasoning_effort medium 3/3 write calls (51-113 s each, 1,638-4,564
  reasoning chars); without effort 2/3 (the third chose `shell mkdir -p src` first, a well-formed call). lz.11 carries
  the same muse files (models/muse_glimmer.py, muse parsers). DFlash still needs meta-models/Muse-Glimmer-30B-assistant
  (not on disk); without it the engine serves plain decode.
- XING4.0 LOCAL (XingChen-AGI/Xing4.0-29B-A4B, `xing4_0`, Apache-2.0, 31.2B total / ~4B active; 2026-10-18; fork
  `lz/xing4-rc13` 1c513d179e17af804901012fa14c9bc6109edc96 on the Nemotron RC13 port 9828cffa8, so ONE pin carries
  RC13 + Nemotron + Xing; pushed, NOT released, NOT real-model smoked yet):
  - Why it could not load: stock mlx-lm (0.31.3 pinned, 0.32.2 latest) has no `xing4_0` (`ValueError: Model type
    xing4_0 not supported`); support is the OPEN mlx-lm PR #1901 (TokenAI-zer). Architecture = DeepSeek-V3 MLA
    attention (latent KV cache, ~46 KB/token across 40 layers) + noaux_tc MoE (64 routed + 1 shared, 4/token, sigmoid,
    routed scale 2.0) + manifold-constrained HYPER-CONNECTIONS (4 residual streams, Sinkhorn-mixed around every
    sublayer, mean over streams at the end). Not hybrid, MoE.
  - Port: `rapid_mlx/models/xing4_0.py` = PR #1901 verbatim (two imports made absolute) + `Fp32RouterGate` (the
    reference routes in float32; mlx-lm's MoEGate in bf16 flips near-ties). Registered in `_register_vendored_archs`.
  - TOKENIZER TRAP: no Xing checkpoint (bf16 or MLX) ships `tokenizer.json`; only SentencePiece `tokenizer.model` behind
    a remote-code `Xing4_0Tokenizer` (auto_map). Never run checkpoint code: the fork vendors that class
    (`rapid_mlx/models/xing4_0_tokenizer.py`) and `_load_with_tokenizer_fallback` loads it when the folder is xing4_0 +
    tokenizer.model. Test plants a raising `tokenization_xing4_0.py` to prove it never runs.
  - TOOL WIRE: `<tool_call>NAME<param_key>K</param_key><param_value>V</param_value>...</tool_call>` (one line, string
    values RAW, others JSON). Parser `xing4` (aliases xing4_0, xing), a Glm47ToolParser subclass with its own scan: a
    value ends at a `</param_value>` followed by the next `<param_key>` or `</tool_call>`; string-schema values
    byte-identical (the GLM parser strips + json.loads, which would cut a file's trailing newline). `<tool_call>` and
    `</tool_call>` are SPECIAL tokens (arrive whole in a stream). The multi-format fallback scanner cannot read this
    wire, so the parser MUST be selected: auto-config does it for any local `xing4_0` folder (is_moe, tool xing4,
    reasoning qwen3 because the template primes `<think>\n` behind enable_thinking and renders `</think>` when off).
  - Reference bug: the checkpoint's `modeling_xing4_0.py` `_init_weights` reads `module.fn` (attr is `hc_fn`) and
    raises on random init; a parity harness must bypass it (the weights it loads are fine).
  - Proof (workhorse, engine env lz.11 archive sERprzWJ… + pytest/torch in /tmp/xingref/pyext): full-model parity vs
    the reference (tiny random config, fp32, HF state dict -> sanitize -> strict load, 96 tokens) max |dlogit| 1.49e-06
    at scale 1.56, top-1 96/96 (`RAPID_MLX_XING_REFERENCE=<dir with modeling/configuration_xing4_0.py> pytest
    tests/test_xing4_0_model.py`). Xing suites 741 passed; wider tool/tokenizer/vendored set 7884 passed with the same
    9 fail + 4 error as base (llguidance/Gemma fixtures absent).
  - WEIGHTS: TokenAI-zer/Xing4.0-29B-A4B-8bit-MLX (the PR author's conversion, mlx-lm layout: stacked switch_mlp,
    absorbed embed_q/unembed_out, MTP layer dropped, affine 8-bit g64) at
    `/Volumes/AI-workhorse/models/goose/TokenAI-zer/Xing4.0-29B-A4B-8bit-MLX` (31 GB). Verified vs the bf16 original
    on 2 of 12 shards: 1,361 tensors, median rel max-error 0.7-0.8%, worst 1.6%; MLA absorption worst 1.5% over 19
    layers; expert-order negative control 78-98% off (scripts: workhorse ~/goose-builds/loop-state/engine-smoke/).
    nativ-community/...-MLX-8bit uses a `language_model.` prefix and would need remapping: do not use.
  - PENDING (needs a free 48 GB+ box, never beside a loaded model): standalone real-model smoke, then the app.
    Serve (workhorse, app argv; not a vision checkpoint so no --text-only):
    `uvx --from "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@1c513d179e17af804901012fa14c9bc6109edc96" rapid-mlx serve /Volumes/AI-workhorse/models/goose/TokenAI-zer/Xing4.0-29B-A4B-8bit-MLX --port 8198 --served-model-name xing-smoke --enable-prefix-cache --hybrid-cache-entries 24 --max-concurrent-requests 8`
    Boot log must say `tool_call_parser=xing4, reasoning_parser=qwen3`. Smoke:
    `python3 ~/goose-builds/loop-state/engine-smoke/smoke.py 8198 xing-smoke` and `... --effort medium` (TEXT clean, no
    leaked `<think>`/`<tool_call>` in content, 3/3 write calls). In the app: engine-wrap pin to the full sha above.
- **2026-10-18 (night, laptop session) — Xing 4.0 live in the STUDIO APP on an unreleased engine (pin-by-wrapper).** Studio
  ~/goose-builds/loop-state/engine-wrap-xing.sh (REV 1c513d17 = lz/xing4-rc13; drops --cache-memory-percent like Gabee's
  RC13 wrapper) set as mlx_engine.spawn_command with the app QUIT; model_id TokenAI-zer/Xing4.0-29B-A4B-8bit-MLX via the
  symlink ~/.goose/models/TokenAI-zer -> /Volumes/AI-workhorse/models/goose/TokenAI-zer; OPENAI_HOST http://127.0.0.1:8090
  added for the benchmark lane (Studio config backup config.yaml.before-nemotron-local-20261018). App mount: served with
  tool_call_parser xing4 / reasoning qwen3, engine argv shows Rapid-MLX@1c513d17. Smoke through the app's :8090: text PASS,
  tools 3/3 plain (6-9 s/call) and 3/3 at medium (14-73 s, 237-570 reasoning tokens). A Nemotron-4bit mount on the Studio was
  REFUSED by the app's memory gate (64.8 GB needed, 62.6 free of 96 GB with other sessions' work) — Nemotron goes to the laptop.
  APP PLANNER BUG (both MoE folders): "config.json declares 512 experts (Xing: 64) but no tensor has a leading dimension of
  512 — the routed weights could not be found, so the active bytes are unknown" -> no speed plan; Run still allowed. The
  planner's expert-tensor detector does not know nemotron_h's / xing4_0's routed-weight layout. Restore the Studio's config
  from the backup when the local runs end (the released engine is the default for its cloud chain).
- **2026-10-18 (night) — GEMMA 4 PREFIX-CACHE MISS: 12 % reuse, two causes, fixed on lz/rotating-boundary-state cc00328e
  (on lz/xing4-rc13 1c513d17, so ONE pin carries Nemotron + Xing + this).** Owner saw Gabee "barely hitting 3-4k context
  cache out of 30-40k". MEASURED (live gemma-4-31B-it 8-bit Forge run, goose request logs `cache_read_input_tokens`):
  a constant 3,265 cached of 15-30k on EVERY turn; every other local run on the same RC13 engine reuses 92-99 %
  (Glimmer 79,455/79,531, Qwen3.8-27B 98-99 %). Goose's messages are strictly append-only (same system, same tools) —
  NOT a client problem. CAUSE 1 (`engine/batched.py _compute_prefix_boundary`): after a `tool` message the Gemma 4
  template renders IDENTICALLY with and without add_generation_prompt (25,569 = 25,569 tokens); the strict-prefix check
  `stable_lcp < len(real)` rejected equality and the fallback dummies the LAST USER message = the original task in an
  agent loop, so the boundary was always system+tools+task (1,168 on the 12B, 3,265 on the 31B). Fix: accept the
  equality case with the 8-token replay margin. CAUSE 2 (`scheduler._memguard_keep_boundary_state`): with a correct
  boundary the 25-30k boundary COPY (3.7-4.4 GB) is still refused beside the live KV on 64 GB ("skipped storing a 4.42GB
  prefix entry ... ceiling 40-43 GB"), and only ArraysCache+KVCache layouts got the turn-end hand-over; a wrapped
  RotatingKVCache reports is_trimmable() False, so it ALSO read as "non-trimmable but not ArraysCache" and gave up, and
  the turn-end prompt+output entry is useless (the next prompt diverges at the reply; a wrapped ring cannot rewind).
  Fix: keep the ring layers' state at the boundary (window-bounded: 0.6-0.7 GB on the 12B at 25k) and hand the
  full-attention KV over at the turn end. PROOF: red/green tests (tests/test_memory_guard_rotating_boundary.py: tiny
  gemma3_text, window 16, real scheduler: cached 0 -> 48, answers == copied == cold; test_prefix_boundary_path_parity
  ::test_boundary_when_the_template_adds_no_generation_suffix 35 -> 317); 215 boundary/hand-over/transient tests pass
  (2 pre-existing HF-offline failures identical on the base). REPLAY of the live run's 10 recorded requests on the
  Studio (gemma-4-12B-it MLX 4-bit at /Volumes/AI-workhorse/models/goose/lmstudio-community/, own engine :8199, boundary
  copies refused past 4k tokens to mimic Gabee's pressure, same hook on both engines): base 1c513d17 = 1,168 cached
  every turn (3-4 %), 39-56 s/request; cc00328e = 93-99 % from turn 2, 1.3-10.6 s/request. Logs /private/tmp/replay-*.log
  on the workhorse. DEPLOY: engine-wrap REV=cc00328e439ffb20e98a50321bf9e36dfc06bc50 (Gabee: ~/t12-bench/engine-wrap.sh,
  keeps its --cache-memory-percent filter; Studio: engine-wrap-xing.sh) with the app QUIT between series — NOT mid-run.
  Release = workhorse session (tag + ENGINE_LAUNCHER bump). Also check other no-generation-suffix templates (any model
  whose render after a tool message equals the generation prompt) — the boundary fix helps them all.
- **2026-10-18 23:2x audit — reading the engine's own counters during a live run (GET /v1/status only):** `cache.hit_rate`,
  `cache.tokens_saved` vs `total_prompt_tokens`, `memory_guard.actions.boundary_states_kept/boundary_handoffs` (the Gemma-4 fix
  cc00328e working: 13/13), `busy_machine_evictions`, `machine_prefix_budget_mb` and `memory_guard.others_bytes`. others_bytes =
  wired + anonymous - purgeable + compressor (file cache NOT counted) with a rise-at-once / fall-only-without-swapouts
  hysteresis — it is real pressure, not a bug: compare with vm_stat before blaming the guard. A miss to read: the released
  app's context probe wants meta.n_ctx or loaded_context_length, the RC13-line engine lists `context_window` (RUN-ISSUES I-12).
- **2026-10-18 (late night) — MEMORY GUARD OVER-SIZED LATENT ATTENTION (MLA) 12.4x: Xing capped at 35,328 tokens (RUN-ISSUES
  I-7).** The guard/admission KV figure is the uniform `2 x num_key_value_heads x head_dim x dtype x layers`; for Xing4.0
  (32 kv heads, head_dim 3584/32=112, 40 layers, bf16) = 573,440 B/token, one-layer copy 14,336. mlx-lm's ABSORBED MLA
  (`DeepseekV3Attention`, reused by xing4_0; also deepseek_v32, glm4_moe_lite, longcat_flash) caches ONE head:
  `cache.update_and_fetch(kv_latent[kv_lora_rank], k_pe[qk_rope_head_dim])` = (512+64) x 2 B x 40 = 46,080 B/token. Result
  on the 96 GB Studio: "82684 tokens needs 131.8 GB, this Mac can hold 75.1 GB, maximum context 35328" -> goose's recovery
  compaction could not fit the Forge task and the session ended at 43 calls (score 0.0617, invalid). mlx-lm deepseek_v2 has
  kv_lora_rank but caches full per-head K/V (no embed_q) — never key a latent rule on the config alone.
  FIX lz/xing-guard-kv 87e71b18 (on cc00328e): `kv_estimation.latent_attention_bytes_per_layer(model, dtype)` from the loaded
  modules (embed_q + int kv_lora_rank + qk_rope_head_dim on EVERY layer, else None); `Scheduler._mla_latent_layer_bytes()`
  overrides per-token growth (baseline/sliding 0) and the guard prior's layer copy. Test
  tests/test_kv_estimation_mla_latent.py (tiny Xing: base 1,536 vs measured 288 B/token -> 288). Patched engine on the real
  weights: guard prior kv_copy 1,152 B/context-token, context_window 262,144. Run the fork's guard tests with the engine env
  (archive sERprzWJ…) + `hypothesis` (test_memory_guard.py imports it; uv pip install --target /tmp/xingref/pyext).
  RULE learned: never start a real-model check on a Mac with a live `run_build.py` (the coordinator had to kill one: the Studio
  dropped to 11 % free + 9.5 GB swap during a Gemma 26B run).
- **2026-10-19 (audit #3) — Gemma 4 still ~10 % reuse on 87e71b18: the NEXT-TURN check was too strict (fixed lz/gemma4-boundary-tail
  1b5f497a5).** Live Gabee 31B run: boundary snapshots stuck at 3,491 of 37.6k (guard: boundary_states_kept 0, handoffs 0).
  Offline emulation of `_compute_prefix_boundary` with the model's own tokenizer (engine env uv archive B2_VXO2n1vVyID1R,
  transformers 5.15.1, apply_chat_template on the request's messages): real == stable = 37,469, but the next-turn probe shares
  only 37,466 — appending a message RE-TOKENISES the last 3 tokens. The code required next_turn_lcp >= len(stable_tokens) and
  fell back to the last-user dummy. Fix: require next_turn_lcp >= stable_boundary (stable - 8 replay tokens). Test
  test_prefix_boundary_path_parity::test_boundary_when_the_next_turn_retokenises_the_tail (red before, green after; file 17/17;
  the 6 boundary/transient files 69 pass; run with ~/Projects/Rapid-MLX/.venv/bin/python, PYTHONPATH=<worktree>, nice 19 — the
  uv engine env has no pytest). Replays that reuse the probe render with the full tail CANNOT see this; emulate with a real
  appended message. Not deployed: needs engine-wrap REV=1b5f497a5… between series, then re-check the guard counters (on 64 GB
  the 5.0 GB prompt-end copy is still skipped, so the ring-keep path must be the one that fires).
- **2026-10-19 (night, audit #4) — read Gemma 4 speed from the engine log, not the call count.** On Gabee (M3 Max 64 GB) Gemma 4
  31B 8-bit on 1b5f497a reuses 91-99 % of each prompt (I-4b fix verified live: `[cache_fetch] HIT prompt_tokens=43136
  cached=40662`), yet turns take ~5 min because DECODE is ~6.1 tok/s (Metal-memory lines every 256 steps ~42 s apart) and the
  model writes 1.5-3.8k reasoning tokens per turn. A goose compaction summary request is an expected cache MISS (new prompt from
  token 4). Speed lever not yet used: Google's drafter google/gemma-4-31B-it-assistant for speculative decoding.
- **2026-10-19 (01:30-02:30, fork) — D-METAL-CAP OVER-PRICED GEMMA 4 KV 8x AND MASKED ITS REFUSAL (RUN-ISSUES I-17; fix
  lz/gemma4-kv-window 3cfe2384 on 1b5f497a5).** Studio Gemma 4 12B 8-bit run openai-cloud-9e5d0bd2 refused: "needs ~42.6 GB on
  top of 38.0 GB (reserved 0.0 + projected 42.6), limit 75.1 GB" -> goose saw "Internal error during streaming" x3 -> REFUSED.
  NUMBERS (real loaded model, kvprobe in /private/tmp on the workhorse): guard 131,072 B/token + 655,360 B/sliding slot vs
  MEASURED 16,384 + 327,680 (cache nbytes after a forward). Factors: 4x head shape (Gemma 4 full layers cache
  num_global_key_value_heads x global_head_dim = 1 x 512; the config's base 8 x 256 is the SLIDING shape) x 2x dtype (mlx-vlm's
  inner TextConfig has no `dtype` -> _infer_kv_dtype_bytes fp32 fallback). AMPLIFIER: max_tokens unset is resolved to
  window - prompt (~222k for a 40k prompt), so T = 262,144 every turn, and an untokenized prompt string was counted as UTF-8
  bytes. Pre-existing (same estimator in e3a92b55; 87e71b18's latent override returns None for Gemma) — NOT a regression.
  FIX: kv_estimation.attention_module_kv_bytes (per-layer self_attn n_kv_heads/head_dim/is_sliding, is_kv_shared_layer = 0),
  Scheduler._kv_element_bytes (loaded params' itemsize first, for growth + latent + quantized re-pricing), prompt counted by
  the engine tokenizer, BackpressureError(error_object=...) = context_length_exceeded when it cannot fit alone (HTTP 400
  pre-stream / SSE object mid-stream), else SSE code rate_limit_exceeded (goose in_stream_error -> RateLimitExceeded -> retry).
  Final head b20ec1b7 (+9acd7982: a waiting request's prompt is tokenized ONCE — _sum_in_flight_kv_bytes re-projects every
  waiting request per admission under the lock; +b20ec1b7: test_metal_cap_enforcement's byte-fallback test now drives the
  fallback). Tests tests/test_gemma4_kv_admission.py (11 red on base, 14 green). DEPLOY REV=b20ec1b73f3ee4bb61aa1013862da516d9691a59.
  TRAP: tests/test_memory_guard_mtp_decode.py::test_a_fixed_verify_cost_is_planned_without_a_crossing[m4max128] takes ~96 s on
  the Studio — a --timeout 90 (thread method) kills the WHOLE run there; use >= 300. HF_HUB_OFFLINE=1 makes
  test_streaming_detokenizer / test_tokenizer_gemma4_hybrid fail on base too (no cached tokenizer). REPLAY (own engine :8197, the app's argv, worktree via
  M_WORKTREE sys.path launcher /private/tmp/m_engine.py, client /private/tmp/m_replay.py = 7 msgs/85k chars/5 tools/max_tokens
  unset): 3 concurrent turns -> base refuses 2 ("projected KV 41.6 GB", masked); fix admits all, peak 29 GB.
  GOOSE GAP (not fixed here): goose's in-stream error mapper (goose-provider-types formats/openai.rs in_stream_error) maps only
  rate limits; a mid-stream context_length_exceeded becomes ServerError, NOT compaction — only the pre-stream 400 compacts.
  Gemma 4 31B on Gabee: dtype present, so only the 2x head-shape over-price (163,840 vs 81,920 B/token); its live log
  (mlx-engine-1792361017063.log, 21 requests to 43k prompt, max_tokens ~219k) has ZERO D-METAL-CAP lines — WHY the same
  estimator does not bite there (50.1 GB cap) is UNEXPLAINED; check before trusting the 64 GB Mac with concurrent turns.
- **2026-10-19 (03:40) — GRANITE 4.2 30B: stale `granite[-_]?4` profile (RUN-ISSUES I-23).** Engine b20ec1b7 boots
  lmstudio-community/granite-4.2-30b-MLX-8bit as hermes / reasoning None / hybrid True. It is DENSE (GraniteForCausalLM) and its
  template primes `<think>\n` behind enable_thinking (default True), so reasoning streams as content and a text call never
  stops. The serve CLI has `--reasoning-parser <name>`, `--no-hybrid`, `--force-hybrid`, `--no-tool-call-parser`: appending
  them to the wrapper's exec line overrides auto-detect (Studio ~/goose-builds/loop-state/engine-wrap-granite.sh adds
  `--reasoning-parser qwen3 --no-hybrid`). Engine fix still owed: key granite profiles on model_type (granite = dense,
  granitemoehybrid = hybrid) and pick qwen3 when the template primes <think>.
  FIXED 2026-10-19 (fork `lz/granite42-profile` 7108304a01658d9cf61f74ea8d60bdf5f9c18090 on b20ec1b7, not released): the
  name rule defers to model_type (granite -> qwen3_coder_xml / qwen3 / not hybrid, = IBM's granite-4.2 aliases), and a
  bare `--tool-call-parser X` now binds. TRAP that hid behind the wrapper: on `rapid-mlx serve` an explicit
  `--tool-call-parser` WITHOUT `--enable-auto-tool-choice` was silently dropped (/v1/models `tool_call_parser: null`):
  non-streaming calls still parsed via the generic fallback scan, STREAMED calls came back as content, finish stop. Check
  `/v1/models` after any parser override. Studio wrapper now passes the app argv only; smoke: no-effort text + 3/3 tools,
  medium 3/3 tools; medium text overthinks past smoke's 600-token cap about half the time (model, not parser).
- **2026-10-19 — the three Macs do NOT run the same goose.** Gabee's app is a t12-line build that sends
  `reasoning_effort: medium`; the Studio and laptop run released 3.0.118, which sends no thinking preference, so the engine's
  R12-T1F injects enable_thinking=False. Gemma 4 then reasons exactly "\n" each turn, and the released formatter folds every
  turn into turn 2 (I-22, fix branch lz/i22-turn-merge 6464f335). Check `[REQUEST]` roles in the engine log on a new machine.
- **2026-10-19 — Nex-N2.5-mini (qwen3_5_moe, abenzerps folder) gets NO parsers from auto-detect (RUN-ISSUES I-27).** Wrapper
  flags `--tool-call-parser qwen3_coder_xml --reasoning-parser qwen3` fix it on 7108304a+ (earlier engines silently ignored
  an explicit --tool-call-parser without --enable-auto-tool-choice — the I-23 finding). Generalise the I-23 template-wire rule.
- **2026-10-19 — NEMOTRON 3.5 BOLT (`nemotron_h_puzzle`) PORTED + the guard's KV over-prices (RUN-ISSUES I-29, I-30).** Fork
  `lz/nemotron-h-puzzle` 6d127474875ded467922111b5825329758751776 (on 7108304a; 1b025e4f5 = port of raullenchai/Rapid-MLX#4679:
  vendored rapid_mlx/models/nemotron_h_puzzle.py over mlx-lm nemotron_h, aliases, sizes, tiny-forward tests). Folder profile is
  TEMPLATE-driven (the PR pins nemotron/qwen3 unconditionally): the Nemotron-H fallback now covers both model types; MoE when any
  block_configs entry is moe. Two guard fixes in the same branch: (1) `_kv_footprint_terms`' mlx-lm fallback priced KV with the
  config dtype probe, which has no dtype in mlx-lm ModelArgs -> fp32 -> KV DOUBLED (Granite 4.2 30B 524,288 vs 262,144 B/token,
  window 69,888 -> 131,072 native); now `_kv_element_bytes`. Read it from the boot line: `token=` in `[memory_guard] enabled: prior`
  = KV/token + 16 x hidden + 6 x intermediate (bf16) -- a KV/token twice the model's real figure is this bug. Qwen3.8 unaffected.
  (2) Nemotron-H block stacks carry no `layer_types`, so they were priced as all-attention (Bolt 180,224 B/token, window 116,480);
  `kv_estimation.block_pattern_kv_bytes` reads the loaded blocks ("*" KV, "M" fixed conv+SSM state, "E"/"-" nothing): Bolt 8,192
  B/token + 170,229,760 B state (= the live `recurrent=162.3MB`), window 262,144. PROBE without loading weights: a stub scheduler
  (`Scheduler.__new__` + model/config/_kv_* fields) over `mlx_lm.utils.load_model(dir, lazy=True)` prints `_kv_footprint_terms()`,
  `_memguard_prior()` and `MemoryGuard.max_context` in a second (scratch m_guardprobe.py / m_windowprobe.py pattern). LIVE (Studio
  app, picker swap, no app quit): Bolt boot clean, smoke text + 3/3 tools (medium: reasoning split), 58.6k prompt admitted 93 s;
  Granite 81.6k admitted (was refused at 80k). DEPLOY TRICK when the app must not be quit (a cloud chain run is live): the engine is
  spawned from the wrapper FILE at every mount, so bump REV in the currently configured wrapper and swap models with the
  #/leanzero-swarm picker -> "Run on this Mac"; config.yaml stays untouched. RESIDUAL: head_dim-128 models never calibrate the
  attention coefficient (every pair slope < one layer's copy is discarded), so the 4-copy prior stays and prefill chunks get cut
  (chunks_reduced 13-25 per long prompt) -- safe, slower. App planner gap: "config.json has no integer num_hidden_layers" for Bolt.
- **2026-10-19 (audit #9) — THE ENGINE DOUBLED HISTORY THINK BLOCKS on a template that renders them on every row (RUN-ISSUES I-32;
  fix lz/think-retain-once e874f045).** Signature: requests whose LAST role is `user` (a goose nudge, a compaction request) reuse almost
  nothing (`cached=` a few thousand of 100k) right after a stored boundary, while tool-ending requests reuse 90 %+. The client is NOT
  the cause if goose's bodies share their prefix: replay the ENGINE's render offline — `rapid_mlx.utils.chat_template.apply_chat_template
  (AutoTokenizer.from_pretrained(<model dir>), engine._normalize_tool_call_arguments_for_template(msgs), tools=...,
  enable_thinking=False, add_generation_prompt=True)` with the engine's uv-archive python (`ps` of rapid-mlx serve), CPU only,
  HF_HUB_OFFLINE=1 — and diff the two renders. `_retain_tool_loop_think_blocks` was armed by literals alone; a probe render now decides.
  Rule: never decide row edits from rendered text (an existing test pins that a row's CONTENT may open with its own think block); decide
  per TEMPLATE with a cached probe render, like `_template_puts_narration_before_calls`.
- **Speed math for a dense model at long context (laptop Granite 4.2 30B, 2026-10-19):** decode tok/s ~= bandwidth / (weights +
  KV bytes per token x context). Granite 30B KV = 64 x 8 x 128 x 2 x 2 B = 256 KB/token -> 25.6 GB read per token at 100k + 32 GB
  weights -> ~9 tok/s ceiling on an M4 Max (546 GB/s); measured ~7. Slow is the architecture, not the NFS model mount.
- **2026-10-19 — Providers > Models delete refused SSD-linked models ("refusing to delete '<id>': /Volumes/AI-workhorse/... resolves
  outside /Users/workhorse/.goose/models").** The Studio's ~/.goose/models/<publisher> are SYMLINKS to the SSD; the listing follows
  them (a20f78b5b) but `hf::delete_local_model`, `hf::remove_partial_repo` and leanzero-link `replica::remove_partial` refused any
  canonical path outside the models dir. Fixed on main ec7ba61d7 (74f751335 + review fix; ships in the release after 3.0.119): one helper `remove_model_dir`, Need::Model
  requires config.json behind a link, never the models dir or an ancestor, per-model link removed with its target, publisher link kept;
  read-only error hint; a CANCELLED download/copy removes the folder only if that job CREATED it, else only `.part` files - review D1 found a Resume+Cancel of an existing incomplete model wiped it, also before the change; ancestor check by dev+inode for macOS firmlinks). Until it ships: delete on the Studio with `rm -rf` of the EXACT SSD path after the marker check, and log it in
  /Volumes/AI-workhorse/models/goose/DELETED-2026-10-19.log. The LAPTOP column is the SAME files over a read-only rclone NFS share
  (192.168.0.2:/ = models/goose), so a laptop-side delete can never work and both columns vanish together; the laptop's listing lags a
  deletion by minutes (rclone dir cache).
- **2026-10-19 — RELEASE 3.0.119: engine pin v0.14.3-lz.12, overnight wrappers RETIRED.** ENGINE_LAUNCHER =
  `rapid-mlx[mtp] @ git+...Rapid-MLX@v0.14.3-lz.12` (tag object 5dfea7d5 -> cd53a8247 = lz/overnight-stack 429d0b40d +
  lz.11's LFM auto-config c56728690, which the overnight stack lacked: pin 429d0b40d alone and a local LFM2.5 folder
  serves tool calls as text again). engine_profile.rs (from t12 d44f8a81b, launcher table only): lz.12 = Guarded, so
  build_serve_command passes NO --cache-memory-percent (prefix_cache_flags, from t12 fe1893dec) — the guard sizes
  its own cache, as the overnight wrappers did. PIPELINE_SINGLE_LINE_REVIEWED_THROUGH = lz.12 (fork
  lz/pipeline-review-lz12 56e0a4165: 117 n/a, 3 equivalent, 11 recorded `gap`s — the Flash split stays on
  8c000705 without the tool-call hardening, split-turn join and reasoning_content sanitising). Installed 08:56 on
  the laptop and 08:57 on Gabee: the `mlx_engine.spawn_command` wrappers (engine-wrap-laptop.sh,
  ~/t12-bench/engine-wrap.sh) were removed with the app quit (backups config.yaml.before-3.0.119-*); the restored
  engines run `Rapid-MLX@v0.14.3-lz.12` without the cache share, memory_guard.enabled true, toolsmoke 3/3 on both.
  The Studio's engine-wrap-granite.sh goes in its install waiter (loop-state studio_waiter.sh). Gabee's t12-line
  goose binary (thinking effort, compaction V2/V3, live window) is GONE with 3.0.119 by the coordinator's
  decision: those features are not on main. A fresh-fetch proof for any new pin: `uvx --no-cache --from
  "rapid-mlx[mtp] @ git+...@<tag>" rapid-mlx --help` with HOME/UV_CACHE_DIR in a scratch dir.
- **2026-10-20 — ENGINE PIN v0.14.3-lz.13 on branch lz/backlog-1019 (not released).** Tag object 3bab1d05 -> c58790b11 (fork
  lz/backlog-1019 = lz.12 + I-28 `rapid_mlx_stop_reason: "repetition"` beside `finish_reason: "length"` (also when the stop lands
  inside a tool call: the cut call arrives with args `{}`, goose fails it with LOOP_STOPPED_TOOL_CALL), I-24 long exact-loop hard
  stop (periods 129..1024, 4 copies, tool-bearing requests only on every lane, `--repetition-guard-max-period`, default 1024),
  I-27 template-wire parsers for folders no family rule claims (Nex-N2.5-mini -> qwen3_coder_xml + deepseek_r1_distill), a prompt
  that closed the think block streams as content (`prompt_thinking_closed`), and the D-METAL-CAP refusal worded as "the largest
  prompt that fits beside that reservation is N tokens" (+ `max_tokens_reserved`; goose also keys on lz_reason
  `metal_cap_exceeds_device` so an lz.12 engine's "maximum context here is N" from that path is never read as a window).
  Pipeline review on the fork's branch lz/pipeline-review-lz13 (1a2a4118d): 5 n/a, 2 gaps (the split's own StreamRelay repeat
  stop sends no stop reason; its priming never sets the closed-think signal). Pin bump recipe that worked: tag annotated at the
  head, `git push origin <tag>`, add the json entries on the pipeline branch (json.dumps indent=2, ASCII, trailing newline — any
  other serialisation rewrites the whole file), run `tests/test_pipeline_single_line_ports_audit.py` (it lists every gap as a
  warning), then ENGINE_LAUNCHER + SUPERSEDED_ENGINE_LAUNCHERS + PINNED_PROFILES Guarded row + PIPELINE_SINGLE_LINE_REVIEWED_THROUGH
  + the two argv goldens in engine.rs. LIGHT SMOKE without the Studio: the configured Flash-Next-4bit is 98 GB on the read-only
  NFS share served FROM the Studio's SSD, so loading it is Studio I/O; instead fetch mlx-community/Qwen3.5-9B-MLX-4bit (5.6 GB)
  to a scratch dir, print the app's argv with a throwaway `build_serve_command` test, run it on a spare port, stream one text +
  one tool call. lz.13 measured 2026-10-20 on the laptop: parsers bind (explicit qwen3_coder_xml/deepseek_r1 over the family's
  hermes/qwen3), memory guard enabled, no cache share, text clean, `write` call whole; direct_url commit c58790b11.
- **2026-10-22 — RECURSIVE FLINT LOCAL (RecursiveSI/flint, `flint` / FlintForCausalLM, Apache-2.0, 39.8B total / 2.5B
  active MoE; fork branch `lz/flint` ff88eb79cdbdb12c3b5c59f34951b79ebd251d40 on the lz.13 pin c58790b11; RELEASED in pin
  v0.14.3-lz.14, 2026-10-23).** Stock mlx-lm (0.31.3 pinned, 0.32.2 latest) has no `flint`; mlx-lm PR #1998 is open; mlx-vlm
  0.7.10 runs it but would put Flint on the multimodal lane and cannot read Recursive's uint8 INT4. So: vendored.
  - Model (`rapid_mlx/models/flint.py`, PR #1998 + the reference's rules): sliding layers = window 513 INCLUDING the current
    token, RoPE; full layers = `no_rope_layer_indices`, NO positional encoding; learned per-head sinks (`softmax_offset`);
    fp32 sigmoid routing, top-6, renormalised (+1e-20), ungated shared expert; fp32 lm_head. Rapid-owned
    (`_RAPID_OWNED_RUNTIME_MODEL_TYPES`): installed over any native mlx_lm.models.flint, checkpoint model_file ignored.
  - TRAP: Recursive's INT4 export packs weights as uint8 (sanitize views them as uint32) and stores the embedding at 8 bits
    without a per-layer config entry; `_flint_quantization_override` reads the shard headers and adds the widths.
  - TRAP: the PR's ModelArgs dropped `max_position_embeddings`: /v1/models said context_window null and the guard advertised
    768,512 on the 128 GB laptop for a 262,144-token model. Fixed (ff88eb79c); now 262,144 / 262,144.
  - Parsers (auto-config by model_type flint): tool `flint` = Xing4.0's reading rules on GLM-4.7's `<arg_key>/<arg_value>`
    tags (glm47 strips + json-decodes values: "42\n" came back as 42); reasoning `flint` = `<think>` only where the model writes
    it (no switch, never primed: untagged output is content from the first char). The template mentions `<think>` (history
    rows), so any source-scan priming guess is wrong for it.
  - TRAP (live): the chat route's default tool-use suffix ("Do NOT think out loud ...") made Flint write `write` calls with NO
    content (5/21 samples at the route's exact prompt, 0/16 without). Tool parsers can now set `AUTO_TOOL_USE_SUFFIX = False`.
  - TRAP (live): `reasoning_effort=medium` = a 2,048-token post-hoc reasoning cap on tool requests; the rest of the thought
    arrived as CONTENT. Flint has no effort control, so its parser sets `graded_effort_cap = False`.
  - KV: 28,672 B/token growth (14 full layers) + 71,680 B per sliding slot (35 layers), window 513 (the checkpoint README's
    28 KiB + 70 KiB); probe `_kv_footprint_terms()` on a lazy load prints (28672, 0, 71680, 513).
  - Proof: parity vs an independent float64 numpy reference 8.7e-06 at scale 6 (top-1 40/40; cached decode across the window
    3.9e-06) and vs mlx-vlm 0.7.10's flint 2.3e-06; route SSE + whole-response tests 34/34 fail with qwen3/deepseek_r1/glm47;
    217-file related suite: same 12 failed + 17 errors as lz.13. Laptop smoke (spare port 8197, app argv, weights over NFS):
    text 4/4, write tools 11/12 plain (the miss = a well-formed `shell` first) and 6/6 at medium, 10-turn agent loop with
    history reasoning + tool results: prefix cache 97-99 % per turn, parallel writes parsed, files byte-correct; ~100 tok/s.
  - WEIGHTS: RecursiveSI/flint-int4 @3b93f905 (22.6 GB, sha256 = its flint-int4.json manifest) at
    /Volumes/AI-workhorse/models/goose/RecursiveSI/flint-int4 (laptop: ~/.goose/models/RecursiveSI/flint-int4, visible over
    NFS without a remount). Chosen over pgmharikrishnan/Flint-MLX-Serve-mixed-4-8bit (also on the SSD): teacher-forced on
    11,242 tokens written that day, int4 ppl 6.82 vs 7.34 (paired diff 0.073 nats/token, 95 % CI 0.039-0.109); Recursive
    trained its quantized builds (blog: distillation recovery) and mlx-community measured INT4 ppl below a 6-bit PTQ of BF16.
    mlx-community/flint-mixed-3-4bit = 3-bit experts (rejected, owner's 4-bit rule).
  - TRAP: right after the download, the laptop's NFS view failed "[load_safetensors] Failed to open file" twice and once
    gave a Metal GPU Timeout during load (while the workhorse was still writing/hashing the other download); a later boot
    loaded in ~32 s. If a fresh model fails to open over NFS, wait for the workhorse's I/O to settle and retry.
  - Launcher: ~/goose-builds/loop-state/engine-wrap-flint.sh (REV = the sha above, drops --cache-memory-percent, no extra
    flags: boot log says tool_call_parser=flint, reasoning_parser=flint). Sampling: generation_config 1.0/0.95/20 is applied
    by the engine; Flint reasons long on hard turns (card: max_tokens >= 32768; goose sends none = engine 32,768).
- **2026-10-23 — DENSE 27B AT 4 tok/s ON THE LAPTOP = Q-447 ON THE SINGLE ENGINE (float32 promotion; fork
  `lz/kv-dtype-extend` @ edd279eb6cb82267d86e5419dcd145b85f9e7089 on the lz.13 pin c58790b11; RELEASED in pin v0.14.3-lz.14).** Symptom (M4 Max 128 GB, app engine lz.13,
  Forge 2.0, sabicap/sabi-noema-MLX-4bit = dense qwen3_5 27B 4-bit, no MTP): 4.0 tok/s at ~47k with prefix-cache HITs every
  turn, ~15-17 at <1k; HireTheRobots Qwen3.8-27B 4-bit 2.9 tok/s at 170k. MoE on the same app/engine fast (Ornith 122B-A10B
  38 -> 21 tok/s from 50k to 200k; Flint 46-62). It was NOT dense-vs-MoE, NOT the memory guard (0 actions, 0 clears; it
  checks per prefill chunk, not per decode step), NOT head_dim-256 decode attention.
  - EVIDENCE, readable from any engine log with no attach: `[cache_store_bytes] tokens=N ... kv=X MB` -> X x 2^20 / N =
    65,536 B/token (bf16, 16 full layers x 2 x 4 KV heads x 256 x 2 B) for every entry until 03:47:10, then 131,072 (float32)
    for EVERY later entry, and `recurrent=` 146.8 -> 149.4 MB (the conv state follows the activation dtype; the SSM state is
    float32 by design). At 03:47:10 `[schedule] ... prompt_tokens=3208 ... running=2`: goose's second request joined while
    the first (1,405) was mid-prefill. HireTheRobots flipped at the identical moment of its run. Ornith's two requests were
    scheduled in the same millisecond (prefilled together, no extend of an empty side), so it stayed bf16 by luck. The
    `[Metal memory] ... cache=0.0GB` on every decode line is the same float32 signature Q-447 recorded on the split.
  - CAUSE: mlx-lm 0.31.3 `BatchKVCache.extend` / `BatchRotatingKVCache.extend` pad a side with no KV via `mx.array([])`
    (FLOAT32); concatenate promotes the batch, the attention output and residual stream follow, `merge` keeps the first
    row's dtype on every restore, and each stored prefix entry hands float32 to the next turn: the session never recovers.
    Persisted entries (`[cache_persist] SAVED` at shutdown, autoloaded at boot) carry it into the NEXT engine too. goose fixed
    exactly this on the tensor split (Q-447, rank_batch.py `extend_in_its_dtype`) but the single engine never got it.
  - FIX: `_mlx_compat.install_batch_kv_dtype_guard` (installed by scheduler.py and mllm_scheduler.py): the cold side gets
    zero-length KV in the filled side's dtype; two filled sides of different dtypes are counted + WARNED. The scheduler's
    fetch drops any prefix entry holding float32 KV on a half-precision model (`[kv_dtype] dropped N prefix entr...`) and
    re-reads the prompt once. `/v1/status kv_dtype {model_dtype, cold_rows_padded, mixed_dtype_extends,
    promoted_entries_dropped}`. Test `tests/test_kv_dtype_cold_join.py` (real Scheduler + BatchGenerator, tiny BF16 qwen3_5
    hybrid on CPU, second request at step 2 of an 8-token-chunk prefill): 7 red on c58790b11 (stored entry float32), green
    on the fix; mutants "guard not installed" / "no fetch drop" killed. Related suite (35 files, CPU): 704 pass, 1 fails the same on base. A float32 tiny model cannot show it (nothing to
    promote): BF16 fixtures only.
  - EXPECTED after the fix: bandwidth model (checked on Granite 4.2 30B dense bf16 on this laptop: predicted 11.3, measured
    10.3 at 30k) gives the 4-bit 27B ~17 GB weights + 3.1 GB KV per token at 47k -> ~20 tok/s ceiling at the measured
    ~449 GB/s, ~15-18 real; ~20+ at short context. That also matches Gabee's 27B Q8 at 11.7 (2x the weight bytes, 0.73x BW).
  - PROVE IT LIVE (between series, app quit; never mid-run): engine-wrap REV = the full sha above, remount sabi-noema, run the
    first ~15 turns of a Forge series. PASS = every `cache_store_bytes` entry >= 1k tokens at 65,536 B/token, recurrent stays
    146.8 MB, `/v1/status kv_dtype.cold_rows_padded >= 1` and `mixed_dtype_extends == 0`, and `[Metal memory]` step lines
    256 steps per <= ~17 s at 40-50k (>= 15 tok/s). If entries stay bf16 and decode is still ~4 tok/s, the float32 theory is
    refuted and the next suspect is the q=1 SDPA path at head_dim 256 / GQA 6 (compare Ornith's slope: 0.15 ms per 1k tokens
    of 24 KB KV). On a pre-fix engine the first run after the fix may log `[kv_dtype] dropped` for autoloaded float32 entries.
  - **LIVE-PROVEN 2026-10-23 (laptop, app, sabi-noema 4-bit, Forge 2.0 call 19 at 48k):** kv 2995.2 MB / 47,924 tok =
    65,535 B/token, recurrent 146.8 MB, `cold_rows_padded` 16, `mixed_dtype_extends` 0, decode 123 tok in 8.05 s after the
    first token = 15.3 tok/s (pre-fix ~4). Launcher: ~/goose-builds/loop-state/engine-wrap-kvdtype.sh. RELEASED 2026-10-23: pin v0.14.3-lz.14
    (see the lz.14 release entry below). Retire engine-wrap-kvdtype.sh after the current laptop series: put
    `mlx_engine.spawn_command` back to the default (app quit, config backed up) once the laptop runs 3.0.127+.
  - CHECK ON ANY NEW MODEL: compute kv/token from the first two big `cache_store_bytes` lines; a doubling mid-run = this bug.
- **2026-10-23 — RELEASE v0.14.3-lz.14 (fork lz/release-lz14 03fa3b38ff4bbf42642d1e3448b1fffce90c4b09 = lz.13 c58790b11 +
  merge lz/kv-dtype-extend edd279eb6 + merge lz/flint ff88eb79c; goose 68c6b5398 pins it; app 3.0.127).** The KV-dtype fix
  and Recursive Flint both ride it. Flint was taken because it sat on the same lz.13 base, merged clean and its suite was
  green; it is model-gated (parser flags default to the old behaviour for every other parser).
  - Pipeline review: lz/pipeline-review-lz14 eec8f6bd7 on 1a2a4118d, 7 n/a (the split never calls mlx-lm
    BatchKVCache.extend: a row prefills alone and joins by merge; Qwen4ExpStateCache.extend = extract + merge).
    goose PIPELINE_SINGLE_LINE_REVIEWED_THROUGH = lz.14; PIPELINE_FORK_COMMIT unchanged.
  - HOW THE FORK SUITE WAS RUN (workhorse, ~25 min): worktrees for head AND base on the SSD
    (/Volumes/AI-workhorse/scratch/release-lz14/), `~/Projects/Rapid-MLX/.venv/bin/python -m pytest -p m_cpu` (a plugin
    doing `mx.set_default_device(mx.cpu)`), PYTHONPATH = plugin dir : worktree : a side `pyext` dir with hypothesis +
    aiohttp only (`uv pip install --target`, then delete anything the venv already has: typing_extensions/idna/attrs),
    `--continue-on-collection-errors --timeout 600`, 10 file shards per arm, skipping test_pipeline_qwen4{,_serve,_vision}.py
    (launch ranks). Compare FAILED/ERROR node ids head vs base. TRAPS: (1) a shard's failures depend on its NEIGHBOUR files
    (test pollution), and the head arm has extra files so its shards differ: every "new in head" id must be re-run in
    isolation on both arms (here 43 = 43, identical). (2) tests/test_batching_deterministic.py hangs at 0% CPU on both
    arms past pytest-timeout (kill it, drop the file from the shard). (3) the venv lacks hypothesis (test_memory_guard.py)
    and aiohttp (test_event_loop.py): without them the WHOLE shard aborts at collection (exit 2).
  - Tag proof (workhorse, tag via uvx, app argv, port 8197): direct_url commit 03fa3b38f; sabi-noema 4-bit, a 4.5k cold
    request joined a 20.9k prefill (running=2) -> kv_dtype cold_rows_padded 16, mixed 0, kv 65,535 B/token, recurrent
    146.8 MB, follow-up HIT 20,864; ~31 tok/s decode at 21k on the M3 Ultra. Flint: parsers flint/flint, smoke text PASS,
    tools 2/3 (TOOL1 reasoned past smoke.py's token cap, the model's habit, not a parser fault).
  - TRAP: BOTH repos (goose and Rapid-MLX) have a commit-msg hook that REJECTS any `Co-Authored-By: Claude` line
    ("This is a PUBLIC repo"). Commit without it, whatever a brief says.
  - Release build: on the workhorse in the worktree with `target` symlinked to ~/Projects/goose/target (copy-binary reads
    ./target), `source bin/activate-hermit; (cd ui/desktop && pnpm install --frozen-lockfile); just release-notarized <v>`,
    then commit notes + package.json, tag v<v>, push, `just publish-release <v> docs/releases/<notes>.md`.
- **2026-10-30 — LONG RUNS (>30 min) section added near the top** (owner rule in CLAUDE.md): context proofs and replays front-load the hardest cell under the app's argv; vigil lines and kill criteria from the 241,320 refusal.
- **2026-10-29 — qwen4_exp VISION WIRED ON THE TEXT LANE: tag v0.14.3-lz.16-ple.2 (fork lz/qwen4-vision-ple 1d4f4072d =
  lz.16-ple.1 + d8b7523b7 + 1d4f4072d), pinned on goose flash-pld-bench 80c696b3ee (goose b4b55fe7df stops passing --text-only
  for it).** HOW: image pads become ids IMAGE_ID_BASE(2^24) + slot*2^14 + k (slot = content-hash registry, process-local);
  the decoder maps them to image_token_id for embed/PLE/MTP and scatters the tower's features (mlx_vlm 0.7.1
  qwen4_exp VisionModel + Qwen3VLProcessor, tower 0.42 GiB); per-row ImageSpans ride the QSA cache meta/extract/merge/
  filter/trim and give multimodal RoPE; image-bearing prefix entries are never saved to disk. Lane reason
  `text_lane_vision`, /v1/models capabilities include "vision" (goose's openai provider then sends images),
  /v1/status text_lane_vision {images_registered, images_encoded, image_requests, features_cached}; off with
  RAPID_MLX_TEXT_LANE_VISION=0 or --text-only. Tests: tests/test_qwen4_exp_text_lane_vision.py (16).
  LIVE (workhorse, runs/2026-10-29-flash-vision): screenshot/chart/parrot/zebra/two images/follow-up/3 concurrent all
  read correct. TRAP: code_edit 8k sha 5aea271218556eef is the PRE-lz.16 (whitespace-stripped stream) output; on
  lz.16-ple.1 and ple.2 it is f9dca5e177ca568e (EXACT vs the expected rename, check_code_edit.py). TRAP: greedy prose is
  not byte-repeatable run to run on Flash + MTP even text-only on the base (first divergence ~250 chars), so 'same
  request twice differs' is not a vision bug. TRAP: on the 96 GB workhorse the memory guard skips every image-bearing
  prefix store (idle room 1.2 GB), so prove the image prefix cache on a 128 GB Mac.
  - SAME DAY: tag v0.14.3-lz.16-ple.3 (aafee8699, goose pin 144db445cf) = ple.2 + /v1/models lists "vision" behind
    an is_text_only alias. TRAP: aliases.json qwen3.8-flash-next-4bit pins is_text_only; goose serves under the alias's
    hf_path, so ple.2 listed [text,tools,experimental] and goose's openai provider sent PLACEHOLDERS for every image
    while the tower was attached. A proof under --served-model-name default cannot see this: always prove under the
    name goose serves. Mule (128 GB) proof: follow-up turn cached_tokens 768, parrot repeat byte-identical, 0 skipped
    prefix stores. The rapid-mlx/Qwen3.8-Flash-Next-4bit build was deleted 10-29 (owner); a new build replaces it.
  - SAME DAY: tag v0.14.3-lz.16-ple.4 (f6cc404c8) loads Jundot/Qwen3.8-Flash-Next-oQ4e-mtp (oMLX, the new build):
    PLE tables spelled `language_model.model.layers.1.ple.ple_embedding.ngram_embedding.shards.K` (raw HF: `model.language_model
    ...shard_K`), 4-bit g32 BF16; MTP head mixed (8-bit attn/hyper-connections/fc_*, 4-bit split switch_mlp experts, BF16
    router) -> quantized per checkpoint tensor; `...ngram_embedding.weight_scale` = 1.0 (folded FP8 scale) dropped; Qwen's
    preprocessor allows 16.7M px/image -> text lane caps 1,003,520 (RAPID_MLX_TEXT_LANE_MAX_PIXELS). Dry check recipe:
    zeros-of-header-shapes through Model.sanitize + nn.quantize(config predicate) vs tree_flatten (2954 = 2954, MTP 76 = 76).
    Live mule: all image checks correct, MTP + PLD active, PLE offload 77.1 GB active. Evidence workmule:/tmp/gvision/proof/out/j2,
    runs/2026-10-29-flash-vision/HANDOFF.md.
- **2026-10-29 — STREAMING WHITESPACE BUG FIXED + LONGCAT TOOL PARSER: tags v0.14.3-lz.16 (= lz.15 + fix, fork
  lz/stream-whitespace b328262b8) and v0.14.3-lz.16-ple.1 (= lz.16 merged with lz.14-ple.1, lz/stream-whitespace-ple
  8c9927ae3). NOT pinned yet (goose main still lz.14; pin lines in goose-wt-ngram HANDOFF.md).**
  - BUG (every local model, every pin up to lz.15): service/postprocessor.py ran every streamed content delta through
    sanitize_output, whose _sanitize_with did pattern.sub(...).strip() on any text holding < [ ]. Deltas " [--",
    "]\n\n\n" lost their whitespace: streamed file writes came out with joined words and missing blank lines.
    FIX: all 10 delta sites use sanitize_content_for_stream (markers only, whitespace kept); sanitize_output returns the
    text unchanged when it removed nothing; clean_output_text/strip_thinking_tags no longer trim the non-stream answer, so
    stream and non-stream are the SAME BYTES. Tests: tests/test_stream_whitespace_fidelity.py (14 fail on lz.15).
  - LONGCAT (Flash-Lite, model_type longcat_flash_ngram): new `longcat` tool parser (Flint/Xing reading rules on the
    <longcat_tool_call>/<longcat_arg_key>/<longcat_arg_value> wire, string values byte-identical), auto-selected by
    model_auto_config from model_type + template. tests/test_longcat_tool_parser.py.
  - LIVE (workhorse, code_edit 8k greedy, runs/2026-10-29-stream-whitespace-fix): Qwen3.5-9B-4bit base stream 7351 vs
    7357 chars, fix byte-equal; LongCat base 41 wrong streamed lines, fix stream EXACT = non-stream EXACT; LongCat
    tool_calls null before, read_file {path, max_lines 20} after, stream and non-stream. Fork suite vs lz.15: identical
    failure set (150 = 150).
  - LEFT: LongCat SuffixDecoding stays refused ("model is hybrid"): enrich_model_config sees its ArraysCache(size=1),
    which holds the n-gram token history; a verify step would feed draft tokens into it and nothing rolls it back on
    rejection, so the refusal is the SAFE outcome even if the label is wrong. Making suffix/PLD work needs a trimmable
    n-gram context cache. LongCat thinking (<longcat_think>) has no reasoning parser yet.
- **MELLUM LOCAL (JetBrains/Mellum2.1-12B-A2.5B-Thinking, `mellum`, Apache-2.0, 12B total / ~2.5B active; 2026-10-19;
  fork `lz/mellum-lz12` 435781119b82ab1b1f6ecd730c438429c30b3ffc = v0.14.3-lz.12 + one commit; pushed, NOT released):**
  - Why it could not load: the engine's mlx-lm is 0.31.3 (no `mellum` module; 0.32.2 ships `mlx_lm/models/mellum.py`).
    transformers 5.15.1 in the engine env DOES know `mellum`, so tokenizer/AutoConfig need no fallback (no
    `_VENDORED_MODEL_TYPES` entry, unlike Xing).
  - Port: `rapid_mlx/models/mellum.py` = mlx-lm 0.32.2 file verbatim (sha256 3c4936de…) with its four relative imports made
    absolute; all helpers identical in 0.31.3 (yarn reads the same keys; the config's attention_factor 1.2773 equals the
    yarn mscale). Registered in `_register_vendored_archs` with the find_spec defer-to-native probe. Architecture: 28
    layers, 3 sliding(1024, RotatingKVCache) : 1 full (yarn x16 from 8k -> 131k), 64 experts top-8 softmax, GQA 32/4,
    qk-norm. Sliding is picked up by the generic layer_types detection (snapshot path); not hybrid.
  - Checkpoint stores experts per expert (`mlp.experts.N.{gate,up,down}_proj.weight`), which sanitize stacks; transformers 5
    holds them FUSED in memory (`experts.gate_up_proj` [E,2I,H]) — a parity test must split that first.
  - TOOL WIRE: Hermes `<tool_call>\n{"name": …, "arguments": {…}}\n</tool_call>` -> parser `hermes`; reasoning `qwen3`
    (model opens `<think>` itself; template renders `<think>\n\n</think>` when enable_thinking is false). auto-config block
    in model_auto_config.py. Boot log: `tool_call_parser=hermes, reasoning_parser=qwen3, is_hybrid=False`, profile
    "pure attention, throttle OFF, spec decode OK".
  - Proof: parity vs transformers 5.15.1 MellumForCausalLM (tiny random, fp32, window 8, 39 tokens) max |dlogit| 2.4e-07,
    top-1 39/39; cached decode == full forward (`RAPID_MLX_MELLUM_REFERENCE=1 PYTHONPATH=<wt>:/tmp/xingref/pyext <engine
    py> -m pytest tests/test_mellum_model.py`). auto-config + kv-estimation + xing suites 411 passed; hermes/vendored/
    reasoning set 193 passed, 1 failed = same gpt_oss_puzzle loader failure on the base.
  - Real-model smoke (workhorse M3 Ultra 96 GB, bf16 weights 24.3 GB, app argv on :8199): smoke.py 3/3 + text PASS both
    without effort and at `--effort medium`. WITHOUT effort the model emits NO reasoning (0 chars on all 4 cases: the
    engine's thinking-off-on-tools default) — that is what an app run gets today (T12 G1 gap). At medium: 1,313-2,522
    reasoning chars per write call, 4.8-8.4 s. Speed (thinking off, 1 request): 2,137-token prompt prefill 1,484 tok/s,
    decode 95.5 tok/s; 33,903-token prompt prefill 2,236 tok/s (TTFT 15.2 s), decode 88.4 tok/s. Metal peak 26.33 GB
    after the 34k prompt (active 25.66 GB). Evidence: /Volumes/AI-workhorse/experiments/2026-10-19-model-review/mellum-smoke/.
  - WEIGHTS: the original bf16 at `/Volumes/AI-workhorse/models/goose/JetBrains/Mellum2.1-12B-A2.5B-Thinking` (24 GB, 5
    shards). In the app: `~/goose-builds/loop-state/engine-wrap-mellum.sh` (loop-state eb77c93) as
    `mlx_engine.spawn_command` with the app QUIT; remove after. Public-repo hook: Rapid-MLX refuses AI credit lines in
    commit messages.

- **2026-10-30 — skills audit restructure:** SKILL.md became a ~130-line CORE card; every section moved verbatim to references/ (coverage proven by machine-sync bin/skill-coverage). The description was trimmed to <= 1,024 chars (Anthropic skill limit); the old one, kept for the record:
  THE source of truth for running ANY local model through goose with the LeanZero MLX engine (Rapid-MLX fork; current pin = ENGINE_LAUNCHER in crates/goose-sidecar/src/engine.rs) on the laptop, the workhorse or Gabee's Mac — which provider (swarm/omlx for chat, built-in openai at the loopback endpoint for benchmarks; never a custom provider or proxy), the app's engine argv (--text-only, MTP, prefix cache), thinking/effort, long-context caching. LOAD IT BEFORE preparing ANY model for use or benchmark (its MODEL READINESS GATE: right build, vision/MTP/n-gram served end to end, max measured context incl. YaRN), before launching rapid-mlx by hand, writing a proxy or custom provider, or running Gauntlet / Forge 1.0 / Forge 2.0 against a local, candidate or fine-tuned model. Also: operate and evolve the engine itself (crates/goose-sidecar, the MLX window, the fork, the engine bake-off), goose's DISTRIBUTED engine (JACCL/ring split, qwen4_exp pipeline, Flash, rank launcher/supervisor) and REMOTE SINGLE (the engine on a LeanZero Link peer); dated record in references/. The raw two-Mac MLX/JACCL layer underneath is the mlx-jaccl-cluster skill.
- 2026-10-30 omni media: gemma-4-e4b-it-8bit hears audio + watches video-as-frames on mlx-vlm 0.7.1; Qwen3-Omni vision needs mlx-vlm >=0.7.11 (whole pin); fork lz/omni-media wires input_audio + video frames (references/omni-media.md).
