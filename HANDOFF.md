# HANDOFF: fix/never-touch-enforce (worktree ~/Projects/goose-never-touch, from origin/main b2f595727 = 3.0.131)

Rule: nothing on the never-touch list or in the protected set may be quit/stopped/unloaded/signalled by ANY path.
Commit with repo identity (leanzero.srl), NO AI credit lines (public repo). Do not release. Delete this file in the final commit.

## Root cause found
- `memfree::categorize` (crates/goose-sidecar/src/memfree.rs) never folded never-touch into the group: a never-touch app
  (Outlook, Teams) stayed `quittable=true`, so the UI `quitOffer` showed Quit next to the "Protected" chip.
- `memfree::quit_app` (sidecar execution point) only checked `category/quittable`, not never-touch/protected.
- ACP `core_memory_free` (local + Link relayed both use it) DID refuse via select_free+recheck (item_refusal), by code reading.
- NEW gap: ACP `mlxEngine/stopOtherEngine` (mlx_engine.rs core_stop_other_engine -> machine::stop_other_engine, SIGTERM+SIGKILL)
  had NO protected/never-touch check. Local only (not in MlxOp relay list).

## Done (uncommitted -> committed as WIP with this file)
1. memfree_protect.rs: `RefusalCode {NeverTouch, Protected}` (`as_str` "neverTouch"/"protected"), `Refusal`, `untouchable(g, list)`
   (never-touch list first, then protected_why; protected_why starting with `NEVER_TOUCH_WHY` => NeverTouch). `eligible` uses it.
   Test `never_touch_apps_are_untouchable_by_bundle_name_and_fragment` (Outlook bundle, "Outlook" fragment, bundle id case, MCP job:node, heavy-run job:python3.12).
2. memfree.rs categorize: never-touch hit => protected_why "on the never-touch list (X)", quittable=false, reclaimable=false.
   `quit_refusal(g, list)` pure + used in `quit_app` (census right before the Apple Event). Test `quit_refuses_never_touch_apps_by_bundle_name_and_fragment`.
3. memfree_servers.rs `app_term_targets` (Force stop) uses `untouchable`.
4. goose-sdk-types custom_requests.rs: `MlxMemoryFreeItemDto.refusal_code: Option<String>` (skip if none).
5. goose acp/server/memfree.rs: `Why {message, code}`; Step::Refuse has `code`; item/server/app_refusal return Why and check
   `untouchable` FIRST; recheck checks untouchable for own engine/orphan too; `refused(.., code)`; literals got `refusal_code: None`
   (indentation needs cargo fmt). Added `pid_refusal` (pure) + `stop_other_engine_refusal(pid)`.
6. mlx_engine.rs core_stop_other_engine: refuses `[code] message; nothing was signalled` before signalling.

## Open items (do in order)
1. `source bin/activate-hermit`; `cargo fmt`; build via ~/Projects/machine-sync/bin/heavy-run (strip=false):
   `cargo test -p goose-sidecar memfree` and `cargo test -p goose --lib memfree` — fix compile errors (other crates constructing
   MlxMemoryFreeItemDto? grep). Existing tests expecting never-touch apps quittable/messages may need updating (e.g. memfree_protect
   `categorize_carries_protection`, ACP `memfree_relayed_never_touch_is_honored` message contains "never-touch" — still true).
2. ACP tests to add in goose acp/server/memfree.rs tests: select_free with quit_apps ["com.microsoft.Outlook"] and force_stop_apps,
   relayed=true remote_switch=true => Refuse code Some("neverTouch"); group label "Microsoft Outlook" with caller list ["Outlook"];
   unload_servers ["job:mcp-web-search-upd"] => code Some("protected"); recheck same; `pid_refusal` on workhorse() groups (mcp pid) => protected.
   Use workhorse()/grp() helpers; grp() makes quittable=true for apps, so set protected_why manually or rely on list.
3. Failing-first proof: temporarily revert the categorize fold, run sidecar tests -> red; restore -> green. Note it in commit msg.
4. UI ui/desktop/src/components/memfree/memfree.ts: `quitOffer(group, allowed, neverTouch?)` and `unloadOffer` -> {show:false}
   when `group.protectedWhy` or `neverTouchHit(group, neverTouch)`; `userApps` exclude protectedWhy. FreeMemoryDialog.tsx line ~752
   pass `neverTouch ?? []`; Force stop button (line ~542): hide when the item's group (holders.groups by id) is protected/never-touch
   or `it.refusalCode`. Add `refusalCode?: string | null` to MemfreeItem. Row already shows Protected chip + reason (protectedReason).
   Tests: memfree.test.ts quitOffer hides for never-touch Outlook + protected; FreeMemoryDialog.test.tsx no `free-memory-quit-app:com.microsoft.Outlook`.
5. Schema: `just generate-acp-types` then `just check-acp-schema` (commit acp-schema.json + ui/sdk generated).
6. Gates: cargo fmt --check; clippy -D warnings (sidecar + goose); memfree tests (sidecar + goose) + `cargo test -p leanzero-link`;
   ui/desktop with hermit Node 24: `pnpm run lint:check` (i18n:check is PRE-EXISTING red, see memory), `pnpm run typecheck`, `pnpm exec vitest run src/components/memfree`, `pnpm run i18n:check` (report as pre-existing if red at HEAD too).
7. Commit (plain `git commit`, no Co-Authored-By), remove HANDOFF.md.

## Paths audited
per-app Quit (core_memory_free Step::App -> quit_app), Force stop (Step::ForceStop -> app_term_targets/terminate_pids), nuclear
(select_free nuclear + recheck), model-server unload (Step::Server -> unload_ai/unload_model/lms_unload_all), other MLX engine stop
(Step::Server -> memfree_servers::terminate_engine), orphan reap (reap_orphaned_goosed), Link relay (MlxOp::MemoryFree -> core_memory_free relayed=true),
mlxEngine/stopOtherEngine (machine::stop_other_engine, GAP fixed), port_holder::claim_port (own-marker proof only, signals goose's own),
sidecar shutdown/supervisor kills (own children only). No other LM Studio/Ollama unload outside memfree.
