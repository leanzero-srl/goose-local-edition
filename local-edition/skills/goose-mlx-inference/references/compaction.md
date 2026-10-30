# goose's compaction on a local model

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

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
