# The distributed engine (split across Macs)

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Contents
- The distributed engine
- The split's prompt cache across CHATS
- MiMo-V2.6-Flash on the mlx_lm pipeline split: correct usage + prompt cache
- MiMo-V2.6 VISION on the split

## The distributed engine (2026-09-24 — one model split across Macs; isolated from the single engine)
- PEER TRAY (2026-10-31, merge 50647a9959): every Mac in a split shows the controller's menu-bar title + " · rank r/N (for <controller>)". Controller main writes `~/.goose/distributed/tray/controller.json` (3 s heartbeat) -> supervisor relay `distributed/peer_tray.rs` (spawned in `supervise`, runs through load/serve/restarts) sends `NodeOp::TrayStatus` per peer on change + every stale/3 s -> peer writes `~/.goose/distributed/tray/rank-status.json` -> peer main (`peerTray.ts`) polls 2 s. Stale (`GOOSE_DISTRIBUTED_PEER_TRAY_STALE_SECS`, default 30) or rank pid gone -> "Rank r/N stopped" for 30 s, then clears + deletes the file; `GOOSE_DISTRIBUTED_PEER_TRAY=off` disables. No controller app -> the peer composes "Split · <model> · loading shards x/y GB|serving". Older Link peers refuse the new op kind (one warn). Wire contract = `ui/desktop/src/peerTray.fixture.json` (Rust + vitest both pin it). Needs a build with it on BOTH Macs.
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
  ps stat `T` = frozen at once. Watchdog per poll (2 s): kernel pressure WARN or HOLD-available < 5% RAM for 3 polls in a
  row → admission held (503 + memory_hold, waited out); reopens only after 2 polls kernel NORMAL and ≥ 1.25 × the WARN
  reserve (6.0 GiB on 96 GiB). Stop (never restarted): kernel CRITICAL at once, or < 2% RAM on 2 polls in a row.
  HOLD-available = free + speculative + INACTIVE + purgeable (`probe::vm_hold_available_bytes`, max'd with the mount
  figure in `parse_sample`); the mount/plan keep free + file cache + purgeable. WHY (2026-10-31, branch
  dist-admission-hold): on the live MiMo split the workhorse (rank 0 Metal ≈80 GB wired) read 3.2–4.8 GiB on the mount
  figure with ≈9 GB inactive and the kernel NORMAL, so the hold flapped 56 times in a night (14 requests 503'd and
  waited 1.3–30 s each, 121 s total) and the CRITICAL stop sat only ~1.3 GiB below. Card: "Held · <node> X GB
  available (needs Y)" from `status.hold`. Evidence: cli log `kind` admissionClosed/watchdogWarn rows, rank0 log
  `POST /v1/chat/completions ... 503` followed by `GET /goose/admission`.
  Restart breaker = the single Sidecar's (3 per 600 s, backoff 1 s → 30 s).
- THE MEMORY HOLD IS WAITED OUT, NOT RETRIED (Q-397, 1d97681d2): the watchdog's close sends `code: "memory_hold"`
  (`distributed::MEMORY_HOLD_CODE`); rank 0's 503 carries `error.code`/`reason`/`admission: "/goose/admission"`, and
  `GET /goose/admission` parks until the reopen (rank_admission.py). goose keys on the CODE (never the prose) →
  `ProviderError::EngineHold` → `engine_hold::wait_for_admission` (no clock; Stop drops it; engine gone = loud error);
  the router fails a held node over first (`HOLD_GOES_TO_CALLER`) and waits only when nothing else serves; the turn line
  reads "Waiting: <watchdog reason>". Any other 503 keeps the 3 retries. Probe: `curl -s 127.0.0.1:<port>/goose/admission`
  answers `{"admission_open": true}` at once when open (it BLOCKS while held — never run it without `-m` on a held split
  unless you mean to wait). Gaps: the pipeline runner's fork 503 has no code (Q-398); a peer's split over the Link relay
  cannot be waited on (relay routes chat/models/status only, Q-399).
