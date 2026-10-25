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

## R1 DONE (2026-10-25) — sidecar, commits 381c668fe, 0e0f17d04
Gates: `cargo test -p goose-sidecar memfree` 29 passed (8 new in memfree_protect); clippy -p goose-sidecar
--all-targets -D warnings clean; `cargo check -p goose` clean; fmt done. ACP change: only `CategorizeCtx {..}`
literal now ends `..Default::default()`.

New pub API (crates/goose-sidecar):
- `memfree::ProcRow` += `pgid: u32`, `has_tty: bool`.
- `memfree::CategorizeCtx` += `cmdlines: HashMap<u32,String>`, `launchd_labels: HashMap<u32,String>`,
  `never_touch: Vec<String>`. `groups_now(ctx)` fills cmdlines + labels itself and unions never_touch with
  the defaults — the ACP only passes the request's/settings' extra entries in `ctx.never_touch`.
- `memfree::Group` += `protected_why: Option<String>`, `used_by: Vec<String>` (empty; ACP fills from lsof),
  `launchd_label: Option<String>`, `nuclear_default: bool`.
- `memfree_protect::{DEFAULT_NEVER_TOUCH, never_touch_list(&[String]) -> Vec<String>,
  never_touch_hit(names, &[String]) -> Option<String>, group_never_touch(&Group, &[String]) -> Option<String>,
  parse_launchctl_list(&str) -> HashMap<u32,String>, launchd_labels(), all_cmdlines(), is_mcp_command(&str),
  launchd_owner(rows, labels), ProtectInput{rows,cmds,labels,bundles,self_pid,self_uid,exempt_roots},
  protected(&ProtectInput) -> HashMap<u32,String>, Mode::{Soft,Nuclear},
  eligible(&Group, Mode, &[String]) -> bool, plan(&[Group], Mode, &[String]) -> Vec<String>}`.
- `memfree_servers::terminate_engine(pid, started_at, own_port: u16) -> Result<String>` (SIGTERM only).
- `reap_orphaned_goosed` now never SIGKILLs (Err "still running … goose does not force it").

TRAPS for the ACP step:
- The ACP sets `reclaimable_by_goose = true` on AiServer groups AFTER categorize (core_memory_holders ~l.135,
  ~l.149): it must (a) skip that when `protected_why.is_some()` and (b) recompute
  `g.nuclear_default = memfree_protect::eligible(&g, Mode::Nuclear, &never_touch)` afterwards.
- Soft must become `plan(&groups, Mode::Soft, ..)`; replace the `mlx:` arm's `machine::stop_other_engine`
  with `terminate_engine` (nuclear/explicit only).
- Not done here: design item 5's example `memfree_dryrun.rs`, config GOOSE_MEMFREE_NEVER_TOUCH (item 2 config
  part), lsof used_by, everything in items 3/4/6.

