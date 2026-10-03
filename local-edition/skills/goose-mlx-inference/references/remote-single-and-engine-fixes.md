# Remote single, the hidden caps, Q-85 / Q-142 / Q-508 (merged from the global copy, 2026-10-03)

Until 2026-10-03 there were TWO diverged copies of this skill: this repo copy (the larger, maintained by the
goose agents) and `~/.claude/skills/goose-mlx-inference` (workhorse-owned). The repo copy is now the only
home and the global path is a symlink to it. These sections existed ONLY in the global copy and are kept
here verbatim. Where they and SKILL.md disagree, the dates decide (newest wins) — in particular:

- **Superseding note on "The Swarm provider" (SKILL.md):** the global copy, written after placement phase 3,
  says the swarm reaches a mesh peer's engine ONLY through REMOTE SINGLE (below), where SKILL.md still says
  "a mesh PEER's sidecar is not a node". The global wording was:

  > unchanged). A mesh PEER's engine is reached ONLY through REMOTE SINGLE below (the engine stays loopback-bound) — LM Link fans
  > LM Studio models across machines. Never add a clock to the queue (gate 5); never let a probe failure read as "idle".

## Remote single — the single engine on a Link peer, chat routed there (placement phase 3, 2026-09-24)
- Wire (leanzero-link `inference.rs`, db291480a): the peer's control service serves `POST /v1/swarm/inference/v1/chat/completions`,
  `GET …/v1/models`, `GET …/v1/status` → its OWN 127.0.0.1 engine, bytes streamed through untouched (no [DONE] ever appended).
  Gates: none wired 501 · owner switch `LEANZERO_LINK_ALLOW_CHAT_SERVING` off (read per request) 403 `chatServingDisabled: … off on
  <host>` · port unnamable 503 · nothing listening 502 `engineUnreachable` (never mounts). Requester: `InferenceRelay` on
  `127.0.0.1:<eph>/relay/<64-hex capability>` → peer through tailscaled SOCKS with the node token; `linkRelayFailed` 502 when the
  peer can't be resolved. The mount itself rides the OLD `/mlx/mount` op → the peer also needs `LEANZERO_LINK_ALLOW_REMOTE_EXECUTION`
  (read at manager BUILD → takes effect at the next Link connect).
- IN FLIGHT (Q-32, c9b5f03a7): a peer whose tailscaled dies sends NO FIN/RST across the mesh — r3-1's request hung 120 s.
  Relay tags each request `x-leanzero-link-stream: <32 hex>`; the peer holds the id while the response body lives and answers
  `GET /v1/swarm/inference/streams/{id}` → `{epoch, live}` (epoch random per ControlService start; unknown id → live:false; an
  older node 404s = "reached, no info"). Looks every `poll_interval`, fresh dial bounded by `connect_timeout`
  (`PeerTimeout::FreshTotal`). Ends ONLY on: 5 (MESH_POLL_FAILURE_LOOKS) unreachable looks in a row · a shown request released
  2 looks running · 5 looks never-received. Before head: 502 `linkRelayFailed: Link peer '<p>' lost this request in flight: …`;
  after: one SSE `{"error":{message,type:"linkRelayFailed",code:502}}` then abort. NO idle-read timeout — a silent healthy
  generation is HELD at every look. TRAP: aborting right after the event dropped it unflushed — one `yield_now` between fixes it.
- SUPERVISOR (Q-31, c9b5f03a7): a daemon exit (try_wait) or wedge (5 failed looks) under a live connection with intent
  `connected` → auth Connecting + reconnect Reconnecting + lastError "LeanZero Link's mesh daemon stopped (…) — restarting it
  with no user action" → the launch reconnect's connect → Reconnected (proven after 5 healthy looks). Dies again before proven,
  or the restart's connect fails → reconnect Failed{reason naming both}, no retry. Wire types unchanged. Tests fake it via
  `killed_instances` / `dies_after_looks` on the fake mesh and `fake_tailnet().kill()/revive()` for the relay.
