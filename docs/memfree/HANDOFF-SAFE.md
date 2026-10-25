# HANDOFF — fix/memfree-safe (soft clean must not touch running apps/processes)

Audit (DONE, read first): ~/Projects/goose-memfree-AUDIT.md. Verdict: 3.0.130 soft clean NOT safe on the workhorse.
Worktree ~/Projects/goose-memfree-safe, branch fix/memfree-safe (--no-track, from origin/main efdea890d).
Repo identity leanzero.srl <office@leanzero.net> is set; never pass -c user.*; NO AI credit lines (public repo).
Do not release. Do not push to main.

## Code map (3.0.130)
- crates/goose-sidecar/src/memfree.rs: ProcRow, CategorizeCtx, Group, categorize(), protected_pids() (self +
  ancestors + claude + ancestors), never_kill(), HOSTS_SESSIONS, verdict(), census (mac FFI: proc_listallpids,
  proc_pid_rusage V4 [72..80], proc_pidpath, proc_pidinfo PROC_PIDTBSDINFO -> bsd() returns (ppid,uid,start)),
  groups_now(), quit_app() (osascript, 15 s, polls 8 s). Tests in same file from ~line 1004.
- crates/goose-sidecar/src/memfree_servers.rs: Ollama/LM Studio REST, lms_cli(), lms_unload_all(),
  select_orphaned_goosed (THE proof), reap_orphaned_goosed (SIGTERM, 10 s, SIGKILL).
- crates/goose-sidecar/src/machine.rs:634 other_engines, :709 stop_other_engine (SIGTERM->SIGKILL; used also by the
  mlxEngine stopOtherEngine ACP — do not change its behaviour, add a memfree-only TERM-only stop instead).
- crates/goose/src/acp/server/memfree.rs: core_memory_holders / core_memory_free (soft = `gooseOwned`: own engine,
  ALL other MLX engines, ALL AI-server models incl. `lms unload --all`, orphans). Types MlxEngineMemory* in
  sdk custom_requests.rs (grep MlxEngineMemoryFreeRequest). Link relay: `relayed` flag; ALLOW_REMOTE_APP_QUIT_KEY.
- ui/desktop/src/components/memfree/{memfree.ts,FreeMemoryDialog.tsx,*.test.*}, acp/memfree.ts,
  settings/app/MemfreeSettings.tsx. One-click button = `free('own', true, [])`, no confirm.

## Design to implement
1. ProcRow += pgid, has_tty (proc_bsdinfo.pbi_flags & PROC_FLAG_CONTROLT(0x2), or e_tdev != u32::MAX), and a
   cmdline map (sysinfo, for MCP detection). launchd labels: parse `launchctl list` (PID\tStatus\tLabel), skip
   labels starting `application.` (GUI app launches). Pure fn `protected(rows, cmds, labels, self_pid) ->
   HashMap<pid, reason>` = self+ancestors; launchd-labelled pids + descendants + their pgids; MCP servers (argv
   token matching mcp-/-mcp/_mcp/mcp_server/modelcontextprotocol, + descendants); has_tty + descendants; descendants of
   sshd/sshd-session; claude/goose(d) sessions + descendants + ancestors (except own engine pids and proven orphans);
   heavy-run/node-run (argv basename) + descendants + every process sharing a pgid with one of those descendants;
   system (existing rules); terminal/editor bundles (HOSTS_SESSIONS). Group gets `protected_why: Option<String>`.
2. Never-touch: config GOOSE_MEMFREE_NEVER_TOUCH (Vec<String>: bundle id, app name or process name, case-insens.),
   default [com.microsoft.teams2, com.microsoft.teams, com.microsoft.Outlook, com.tinyspeck.slackmacgap, us.zoom.xos,
   com.apple.Terminal, com.googlecode.iterm2, dev.warp.Warp-Stable]. Request also carries `neverTouch` from the
   caller's settings; the executing Mac unions it with its own (so it holds remotely both ways).
