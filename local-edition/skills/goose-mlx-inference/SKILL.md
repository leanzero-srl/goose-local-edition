---
name: goose-mlx-inference
description: Source of truth for running ANY local model through goose with the LeanZero MLX engine (Rapid-MLX fork; pin = ENGINE_LAUNCHER in crates/goose-sidecar/src/engine.rs) on the laptop, workhorse, workmule or Gabee. Load BEFORE preparing a model for use or benchmark (MODEL READINESS GATE - right build, vision/MTP/n-gram served end to end, max MEASURED context incl. YaRN), before launching rapid-mlx by hand, writing a proxy or custom provider, or running Gauntlet / Forge 1.0 / 2.0 on a local, candidate or fine-tuned model. Covers providers (swarm/omlx for chat, built-in openai at the loopback endpoint for benchmarks), the app engine argv, thinking/effort, long-context caching and the memory guard; operating and evolving the engine (crates/goose-sidecar, the fork, bake-offs), the DISTRIBUTED engine (JACCL/ring split, qwen4_exp pipeline, rank launcher) and REMOTE SINGLE over LeanZero Link. Raw two-Mac MLX/JACCL layer: mlx-jaccl-cluster.
---

# goose-mlx-inference: CORE CARD

Read this card whole. It holds every hard rule, the Definitions of Done and the gates. The references are LOOKUP:
open only the file the index below names for your task. This card wins over any reference line. Dated history:
references/CHANGELOG.md (never read it to decide what to do today).

## HARD RULES
1. **Read the engine pin, never remember it.** `ENGINE_LAUNCHER` in `crates/goose-sidecar/src/engine.rs` on the
   branch you build (main and the release branches differ). Installed app:
   `strings "/Applications/Goose Swarm.app/Contents/Resources/bin/goose" | grep -o 'Rapid-MLX@[^ "]*'`.
2. **The engine is mounted BY THE APP, never typed.** Its argv comes from `build_serve_command` (engine.rs) and the
   pin's profile (`engine_profile.rs` `PINNED_PROFILES`). Write down the real argv from `ps`, never a remembered one.
3. **NEVER, for anything compared or published:** a hand-typed `rapid-mlx serve`; a custom provider or a
   request-rewriting proxy; `run_build.py` headless; an edited chat template. If the app cannot do it, fix goose or
   the fork, release, install.
4. **Providers:** chat = `swarm` (routes MLX turns through `omlx`). Benchmarks (Gauntlet / Forge 1.0 / 2.0) = the
   app's Benchmark view only (CDP when unattended), entrant "Single model", provider `openai` with its host at the
   engine's loopback endpoint. `swarm`/`omlx` are refused by the bench fence. Never `node-run` a benchmark.
5. **MODEL READINESS GATE before any local benchmark AND before putting a model into use** (section below).
   A step that cannot pass is a CAVEAT next to the score, never silence.
6. **A measurement counts only under the real conditions.** Context, speed or memory "proven" with
   `--disable-prefix-cache`, cold single requests or a different argv is a PROXY: say so, and re-measure with the
   app's exact argv in a session that GROWS to the target before relaying a number (the 10-29 "400k proven" ran cold;
   the real run was refused at 241,320).
7. **Hands off a live run's engine.** While a benchmark or long run is live on an engine: read-only
   `GET /v1/status` and `/v1/models` and the engine log only. No generation requests, no probes, no remount, no
   wrapper/REV swap, no app quit or install. Engine proofs and replays run BETWEEN series with the app quit, or on
   another Mac (sources: references/CHANGELOG.md 2026-10-18 audit "GET /v1/status only", "PROVE IT LIVE (between
   series, app quit; never mid-run)"; goose-benchmark-iteration "never kill the app or install a release while a
   run is live").
8. **Before ANY model mount or bench:** `python3 local-edition/mlx/gates.py mount --model-path <dir> --port <p>`
   (exit 1 = BLOCKED, stop). Session start `gates.py snapshot`, session end `gates.py verify-fleet`.
9. **LM Studio / LM Link / `lms` are never removed or reconfigured.** Sidecar ports never 1234/11434. Tests mount
   qwen3.5-9b 4-bit only, downloaded through our own path; the fleet's 27B is off-limits for test mounts.
10. **Weights live on the workhorse SSD** (`/Volumes/AI-workhorse/models/goose/...`; laptop = read-only NFS mount over
   Thunderbolt). Download on the workhorse. "Model gone" needs a positive control: the same dir lists a real
   `publisher/name/config.json`. SSD writes pass `~/Projects/machine-sync/bin/ai-disk-guard --need-gb N --out <dir>`.
