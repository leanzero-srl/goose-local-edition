# goose "Free up memory" audit — 3.0.130 (origin/main efdea890d), 2026-10-25

VERDICT: the shipped soft clean is NOT safe on the workhorse. One click on "Free goose's own" (no confirm)
would SIGTERM then SIGKILL a running rapid-mlx engine another process owns (workhorse pid 97100, port 8093,
Tess-4.5-27B, ppid 1, not goose's), and would run `lms unload --all` against LM Studio for any process using it.

## What the soft path (`memoryFree {gooseOwned: true}`) touches today
Source: crates/goose/src/acp/server/memfree.rs `core_memory_free` (lines ~552-620), sidecar memfree.rs /
memfree_servers.rs / machine.rs. Desktop: FreeMemoryDialog.tsx button `free('own', true, [])` — NO confirm.

| # | action | code | can it hit something another process uses? |
|---|---|---|---|
| 1 | unmount goose's own engine | `core_unmount(EngineStopper::Owner)` when `reclaimable_by_goose` | Guarded: busy = holders record (open reply / swarm run / unreadable) or `active_requests != 0`. OK. |
| 2 | stop EVERY other MLX engine | `machine::stop_other_engine(pid, started_at, port)` for each `mlx:<pid>` group (`machine::other_engines`: any python serving mlx_lm.server / rapid-mlx serve, or distributed, on any port but goose's own) | YES. No in-use check at all; SIGTERM, grace, then SIGKILL. Hits benchmark/eval engines, another goose install's engine, a forge-tuner eval server, a distributed rank. |
| 3 | unload every LM Studio model | `servers::unload_model` per model via REST `POST /api/v1/models/unload`; if REST fails (token required — the workhorse's LM Studio DOES require one) and `~/.lmstudio/bin/lms` exists: `lms unload --all` | YES. Unloads models any client (an MCP server, a script, another session) is using; `--all` is unconditional. |
| 4 | unload every Ollama model | `POST /api/generate {model, keep_alive: 0}` per model from `/api/ps` | YES. No in-flight/client check. |
| 5 | reap left-over goose serve | `reap_orphaned_goosed`: proof = ppid 1, same uid, THIS install's goose binary, `serve --platform desktop --host 127.0.0.1`; SIGTERM, 10 s, SIGKILL | Proof is sound for goose's own orphans; SIGKILL leg unnecessary. |
| 6 | quit apps (per-app button, has confirm) | `quit_app`: osascript `tell application id … to quit` | Only user-picked; blocks system, other uid, goose, ancestors of self/claude, terminal/editor hosts (HOSTS_SESSIONS). NOT blocked: launchd-labelled jobs' apps, Teams/Outlook/Slack/Zoom (no never-touch list), apps hosting MCP servers. |

Over the Link: `gooseOwned` needs only the peer's `manage_models` switch; apps need GOOSE_LINK_ALLOW_REMOTE_APP_QUIT.
So a laptop click on the workhorse card runs rows 2-5 on the workhorse with no confirm.

Not protected anywhere in 3.0.130: launchd labels (net.leanzero.*, com.leanzero.*, actions.runner.*),
MCP servers (except as descendants of nothing), TTY/ssh descendants, heavy-run/node-run process groups,
descendants of claude/goose sessions (only their ancestors are protected).

## Workhorse dry run (read-only, 2026-10-25 ~19:xx, nothing signalled)
Workhorse runs Goose Swarm 3.0.129 (no memfree yet); this is what 3.0.130's soft path WOULD touch once installed
or when reached over the Link:
- rapid-mlx pid 97100 (ppid 1, port 8093, Tess-4.5-27B-MLX-4bit, ~0.5 GB RSS, not launchd, not goose's port) ->
  row 2: SIGTERM/SIGKILL. UNSAFE.
- LM Studio 13856 (+ ~/.lmstudio node 14220, lmlink-connector 14330): REST needs a token (401 invalid_api_key),
  `~/.lmstudio/bin/lms` exists -> row 3 marks it reclaimable and runs `lms unload --all`. `lms ps` right now:
  no models loaded, so today a no-op — but unconditional the moment any model is loaded.
- Ollama: not installed. Orphan goosed: none (goose serve 65933's parent is the app).
- NOT touched by soft (correct): TTS python 9375/9474 (content pipeline, pgid 36740) = `job`, never acted on;
  leanzero-link worker (node/tsx 14200..), rclone 4521, actions runners, headscale, MCP servers = `job` or
  `system`, never acted on by soft. They ARE offerable for nothing (no Quit button: not userApp).
- launchd: 40+ net.leanzero.* / com.leanzero.* / actions.runner.* labels present; none are protected by name,
  they are only safe because soft never touches `job` groups.

## Fix (branch fix/memfree-safe) — see HANDOFF at branch history (handoffs dropped in 72080e804)
Soft = own idle engine + proven orphans (SIGTERM only). Rows 2-4 move to the confirm list with "used by"
(TCP clients of the port, parent, launchd label). Protected set + never-touch list + nuclear mode.

## After the fix (branch fix/memfree-safe, workhorse dry run 2026-10-25 ~21:15, nothing executed)
Full output: docs/memfree/dryrun-workhorse-2026-10-25.txt (example crates/goose-sidecar/examples/memfree_dryrun.rs).
- LEGACY 3.0.130 soft (one click, no confirm): 2 groups, 28.0 GB = rapid-mlx 97100 (27.4 GB footprint, SIGTERM->SIGKILL)
  + LM Studio (`lms unload --all`). The 0.5 GB above was RSS; phys footprint of the live engine is ~27 GB.
- NEW soft (one click): 0 groups, 0 GB (no own engine on 8090, no proven orphan). Correct.
- NEW nuclear ticked by default: 1 group, 0.6 GB (LM Studio, no clients, no models loaded).
- NEW nuclear offered but NOT ticked: rapid-mlx 97100, 27.4 GB, "used by: sshd-session (pid 97112) is connected to it"
  (someone reaches port 8093 over an ssh port-forward). Fix abcaa3e60: anything with used-by is never pre-ticked.
- PROTECTED: 15 groups, 9.0 GB, each reason checked: macOS (part of macOS), Warp/claude (session), Chrome (driven by
  automation: pid 38975 `--remote-debugging-port=9333` mp-vendor-profile; was a nuclear default before 8f17a0e34),
  Goose Swarm (CDP-driven; reason cosmetic, it is also "runs this goose"), TTS python3.12 (heavy-run), Runner.Listener
  + node (actions.runner launchd), headscale, postgres, link-auth esbuild, Playwright (com.user.mcp-web-search),
  BetterDisplay + Sparkle Updater (launchd sparkle jobs), the dry run itself (ssh). MCP servers/link worker/rclone are
  `job` groups: protected or never eligible. Teams/Outlook/Slack/Zoom not running there (never-touch 0, checked pgrep).

### rapid-mlx finding (detection gap in BOTH legacy and new code — FIXED 8eae34e24)
The first dry run saw 0 other MLX engines because 97100's argv is `python rapid-mlx --no-telemetry --no-banner serve …`:
`probe::python_engine_kind` took the subcommand as the FIRST token after the program (`--no-telemetry`), so `serve` was
never matched. Now the subcommand is the first non-flag token (test row 107 in foreign_engines_are_named…). This
classifier is shared with the distributed preflight's foreign-engine check, so that check also missed such engines
before; it now sees them (a mount/Run preflight will now name 97100 as a foreign engine — correct, but new behaviour).
Remaining edge: a global flag WITH a value before `serve` (`rapid-mlx --log-level INFO serve`) still misses (reads
`INFO`); safe direction (miss, not a false positive).

## U2 (2026-10-25 ~21:40) — re-run with the final branch code, plus a real nuclear free on Gabee
Workhorse dry run vs R3: same selections. Legacy soft now 30.3 GB (97100 grew to 29.7 GB); new soft still 0;
nuclear ticked still only LM Studio 0.6 GB; 97100 still offered unticked (sshd-session 97112). Protected 17 groups
40.7 GB (was 15 / 9.0): the TTS python heavy-run job is now 32 GB, and gh + Python under launchd
net.leanzero.runner-watch are new, both protected. Never-touch 0.
Gabee: real core_memory_free, nuclear, quitApps=[org.videolan.vlc], neverTouch=[Microsoft Teams]: only VLC quit
(osascript, no SIGTERM), Teams pids unchanged, ps diff otherwise only transient loops. Full evidence:
~/Projects/goose-memfree-live-gabee.txt. Not live-tested: SIGTERM escalation, server unload, Link relay, UI.
