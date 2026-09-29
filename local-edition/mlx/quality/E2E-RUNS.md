# E2E runs — the real-use loop (one row per turn per run; comparable numbers)

| run | build | config | brief | turn | secs | tools offered | agent calls: input / cache_read | helper labels (output tok) | deliverable |
|---|---|---|---|---|---|---|---|---|---|
| E2E #1 | 3.0.40 | split tensor, 141,568 window (a test engine held 30 GB at start) | jira-migration-readiness | 0 | 1220 | 79 | 28–55k; 2 of ~6 cold (49,292 / 0) | 100–219 (thinking) | kickoff.md ✓ all facts |
| E2E #1 | 3.0.40 | ″ | ″ | 1 | 507 | 79 | 55,977 cold mid-turn (Q-73) | 97–145 | 2 memories saved ✓ |
| E2E #1 | 3.0.40 | ″ | ″ | 2 | 3207 | 79 | 5 of 9 cold (62,154 / 0); a label ran to 3,383 thinking tokens | up to 3,383 | EOL dates + JCMA answer from atlassian.com ✓ |
| E2E #2 | 3.0.41 | split tensor, 262,144, GOOSE_TOOL_DEFERRAL | ″ | 0 | 551 (−55%) | 27 | 30–35k; first cold, then 29,295 / 33,319 / 33,890 / 34,489 reused (≈98%) | 5–7 (no thinking) | kickoff.md ✓ all facts |
| E2E #2b | 3.0.41 | Studio single over Link, 262,144, deferral | ″ | 0 | 300 | 27 (+2 load_tools) | 3 of 32 agent calls cold across turns 0–2 | — | kickoff.md ✓ |
| E2E #2b | 3.0.41 | ″ | ″ | 1 | 43 | ″ | ″ | — | 2 memories saved — to ~/.goose/memory/working-agents.txt (category "working-agents") |
| E2E #2b | 3.0.41 | ″ | ″ | 2 | 573 (split #1: 3207) | ″ | ″ | — | 4 web searches + 28 shell |
| E2E #3 | 3.0.45 | split tensor, 262,144, deferral | jira-migration-readiness | 0 | 1021 — split DIED mid-answer | 32 | first call 30,735 cold, 7k+ thinking tokens at 11.5 tok/s (E2E #2's same call: 3,367) | — | none: at 10:07:37 both ranks stalled (rank 0 R spinning in a collective, rank 1 S 0% CPU), the hang rule killed both at 10:07:57 (Q-114); TB/GPU/memory/build refuted |
| E2E #4 | 3.0.46 | Studio single over Link, 262,144, deferral | ″ | 0 | 170 (#2b: 300) | 27 | 31,552 / 32,428 cached | — | kickoff ✓ |
| E2E #4 | 3.0.46 | ″ | ″ | 1 | 81 (#2b: 43) | ″ | ″ | — | 2 memories saved (global ISO+British rule; project zero-npm rule) |
| E2E #4 | 3.0.46 | ″ | ″ | 2 | 1333 (#2b: 573) | ″ | 37,649 / 38,173 → 63,552 / 64,021 | — | 77 tool calls, no answer: web search returned empty results `serper-failed-no-fallback` (no key, browser engines off — Q-117); the model pip-installed a package and ran a 300 s find. Run stopped here for the 3.0.47 install (Q-115). Studio cache bounded 20.1/22.4 GB, footprint 45–66 GB |
| E2E #3c | 3.0.51 | split tensor 27B, 262,144, deferral | jira-migration-readiness | 0 | 2122 (stopped call: 21,741 tokens, 0 streamed — Q-141) | 27 | 30,869 / 32,247 | reasoning off | Q-114 proven (no stall past 2× the old hang); Q-135 proven (no thinking) |
| E2E #3c | 3.0.51 | ″ | ″ | 1–5 | 110 / 690 / 1380 / 516 / 924 | 29 | every call after turn 2 read 31,385 cached of 58–66k — the turn-context move (Q-142) + a load_tools cold prefill (Q-107) | — | run stopped after turn 5: build superseded by Q-107/Q-141/Q-142; memory created: client-deliverable-conventions.txt (deleted) |

