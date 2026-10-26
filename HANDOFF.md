# HANDOFF: sticky "Failed" engine card + Engine panel stuck "Checking" (workmule, 3.0.132)

Branch `fix/engine-failure-card-sticks` (created `--no-track` off origin/main 0925b96990), worktree
`~/Projects/goose-wt-enginecard`. NOTHING edited yet except this file. Do not commit the primary checkout
(~/Projects/goose has someone else's staged skill edit). Public repo: no Co-Authored-By trailer (hook rejects).
Push: `git push -u origin HEAD:fix/engine-failure-card-sticks`. No DMG / release.

## The task (owner bug report)
Sidebar red card "Failed · Single · this Mac · <model> · the engine process (pid 25606) exited: exit status: 143 —
no goose path stopped it" stayed for hours; Engine panel showed spinner "Checking · Single · this Mac",
"Served model: no model mounted", RUN IT "Measuring your Macs and the model…" forever. Cause of the death: an
agent ran `kill -TERM` on the rapid-mlx pid at 15:49 local (13:49Z). App restart cleared it.
Asked: (1) dismiss works + card clears itself when status moves on; (2) panel never stuck in Checking/Measuring
(bounded timeout -> real state with retry); (3) external SIGTERM (143) of an idle engine = neutral
"Stopped outside goose", not red; one rule for every place the state is derived; regression test; live check
per goose-feature-dev skill (packaged app + CDP, isolated GOOSE_PATH_ROOT/GOOSE_USER_DATA_DIR) or say unverified.

## Findings so far (file:line at origin/main 0925b96990)
- Sidebar card = `ui/desktop/src/components/engineGlance/EngineGlanceInApp.tsx` `EngineGlanceDockSlot`
  rendering `EngineGlanceCard` (variant dock). The eye-slash = `engine-glance-hide`
  (EngineGlanceCard.tsx:990-997, i18n `engineGlance.hide`) -> `setGlancePrefs({inApp:false})`: it HIDES THE
  WHOLE CARD (persistent pref, replaced by a "Show the engine card" row, `dockRestorable`), it is not a
  dismiss of the failure. Not broken, means something else. Rules: `utils/engineGlanceRules.ts:56-63`.
- Card content: `utils/engineGlance.ts:705-740` single engine: `single.mode==='failed'` -> stage 'failed',
  detail = `single.failedError`. Snapshot built in main by `utils/mlxEngineMonitor.ts` `readSingle` (~588-668):
  `failedError = report.state==='failed' ? report.lastError`.
- ENGINE TRUTH keeps "failed" forever: `crates/goose-sidecar/src/engine.rs:2373-2398` — ManagerState::Running
  with an exited sidecar -> status.state="failed", last_error=`engine_exit_message(&exit)` (engine.rs:1462-1484)
  on EVERY poll until a Mount/Unmount changes ManagerState. The exit sentence is built in
  `crates/goose-sidecar/src/lib.rs:1316-1345` `exit_account` (signal case "the signal came from outside").
  Note: exit status 143 is a normal exit code (launcher/uvx turned SIGTERM into 143), so `signal` is None there —
  the "outside" branch does not fire; a 143 rule must match exit code 143 OR signal SIGTERM with stopped_by None.
  Other copies of the "failed" derivation to unify: engine.rs:1871, 2106 (mount failures — keep red),
  `utils/mlxTray.ts:1027`, `components/leanzero-swarm/MlxStateTile.tsx:1390-1470` (word/phase; Q-254 swap
  rule `swapStopsEngine`), `MlxEngineView.tsx:913`, `utils/engineGlance.ts:519 nodesNavChip`,
  fixtures `utils/nodeSwap.fixtures.ts:194` (already models exit 143 = swap's stop).
- "Checking": `MlxStateTile.tsx:1456` word = state ?? (unreachable ? 'unreachable' : 'checking'); state comes
  from `MlxEngineView.tsx:2651-2680` via `useLocalEngineStatus` (`components/leanzero-swarm/useMlxEngineStatus.ts`)
  -> `createSharedPoll` in `components/leanzero-swarm/sharedStatusPoll.ts`. ROOT of "forever": `tick()` returns
  while `inFlight > 0` and `readNow` has NO timeout — one ACP `mlxEngine/status` call that never resolves freezes
  every watcher of that store for the life of the window (reads stays 0 -> Checking). Same likely for the
  placement plan behind "Measuring your Macs and the model…" (`PlacementCard.tsx:1569`, `NewNodeDialog.tsx:112`):
  check its call for a timeout too.
- Why a status call could hang on the mule: Engine tab passes `fitModelId` -> `core_status`
  (`crates/goose/src/acp/server/mlx_engine.rs:650-682`) runs `manager.mount_fit(model_id)` which reads the model
  folder; the mule's models dir `~/.goose/models` is an NFS mount `192.168.0.2:/` (Thunderbolt from workhorse).
  A stalled NFS read hangs the call; main's monitor (no fitModelId) kept working, which matches the symptom
  (card updated, panel stuck). Also `manager.status()` takes the state lock + `sidecar.exited()` ->
  `last_words()` (lib.rs:313) awaits the stderr reader. NOT PROVEN from logs (only errors are logged;
  log on mule: ~/.local/state/goose/logs/cli/2026-10-26/20261026_154156.log, 6317 "executing a remote mlx op over
  the mesh", 16 status errors all peerUnreachable for the Studio). Fix must not depend on which await hung.

## Plan (do all)
1. sharedStatusPoll.ts: bound every read (e.g. a `timeoutMs` option, default ~3x interval, min 10 s): a read
   that does not land in time counts as FAILED with a clear error ("goose did not answer the engine status in
   Ns") so the view shows the err banner + Retry, and the next tick reads again (in-flight slot released;
   a late answer from the abandoned read is dropped by seq). vitest in sharedStatusPoll test file (fake timers,
   a never-resolving read -> failed snapshot -> next tick reads again).
2. Same bound for the placement plan fetch (find it from PlacementCard `loading`), resolving to an error state
   with retry. Grep every `createSharedPoll(` user; all inherit the bound.
3. Engine-side bound: wrap `manager.mount_fit` in core_status with `tokio::time::timeout` (few s) ->
   mount_fit_error "reading the model folder did not finish in Ns (a network-mounted models folder can stall)".
   Rust test if cheap.
4. External stop rule, ONE place: sidecar/engine side — classify exit with stopped_by None and (exit code 143
   or signal SIGTERM/SIGINT) as "stopped outside goose": status.state "stopped" (not "failed") + a neutral
   `stopped_by`/detail "Stopped outside goose (SIGTERM from another process)". There is already a
   `status.stopped_by = stopped_by_wire(...)` field (mlx_engine.rs:670) and ENGINE_STOPPED_BY — reuse it. Then
   UI: mlxEngineMonitor readSingle gets 'off' for stopped -> card goes grey/away (dockShown needs
   engine.present; stage 'off' => present false). Keep SIGKILL/other codes red. Table test on the pure
   classifier (engine.rs tests near 4009 already assert exit wording).
5. Card clears itself: with (4) + engine truth (Mount/Unmount change ManagerState) it already follows status;
   add a glance-level dismiss of a failure? Prefer: make the eye control's label/tooltip say "Hide the engine
   card" (it is) and add an explicit "Dismiss" (X) on a failed card that clears it until the status's
   lastError changes (renderer-side key = lastError string, held in glanceStore). Test in EngineGlanceCard.test.tsx.
6. i18n: new strings into en.json + all 15 catalogs (file order, not sorted). Gate: see skill (cargo fmt,
   clippy -D warnings, cargo test -p goose-sidecar, cd ui/desktop && pnpm run lint:check && pnpm run test:run).
   In a worktree: `cd ui && pnpm install --frozen-lockfile`, `source bin/activate-hermit`,
   `export CARGO_TARGET_DIR=~/Projects/goose/target` AFTER hermit.
7. Live: packaged app isolated (skill recipe), crafted ACP override via skill references/cdp-acp-override.mjs to
   show (a) status that never answers -> panel leaves Checking after the bound, (b) failed 143 -> neutral.
   Never open a window on the owner's screen without saying so (this is the workhorse; owner may be at it).

## STATE 2026-10-26 (agent 2, stopped at the 160k context guard) — WIP commit, gate NOT run yet
DONE (uncompiled for Rust; vitest green for the two UI test files below):
- NEW `ui/desktop/src/utils/timeLimit.ts` (`withTimeLimit`, `TimeLimitError`, `limitWords`); acpConnection.ts's
  private withTimeout replaced by it.
- `sharedStatusPoll.ts`: `createSharedPoll(read, {what, limitMs})`, `readLimitMs(i)=max(10s,3*i)`; every read
  raced against the limit -> failed snapshot "goose did not answer <what> in N s; reading it again", slot freed,
  late answer dropped (race settled). Users named: useMlxEngineStatus ('the engine status'),
  useMlxDistributedStatus ('the split’s status'), useChatServedBy ('what the engine is doing').
  Tests appended to sharedStatusPoll.test.tsx (describe 'a read goose never answers', 3 tests) — PASS.
- `acp/mlx-placement.ts`: `PLACEMENT_PLAN_LIMIT_MS=45_000` around placementPlan (failure published to
  latestPlacementPlans). Test `acp/mlx-placement.timeLimit.test.ts` — PASS. PlacementCard already has a refresh
  button (header) + err banner; NewNodeDialog has Retry; usePlacementPlans -> 'failed'.
- `MlxEngineView.tsx`: "Engine unreachable" banner gets a "Read again" button (i18n `mlxEngineView.statusRetry`,
  new prop `onRetryStatus` on EngineSection). i18n catalogs NOT yet updated (run i18n:extract / add to en.json +
  15 catalogs, see skill changelog 2026-09-23 (1) for the safe catalog script).
- `crates/goose/src/acp/server/mlx_engine.rs`: core_status mount_fit now via `bounded_fit(MOUNT_FIT_LIMIT=5s,
  &MOUNT_FIT_BUSY, fut)` — spawn_blocking + Handle::block_on, tokio::time::timeout, single-flight AtomicBool with
  Drop guard. NOT COMPILED. Rust test still to write in mlx_engine.rs `mod tests` (~line 1600): a future waiting on
  a std mpsc/oneshot -> Err contains "did not finish in"; second call while busy -> Err "earlier read"; release
  it, then busy clears (never use std::future::pending in spawn_blocking: runtime drop waits forever).

TODO
3. External stop rule (design decided, not coded):
   - goose-sidecar lib.rs `SidecarExit` (372): add `code: Option<i32>`, `signal: Option<i32>` (fill at 474 from
     `status.code()` / `ExitStatusExt::signal`) + `pub fn stopped_outside_goose(&self)->bool` =
     stopped_by.is_none() && (code==Some(143)||code==Some(130)||signal==Some(SIGTERM)||signal==Some(SIGINT)).
     SIGKILL / other codes stay failed.
   - engine.rs status() Running branch (~2380): if exit.stopped_outside_goose() -> state "stopped", model_id None,
     last_error None, new `EngineStatus.stopped_outside: bool = true` (add field ~1254, Default false);
     else unchanged "failed".
   - mlx_engine.rs `stopped_by_wire(state, stopper, outside: bool)`: outside && stopper None -> "outside"
     (update its test ~1778 and the caller at core_status). Update doc on `stopped_by` in
     goose-sdk-types custom_requests.rs:2620 (add `outside`), regenerate schema if a generator touches docs
     (acp-schema.json carries descriptions: `just generate-acp`-ish; check `git grep -n "stoppedBy" crates/goose/acp-schema.json`).
   - mlx_remote_single.rs:1028 restore_step: add arm `Some("outside")` -> Decline("something outside goose
     stopped {peer}'s engine there, so this Mac does not start it again — Run it again to bring '{model}' back")
     + test beside 2476.
   - Table test of the classifier in engine.rs tests (~4009): 143/130/SIGTERM/SIGINT none -> outside;
     137/SIGKILL/1 -> failed; stopped_by Some(..) + 143 -> not outside.
   - UI: stopped -> mlxEngineMonitor readSingle 'off' -> glance present false (card clears). Show the neutral
     words "Stopped outside goose" where the stopped state is named: MlxStateTile word (STATE_WORD 'stopped')
     when status.stoppedBy==='outside' (thread stoppedBy through servingEngine input), test in MlxStateTile test.
     mlxTray.ts:1027 / engineGlance nodesNavChip follow state, no change needed.
4. Dismiss (X) on failed card: EngineGlanceCard.tsx (eye = engine-glance-hide at ~990 keeps its meaning). Add
   `engine-glance-dismiss` X only when stage==='failed'; store dismissed key = detail (lastError) in glanceStore
   (renderer); card hidden while engine.stage==='failed' && detail===dismissedKey; a changed detail shows it
   again. i18n `engineGlance.dismissFailure` "Dismiss this failure". Test in EngineGlanceCard.test.tsx.
5. i18n all 15 catalogs; gate: `source bin/activate-hermit; export CARGO_TARGET_DIR=~/Projects/goose/target`;
   via ~/Projects/machine-sync/bin/heavy-run: cargo fmt --all; cargo clippy --all-targets -p goose -p goose-sidecar
   -- -D warnings; cargo test -p goose-sidecar; cargo test -p goose --lib mlx_; cd ui/desktop && pnpm run
   lint:check && pnpm run test:run. (ui deps already installed in this worktree.)
6. Live check per skill (packaged + cdp-acp-override): never-answering mlxEngine/status -> banner after 10 s;
   status {state:'stopped', stoppedBy:'outside'} -> neutral. Or state unverified.

## STATE 2026-10-26 ~19:05 (agent 3, stopped at the 160k guard) — code for (3) and (4) written, gate NOT run
DONE this round (uncompiled Rust; the two UI test files below PASS in vitest):
- (3) sidecar lib.rs: `SidecarExit` gained `code`/`signal` (filled in `exited()`), `stopped_outside_goose()`
  (stopped_by None && code 143/130 or SIGTERM/SIGINT) + table test `a_stop_signal_from_outside_goose_is_a_stop...`.
  engine.rs: `EngineStatus.stopped_outside: Option<String>` (#[serde(default)]), Running-branch arm -> state
  "stopped", model_id None, words from new `outside_stop_message`; integration test
  `an_engine_stopped_from_outside_goose_reads_stopped_not_failed` (SIGTERM signal + a python that exits 143 on TERM).
  goose-sdk-types custom_requests.rs: `stopped_by` doc names `outside`; NEW DTO field `stopped_outside`.
  mlx_engine.rs: `stopped_by_wire(state, stopper, outside)` (+ tests), DTO mapping, bounded_fit test
  `a_fit_read_that_stalls_is_given_up_on_and_no_second_one_joins_it` (oneshot-held future, multi_thread rt).
  mlx_remote_single.rs: restore_step `Some("outside")` -> Decline + assertion in
  `only_a_stopped_engine_nobody_stopped_is_restored`.
  UI: MlxStateTile `stoppedBy` prop, word `stoppedOutside` ("Stopped outside goose", i18n
  `mlxStateTile.state.stoppedOutside`); passed from MlxEngineView tile + StateBadge. Tests appended to
  MlxStateTile.test.tsx (describe '3.0.132: ... stopped from outside goose') PASS.
- (4) glanceStore.ts: `glanceFailureKey`, `dismissGlanceFailure`, `useDismissedGlanceFailure`,
  `withoutDismissedFailure` (reset in resetEngineGlanceForTests). EngineGlanceInApp DockSlot applies it and passes
  `onDismissFailure`; EngineGlanceCard draws X `engine-glance-dismiss` only when stage failed (i18n
  `engineGlance.dismissFailure`); eye `engine-glance-hide` unchanged. Tests appended to EngineGlanceInApp.test.tsx PASS.
- `cargo fmt --all` ran clean. A background `heavy-run cargo test -p goose-sidecar` was started at ~19:00
  (log /private/tmp/claude-501/-Users-workhorse-Projects/b3c77359-c57d-472f-be64-067a0ce26b03/scratchpad/t1.log) —
  result unknown to the next agent: RE-RUN it.

TODO (next agent, in order)
1. i18n: `cd ui/desktop && pnpm run i18n:extract` exited 127 here (pnpm not on PATH without hermit: run it after
   `source bin/activate-hermit` from the worktree root). New keys: mlxEngineView.statusRetry,
   mlxStateTile.state.stoppedOutside, engineGlance.dismissFailure. Add to all 15 catalogs in FILE order after the
   en.json neighbour (skill changelog: json.dumps(ensure_ascii=False, indent=2)+newline, NOT sorted), translated.
2. Schema: `just generate-acp-types` (DTO field + doc changed; CI check-acp-schema diffs acp-schema.json,
   acp-meta.json, ui/sdk/src/generated). Then the UI can read `status.stoppedOutside` if wanted (not used yet).
3. Gate (heavy-run, one at a time; script
   /private/tmp/claude-501/-Users-workhorse-Projects/b3c77359-c57d-472f-be64-067a0ce26b03/scratchpad/rs.sh = hermit +
   CARGO_TARGET_DIR=~/Projects/goose/target + "$@"): cargo build -p goose-cli; cargo clippy --all-targets -p goose
   -p goose-sidecar -p goose-sdk-types -- -D warnings; cargo test -p goose-sidecar; cargo test -p goose --lib mlx_;
   cargo test -p goose-sdk-types; ui: pnpm run lint:check && pnpm run test:run.
   Check other EngineStatus/SidecarExit consumers compile (only struct literals: engine.rs 2307, lib.rs exited()).
4. Negative controls (copy file to scratchpad, break the fix, run the one test, copy back — never git checkout --):
   stopped_outside_goose -> return false (sidecar tests fail); stopped_by_wire `None if outside` arm removed;
   restore_step outside arm removed (test asserts wording "something outside goose stopped" -> fails);
   MlxStateTile stoppedOutside branch removed; DockSlot `push = live` (dismiss test fails); bounded_fit timeout
   (replace limit with 60 s -> stall test fails/timeouts).
5. Live check per skill (packaged app + cdp-acp-override) or state unverified. Commit, push
   `git push origin HEAD:fix/engine-failure-card-sticks`. No DMG.
- 19:10 RESULT of that background run: goose-sidecar compiles; lib unit tests 490 passed / 0 failed, incl.
  `a_stop_signal_from_outside_goose_is_a_stop_everything_else_is_not` and
  `an_engine_stopped_from_outside_goose_reads_stopped_not_failed` (the latter took >60 s: check whether a settle or
  wait_exited is slow on the 143 leg before calling it done). Integration tests green. DOCTESTS failed with
  E0460/E0463 "found possibly newer version of crate" = the shared ~/Projects/goose/target being rebuilt by another
  worktree at the same time (environmental, not this change): re-run `cargo test -p goose-sidecar --doc` alone.
  Still uncompiled: crates goose + goose-sdk-types (mlx_engine.rs, mlx_remote_single.rs, custom_requests.rs).

## STATE 2026-10-26 ~19:40 (agent 4, stopped at the context guard) — FULL GATE GREEN, negative controls + live NOT done
DONE (commits 76bee1293a i18n, 7ed5ed57e8 acp regen, then "MlxEngineView typecheck" fix):
- i18n: 3 keys in en + all 15 catalogs (file order, translated; script scratchpad add_i18n.py). i18n:check passes (4053 msgs).
- `just generate-acp-types` ran; acp-schema.json + ui/sdk/src/generated committed.
- lint:check found `stoppedBy` missing on the hand-written `MlxEngineStatus` (ui/desktop/src/acp/mlx-engine.ts:66):
  added `stoppedBy?` + `stoppedOutside?`.
- GATE (all EXIT 0): cargo fmt --check; clippy --all-targets -p goose -p goose-sidecar -p goose-sdk-types -D warnings;
  cargo test -p goose-sidecar (unit 490/0, integration, doctests all ok); cargo test -p goose-sdk-types (8/0);
  cargo test -p goose --lib mlx_ (105/0, incl. fit-stall, outside stoppedBy, restore_step outside);
  cargo build -p goose-cli; pnpm run lint:check; pnpm run test:run (555 files passed).
- "SIGTERM test >60 s": MEASURED environmental. Same binary: 110 s twice (each mount->running 36 s), then 4.7 s on
  three runs; sibling a_load_that_ends 26 s -> 2.2 s in the same windows. Per-status calls were <1.2 ms; nothing in
  the test or fix is slow. No change made.
TRAPS (cost time this round):
- Workhorse is macOS 27 now: every cargo cmd needs `--config 'profile.dev.package."*".strip=false'` (sqlx_macros
  "mis-aligned LINKEDIT"). Own target: CARGO_TARGET_DIR=/Users/workhorse/Projects/goose-wt-enginecard/target
  (scratchpad rs.sh sets it) — the shared ~/Projects/goose/target raced with the mimo-distributed worktree.
- Long cargo jobs: heavy-run in background + a foreground `for i in seq; grep -q ^EXIT log; sleep 15` wait (<=600 s).
TODO (next agent)
1. Negative controls (copy file to scratchpad, break, run ONE test, cp back; never git checkout --), each must FAIL:
   lib.rs stopped_outside_goose -> false (a_stop_signal..., an_engine_stopped_from_outside...);
   mlx_engine.rs stopped_by_wire drop `None if outside` arm (an_engine_stopped_from_outside_goose_says_outside...);
   bounded_fit: limit ignored (use 60 s) (a_fit_read_that_stalls...); mlx_remote_single restore_step drop outside
   arm (only_a_stopped_engine_nobody_stopped_is_restored); sharedStatusPoll limit removed (sharedStatusPoll.test.tsx
   'a read goose never answers'); mlx-placement PLACEMENT_PLAN_LIMIT_MS race removed (mlx-placement.timeLimit.test.ts);
   MlxStateTile stoppedOutside branch (MlxStateTile.test.tsx); EngineGlanceInApp DockSlot without
   withoutDismissedFailure (EngineGlanceInApp.test.tsx).
2. Live check per goose-feature-dev skill (packaged app, isolated GOOSE_PATH_ROOT/GOOSE_USER_DATA_DIR, own CDP port,
   references/cdp-acp-override.mjs): never-answering mlxEngine/status -> err banner + "Read again" after 10 s;
   status {state:'stopped', stoppedBy:'outside'} -> tile "Stopped outside goose", sidebar card gone. Else say unverified.
3. Push `git push origin HEAD:fix/engine-failure-card-sticks`. No DMG.

## STATE 2026-10-26 ~20:45 (agent 5) — NEGATIVE CONTROLS + LIVE CHECK DONE; branch ready to merge
Negative controls (break -> run that one test -> restore -> green), all FAIL with the fix broken:
- lib.rs stopped_outside_goose forced false: a_stop_signal_from_outside_goose... FAIL, an_engine_stopped_from_outside_goose_reads_stopped... FAIL
- mlx_engine.rs `None if outside` arm removed: an_engine_stopped_from_outside_goose_says_outside... FAIL
- bounded_fit timeout removed: a_fit_read_that_stalls... HUNG 14 min (never passed) -> test now has an outer 10 s limit
  (fa608fa0e0) and FAILS in 10.0 s; single-flight busy check removed: FAIL
- mlx_remote_single restore_step outside arm removed: only_a_stopped_engine_nobody_stopped_is_restored FAIL
- sharedStatusPoll bound removed: 3/3 'a read goose never answers' FAIL; placement limit -> 1e9: FAIL
- MlxStateTile stoppedOutside branch removed: FAIL; DockSlot without withoutDismissedFailure: FAIL;
  dismissed failure never coming back (key compare dropped): FAIL
Restored: cargo test -p goose --lib mlx_ 105/0; goose-sidecar targets ok; the four vitest files green; fmt + clippy -D warnings clean.
Live (dev package, isolated GOOSE_PATH_ROOT/GOOSE_USER_DATA_DIR, CDP 9461, engine port 8191, mlx-community/Qwen3.5-9B-MLX-4bit):
(a) SIGTERM uv pid -> status stopped/stoppedBy outside, Engine tile "Stopped outside goose", no sidebar card: PASS
(b) SIGKILL -> red card with X + eye; X removes the card (Nodes chip + Engine tile still say Failed, by design); a new failure
    (exit 137) brings the card back: PASS
(c) never-answering mlxEngine/status -> Checking at 4 s, "Engine unreachable: goose did not answer the engine status in 10 s"
    + "Read again" at 13 s; lifted hang + Read again -> real state: PASS
Evidence: /Volumes/AI-workhorse/media/goose/enginecard-2026-10-26/*.png. Traps -> goose-feature-dev skill changelog 2026-10-26.
Open (not this branch): SIGKILL of the uv launcher orphans the rapid-mlx python (still serves the port; status failed, monitor Idle).