- Live tests + measured numbers: `references/distributed-engine-history.md`, section "goose's DISTRIBUTED engine" (moved from the mlx-jaccl-cluster skill 2026-10-03).
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
- COMPACTION IN FORGE 2.0 LOCAL RUNS (analysis 2026-10-16, runs cloud-8e3bdc1d (B) and cloud-f02f8ef9 (A) on Gabee, parse scripts in the session scratchpad `compaction/`):
  - All 6 compactions were threshold-triggered at 102.6k-109.8k = 0.8 x goose's DEFAULT_CONTEXT_LIMIT 128,000, because `context_window_probe_missed ... api/v0/models 404` on main. The engine serves 158,833 (guard `max_context_idle`). The window probe that reads a guarded `max_model_len` exists on `t12/local-bench-fidelity` (`probe_context_window`, `served_window.rs`), NOT on main (`crates/goose-providers/src/openai.rs:665`).
  - The summaries were GOOD (file list, decisions, next step), yet the model re-read everything: A re-read 14 files in 7 calls (7.2k->34.8k) then re-planned 16,970 tokens / 1,154 s; B C1 re-read 20 files then re-planned, and its next step was written 2 h 37 min later. Richer summary text is NOT the lever; giving back the working set is.
  - Summary calls cost 214-1,280 s each (B ~83 min of 6.75 h incl. cold restarts). B C4: the chat-shaped summary request was answered with a tool call and fell back to the transcript shape (0 of 85k cached, 569 s to first token).
  - Context bloat: one `tree` result = 161,890 chars incl. node_modules (64% of A's context; `developer/tree.rs:63-72` has no cap, unlike shell's 50,000-byte cap at `shell.rs:151`). Old reasoning is kept (Qwen3.8 `preserve_thinking`): 34-43k tokens at C1-C3. Task prompt duplicated per compaction (`pillars.rs:121` vs the trimmed compare at `:648`): x5 by C5 (~25k chars).
  - Ranked fixes: (1) ship the window probe; (2) cap tree + skip node_modules; (3) re-inject the working set (on-disk content of read/written files, newest first, within a budget) after compaction; (4) drop old reasoning once at the threshold, never per turn; (5) summary call with thinking off; (6) retry a tool-call-answered summary once with tool_choice none before the cold transcript fallback; (7) dedupe the task prompt.
- COMPACTION V2 (2026-10-16, branch `t12/compaction-fixes`, design `local-edition/design/COMPACTION-V2.md` with
  primary-source research beside it; NOT yet adversarially reviewed or run live). Answers main's "COMPACTION IN
  FORGE 2.0 LOCAL RUNS" analysis (fae6d42bc). Built: `GOOSE_TREE_BOUNDED` (dependency folders named, listing cut
  the shell's way); the task prompt kept once (trimmed compare + hidden-copy chain); summary with reasoning off
  `GOOSE_COMPACTION_SUMMARY_THINKING=off` (prefix byte-identical at `medium`, rendered on run B's own request;
  xhigh/low write a system line); tool-call-answered summary retried once with tool_choice none
  `GOOSE_COMPACTION_SUMMARY_TOOL_CALL_RETRY`; the WORKING SET (current on-disk content of files read/written,
  newest first, whole or named, `GOOSE_COMPACTION_WORKING_SET_SHARE=0.25`); STAGE 1 MASK before any summary
  (`GOOSE_COMPACTION_MASK_FIRST`, `GOOSE_COMPACTION_DROP_REASONING`: old outputs -> stubs of facts, old reasoning
  dropped, no model call, taken only when it leaves <= limit x (2·threshold-1)); chats mid-loop keep a verbatim
  tail (GOOSE_COMPACT_KEEP_TAIL unset). Offline replay of run B: stage 1 taken at C1 only (104.5k -> 68.3k); at
  C2/C3 the working set gives back what masking takes. LIVE METRIC: calls and tokens until the first write after
  each compaction (baseline B C1 37 calls and no write; C2 18 calls / 74.6 min). Replay commands: design doc §7.
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

