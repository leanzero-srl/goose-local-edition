# HANDOFF: fix/fix130 (five 3.0.130 defects) — D2, D13, D14 DONE; D5 leftover + stale cards open

Agent stopped at the ctx-guard 160k warning (system prompt is large); a fresh agent continues here.
This file REPLACES rel/3.0.130's own HANDOFF.md in this branch only (that one still lives on
rel/3.0.130). `git rm HANDOFF.md` before anything merges.

Worktree ~/Projects/goose-fix130, branch fix/fix130 (--no-track) from rel/3.0.130 387134018.
Repo identity is set (leanzero.srl <office@leanzero.net>); never pass -c user.*; no AI credit lines.
Do NOT edit ~/Projects/goose-rel130 (its gate is running). Do not merge, do not release.
Evidence: /Users/mihaiperdum/.claude/plans/goose-node-load-HANDOFF.md lines 419-443 ("Agent 15").

## Gate (copy rel130's script, change the cd)
/private/tmp/claude-501/-Users-mihaiperdum-Projects/9235260e-4bbb-41c2-aefc-cd516bd51e28/scratchpad/gate.sh
-> copy to gate130fix.sh with `cd /Users/mihaiperdum/Projects/goose-fix130`. It uses
CARGO_TARGET_DIR=~/Projects/goose-strategies/target, `--config 'profile.dev.package."*".strip=false'`,
`~/Projects/machine-sync/bin/heavy-run --timeout 3600 --lock-wait 3600 --`. Add
`cargo test -p goose-server remote_executor` (already there) and the vitest/i18n steps. Run it
backgrounded and watch it with Monitor (until-loop on "ALL DONE"). Known: waitClock.test.ts may trip
on git-ignored ui/desktop/src/swarm-bench/.

## D2 — Offline in <= 3.0 s end to end (live 3.4-4.0 s)
Where the time goes (read, not measured): crates/leanzero-link/src/state.rs:451-461
PEER_LIVENESS_INTERVAL 800 ms, PEER_LIVENESS_DIAL_TIMEOUT 1200 ms, PEER_LIVENESS_TIMEOUTS 2
(liveness_peer_loop ~line 800: sleep only when timeouts==0, retry at once). Engine worst =
0.8 + 2x1.2 = 3.2 s. Desktop adds the roster poll: ui/desktop/src/components/leanzero-swarm/
useMacs.tsx:201 LINK_POLL_MS = 1500 (useVisibleInterval, refreshLink = linkStatus + linkNodes, both
in-memory on goosed). No event subscription exists in the desktop (grep NodeStateChanged: none).
Measured live 3.4-4.0 = engine ~2.8 avg + poll ~0.75 avg. Event propagation is not the cost
(mark_peer_unreachable publishes at once; /nodes reads the registry directly).
Fix: INTERVAL 500 ms, DIAL_TIMEOUT 900 ms (two-timeouts rule unchanged) -> engine worst 2.3 s;
LINK_POLL_MS 500 -> worst on screen 2.3 + 0.5 + ~0.05 IPC = 2.85 s. Update the doc comments
(state.rs:451-461 "3.2 s"; useMacs.tsx:200 "~3 s").
Failing-first: crates/leanzero-link/tests/control_service.rs ~984-1035 two tests assert <= 3.5 s at
production cadence (kill -> SocksTimeout path, and port-close -> refused path). Tighten both to
<= 2.4 s (rename fns "..._within_2_4_s_...") — fails today (avg ~2.8). Keep
one_timed_out_liveness_dial_does_not_mark_a_live_peer_offline (~1101) green: that is the
no-false-Offline proof. Desktop: add a vitest asserting LINK_POLL_MS <= 500 with the budget comment
(engine worst 2.3 s + poll <= 3.0 s). Confidence: HIGH on the arithmetic; MEDIUM that 900 ms never
times out twice on a DERP-relayed path (comment says a tailnet connect is well under 500 ms).
Optional, not required: user Disconnect (manager.rs:1644 disconnect -> teardown_active) sends NO
peer-leaving notice; announce_leaving (manager.rs:1462) exists but LeaveReason only has
Quitting/Restarting (wire.rs:72) — adding a variant breaks older peers' parse (400). Leave it.

## D13 — failed prompt turn reported Done/exit 0
Cause: jobs.rs watch_prompt (~1557-1600) only watches session liveness and always finishes
`Done, Some(0)`. A provider failure ("swarm chat: no node can serve this turn", swarm_router.rs:1295)
does NOT error the stream: agent.rs ~3068-3110 yields an assistant Message with
`.with_provider_error(ProviderErrorNotice{class,transient,said,detail})`
(goose-provider-types/src/conversation/message.rs:700/714) and the stream ends normally.
Design:
1. leanzero-link state.rs ~123: add `pub enum RunEnd { Completed, Failed(String), Cancelled }` and
   `fn finish(&self, end: RunEnd) {}` (default no-op) on trait ExecuteLog.
