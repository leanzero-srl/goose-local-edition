# Adversarial review: goose fix/memfree-safe (97e41ef19), 2026-10-25

Scope: every path in `git diff origin/main...fix/memfree-safe` that signals, quits or unloads. I checked the code in
~/Projects/goose-memfree-safe and checked the live workhorse READ-ONLY (ps, launchctl list, lsof). Nothing was signalled.

VERDICT: FIX-THEN-SHIP. The soft clean cannot reach anything goose does not own: only `Step::OwnEngine` and
`Step::Orphan` come out of `goose_owned`. The nuclear clean and the remaining SIGKILL leg have confirmed defects.

## CONFIRMED

C1 (high). LM Studio is pre-ticked while LM Link peers are using it.
`memfree.rs` (acp) `server_port()` ~L203 and `enrich_used_by()` ~L217 look for clients only on the REST port (1234).
Live workhorse: `lmlink-connector` pid 14330 listens on `*:34267`. That is LM Studio Link, which other Macs use to reach
this LM Studio's models, and those peers never touch 1234. So `used_by` stays empty, `nuclear_default` is true, and
the dry run shows LM Studio TICKED. When a peer has a model loaded, the confirm runs REST unload or `lms unload --all`
under it. Fix: never pre-tick LM Studio while lmlink-connector is running, or count its port's clients as users.
FIXED: servers goose did not start are never pre-ticked (memfree_protect::is_foreign_server, nuclear_default false for AiServer and mlx:*), LM Link connector port + its clients added to LM Studio's used_by (memfree_servers::lmlink_*). Desktop never ticks a server row even if an older engine says so. Shas 1ad231904, 1e41015ac, a4eb3311c (desktop) + 81488e799 (i18n). Tests memfree_c1_c2_servers_goose_did_not_start_are_never_ticked, memfree_lmlink_connector_names_its_ports_peers_and_clients, vitest 'C1/C2: a server row is never ticked'. Workhorse dry run 2026-10-25 22:15: LM Studio offered NOT ticked, used_by 'LM Link: lmlink-connector (pid 14330) listens on 34267'.

C2 (high). "Used by" only sees a connection that is open at census time.
`memfree_servers.rs` `port_clients()` ~L452 (ESTABLISHED only) feeds `protect::nuclear_default` (memfree_protect.rs
~L392). A job that calls rapid-mlx or LM Studio with short HTTP requests (no keep-alive) between its turns shows
no client, so the engine is pre-ticked. rapid-mlx 97100 (29.7 GB, ppid 1, own pgid, started by hand/nohup, not
launchd) is unticked today ONLY because ssh forward 97112 holds a socket to 8093 (verified with lsof). If the forward
drops, it is pre-ticked. Fix: an engine or server goose did not start is never pre-ticked. Default every
`mlx:*` and `ai:*` group to unticked and let the person tick it.
FIXED: same rule as C1, no reliance on a census-time connection; every mlx:* / ai:* defaults unticked. Shas 1ad231904, a4eb3311c (desktop) + 81488e799 (i18n). Test memfree_c1_c2_servers_goose_did_not_start_are_never_ticked + dialog nuclear test.

C3 (medium). No used-by re-check at confirm time (TOCTOU).
`select_free()` `check` closure (acp memfree.rs ~L549) re-checks protected, never-touch and `eligible`, but NOT
`used_by`. The ticks are fixed once in `openNuclear()` (FreeMemoryDialog.tsx ~L318). If a client connects or a
model loads between opening the panel and pressing Free, it is still stopped or unloaded. Fix: re-run
`enrich_used_by` in core_memory_free and refuse (or re-confirm) a server that gained a user.
FIXED: request seenUsedBy (UI sends each picked server's usedBy as shown, nuclear and single Unload); core_memory_free takes a fresh groups() before each step and recheck() refuses a server with a new used_by line. Shas 37b943c12, bf39436cc (schema), a4eb3311c (desktop) + 81488e799 (i18n). Tests memfree_c3_each_item_is_rechecked_and_a_new_user_refuses_a_server, vitest nuclearRequest seenUsedBy + 'unloads one server' seenUsedBy.

C4 (medium, possible data loss). Nuclear sends SIGTERM to an app that is asking to save.
`quit_app` (sidecar memfree.rs ~L1088) reports "still running (it may be asking to save)" when the app shows a save
sheet, or after the 15 s osascript timeout. `quit_one_app` (acp ~L869) then SIGTERMs every pid when `nuclear` is
set. A Cocoa app exits on SIGTERM without saving, so a person's "Cancel" or an unanswered save sheet becomes lost
work. This follows the spec ("graceful quit then SIGTERM"), but the confirm text does not warn about it. Fix: skip
SIGTERM for an app that is still running after a quit it accepted. Or say it in the confirm dialog.
FIXED: nuclear never SIGTERMs; an app that did not quit is reported 'didn't quit (maybe unsaved work); it was left running' with forceStoppable; only an explicit forceStopApps (per-app Force stop button behind useConfirmDialog saying unsaved work is lost) gets SIGTERM. Nuclear confirm now says apps that do not quit are left running and reported. Shas 37b943c12, a4eb3311c (desktop) + 81488e799 (i18n). Tests memfree_c4_nuclear_never_force_stops_unless_the_person_asks, memfree_c4_an_app_that_did_not_quit_is_reported_and_left_running, vitest 'C4: an app that did not quit gets a Force stop'.

C5 (medium, against "never SIGKILL"). The soft path still reaches SIGKILL.
`Step::OwnEngine` → `core_unmount` → `MlxEngineManager::unmount` (engine.rs L2177). For a supervised engine that is
`sidecar.shutdown` → `terminate` → `sigkill_tree_or_pid` (lib.rs ~L1371: group SIGKILL after the grace). For a
Leftover it is `reclaim_port` → `port_holder::claim_port` (port_holder.rs ~L964: SIGTERM then SIGKILL). Both only hit
goose's own proven engine, so they are not a cross-process hazard. They do contradict the "never SIGKILL" statement
in the handoff and the UI if that statement covers soft. Fix: state it as "nuclear never SIGKILLs other processes",
or give memfree a SIGTERM-only unmount.
FIXED: memfree's own-engine step uses MlxEngineManager::unmount_graceful (SIGTERM, grace, no SIGKILL, reports 'goose's engine did not exit; goose does not force it'); leftover path TERM-only (claim_port_with(..,false)). Normal unmount unchanged. Sha 37b943c12. Tests memfree_c5_graceful_shutdown_never_sigkills_an_engine_that_ignores_sigterm, memfree_c5_an_unforced_port_claim_sends_sigterm_only.