### The split's prompt cache across CHATS (2026-09-29 — Q-498 a6d70abba, Q-502 9cb716449, Q-508 cutting)
- Measured on the 27B tensor split (MacBook rank 0 + Studio rank 1, 262,144 window): KV = 32,768 B per token per
  rank (16 full-attention layers × K,V × 2 KV heads/rank × 256 × 2 B) + 76,972,032 B recurrent state per sequence.
  The plan's KV charge (= prompt-cache byte limit) = 17,333,813,248 B = one live + one cached context. Rank peak
  ~44.9 GB each.
- Before Q-502, admission and eviction protected only the NEWEST conversation. The owner's second chat (77,683-token
  row + an 84-token side call) evicted #3x's 200k prefix, and each switch cost an 18-min cold re-read (E2E #3x, 15:10).
  Keeping both under the old rule needs 29.07 GB; the MacBook lacks it.
- Q-498: the engine SAYS it. `/v1/status requests[].evicted_prefix` and the rank-log line `GOOSE_RANK_PREFIX_LOST`
- Q-522 (2026-10-31, mimo-distributed 69ee5cf67e + main e8b721c1ab): `while_keeping: null` on a PREFIX_LOST = NO conversation was kept. Cause: goose's chat names no `rapid_mlx_transient_tail` since a79067e2f0 (kept blocks), and `_tokenize` only cut/kept tail-bearing requests, so every chat ran on mlx_lm's type-count eviction; the per-decode-step yield (TrackedBatchGenerator.next) took the chat's own prompt[:-1] beside a stale "user" entry (run 8985a86d: summary read 35,503 of 212,772, 30 min). Fix: spec `keep_tailless_conversations` (tag mlxLmServerTaillessPipelineSplit) keeps a tool-bearing tailless chat (`Conversations.tailless_turn`; a same-session side call with another head is not the chat). A run built before these commits still has the defect.
  drive the glance/tray: "another chat pushed N of this conversation out of the engine's memory".
- Q-502: every conversation's kept prefix + head are protected, and a request whose padding would evict another
  chat's kept entries is HELD until rows finish (no timer). The spec tag `mlxLmServerEveryConversation` goes to every
  rank, and a 3.0.79 peer refuses it: BOTH Macs need the same build. Replay: #3x 0 → 199,798 cached at 17.33 GB; the
  side call held 42 s longer.
- Q-503: goose read free memory with host_statistics64, which macOS rate-limits for non-Apple processes (a cached
  copy returned as success). memory.rs now reads the vm.page_* counters live through sysctl.

### MiMo-V2.6-Flash on the mlx_lm pipeline split: correct usage + prompt cache (2026-10-27, mimo-distributed b8bb49344)
- Model `mlx-community/MiMo-V2.6-Flash-RL-mxfp4-q8` (167 GB, mimo_v2, 48 layers: 9 full attention + 39 SWA window 128,
  attention sinks on SWA). Ranks run `mlx_lm.server --pipeline` under goose's TENSOR wrapper (rank_wrapper.py +
  rank_pipeline_split.py) in the pipeline env (`~/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12`, mlx_lm 0.31.3 +
  vendored mimo_v2). NOT pipeline_qwen4_serve.
- Correct usage (HF card + Xiaomi docs mimo.mi.com "model hyperparameters", updated 2026-10-20): temperature 1.0,
  top_p 0.95 (Xiaomi's API forces them); thinking ON by default (`enable_thinking=False` renders `<think></think>`;
  thinking on renders a bare `<|im_start|>assistant\n` and the model writes `<think>` itself); history assistant rows
  render `<think>{reasoning_content}</think>`; tool calls = qwen3_coder XML with NO newline
  (`<tool_call><function=N><parameter=p>v</parameter></function></tool_call>`); eos 151643/151645/151672;
  generation_config `max_new_tokens 2048` (vLLM trap: becomes the default cap; goose's rank_budget ignores it, good).
- What goose already did right: sampling from generation_config (rank_sampling: temp 1.0 / top_p 0.95; `do_sample:false`
  is a bool and ignored, correct); reasoning parsed by mlx_lm's state machine (`<think>` single tokens, delta field
  `reasoning`, goose reads it); goose sends `reasoning_content` back (preserve_thinking_context = !is_direct_openai_host).
