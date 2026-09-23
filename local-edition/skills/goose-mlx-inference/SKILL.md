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
- `crates/goose-sidecar` — supervisor (spawn/health/restart+breaker/per-pid kill), Rust MemoryGate
  (parity with gates.py G1), `hf.rs` (MLX HF search `filter=mlx`, snapshot downloads with
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