3. SOFT (`gooseOwned: true`) = own idle engine + proven orphans only (orphan reap: drop SIGKILL, SIGTERM only).
   gooseOwnedBytes = orphans (+ own engine when no fit). Other MLX engines + LM Studio + Ollama become
   `confirmable` groups with `usedBy: Vec<String>`: TCP ESTABLISHED clients of the server's port
   (`/usr/sbin/lsof -nP -iTCP:<port> -sTCP:ESTABLISHED -Fpc`, exclude the server's own pids), parent process name,
   launchd label. Response += serversBytes, per-group protectedWhy/usedBy/launchdLabel/nuclearDefault.
   verdict(): confirmable = servers (by group id) + apps (bundle id); UI looks up by bundleId OR id.
4. Request += `unloadServers: Vec<group id>`, `nuclear: bool`, `neverTouch: Vec<String>`. Server re-validates every
   item against a FRESH census + protected + never-touch, refuses with a message otherwise. Unload = API (REST per
   model; `lms unload --all` only when that server was explicitly selected); MLX engine = SIGTERM only (new fn).
   Nuclear apps: osascript quit, wait, then SIGTERM per still-alive pid ONLY if uid==self && !protected &&
   !never-touch && !system; never SIGKILL, never a group. Relayed: unloadServers/quitApps/nuclear need
   GOOSE_LINK_ALLOW_REMOTE_APP_QUIT on the peer (default off); soft over Link stays on manage_models.
   After: re-run mount_fit, return before/after (exists).
5. Pure `plan(groups, mode Soft|Nuclear, never_touch) -> Vec<group id>` in sidecar, table-tested; an example
   `crates/goose-sidecar/examples/memfree_dryrun.rs` prints LEGACY-3.0.130 soft, NEW soft, NEW nuclear lists
   (census + launchctl + lsof + GET-only server reads; executes nothing) for the workhorse live dry run.
6. Desktop: one-click button frees soft only; per-server "Unload" with confirm showing usedBy; "Free everything
   except protected" view: every nuclearDefault group ticked (custom checkbox, lz primitives, no native
   select/confirm, no left rails/tints), GB each + total, untick to customise, ONE useConfirmDialog confirm, then
   memoryFree({gooseOwned, quitApps, unloadServers, nuclear:true, neverTouch}); protected chip with reason;
   show after-fit. Settings › App: Never-touch list editor. i18n all 16 catalogs (file order). Regenerate:
   `cargo run -p goose --bin generate-acp-schema` and `cd ui/desktop && pnpm run build-goose-sdk`.
7. Tests (fixtures from the dry run): launchd-labelled MCP server (net.leanzero.mcp-web-search-upd), heavy-run job
   pgid (TTS python 9375 pgid 36740 shape), claude session + its MCP child, Teams (com.microsoft.teams2), rapid-mlx
   other engine 8093 -> never in soft; nuclear never selects protected/never-touch; unticked untouched; relayed
   never-touch honored; soft never includes a non-goose model.
8. LIVE: build the example, scp to workhorse, run (NOTHING executed). Gabee: nuclear against TextEdit you launch,
   Teams on never-touch -> only TextEdit quits (Gabriela is using Gabee: touch nothing else).

## Commands / gates
source bin/activate-hermit; heavy builds via ~/Projects/machine-sync/bin/heavy-run -- <cmd>;
cargo --config 'profile.dev.package."*".strip=false' ...  (macOS 27 sqlx trap). Consider
CARGO_TARGET_DIR=~/Projects/goose-fix130b/target (warm 3.0.130 target).
Gates: cargo fmt; cargo clippy --all-targets -- -D warnings; cargo test -p goose-sidecar memfree; cargo test -p goose memfree;
cd ui/desktop: pnpm run lint:check, typecheck, test:run (hermit Node 24 — bare pnpm is v22 and masks exit codes),
i18n:check; check-acp-schema (grep package.json / justfile).

## Workhorse facts (dry run 2026-10-25)
Goose Swarm 3.0.129 (net.leanzero.goose-swarm), goose serve 65933 parent app. rapid-mlx 97100 ppid 1 port 8093 Tess-27B.
LM Studio 13856 REST needs a token; ~/.lmstudio/bin/lms present; `lms ps` = no models. No Ollama.
TTS python 9375 (pgid 36740), link worker node 14200, rclone 4521, actions runners, 40+ net.leanzero.* labels.