- Mismatches FIXED: (1) tool parser json_tools -> qwen3_coder (95784b84b, the live agent). (2) THINKING: the rank turned
  thinking OFF on every tool-bearing request (single-engine rule Q-135) and goose's `openai` bench provider sends no
  effort (T12 G1 still unmerged, lives on t12/local-bench-fidelity) -> a local MiMo Gauntlet was a NON-thinking run vs a
  cloud baseline that thought (OpenRouter 10-02: 265k reasoning tokens). Now `TOOL_TURNS_THINK` (rank_thinking.py, set
  by rank_pipeline_split.py): tool turns keep the template's default (on); pins and tool-less helper calls stay off.
  (3) CACHE: see below.
- Prompt cache design (mlx_lm 0.31.3 LRUPromptCache): a RotatingKVCache past its window is NOT trimmable, and a cache is
  trimmable only if every layer is, so a longer entry is never trimmed back; reuse is exact or a stored strict prefix
  (segment-end snapshots + the prompt+answer entry). A restored ring holds exactly the trailing window before that
  point (extract() rolls a rotated ring to temporal order and drops left padding), so a hit == cold, by design. Ranks
  evict alike via the weighted nbytes (rank_pipeline_split.py). NO gap for append-only chats: a "fix" that cut
  tail-less thinking-on prompts at their last message (b47a3ef8f) was REVERTED (312cf5a3e) after the real-mlx_lm test
  showed upstream already reuses prompt[k-1]-1 tokens per step with the cut off (an entry the next request extends);
  the cut only doubled snapshots. Guard: launch.rs `a_thinking_mimo_chat_without_a_tail_reads_its_conversation_
  through_real_mlx_lm` (tiny gemma3_text, rings window 8, MiMo template fixture, thinking on, tools, no tail):
  cached_tokens == what the LRU itself restores, == prompt[k-1]-1, rings restored past the window, warm == cold logits.
- "cached_tokens 3 of 15,597" (live 16k direct prompt) is NOT a broken counter: a one-shot prompt is cold by
  construction; 3 = mlx_lm's system segment (rendered WITHOUT tools, MiMo puts the tools system block first). Through
  goose (omlx, transient tail) the live split read 19,608 of 19,902 (llm_request logs, cache_read_input_tokens).
- LIVE (2026-10-27 13:20, split 18/30, served window 118,016, client scratchpad p1/cachetest.py): 10,177-token prompt
  cold TTFT 23.6 s -> repeat 0.15 s (cached 10,176), greedy outputs identical; a second conversation cold 25.7 s ->
  0.48 s, identical; agent chat (tools, thinking on, 3 calls) every turn cached = previous prompt - 1, all answers
  correct incl. a table row 4k tokens back. Verdict: reuse real, counter right, SWA restore exact.
- A shared prefix INSIDE one user message is never reused (no snapshot inside a message): test cache reuse with
  multi-turn appends, not "same table, different question in the same message".
- Cloud baselines on the board: Gauntlet sb-7.2 0.7909 (2026-10-02, OpenRouter xiaomi/mimo-v2.6-flash, 2.1 h, 131
  requests, 37M in / 36.5M cached / 400k out); Forge 2.0 0.5283 median of 2 (0.5538 / 0.5028, 2026-10-11).