2. jobs.rs FileLog (~1447): hold `Arc<Mutex<Option<RunEnd>>>` + Notify; `finish` stores it.
   watch_prompt: when an end arrives -> Completed => Done Some(0); Failed(r) => Failed Some(1)
   reason r; Cancelled => Cancelled Some(EXIT_CANCEL). Append "[goose-job] end <state> exit <n>
   <reason>" to the log (append_line). Session-gone with NO reported end -> Lost, None, reason
   "the run ended without reporting how it ended" (gate 1: no fabricated Done).
3. goose link_serve.rs drive_remote_reply (~400-490): track `failed: Option<String>` — set when
   an AgentEvent::Message carries metadata.provider_error (use notice.detail), or "error" break;
   run_remote_reply calls log.finish(...) after the "[finished: …]" append. Map "cancelled".
4. goose-server remote_executor.rs pump_job_log (~190-235): MessageEvent::Message with
   provider_error -> remember; MessageEvent::Error -> Failed(error); Finish{reason} -> Completed
   unless a failure was seen ("cancelled" reason -> Cancelled); idle-check end -> Lost path (no
   finish call).
Tests (failing first): jobs.rs unit test with a fake RemoteExecutor whose execute_logged calls
log.finish(RunEnd::Failed("no node can serve this turn")) -> job state Failed, exit 1, reason
contains the text, log has the end line. link_serve.rs test next to
execute_logged_writes_the_runs_end_into_the_sink (~1131) for the provider_error path; goose-server
pump_job_log test. Prompt jobs currently have ZERO tests (grep JobKind::Prompt: jobs.rs only).
Watch: test RemoteExecutors in leanzero-link/tests/{manager,control_service}.rs use the default
no-op finish; none submits prompt jobs, so the Lost path does not break them.

## D14 — deleted ledger dirs stay listed until restart
jobs.rs JobRegistry::list (~1059) / get (~800) serve the in-memory map only; rebuild runs once.
Fix: in list() (and view/get), drop entries whose `entry.dir.join("job.json")` no longer exists
AND whose state is terminal (a running job whose dir vanished: keep it listed but its watchdog
still owns the process — decide: drop only terminal ones, log the drop at info). Failing-first unit
test: submit/insert a finished job, remove_dir_all its dir, list() must not contain it.

## D5 leftover — packaged benchmark runner run_build.py not detected
Launch: ui/desktop/src/main.ts ~4893 (GOOSE_SWARM_BENCHMARK env, argv
`.../Resources/swarm-bench/bench/run_build.py`). Detector: grep "benchmark_run.py" in
crates/leanzero-link/src/load.rs (D5/D6 argv rule) — add run_build.py under a swarm-bench/bench/
path and/or the GOOSE_SWARM_BENCHMARK env marker (read main.ts first for what it sets). Mirror in
~/Projects/machine-sync/bin/node_fleet.py + its test; commit AND push machine-sync (its own repo).
Live evidence: `prepare.py, see benchmark` text inside a claude -p argv must stay NOT flagged.

## Stale cards — My Macs freeze while hidden
useMacs.tsx useVisibleInterval (~232-256) stops polling when hidden. Fix: keep a low-rate poll
while hidden (e.g. 15 s) AND on visible refresh at once; cards render "updated Xs ago" when their
data is older than ~10 s (needs a fetchedAt per roster/facts; i18n key in all 16 locales,
`pnpm run i18n:check`). Vitest with fake timers + defineProperty(document,'visibilityState').

## Status
2026-10-25: D2, D13, D14 coded, failing-first proven, committed on fix/fix130 (not merged/pushed).
D2 ab63e2763: INTERVAL 500 / DIAL_TIMEOUT 900, LINK_POLL_MS 500; link-drop test failed at 3.12 s
before, both <= 2.4 s tests pass after; linkPollBudget.test.ts failed (1500) then passes.
D14 9abedac08: JobRegistry::prune_deleted in get/list/briefs (terminal + job.json gone only).
D13 (commit after 9abedac08): ExecuteLog::finish(RunEnd) default no-op; FileLog -> RunEndSlot; watch_prompt
takes the end from the report, session gone with no report within PROMPT_END_GRACE 5 s = Lost
(the old prompt test now expects lost: FakeExecutor never reports). link_serve: job_run_failure
(provider_error.detail) + run_end; remote_executor pump reports Failed/Completed/Cancelled,
nothing on a silent vanish. Tests: control_service jobs_http a_prompt_job_whose_turn_* (failed
before the fix), link_serve a_provider_error_notice_is_the_runs_failure + run_end, goose-server
the_pump_reports_*. Not live-verified on a real failing swarm turn.
Gate script: scratchpad/gate130fix.sh (log gate130fix.log).
Open: D5 leftover, stale cards (sections above), untouched.
Gate 2026-10-25 20:03 ALL GREEN: fmt, clippy -D warnings (link, goose, goose-server), test-link (all), goose link+link_serve 56, goose-server remote_executor 8, lint, typecheck, vitest 4 files/24.
