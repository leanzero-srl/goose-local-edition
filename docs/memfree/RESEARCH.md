# goose "Free up memory" — research (2026-10-25, Gabee's Mac)

Owner problem: My Macs / Nodes card on Gabee (goose 3.0.128) read "Can't run — Does not fit: short 21.2 GB on
Gabee Macbook, needs 30.6 GB, 9.4 GB free", own use 70%. Ask: build a way for goose to help a Mac clear memory.

## 1. What goose already counts as free (code, read 2026-10-25 at origin/main d0d65b401)

- `crates/goose-sidecar/src/memory.rs::darwin_reading`: available = (free − speculative) + file-backed
  (`vm.page_pageable_external_count`) + purgeable. So file cache and purgeable memory are ALREADY counted as free.
- `crates/goose-sidecar/src/fit.rs::budget_bytes`: budget = min(available − RAM × 0.093, Metal ceiling − other
  MLX engines' resident bytes). On a 64 GB Mac the margin is 5.95 GB. "9.4 GB free" on the card is this budget, so
  available was ~15.4 GB; fitting 30.6 GB needs available ≥ 36.6 GB, i.e. ~21 GB of APP memory (anonymous pages,
  compressed, wired) has to leave.
- `machine.rs::other_engines` + `stop_other_engine(pid, started_at, own_port)` (ACP `mlxEngine/stopOtherEngine`)
  already find and stop OTHER MLX engines (mlx_lm.server / rapid-mlx / distributed ranks), pid+start-time checked,
  SIGTERM → grace → SIGKILL. `fit::NodeMemoryFacts.other_engines_bytes` charges them against the ceiling.
- `ui/desktop/src/utils/orphanGoosedReap.ts` (`selectOrphanedGoosed`, `reapOrphanedGoosed`) already reaps
  orphaned `goose serve` processes at app start.
- Remote: `crates/leanzero-link/src/state.rs::MlxOp` + `MlxControl` trait; goose's impl `GoosedMlxControl`
  (`crates/goose/src/acp/server/mlx_engine.rs:1432`) maps `/v1/swarm/mlx/<op>` to the same `core_*` functions,
  all gated by `ControlConfig::allow_remote_execution` (NodeAllows.manage_models), default OFF.
- Card: `ui/desktop/src/components/nodes/nodeGlance.ts` (state `cantRun`, line kind `outcome` with
  `candidate.fit.status === 'short'`, ~line 325) and `NodeCard.tsx` (`primary` action switch ~line 485; `details`
  returns null for MLX). Mac load bars: `components/leanzero-swarm/MacLoadPanel.tsx`.

## 2. What genuinely frees memory on macOS / Apple Silicon (verdicts)

Sources are the standard primary docs: purge(8) and memory_pressure(1) man pages (`man purge`,
`man memory_pressure` on the Mac), LM Studio CLI docs (lmstudio.ai/docs/cli, `lms unload`), Ollama API/FAQ
(github.com/ollama/ollama docs/api.md and docs/faq.md, `keep_alive`), Apple "App Nap" energy guide. NOT re-fetched in
this session (context budget); the next agent should paste the quoted lines before citing them outward.

| Lever | Frees what a GPU load needs? | Verdict |
|---|---|---|
| `purge` (root) | Drops file cache + purgeable. goose already counts those as available, so the budget does not move. | SNAKE OIL for goose's fit (it can even slow the load: weights re-read from disk). |
| `memory_pressure -l warn/critical` | A test tool: allocates memory to SIMULATE pressure so apps release caches. Forces compression/swap of others; jetsam may kill idle daemons. | HARMFUL as a cleaner; never ship. |
| "RAM cleaner" apps (CleanMyMac "Free up RAM" et al.) | Run purge-like cache drops, or allocate-and-release to push pages to compressor/swap. | SNAKE OIL: the gain is file cache (already free to goose) or memory moved to swap that faults back. |
| Activity Monitor / Stats.app | Measurement only (Stats shows the same app/wired/compressed/cache split). Quit in Activity Monitor = the app's own quit. | Fine as instruments, no magic. |
| App Nap | Throttles CPU/timers of hidden apps; does not release memory. | Not a lever. |
| Compressed memory / swap | Pages already compressed still count against goose (not in available). Freed only when their owner frees or exits. | Real only via the owner process. |
| `iogpu.wired_limit_mb` (root sysctl) | Raises the Metal ceiling, does not create RAM; ceiling is not the binding term here (budget = available side). | Out of scope; root; risky (OOM of the whole Mac). |
| Unload idle models in goose's own engine / stop other MLX engines | Real: weights are anonymous/wired GPU memory; fully returned on exit. | WORKS — biggest reliable lever. Already half built (`stop_other_engine`). |
| Orphaned goose serve / swarm engines | Real, small (0.1–1 GB each unless an engine). | WORKS — exists for goose serve at app start. |
| LM Studio `lms unload --all` / its REST | Real: frees the model's memory (process stays). | WORKS when LM Studio is present. |
| Ollama `keep_alive: 0` (POST /api/generate {model, keep_alive:0}, list via GET /api/ps) | Real: unloads immediately. | WORKS when Ollama is present. |
| Docker Desktop VM | Real: the VM's RAM (often 2–8 GB) returns only when Docker quits / VM stops. | WORKS via the app's own quit. |
| Quitting user apps (Chrome, Edge, Teams…) gracefully | Real: the app's footprint + its compressed pages. Unsaved work risk is the app's own quit dialog. | WORKS but only with the person's consent per app. |

## 3. Measured on Gabee (64 GB, 16 KB pages), 2026-10-25 18:08, read-only, nothing quit

vm_stat: free 1.9 GB, file-backed 16.4 GB, purgeable 0.85 GB → goose available ≈ 19.1 GB, budget ≈ 13.1 GB.
Anonymous 35.6 GB, wired 7.6 GB, compressor occupies 1.7 GB (holds 4.6 GB), swap used 0.37 GB.
`memory_pressure -Q` "free percentage 85%" while only 19 GB was available — confirms memory.rs's note that
memorystatus_level is not a usable instrument. No goose model, LM Studio or Ollama was loaded at this moment
(state differs from the screenshot); load average 18.9 (another agent's cargo gate on this Mac).

phys_footprint (`top -l 1 -stats pid,mem`, grouped by .app bundle; script
`/private/tmp/claude-501/-Users-mihaiperdum-Projects/9235260e-4bbb-41c2-aefc-cd516bd51e28/scratchpad/mem/census.py`):
total 28.3 GB over 1000 processes. Google Chrome 6.20 (36 procs), Microsoft Edge 2.65, WindowServer 2.00 (system),
claude CLIs 1.99, root daemons 1.83, Outlook 0.76, node 0.75, Goose Swarm 0.75 (app+goose serve), Warp 0.72,
Blisk 0.69, Word 0.65, WhatsApp 0.50, Sublime 0.28, Arc 0.25, rest < 0.25 each.

Reclaimable by category NOW: goose-owned leftovers ≈ 0 (none running; at screenshot time a loaded model would have
been the main term); other AI engines 0; user apps ≈ 15.5 GB if EVERY user app quit (Chrome+Edge+Outlook+Word+
WhatsApp+Warp+Blisk+Sublime+Arc+...), plus their share of the 4.6 GB compressed; system 0 (never touched).
VERDICT: on Gabee the 21.2 GB gap cannot be closed by goose-owned reclaim alone, and even quitting every user app
lands at about 15–19 GB. The honest product answer for that card is: free what can be freed AND say plainly when
the remaining gap is larger than everything quittable — offer the smaller quant (the Q6 24 GB / Q4 18 GB of the
same model fit in the measured budget after quitting the browsers). Cargo/rustc jobs (agents' gates) are a large,
transient holder on this Mac; they belong to the "jobs" category and are never killed by this feature.

## 4. DESIGN (confidence per piece)

1. Engine census — `crates/goose-sidecar/src/memfree.rs` (new). `proc_pid_rusage(RUSAGE_INFO_V4).ri_phys_footprint`
   per pid (libc / raw FFI; same number as top's MEM), app bundle from the executable path
   (`proc_pidpath`, cut at the first `*.app/`; helpers fold into their outermost bundle), category:
   `gooseEngine` (classify_foreign_engines + own engine port), `gooseOrphan` (goose serve whose parent is
   launchd and no live Goose app — reuse the orphanGoosedReap rule, port it to Rust), `aiServer` (LM Studio,
   Ollama, Docker), `userApp` (owned by this uid, in /Applications or ~/Applications, has a bundle id), `system`
   (other uid, /System, /usr, /Library/Apple, WindowServer, loginwindow, Finder, Dock, the Goose app itself, the
   terminal/Claude processes running this session). Pure grouping/categorise function with a table test.
   Confidence HIGH (all mechanisms measured or already in the repo).
2. Never-kill list: system category, the Goose app and its goose serve, any pid the person's current
   session/terminal descends from, apps not owned by the uid. Unit-tested. HIGH.
3. goose-owned reclaim, one click: unload idle models in goose's engine (Unmount when not serving a reply —
   holders.rs reply locks say if a reply is open), `stop_other_engine` for other MLX engines, kill orphaned goose
   serve (SIGTERM), LM Studio `lms unload --all` (or `POST /api/v0/models/unload` — verify path in its docs),
   Ollama `GET /api/ps` then `POST /api/generate {model, keep_alive:0}` per model. MEDIUM-HIGH (LM Studio REST
   path unverified; Gabee has neither installed, so only fixture-tested there).
4. User apps: per app, the desktop's own ConfirmDialog (house dialog, no window.confirm), then
   `osascript -e 'tell application id "<bundle id>" to quit'` (the app shows its own save prompt; never SIGKILL);
   result re-measured. MEDIUM (AppleScript automation permission prompt "Goose wants to control X" appears the first
   time per app; must be tested live on a throwaway app, not Gabriela's apps).
5. Re-run fit: call the existing `mlxEngine/status` mountFit / placement after the reclaim and show
   before→after budget. HIGH (existing rule, no recomputation in the desktop).
6. ACP: `_goose/unstable/mlxEngine/memoryHolders` {nodeId?} → {availableBytes, budgetBytes, groups[]};
   `_goose/unstable/mlxEngine/memoryFree` {nodeId?, gooseOwned: bool, quitApps: [bundleId]} → per-item report +
   new reading. Remote: `MlxOp::MemoryHolders` / `MlxOp::MemoryFree` through `GoosedMlxControl`, under
   `allow_remote_execution`; quitting apps on a peer additionally needs a NEW switch
   `GOOSE_LINK_ALLOW_REMOTE_APP_QUIT` (config.yaml, default false, read on the PEER inside the handler, reported
   in `NodeAllows.quit_apps`, serde default false). MEDIUM (remote path never exercised yet).
7. UI: "Free up memory" secondary Button on the cantRun card when `fit.status === 'short'` and in MacLoadPanel
   per Mac; a dialog listing groups (solid category chips, bold GB, no left rails, no tints, custom controls),
   "Free goose's own (x GB)" one-click, per-app Quit with confirm, and a final line "short by N GB after
   everything goose can free — try <smaller quant>" when the gap exceeds the reclaimable total. MEDIUM.
8. Tunables (Settings › App): `GOOSE_MEMFREE_ENABLED` (default true), `GOOSE_MEMFREE_SHOW_APPS_MIN_MB`
   (default 200 — hide tiny holders), remote app-quit switch (above). HIGH.

Status: research done; build NOT started in this session (context budget). See
`~/Projects/goose-memfree-HANDOFF.md`.