- ACP (ad9e0a4b5): `mlxEngine/remoteSingleStart {peer, modelId}` → `{started, refusal?{code,message}, status}`; `remoteSingleStop
  {keepMounted?}` → `{unmounted, unmountError?, status}`; `remoteSingleStatus {}`. Codes: linkNotConnected · unknownPeer ·
  chatServingDisabled · remoteManagementDisabled · peerTooOld · peerMountFailed · peerUnreachable · distributedOwnsThisMac ·
  remoteSingleActive. The switch is checked THROUGH THE RELAY before any mount (a peer that hasn't opted in costs no 27B load).
- Route record `<state>/mlx-remote-route.json` (0600, pid + relay URL w/ capability): every window routes alike; OMLX_HOST follows
  it (route > distributed > single port). Router: one `mlx-remote` node `remote-<peerHostname>` (NO ':' — the desktop splits
  "<id>: <reason>" on it), own omlx provider instance at the relay (`load_provider("omlx")`, base_url replaced), capacity = the peer's
  `maxConcurrentRequests` (new in its mlxEngine/status), in-flight = the peer engine's /v1/status; this Mac's sidecar is set aside.
- LIVE (2026-09-24, hermetic mesh on the two real Macs, receipts in 0c5ba9efd): the 27B (MTP) on the M3 Ultra
  ready ~25 s after start, a goose chat turn from the MacBook answered by it — router `pick node=remote-WorksMacStudio.lan`, engine
  `generation_tps` 29.9, prefill 314 tok/s. Client drop → the peer engine idle within 1 s. Peer engine SIGKILLed mid-stream →
  the requester's body ERRORS (curl exit 18, 390 chunks, no [DONE]) — never a clean short answer.
- HOW to run the hermetic two-Mac mesh beside the installed apps (they hold 41226 + ~/.leanzero): throwaway Headscale on the
  workhorse (`~/.leanzero/bin/headscale serve -c <scratch>/config.yaml`, listen 127.0.0.1:18088, grpc 50553, metrics 19790 — the
  production one uses 8790/50443/9790), `ssh -N -L 18088:… -L <acp>:…` from the laptop, a 20-line fake worker answering
  `/v1/mesh/join-key` {authKey, loginServer, nodeSecret, expirySeconds}, goosed with HOME=<scratch>/home (→ its own .leanzero),
  GOOSE_PATH_ROOT, LEANZERO_LINK_WORKER_URL, LEANZERO_LINK_CONTROL_PORT=41236 (50bafa9da), LEANZERO_TAILSCALED/_CLI = the app's
  bundled binaries, UV_CACHE_DIR/UV_PYTHON_INSTALL_DIR = the real ones. TRAPS: the tailscaled socket path must fit sun_path
  (~104 bytes) — a scratchpad path failed with `bind: invalid argument` → use a short symlinked base; the models scanner does
  not follow symlinked model dirs → `cp -cR` (APFS clone, instant, 0 bytes); the 27B on the workhorse lives in
  ~/jaccl-smoke/models, not ~/.goose/models.
- FIXED 92da453f4: status() used to report `running` from the stored ManagerState (the child exit was only seen on a re-mount),
  so a SIGKILLed engine read `running` for minutes with its uvx launcher a zombie. Now `Sidecar::exited()` (try_wait, reaps) runs
  on EVERY status poll → `failed` "the engine process (pid N) exited: signal: 9 (SIGKILL) — not restarted automatically; Mount
  restarts it (the crash breaker applies). Last log lines: …". Policy = the distributed supervisor's with restartOnFailure off:
  no silent restart; the supervisor stays in Running so an identical Mount restarts through ensure_running's breaker. Remote
  single's unmount-before-mount workaround was removed (a Mount already restarts a dead/hung identical engine).

- 3.0.27 walkthrough fixes (2026-09-24, 88698230d a4f1f3868 e74d45f67 75dad8c17):
  - remoteSingleStatus now carries `peerComputerName` (the route record keeps the roster's computer_name) and `baseUrl` (the
    relay). THE ONE NAME: UI `routePeerName` = `macName` over those facts; backend prose uses `PublishedRoute::peer_name()`.
    The router's node id / reason text KEEP the hostname (parseNoNodeError splits on `\S+`) — NoNodeNotice maps it back.
  - The Engine tile IS the remote engine while a route is up: live read = `readMlxLiveStatus(<relay>)` (mlxLiveStatusUrl now
    KEEPS the base path, so `/relay/<cap>/v1/status` works); main's monitor reads the relay too (`engine: 'remote'`), tray
    speaks for it with "Stop serving from <Mac>" (`stop-remote`). The old tray rule "generationTps>0 = writing" painted an
    idle peer green — phase is `remotePhase(state, activity)` from the live read.
  - Run across both Macs for another model: discover(candidate's link: nodes, model) → `splitConfigFor` (saved config kept
    when it spans the same hosts; only modelId, modelDir, pipelinePython, context/slots replaced) → provision if an env is
    absent/broken → `distributedStart(config)` (persists + preflights). Discovery model-node states are `match|absent|differs`.
  - RESTORE ON RELAUNCH: backend `providers/mlx_serving_intent.rs` (`<state>/mlx-serving-intent.json`) written by the LOCAL
    ACP handlers only (Mount, remoteSingleStart, distributedStart), removed by the matching explicit stop; shutdown and the
    mesh proxy (core_mount) never touch it. Read: `mlxEngine/servingIntent`. Renderer `mlxRestore.ts` runs once per app
    launch (main `mlx-restore-claim`), line on Engine tab + composer strip + tray. UNVERIFIED LIVE as of 75dad8c17.

## 2026-09-25 — the hidden generation caps (Q-65 class): split 512, single engine 32768 — both removed
- goose sends NO max_tokens (formats/openai.rs, on purpose). So the SERVER's default for an absent one is the
  real cap. Check it whenever an engine changes: split = mlx_lm.server `--max-tokens` 512 (fixed goose
  39db807f0, rank_budget.py); single = rapid-mlx serve 32768 (cli.py effective_max_tokens), fixed by fork
  v0.14.3-lz.5 (d1b58f6ed, branch lz/unset-max-tokens), pinned in engine.rs ENGINE_LAUNCHER.
- Why `serve --max-tokens <window>` is NOT a fix: the chat route checks prompt + max_tokens <= window, so it would
  400 every request. The budget must be derived per request: room = window - counted prompt (lz.5, routes
  chat/responses/completions; explicit client/operator caps untouched; no stated window → resolved default).
- MEASURED: the admission gate's KV projection is DEAD for mlx-lm `.args` models (the 27B: `_resolve_kv_bytes_per_token`
  = 0, no `.config`; only `_read_kv_dims` reads them, for the /v1/models card). So admission = `active < cap` only.
  lz.5 adds a decode-time memory stop: at the cap, after the prefix/paged caches evict, the LONGEST generation
  finishes `length` (metric num_memory_full_stops).
