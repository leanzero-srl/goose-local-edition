# HANDOFF — fix every CONFIRMED finding in ~/Projects/goose-memfree-REVIEW.md (C1..C8)

Worktree ~/Projects/goose-memfree-safe, branch fix/memfree-safe (base 97e41ef19). Repo identity is set:
never pass -c user.*; NO AI credit lines / Co-Authored-By (public repo). Do not push to main, do not release.
Read docs/memfree/HANDOFF-SAFE.md for the code map, commands and gates (hermit Node 24; heavy-run; strip=false).
Every fix lands with a FAILING-FIRST test (write test, see it fail, then fix).

## Code facts already read (don't re-derive)
- ACP: crates/goose/src/acp/server/memfree.rs. `groups()` (~L261) = groups_now + enrich_ai_servers +
  enrich_used_by + `g.nuclear_default = protect::nuclear_default(g, nt)`. `select_free()` (~L507) pure ->
  Vec<Step>; its `check` closure re-checks protected/never-touch/eligible but NOT used_by. `core_memory_free`
  (~L945) builds ONE fresh census before the loop, then runs steps serially (apps take up to 15 s each).
  `quit_one_app` (~L871) SIGTERMs when `!ok && nuclear`. `stop_own_engine` -> `core_unmount(.., Owner)`.
  `server_port` (~L206) = REST port only (LM Studio 1234); `enrich_used_by` (~L221) uses lsof on that port.
- Sidecar: memfree_protect.rs `protected()` (~L239) — `exempt` (own engine + PROVEN orphans + descendants)
  skips EVERY rule except "another user's". `never_touch_hit` (~L48) = exact eq_ignore_ascii_case.
  `eligible`/`nuclear_default` (~L367/L387). MCP rule marks MCP pids + descendants only (not ancestors).
  memfree.rs `categorize` (~L391): group protected when ANY pid in shield (reason = biggest pid's);
  exempt built at ~L608 from own_engine_pids + orphan_goosed_pids; nuclear_default set at ~L657.
  `quit_app` (~L1024): osascript 15 s timeout, polls 8 s, returns ok=false "may be asking to save".
- Soft SIGKILL legs (C5): engine.rs `MlxEngineManager::unmount` (~L2177) -> sidecar.shutdown -> terminate ->
  lib.rs `sigkill_tree_or_pid` (~L1371); Leftover -> `reclaim_port` -> port_holder.rs `claim_port` (~L964).
- Desktop: ui/desktop/src/components/memfree/{memfree.ts (nuclearRows, nuclearRequest, neverTouchHit mirror,
  DEFAULT_NEVER_TOUCH), FreeMemoryDialog.tsx (openNuclear ticks from nuclearDefault ~L318; confirm text;
  soft request ~L506)}, acp/memfree.ts. Types: crates/goose-sdk-types/src/custom_requests.rs
  (grep MlxEngineMemoryFreeRequest / MlxMemoryFreeItemDto). i18n: 16 catalogs, insert after en-order predecessor.

## Design (implement exactly)
1 (C1+C2) `nuclear_default` is FALSE for every server goose did not start: Category::AiServer and every
  `mlx:*` GooseEngine (LM Studio, Ollama, rapid-mlx/other MLX). Still listed + offered + used_by shown.
  Only gooseEngine / proven orphans / plain quittable apps may default ticked. LM Link: when a process named
  `lmlink-connector` (basename) runs, add to the LM Studio group's used_by a line naming it (pid + its LISTEN
  ports via lsof `-a -p <pid> -iTCP -sTCP:LISTEN -Fn`) AND the ESTABLISHED clients of those ports. Pure helper
  in memfree_servers (e.g. `lmlink_users(rows, listen_ports, clients) -> Vec<String>`) with a test.
  Mirror in memfree.ts `isServerGroup` rows: never ticked even if an older engine says nuclearDefault.
2 (C3) Request += `seen_used_by: HashMap<String, Vec<String>>` (serde default, camel `seenUsedBy`); UI sends
  each picked server's usedBy as shown. In core_memory_free, BEFORE EACH Server/App/OwnEngine/Orphan step,
  take a fresh `groups()` and re-validate that one item via a pure fn (e.g. `recheck(fresh, &step, req, nt)
  -> Option<refusal>`): protected/never-touch/eligible again, and for servers any used_by line not in
  seen_used_by[id] => refuse "<label> became used since you looked: <new lines>" (skipped, reported).
