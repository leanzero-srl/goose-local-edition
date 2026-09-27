# MLX quality loop — BACKLOG (one row per open item; rewritten each tick; closed rows live in FINDINGS-LEDGER.md)

Updated 2026-09-27 (ledger reconciled against main + the release checkouts). Ratio today: 167 rows · 55 proven live · 83 awaiting live prove · 2 cutting · 18 open (+ 4 shipped before proving existed · 3 parked · 2 refuted).
Before the reconcile: 51 "open" · 6 cutting/framed · 49 awaiting · 28 "fixed" with no prove status · 23 proven · 10 other — of the 51 "open": 32 were already fixed and shipped (12 of them proven live), 2 parked, 1 refuted, 16 truly open.

## 1. Awaiting LIVE proof (prove in the named run, then mark "PROVEN LIVE <run>")
| id | what | proven by |
|---|---|---|
| Q-107 · Q-141 · Q-142 | load_tools keeps the prefix; a tool call streams while written; 2nd+ tool calls read within ~4k of input | E2E #3f (27B tensor, jira brief, 3.0.56) calls.csv + chat |
| Q-162 | no hang kill during a long prompt-cache search / compaction call | E2E #3f past its first compaction, rank0 log |
| Q-13 · Q-151 · Q-153 | status line: "reading … of about Y at its measured Z tok/s", then "Writing for …"; counter adds tokens being written | E2E #3f turn 0 screenshots |
| Q-147 | the running session is marked in every list; Active now | livecheck green during E2E #3f |
| Q-132 · Q-109 | checker rows run reasoning-off and never block; "All N steps" with an undone step gets the goose check line | E2E #3f calls.csv checker rows; a 20-step census on 3.0.56 |
| Q-84 · Q-91 · Q-88 · Q-94 | goose check line on a failed write / unsupported claim; condensed pairs are fact records; no empty "new turn" closing | E2E #3f transcript read turn by turn |
| Q-96 · Q-97 · Q-99 · Q-100 | load_tools finds the right family; title within turn 0; tool cards carry the label + error; Thinking rows solid | E2E #3f screenshots |
| Q-87 · Q-89 · Q-92 · Q-93 · Q-98 · Q-68 | two saves to one category both kept; a ~ chat keeps its own ledger; no invented reasons; card not clamped; memory search floor; recall suggestions | E2E memory turn (turn 1–2 of the jira brief) |
| Q-79 · Q-104 · Q-115 | rank memory stays within the plan beside helper calls; goosed writes ~0 MB under a run | GOOSE_RANK_MEM + harness/diskio.py during E2E #3f |
| Q-101 · Q-102 | the agent's shell runs the user's own node, shims only as fallback | `node -v` in an installed-app chat |
| Q-148 · Q-149 · Q-150 · Q-152 · Q-154 · Q-155 · Q-156 · Q-157 · Q-158 | engine + chat surfaces from live round 1 | live critic round 2 on 3.0.56, during E2E #3f |
| Q-12 · Q-41 · Q-42 · Q-43 · Q-44 · Q-45 · Q-46 | chip menu → Open Engine (no Switch-models dead end); fit badges name the Mac; details say whose engine; run book kept; Models chips; GB notes | live critic round 2 |
| Q-67 · Q-71 · Q-72 · Q-80 · Q-82 · Q-130 | code themes both modes; split window wording; split trade-off + Long-documents ranking; Skills count; proposal card origin; Add node names | live critic round 2 |
| Q-120 · Q-125 · Q-123 · Q-124 · Q-129 | fits-once-stopped badge; saved-setup line; tile = Run it = tray run count before/after relaunch; chip = this chat only | critic walk + relaunch with the Studio route up |
| Q-33 · Q-58 · Q-59 · Q-63 · Q-64 | tile "Lost contact … reconnecting" (no raw 502); tray plain words; bar clears with main; one spinner side; one amber episode | recovery.mjs kill-link + relaunch-peer on 3.0.56 |
| Q-111 | "isn't running" only after the Studio's own Leaving, with actions | quit goose on the Studio with a route up |
| Q-17 | a foreign 39k request → the bar says busy, not "ready" | busy.mjs-style foreign request on the Studio, chat open |
| Q-81 · Q-121 · Q-122 | split-stop notice survives relaunch; the cut answer says the split stopped | stop a split under a turn, relaunch, reopen |
| Q-77 | restore waits out goose's own leftover rank | update + relaunch with the split up |
| Q-29 · Q-76 · Q-106 | Run here mid-peer-mount → one engine; unproven pids never signalled; two loads at once → the second waits, named | R5 switch races on 3.0.56 + census |
| Q-134 · Q-145 · Q-160 | pipeline admits a short request beside long ones; aging; admission by KV need | Flash pipeline load 26e (canaries every 60 s) |
| Q-75 · Q-131 · Q-133 · Q-143 · Q-144 | pipeline restores prefixes; every served name answers; undeclared tool = failed call; turn-context on tool results; XML guard armed | E2E #5b on the Flash pipeline |
| Q-138 · Q-139 | no bundled-mcps process after quit; MCP files never in the session dir | quit mid-session + ps; a search + fetch + doc in one chat |
| Q-140 · Q-163 · Q-78 | load-sensitive tests and the pwd flake stay green; clean.sh flags a stand-in rank | the next CI runs on main; clean.sh on the next leftover |
| Q-6 | fixed a63f213c0 (branch worktree-agent-a5c38d284ed596c0f) | swarm chat on the installed build: no "Coding · Agent" toggle, one "Recipes & loops" launcher |
| Q-7 | fixed 83a0e8a44 (branch worktree-agent-a5c38d284ed596c0f) | chat on the split or routed to Work: Recipes & loops › Build a recipe with the fleet answers from that engine |
| Q-9 | fixed 4bd9b5874 (branch worktree-agent-a5c38d284ed596c0f) | bug icon tooltip, aria-label and dialog title all "Report a problem" |
| Q-21 | fixed 099406f01 263678d18 (branch worktree-agent-a5c38d284ed596c0f) | Escape closes Report a problem and the Recipes & loops hub; focus back on the opener |
| Q-60 | fixed dbddaf9c6 (branch worktree-agent-a5c38d284ed596c0f) | kill a split rank mid-chat: the counter keeps "N / 262k" |
| Q-23 | fixed f17a9f5bb (branch worktree-agent-a5c38d284ed596c0f) | Report a problem backdrop half-strength, folder chip under it (both themes) |

