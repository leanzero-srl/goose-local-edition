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
- THE MEMORY HOLD IS WAITED OUT, NOT RETRIED (Q-397, 1d97681d2): the watchdog's close sends `code: "memory_hold"`
  (`distributed::MEMORY_HOLD_CODE`); rank 0's 503 carries `error.code`/`reason`/`admission: "/goose/admission"`, and
  `GET /goose/admission` parks until the reopen (rank_admission.py). goose keys on the CODE (never the prose) →
  `ProviderError::EngineHold` → `engine_hold::wait_for_admission` (no clock; Stop drops it; engine gone = loud error);
  the router fails a held node over first (`HOLD_GOES_TO_CALLER`) and waits only when nothing else serves; the turn line
  reads "Waiting: <watchdog reason>". Any other 503 keeps the 3 retries. Probe: `curl -s 127.0.0.1:<port>/goose/admission`
  answers `{"admission_open": true}` at once when open (it BLOCKS while held — never run it without `-m` on a held split
  unless you mean to wait). Gaps: the pipeline runner's fork 503 has no code (Q-398); a peer's split over the Link relay
  cannot be waited on (relay routes chat/models/status only, Q-399).
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
- PROMPT CACHE ON THE READ SURFACES (Q-337/Q-338, 2026-09-28): `prefilled_tokens` is the prompt POSITION, the
  restored prefix INCLUDED, in both split modes — mlx_lm 0.31.3's own progress counts only the tokens past the
  prefix (`total` = len(rest)), so rank_wrapper.py adds `prompt_tokens - total` back (before 9cdf7455c a warm
  tensor read showed ≤1% and no prefill rate). Rapid-MLX's single engine sends per-request `cached_tokens` but
  it is the Request default 0 until `cache_hit_type` is set — the parser (`cachedTokensOf`) reads that as
  UNKNOWN. One split for every surface: engineFigures.ts `promptCacheOf` / `readBarOf`, drawn by
  PromptReadBar.tsx (teal cached part + reading-blue new part; `--color-lz-cache`, `-on-fill`).
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
- TOOL CALLS STREAM ON EVERY WAY (Q-141, 2026-09-26). mlx_lm 0.31.3's `handle_completion` sends NOTHING while its
  state machine is in "tool" (`gen.state != "tool"`, server.py:1478) — E2E #3c's rank 0 generated 21,910 tokens of one
  call (RANK_STATE uid 15, ended "removed") with zero chunks at goose. rank_tool_stream.py + the relay in
  rank_wrapper.py (`StreamedToolCalls`, wraps mlx_lm's own handle_completion) send the open frame (id, name, `{`) and
  argument fragments as OpenAI `tool_calls` deltas (the pipeline did NOT, until lz-pipeline-qwen4.13 — below); the end reconciles against
  `qwen3_coder.parse_tool_call` on the whole text and sends the remainder, so the call is byte-identical; on a
  refusal nothing more is sent (the client fails the call; rank log `GOOSE_RANK_TOOL_CALL_UNPARSED`). Only
  string-typed values stream before their close (typed ones are converted whole); a call whose text never forms
  `<function=NAME>` stays silent until it ends (the fork does the same). goose's forming line names the tool
  ("goose is writing a tool call to shell — 4.2k chars of arguments", FormingProgress.writing). Tests:
  `the_tool_stream_sends_exactly_what_mlx_lms_parser_reads`, `a_streamed_tool_call_reaches_the_client_while_it_is_written`
  (negative control: unpatched handler, 1 frame after the close).
- TOOL PARAMETER TYPES ON THE SPLIT (Q-232, eae39751e, 2026-09-28). mlx_lm 0.31.3's qwen3_coder reads `str(param["type"])`: anything but a plain type name goes to `ast.literal_eval` (`10m` under [string,null] = SyntaxError, the call lost; `true` under [boolean,null] = ValueError) and a property with no `type` (a `$ref`) is a STRING (read_image's crop arrived as JSON text). Rapid-MLX's parser (single + pipeline, `api/tool_calling.py _schema_type`) already reads these as their type. rank_tool_schema.py, installed over `qwen3_coder._get_arguments_config`, makes the split read them the same way. TRAP before re-filing: goose's own wire has NO top-level [T,null] — `create_request` → `normalize_nullable` (goose-provider-types openai.rs) rewrites them; audit the WIRE (`~/.local/state/goose/logs/llm_request.*.jsonl` `input.tools`), not the schemars output. Repro any parser claim against the installed envs (tensor ~/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12, pipeline rapid-mlx-pipeline-qwen4-py3.12, single = the uvx archive whose rapid_mlx dist-info direct_url names the ENGINE_LAUNCHER tag) with PYTHONDONTWRITEBYTECODE=1.
- A REFUSED CALL ON THE SPLIT IS A NAMED 500 (Q-233, d7a318b5b, 2026-09-28). mlx_lm buffers a non-streamed answer's 200 BEFORE formatting it, and its ToolCallFormatter skips only ValueError: a SyntaxError dropped the connection (RemoteDisconnected), a ValueError dropped the CALL from a 200. The wrapper's NamedToolCallFormatter (non-streamed only) makes any parser refusal a 500 `code: tool_call_unparsed` with the parser's words + `tool_text`, and GOOSE_RANK_TOOL_CALL_UNPARSED `{"stream": false}` in the rank log. http.server rule: `_headers_buffer` NON-empty = status line buffered, nothing sent (discard it and answer); EMPTY = on the wire (re-raise). An mlx_lm name the wrapper patches also goes into the stand-in `mlx_lm/server.py` in launch.rs's tests (three tests went red on `ToolCallFormatter`).
- WHICH LAYER LOST THE TEXT? READ THE RANK'S TOKEN TRAIL (Q-371, 2026-09-28). Every split rank logs `GOOSE_RANK_STATE` every ~2 s with, per generating row, `generated` + `crc` (zlib.crc32 chained over each sampled token id as 4 little-endian bytes, rank_state.py `TokenTrail`) + `checkpoints` at every power of two, and `ended` for the finished row — rank 0 in `~/.local/state/goose/logs/distributed/rank0-*.log`, rank 1 on the Studio (`ssh workhorse 'grep GOOSE_RANK_STATE ~/.local/state/goose/logs/distributed/rank1-*.log'`). ~300 (generated, crc) pairs per 3k-token answer. Rebuild the sampled sequence from what goose STORED (the answer's text message + the call rendered in the qwen3_coder wire + `<|im_end|>`, tokenized with the model's tokenizer) and fold it: every pair matching = nothing between the sampler and sessions.db changed a byte; a window that fails = brute-force that window's segmentations (the model sometimes spells non-canonically: ` ((`+`(_`, `in`+`grid`). #3r's `write` (771288, uid 90, 3,025 tokens): all 314 pairs matched the STORED arrow-less text, ` =>` sampled once; the arrows-restored control mismatched 240 — the model never wrote them. The first 512 tokens' brute force also names the answer's first token (`The`), which ties a uid to its message. llm_request.N.jsonl is a 10-file ring: the raw stream of a call an hour old is gone, the trail is not.
- A VALUE HOLDING `</parameter>` ARRIVES WHOLE ON THE SPLIT (Q-372, 2026-09-28). mlx_lm 0.31.3's qwen3_coder ended a value at its FIRST `</parameter>` and still returned the call — `const close = "</parameter>";` in a `write` became a file ending at `const close = "`, silently (the Q-141 streamer mirrored it). rank_tool_stream.py `install_positional_parameters` (installed by the wrapper after Q-232's) reads values the single engine's way (Rapid-MLX `tool_call_scan.py`): a value runs to the next header whose name the tool DECLARES with a `</parameter>` before it, and ends at the last `</parameter>` before that header or the call's end. The streamer streams a value up to its first `</parameter>` and finishes it when the sibling header or the call's end places it; text after an in-value `</parameter>` that is not the next frame is withheld as `tool_close_pending` (GOOSE_RANK_WITHHELD). Residual ambiguity, same as the single engine: in-value text `</parameter>…<parameter=DECLARED>` IS read as the next parameter. Test: `a_written_file_crosses_the_tensor_tool_path_byte_for_byte` (its negative control asserts mlx_lm's shipped cut).
- THE XML GUARD CLOSES A VALUE WHOSE LINE STARTS WITH `</parameter>` (Q-373, measured offline 2026-09-28): rank_xml_guard.py's after-close rule arms on a line-start `</parameter>` inside a call, so a file whose own text has such a line (XML/docs about this wire) can only continue `\n<parameter`/`\n</function>` — the model is forced to end the value there. A mid-line `</parameter>` (a string literal) does not arm it. Probe: feed the tokens of `<tool_call>\n<function=write>…<parameter=content>\n<config>\n</parameter>` to `XmlSkeletonGuard` with the model's tokenizer and read what survives the mask (`['\n']`).
- A STALE RUNNER ENV IS REBUILT BY THE NEXT START OF ITS RUNNER, NOT BEFORE (Q-234, a60f985f9, 2026-09-28). An env behind its pin is normal until a start of THAT runner: the pipeline env is proven only on a Flash start (runnerEnv FAIL → update_runners → `uv pip install` in place; uv 0.11.7 and 0.11.28 both measured replacing the fork's commit). The env's commit: `cat ~/.goose/distributed/<env>/lib/python3.12/site-packages/rapid_mlx-*.dist-info/direct_url.json`; the app's pin: `strings "/Applications/Goose Swarm.app/Contents/Resources/bin/goose" | grep -o 'Rapid-MLX@[0-9a-f]\{40\}'`. A Link peer builds ITS goose's pin: two Macs on different goose versions now fail the update by name (PinWatch) — install the same goose on both.
- TESTS WAIT ON EVENTS (Q-235, 62763b394). A Link rank test waits for the join / Lost / Restored or the rank's end, at the relay's READY_TICK — never `for _ in 0..N { sleep }`. Load check: `yes` hogs (killed per pid) beside the full lib suite.
- READ WHAT A SPLIT ANSWER WITHHOLDS (Q-146 instrument 56e1487ba, 2026-09-26). E2E #3d's agent call generated 9,945+
  tokens in 17 min and sent nothing. `curl -s localhost:<port>/v1/status | jq '.requests[].stream'` on rank 0 now shows
  per streamed chat request: `parser_state`, `generated_chars` / `sent_chars` / `since_sent_chars` (since the last frame
  with content), `withholding` {mode, reason, withheld_chars}, `tool_call` (the Q-141 streamer's phase, parameter,
  string_value, broken), `tail` (last 2,000 chars, control sequences as written); `stream: null` = not a streamed chat.
  Modes: tool_not_streamed · tool_broken · tool_unread (JSON inside <tool_call>, prose between parameters) ·
  tool_close_pending (Q-372: a value met `</parameter>` and what follows is not the next frame — more of the value, or
  text the parser drops) · tool_typed_value (non-string values go out whole once the next declared header or the
  call's end closes them). Rank log: `GOOSE_RANK_WITHHELD` enter/leave, the
  leave carrying the words. rank_stream_watch.py (StreamWatch); `take` in the wrapper's counted(), frames counted by
  wrapping the handler's generate_response. TRAP: /v1/status `completion_tokens` is counted in the HANDLER thread as it
  consumes — a high count with nothing on the socket means the handler is withholding, not back-pressure. The full
  withholding-path table (which paths Q-141 covers) is in FINDINGS-LEDGER Q-146. Test:
  `the_status_names_what_a_streamed_answer_withholds`. Test runs against the live venv: prefix
  `PYTHONDONTWRITEBYTECODE=1` so nothing is written into ~/.goose/distributed.
- THE PIPELINE'S OWN STREAM (Q-178/Q-179, fork lz-pipeline-qwen4.13 = 1b43e84a0, 2026-09-27). Its route-layer
  StreamingPostProcessor sends a qwen3_coder_xml call's header + `{` then — once a string value is unquoted
  (`_legacy_raw_stream`, upstream #1515) — NOTHING until the answer ends (E2E #5b: "1 chars of arguments" for 4,624
  tokens; reproduce on CPU with the Flash tokenizer from ~/.goose/models + the fork's postprocessor). The fork's
  `pipeline_stream.py` StreamRelay reads the checkpoint's single-token markers by ID (a BPE-held space arrives with
  the marker: ' <tool_call>'), owns a declared call (streamed, verified at close against `extract_tool_calls`), hands
  undeclared/unreadable calls to the postprocessor unchanged, and carries the Q-146 `stream` report (`_Job.stream`;
  pipeline_rank.py puts it on each /v1/status row) + the Q-161 repeat/text-cycle stops (`last_engine_stop`). Its
  reader is Rapid-MLX's parser, NOT mlx_lm's: a value ends at the LAST `</parameter>` before the next DECLARED
  parameter; a JSON-string value is decoded. PREFIX CACHE, one copy: a row keeps a boundary record (GDN states + QSA
  rings, 42/74 MB on Flash) and its own cache cut to the boundary becomes the entry when it leaves; the old copy
  beside the batch stopped fitting at 53k (rank 1 room 0.91 GB beside a second full-context row). The step after a
  big answer still reads that answer once (goose's transient tail sits after the boundary). TRAPS: goose's
  pipeline_rank.py `_start` wrapper must pass the fork's extra args through (`*directives`); fork tests that
  import `test_pipeline_qwen4_serve.py` launch ranks — the in-process ones live in `_stream_parity`, `_prefix_adopt`,
  `_row_guard`, `_continuous`, `_srpf`; the real-fork goose tests need `GOOSE_TEST_PIPELINE_PYTHON` = a venv at the
  pin (never the live ~/.goose/distributed env while a split runs).
- A CLIENT THAT LEAVES ENDS ITS ROW (Q-181, 2026-09-27; tensor rank_wrapper.py `client_left`, fork
  lz-pipeline-qwen4.14 = 8c0007054 `watch_client`). mlx_lm 0.31.3 notices a closed connection ONLY when a write
  raises — and nothing is written while text is withheld (typed value, held repeat, mlx_lm's own "tool" state) or
  while a NON-streamed answer runs: goose's Stop left the split generating 48,466 tokens / 4,413 s for nobody. The
  tensor handler now peeks the socket (zero-wait poll + MSG_PEEK: EOF or reset) at every progress report and token,
  then `ctx.stop()` → rank 0's `uids_to_remove` → every rank removes the row next step. The pipeline awaits
  `http.disconnect` beside every chat answer (uvicorn's h11 reads EOF → connection_lost); MEASURED before: streamed
  was already cancelled by Starlette's own listener (ASGI spec 2.3 — at 2.4 Starlette waits for a failed write
  too), whole answers ran on (201 → 3,087 tokens in 3 s). Both name it: `last_engine_stop.reason` =
  `cancelled_by_client` (+ phase, how) and a GOOSE_RANK_CANCELLED_BY_CLIENT line on both runners. A
  split still generating with no ESTABLISHED client on its port is this bug back. Fork CPU tests: force
  `mx.set_default_device(mx.cpu)` with a `-p` plugin and skip `test_pipeline_qwen4.py`, `_serve.py`, `_vision.py`
  (they launch ranks); a scratch dir holding a `bisect.py` shadows the stdlib — run scratch scripts from a subdir.
- A DROPPED ROW LEAVES AT THE NEXT PROMPT STEP, AND EVERY HELD ROW IS LISTED (Q-231, 2026-09-27; tensor
  tag `mlxLmServerPrefillYield`, spec `prefill_step_yields`). Q-181's "every rank removes the row next
  step" was FALSE on a group: mlx_lm 0.31.3 removes stopped rows and takes new requests only after its
  TimeBudget loop, and on a distributed group that budget is a COUNT of steps fitted to 0.5 s of DECODE
  (~5 on the 27B split) — five prefill slices of seconds each. E2E #3m: goose's turn_priority dropped
  three end-of-turn fact checks (1,775/1,770/5,453 tok) at 20:16:17.067Z; the loop read them to the end
  (`steps` 1116 frozen on BOTH ranks 20:16:14→20:16:59Z, prompt cache +3 user segments, uid 29
  "generated 1, removed") and the user's 88,660-tok call queued 38.9 s while /v1/status listed nothing
  but it. Now `PromptStepBudget` ends the loop after any step that read prompt tokens (every rank alike —
  hence the tag: a peer on the older tag REFUSES the rank, so BOTH Macs need the new goose), and
  `DepartureContext` makes the loop's own `_should_stop` read the client socket and wake a handler
  already waiting (`ClientDeparted` on its answer queue, captured by `RequestTap`) so the stop is still
  named. /v1/status rows add `client` ("host:port"), `held_for_room`, `stopped` (last_engine_stop shape),
  `stopped_after_s`, `leaving` (answer ended, row still in the batch); `num_running` counts leaving rows;
  GOOSE_RANK_STATE `requests` {uid: req id}; GOOSE_RANK_ROW_LEFT {request_id, uid, how, stopped,
  held_after_stop_s, held_after_answer_s} — the live prove's number (expect < one prefill slice). A
  request stays listed after its handler left ONLY while its id is in `in_batch` (never on an absence of
  word from the loop — the first version ghosted every stand-in request). TRAPS: a test that builds
  mlx_lm's `TimeBudget()` hangs forever in `jaccl::TCPSocket::accept` (mx.distributed.init under the
  spec's JACCL env) — build it with `__new__` at its signature's defaults; the stand-in mlx_lm module
  in launch.rs's rank-program tests must grow every seam the wrapper's guard list checks.
- TURN-START CACHE MISSES HAVE THREE GOOSE-SIDE CAUSES (Q-294, 2026-09-28, E2E #3o on 3.0.66). HOW TO MEASURE
  ONE: (1) the rank0 log prints `Prompt Cache: N sequences, X GB` + per type right before EVERY fetch, and
  `Prompt processing progress: a/b` where b = the tokens left to read (b = prompt → a full miss); (2) copy
  `~/.local/state/goose/logs/llm_request.[0-9].jsonl` on every mtime change (only the last 10 calls survive)
  and diff consecutive MAIN calls message by message (first differing index/char); (3) sessions.db
  (`?mode=ro`) rows whose text starts "[goose's record of an earlier tool call" + their `timestamp` say
  when a condensation was written; an entry of N tokens on the 27B split holds N × 32,768 + 76,972,032 B
  (RANK_ADMISSION `kept_prefix` 3,642,163,200 for `held_tokens` 108,801 — exact), so the per-type GB
  figures say which prefixes are still there.
  CAUSE A — tool-pair condensation (context_mgmt `maybe_summarize_tool_pairs`): once visible tool calls
  exceed cutoff+10 (cutoff 31 at 262k × 0.8) EVERY reply condenses the 10 oldest pairs into the session
  DB; the NEXT turn reloads them as records appended to an early user message → the prompt diverges at
  message ~2–5 (turn 6: msg 5 len 2,114 → 6,476) → turn 5 read 0/109,655, turn 6 40,244/102,210 (the
  system segment only), turn 7 0/108,801, each ~15 min cold, to save 4–12k tokens. Fix: a chat agent
  keeps its pairs while the provider reports cache reads (`prompt_cache_read`; swarm workers unchanged).
  CAUSE B — end-of-turn helpers (fact checks 1,452–23,686 tok, labels) batch together at turn end; mlx_lm
  types EVERY chat request's context segment "user", so a helper's took Q-182's newest-"user"
  protection, and `admits` left room only for a prefix as wide as the helper batch: turn 7's eight rows
  trimmed the cache below the agent's 3.96 GB prefix; turn 5's five left turn 4's unusable END entry
  (assistant 3.80 GB) and no prefix — A and B both hit turn 5, so each fix alone still misses there.
  Fix: tag `mlxLmServerConversationPrefix`, spec
  `keep_conversation_prefix` — the kept entry is the key a transient-tail request's boundary cut
  (`rank_boundary.py ConversationPrefix`, matched by length+hash at insert, held by identity), and rank
  0's `admits` leaves room for its bytes (RANK_ADMISSION `conversation_prefix`). Replay test
  `end_of_turn_helpers_leave_the_conversation_prefix_cached_through_real_mlx_lm` (each half alone
  still reads 0). CAUSE C — a reloaded tool ERROR renders differently from the turn that produced it:
  `tool_result_serde` stored `ErrorData` as its Display ("-32002: Tool 'read_file' not found…") and read
  it back as INTERNAL_ERROR with that whole text as message, so every later turn sent "-32603: -32002:
  …" (turn 3: 0/76,117 with the 74,913-token prefix still cached; turn 2 held the run's only
  status:"error" result). Fix: `stored_error` parses the code back. GENERAL RULE: anything goose sends
  that is rebuilt from the session DB at a turn start must render byte-identical to what the turn sent
  from memory — check a new field's serde round-trip against the formatter's output.
- COMPACTION IS THE CONVERSATION'S NEXT REQUEST (Q-342, 2026-09-28, E2E #3p on 3.0.69). The old summary
  request put the history as TEXT in the system message (transcript shape) → 0 of 97,590 read, 441 s of
  prefill before the first token, 678 s in all (12:21:28 → 12:32:46), while the rank still held the
  chat's ~142k prefix. Now (`context_mgmt::SummaryRequest::ExtendsChat`): the chat's own system prompt +
  tools (after disclosure) + messages through the chat's fix_conversation/`messages_for_provider`, then
  ONE instruction message (joined to a trailing user message; its own message after tool results), sent
  via `model_config::complete_as_the_chat` with the CHAT's model config — NOT the helper path. Transcript
  stays for swarm workers (golden), the context-length RECOVERY compaction, and a distinct fast model.
  THINKING SWITCH FACTS (measured on the split's wrapper in the real-mlx_lm replay; the single Rapid-MLX
  engine does the same per rank_thinking.py's account of its helpers.py): a request carrying tools with no
  pinned `enable_thinking` resolves thinking OFF (Q-135),
  so goose's chat requests render off and a helper's pinned-off renders identically; a switch the chat did
  NOT render (pinned ON) reads 0 — Qwen3.8 writes "Reasoning effort is set to xhigh…" into the SYSTEM block.
  Replay `a_tool_step_reads_the_prefix_before_the_turn_context_through_real_mlx_lm`: chat-shaped summary
  4,392 of 4,760 (= the last boundary), thinking-on 0 of 4,965, transcript 0 of 1,749. HOW TO MEASURE a
  compaction live: the in-flight `llm_request.<uuid>.jsonl` (copy it — it is renamed into the numbered
  rotation when it ends) holds the body; the rank log's `GOOSE_RANK_ADMISSION held_tokens` is its size,
  the `Prompt processing progress: a/b` b is the cold part, the `"ended": {"uid": …, "generated": N}`
  state line its end. OPEN after it: the first request after a compaction prefills system+tools cold
  (#3p: 44,053, 150 s — Q-347: mlx_lm snapshots a segment only past the tokens a request read from cache).
- ONE BUILDER FOR THE SYSTEM PROMPT (Q-346, 2026-09-28, b52f65f60). `Agent::prepare_tools_and_prompt(&Session)`
  (reply_parts.rs) is the only thing that builds a chat request's tools + system prompt: base template +
  extras (subdir hints, root hints) → toolshim rewrite → the session's PROJECT instructions last. The reply's
  first call, the mid-turn refresh (tools updated OR new subdir hints — hints load FIRST, one rebuild), the
  compaction's chat-extending request and the post-compaction count all call it. Before: the project text was
  appended only at the reply's start, so every refresh dropped it for the rest of the reply AND rewrote the
  prompt's tail (a prefix change → cold prefill). RULE: never append to `system_prompt` outside the builder;
  a part added anywhere else vanishes at the next refresh. Same inputs → byte-identical prompt (the date line
  is fixed per PromptManager). Tests: tests/agent.rs `a_hints_refresh_mid_turn_keeps_the_projects_instructions`,
  reply_parts `a_tools_refresh_changes_the_prompt_only_by_the_refreshed_extension`.
- THE CHAT'S HEAD IS A KEPT ENTRY OF ITS OWN (Q-347, 2026-09-28, fixed 4b16c7ba3, tag
  `mlxLmServerStableHead`, spec `keep_stable_head` — both Macs need the new goose). WHY: mlx_lm 0.31.3
  cuts a "system" segment only on a request ending on a user message and snapshots a segment only past
  what the request read from cache, so the head (system prompt + tools, #3p: 40,361 tokens = 1.40 GB)
  had ONE entry, the session's first request's, and its type-count eviction takes the oldest "system"
  entry once helpers' system segments (fact checker, reviewer, labeler — three distinct prompts)
  outnumber the rest: #3p 10:59:33, five minutes into the chat. After that the head only lived inside
  non-trimmable conversation entries; a compaction (or a new chat with the same tools) read 0. NOW:
  every agent request (naming its transient tail) ends a segment at mlx_lm's OWN system-segment end
  (`head_probe` = leading system messages + an empty user turn; `head_end`; `cut_at_head` — a tool step
  gains the cut, a user-ending request is unchanged); `KeptEntry` tracks it like the conversation
  prefix; `pop_keeping` holds both while anything else is left, the head going before the prefix; and
  `admits` leaves room for the head's measured bytes beside max(batch-wide prefix, conversation prefix).
  COST: one extra render+tokenize of the system block per agent request; 1–2 of an 8-helper burst wait
  once the prefix nears 4.74 GB (#3p's 11.83 GB plan). LIMIT: a lone row wider than (limit − prefix −
  head)/32,768 B (~171k tokens on #3p's plan) still takes the head first. HOW TO MEASURE the head:
  render the captured request with the model's own tokenizer (`AutoTokenizer.from_pretrained(<model
  dir>)`, `apply_chat_template(system + [user ""], tools=…, add_generation_prompt=False)` vs the full
  prompt → first differing token) UNDER THE SWITCH THE ENGINE RENDERED (`enable_thinking=False` for a
  tool-carrying request on the split, Q-135 — the template's default writes a reasoning line into the
  system block: 40,399 vs the true 40,361; check the full render equals the engine's prompt_tokens) — run the script from a dir with no `inspect.py` in it (a scratch
  `inspect.py` shadows the stdlib and transformers dies at import). LIVE: rank log system bytes stay ≥
  the head's through every burst; RANK_ADMISSION `stable_head`; the first post-compaction request's
  first progress ≈ prompt − head. Replays: `a_compacted_chat_reads_its_system_prompt_and_tools_from_
  the_cache_through_real_mlx_lm` (tiny server; control 0) and `a_compacted_chat_reads_its_head_at_e2e_
  3p_sizes_through_real_mlx_lm` (#3p sizes; control, eviction alone and room alone all 0). TRAP: a
  Rust `r#"…"#` test program must not contain `"#` (an f-string `f"#3p…"` ended the raw string).
- A COLD ROW JOINING A ROW MID-PREFILL TURNED THE SPLIT FLOAT32 (Q-447, 2026-09-28, fixed b49982c26, tag
  `mlxLmServerKvDtype`, spec `batch_kv_dtype` — BOTH MACS NEED THE NEW GOOSE: the dtype sets every
  tensor-parallel all_sum's byte count, a mixed pair would pair float32 with bfloat16). WHY: mlx_lm 0.31.3
  `BatchKVCache.extend` (models/cache.py:1060) pads a side with no KV via `mx.array([])` = FLOAT32; a cold
  request joining a prompt batch whose row already stepped (a helper at chat start) promotes the batch; the
  residual stream, every later layer and every cache entry go float32, and `merge` (first row's dtype)
  carries it to every restore — prefix chain and kept head — until a merge led by a bfloat16 row casts back.
  COST measured on the 27B split: 4.05–4.19 tok/s vs 10.35–12.26 on the same chat after the flip. DETECT
  FROM LOGS (no attach): entry bytes = H × 65,536 + 78,354,432 (float32) vs H × 32,768 + 76,972,032
  (bfloat16) — read `stable_head`/`conversation_prefix` in RANK_ADMISSION against the head position in the
  `Prompt processing progress` lines; decode tokens per `_generate` loop from RANK_STATE (steps vs
  trails.generated every 2 s: ~2 float32, 5–7 bfloat16); MLX free-buffer cache (RANK_MEM `cache`) ≤ 0.1 GB
  while decoding float32 vs 2–3.5 GB bfloat16. The rank now emits RANK_KV_DTYPE_MIXED if two dtypes ever
  meet in an extend/merge. The buffer-sharing/per-step-copy theory was REFUTED (0 buffer moves, pointer
  tracking). Replay: `a_cold_request_joining_a_row_mid_prefill_keeps_the_models_dtype_through_real_mlx_lm`
  (tiny qwen3_5 in BFLOAT16 — the float32 tiny model cannot show a promotion; the helper's first prompt
  step is held until the agent request is queued; CPU step times do NOT show the slowdown, float32 is not
  slower on the CPU). TRAP: numpy's buffer protocol refuses bfloat16 (`'oat16'` PEP 3118 error) — probe
  bf16 arrays by dtype/nbytes, not `np.array(x, copy=False)`.

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
- AUTO MUST MEAN THE SAME ON EVERY WAY (Q-135, 2026-09-26). Only Rapid-MLX has the auto-disable; mlx_lm.server
  (tensor) and the fork's `pipeline_qwen4 serve` hand an absent switch to the template, and Qwen3.8 reads
  undefined as ON at effort xhigh (+209-char "Reasoning effort is set to xhigh…" system line, `<think>\n` instead
  of `<think>\n\n</think>\n\n`). Measured: split turn 0 of the Jira brief 1021 s / 1195+ s, first agent call 7k /
  10.4k+ thinking tokens; single 170–178 s, 65–817 tokens, none thinking. Fix: `distributed/rank_thinking.py`
  (port of chat.py's effort-none → tools gate → casual gate → `_extract_thinking_from_request`), applied by
  rank_wrapper.py (`validate_model_parameters`, rank 0, before the request is shared) and pipeline_rank.py (goose's
  /v1/chat/completions over the fork's). Top-level graded `reasoning_effort` / `reasoning_max_tokens` /
  `reasoning{effort}` are REFUSED 400 `unsupported_parameter` on the split (the single engine translates them).
  Proof: `every_way_renders_the_same_prompt_for_the_same_setting` (launch.rs, pipeline env). Offline render
  recipe: tokenizer from the model dir on CPU (transformers, no weights) + `git archive <tag> rapid_mlx`.

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
  rank_wrapper.py wraps ResponseGenerator.generate; pipeline_rank.py extends the fork's
  _Job/_Engine._start/_Engine.prefill/_build_app — no fork change; since Q-134 (fork c8d6d5faf, lz-pipeline-qwen4.5)
  the pipeline admits CONTINUOUSLY: a queued request joins between decode steps when a slot is free and its KV fits,
  prefilling alone in its own cache — run_batch/_step-as-seam are gone, so a pin bump that renames a seam must move
  pipeline_rank.py and launch.rs's real-fork test with it (run it: GOOSE_TEST_PIPELINE_PYTHON=<a venv at the pin>). TRAP: mlx_lm's first prompt-progress report comes after its first chunks (30 s on a 15k prompt) —
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
- A HELD ENGINE PORT IS NAMED, AND STOPPED ONLY ON PROOF (Q-240, 984b74763, 2026-09-28). Before: a mount over a
  held port said "port N has an unsupervised listener — unmount/reclaim it first" (no pid), and the Sidecar start /
  crash-restart had NO check — a stand-in serving the expected id made `Sidecar::start` return Ok for a child that
  then died on the bind (measured red). Now every sidecar spawn carries `GOOSE_SIDECAR=<name>@<base_url>` (the
  engine's name is `engine::ENGINE_SIDECAR_NAME` "mlx-engine"; inherited uv → python, `ps -E -ww -p <pid>` shows it),
  and `port_holder::claim_port` runs before EVERY spawn: each LISTEN pid is read with its lineage and passes
  `ownership_proof` only if it is this uid, carries OUR marker, every same-group launcher above it does too, and the
  chain's top was started by init (its goosed is gone) or by this process — then it is stopped per pid (identity =
  start time + argv + marker re-read before each signal; SIGTERM, GRACE, SIGKILL). Anything else is `PortHeld` /
  `UnsupervisedListenerError` naming pid, full argv, the failed rule, the live starter ("quit what started it (pid
  N)") or `kill <pid>` — nothing signalled, no other port tried. TRAP: an engine started by a goose OLDER than
  984b74763 carries no marker and is refused, named — never reaped. Tests: tests/port_holders.rs (real orphans via a
  process-group `sh` that exits), engine `a_mount_stops_its_own_leftover_engine_and_names_one_it_may_not_stop`.
  THE PANEL READS THE SAME HOLDERS (Q-249, 2026-09-28): `status()` fills `stray_listener_holders` (pid, argv, ours,
  `not_ours_rule` = `NotOursRule::as_str` unreadable|initOrSelf|otherUser|otherEngine|noMarker|liveStarter, the
  reason, live starter pid+argv) — only while no mount is in flight (a start's own child is not a stray) — or
  `stray_listener_holders_error`. A new `NotOurs` arm needs a `NotOursRule` AND a catalog phrase, or the panel falls
  back to the raw reason.
- UNMOUNT STOPS ONLY WHAT THE PROOF CALLS OURS; ONE STEP FOR EVERY SURFACE; HOLDERS CACHED (Q-252/Q-251/Q-253,
  2026-09-28, bf7ef944f + 63a8d72b3). Before: Unmount's `reclaim_port` SIGTERMed EVERY LISTEN pid on the port (another
  goose's live engine, a terminal's server — red with real stand-ins) and the panel offered Unmount beside "Not this
  goose's". Now the reclaim IS `port_holder::claim_port`; `unmount() -> Result<(), UnmountRefused>` ("Unmount stopped
  nothing: port N is held by … ; <step>"), core_unmount → invalid_params_err; the desktop renders Unmount only when
  `strayListenerStep.kind == "start"`. Q-258's cfg(test) assert (no unit test may reach the reclaim on 8090) stays in
  front of it. THE STEP lives in ONE place, `port_holder::next_step` → `NextStep` (`start | quitStarter{pid} |
  restartGoose{pid} | otherPort | kill{pids}`, words in Display): PortHeld, UnsupervisedListenerError, UnmountRefused,
  the swarm's events (via reuse_port) and status `stray_listener_step` (wire `strayListenerStep {kind, pid?, pids?,
  text}`) read it; the banner's `strayStep` only MAPS the wire kind — never re-derive it in TS. An unmarked holder
  whose first ancestor outside its group runs THIS process's own program (`own_program()`; goosed is `goose serve`)
  gets `live_starter` under NoMarker → "restart the goose that started it" (Q-251: a goose older than the marker,
  alive); `reuse_verdict` still shares only rule LiveStarter, so an unmarked engine is never reused. InitOrSelf's step
  is "give the engine another port" (it used to say `kill <goosed pid>`). STATUS COST: lsof is 60 ms median on a
  1,000-process Mac (`-nP`, `-a -u` no faster); the status's `HoldersCache` reads the LISTEN set in-process via
  libproc (`port_holder::listener_pids`, 2.9 ms, identical to lsof Mac-wide incl. the client-connection negative
  control; socket_fdinfo offsets are from <sys/proc_info.h>, checked by `listener_pids_agree_with_lsof`) and re-judges
  only when that set or any judged process's start time/parent changes → polls 97–107 ms → ~2.3 ms. TRAPS: the hermit
  `python3` shim re-execs under its own argv[0] (a python `arg0` stand-in keeps the shim's path) — the Q-251 test's
  "goose" starter is `/bin/sh` with `arg0(current_exe)` and `set -m` so its background engine leads its own group;
  `git checkout <sha> -- file` after a merge commit silently drops later uncommitted edits (redo them).
- A SWARM BUILD REUSES AN ALREADY-SERVING ENGINE ONLY ON THE SAME PROOF (Q-248, 2026-09-28). Before: goose-cli's
  `SidecarEngine::ensure_loaded` returned Ok whenever `/v1/models` on `mlx_engine.port` served the pool id — any
  listener (unmarked stand-in, a dead goose's leftover, an engine marked for another port) was adopted (measured red,
  4 tests). Now the fast path runs `port_holder::reuse_port(port, engine::engine_marker(port))` → `reuse_verdict` over
  `ownership_proof`'s verdicts: `Own` (chain top started by this process) → reuse, silent; `Supervised{holder,
  starter}` (our marker, live starter — the desktop window's engine, which S8's HeldByBuild protects) → reuse, said
  once per pid as `sidecar-engine-shared{port, model_id, pid, starter, argv}`; `Leftover` (starter gone) → NOT
  adopted, `sidecar-leftover-not-adopted{port, model_id, holders}`, and the mount proceeds (its start stops it per
  Q-240); anything else (unmarked, other uid, other marker, mixed, no pid named) → `engine-port-held: …` naming pid,
  argv, the failed rule and the step — the device leaves the pool through `engine-mount-failed`. TRAP: an in-process
  stub listener is "this goosed itself" and is refused — reuse tests need a REAL stand-in process
  (`swarm_engine.rs` tests `fast_path_ownership`). Off unix nothing is ever reused (`engine-port-held`, unreadable).
- LOADING OFF (THE DEFAULT) PROVES THE SAME WAY (Q-250, 2026-09-28). With `allow_model_load: false` the pre-warm and
  `ensure_loaded` never run, so Q-248 alone left `exclude_unmountable_sidecar_devices` keeping ANY sidecar device whose
  id the port served (measured red, 3 tests). Now its catalog keep asks the device's engine `SwarmEngine::prove_served`,
  which reads the SAME verdict (`SidecarEngine::served_by`, shared with `reuse_served`): Own/Supervised → kept exactly
  as before (Supervised said once per pid, `sidecar-engine-shared`, drained at the next `take_probe_absences` seam);
  Leftover → excluded (`sidecar-device-excluded{id, reason: "sidecar-leftover-not-adopted: … next step: mount the engine
  from a goose window or enable loading …"}` — with loading off nothing would stop and supervise it); anything else →
  `sidecar-device-excluded{reason: "engine-port-held: …"}` naming pid, argv, rule, step. Nothing is signalled. LM Studio's
  `prove_served` is Ok by definition (goose starts no process behind it). The desktop's own engine is `Supervised`
  because the Electron app spawns `goose swarm` as a separate process whose parent is not goosed.
- A KILLED CHILD IS "GONE" TO sysinfo BEFORE ITS PARENT CAN SEE IT (Q-245, 2026-09-28). Measured: for 48 of 50
  SIGKILLed children `proc_pidinfo` (so `machine::process_start`/`prove`) answered nothing while `waitid(WNOWAIT)` did
  not yet report the exit. A test that waits for a killed child and then asserts the supervisor's next `try_wait`
  sees it must wait for `ps -o stat= -p <pid>` = Z (or empty), not for sysinfo. Test waits in goose-sidecar end on
  the event (the bind, the drains' EOF — `drain_rank_output` returns its handles — the state line) or on the
  process's exit with its words; `launch.rs`'s `wait_for` (600 × 100 ms) is deleted.

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
- 2026-09-26 Q-114 ROOT CAUSE (goose branch q114-gdn-padding-leak b589e04f6, not merged): the 27B tensor split hung at
  ~10.5k GENERATED tokens (10,447 / 10,522 / 10,537 — a step count, not a context width). mlx_lm 0.31.3's
  `ArraysCache.advance` decrements `left_padding` lazily, one new Metal buffer per `-= N`; 47 of the 27B's 48
  linear-attention caches are never read, so each decode step pins 47 buffers until MLX's `resource_limit` (499,000)
  throws `[metal::malloc] Resource limit (499000) exceeded` inside async_eval — and MLX 0.32.2's eval_impl error path
  deadlocks (synchronize behind an uncommitted fence signal), so nothing is raised: one rank sits `S` 0%, the peer spins
  `R` in all_sum and its GPU logs a command-buffer timeout. Fix: rank_wrapper evaluates every counter a step advanced
  (`settle_counters`); measured 21,074 tokens clean. The hang rule now reads per-rank GPU time (ioreg
  IOGPUDeviceUserClient accumulatedGPUTime) — CPU time missed a stuck rank 0 that still answered our own polls (4 min
  19 s, no event). Any single rank stalled > ~5 s (e.g. `vmmap` on it) reaches the same deadlock or a GPU-Timeout death.
  Detail, tools (sample, the __cxa_throw logger) and traps: skill mlx-jaccl-cluster, section "Q-114 ROOT CAUSE".
- 2026-09-27 Q-162: the hang rule killed a WORKING tensor split. E2E #3e's 259,408-token compaction call sat 22 s on
  rank 0 in mlx_lm 0.31.3 `PromptTrie.search` (cache.py:1612, the "longer" DFS) — no step, no GPU, both ranks `R` —
  because every push copies the whole path (`extra + [tok]`): quadratic in the branch depth. Measured on the evidence
  shape (2 entries ~260k, prompt leaves after 3 tokens): upstream 164,329 ms, goose's `rank_prompt_search.py` 32 ms,
  identical results (20,000 random tries vs the REAL upstream incl. ties/errors; 15 × ~230k-token branches: 251 ms).
  The result MUST stay upstream's exactly — every rank runs its own cache, a different reused prefix = a different
  prefill step count = collectives paired off. The search publishes `at: cache_lookup`; `CPU_ONLY_PHASES` (mod.rs)
  lets the hang rule count THAT rank's own CPU advance as progress. Never add a place inside a step or collective to
  that list: CPU spinning there IS Q-114. Mixed releases: a peer on the old wrapper still searches slowly under
  `at: batch`, so both Macs need the release.
- 2026-09-26 Q-143 (fork 09f645526, tag lz-pipeline-qwen4.6; goose 5dbafb733): THE PIPELINE LINE OF THE FORK BRANCHED
  AT lz.2 (42d207cfc). Every single-engine fix in `rapid_mlx/engine/batched.py` / `api/models.py` after lz.2 reaches
  `pipeline_qwen4 serve` (which imports them) ONLY if cherry-picked — check before any pipeline pin bump:
  `git -C ~/Projects/Rapid-MLX diff lz-pipeline-qwen4.<N> v0.14.3-lz.<M> --stat -- rapid_mlx/engine/batched.py rapid_mlx/api/models.py`.
  Q-143 was exactly that: lz.6's `_on_tool` (8a15af575) was missing, /v1/models listed only the tail, goose posted the
  turn-context block as its own user turn on Flash. MEASURED: that own-turn shape still reused 93.8–98.7% on the
  pipeline (the stable-message rule drops a user turn that is only the block) — the defect was the SHAPE (Q-94), not
  the cache. Offline method with no ranks: TestClient over the fork's `_build_app` + a real `_PrefixIndex` + a thread
  that admits/stores/pushes ("done", …), fed logged llm_request inputs re-shaped the way formats/openai.rs
  would for that tree's /v1/models, with the served model's own tokenizer — the fork test
  `tests/test_pipeline_qwen4_serve_transient_tail.py` is the template. TRAP: a digit word-level test vocab ("w10") is split by
  transformers 5's fast wrapper into "w1"+"0" — use letters-only words. Pipeline fork tags so far: .1 prefix cache,
  .2 cache budget, .3 aliases, .4 refused_tool_calls, .5 continuous admission, .6 tail on tool.
- 2026-09-26 Q-145 (fork c24f6b55e, branch q145-srpf, untagged at write time): PIPELINE PREFILL ORDER IS
  SHORTEST-REMAINING-FIRST at chunk granularity. Several rows may prefill at once (`_Engine.prefilling`, each its own
  cache + slot + KV reservation); every rank runs the chunk of `_shortest` (fewest prompt tokens left, then earliest
  admitted) — derived from ranges all ranks hold, NO new collective word; rank 0 admits the queued head (`_order`:
  protected oldest-first, then fewest left) only when it would be that pick. Aging is in prefill TOKENS, not seconds:
  `_Job.waited` >= own tokens left ⇒ protected (nothing later jumps it). `_Plan.abort` is a per-slot flag list; the plan
  header is [cmd, leave×slots, abort×slots, joiner×7 (the 7th = Q-144's tools word), evictions]. `_State.held` is GONE → `_State.waiting` (goose's
  pipeline_rank.py must read it — companion branch q145-pin-companion, lands WITH the pin bump). A canary arriving
  while BOTH slots are held (one decoding, one prefilling) still waits for a row to leave — slot-bound, not order-bound.
  TRAP (cost: ranks launched beside a live Flash split): in the fork's tests, `test_pipeline_qwen4.py`,
  `test_pipeline_qwen4_serve.py` and `test_pipeline_qwen4_vision.py` launch REAL ranks via `mlx.launch --backend ring`
  on localhost; only `test_pipeline_qwen4_continuous.py` and `test_pipeline_qwen4_srpf.py` are in-process. While a split
  holds the Mac, run only those two, on CPU: a runner that does `mx.set_default_device(mx.cpu)` then `pytest.main`,
  with the worktree first on sys.path (the venv's editable finder points at the main checkout).
- 2026-09-26 Q-144 (fork b1bc3b8d9 = tag lz-pipeline-qwen4.7; goose pins 419306f70 = tag lz-pipeline-qwen4.8 = .7 +
  Q-145 cherry-picked; branch lz/pipeline-single-line-port): THE AUDIT OF
  lz.3..lz.9 AGAINST THE PIPELINE. lz.3 KV-quant n/a (no --kv-cache-dtype on the pipeline), lz.4 MTP logprobs n/a (no
  MTP/logprobs; only ints cross threads), lz.5 unset max_tokens equivalent since 5675768ea, lz.6 ported (Q-143), lz.7
  XML skeleton guard PORTED — it runs on the LAST rank (the one that samples; `_sample`), armed per row by a `tools`
  word in the plan header (`_PLAN_TAIL` 8), rules from the checkpoint's own tokenizer (Flash arms 12; log line
  `[pipeline] rank N: xml tool-call skeleton guard armed (12 rules)`), lz.8 PORTED ADAPTED — the pipeline's own
  `_PrefixStore.put` held the QSAIndexCache raw ring as a VIEW of the chunk's raw keys (entry 1.346x charged at
  512-token chunks → 1.025x with `_own_bytes`), lz.9 equivalent a863c60c5. THE GUARD THAT REFUSES A REPEAT: the fork's
  `tests/test_pipeline_single_line_ports_audit.py` fails on any `git rev-list <newest v*-lz.*> ^HEAD ^v0.14.3` commit
  missing from `tests/pipeline_single_line_ports.json` (review = ported/equivalent naming pipeline commits in HEAD, or
  n/a with a reason); goose's `the_pipeline_pin_was_reviewed_against_the_single_engine_pin` fails when ENGINE_LAUNCHER's
  tag != `PIPELINE_SINGLE_LINE_REVIEWED_THROUGH` (provision.rs). A single-engine pin bump therefore = review the new
  single-line commits on the pipeline line, bump `reviewed_through` in the fork manifest AND the goose constant.
  Offline method: in-process singleton group + a real tiny `PipelineStage` (float32 — CPU gather_mm takes nothing else)
  with a scripted logit bias (`tests/test_pipeline_qwen4_single_line_ports.py`); bytes held = `mx.get_active_memory()`
  deltas after gc + clear_cache, per cache member. Also in-process (safe beside a live split): the port tests, the
  audit test and `test_pipeline_qwen4_serve_transient_tail.py`. Goose's real-fork seam tests
  (`the_pipeline_program_patches_the_real_forks_seams`, `every_way_renders_the_same_prompt_for_the_same_setting`) SKIP
  silently unless an env proves the pinned commit — point `GOOSE_TEST_PIPELINE_PYTHON` at a scratch venv
  (`uv venv --python 3.12` + `uv pip install "rapid-mlx @ git+…@<commit>" mlx==0.32.2 mlx-lm==0.31.3 mlx-vlm==0.7.1`,
  ~0.6 GB) and grep `--nocapture` for "skipped". Q-145 receipt: at 419306f70 the seam test's stand-in
  `engine.joining = …` failed ("property 'joining' … has no setter") — the companion branch had not run it.
  TRAP: a scratch script named `attrs.py` or `bisect.py` on sys.path shadows the stdlib/attrs package and breaks
  pytest/fastapi imports — name scratch files `m_*.py`.
- 2026-09-27 Q-103 (fork f0a3cd07b = tag v0.14.3-lz.10, branch lz/single-srpf; pipeline review a18e14fd4 = tag
  lz-pipeline-qwen4.12, JSON-only on 7d3327202): THE SINGLE ENGINE'S ONE-ROW ORDER. Cause: under MTP
  `_max_running_sequences()` is 1 (vendored B=1 verifier) and admission kept one request for its whole life, prefill
  included. Fix = the pipeline's SRPF without ever batching two rows: at a chunk boundary a waiting request with fewer
  prompt tokens left takes the engine and the prefilling row is PARKED — `BatchGenerator.remove(uid,
  return_prompt_caches=True)` → its `prompt_cache` with `cached_tokens` advanced (the prefix-hit shape), resumed on
  the hit path; `_srpf_parked_tokens` keeps parked tokens out of usage `cached_tokens`. Aging is MEASURED ENGINE
  SECONDS of later arrivals' steps (a jumper also DECODES before the parked row resumes), converted at the measured
  full-chunk prefill rate. Memory gate = cap vs active + parked copy + jumper horizon, priced from the live row's
  buffers (the 27B's config projection is 0). Knob: `serve --singleton-prefill-order srpf|fifo` (default srpf; fifo =
  lz.9 for A/B). Read it live: `/v1/status` → `singleton_prefill_order` {parks, refused{no_memory_cap|kv_budget|
  unverifiable_cache|row_not_in_a_lone_prefill}, prefill_seconds_per_token, parked[]}, status rows phase "parked".
  Residuals: a request arriving during a DECODE waits it; `kv_cache` int8/int4 profiles refuse parks (batched
  offsets are arrays). OFFLINE METHOD (no GPU): the REAL Scheduler + BatchGenerator + vendored MTP on a tiny
  `mlx_lm.models.qwen3_5.TextModel` (full_attention_interval=2 → ArraysCache + KVCache) with
  `inject_mtp_support(model, allow_random_init=True)`, `mx.set_default_device(mx.cpu)` and
  `mx.metal.is_available = lambda: False` (BatchGenerator reads the Metal working set otherwise); stub
  `_resolve_metal_cap_bytes`/`_current_metal_active_bytes`, drive `_srpf_clock`. full_attention_interval=1 breaks
  the MTP injector (create_ssm_mask on a KVCache) — use a tiny llama at max_num_seqs=1 for the pure-attention case.
  Fork regression on CPU: a pytest plugin module doing `mx.set_default_device(mx.cpu)` passed with `-p`, same file
  list on the new branch and the base tag, compare FAILED sets (the base fails the same model-fixture tests on CPU).
- 2026-09-28 Q-423: "Server error: Internal error during streaming" is the ENGINE's text (Rapid-MLX lz.10
  rapid_mlx/service/helpers.py:4389, disconnect_guard's generic `except Exception` arm; F-131 sanitizes the SSE and
  `logger.error(... exc_info=True)` puts the exception on stderr FIRST). The single engine's stderr is now kept in
  `~/.local/state/goose/logs/mlx-engine/mlx-engine-<ms>.log` (stamped, rank_log's bound) and every ERROR line is a
  WARN in goosed's log — before this it lived only in a 200-line memory tail (`tracing::debug!`), so look THERE first.
  The serving node completes the words (Link proxy `ChatServing::stream_errors`, router for the local engine) via a
  clock-free barrier: FIONREAD on the pipe == 0 and nothing in the reader's hands. Studio /v1/status after the failure
  read metal peak 83.15 GB vs the 83,494,174,720 B ceiling; a replay of a 191,200-token prompt on the M4 Max read
  rss 73 GB at 180k and pushed the MacBook to pressure WARN — never replay a 190k prompt on a Mac already using
  ~58 GB (the coordinator killed it at 0.8 GB free). The fit rule's 64 KiB/token KV is ~2.5x under the engine's
  measured growth (Q-424).
