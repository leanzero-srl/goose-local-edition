# HANDOFF — floating window fix (branch fix/floating-window, commit 6fd39b5348)

## Done (committed)
- Cause + fix for X/minimize opening goose, close not sticking, peer not showing: see the commit message of 6fd39b5348.
- Files: ui/desktop/src/engineGlanceDesktop.ts (gooseRaised/press/revert/hold, GlanceDismissal use), utils/engineGlanceRules.ts
  (glanceProcesses, GlanceDismissal, pointIn, hosting live), main.ts (wiring, PeerRankWatch every 10 s), peerRankWatch.ts,
  utils/peerRank.ts, utils/engineGlance.ts (runningKeys, peerRanks → hosting with layers), mlxModeLabel.ts (layers line),
  hooks/useMlxTrayActions.ts (idle read 10 s), EngineGlanceDesktopRoot.tsx (press). 8 strings × 16 catalogs.
- Gate: `pnpm run lint:check` EXIT 0; targeted vitest green; negative controls fail 6 new tests. Full vitest: /tmp/x/vitest-all.log.

## Open (NOT done — next agent)
1. Live check on Gabee (`ssh gabee`; laptop is the owner's, in use + benchmark; workhorse/workmule = MiMo, DO NOT TOUCH).
   Gabee's installed Goose (pid 20705 / 70900 at 11:2x) was NOT serving a model; lid closed, external display.
   Recipe (skill goose-feature-dev "THE ONE RECIPE"): package from this worktree on Gabee via
   `~/Projects/machine-sync/bin/node-run --host gabee --cwd ~/Projects/goose-wt-floatwin -- 'source bin/activate-hermit && cd ui/desktop && pnpm install --frozen-lockfile && pnpm run package'`
   (worktree must be synced/pushed to Gabee first: push the branch, `git worktree add` there). Copy goose/tailscale(d) from
   `/Applications/Goose Swarm.app/Contents/Resources/bin/` into ui/desktop/src/bin/ (no cargo needed — UI-only change).
   Quit installed app by pid, launch dev build with GOOSE_PATH_ROOT/GOOSE_USER_DATA_DIR/--remote-debugging-port, config
   `mlx_engine: {port: 8391}`; use references/cdp-acp-override.mjs to craft `distributedStatus` with `hosting` (peer role) and a
   running session → assert the engine-glance page shows "Rank 1 of …" / dismissal holds across blur; then quit by pid,
   `open -a "Goose Swarm" --args --remote-debugging-port=9897`, `~/Projects/machine-sync/bin/goose-single-guard --check`.
2. REAL-CLICK proof is impossible from ssh: CGPreflightPostEventAccess=false on Gabee (ssh and via Terminal .command).
   Needs a person clicking X on the floating window, or a helper granted Accessibility. Main.log line to read:
   "engine glance: put back goose window N its click raised (focus)".
3. ssh-rank path (peerRankWatch) only unit-tested; a live split laptop+Gabee was not run.
4. Remote-single peer: shows while its engine is busy (single engine), no requester name (the peer has no record of who mounted).
5. After merge: release is a separate owner decision.
