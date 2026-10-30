# goose's compaction on a local model

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Contents
- The defect (Flash Gauntlet 0.3927): WHEN / WHAT WAS LOST / AFTER
- Round 1 FIX (compaction-fix): reserve point, summary gate, verbatim tail, named files; TRAP: where a run's sessions.db lives
- REVIEW FIXES (round 2, compaction-fix-2); TRAP: TokenCounter ignored reasoning; E2E on Qwen3.5-9B; MUTATION PROOF
- ROUND 4 (compaction-fix-4): provider-anchored decision, fact sheet beside every summary, structural gate, summary from reasoning
- ROUND 5 (compaction-fix-5): flagging (size floor, thin sections, future intent), fact sheet first when flagged, "(unverified)" numbers, still-to-do from tool calls
- ROUND 6 (compaction-fix-6): temp-dir paths are no secret, reasoning summaries start at a named heading, look-ups are not open errors; 5b reconciled

## goose's compaction on a local model (2026-10-30, branch compaction-fix)

Owner: "why did goose compact while cloud didn't? ... our compact engine might be fucked." Measured on the Flash-Next
Gauntlet sb-7.2 (ple.6, 360k, 0.3927 vs cloud 0.6846; engine log runs/2026-10-29-flash-vision/attempt-2/mlx-engine/)
and the stopped run (stopped-run-1/userdata/benchmark/runs/build/.openai-cloud-39b73921-...-runtime/goose/state/logs/
llm_request.9.jsonl = the first request after its compaction, summary text inside).
- WHEN: 0.8 x 360,000 = 288,000; the call that crossed it had 282,806 prompt + 6,700 out. The summary request was
  agent call 96 of 151 (289,943 prompt, 282,774 cached, same model, thinking medium, 4,316 tokens out, 299 s).
  Growth before it 1.87k tokens/call (calls 64-106), after it 1.28k: 55 calls more would have needed 360-394k, so the
  window could NOT have finished the task; compaction was due around call 130 anyway. Cloud 1M -> 800k point, peak
  322,851, never compacted. 0.8 is not the defect; it only moved the compaction ~20 calls earlier.
- WHAT WAS LOST (the defect): the model-written part of the stopped run's summary was ONE line, "I will now proceed to
  document the design decisions and run the final verification sequence." None of the asked sections. goose stored it.
  The rest was code-built: the task prompt (twice: kept block + preserved request), file names with +/- line counts,
  8 failed calls. No tail (GOOSE_COMPACT_KEEP_TAIL default 0 for chats), no file contents, no decisions, no TODO.
- AFTER: stopped run, 13 of the next 17 calls re-read (ls, BROWSER-TESTING, STARTER, SB7-CONTRACT in 3 chunks,
  VISUAL-CONTRACT, __main__.py, 4 greps of its own modules) - each doc had been read once before. ple.6 run: median
  output per call 344 tokens after vs 1,144 for the 56 calls before; 3 calls over 2k tokens (writes) vs 22; 35.7k
  output tokens after vs 175.6k. Its session DB was deleted at cleanup, so its summary text is not recoverable.
- FIX (compaction-fix): (1) the point is `context_mgmt::compaction_threshold`: limit minus (output cap + 1/16 window),
  never under 0.8; 360k/32,768 -> 304,732; GOOSE_AUTO_COMPACT_THRESHOLD wins, GOOSE_COMPACTION_RESERVE_TOKENS
  overrides the reserve; swarm workers keep the configured ratio. (2) an answer missing any of the five sections
  (Where we are / What each file does / Decisions and reasons / Still to do / Next step) is asked once more on the
  same cached prefix (SUPERSEDED by round 4 below: no transcript request after it). (3) chats keep a verbatim tail sized to the kept share
  (1/16 window); the summary request still sends the whole conversation (hybrid caches cannot trim). (4) files the
  person's messages name are kept whole from disk in the kept block, largest dropped first to fit.
- TRAP: a run's sessions.db lives in the run's hidden .<run>-r0-runtime/goose/data/sessions; keep it before deleting a
  dev profile, it is the only copy of the summary text.