- Quick check without waiting for a long output: /v1/status `requests[].max_tokens` shows the budget while a request
  runs (lz.4: 32768; lz.5: 262144 - prompt, e.g. 262101 for a 43-token prompt).
- Fork tests: `~/Projects/Rapid-MLX/.venv/bin/python -m pytest tests --ignore=tests/integrations` from the worktree
  imports the worktree's package; 26 failed + 13 errors are pre-existing on lz.4 (model/network fixtures) — diff
  the failure list against a lz.4 worktree, never read the count alone.

## 2026-09-26 — Q-85: `</parameter>\n!` residue in tool args (single engine) — engine-side skeleton guard (lz.6)
- CAUSE (measured, lz.5, Qwen3.8-27B-Atlassian-Q8, the session's own request replayed): after the LAST parameter's
  `</parameter>\n` the model's logprobs are `}` -0.5, `]` -1.75, `!` -2.25, `</` -3.0 (greedy leaves the grammar; between
  two params `<` is 0.0). Raw text: `…\n</parameter>\n!\n</parameter>\n</function>\n</tool_call>`. MTP is NOT the cause
  (2/3 without MTP). The fork's qwen3_coder_xml parser takes the LAST `</parameter>` (omlx#2507) → residue becomes payload;
  mlx_lm's parser takes the FIRST → the split looked clean while the same model emitted the same junk.
- `qwen3_xml` in Rapid-MLX = the JSON-body (hermes-style) parser (qwen_tool_parser.py:28) — NOT vLLM's streaming XML parser.
  The vLLM "switch to qwen3_xml" advice does not transfer. Parser stays qwen3_coder_xml.