3 (C4) quit_one_app never SIGTERMs by default. Not-quit => ok=false, message "didn't quit (maybe unsaved
  work)", new DTO field `force_stoppable: bool` (true when the fresh census still shows it as an unprotected,
  not-never-touch user app of this uid). Request += `force_stop_apps: Vec<String>` (bundle ids): ONLY these
  get app_term_targets + terminate_pids (SIGTERM, never SIGKILL), after the same per-item recheck. Relayed
  needs ALLOW_REMOTE_APP_QUIT like quit_apps. UI: per-item "Force stop" button (lz button + useConfirmDialog,
  confirm says unsaved work is lost), sending memoryFree({forceStopApps:[bid], nuclear:true,...}). Nuclear
  confirm text states plainly: apps are asked to quit; one that does not quit is left running and reported.
4 (C5) memfree's OwnEngine step uses a graceful unmount: SIGTERM (owned group per existing proof rules or
  pid), wait, if still alive report "goose's engine did not exit; goose does not force it" — no SIGKILL.
  Add e.g. `MlxEngineManager::unmount_graceful()` (or a flag) — do NOT change the normal unmount/stopOtherEngine.
  Leftover path: TERM-only too (don't call claim_port's SIGKILL leg).
5 lows: (C6) proven orphans stay exempt ONLY from the sessions rule, the "this is goose" bundle rule and the
  MCP-host rule; launchd/tty/ssh/MCP-self/automation/heavy-run apply to them (own engine keeps full exemption).
  Add a second field in ProtectInput (e.g. `orphan_roots`). Test: orphan under a launchd label / with tty /
  under sshd is protected. (C7) never_touch_hit: name == entry || name contains entry (ci) (bundle id prefix
  covered); test "Teams" protects group label "Microsoft Teams" + "com.microsoft" prefix; mirror memfree.ts
  neverTouchHit + its test. (C8) in protected(): every ANCESTOR of an MCP pid that is inside an .app bundle
  (outer_app Some) is protected "hosts an MCP server (pid N)"; skip pid<=1; test: an Electron app (not in
  HOSTS_SESSIONS) with an MCP child => app group protected, not nuclear_default.

## Then
Regenerate schema (`cargo run -p goose --bin generate-acp-schema`; `cd ui/sdk && pnpm exec tsx generate-schema.ts`).
Gates: cargo fmt; clippy --workspace --all-targets -D warnings; cargo test -p goose-sidecar memfree; cargo test
-p goose memfree; ui/desktop lint:check, typecheck, test:run, i18n:check; check-acp-schema (regen + git diff
--exit-code). Rebuild crates/goose-sidecar/examples/memfree_dryrun.rs (--release), scp to workhorse, run
READ-ONLY; confirm ticked = goose-owned only (LM Studio + rapid-mlx unticked, LM Link in LM Studio used_by).
Append FIXED notes per C1..C8 to ~/Projects/goose-memfree-REVIEW.md. Commit.

## F1 PROGRESS (2026-10-25) — stopped at the context guard; continue from here
DONE (failing-first, red then green; `cargo test -p goose-sidecar memfree` 41 passed):
- 1ad231904 sidecar: C1/C2 `memfree_protect::is_foreign_server` + `nuclear_default` false for AiServer and `mlx:*`
  (categorize now calls nuclear_default too); C6 `ProtectInput.orphan_roots` (categorize passes own engine as
  exempt_roots, orphans as orphan_roots; orphans exempt only from sessions / "this is goose" / MCP-host rules);
  C7 `never_touch_hit` = lowercase contains; C8 MCP-host rule ("hosts an MCP server (pid N)", ancestors in an .app,
  pid>1, not orphans). Tests: memfree_c1_c2_servers_goose_did_not_start_are_never_ticked,
  memfree_c6_a_proven_orphan_keeps_launchd_tty_and_ssh_protection,
  memfree_c7_never_touch_matches_natural_names_and_bundle_prefixes, memfree_c8_an_app_hosting_an_mcp_child_is_protected
  (+ updated a_server_in_use_is_offered_but_not_ticked, categorize_carries_protection, never_touch_matching).