- REVIEW FIXES (2026-10-30, branch compaction-fix-2; an adversarial review of compaction-fix confirmed these):
  (1) the point read `ModelConfig::max_output_tokens()`, a fictitious 4,096 when max_tokens is unset, which is how
  goose talks to every local engine: Flash 360k compacted at 325,904, not 297,232. Now `output_reserve_tokens`:
  max_tokens if set, else UNSET_OUTPUT_RESERVE_TOKENS 32,768 (the engines' serve default; Flash attempt-2: 154
  answers p50 469, p90 4,316, p99 15,607, max 32,768). The summary request is sent only with
  `summary_answer_room` (16,384 or a quarter of the window) left by goose's count, else the transcript, itself
  measured to fit; the second ask is checked too. A SET reserve (GOOSE_COMPACTION_RESERVE_TOKENS) may go below 0.8,
  floor 0.5; the derived one keeps 0.8. (2)+(3) [round-2/3 gate, REPLACED in round 4 below] the gate was `pillars::written_sections`: each of the 5 sections
  needs >= 5 letters/digits of its own (its heading and the instruction's ask removed), headings inside code fences
  do not count, and `## X:`, `### X`, `**X**`, `**X:** text`, `X:`, `## 1. X` are read. (4)+(5) the tail is
  counted in TOKENS (images by pixels/750 via token_counter::image_tokens) and drops a tool result together with a
  request that does not fit; harness nudges are tail rows, but never the person's request (the request is re-sent
  before the tail). Recovery compactions keep NO tail (the window was just proven wrong). The kept block is
  refitted to its 1/16 in tokens. (7) named files: secret-like names (env, pem, key, tfstate, secrets,
  credentials, id_rsa ... SECRET_NAME_PARTS), negated mentions ("don't read X"), files > 64 KiB are never read.
  (8) `context_mgmt::tool_call_cutoff` reads `effective_context_limit` like the guard. Chat transcript requests (recovery only since round 4): max 5
  requests (was 8); with a measured window impossible steps are skipped without a call.
- TRAP (found by the E2E): goose's TokenCounter ignored REASONING content, which the openai format sends back as
  reasoning_content. Flash stopped run: 224 messages = 239,628 prompt tokens at the engine, 112,754 by goose
  (692k chars of thinking uncounted). Fixed in count_chat_tokens; any budget made before it was ~2x low on a
  thinking model's chat.
- E2E (Qwen3.5-9B-MLX-4bit on the workhorse, goose pin lz.14, port 8297, the stopped run's 224 messages,
  262,144 window): before the reasoning fix the whole 224 messages went (239,628 engine tokens, tail 52 rows,
  685 s) and the summary was 2,350 tokens; after it the conversation cut to 205k goose tokens was 148 messages =
  164,126 engine tokens (goose now counts ~20% HIGH on Qwen, the safe side), tail 12 rows, 155 s, summary 2,542
  tokens out (thinking included) - far inside the 16,384 room. Gate PASSED both times, sections 289-4,005 chars; the Flash one-liner, the parroted template, the summary inside a fence and the summary
  cut after its first section all FAILED the gate. Driver: `context_mgmt::tests::e2e_a_real_model_writes_a_summary_the_gate_reads`
  (ignored; env E2E_SESSION_JSON, E2E_WINDOW, E2E_PROMPT_TOKENS, GOOSE_MODEL=GOOSE_FAST_MODEL, OPENAI_HOST).
  Export a session for it from a run's sessions.db: rows with metadata agentVisible=false are the pre-compaction
  conversation; set agentVisible true and dump [{id, role, created, content, metadata}].
- MUTATION PROOF: every fix has a test that fails when the fix is reverted (incl. the reviewer's two: request
  before tail disabled, guard ignoring the reserve). crates/goose/tests/compaction.rs had 4 tests failing since
  430d85f003 (their scripted answers lacked the 5 sections; the visibility helper predated the kept tail): fixed.