- FIX (LANDED): Rapid-MLX **v0.14.3-lz.7** = lz.6 + `rapid_mlx/xml_tool_close_guard.py` (eb9ed506d, branch
  lz/xml-param-residue-lz6); goose pin a6e54c8d4. Five skeleton points masked to the template's continuations: line-initial
  `</parameter>` → `\n<parameter|\n</function>`; line-initial `</function>` → `\n</tool_call>`; `<tool_call>` → `\n<function`;
  `</tool_call>` → `\n<tool_call>` | stop ids; after `</tool_call>\n` stop ids stay legal (the guard never forces a call).
  MTP-transactional, on-device; `RAPID_MLX_XML_CLOSE_GUARD=0` = the A/B. FOUR live rounds, each time the model left the
  skeleton somewhere else: (1) after-newline rule only → `!` after `</tool_call>` + 3354 steps of invented tool results;
  (2) → `</parameter>!` one token earlier; (3) spec refused to arm — the scheduler holds mlx-lm's TokenizerWrapper (no
  `len()`; use forwarded `get_vocab()`), loud warning, run = a 2nd control; (4) `</function>\n!` left a call open → 28
  looping calls. Studio receipts: guard off 3/3 replays and a warm CLI census 17/46 calls with residue; guard on 0 residue
  in 20 replays (48 calls) + 3 censuses (105 calls).
- TAG TRAP (2026-09-26): another agent took lz.6 (transient tail on tool) while this was in flight →
  `git ls-remote --tags origin 'v0.14.3-lz.*'` BEFORE naming a tag in any commit; cherry-pick onto the newest lz tag.
- LIVE PROOF RECIPE (Studio, one engine): ~/q85/serve.sh <label> [guard|noguard] (uvx from a fork SHA + parser tap),
  ~/q85/stop.sh <label> (per pid); laptop `ssh -f -N -L 18571:127.0.0.1:18571 workhorse`; replay = the session's
  llm_request input cut before the first poisoned call; census = `goose run --no-profile --with-builtin developer
  --provider openai --model q85`, OPENAI_HOST=http://127.0.0.1:18571, isolated GOOSE_PATH_ROOT, 20 numbered steps.
- OPEN OBSERVATION: on a WARM engine the 27B (engine default sampling, goose CLI) skips/repeats steps and claims calls
  "came back attached" / acts on facts not in its prompt — guard on AND off. Suspect hybrid prefix-cache state; unmeasured.
- RAW-TEXT TAP (no fork edit): `PYTHONPATH=<dir with sitecustomize.py>` that wraps
  `Qwen3CoderToolParser.extract_tool_calls[_streaming]` and appends `current_text` to a jsonl; run the lz env's python
  `…/archive-v0/<env>/bin/rapid-mlx serve …` with PYTHONPATH=<fork worktree>:<tap> to run fork SOURCE without a tag.
  Logprobs: non-stream `logprobs:true, top_logprobs:8` on /v1/chat/completions works on MTP (lz.4+).