11. **One Goose app per Mac** (`~/Projects/machine-sync/bin/goose-single-guard --check`; recipe in goose-feature-dev).
12. **Git identity on the goose repo: `leanzero.srl <office@leanzero.net>`** (public repo). Edit this skill in
   `~/Projects/goose/local-edition/skills/goose-mlx-inference` (the global path is a symlink) and COMMIT it there.
13. **Claims need evidence you checked yourself.** An agent's "fixed / green / proven" is a claim until one piece of
   hard evidence is re-checked independently (a test you re-ran, the log line, the served `/v1/models`). Hand agents
   facts and files, never your hypothesis as fact; answer "why" from the engine log and telemetry, not memory.
14. **Clean up when done:** `bash ~/.claude/skills/goose-clean/scripts/after-work.sh` (`--go` deletes pure caches),
   then this skill's leftovers by EXACT path (references/operations.md). KEEP `~/.cache/rapid-mlx/prefix_cache`.

## MODEL READINESS GATE (full text, evidence and traps: references/readiness-gate.md)
Write each answer into the run's NOTES.md / parity file.
1. **Right build?** Provenance + HF commit pin; every MLX build compared (quant recipe of sensitive layers, card
   "Status": "experimental"/"text-only"/"not enabled" = red flag); tokenizer/chat_template/generation_config
   byte-identical to official; sha256 of every file. Ledger row. Delete the loser by exact path.
2. **Every capability served END TO END:** `/v1/models` lists vision under the served name; a real image described
   correctly; MTP and prompt-lookup counters move in `/v1/status` (also under sampling, temperature 1.0); PLE in
   Metal memory. `--text-only` on a model with a tower = FAIL.
3. **Context = largest the model AND machine hold, MEASURED:** native + YaRN (`rope_scaling: yarn` profile key on
   qwen4_exp); stepped needle + doc_quote (128k/262k/300k/400k/512k) with the app's exact argv, prefix cache on,
   growing multi-turn session; record TTFT, peak Metal GB, swap; operating point = largest step clear of the Metal
   ceiling (workmule ~115 GB, workhorse ~77, laptop ~115, Gabee ~48). Then set `GOOSE_CONTEXT_LIMIT` (and
   `context_limit`) to what the memory guard ADMITS. Never leave goose's 128k default on a model proven for more.
4. **Speed levers on and proven lossless** (MTP, n-gram, PLE, prefix cache): greedy A/B at 8k and at the operating
   context, code_edit byte-identical, streamed == non-streamed bytes. A lever that changes output or gains nothing
   stays OFF with a ledger line.
5. **Machinery + memory tunables (lz.20+):** Mac or split from the measured peak (node-probe; Studios first). One
   Goose app per Mac. Then, on THAT Mac: (a) device spec first: `sysctl hw.memsize iogpu.wired_limit_mb`, MLX working
   set, `memory_pressure`, and after mount `/v1/status memory_budget` (limit_bytes, machine breakdown); (b) set every
   tunable ON PURPOSE in Settings > LeanZero MLX > Memory (config `mlx_engine.memory.<key>` -> engine flag):
   metal_limit_gb -> --metal-limit-gb (blank = auto), memory_floor_gb -> --memory-floor-gb (default 5% of RAM),
   prefix_cache_gb -> --cache-memory-mb, prefill_step_size -> --prefill-step-size, max_context_tokens ->
   --max-model-len, guard_margin_gb -> --memory-guard-margin-gb, max_concurrent_requests -> --max-concurrent-requests,
   auto_utilization_floor/ceiling -> --auto-utilization-floor/-ceiling, auto_headroom_fraction ->
   --auto-headroom-fraction, system_reserve_fraction -> --system-reserve-fraction; macOS wired limit via the same
   tab's button (admin password); (c) record in parity: `memory_tunables_gate.py emit --engine-port <p>` (15 rows);
   (d) verify via `/v1/status` memory_budget (tunables + limit_bytes + effective_ceiling_bytes). Full text:
   references/readiness-gate.md step 5.
6. **Benchmark only:** parity table vs the cloud entrant (context, vision, thinking sent, sampling, tool parser,
   output cap, timeouts), every row PASS before Run:
   `python3 ~/goose-builds/loop-state/local-parity/parity_check.py --port <p> --config <root>/config/config.yaml --cloud-input-modalities text,image --min-context <cloud peak; sb-7.2 = 322851>`.
   Also record: app version, engine argv from `ps`, `mtp.safetensors` present, Benchmark-form provider + host, one
   rendered prompt showing the effort. Known gap: the tier's effort pin does not reach a local model (T12 G1): caveat.