- ROUND 4 (2026-10-30, branch compaction-fix-4; an independent review of round 3 found the word-heuristic gate
  fragile both ways and the guard's count wrong). THE DESIGN NOW, replacing every gate rule above:
  (A) GUARD COUNT = the provider's prompt for the chat's LAST call (`note_sent_prompt`, recorded in the reply loop
  and after a compaction) + goose's count of ONLY the rows written since (`chat_context_tokens`, LastCall); after a
  reload the stored usage + the rows after its answer (StoredUsage); only with neither the whole request incl. the
  system prompt and tools (Estimated). Rows are counted as a SEQUENCE everywhere (`TokenCounter::message_costs`:
  an answer's reasoning that its split tool-call rows repeat counts once; the kept tail adds it back only for the
  row it opens on, `token_counter::run_tokens`). goose's own counts x GOOSE_COUNT_MARGIN 1.065 (o200k ran -3.4%
  under Flash-Next, -5..-6% under Qwen3.5-9B). Measured on the real 223-row transcript (fixture outside the repo:
  workhorse ~/scratch/compaction4/qwen9b-223.json; ignored test `real_transcript_the_guard_is_within_the_engine_s_count`,
  recorder `real_transcript_record_engine_prompts`): round-3 guard 279,633 vs engine 243,299 (+15%); now estimate
  +2.86% (Flash) and -0.6..+1.0% (9B, 93-181 rows); provider path, 15 pairs: -0.5..+1.0%, +3.6..+4.6% only across a
  951 KB image the --text-only engine did not count (safe side). Small prompts (31k) still estimate -6% (no decision there).
  Swarm workers keep the golden count (GuardCount::Worker).
  (B) FACTS BACK EVERY SUMMARY: the kept block (`pillars` + `facts.rs`) always carries the person's messages
  (the first, the task, is never cut; the newest is re-sent right after and the block says so), each written file
  with its size and what it defines (functions/classes/exports/routes, read from disk, else the conversation's last
  write with its edits applied; else its head), the errors still OPEN (a failure the same call later fixed is
  counted as settled), the shell commands that worked (exit 0, not looks), the last 10 tool calls with outcome, the
  summaries earlier compactions stored (carried word for word), and for named files too big to keep whole their
  headings + binding lines (must/never/always/only/required/shall/exactly). THE GATE IS STRUCTURAL ONLY
  (`summary_shortfall`): refuse when sections are missing or 3+ of 5 are empty (a section echoing the instruction's
  ask is empty); "None.", "Keine.", "- none", non-English headings, bold or colon headings all pass; NO vacant-word
  list, NO anchor heuristic. A summary written in the REASONING (Flash-Next thinking on: 1 of 3 real runs left 0
  visible chars) is taken from the reasoning's last complete block, no retry. One retry at most, then the better
  answer is stored FLAGGED (`[goose could not check this summary: ...]`) beside the facts: no transcript prefill
  (6-8 min at 250-300k on local models). The transcript path stays only for a refused/unfittable request
  (recovery, bounded to 5 calls) and a different summary model; its answer is checked the same way, flagged, never
  re-asked. (C) Swarm workers: transcript request with no max_tokens and the golden 5-step ladder
  (`summarize_a_transcript_as_the_swarm`), golden kept tail; only change kept: tail copies dated now (no request
  carries a date; the store's read-back order now equals the in-memory order).
  (E) LIVE: see the round-4 E2E line in the NOTES of runs/2026-10-30-flash-fixes-verify (Qwen3.5-9B, workhorse :8297).
- ROUND 5 (2026-10-30, branch compaction-fix-5; the verifier's GRADES4 on 7d16de1ffb, ~/scratch/compverify/evidence4):
  the structural gate stored two non-summaries UNFLAGGED (5 bold lines of 2-3 words; reasoning-only planning notes),
  all 3 real Flash-Next summaries dropped D4 (framing via the camera; the crashed `fitcheck.py` never ran clean) and
  run 2 invented "context ~60% of 409,600". NO WORD LISTS (round 3 broke both ways). FLAG, NEVER REJECT
  (`context_mgmt/summary_check.rs`, applied in compact_core after the gate, chat path only; swarm untouched):
  (1) THIN: section bodies < 1% of the replaced chars (format_message_for_compacting, no reasoning), clamped
  1,000..6,000 chars and <= replaced/10. Measured: real summaries 10,657/11,425/11,912 chars = 3.1-3.5% of 340,773;
  junk 131-230 chars (0.04-0.07%). 1.5% of tokens (the first idea) sits AT the good ones: rejected.
  (2) more than half the section bodies under one sentence: 0 or 2 words (CJK: 2 chars a word); ONE word is the
  empty-list answer ("None.", "Keine.", "无。") and counts as said. (3) more than half the sentences open with
  "I will" / "Let me" / "Next I" / "Now I" (the only phrase check). Counter-test: "None."/"Keine." sections and
  short German/Chinese summaries are NOT flagged.
  FLAGGED LAYOUT: `[goose could not check this summary: <why>. goose's facts come first and are authoritative…]`,
  then the kept block, then "The model's answer (flagged; goose's facts above win):" + the answer.
  `pillars::model_part` reads both layouts (earlier_summaries still carries the answer). Telemetry: JSON log
  events `fields.event = "compaction_summary_flagged"` (warn, with `reason`) and `"compaction_numbers_unverified"`
  (warn, `count`) — grep the CLI/server log to count them per run.
  (4) NUMBERS: every number of >= 2 digits standing as its own word in the model text that no row showed (text,
  tool-call args, tool output; NOT reasoning; plus the kept block and the system prompt) gets " (unverified)";
  a share "<n>% <one word of <=4 letters> <number>" whose exact phrase (commas/whitespace folded) is in no row gets
  it after the pair. Real case: FIX4-2 "~60% of 409,600 (unverified)" (409,600 alone is in 25 turn-context rows).
  Names (pay_00171, R0, v2, 12k, hashes) are not checked.
  (5) STILL TO DO in the kept block (`## Still to do, by the tool calls (N)`, first in the errors part): each open
  error as "make `<call>` succeed", then TODO/FIXME markers in written files (3 a file), then the newest
  `todo_write` scratchpad's unchecked `- [ ]` items; max 12, trimmed after failed calls when the block must fit.
  D4 test: the stored block lists `python3 fitcheck.py` as open while FIX4-2's own Still-to-do only deletes it.
  Real-transcript check (ignored): GOOSE_COMPACTION5_DIR=~/scratch/compaction5 cargo test -p goose
  real_flash_next_summaries -- --ignored --nocapture (flash-next-223.json exported from verifier pre.db).
- ROUND 6 (2026-10-30, branch compaction-fix-6 from main 1c71262a7d; replaces the unmerged compaction-fix-5b
  a71e327a19, deleted). The verifier's FIX4 defects (runs/2026-10-30-flash-fixes-verify/NOTES.md) checked one by one
  against main (round 5): (a) FIXED here: `facts::file_text_now` ran the secret-NAME check on the WHOLE absolute path
  and "private" is in SECRET_NAME_PARTS, so every file of a workspace under /private/tmp or /private/var/folders
  (every benchmark run: /private/tmp/gfix, /private/tmp/gvision/...) fell back to "as last written in the
  conversation". `pillars::is_secret_like_path(path, working_dir)` now judges only the path BELOW the chat's folder,
  the OS temp dir, /tmp or home (raw + canonical); elsewhere the whole path. Named files (`named_documents`) were
  never affected: they already judge the path relative to the chat's folder. RULE: secret = a NAME (file or folder
  below the chosen folder), never a location. (b) ALREADY HANDLED by round 5: the verifier's five bold planning lines
  pass the structural gate and are stored FLAGGED (3 of 5 sections under one sentence), on a small chat too (test
  five_bold_planning_lines_are_flagged_on_a_small_chat_too). 5b's fix (refuse an answer without the five names)
  was NOT ported: it rejects instead of flagging and would refuse translated headings, which round 5 accepts on
  purpose. (c) FIXED: `summary_in_reasoning` starts a block only at one of the five NAMED headings (reasoning is full
  of "Constraints:"/"Approach:" colon lines; planning notes were stored unflagged, never re-asked: now re-asked once,
  then stored flagged) and `without_trailing_thought` cuts the reasoning after the last section's first paragraph
  ("Wait, I should double-check..."). A visible answer keeps the round-4 reading. (d) FIXED: a failed shell call
  whose every step only looks (LOOKING_COMMANDS, now also test/[/[[/type/readlink/realpath; `sed -i` excluded) is
  counted on one line "(N failed look-ups ... are not errors)", not listed; a command's `cd <dir> &&` prefix is
  dropped from the settle key, so the same command rerun from another folder settles it. Not ported from 5b (not
  in scope, low): the 1.065 margin on the summary request's own sizing and the post-compaction estimate marker.
  PROOF: 6/6 mutations killed (~/scratch/compfix6-mutate.py pattern: revert one fix, run its test). REAL MODEL
  (Qwen3.5-9B-MLX-4bit, rapid-mlx lz.19 on workhorse :8297, e2e_a_real_model_writes test, the Flash-Next 190-row
  transcript, workspace rsynced to its REAL path /private/tmp/gvision/.../r0): see the round-6 numbers below.
  Round-6 numbers (same input, kept block is deterministic): BEFORE (main 1c71262a7d code) 0 of 11 written files
  "on disk now", 11 "as last written in the conversation"; "Errors still open (3)" incl. `find ... ; wc -c
  app/**/*.py` (a glob matching nothing). AFTER: 11 of 11 on disk now; "Errors still open (2)" + "(1 failed
  look-ups ...)": read_image /tmp "Operation not permitted" (real, never settled) and `cd /tmp && curl .../v3/
  reversals | python3 -c ...` (real failure; the later success at row 75 ran a DIFFERENT python snippet, so exact
  key match does not settle it: left open on purpose, no fuzzy matching). Named contract files kept verbatim in
  both (SB7-CONTRACT.md, VISUAL-CONTRACT.md, ... under the /private/tmp workdir). Model text (9B, sampled): before
  passed first ask; after run 1 wrote "# Decisions" for the 4th heading twice -> stored flagged; after run 2
  passed first ask, unflagged (variance, not the change). Evidence ~/scratch/compfix6-evidence was copied to
  /Volumes/AI-workhorse/runs/2026-10-30-flash-fixes-verify/compaction6/ (before/after*-seen.txt).
  TRAP: providers::key_connection only_a_refused_loopback... fails on this workhorse (macOS 27 accept-queue
  behaviour) on main too: not a compaction regression.