- TRAP (cost: the MacBook's GPU, 2026-09-26 ~00:40): a second 27B engine beside another agent's 27B + a tensor rank under
  memory pressure → Metal `kIOGPUCommandBufferCallbackErrorOutOfMemory` mid-decode, the step thread wedged, SIGKILLed
  engines stuck in `?E`, then EVERY Metal client (a 1-line `mx.ones` probe, Finder) in `U`. Check `memory_pressure -Q` and
  `ps … | grep 'rapid-mlx serve'` BEFORE starting a live engine; one 27B at a time per Mac.

## 2026-09-26 — Q-142: split prompt cache vs goose's turn-context block (fixed a8a26ea84, awaiting live prove)
- MECHANISM: goose moves its `<turn-context>` block to the NEWEST message every call (moim.rs, Q-94). On a hybrid
  (ArraysCache, non-trimmable) cache an entry is reusable only if the new prompt holds its WHOLE key. goose already
  names the block as `rapid_mlx_transient_tail` to any engine whose /v1/models entry lists it in `request_extensions`
  (goose-providers openai.rs `transient_tail`; `_on_tool` keeps the block joined to tool results). Single engine lz.6+
  and the pipeline fork snapshot before it; the tensor wrapper declared nothing → only the system segment reused.
- FIX: rank_boundary.py + rank_wrapper.py `_tokenize` cut a segment at the stable boundary (Rapid-MLX's rule, −8 replay
  tokens); mlx_lm 0.31.3's own end-of-segment snapshot stores it. All ranks must cut alike → spec tag
  `mlxLmServerTransientTail`, field `transient_tail_boundary`.
- OFFLINE METHOD (no GPU): the mlx venv python + `mlx_lm.utils.load_tokenizer(<model dir>)` + mlx_lm's own
  `ResponseGenerator._tokenize` on `llm_request.*.jsonl` line 1 (input); token counts matched live input_tokens exactly.
  COPY the rotating llm_request.[0-9].jsonl files first — an E2E rotates them in minutes.
- REAL-mlx_lm CPU TEST RECIPE: tiny random `qwen3_5.Model` saved with save_model/save_config + a byte-level BPE
  (no merges) PreTrainedTokenizerFast with the Qwen3.8 template; `mx.set_default_device(mx.cpu)` AND
  `mx.metal.is_available = lambda: False` (BatchGenerator.__init__ otherwise reads Metal's working set — KeyError on CPU);
  pop MLX_RANK/MLX_IBV_DEVICES/MLX_JACCL_COORDINATOR/MLX_HOSTFILE before ModelProvider (rank_env.py exports them);
  `sys.excepthook` → os._exit(1) (the generation thread is non-daemon: a failed check otherwise hangs forever);
  `print(..., flush=True)` before os._exit.
- goose-side "keep every block in place" was measured and rejected: +337 tokens/call forever for ≤ the previous answer.
- PIPELINE (unfixed, reported): the fork declares only `rapid_mlx_transient_tail` (not `_on_tool`) → goose posts the
  block as its own user turn after tool results on the pipeline split — Q-94's bad shape (Q-108: ~1 in 6 false claims).

## 2026-09-29 — Q-508: the split names conversations by goose's session id (branch q508, not merged)
- CHAIN (measured from code + a wiremock test, no live requests): reply_parts/background_work/context_mgmt scope
  `session_context::with_session_id` → omlx is declarative (engine openai) → `openai_def::from_custom_config` adds
  `session_id_request_builder` → header `agent-session-id: <session id>` on EVERY chat POST (test
  `the_omlx_provider_names_the_session_on_the_requests_the_split_keeps_by`). Bodies carry NO session field (llm_request
  logs: model/stream/stream_options/tools/rapid_mlx_transient_tail/chat_template_kwargs only). Logs never record headers.
- Only `agents/agent.rs` injects the turn-context (moim) → only the agent loop's requests name a transient tail → one
  tail-bearing chat per session id (subagents get their own session id).
- mlx_lm 0.31.3: `CompletionRequest` is a plain dataclass pickled whole by `_share_object` (server.py:492) → any
  attribute set in rank 0's `handle_chat_completions` (`self.headers`) reaches every rank's `_tokenize` (the
  transient_tail precedent). Spec tag `mlxLmServerNamedConversation`, field `name_conversations`.
- GAP (link-backend's surface): leanzero-link inference.rs forwards only content-type/accept
  (`PASSED_REQUEST_HEADERS`) — a request crossing the relay arrives unnamed → token fallback,
  GOOSE_RANK_CONVERSATION_UNNAMED once per User-Agent.
- #3w numbers (rank0 ...1790649911215): head 1,453,850,624 B = 42,019 tok; last pre-compaction prefix 6,946,553,856 B
  = 209,643 tok (08:12:56Z); compaction between 08:12:56Z and 08:18:12Z (ledger: 210,128 → 49,189); compacted calls
  61,757 / 72,638 / 77,686. Bytes→tokens: (B − 76,972,032) / 32,768.
- FAST ITERATION on rank python checks: extract a test's `let checks = r#"…"#` and run it with the preludes
  (rank_prefill.py, rank_boundary.py) under the tensor venv python — seconds, not a cargo build. RED without a fix:
  the pre-fix file + a shim that accepts-and-ignores the new argument → behavioural assertion failures, not TypeErrors.
- WORKTREE-ISOLATION TRAP: bash lines with `$VAR` operands, heredocs to python or `git` inside compound commands are
  refused; write scripts to the scratchpad and run them with literal paths; `git -C <worktree> …` plain.
