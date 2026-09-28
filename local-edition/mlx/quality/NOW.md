# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 21:2x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.72 on BOTH Macs 21:0x (DMG kept). The SPLIT could not start: MacBook 12 GiB free (other sessions'
  jest/python + swap 79 GB) — goose refused loudly and correctly. 27B runs on the STUDIO SINGLE over Link
  (split-start --place studio, 27 s). LIVE: answered 20260928_17's inactive-lead card via the recommended button →
  card cleared, item 'answered', answer message delivered 18:00:09Z; its answer turn = 176k-token cold prefill on
  the Studio (10+ min). Q-409 seen LIVE (capability in activity.baseUrl); Q-417 filed (remote: modelId null).
- Was: 3.0.71 on BOTH Macs 18:0x (DMG kept), split-start smoke OK ('OK'); Q-350 LIVE-PROVEN (activity read
  'distributed/mounting … warming' during the restore). E2E #3s STOPPED at turn 3 (memory hold/Q-397 under 40 GB swap — other sessions + my agents'
  cargo; see E2E-RUNS). Next E2E (#3t) on 3.0.72 with NO cargo on the MacBook. Was session 20260928_30 — Q-390 LIVE-PROVEN (workingDirVerified, opened via Projects → New session here).
- Was: 3.0.70 on both Macs (main 95324ea8d: critic batches A–E, Q-298 needs-you supersede, Q-292/293 NOT yet).
  Split up. E2E #3r ENDED 17:2x at turn 18 (NOT a hang — my misread: a `find $HOME` shell call ran its 300 s,
  Q-394 compaction lost the work folder; then a memory hold my builds caused failed the turn, Q-397. Q-392 →
  stream-reset wording + Q-393 log flush, done (worktree-agent-a6310015d97e90297); Q-394, Q-397 cutting). Was: E2E #3r RUNNING (RU-2026-09-28-3r-split-tensor, jira brief) since 13:24 — #3q died when MY probe
  navigated the main window (r1's chatUrl then pointed at the Engine tab); turn 0 big call 40,764/56,280 cached.
- LIVE-PROVEN on 3.0.69 (#3p, 13 turns): Q-294/295/296 — every turn's first call 97–99% from cache (was 0% on #3o
  turns 3/5); Q-297 holding (skill loaded once). #3p turn 9 = a compaction that re-read ~143k cold → Q-342.
- On main after 3.0.70 (→ 3.0.71): Q-292/293 (Run it row + gate chip read Q-276's credit), Q-325 (focus ring
  drew nothing app-wide; 664 dead classes remapped + a guard — VISUAL blast radius ~400 host sites: walk it live),
  Q-337/338 (owner: cached vs new two-part prompt-read bar; split position includes the cached prefix).

## CI
- Green through fe97eb652; 0828e9ca8/838e041a9 running.

## Agents
- Q-344 (a card answer must not supersede its sibling; ON the Q-340/341 branch — they land together) ·
 
- Q-342 MERGED to main 757c3458a (gate green). Q-346 ON MAIN d710d7059 (gate green). Q-350+343 /tmp/merge-q350: UI green (vitest 4169), cargo running. Q-347 (stable head kept
  entry; wire tag → BOTH Macs need 3.0.71) → /tmp/merge-q347, gate /tmp/g347.out after g350. All gates run from
  scratchpad/gates.sh + gate347.sh in their own session (five background gates were killed, cause unproven). Disk swept 16 → 75 GB (my old scratchpad held 56 GB).
- Q-367 (tool-not-found error leads with the closest real tools) + Q-369 (load_tools' false message) DONE →
  /tmp/merge-q367, gate /tmp/g367.out after g347. Q-368 = model behaviour (no goose cause; 4 restatements, turn 2). Turn 2 ended 761 s, 27 tools, a correct
  sourced answer (EOL 2026-03-30 / 2028-03-30 / 2029-03-28). Turns 3–4 done (1106 s / 176 s);
  turn 3's lost `=>` = the MODEL's sampling (token-CRC proof, Q-371); found Q-372
  (a value holding </parameter> was cut) → /tmp/merge-q371, gate /tmp/g371.out after g359; Q-385 SCHEDULED
  (model blames the tool; sampling measure waits on the split free). Turn 5 running. Studio-side: forge-tuner got a
  goose tool-call training proposal (docs/GOOSE-TOOLCALL-ROUND.md, 6631628) — owner picks public vs private data.
- Owner features (designs DESIGN-Q357/358/359-*.md): Q-357 compaction revamp CUTTING (all slices, on merge-q342) ·
  Q-358 search S1+S2 CUTTING; notes S3–S5 QUEUED behind: merge-q344 · Q-359 C0/C1/C2/C4 DONE → /tmp/merge-q359, gate /tmp/g359.out after g367;
  follow-ups Q-379..382 CUTTING (on merge-q359); C3 SCHEDULED
  waits on: relay decode tok/s + 4-delegates-on-2-Macs wall time.
- Q-340/341/344 ON MAIN 838b93b62 (gate green). Q-358 notes S3–S5 DISPATCHED (was queued behind it).
- Queued: Q-334 (focus ring colour outside the local edition), Q-335 (~55 faded opacity uses), Q-336/261/281
  (model behaviour, measured on #3q).

## 3.0.71 integration
- 3.0.71 = main db07adc4b (g071 GREEN: vitest 4218, lib 2058, acp/session/sidecar 410/provider 449/server/mcp+cli,
  clippy, dev gates, schema, wincheck). RELEASE BUILD RUNNING (~/goose-builds/release-3.0.71.log, own session).
  Install BOTH Macs (Q-347 wire tag) at a #3r turn boundary; #3r is compacting now (159,636 tok, 0 cached = the
  3.0.70 baseline). Was: ONE branch /tmp/merge-071 = main + q350/343 + q347 + q367/369 + q359 + q371/372 + q358 search; ONE full gate
  /tmp/g071.out (scratchpad/gate071.sh, own session) after g350 — per-lane gates dropped (each lane gated in its
  worktree). Green → ff main → build 3.0.71 → install both Macs at a #3r turn boundary. Q-364 DONE (branch worktree-agent-a9eba4640e56e14fc,
  2 commits on merge-071) — merge into merge-071 AFTER g071 ends (never mid-gate), then lib+clippy+wincheck; Q-358 notes S3–S5 DONE
  (worktree-agent-a71ddfd61a9eb69b1, 3 commits) joins the same post-gate merge → second full gate g071b; ledger_resolve now base-aware (lost Q-368 twice).

- BATCH 2 → 3.0.72: /tmp/merge-072 = merge-071 + Q-364 + Q-358 notes + Q-357 (sdk conflicts taken one side, then
  REGENERATED by the gate); gate /tmp/g072.out (1 red: Q-391 racing test, cutting) on its OWN target ~/goose-targets/g072 (main target builds 3.0.71).
  Q-379..382 DONE (worktree-agent-ad123d197e6e9ffd8, on merge-q359) — merge into merge-072 AFTER g072, then a
  short gate; Q-383 (load-flaky vitest waits) cutting.
  g072 GREEN except the schema check (fixed 90c092c23: SDK regenerated) and the Q-391 race (fixed). BATCH 2b
  MERGED into merge-072 (6ada7bcd0); its full gate QUEUED behind: a cargo slot — #3s runs on the split and
  Q-394/Q-397 agents hold the MacBook's cargo (≤1 job rule). Q-394(a) DONE (worktree-agent-a54da611728175b1f: compaction keeps the work folder + absolute paths) — joins
  merge-072 after g072b; Q-394(b) (find ~ sat 300 s in the app) investigating; Q-397 DONE (worktree-agent-a55855b8d10a78827: memory_hold code + blocking /goose/admission
  wait, only the rank-0 Mac needs it) — joins merge-072 after g072b with Q-394a. BATCH 3 (→ 3.0.73): Q-399 DONE (worktree-agent-a77c5e29f0ce12c86, + Q-401
  fixed; both Macs need it), Q-400 DONE (branch q400-router-queue-depth: queue-depth leak + end-of-turn reviewers superseded by
  their chat's next turn — #3r piled 19); Q-402 DONE (worktree-agent-a731d604ff79a5e18, carries Q-399) ; Q-403 DONE (worktree-agent-ab91276a8499855c8 on
  q400: /v1/status stops counting a departed client, held/queued departed never start; rank-0 Mac only); Q-409 DONE (worktree-agent-a4d8f43109c869f1e on
  Q-402: shared Rust/TS redaction fixture; bad-json detail DID carry the capability); Q-406 (shell timeout kills
  only bash; TCC-blocked find — the Q-394b root cause) cutting; Q-398 (pipeline fork's 503 has no code) QUEUED behind: agent cap. Was BATCH 2b list: Q-379..382 (worktree-agent-ad123d197e6e9ffd8), Q-388/389
  (worktree-agent-a1d887d0d5490caf6: create-doc path + xlsx read, MCP patch), Q-383, Q-391, Q-392 when done.
  Q-390 (r1 opens the chat in <round>/work) cutting.
- 429 at 16:5x (limit resets 18:40): resumed by SendMessage — Q-383 (a3e9aab…, WIP untracked testClock.ts +
  waitClock.test.ts) and Q-388 (a1d887d…, 3 commits + an uncommitted patch edit, its /tmp/q388/gate.sh still
  running). If either dies again: re-send after 18:40, same ids.

- 3.0.72 = main 104d3d07d (final gate GREEN: vitest 4281, 43 suites, clippy, schema, wincheck). RELEASE
  BUILDING (~/goose-builds/release-3.0.72.log). Then install both Macs → split-start → E2E #3t with NO cargo.
  Was merge-072 49e2269d0: batch 2 + 2b (g072b GREEN but the waitClock guard → fixed 967ef755d) + Q-397 +
  Q-394a (acp_ws harness conflict hand-unioned, shared brace fixed, rustfmt ok). FINAL gate /tmp/g072c.out (RELAUNCHED 19:4x after a Q-397×Q-357 seam: the hold status line lacked
  `compaction: None` — fixed; main + Q-405 merged in; app QUIT to free the idle split's ~37 GB — swap was 80 GB)
  (scratchpad/gate072c.sh, own session). Green → ff main → release 3.0.72 → install both → E2E #3t, NO cargo.
- CI red once on e3b810895: Q-346's lib test raced sources.rs tests swapping GOOSE_PATH_ROOT → Q-405 fixed on main
  274e12820 (env lock); the failed job re-run. merge-072 takes it when main is merged in before the ff.
- Spend/usage limit hit 19:1x (resets 21:50): Q-399, Q-400, Q-394b agents RESUMED by SendMessage on the owner's
  "retry please all"; if they die again, re-send after 21:50.

- 3.0.73 = main f51d4f9da (gate GREEN: vitest 4316, 54 suites, clippy, schema, wincheck); RELEASE BUILDING
  (~/goose-builds/release-3.0.73.log). Memory recovered (the orphaned lz-ppm python is gone; swap 10 GB, disk
  123 GB) → install 3.0.73 on both, split-start, E2E #3t on the SPLIT. Was /tmp/merge-073: Q-399/401 + Q-402 + Q-409 merged (5d149699b); Q-400+403 merged (8805a72e8 — rank_wrapper conflict resolved by its
  author); FULL GATE /tmp/g073.out (scratchpad/gate073.sh, target g073); Q-406 MERGED (13280e94d: own process groups, proof-gated
  group kill, named TCC cause; #3r trace YES); Q-407 (goosed crash leaves commands running) cutting.

- CI: red 6× 17:15–18:10 on three timing-flaky integration tests (Q-397's hold test ×4, needs-you superseded,
  link control Offline) → Q-420 cutting; green runs interleave.
- BATCH 4 (→ 3.0.74): Q-417 DONE (worktree-agent-ad33799988ff81b4d: one servedModel() for single/split/remote;
  also the split's running modelId was null); Q-407 (goosed crash leaves commands) cutting.

## Next actions (in order)
1. Merge Q-340/341+344, Q-342, Q-350/343 via scratch + gate → build 3.0.71 → install at a #3r turn boundary.
2. On 3.0.71: live critic walk (Q-325's visual change, Q-337's bar, needs-you fold/queue), then E2E #3s.
2a. Owner: the E2E must ANSWER needs-you cards → Q-376 ON MAIN (r1.mjs + needsyou.mjs answer via the UI from the brief's
   needsYou guidance; needsyou.tsv; stop rules) — live prove on #3s. On 3.0.71, before #3s: answer #3p's 2 open cards (20260928_19) and
   20260928_17's by hand over CDP as a first live prove.
2b. Before 3.0.71: one FULL gate on merged main (lanes were gated on different bases).
3. Live proves still open: Q-254/272 swap words, Q-257 two windows, Q-276 kill -9 → restore, Q-278 J2, J4.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch (REAL pnpm install when the SDK changes), gate, ff main. Coordinator assigns Q ids; agents use
  their OWN scratch folders. rustfmt --check hand-resolved .rs conflicts. Tests never touch port 8090.
- Never navigate the main window while an E2E runs (split-start/probes moved #3q's page at 13:13).