C6 (low). A "proven orphan" is exempt from every other protection.
`categorize` passes `own_engine_pids + orphan_goosed_pids` as `exempt_roots` (sidecar memfree.rs ~L611), and
`protected()` skips exempt pids for the launchd, terminal, ssh and session rules (memfree_protect.rs ~L240). The proof
is: ppid 1, same uid, the same binary as this install, and argv `serve --platform desktop --host 127.0.0.1`. A
desktop-shaped `goose serve` that runs under a launchd plist, or through nohup/setsid from a harness or ssh, passes
that proof and gets one SIGTERM. "Orphan" also does not mean "idle": the busy record and the Link clients are checked
for the own engine only, never for an orphan. Live workhorse: none today (the only serve is 65933 with parent app
65769). Fix: also require no launchd label and no TTY, and read the holders record for the orphan.
FIXED: ProtectInput.orphan_roots; proven orphans are exempt only from the sessions, 'this is goose' and MCP-host rules; launchd/tty/ssh/automation/heavy-run apply. Sha 1ad231904. Test memfree_c6_a_proven_orphan_keeps_launchd_tty_and_ssh_protection.

C7 (low). The never-touch list uses exact matching, so natural names miss.
`never_touch_hit` (memfree_protect.rs ~L52) is a case-insensitive EQUALITY test against the bundle id, the label and
the id name. An owner entry "Teams", "Outlook" or "Slack" does not match "Microsoft Teams" / "Microsoft Outlook" /
"Slack" apps unless the name is exact, and the entry silently protects nothing. The defaults use bundle ids, so the
defaults themselves are fine. I did not check whether MemfreeSettings warns about an entry that matches nothing.
Fix: show "matches: <app>" or "matches nothing" next to each entry.
FIXED: never_touch_hit = case-insensitive equal or contains (engine) and the desktop neverTouchHit mirror. Shas 1ad231904, a4eb3311c (desktop) + 81488e799 (i18n). Tests memfree_c7_never_touch_matches_natural_names_and_bundle_prefixes, vitest 'C7: a natural name or a bundle prefix matches by contains'. Not done: a 'matches nothing' hint in Settings.

C8 (low). An Electron app that hosts MCP servers as children can be pre-ticked.
`protected()` marks MCP processes and their descendants, never their ancestors. An app that is not in
HOSTS_SESSIONS (for example ChatGPT, Codex, Windsurf or Cline hosts) and spawns MCP children stays quittable and
pre-ticked, and quitting it ends those servers. Claude Desktop, VS Code and Cursor are covered. Fix: protect an app
group when any descendant is an MCP server.
FIXED: an .app ancestor of an MCP pid (pid>1) is protected 'hosts an MCP server (pid N)'. Sha 1ad231904. Test memfree_c8_an_app_hosting_an_mcp_child_is_protected.

## REJECTED (each checked)
- Soft reaching a server, an app or another engine: REJECTED. With `goose_owned`, `select_free` emits only
  OwnEngine/Orphan (`soft_kind`). A soft request carries empty unloadServers/quitApps (FreeMemoryDialog L506).
- A Link peer without the switch triggering servers or apps: REJECTED. When `explicit=false`, every server/app/nuclear
  item is refused. The caller's never-touch list is only ADDED to the local defaults and config
  (`effective_never_touch`). The op is still destructive and needs `manage_models`. Note: one switch
  (GOOSE_LINK_ALLOW_REMOTE_APP_QUIT) unlocks app quit, server unload AND other-engine SIGTERM, which is broader than
  its name says.
- PID reuse: REJECTED. reap_orphaned_goosed, terminate_engine and terminate_pids each re-prove (pid, started_at)
  right before kill().
- SIGKILL in the new memfree code: REJECTED. memfree_servers.rs sends SIGTERM only. The only `kill()` there is
  lsof's own child.
- Live misclassification of protected jobs: REJECTED. TTS python and the heavy-run lock (66893, Python.app, matched by
  the `heavy-run` argv), runner, headscale, postgres, link-auth, mcp-web-search Playwright and the automation Chrome
  9333 are all protected. rclone 4521 (leader 4498 gone) is a `job` and never eligible. No orphan goosed. Teams,
  Outlook, Slack and Zoom are not running.