- 1e41015ac sidecar: LM Link — `memfree_servers::{LMLINK_CONNECTOR, parse_lsof_listen_ports, lmlink_users,
  lmlink_used_by, lmlink_lines_for, fn lsof (shared runner)}`; wired into ACP enrich_used_by and the dry-run example.
  Tests: memfree_lmlink_listen_ports_parse, memfree_lmlink_connector_names_its_ports_peers_and_clients.
NOT DONE (in order):
- ACP C3: request `seen_used_by` (HashMap<String,Vec<String>>, serde default) + pure `recheck(fresh,&step,req,nt)`;
  fresh `groups()` before each Server/App/OwnEngine/Orphan step in core_memory_free (~L960), execute with the FRESH
  group. Refactor select_free's `check` closure into a fn reused by recheck.
- ACP C4: quit_one_app never SIGTERMs; ok=false "didn't quit (maybe unsaved work)"; MlxMemoryFreeItemDto +=
  `force_stoppable: bool` (serde default) — constructions at memfree.rs L436/L458/L814/L838/L862/L880; request +=
  `force_stop_apps: Vec<String>` -> new Step::ForceStop(bundle) gated like quit_apps (relayed switch) + recheck, then
  servers::app_term_targets + terminate_pids.
- ACP C5: engine.rs `unmount` (L2177) -> Running: `sidecar.shutdown()` (lib.rs L734: terminate L1338 ->
  sigkill_tree_or_pid L1375); not supervised: reclaim_port -> port_holder claim_port (L964 SIGKILL leg). Add
  `MlxEngineManager::unmount_graceful()` (SIGTERM + wait + "goose's engine did not exit; goose does not force it"),
  leave unmount/stopOtherEngine unchanged; memfree stop_own_engine (memfree.rs ~L823) uses it via a new core fn.
- UI (ui/desktop/src/components/memfree): isServerGroup rows never ticked; send seenUsedBy; Force stop button
  (useConfirmDialog) sending {forceStopApps:[bid], nuclear:true}; nuclear confirm text; neverTouchHit contains-mirror
  + test; i18n 16 catalogs. Then schema regen, all gates, dry run on workhorse, FIXED notes in the REVIEW file.
Build env: `source bin/activate-hermit; export CARGO_TARGET_DIR=~/Projects/goose-fix130b/target;
~/Projects/machine-sync/bin/heavy-run -- cargo test --config 'profile.dev.package."*".strip=false' -p goose-sidecar memfree`.

