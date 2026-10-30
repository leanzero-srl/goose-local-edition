# App surface: live state, mount refusal, memory, release, tray

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Contents
- Live state: the tile, the tray, and "who is using it"
- Mount refusal, load progress, reading vs writing
- Available memory on macOS
- Releasing a notarized macOS build
- Verifying the menu-bar tray live

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
  Detail, tools (sample, the __cxa_throw logger) and traps: `references/distributed-engine-history.md`, section "Q-114 ROOT CAUSE".
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