## R2 PARTIAL (2026-10-25) — stopped at the 200k context guard; continue from here
Done (committed in this step): sidecar `memfree_servers::{base_port, parse_lsof_fpc, port_clients, used_by_lines,
app_term_targets, terminate_pids}` + 4 tests (sidecar memfree 33 passed); ACP memfree.rs rewritten: config
`GOOSE_MEMFREE_NEVER_TOUCH` (defaults always apply, setting only adds), `effective_never_touch(config, caller)`,
AI enrichment skips protected + nuclear_default recomputed, `enrich_used_by` (lsof), pure `reach()` (soft =
plan(Soft); gooseOwnedBytes = soft minus own engine when fit), pure `select_free()` -> Vec<Step>, nuclear app
SIGTERM escalation `quit_one_app`, other engine = `terminate_engine`; 5 new ACP tests (lib memfree 10 passed);
clippy -p goose-sidecar -p goose -p goose-sdk-types --all-targets -D warnings clean (clippy needs `cargo clippy
--config '…strip=false'` AFTER the subcommand). Example crates/goose-sidecar/examples/memfree_dryrun.rs built
(--release, target goose-fix130b) and run on the workhorse -> ~/Projects/goose-memfree-dryrun-workhorse.txt.
FIRST RUN FINDINGS: own port default 8090 found 0 engines, 0 other MLX engines (rapid-mlx 97100 not seen — check
it is still running / other_engines detection), legacy soft 0.6 GB (1 group), new soft 0, nuclear default 2
groups 1.1 GB, protected 13 groups 8.2 GB, never-touch 0 (Teams not running?). Re-check the list content.
OPEN: (1) full `cargo test -p goose memfree` result (was running in background, log in the R2 agent's scratchpad —
rerun); (2) append dry-run key lines to ~/Projects/goose-memfree-AUDIT.md "After the fix"; (3) regenerate schema
`cargo run -p goose --bin generate-acp-schema` + `cd ui/desktop && pnpm run build-goose-sdk` (hermit Node 24),
check-acp-schema, commit generated files; (4) caveat: nuclear SIGTERMs an app that refused quit (e.g. a save
dialog) — unsaved work is lost; the UI confirm must say so.
ACP CONTRACT (desktop step): holders request += `neverTouch: string[]`; holders response += `serversBytes`;
group += `protectedWhy?`, `usedBy: string[]`, `launchdLabel?`, `nuclearDefault: bool`; verdict `appsToQuit` now
holds app bundle ids AND server group ids (look up by bundleId OR id). free request += `unloadServers: string[]`
(group ids `ai:…`/`mlx:<pid>`), `nuclear: bool`, `neverTouch: string[]`. Relayed: everything but `gooseOwned`
needs GOOSE_LINK_ALLOW_REMOTE_APP_QUIT (refused per item, message names the key); `quitAppsAllowed` reflects it.

## R3 DONE (2026-10-25) — tests, workhorse dry run, schema
Commits: 8eae34e24 probe subcommand = first non-flag token (rapid-mlx miss); 8f17a0e34 protect CDP-driven browsers
(`--remote-debugging-port/-pipe`, `--enable-automation`, + descendants) and group reason = biggest protected pid's
(System groups always "part of macOS"); abcaa3e60 `memfree_protect::nuclear_default(g, nt)` = eligible(Nuclear) &&
used_by empty (ACP + example use it; explicit picks still validate with `eligible`); 99c1e1878 regenerated
acp-schema.json + ui/sdk/src/generated (acp-meta.json unchanged).
Gates: `cargo test -p goose memfree` 10 passed; `cargo test -p goose-sidecar -- probe:: memfree machine::` 64 passed;
clippy -p goose-sidecar -p goose --all-targets -D warnings clean; check-acp-schema (regenerate + git diff
--exit-code) OK; ui/desktop typecheck clean. ui needed `pnpm install --frozen-lockfile --ignore-scripts` in this
worktree first (no node_modules); the SDK types step is `cd ui/sdk && pnpm exec tsx generate-schema.ts`.
Workhorse dry run (~/Projects/goose-memfree-dryrun-workhorse.txt; AUDIT "After the fix"): legacy soft 2 groups 28.0 GB
(rapid-mlx 97100 27.4 GB footprint + LM Studio), new soft 0, nuclear ticked 1 (LM Studio 0.6 GB), nuclear offered
unticked 1 (97100, used by an sshd-session port-forward), protected 15 groups 9.0 GB, never-touch 0 (none running).
Findings: rapid-mlx missed because global flags precede `serve`; the classifier is shared with the distributed
preflight, which now ALSO sees such engines (new behaviour). Automation Chrome 38975 (mp-vendor-profile, CDP 9333)
was a nuclear default; now protected (whole Chrome bundle group, since a quit names the bundle).
OPEN: (1) desktop step (design item 6) not started; (2) Goose Swarm's protected reason reads "browser driven by
automation" (its CDP renderer is the biggest pid) — true but "runs this goose" would read better; (3) a global flag
with a value before `serve` still misses (safe direction); (4) the unsaved-work caveat for nuclear SIGTERM must be
in the UI confirm text; (5) Gabee live nuclear test (item 8) not done.

## U1 DONE (2026-10-25) — desktop (design item 6), commits 45bce2642, 7f13420bb
- ui/desktop/src/acp/memfree.ts: `memoryHolders({..., neverTouch})`, `memoryFree({gooseOwned, quitApps,
  unloadServers, nuclear, neverTouch, fitModelId})` (contract field names), `readNeverTouchExtras()`.