- Tests: cargo test -p goose-sidecar 501 lib + integration green on the laptop (node-run), fmt clean.
- 2026-10-27 LOCAL GAUNTLET sb-7.2 (split 18/30, window 119,808, dev app 3.0.132 @ 9411914e0f): 0.5894 vs cloud
  0.7909 (raw 0.8083 vs 0.838; admission ceiling 0.599 from s_visible_surface 109/120 pixels). 130 calls, 4h32m + 9.5
  min scoring, 5 compactions (cloud 0 at 1M), decode 27.4 tok/s. Losses are all 3D pixel geometry; backend checks beat
  cloud. TRAPS: (1) the bench's `openai` entrant at a loopback engine never asked /v1/models before sending an image ->
  404 "Only 'text' content type is supported" killed run 1 at 2h08m; fixed 9411914e0f (placeholder + one retry without
  images + permanent-4xx marker). (2) The mlx-community quant is TEXT-ONLY (no vision tower), cloud lists image input:
  a vision-parity caveat on every local score; mlx-vlm has mimo_v2 vision, our split cannot serve it. (3) goose
  compacts at ~0.88 of the served window: set the split to >= 256k AND GOOSE_CONTEXT_LIMIT before a parity run.
  Pre-flight: ~/goose-builds/loop-state/local-parity/parity_check.py (PASS/FAIL table). Plan: goose-wt-mimo
  HANDOFF.md "Session 12 plan". Run brun-0312730b was published before the owner's hold; removal is his call.

### MiMo-V2.6 VISION on the split — Session 12 (2026-10-27, goose mimo-distributed 1a2b0b8267, fork lz-pipeline-qwen4.16)
- MODEL NOW: EigenLabs/MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp (171 GB, MOPD = what Xiaomi's API and so the cloud baseline
  serve since 2026-09-25; text quant identical to the old mlx-community RL mxfp4-q8, which is DELETED). mlx-vlm key layout:
  language_model.* + vision_tower.* (1.457 GB bf16) + audio_encoder/speech_embeddings (0.277) + MTP (1.976 bf16).
- HOW IT WORKS: LM positions are plain 1-D (HF modeling, SGLang, vLLM: arange, no M-RoPE). Rank 0 (HTTP) encodes images
  with the fork's mimo_v2_media ViT, each <|image_pad|> becomes N hash-derived MARKER ids (unused vocab rows >= 151680) so
  the prompt cache never mixes images; features ship once per image inside the pickled request to the LAST pipeline rank
  (it runs layer 0), which swaps marker rows before layer 0. rank_media.py does the mlx_lm.server wiring; /v1/models then
  declares "vision","video" and goose's providers send image parts (no provider change).
- Image tokens = (H/16)(W/16)/4 after smart_resize to multiples of 32, Xiaomi pixels 8192..8388608 (config.json
  processor_config, NOT the converted build's preprocessor_config). Video: SGLang layout <|mimo_video_start|> + per frame
  pair "MM:SS"<|vision_start|>pads<|vision_end|> + <|mimo_video_end|>, 1 fps, 8..3600 frames.
- TRAP normalisation: upstream preprocessor_config + vLLM say ImageNet (default), SGLang/mlx-vlm/EigenLabs say CLIP.
  GOOSE_MIMO_IMAGE_NORM=clip|imagenet; settle with a live colour A/B.
- TRAP planner: omni names broke layer_index (everything became "shared" = charged to every rank); layer_split.rs now
  strips language_model., charges vision to rank 0 only, audio/MTP nowhere.
- PARITY: torch-CPU HF reference vs ours, float32 cosine >= 0.99998 per layer up to 1,900 patches, bf16 0.990-0.9999
  (torch bf16 0.993-0.9999); tests/mimo_media/vision_parity.py (prep/ref/cmp, ref python = venvs/voice-qa).
- Knobs: GOOSE_MIMO_MEDIA=0, GOOSE_MIMO_IMAGE_MAX_PIXELS, GOOSE_MIMO_VIDEO_MAX_TOKENS (16384), GOOSE_MIMO_VISION_BITS (0/8).
- NOT DONE: live run (Macs busy with voice), audio (plan in HANDOFF Session 12), MTP. HANDOFF.md "Session 12 vision".