## DEFINITION OF DONE
- **Model ready / benchmark launched:** all six gate steps written in NOTES.md with evidence paths; parity_check exit 0;
  `long_run_gate.py` passed (LONG RUNS below); a vigil running. Not done: "the engine accepted the prompt".
- **Engine pin bump:** fork tag pushed; `ENGINE_LAUNCHER` bumped AND the old one appended to
  `SUPERSEDED_ENGINE_LAUNCHERS`; a `PINNED_PROFILES` row (engine_profile.rs); `PIPELINE_SINGLE_LINE_REVIEWED_THROUGH`
  (distributed/provision.rs) + the fork manifest's `reviewed_through`; both argv goldens in engine.rs; proven by
  `uvx --from git+…@<tag> rapid-mlx serve --help`, `/v1/status` fields, and the ignored live test
  `cargo test -p goose-sidecar -- --ignored live_mount_of_the_real_engine`. Runbook: references/engine-and-layout.md.
- **Engine fix:** red/green test in the fork; a replay of recorded requests with the app's argv between series;
  one row in `local-edition/mlx/experiments.jsonl` (`void_reason` when not a pass) + a dated `LEDGER.md` entry;
  `NOW.md` updated in the same commit; independent check of the claim (rule 13).
- **Skill edit:** committed in the goose repo; `~/Projects/machine-sync/bin/skill-lint goose-mlx-inference` exit 0.

## GATES IN CODE (each refuses with a non-zero exit)
| gate | command | refuses |
|---|---|---|
| mount gate | `python3 local-edition/mlx/gates.py mount --model-path <dir> --port <p>` | a mount that breaks fleet rules (self-test: `gates_selftest.py`) |
| parity | `~/goose-builds/loop-state/local-parity/parity_check.py ...` | any parity row FAIL |
| long run | `~/goose-builds/loop-state/long_run_gate.py` | a long run without CAUSE + VIGIL |
| one Goose | PreToolUse hook `goose-single-guard` | a second Goose app launch |
| SSD floor | `ai-disk-guard --need-gb N --out <dir>` | < 100 GB + N free, or a dump path |
| skill shape | `~/Projects/machine-sync/bin/skill-lint <skill>` | CORE card > 200 lines, missing DoD/index, nested or TOC-less refs |
| commit message | commit-msg hooks in goose and Rapid-MLX | an AI credit line (Co-Authored-By: Claude) |
Prose only (no code gate yet): rules 3, 6, 7, 13 (proxy measurement, live-run hands-off, claim verification).