- memfree.ts (pure, tested): `isServerGroup` (mirror of engine is_server), `unloadOffer`, `isGooseOwnGroup`,
  `DEFAULT_NEVER_TOUCH` (copy of memfree_protect's; keep in sync), `neverTouchExtrasFrom` (list / JSON / comma),
  `effectiveNeverTouch`, `neverTouchHit` (mirror of group_never_touch), `nuclearRows` (offer/protected/left),
  `nuclearRequest` (only ticked offered rows; protected ids dropped), `MEMFREE_NEVER_TOUCH_KEY`.
- FreeMemoryDialog: soft button = gooseOwned only; Unload per server (useConfirmDialog, detail lists usedBy);
  "Free everything except protected" view (lz Checkbox, ticked = nuclearDefault, goose's own as its own row,
  GB each + total, protected/never-touch as accent Chip + reason, left-alone as stopped Chip + why), ONE
  confirm with the SIGTERM/unsaved-work warning, then back to the list with before→after. Protected chip
  + usedBy line also in the list view. Remote-off peer: offers disabled, not pre-ticked.
- Settings › App (MemfreeSettings): Never-touch editor; defaults as chips, additions saved as a list
  (defaults/dupes dropped). Engine `config_never_touch` + union means storing only additions is correct.
- i18n: 28 keys in all 16 catalogs, each inserted after its en-order predecessor (locales are not sorted).
Gates (hermit Node 24.10): lint:check 0 (tsc + eslint --max-warnings 0 + i18n), test:run 554 files / 5171
passed / 3 skipped, i18n:check 0 (4042 messages). New tests: 4 dialog, 6 pure, 1 settings.
OPEN: not run live in the app (no CDP check); item 8 Gabee nuclear test still open; the ticked total uses the
group footprint for servers (the engine's serversBytes counts loaded models), so the GB shown can overstate
what unloading LM Studio frees.

## U2 DONE (2026-10-25) — full gates + live tests
Gates (all rc 0): cargo fmt --check; `cargo clippy --workspace --all-targets -- -D warnings` (re-run -p goose after
the live test was added); `cargo test -p goose-sidecar memfree` 35 passed; `cargo test -p goose memfree` 10 passed;
check-acp-schema (regenerate schema + `cd ui/sdk && pnpm exec tsx generate-schema.ts`, git diff --exit-code) clean;
ui/desktop (hermit Node 24.10) lint:check, typecheck, i18n:check 0; test:run 554 files / 5171 passed / 3 skipped.
Workhorse dry run (final code, nothing executed; appended to ~/Projects/goose-memfree-dryrun-workhorse.txt):
legacy soft 2 groups 30.3 GB; new soft 0 / 0 GB; nuclear ticked 1 (LM Studio 0.6 GB); offered unticked 1
(rapid-mlx 97100 29.7 GB, used by sshd-session 97112); protected 17 groups 40.7 GB (TTS python3.12 heavy-run job
now 32 GB; new: gh + Python under launchd net.leanzero.runner-watch); never-touch 0 (none running).
Gabee live (I was running ON Gabee's Mac, hostname Mac.lan — `ssh gabee` does not resolve there; local run):
new ignored test `acp::server::memfree::tests::live_nuclear_free_quits_only_the_named_app` drives the REAL
core_memory_holders + core_memory_free; env MEMFREE_LIVE_QUIT=<bundle>, MEMFREE_LIVE_NEVER_TOUCH, and
GOOSE_MEMFREE_SHOW_APPS_MIN_MB=0 for a small app. Run: build `cargo test -p goose --lib --no-run`, then
`<deps/goose-*> live_nuclear_free --ignored --nocapture`. App = VLC (TextEdit is com.apple.* = system).
Result: VLC 91798 quit via osascript (ok, 0.1 GB), 1 item only; Teams pids 570 817 78469 before = after (main
Teams app was not open; agent group protected by launchd label); ps diff = VLC + a cycling rsync loop + my ps.
Evidence ~/Projects/goose-memfree-live-gabee.txt.
OPEN: not exercised live: the nuclear SIGTERM escalation (VLC quit cleanly), unloadServers on a real server,
the relayed (Link) refusals, the desktop UI over CDP; Teams main window was not running, so never-touch was
proven by unit tests + the protected launchd agent, not against a live Teams.app main process.
