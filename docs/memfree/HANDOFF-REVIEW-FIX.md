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