## Tool-call failure census (2026-09-25, sessions.db)
| run | engine | calls | failed | causes |
|---|---|---|---|---|
| E2E #1 | split (mlx_lm) | 23 | 3 | 2 × web page 404 (model-guessed URLs), 1 shell |
| E2E #2 | split (mlx_lm) | 11 | 2 | 2 × edit old_str mismatch (model; goose offered "Did you mean") |
| E2E #2b | Studio single (Rapid-MLX, qwen3_coder_xml parser) | 71 | 36 | ~22 `</parameter>\n!` appended to shell args + 3 × write "missing field `path`" (Q-85, engine parser) · cascades (files never written) · model: `cat -A` on macOS, analyze on .md |
| E2E #3d | 3.0.52 | split tensor 27B, 262,144, deferral — GREEDY (Q-159) | jira-migration-readiness | 0 | ~2,700 (one answer of 57 tool calls: 54 identical ledger_append, ~24k tokens, 40 min) | — | next call read 39,591 of 52,805 cached (75%) | — | halted after turn 0: Q-159 greedy sampling confounds every split E2E; Q-146 reframed; live critic round 1 walked it (Q-147..Q-158); chat ledger (54 dupes) archived + deleted |
| E2E #3e | 3.0.55 | split tensor 27B, 262,144, deferral, sampling = generation_config (Q-159) | jira-migration-readiness | 0 | 24,694 (ONE answer: 221,604 tokens, 649 tool calls = 324 × identical write+mkdir; Q-159 guard skipped the copies) → compaction call 259,408 in → hang rule killed the split mid-cache-search (Q-162) | — | — | sampled | turns 1–29: r1 fired 29 × 5 s notices into the stopped split (harness fixed: a notice ends the round). Q-161 + Q-162 filed, agents out. The heartbeat did not fire 00:35→07:50 (cause unknown) so the loop saw none of it for 7 h |
| E2E #3f | 3.0.57 | split tensor 27B, sampled, Q-161 guard armed | jira-migration-readiness | 0 | stopped at ~6 min: a write call (1,692 chars) then 3,251 chars of `!
</parameter>
</function>` looping outside the call (Q-161 reopened) | — | — | — | livecheck GREEN all turn (running session marked, Q-147 live); the Q-151 line "Writing for 5m 5s · 1.9k tokens · 10.9 tok/s — a tool call to write" + the forming disclosure showed the loop in words (Q-151 live); runwatch missed it (size rules only) → words rule added |
| E2E #5b | 3.0.58 | split PIPELINE Flash, sampled | python-stdlib-backport | 0 | stopped at ~29 min mid-turn: 17 agent calls; 97% cached until a 6,885-token VENDORED.md write (a legit 12 KB file), then 0 cached per 60k call (~4 min each, Q-179); the forming panel showed 1 char of a 4.6k-token write (Q-178) | 17 | 40,058→46,926 then 0 | sampled | livecheck green (after the harness title fix); runwatch size alarm = a legit write (reworded as an alarm) |

## #3r — 2026-09-28 13:24–17:2x · 3.0.70 · split tensor · jira brief
- 18 turns (0–17 done). Cache 97–99% every turn until compaction. Turn 2: invented websearch/fetch ×12 (Q-367 fixed). Turn 3: a sampled slip dropped `=>` (Q-371 = model; Q-372 parser fix; Q-385). Turn 15: create-doc filed the report under ~/docs (Q-388). Compaction (turn 18): 13:44→14:10, 159,636 prompt 0 cached, 5,740 written = the 3.0.70 baseline for Q-342/347. Post-compaction request 47,210 cold (162 s), reply lost on a CLOSED stream during a memory hold caused by my builds → hang (Q-392). r1 killed 17:2x.

## #3s — 2026-09-28 15:05–15:47 UTC · 3.0.71 · split tensor · jira brief (STOPPED at turn 3)
- Q-390 LIVE: chat opened in <round>/work via Projects → New session here, verified in sessions.db. Q-350 LIVE (restore reads distributed/mounting).
- Turn 2 (web): the real leanzerowebsearch tools from the first call, 0 invented names (#3r: 12). Cache warm from call 2 (40,617/41,677).
- Turn 2 ENDED in "Ran into this error: 503 … not admitting new requests" (Q-397, the memory hold) and turn 3 sat 15 min under the hold: swap 40.3/40 GB used — other sessions' test runs + my two agents' cargo. Stopped: a run under swap measures the machine, not goose. Next E2E only with no cargo on the MacBook.
- Not reached: needs-you answering (no card raised), compaction (78k/250k).

### E2E #3u — 3.0.73, split tensor (262,144), jira-migration-readiness, session 20260928_47 — COMPLETE, 30/30 turns (23:08 → 02:59)
- All 30 turns done; no needs-you card was raised (so Q-376 was not exercised); no hang, no 503.
- Found this run:
  - goose: Q-447 (split decode 4 vs 11 tok/s: float32 KV promotion; fixed in 3.0.74), Q-450 (claim check missed
    an unrun re-run), Q-454 (create-doc wrote tables as prose), Q-455 (the compaction countdown read as a work
    budget).
  - model: Q-448, Q-451, Q-453 (parked to forge-tuner).
- Compaction at 02:26 (209,739/262,144):
  - Q-342 PROVEN (the summary read 209,120 from cache);
  - Q-347 PROVEN (the next request read the 41,780-token head from cache).
  - Turn 28's recall after compaction was correct: 24-month cutoff, lead rule incl. svc-edi, FRT a separate wave.
- Turn 2 web: 0 invented tool names (#3r: 13 of 30).
- Deliverables: notes, gen-users/identity-plan/plan-projects/audit scripts + 37 tests + refresh.sh, README
  (followed from clean), readiness docx + PDF (rebuilt with real tables), email, status note.
- Model slips: 3,728 vs 4,328 (it caught this itself); "no command" (Q-453); "send first thing Monday", which
  is after Friday's call.
- Long turns: 20 (1,625 s, 24 tools) and 26 (1,853 s, 37 tools), docx surgery by regex.

### E2E #3v — 3.0.74, split tensor (262,144), python-stdlib-backport, session 20260929_5 — STOPPED after turn 8 (00:38 → 05:44) to install 3.0.76
- 9 turns: 0 (1,110 s, vendoring + sha256 manifest), 1 (memories saved and split correctly), 2 (parser map), 3–4
  (test.support shim on 3.9; the recursion tests rewritten honestly), 5–7, 8 (3,584 s, 87 tools: positions API).
- goose proven:
  - Q-367 LIVE (an invented `read_file` → closest tools → the right tool in one step);
  - the repeat guard broke a model loop in turn 8 (the same narration + call ×3 → "Not run: …" → a different
    step);
  - decode 9.3–11.5 tok/s at 42–62k under load avg ~140 (Q-447 holding).
- goose defect found: Q-469 (recall read "Quick one, no need…" as a correction and told the model to save its own
  line as the person's rule; the model refused). Fixed in 3.0.76.
- Model: turn 7 declared a design without writing it (caught itself in turn 8); several assert-then-retract
  loops on its own wrapper (Q-451 class).

### E2E #3w — 3.0.76, split tensor (262,144), cafe-allergen-menu-site, session 20260929_12 — COMPLETE, 37/37 brief turns (02:45 → 08:38 UTC, 350 min)
- 46 turn rows (brief turns + needs-you answer turns), 1,125 tool calls, no hang, no 503. Peak context 79k/262k,
  so compaction was not reached.
- Split decode held for the whole run: 482 calls ≥50 tokens, median 10.7 tok/s, p10 9.0, min 7.8 (Q-447 stays
  fixed; before 3.0.74 this was 4).
- Needs-you (Q-376 PROVEN LIVE): 9 cards across turns 4/7/9/10/30. Every answer was delivered, the card cleared
  and the reply used the answer. 4 of the 9 answers were WRONG, and that was the harness: the "mince" guidance
  answered a sulphite question with the price. Fixed after the run (needsyou.mjs, most specific match). goose
  handled it well: "That answers the price, not the sulphites" → a safe sulphites tag, and it stopped
  re-asking after the fourth.
- Chat search + note (Q-358): search_chats found the Harbourline PDF recipe; turn 20 drafted a note to
  "Harbourline Jira Migration Assessment". The run never clicked it (r1 did not act on notes then). It was
  delivered after the run with deliver-pending.mjs; the evidence is in notes.tsv / deliver-pending.json of this
  round.
- Found: Q-481 (edit calls without `path`, model; parked) → Q-482 mitigation; Q-483..486 (sidebar: Active now
  crammed, raw-path subtitle, two "work" projects, need-you counts disagree); Q-487 ("Noticed your answer"
  after a plain instruction: `open_question` matched a relative "which").
- Long turns: 0 (1,691 s), 3 (1,652 s), 22 (3,444 s, 122 tools), 23 (1,802 s).

### E2E #3x — 3.0.78, split tensor (262,144), coffee-checkout-double-charge-postmortem, session 20260929_15 — STOPPED at turn 4 of 41 (12:02 → 15:48)
- Why stopped: the run had stopped measuring goose-in-one-chat. The owner used a second chat (20260928_19) from 12:05,
  and the two chats evicted each other's split prompt cache (Q-498): every switch re-read #3x's ~200k prompt cold,
  ~11-20 min per call. Turn 1 (the memory turn) never reached the chat (Q-492), so the memory checks were void.
  3.0.79 (Q-487..494, Q-496) was built and waiting.
- Turn 0: 478 s, 8 tools; the notes and git repo were set up. Turn 1: LOST — an @ in the last word opened the file
  picker, which took the Enter (Q-492); r1 counted it "done" in 5 s (harness fixed: sentLanded + SEND_LOST).
- Turn 2: 8,349 s, 152 tools, 163k. The fake-log generator (80,712 nginx lines, app.log, orders, charges) was
  byte-identical over two runs and passed 36/36 of its own planted facts, after a ~100-min grind on timezone/RNG
  determinism. The model reasoned forward throughout ("Let me stop and rethink this from scratch") and never looped
  verbatim. The goose check flagged its row counts as "years" (Q-494); right after, it re-read its file and found 3
  real bugs.
- Turn 3: 398 s. Turn 4: ~2 h, mostly cold prefills under Q-498; stopped from the main window at 15:48.
- Q-481 still shows: edit calls without `path` (the Q-482 message "Call edit again with path first" is working).
- Found: Q-492, Q-493, Q-494, Q-496, Q-498, Q-500, Q-501. Decode held 10.5–12 tok/s.