## 2. Cutting now
| id | what | owner |
|---|---|---|
| Q-161 | tensor split: one answer of 221,604 tokens = 324 identical write+mkdir pairs | GPU investigation agent (worktree agent-abd19485320c16b61, sole GPU user) |
| Q-164 | one /v1/completions request (IndexError in insert_segments) kills both split ranks | the same agent (same generation path, holds the repro) |

## 3. Open, ranked (dead end > misleads > stability > friction > cosmetic)
| id | class | what | next step (where) |
|---|---|---|---|
| Q-14 | misleads | recall names chatrecall while it is disabled → 26 shell calls hunting a session | name it only when enabled (recall.rs:971-976) · engine |
| Q-18 | misleads | the swarm session still defaults to 128000 until the router's first pick | read context_limit of a 3.0.56 chat in sessions.db; persist the probed window (providers/swarm.rs:623-627) · engine |
| Q-146 | misleads | a streamed split answer can withhold text for 17+ min | read /v1/status stream.tail + withholding (56e1487ba, live since 3.0.55) on the next silent call, then fix from the words · engine (tensor wrapper) |
| Q-37 | misleads | Link "did not come back:" prefix on every supervisor failure | show the supervisor's reason alone (LeanZeroLinkSection.tsx:392-398) · UI |
| Q-103 | stability | a short request waits minutes behind a long prefill (single engine and tensor split) | carry Q-134's between-decode-steps admission to the single engine's MTP path; A/B the canary under R2 3×13k · engine (Rapid-MLX fork) |
| Q-28 | friction | Run on this Mac refused while the split runs | stop the split first via the servingWays switch (PlacementCard.tsx:1456-1462, 1667-1671) · UI |
| Q-38 | friction | the Reconnecting card never shows lastError | pass lastError to ConnectingCard (LeanZeroLinkSection.tsx:435-450, 762) · UI |
| Q-20 | friction | Sampling opens on this Mac while the Studio serves | default to the route's peer (MlxEngineView.tsx:2431) · UI |
| Q-22 | friction | "requests not from this app's chats or /v1" | "N requests from another app" (mlxTray.ts:456, MlxStateTile.tsx:173) · UI |
| Q-26 | cosmetic | a writing engine's Run it chip is grey | pass live activity into wayServing (PlacementCard.tsx:794-826) · UI |
| Q-24 | cosmetic | "recalled: memories … · past session <id>" jargon, faded | plain words in recall_line (recall.rs:879-896) + solid ink · engine words + UI |
| Q-25 | cosmetic | "1 other split › not supported yet" outside Details | move under the split's Details (PlacementCard.tsx:1770-1795) · UI |

Parked (evidence in the ledger): Q-11 (no panic in 115 min of R2), Q-90 (reviewer recall 33/40; the motivating pair 2/8, no design in hand), Q-19 update row (cache-size arms not needed).