## F2 PROGRESS (2026-10-25, second agent) — stopped at the context guard; continue from here
DONE (engine complete; `cargo test -p goose memfree` 13 passed, `-p goose-sidecar memfree` 43 passed):
- 37b943c12 C3/C4/C5. ACP memfree.rs: `item_refusal`, `new_users`, `server_refusal`, `app_refusal`, pure
  `recheck(fresh,&step,req,nt) -> Option<Step::Refuse>`; core_memory_free takes a fresh `groups()` before EACH
  non-refuse step and runs it with the fresh group/ai. `Step::ForceStop(bundle)` from `req.force_stop_apps` (same
  explicit/relayed gate + app_refusal); `quit_one_app` never signals -> `not_quit_report` ("<label> didn't quit
  (maybe unsaved work); it was left running (<detail>)", force_stoppable when app_term_targets non-empty);
  `force_stop_app` = SIGTERM via terminate_pids. Own engine: mlx_engine.rs `core_unmount_graceful` ->
  engine.rs `MlxEngineManager::unmount_graceful` (Running -> lib.rs `Sidecar::shutdown_graceful`: killpg/kill TERM,
  grace, no SIGKILL, handle kept if alive; Mounting refused; leftover -> `reclaim_port_graceful` ->
  port_holder `claim_port_with(..,false)` / `stop_signals(force)`); `release_port_with(group, force)` +
  `reclaim_group_residue(..,force) -> bool`. normal unmount unchanged.
  DTOs (goose-sdk-types custom_requests.rs): request `seen_used_by`, `force_stop_apps`; item `force_stoppable`.
  Tests: memfree_c3_each_item_is_rechecked_and_a_new_user_refuses_a_server,
  memfree_c4_nuclear_never_force_stops_unless_the_person_asks, memfree_c4_an_app_that_did_not_quit_is_reported_and_left_running,
  (sidecar lib.rs) memfree_c5_graceful_shutdown_never_sigkills_an_engine_that_ignores_sigterm,
  (port_holder) memfree_c5_an_unforced_port_claim_sends_sigterm_only (written after the code; the lib.rs one was red first).
- bf39436cc schema + ui/sdk generated regenerated (re-run before check-acp-schema gate to confirm clean).
NOT DONE (desktop, in order; files ui/desktop/src/components/memfree/):
- memfree.ts: MemfreeItem += `forceStoppable?: boolean`; nuclearRows: `ticked: !isServerGroup(group) &&
  group.nuclearDefault === true` (server rows never ticked); NuclearFreeRequest += `seenUsedBy: Record<string,string[]>`
  (each ticked server's usedBy ?? []) and `forceStopApps: string[]` (empty); neverTouchHit -> contains-match mirror
  (`names.some(n => n === e || n.includes(e))`, e = entry trimmed lowercase, skip empty). Tests in memfree.test.ts
  first (red): server row with nuclearDefault true is unticked; seenUsedBy sent; "Teams" hits label "Microsoft Teams",
  "com.microsoft" hits bundle prefix.
- FreeMemoryDialog.tsx (~L318 openNuclear, confirm text, result list): per-item "Force stop" lz button on result items
  with forceStoppable, via the app's useConfirmDialog (no native confirm/select); confirm says unsaved work in that app
  is lost; sends memoryFree({forceStopApps:[bundleId], nuclear:true, gooseOwned:false, quitApps:[], unloadServers:[],
  neverTouch}). Nuclear confirm text: "Apps are asked to quit. An app that does not quit is left running and reported."
  Dialog test in FreeMemoryDialog.test.tsx.
- i18n: new keys in all 16 catalogs, each after its en-order predecessor; `pnpm run i18n:check`.
- Gates (hermit Node 24): cargo fmt --check; cargo clippy --workspace --all-targets -D warnings (strip=false config after
  subcommand); both memfree suites; ui/desktop lint:check, typecheck, test:run, i18n:check; check-acp-schema.
- Dry run: rebuild crates/goose-sidecar/examples/memfree_dryrun.rs --release (grep it executes nothing: no kill/quit/
  unload calls), scp to workhorse, `ssh workhorse 'zsh -lc "..."'`, append to ~/Projects/goose-memfree-dryrun-workhorse.txt;
  expect ticked = goose-owned only, LM Studio + rapid-mlx unticked, LM Link line in LM Studio used_by.
- FIXED notes per C1..C8 in ~/Projects/goose-memfree-REVIEW.md (shas 1ad231904 C1 C2 C6 C7 C8, 1e41015ac C1 LM Link,
  37b943c12 C3 C4 C5, + desktop sha).

## F3 DONE (2026-10-25, third agent) — all C1..C8 closed
- a4eb3311c desktop: memfree.ts server rows never ticked, nuclearRequest sends seenUsedBy + forceStopApps [],
  forceStopBundle(), neverTouchHit contains mirror; FreeMemoryDialog per-item Force stop (lz destructive button,
  useConfirmDialog, testid free-memory-force-<bundle>), single Unload sends seenUsedBy, nuclear confirm text honest;
  acp/memfree.ts carries seenUsedBy/forceStopApps. 81488e799 i18n 4 new keys + changed nuclearConfirmBody, 16 catalogs.
- Gates all rc 0: fmt --check; clippy --workspace --all-targets -D warnings; sidecar memfree 43; goose memfree 13
  (1 ignored); lint:check; typecheck; test:run 554 files / 5175 passed / 3 skipped; i18n:check 4046; check-acp-schema clean.
- Workhorse dry run appended to ~/Projects/goose-memfree-dryrun-workhorse.txt (22:15): ticked 0; offered-unticked
  LM Studio 0.6 GB with the LM Link line; rapid-mlx was not running so its leg is covered by unit tests only.
- FIXED notes under C1..C8 in ~/Projects/goose-memfree-REVIEW.md. Open (low): Settings "matches nothing" hint (C7).