## LONG RUNS (>30 min)
Rule home: ~/.claude/CLAUDE.md "ANYTHING OVER 30 MINUTES NEEDS CAUSE, A CHEAPER-PATH CHECK AND A VIGIL" (2026-10-30). Benchmark launches also pass the coordinator's `~/goose-builds/loop-state/long_run_gate.py`. This skill's specifics:
- **Long jobs:** (1) stepped context proofs on the target Mac (2026-10-29 YaRN, workmule, ~/Projects/machine-sync/bin/node-run): needle 400k TTFT 1,048 s, 512k TTFT 2,292-2,626 s per cell (/Volumes/AI-workhorse/experiments/2026-10-29-qwen4-yarn/NOTES.md). (2) Session replay with the app's argv: (# pin: a context proof measures the target Mac's own engine) `~/Projects/machine-sync/bin/node-run --host workmule --gpu --heavy --max-rss-pct 88 -- 'TRACE=1 REPLAY=.../session1.json zsh /Volumes/AI-workhorse/runs/2026-10-29-flash-vision/reclaim/proof.sh rep7 46a7848bb 409600 9000'` = 65 min (00:00-01:05, runs/2026-10-29-flash-vision/reclaim/rep7/NOTES.md). (3) The local Gauntlet the gate prepares for: 3h53m (Flash-Next ple.6) to 4h32m (MiMo split). (4) Fork test suite ~25 min (under the bar unless chained).
- **Past incident:** 2026-10-29 the YaRN proof ran COLD with `--disable-prefix-cache` and showed 400k at 96 GB; the Gauntlet then ran the app's argv (prefix cache on) and the memory guard, still on its 192 B/(chunk*ctx) prior after 2 h, refused 241,320 tokens ("needs 104.8 GB, this Mac can hold 103.9 GB"), goose compacted at 59 % of the 409,600 given, and the run was stopped. Earlier the same day the rapid-mlx build ran blind (0.0065). A grow-to-target replay with the app's argv (rep7, 65 min) found the real warm maximum (~365k) the long runs had not.
- **Cheaper probe first:** before any long proof or run, on the mounted engine (about 2 min): `curl -s localhost:<port>/v1/models` (vision listed under the SERVED name, `context_window`), `curl -s localhost:<port>/v1/status` (`memory_guard.peak_model.source`, MTP and prompt-lookup counters moving after one request), one real image described correctly; then `python3 ~/goose-builds/loop-state/local-parity/parity_check.py --port <port> --config <root>/config/config.yaml --min-context <target>` (about 1 min). Only the context question needs the replay, and the replay replaces a Gauntlet, never the other way round.
- **Front-load (first 30 min):** order a context proof so the hardest cell runs first under the app's exact argv (prefix cache on, `--hybrid-cache-entries 24`, `--max-concurrent-requests 8`): grow a multi-turn session straight to the target band instead of climbing 128k -> 262k -> 400k cold; the first image request and the first sampled (temperature 1.0) request with n-gram proposals > 0 happen in the first minutes, not after the long cells.
- **Vigil checklist:** run by a separate supervisor agent (or a 15-min cron tick in the coordinator session), never by the session that launched the proof; every 15 min one line in the run's VIGIL.md:
  - engine log `<GOOSE_USER_DATA_DIR>/state/logs/mlx-engine/mlx-engine-<ts>.log` (proofs: the job's steps.txt + `~/Projects/machine-sync/bin/node-jobs log <id>`): `[memory_guard] enabled: prior ...` at boot, then the peak model leaves the prior (`/v1/status` source "calibrated" or measured groups); `[schedule] ... tokens_to_prefill=N, M cached` with M advancing every turn; `Chat completion (stream): ... tok/s`.
  - absent: `needs N GB but this Mac can hold`, `skipped storing`, a hit frozen at one value (rep6b froze at 200,785), `cold requeue` on every turn.
  - per proof cell: needle FOUND with the expected sha, peak Metal GB below the Mac's ceiling (workmule ~115, workhorse ~77, laptop ~115, Gabee ~48), swap flat.
- **Kill criteria:** a refusal below the configured context; swap growing (stop the step before it, per the readiness gate); peak within 1 GB of the device ceiling (rep7 peaked 103.78 vs 103.90); TTFT past 1,000 s on a warm turn or a cold requeue every turn (1,560 s at 373k); a greedy A/B that is not byte-identical (lever stays OFF); n-gram proposals 0 under sampling.

## REFERENCE INDEX (one level deep; open only what your task needs)
| file | read when |
|---|---|
| references/readiness-gate.md | preparing a model; context/YaRN/memory-guard traps with numbers (Flash-Next, MiMo) |
| references/running-a-local-model.md | engine argv details, MTP sidecar layout, providers, long single sessions, the effort gap |
| references/memory-guard.md | OOM, refusals ("needs N GB"), cache sizing, guard design and measurements |
| references/engine-and-layout.md | campaign, engine verdict, upstream draw + pin bump runbook, where the code lives |
| references/providers-and-thinking.md | Swarm provider surface, thinking/effort controls per model |
| references/engine-features.md | KV-cache compression, MTP logprobs, warm vs cold census, placement planner |
| references/app-surface.md | live tile/tray, mount refusal + load progress, macOS available memory, notarized release, tray CDP check |
| references/distributed-engine.md | the split across Macs (JACCL/ring, qwen4_exp pipeline, MiMo split, vision on split) |
| references/distributed-engine-history.md | dated 09-23..26 record of the distributed engine |
| references/remote-single-and-engine-fixes.md | remote single over Link, hidden generation caps, Q-85/142/508 |
| references/evolution-loop.md | experiments.jsonl / LEDGER / QUEUED-FIXES / TRUMP-CARDS / NOW.md loop, fan protocol, one-home rule |
| references/compaction.md | goose compacting a local-model session: when, what the summary lost, the compaction-fix design, review fixes, and round 4 (provider-anchored guard, facts beside every summary, structural gate) |
| references/prompt-lookup-ngram.md | the prompt-lookup (n-gram) lever record |
| references/qwen4-flash-offload.md | Qwen3.8-Flash-Next PLE offload facts (tag check, laptop blocker, sqlx strip trap) |
| references/hard-rules-detail.md | full text and WHY of rules 8-12 (weights mount, restore at launch, swarm-surgeon) |
| references/long-runs.md | copy of the LONG RUNS section above |
| references/omni-media.md | audio/video input: which omni model works on which mlx-vlm, the wire path, traps |
| references/operations.md | clean-up leftovers list |
| references/CHANGELOG.md | dated diary 2026-10-17 onward: per-model results (RC13, Nemotron, Gemma 4, Granite, Mellum...) |
Grep across all: `grep -rn "<term>" ~/.claude/skills/goose-mlx-inference/references/`.
