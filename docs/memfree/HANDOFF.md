# HANDOFF — goose "Free up memory" (2026-10-25, written by the research agent on Gabee)

Read first: `~/Projects/goose-memfree-RESEARCH.md` (facts, verdicts, measured numbers, DESIGN with confidence).
Skill: `goose-feature-dev` (layer map, gate, CDP live check). Owner brief = the task text: Phase 2 BUILD.

## State
- Phase 1 research: DONE (file above, also committed as docs/memfree/RESEARCH.md on the branch).
- Phase 2 build: NOT STARTED. The research agent hit the 160k context guard after research.
- Worktree: `~/Projects/goose-memfree`, branch `feat/memfree`, created `--no-track` from origin/main d0d65b401
  (3.0.128). Another agent is releasing 3.0.129 from `~/Projects/goose-fix129*` — never touch those; rebase
  `feat/memfree` onto origin/main after 3.0.129 lands (`git fetch && git rebase origin/main`).
- Machine: Gabee (Mac.lan, M3 Max 64 GB, macOS 27). Gabriela is USING it: never quit/kill her apps.

## Build plan (in order; each step = commit, `<area>: <claim>`, repo identity, NO AI credit lines)
1. `crates/goose-sidecar/src/memfree.rs`: census (proc_listpids + proc_pid_rusage V4 ri_phys_footprint +
   proc_pidpath + uid), pure `categorize(rows, ctx) -> Vec<Group>` and `never_kill(pid,…)`; table tests
   (helpers fold into outer .app; WindowServer/loginwindow/Finder/Dock/other-uid = system; Goose app + its goose
   serve + the caller's ancestors = protected; MLX engines by `distributed::probe::classify_foreign_engines`).
2. goose-owned reclaim in sidecar/goose: unmount own idle engine (only when no open reply — holders.rs), reuse
   `machine::stop_other_engine`, SIGTERM orphaned goose serve (port selectOrphanedGoosed's rule), LM Studio
   (`lms unload --all` if on PATH / its REST — VERIFY the endpoint in lmstudio.ai docs first), Ollama
   (`GET :11434/api/ps`, `POST /api/generate {model, keep_alive:0}`). Each returns freed bytes measured
   before/after by footprint, not guessed.
3. App quit: `osascript -e 'tell application id "<bid>" to quit'`, never SIGKILL, refuse system/protected.
4. ACP (path E): sdk-types `MlxEngineMemoryHoldersRequest/Response`, `MlxEngineMemoryFreeRequest/Response`
   (custom_requests.rs near the mlxEngine types ~2734), handlers in `crates/goose/src/acp/server/mlx_engine.rs`
   (`core_memory_holders/free`), routes in custom_dispatch.rs (pattern: `dispatch_mlx_engine_stop_other_engine`
   ~1016), remote: add `MlxOp::MemoryHolders/MemoryFree` in `crates/leanzero-link/src/state.rs` (path() +
   from_path()) and arms in `GoosedMlxControl::dispatch` (~1432). Remote app quit: new config switch
   `GOOSE_LINK_ALLOW_REMOTE_APP_QUIT` default false, checked on the PEER; add `quit_apps` to `NodeAllows`
   (wire.rs:111, serde default false). Then `cargo run -p goose --bin generate-acp-schema` and
   `cd ui/desktop && pnpm run build-goose-sdk`.
5. Desktop: `ui/desktop/src/acp/` wrapper; a `FreeMemoryDialog` (lz Dialog/Button primitives, solid category chips,
   bold GB, NO left rails / tints / native select / window.confirm); entry points: NodeCard.tsx `primary` for
   state `cantRun` with fit short (nodeGlance.ts ~325 gives `candidate.fit`), and MacLoadPanel.tsx per Mac;
   after a free, re-read `mlxEngine/status` mountFit and show before → after. When the gap > everything
   reclaimable say so and name the smaller quant. Tunables: `GOOSE_MEMFREE_ENABLED` (true),
   `GOOSE_MEMFREE_SHOW_APPS_MIN_MB` (200) in Settings › App (pattern `settings/app/MlxRestoreSettings.tsx`).
   i18n into all 15 catalogs, FILE order (not sorted).
6. Tests: Rust unit (grouping, categories, never-kill), an ACP test (crates/goose/tests, one fn per Config test
   file), vitest for the dialog + the card entry.
7. LIVE on Gabee: load a small model YOURSELF (goose engine, e.g. a ~4–8 GB MLX model already in the models dir),
   measure budget, run "free goose's own" from the packaged app (isolated profile per the skill recipe, own CDP
   port, mlx_engine port != 8090, tell the owner before a window opens on Gabee's screen), assert budget rose by
   the measured footprint (± drift 2% RAM). Do NOT quit any of Gabriela's apps; app-quit live test only on an app
   you launch yourself (e.g. TextEdit with a new empty doc, or Chess).

## Commands
```
cd ~/Projects/goose-memfree && source bin/activate-hermit
# macOS 27 sqlx trap: every cargo build/test here needs
cargo --config 'profile.dev.package."*".strip=false' build -p goose-cli
cargo clippy --config 'profile.dev.package."*".strip=false' --all-targets -- -D warnings
# heavy builds go through: ~/Projects/machine-sync/bin/heavy-run (check its --help)
cd ui/desktop && pnpm install --frozen-lockfile && pnpm run build-goose-sdk && pnpm run lint:check && pnpm run test:run
```

## Traps
- Free memory on the card is the fit BUDGET, not vm free; purge/memory_pressure don't move it (research §2).
- `memory_pressure -Q` percentage is meaningless for this (85% "free" with 19 GB available).
- sysinfo `process.memory()` is RSS, not footprint; use proc_pid_rusage for the census numbers.
- Load average ~19 on Gabee from the 3.0.129 gate: measure before/after within seconds, drift is real.

## Coordinator decisions + facts found by build agent 1 (2026-10-25 18:15, read before coding)
Context budget forces a split: AGENT R (Rust: sidecar census + reclaim + ACP + Link) then AGENT U (desktop + live).
Verdict rule (owner, HONEST FIRST): on a short fit compute from LIVE measurement
  gooseOwnedBytes (own idle engine + own leftover on port + other MLX engines + orphaned goose serve + idle LM Studio/Ollama models),
  appsBytes (phys_footprint of quittable user apps, grouped by outer .app bundle, >= GOOSE_MEMFREE_SHOW_APPS_MIN_MB),
  shortBytes (fit.short_bytes()). verdict: "fitsByGooseOwned" (gooseOwned >= short) | "fitsWithApps" (gooseOwned+apps >= short)
  | "cannotFit" (short - gooseOwned - apps = remaining) + alternatives: smaller variant (same family, other quant/size, from
  the models list / catalog, whose need fits budget+gooseOwned+apps) and/or a peer Mac (mlx_placement::mount_alternative,
  already used by mount_refusal in mlx_engine.rs ~770). Pure fn `verdict(short, goose_owned, apps) -> Verdict` with a table
  test; fixture = Gabee numbers: need 30.6 GB, budget 9.4 GB, short 21.2 GB, gooseOwned 0, apps 15.5 GB -> cannotFit, remaining 5.7 GB.
Existing pieces to REUSE (do not rebuild):
- `MlxEngineManager::start_frees()` (sidecar engine.rs:1736) -> StartFrees::{Nothing, Mounted{model_id,bytes}, Leftover{pids,port,bytes}};
  `global_manager().unmount()` reclaims both Mounted and a proven-own Leftover. Idle = status().active_requests == Some(0) AND no
  open reply in holders::read_all(holders_dir()) (HolderKind::Goosed{replies} non-empty with !waiting = busy; SwarmRun = busy).
- `machine::other_engines(Some(own_port))` + `machine::stop_other_engine(pid, started_at, own_port)` (machine.rs:634/709) for other MLX engines.
  NOTE its resident_bytes is sysinfo RSS (undercounts MLX); census should use phys_footprint instead.
- phys_footprint FFI exists in leanzero-link/src/load.rs:526/757 (`pid_memory_bytes`, RUSAGE_INFO_V4 offsets [72..80]) but
  leanzero-link DEPENDS ON goose-sidecar, so memfree.rs (sidecar) needs its own small FFI (proc_listallpids, proc_pid_rusage,
  proc_pidpath, proc_pidinfo PROC_PIDTBSDINFO for uid+ppid). port_holder.rs:664 already calls libc::proc_listpids.
- ACP pattern: core_stop_other_engine (mlx_engine.rs:804), dispatch route custom_dispatch.rs:1015, types custom_requests.rs:2666-2700,
  remote GoosedMlxControl::dispatch mlx_engine.rs:1432 + MlxOp in leanzero-link/src/state.rs, NodeAllows wire.rs:111.
- fit_to_dto / mount_refusal (mlx_engine.rs:600/770) give fit + alternative placement.
Contract for AGENT U (ACP, camelCase):
  `_goose/unstable/mlxEngine/memoryHolders` {nodeId?, fitModelId?} -> {availableBytes, budgetBytes, totalBytes, fit?: MlxMountFitDto,
     groups: [{id, label, category: "gooseEngine"|"gooseOrphan"|"aiServer"|"userApp"|"system"|"job", bytes, pids:[{pid,startedAt}],
     bundleId?, quittable: bool, reclaimableByGoose: bool, why?}], gooseOwnedBytes, appsBytes, shortBytes?,
     verdict?: {kind: "fitsByGooseOwned"|"fitsWithApps"|"cannotFit", remainingBytes, appsToQuit:[bundleId]},
     alternatives: [{kind:"smallerVariant", modelId, needBytes} | {kind:"peerMac", nodeId, label, budgetBytes}], quitAppsAllowed: bool}
  `_goose/unstable/mlxEngine/memoryFree` {nodeId?, gooseOwned: bool, quitApps: [bundleId], fitModelId?} ->
     {items:[{id,label,ok,freedBytes?,message}], before: {budgetBytes}, after: {budgetBytes}, fit?: MlxMountFitDto}

## AGENT R state (2026-10-25 ~18:30, agent 2 stopped at the 160k guard BEFORE writing code)
Nothing of steps 1-7 is coded yet. The ctx guard fired at 161k: the injected global CLAUDE.md + skill text alone is
~150k, so a fresh agent must NOT load skill `goose-feature-dev` in full (this section carries what it needs) and keep reads tight.
Facts read (file:line at e4cfd942a), so the next agent skips the survey:
- Deps: goose-sidecar has libc 0.2 (unix), sysinfo 0.37, reqwest, serde_json, tokio; NO plist dep. `plist 1.9.0` is already in
  Cargo.lock (transitive) -> `cargo add plist@1.9 -p goose-sidecar --target 'cfg(target_os = "macos")'` costs no new download.
- FFI: libc exposes `proc_pidinfo`, `PROC_PIDTBSDINFO`, `proc_bsdinfo` (pbi_ppid, pbi_uid, pbi_start_tvsec) — usage example
  leanzero-link/src/mesh.rs:1286. `proc_pid_rusage` + `proc_listallpids` are declared by hand in leanzero-link/src/load.rs:611
  (`extern "C" { fn proc_pid_rusage(pid:i32, flavor:i32, buf:*mut c_void)->i32; fn proc_listallpids(buf:*mut c_void,size:i32)->i32; }`,
  rusage V4 = flavor 4, 512-byte buffer, phys_footprint = bytes [72..80] LE, list buffer 8192 i32). Copy that pattern into
  memfree.rs (sidecar cannot depend on leanzero-link). proc_pidpath: declare `fn proc_pidpath(pid:i32, buf:*mut c_void, size:u32)->i32` (buf 4096).
- Own engine: `global_manager()` (engine.rs:2516); `.start_frees().await -> StartFrees::{Nothing, Mounted{model_id,bytes:Result<u64,String>},
  Leftover{pids,port,bytes}}` (engine.rs:1053/1736), `.bytes()`; `.unmount().await -> Result<(),UnmountRefused>` (2177);
  `.status().await.active_requests: Option<u32>` (1221); `.settings().port`; `.mount_fit(model_id).await -> Result<FitVerdict>` (1674).
  IMPORTANT: mount_fit ALREADY credits StartFrees bytes as available (`freed_by_switch_bytes`, engine.rs:1712), so fit.short_bytes()
  already excludes the own engine. In `verdict(short, goose_owned, apps)` pass goose_owned WITHOUT the own-engine bytes, else double count
  (still list the own engine as a gooseEngine group with reclaimableByGoose).
- Busy check: `holders::read_all(&holders::holders_dir()?)` -> Vec<HolderEntry::{Live(HolderRecord), Stale{..}, Unreadable{..}}>;
  HolderRecord.kind = HolderKind::Goosed{replies: Vec<ReplyHold{waiting: bool,..}>} | SwarmRun{..} (holders.rs:83-134).
  Busy = any Live SwarmRun, or Goosed with a reply where !waiting; Unreadable = treat as busy (unknown is never "idle").
- Other engines: `machine::other_engines(Some(own_port)) -> Vec<OtherEngine{pid,started_at,kind,command,port,resident_bytes}>`
  (machine.rs:634; resident_bytes is sysinfo RSS, re-read footprint per pid in the census), `machine::stop_other_engine(pid, started_at, own_port)`
  (709) -> StopReport. `machine::prove(pid, started_at) -> Liveness` (242) and `machine::process_start(pid) -> Option<(u64,bool)>` (275)
  give the pid+start-time check for the goose-serve SIGTERM. `fit::gb(bytes)` formats GB.
- fit.rs: FitVerdict{verdict, need (need.total_bytes(), weights_bytes), facts, budget_bytes, message}, short_bytes() Some only on Block.
- ACP (crates/goose/src/acp/server/mlx_engine.rs): fit_to_dto(model_id,&fit) at 600; core_status 651 (pattern for fitModelId);
  mount_refusal 754 calls `super::mlx_placement::mount_alternative(&model_id, need.weights_bytes).await -> Result<(Option<candidate>, badge), String>`;
  core_stop_other_engine 804 (port = synced_manager()?.settings().port); core_models_list 856 (models via
  `hf::list_local_models(&expand_tilde(&load_engine_settings()?.models_dir))`, each {id,size_bytes,complete}); local handlers in
  `impl GooseAcpAgent` 1120+ with remote relay pattern:
  `if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) { return self.mlx_engine_relay(&node, MlxOp::X, &req).await; }`
  GoosedMlxControl::dispatch match at 1433 (`MlxOp::X => mlx_response_to_value(core_x(mlx_req_from_value(request)?).await)`).
  The relayed path enters core_* directly, so the quit-apps refusal (GOOSE_LINK_ALLOW_REMOTE_APP_QUIT off) goes in a
  `core_memory_free(req, relayed: bool)` param, true from the dispatch arm.
- SDK types: custom_requests.rs MlxEngineStopOtherEngineRequest at ~2685 is the template (JsonRpcRequest + #[request(method, response)],
  camelCase, JsonSchema); MlxMountFitDto at 2713.
- Not yet read: custom_dispatch.rs ~1015 route, leanzero-link state.rs MlxOp path()/from_path(), wire.rs:111 NodeAllows,
  ui/desktop/src/utils/orphanGoosedReap.ts selectOrphanedGoosed rule, LM Studio unload docs (WebFetch lmstudio.ai/docs).
Commands (unchanged): `cd ~/Projects/goose-memfree && source bin/activate-hermit`; every cargo build/test with
`--config 'profile.dev.package."*".strip=false'` (clippy: after `clippy`); heavy ones via `~/Projects/machine-sync/bin/heavy-run --timeout 3600 -- <cmd>`.
Commit ONLY own paths (crates/**, acp-schema.json, acp-meta.json, ui/sdk/src/generated); AGENT U shares the worktree.

## COORDINATOR state (build agent 1, 2026-10-25 ~18:30) — context guard at 170k, stopping
Running in background when I stopped (same worktree, both commit only their own paths):
- AGENT R1: sidecar memfree.rs (census/categorize/never_kill/verdict), own-engine + other-MLX reclaim, graceful app quit,
  ACP types/routes/handlers (local), schema regen commit "acp: memoryHolders/memoryFree schema". Writes "AGENT R1 state" here if cut.
- AGENT U: ui/desktop helpers, acp/memfree.ts, FreeMemoryDialog, NodeCard/MacLoadPanel entries, Settings › App tunables,
  i18n x15, vitest, fixture screenshots. Waits on R1's schema commit. Writes "AGENT U state" here if cut.
STILL TO DO after they finish (next coordinator, fresh session):
1. AGENT R2: Ollama (/api/ps + keep_alive:0) + LM Studio (verify unload endpoint in lmstudio.ai docs first) + orphaned goose serve
   reclaim; Link MlxOp::MemoryHolders/MemoryFree + GoosedMlxControl arms; GOOSE_LINK_ALLOW_REMOTE_APP_QUIT (default false, read on
   the peer) + NodeAllows.quit_apps serde default + old-peer test; one ACP integration test in crates/goose/tests.
2. Full gate (fmt, clippy -D warnings with strip=false config, cargo test touched crates, pnpm lint:check + test:run).
3. LIVE on Gabee: package app (isolated GOOSE_PATH_ROOT + GOOSE_USER_DATA_DIR, own CDP port, mlx_engine port != 8090), load a
   small MLX model in that engine, open Free up memory, free goose's own, assert budget rises by the measured footprint (±2% RAM).
   App-quit live only on an app the agent launched itself (TextEdit/Chess). Never quit Gabriela's apps.
4. Rebase onto origin/main once 3.0.129 lands (`git fetch && git log origin/main`), resolve keeping both; i18n by key replay.
LESSON: subagents here start near 100k+ (CLAUDE.md + memory index); do NOT load goose-feature-dev in full — brief the facts.

## AGENT U state (2026-10-25 18:17, stopped at the context guard ~161k)
DONE (committed, `desktop memfree: ...`): `ui/desktop/src/components/memfree/memfree.ts` (types mirroring the ACP
contract, `verdictOf` mirror of the engine rule, `verdictLine` -> {fitsByGooseOwned freeGb | fitsWithApps freeGb+app
labels | cannotFit everythingGb+shortGb+alternatives}, `groupByCategory`, `quitOffer`, `afterLine`, tunable keys +
readers `memfreeEnabledFrom`/`boolOnFrom`/`minMbFrom`), `memfree.fixtures.ts` (GABEE fixture: short 21.2, apps 15.5 ->
cannotFit 5.7), `memfree.test.ts` 16 tests green. NO i18n strings yet (pure ts, no messages).
NOT DONE (steps 2-8 of the brief): R's schema commit had not landed at 18:17 (`git log --oneline | grep 'acp: memoryHolders'`).
Next agent, in order:
1. After R's commit: `cd ui/desktop && pnpm install --frozen-lockfile && pnpm run build-goose-sdk`; wrapper
   `src/acp/memfree.ts` like `src/acp/mlx-engine.ts` (client.goose.mlxEngineMemoryHolders_unstable / ...MemoryFree_unstable).
2. FreeMemoryDialog: use `OverlayDialog`/`OverlayDialogTitle` (ui/OverlayDialog) + `useConfirmDialog` (ui/useConfirmDialog)
   + lz `Button`/`Chip` and tokens SURFACE/TYPE/WEIGHT/TNUM/TONE_TEXT/cx — exact pattern = `JobDialog` in
   components/leanzero-swarm/MacLoadPanel.tsx:239. Bold GB via ICU `<b>{gb}</b>` + `b: (c) => <strong>{c}</strong>`
   (DictationSettings.tsx:41 does it). GB format = GiB, 1 decimal (NodeCard.tsx:178 `gb`).
3. Entry points: NodeCard.tsx `primary` switch case 'details' returns null for mlx (line ~558) — add the button when
   glance.state==='cantRun' && glance.line.kind==='outcome' && glance.line.candidate.fit.status==='short'; card opens the
   dialog itself (no new onAction kind). `useEdition()` THROWS without EditionProvider and NodeCard tests don't wrap one ->
   add a non-throwing reader in contexts/EditionContext.tsx. MacLoadBars (MacLoadPanel.tsx:120) is used by NodeCard:626
   and MyMacs:551 — add the per-Mac entry there (target = Link nodeId; omit nodeId for the self Mac).
4. Settings › App: copy MlxRestoreSettings.tsx (acpReadConfig/acpUpsertConfig); register beside it in AppSettingsSection.tsx.
5. i18n: extract, then all 15 catalogs in FILE order (not sorted), json.dumps(ensure_ascii=False, indent=2)+'\n'.
6. Gate `pnpm run lint:check && pnpm run test:run`; screenshots per skill recipe into the scratchpad memfree-shots/.
