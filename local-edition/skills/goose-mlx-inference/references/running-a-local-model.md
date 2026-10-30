# Running a local model: ground truth

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## RUNNING A LOCAL MODEL — ground truth (read this FIRST; 2026-10-14)
Written after a session ran the T12 Forge 2.0 bake-off on Gabee's Mac HEADLESSLY without loading this skill: it tried
provider `omlx` (refused by the bench fence), hand-launched Rapid-MLX with its own flags, wrote a custom provider and a
request-rewriting "think_proxy", and then spent a night fixing problems it had created (lost tool calls, 30-min 504s,
prefix snapshots silently skipped → 7-14 min per turn). Owner 2026-10-14: "Headless is not my preferred way honestly!"
Every line below has file:line evidence in the T12 plan's "Local bake-off REDESIGN" section
(~/.claude/plans/let-s-study-what-went-sparkling-papert.md).

- **Engine pin:** read it, never remember it — `ENGINE_LAUNCHER` in `crates/goose-sidecar/src/engine.rs` (2026-10-14:
  `rapid-mlx[mtp] @ …Rapid-MLX@v0.14.3-lz.10`). [STALE 2026-10-30 audit: an example only; goose main pinned lz.14, branches lz.16-ple.6 / lz.18. Read engine.rs on the branch you run.] The installed app's pin:
  `strings "/Applications/Goose Swarm.app/Contents/Resources/bin/goose" | grep -o 'Rapid-MLX@[^ "]*'`.
- **The engine is mounted BY THE APP, never typed.** `build_serve_command` (engine.rs ~651-750) [line drift 2026-10-30: `pub fn build_serve_command` is at engine.rs:758 on main] passes:
  - `--text-only` for any vision-declared checkpoint (every Qwen3.8 hybrid) EXCEPT a qwen4_exp checkpoint with its tower
    tensors in the index (Qwen3.8-Flash-Next: `TEXT_LANE_VISION_FAMILIES`, pin >= lz.16-ple.2, 2026-10-29): that one reads
    images on the text lane with MTP/PLD/PLE/batching/prefix cache kept. For the others: without it the model lands on the multimodal
    lane: 1 sequence, `--cache-memory-*` ignored, snapshots over `APC_MEMORY_MAX_GB` (default 5.2 GiB on 64 GB)
    silently skipped.
  - `--max-concurrent-requests 8`, `--enable-prefix-cache --cache-memory-percent 0.35 --hybrid-cache-entries 24`. [STALE 2026-10-30 audit: on a Guarded (memory-guard) pin the app passes NO `--cache-memory-percent`; see engine.rs `build_serve_command` doc comment, and readiness-gate.md step 3.]
  - MTP via `--speculative-config {"method":"mtp","model":<dir>,"num_speculative_tokens":3}` whenever
    `mtp.safetensors` sits in the model folder. The `[mtp]` extra only installs deps. MTP needs real MTP weights:
    `mlx convert --q-mtp` defaults to OFF, so a home-made Q8 has none unless converted with `--q-mtp`.
    TRAP (2026-10-18, Coder390): `--q-mtp cyankiwi` writes INT4 layer linears with a DENSE `mtp.fc`; the engine's
    injector sizes the module from `mtp.fc` ("Sidecar fc is full-precision; leaving MTP module full-precision") and then
    refuses the packed layers ("7 tensor(s) whose shape disagrees") -> boot exit 3, the app view sticks on "Mounting"
    (press Stop). What works: the base's layout `mtplx-bf16` = the checkpoint's own BF16 mtp tensors with every
    `*norm*.weight` shifted +1 (HF Qwen3.5 RMSNorm stores an offset; measured base sidecar = HF + 1.000 on 4 norms),
    metadata {"format":"mlx","mtp_layout":"mtplx-bf16"}, plus config `mtplx_mtp_quantization` copied from the base.
    Tool: Gabee ~/t12-bench/mk_mtp_sidecar.py <hf mtp-bf16.safetensors> <model dir>/mtp.safetensors. Coder390 then
    accepted ~2.7 tokens per verify step, 19.9 tok/s vs 12.3 without MTP.
  - No `--timeout` and no `max_tokens` (engine default 32,768 out). The engine's 1800 s default applies only to
    NON-streaming requests (`routes/chat.py` `_wait_with_disconnect`) and the multimodal lane's first chunk; the app
    streams on the text lane, so no request cap applies, and goose's client has only a connect timeout. A
    non-streaming proxy brings the 30-min 504 back (the T12 pilot did exactly that).
- **Providers:**
  - Chat = `swarm` (SwarmProvider). Its router sends MLX-engine turns through `omlx` (`swarm_router.rs:189,228`) [2026-10-30: the file is crates/goose/src/providers/swarm_router.rs; the line numbers drifted, grep `omlx`].
  - omlx/lmstudio are registered but never offered; an active one is migrated once to swarm.
  - Only `omlx` gets `rapid_mlx_transient_tail` and thinking-off on helper calls (`goose-providers/src/openai.rs:377,427-489`).
- **Benchmarks (Gauntlet / Forge 1.0 / Forge 2.0) on a local model:**
  - Launch ONLY from the app's Benchmark view (goose AGENTS.md GATE 3), driven over CDP when unattended
    (`--remote-debugging-port=9897` + `~/goose-builds/loop-state/` cdp tools / `bench_dispatch.mjs`).
  - Entrant "Single model", provider **`openai`** with its host at the engine's loopback endpoint (`bench_isolation.py:141-143`,
    `run_build.py:614-616`).
  - `swarm` and `omlx` are REFUSED by the fence ("no relay endpoint is known"), and custom providers cannot be picked in
    the form (`CloudEntrant.tsx:61`).
- **KNOWN GAP (open, T12 G1):** the tier's effort pin (`GOOSE_THINKING_EFFORT`, Forge = medium) reaches only OpenAI
  Responses models (`formats/openai.rs:2179-2189`). Rapid-MLX turns thinking OFF on tool-bearing requests unless
  `chat_template_kwargs` is set. So a local entrant run through the app today is NOT at the cohort's effort.
  - Fix it in goose (capability-probed `chat_template_kwargs` on agent turns), never with a proxy.
  - Verify in the rendered prompt.
- **Long single sessions (100k+ tokens):** a 27B hybrid prefix snapshot is 64 KiB/token + ~151 MB of DeltaNet state.
  - Text lane: the budget = `--cache-memory-*` (env `RAPID_MLX_PREFIX_CACHE_MAX_BYTES` wins), and an oversize entry logs
    "Cache entry too large".
  - Multimodal lane: env `APC_MEMORY_MAX_GB` + `RAPID_MLX_PREFIX_CACHE_MAX_BYTES`, and the skip is silent.
  - Diagnose with the per-call `prompt_tokens`/`total_ms` in the run's `telemetry.jsonl` vs `[mllm_apc] … HIT` lines.
    A 60-token reply taking minutes = a full re-prefill.
  - Realistic reuse ceiling on a 64 GB Mac: 120-150k tokens.
- **NEVER, for anything that will be compared or published:**
  - a hand-typed `rapid-mlx serve`;
  - a custom provider or a request-rewriting proxy;
  - `run_build.py` headless;
  - editing the model's chat template (`pin_medium.py` was a pilot hack; restore `*.orig-xhigh`).
  If the app cannot do it, the fix is in goose or the fork, released and installed.
- **Before any local-model run, pass the MODEL READINESS GATE (top of this file) and write down:**
  - the app version;
  - the engine argv from `ps`;
  - whether `mtp.safetensors` exists;
  - the provider + host in the Benchmark form;
  - one rendered prompt showing the effort.
