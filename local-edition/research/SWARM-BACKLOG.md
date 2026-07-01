# Swarm backlog — mistakes captured DURING runs, fixed AFTER (rinse-repeat)

The hands-on cycle (user, 2026-06-30): I CONJURE a unique complex prompt -> launch goose (ASK on) -> ANSWER
its clarifying questions (write .swarm/clarify-answers.json) -> MONITOR its progress + reasoning LIVE, catch
mistakes -> log them HERE -> on finish, deep-study + WORK THIS BACKLOG (fix the swarm) -> repeat with a new
unique app. Each app is diverse/unique (my own reasoning); the archetypes are just templates.

## The ASK handshake (how I answer goose's questions)
Launch with: `GOOSE_SWARM_ASK_FLOOR=75 GOOSE_SWARM_ASK_FILE=1 GOOSE_SWARM_ASK_MAXQ=4` (+ the usual
GOOSE_SWARM_SMOKE/SPLIT/DONE_GATE/CONTRACTS/REVIEW). Floor 75 is bumped to ~80 for the 27B (asks readily on a
complex spec). During PLANNING (~2-8 min in) if plan-confidence < floor it writes
`<run-cwd>/.swarm/clarify-questions.json` (a JSON with "questions":[...]), emits `low_confidence_ask`, and
BLOCK-polls (every 5s, up to 1800s = 30 min) for `<run-cwd>/.swarm/clarify-answers.json`. I ANSWER by writing
a JSON array of answer strings (one per question, same order) to clarify-answers.json — AS the user, concrete
+ decisive. Then it re-plans with my answers + emits `low_confidence_answered`. POLL for the questions ~3-4
min after launch (and again before the 30-min window closes). Capture the QUESTIONS it asks here too — what
the 27B is unsure about on a complex spec is itself a signal.

## Monitoring cadence per run (catch mistakes live)
- ~240s after launch: check for clarify-questions.json -> ANSWER if present.
- every ~10 min: read the latest .swarm/run-*.jsonl (task_dispatched/completed/retry, judge_verdict
  re_dispatch vs observed, replanned, pre_review) + spot-read a worker session trace (the session_id -> the
  session DB) for REASONING errors (wrong sibling contract, a dropped feature, a wrong constant, looping).
  Log anything wrong below with [app][phase] what-went-wrong + the likely swarm-side cause.
- on finish: deep-study (7 points + LOC + golden run) + triage this backlog into fixes.

## BACKLOG (open) — fix after the current app finishes
(none yet — first hands-on run pending)

## DONE (fixed)
(none yet)

### [APP11][core-logic] worker produced UNBOUNDED loops (while len<count / while True, no max-iter cap)
A degenerate input loops forever; pytest hung 2min -> tests-core + integrate-verify timed out -> run FAILED.
The 27B could not fix it. SWARM-SIDE FIX IDEA (MED): the worker prompt for any search/generation loop should
mandate a bounded loop (a max-iterations guard or an explicit termination proof), and/or the DONE_GATE could
flag a `while True:` / `while <grows>:` with no break/bound in an owned file as a likely-hang smell and ask
for a cap. Confidence MED (a heuristic loop-detector has false positives; the prompt nudge is safer). Verify
by re-running a recurrence/search app and checking no unbounded loop ships. NOTE: the worker-timeout already
prevents a hung test from hanging the whole RUN (good) — this is about the produced APP not hanging.

### [UNIQ1][clarify] ASK mechanism worked WELL on a complex spec — asked the RIGHT 4 questions (POSITIVE)
plan_confidence 60 (single valid skeleton of 2) -> below the ~80 floor -> it asked 4 genuinely-ambiguous
questions and BLOCKED for answers (the file handshake worked): (1) head-to-head tiebreak definition;
(2) output format (ASCII table vs JSON); (3) schedule idempotency when fixtures already exist; (4) bracket
round numbering direction. These are exactly the under-specified parts of the spec -> the clarify-question
generator is GOOD. Answered as the user (head-to-head = direct-matches mini-league then alphabetical; ASCII
tables for standings/bracket + plain lines for fixtures/form; schedule errors if fixtures exist; rounds 1..k
increasing to the final). POSITIVE finding: the ASK feature is working as intended on a complex app.
OBSERVATIONS to keep (not yet bugs): 1-of-2 skeleton drafts invalid on this complex spec (inert-60, no
cross-check) + ~10min planning before the gate. If repeated across the next complex apps -> backlog a
skeleton-drafter robustness fix; for now just an observation.

### [UNIQ1][plan] CONFIRMED + ROOT-CAUSED: the ASK re-plan WASTEFULLY re-drafts the full skeleton (MED-HIGH fix)
At 26min the league run is STILL in planning (research -> scout -> skeleton round 1 -> conf 60 -> ASK ->
my answers -> skeleton round 2 -> detailing 8 subtasks). The ASK re-plan re-entered the FULL plan() and
re-drafted the skeleton (a 2nd best_of_n round). But my answers were about SEMANTICS — head-to-head
tiebreak definition, output format (ASCII vs JSON), schedule idempotency, bracket round numbering — which
affect the DETAILING (the per-subtask specs), NOT the skeleton STRUCTURE (the ~8 modules: store, schedule,
standings, bracket, cli, etc. are identical regardless of those answers). So re-drafting the skeleton on the
ASK answers is WASTED work (~5-8 min on the 27B). FIX (MED-HIGH, in scope): on the ASK re-plan, REUSE the
already-picked skeleton and ONLY RE-DETAIL, folding the Q&A answers into the detailing prompts (they clarify
the spec, not the decomposition). Locate the re-plan path (after ask_clarifying_questions returns the Q&A,
the code re-enters the plan loop) and short-circuit the skeleton-draft step when a skeleton already exists +
answers were just provided. VERIFY: a complex app with ASK answers reaches EXECUTE ~5-8min sooner.
NUANCE/risk: if an answer DID change the structure (rare — e.g. "actually make it a web app"), re-detail-only
would miss it; gate the reuse on answers being clarifications (the common case) and keep a re-draft path for a
structural pivot. Confidence MED-HIGH that re-detail-only is correct for clarification answers.

### [UNIQ1][execute] INTEGRATION-AT-SCALE CONTRACT DRIFT on the `fixtures` table — CAUGHT LIVE (MED fix)
The integration-at-scale failure mode I was watching for, MANIFESTED on a ~1000-LOC app. Three modules
disagree on the fixtures table schema:
- schema.py (core-db) CREATEs fixtures(id, league_id, home_team, away_team, round_num) UNIQUE(league_id,home_team,away_team).
- fixtures.py (list cmd) SELECTs f.round_num / f.home_team / f.away_team  <- matches schema.py. GOOD.
- scheduler.py (schedule-engine) CREATE TABLE IF NOT EXISTS fixtures(... round INTEGER ...) + INSERT INTO
  fixtures (league, round, home, away)  <- DIFFERENT column names (league vs league_id, round vs round_num,
  home/away vs home_team/away_team) AND stores the league NAME not the id. DIVERGED.
RUNTIME EFFECT: league-create runs schema.py init_schema -> fixtures has league_id/home_team/away_team/round_num.
Then `schedule` runs scheduler.py -> its CREATE IF NOT EXISTS is a no-op (table exists) -> INSERT INTO
fixtures(league,round,home,away) FAILS: "table fixtures has no column named league". So the schedule command
is BROKEN unless integrate-verify catches+fixes it. This is exactly the AB-CONTROLLED draw class (cross-module
CONTRACT DRIFT hidden by isolation-only unit tests): each module unit-tests fine in isolation; the END-TO-END
integration breaks. The CONTRACTS (frozen-interface) feature did NOT prevent it — it froze module FUNCTION
signatures but NOT the shared DB SCHEMA / column names.
SWARM-SIDE FIX (MED, in scope): include the SHARED DB SCHEMA (the exact CREATE TABLE column names from the
core-db/schema module) in the CONTRACTS frozen-interface bundle, so every DB-touching module is told the EXACT
columns and cannot invent its own (league vs league_id). Alternatively/additionally the schema module should be
the SINGLE source of truth and other modules must import it, never re-CREATE the table. WATCH: does
integrate-verify (pending — runs after standings-form-cmds) actually RUN `schedule` end-to-end and catch+fix
this? If yes -> the integrate-verify + DB-schema-in-contracts combo handles it. If it ships broken -> the
swarm did NOT prevent a real cross-module integration failure at scale = the headline finding for the user.

## DONE (fixed)
- [1-PER-NODE VIOLATION] (user caught live: gabee +1 QUEUED while workhorse idle). TWO root causes, both fixed:
  1. NO judge re-judge cooldown -> an OK long worker re-judged every 15s tick (146 calls/42min, 88 observed)
     = wasted model calls that queue on a busy node. FIX: 60s JUDGE_REJUDGE_COOLDOWN_SECS, under-cap only
     (cap-exhausted terminal-fail stays prompt). commit 2510556f0. ~4x fewer judge calls.
  2. idle-jobs (judge/pre-review) picked an idle device by model_id but never CLAIMED it (no in_flight bump)
     -> a worker dispatch / the next idle-job stacked a 2nd call on the same node while another idled. FIX:
     idle-jobs bump devices[i].in_flight + the IdleSlotGuard releases it; pre-review/speculative gates ->
     idle_capacity()==0 (in_flight now tracks idle-jobs). commit 01c9c580c. 1-per-node now holds across
     workers + judge + pre-review. 34 swarm tests green 5x.
  MONITORING LESSON: I was watching the swarm JSONL (scheduler view), NOT the LM Studio per-node queue — they
  diverged. Going forward, when checking idle/queue, READ lms ps per-node, not just the jsonl dispatch counts.

### [UNIQ1][cli-entry] BUILT-BUT-UNWIRED ENTRY at scale — cli.py registers ZERO commands (HIGH-value, MED fix)
925 LOC / 13 modules but cli.py defines an empty click group and never add_command()s the command modules ->
the whole app is unusable. integrate-verify could not fix it. This is THE integration wall at ~1000 LOC.
SWARM-SIDE FIXES (pick by confidence): (a) the SMOKE GATE should assert the entry exposes the spec-advertised
COMMANDS, not just that --help runs without crashing (run `python -m pkg --help`, parse the Commands section,
and if the spec advertises N subcommands but the CLI lists ~0, that is a finding) — deterministic, MED-HIGH.
(b) the architect should make ONE explicit cli-entry/wiring subtask whose contract is "import + register EVERY
command module into the group" with the full command list frozen, and integrate-verify must run EACH advertised
command (catch the unwired ones). (c) combined with the DB-schema-in-CONTRACTS fix (the fixtures drift), the
two together harden integration-at-scale. Confidence MED-HIGH on (a) — it is deterministic + grounded.

### [VERIFIED] 1-per-node fix CONFIRMED working on UNIQ2 execute (lms ps, not just jsonl)
UNIQ2 graph tool (on the binary WITH the cooldown + device-claiming fix). At 23min in EXECUTE, lms ps shows
ALL 3 nodes GENERATING, exactly 1 task each (loader->gabee, node-queries-and-centrality->mihai,
cli-entry-point->workhorse). NO node +QUEUED, NO node IDLE. So the execute-phase 1-per-node fix HOLDS (the
gabee +1-QUEUED-while-workhorse-idle bug is gone). Monitoring discipline: ALWAYS check lms ps PER-NODE each
cycle (jsonl alone hid the original bug). Remaining avoidable-idle fix (best_of_n=fleet) verifies on UNIQ3.

### [DONE] unwired-cli at scale -> explicit ENTRY-WIRING instruction (52715d760, MED-HIGH, grounded)
UNIQ1 cli.py registered ZERO commands (unusable). UNIQ2 PROVED the 27B wires correctly when told (my ASK
answer "every command a registered subcommand" -> all 8 subcommands wired). Fix: the integrator worker prompt
now explicitly states ENTRY WIRING is the #1 multi-module integration failure (register every advertised
command, --help must list each). Prompt-only. VERIFY on UNIQ3+ (a NON-asking complex app should now wire its
CLI). Pairs with the still-open DB-schema-in-CONTRACTS (the other integration failure: the fixtures drift).

### [VERIFIED] planning-idle fix (best_of_n=fleet) CONFIRMED on UNIQ3 (lms ps)
UNIQ3 PLAN banner: "drafting 3 skeleton candidate(s) IN PARALLEL" (was 2). lms ps during the draft: ALL 3
nodes GENERATING (gabee/mihai/workhorse). So no node idles during skeleton drafting now. Both idle issues
the user caught are fixed + VERIFIED live: execute-stacking (UNIQ2: 1-per-node) + planning-idle (UNIQ3:
3-node draft). Remaining idle is only the truly-serial lone-27B skeleton pre-step + the integrate-verify tail.

### [UNIQ3][planning-idle INVESTIGATED] the 2-idle is LEGITIMATE serial sub-steps, NOT a fan-out bug
The user watches idle; I dug in (read parallel_plan ~2950-3188). After the 3-draft skeleton pick (which DOES
use 3 nodes — verified), the serial sub-steps that briefly idle 2 nodes are: (1) verbalized_confidence — ONE
planner self-rating call (line 3155), needed for the ASK confidence blend (0.7 agreement + 0.3 verbalized) +
the uncertainties; CANNOT be parallelized (one LLM call) and CANNOT be skipped when confident (0.7*max-agree
= 70 < the 80 floor, so the verbalized could always drop the blend below floor -> the ASK correctness needs
it); bounded by planner_timeout_secs.max(90) so a stall fails fast. (2) the DETAILING TAIL — the last subtask
spec (integrate-verify) being written on 1 node while the others are done (the tail confirms detailing DID
fan out: "detail stages-module" + "detail integrate-verify" on different nodes). So NO fan-out bug; these are
irreducible serial moments. HONEST: planning on a complex spec is ~20min (research + 3-draft + verbalized +
detailing) — the only avoidable planning idle (best_of_n=2->fleet) is already fixed. The verbalized was slow
this run (part of the 20min); it is one bounded call, not a bug. Verified, not hand-waved.

### [DONE] DB-schema-in-CONTRACTS (e3d8ef7b4, MED) — the contract stub generator now freezes table+column names
UNIQ1 fixtures-table drift (scheduler.py league/round/home/away vs schema.py league_id/home_team/away_team/
round_num) -> schedule broke at runtime. Fix: generate_contracts system prompt now appends a # SCHEMA block
(each table + exact columns) for any DB-owning module, so all DB modules use the SAME columns. Prompt-only,
MED (LLM-dependent). VERIFY on the next complex DB-app (a SQLite app — does the schema stay consistent?).
Pairs with entry-wiring -> the two UNIQ1 integration failures both have a fix now.

## MONITORING RUBRIC (user, every cycle + ALWAYS an improvement set after each app)
Each monitoring check reviews FOUR dimensions, and on EVERY app finish produce an improvement-items set here:
1. IDLE MODELS — lms ps PER-NODE. Each idle node: is it filled by the idle-node fix (pre-review/judge fired?
   grep pre_review/judge_verdict counts) or legitimately serial (lone-27B brief, verbalized-confidence,
   dependency-chokepoint tail, single integrate-verify)? If avoidably idle -> diagnose phase + FIX.
2. REASONING QUALITY — read a worker session trace / the produced modules: wrong sibling contract, dropped
   feature, wrong constant, looping/over-reading, unbounded loop. Judge re_dispatch vs observed ratio.
3. ARTIFACT QUALITY — read the produced code: clean structure + typing + docstrings? smells (while True,
   eval(, bare except:, stubs, fake impls)? does it match the spec? run it (golden) on completion.
4. IMPROVEMENT ITEMS — ALWAYS append concrete swarm-side improvements found (or "none — clean") below.

### [UNIQ3][monitor] mid-execute review — CLEAN (no new items)
IDLE: legitimate — 4 pre_reviews fired (idle-node fix working) + dependency-chokepoint tail (cli/tests/iv
wait on stages). REASONING: sound. ARTIFACTS: HIGH — stages.py frozen dataclasses per stage, Literal/Union
typing, docstrings; NO while-True/eval/bare-except across modules. Judge ~33/35min ~= 1/min (cooldown holds).
IMPROVEMENT ITEMS: none from the artifacts (clean). Full golden-run review on completion.

### [DONE-A] atomic writes: GOOSE_SWARM_SKELETON_FIRST (3fe9967b2) — skeleton-first for entry files
Research workflow (6 agents, ATOMIC-WRITES-DESIGN.md) found the over-read is MANDATED: swarm.rs:5247 hard-rules
one-big-write + plan-whole-file-first, which CONTRADICTS the judge over_read hint (judge.rs:288). Direction A
(env-gated): entry-file worker writes a compiling skeleton (imports + every command registered, placeholder
bodies) FIRST, confirms it imports, THEN fills bodies — one early write provably disarms the over_read kill
(gated on !any_owned_written). Mechanism HIGH, weak-model compliance MEDIUM. SKIPPED the completion-guard
placeholder-scan (heuristic, false-positive risk on legit pass) — backstopped by integrate-verify + smoke.
A/B NEXT: launch an app with GOOSE_SWARM_SKELETON_FIRST=1, compare the entry-file write pattern (early skeleton
write? fewer over_read flags? faster-to-first-write?) vs default.
### [B] detailer build-order checklist (count capped) — SECOND, depends on A softening 5247. Do the cheap
variant ONLY; do NOT lower the 2x-3x multiplier (coarsens -> MORE over-reading). [C] multi-model single-file
slicing — PROBABLY NEVER (lowest conf, merge reintroduces drift, net-negative on 3 nodes).

### [UNIQ4][clarify] 4 questions answered (transfer semantics + account-type + overdraft)
UNIQ4 (SQLite budget, SKELETON_FIRST=1) asked at 14min (conf<floor): transfer amount sign? transfer-pair
delete cascade? account-type behavior? insufficient-funds error? Answered: transfer --amount positive
auto-direction (debit from / credit to); deleting either transfer row deletes the linked partner (atomic);
type is display-only label; overdrafts ALLOWED (tracker records reality), transfer error = structural
insufficient-detail only. Good questions — the model correctly surfaced real ambiguities. NO wiring/schema/
skeleton answers given (the fixes handle those — UNIQ4 tests them). All 3 nodes idle during the ASK = the
block-poll handshake (legitimate, blocked on human input).

### [UNIQ4][improvement + smoke-gate-PAYS-OFF] flat layout (root __main__.py) is not `-m` runnable
UNIQ4 built a FLAT layout (cli.py + commands_*.py + root __main__.py, no package), so `python3 -m <pkg>` has
no package to run -> smoke gate FAILED "no -m entry point, app may be unrunnable" + dispatched a smoke-fix
(27 tool calls, progressing). EVIDENCE the SMOKE GATE PAYS OFF: it caught a genuinely-unrunnable-via-`-m`
app the run would have shipped. IMPROVEMENT ITEM (prevention, MED): the architect/layout guidance should
prefer a `-m`-runnable PACKAGE (pkg/__main__.py) OR, if flat, the entry-wiring + smoke expectations must
match `python3 cli.py` not `-m`. The smoke-fix repairs it (caught+fixed) but at a time cost. For the STUDY:
run via `python3 __main__.py`/`python3 cli.py` (flat) unless the smoke-fix restructured into a package.

### [integrate-verify false-negative — BLOCKED on visibility, instrument first]
Tried to diagnose why integrate-verify fails working apps (UNIQ2/UNIQ3) vs a buggy one (UNIQ4). The jsonl
does NOT surface WHY: UNIQ3 shows ok:None + attempts:3 (exhausted retries) with EMPTY output + no session_id
on the task events. So I cannot tell a true fail (UNIQ4 budget bug) from a false one (UNIQ3 fully working)
without the verdict reason. FIRST STEP (visibility, like phase-timing): instrument integrate-verify to EMIT
its PASS/FAIL + the specific failing check (which command, expected vs actual) into the run_finished JSON +
report. THEN the false-negative is diagnosable + fixable (currently ok:None likely = the worker PASS/FAIL is
not parsed -> defaults to failed even when the app runs). Deferred until after the skeleton-first A/B; do NOT
start an intricate parse change at marathon depth without the visibility first.

### [integrate-verify visibility — DIAGNOSED, ready to implement] the failure reason is never stored
Confirmed: a failed task (incl. integrate-verify) records NO reason anywhere — RunReport.failed (scheduler.rs:44)
is just Vec<TaskId>; the real reason is the DispatchError at swarm.rs:5450-5477 (Transient stall / Terminal),
eprintln'd to stderr only, never persisted. integrate-verify owns no files so it is NOT the DONE_GATE/completion
guard; its attempts:3 + ok:None = it EXHAUSTED retries, almost certainly a Transient STALL (heavy end-to-end
verify task that the slow 27B cannot finish within the idle-timeout). So the app WORKS but the verify STEP timed
out -> run reports FAILED = the false-negative. FIX (additive, low-risk, scheduler): add fail_reasons:
HashMap<TaskId,String> to RunReport (scheduler.rs:42), capture the last DispatchError msg when a task -> Failed
(~589/622/638), surface it in run_finished JSON + the text report (like phase-timing). THEN the false-negative is
diagnosable per run. LIKELY real fix after that: a stalled/timed-out integrate-verify should NOT fail the whole
run if the smoke gate + the produced app pass (distinguish verify-incomplete from app-broken). Implement with a
scheduler_mock test next focused cycle — do NOT rush a scheduler edit at marathon depth.

### [FIXED] integrate-verify false-negative (judge over-read kill) — 6e1547b2d
attempt_history (already in the report.tasks[], I was reading the wrong place) proved it: UNIQ3
integrate-verify = judge_killed x3 verdict over_reading. ROOT CAUSE: the behavioral over-read gate fires on
!any_owned_written; integrate-verify owns NO files -> permanently armed -> guaranteed kill after ~16 tool calls
(it legitimately reads the whole program + runs it). FIX: both over-read gates now require !owned_files.is_empty()
so they only apply to workers that HAVE files to write; no-owned verifiers are bounded by the idle worker_timeout
instead. Test over_read_exempts_no_owned_task. VERIFY on UNIQ6+ (new binary): integrate-verify should run to
completion, run-status should stop false-failing working apps. REMAINING cause (separate): UNIQ2/ABskeloff
integrate-verify failed with 0 attempts = BLOCKED by the failed tests subtask -> a failed test/dep should not
fail the run if the app RUNS (smoke passed). Pairs with tests-subtask-produces-nothing.

### [test-suite cost — campaign data] UNIQ5 test-suite = 23.5min (1408s), the single biggest task
The test-suite subtask is a major time sink on complex apps: UNIQ5 test-suite took 1408s (23.5min) — more than
any module. Combined with tests-subtask-produces-NOTHING (UNIQ4 + AB run2 shipped zero tests despite the cost),
the test-suite phase has QUESTIONABLE payoff: either very slow (UNIQ5) or empty (UNIQ4/run2). CAMPAIGN QUESTION
(quality+time, per the user method): does the per-app test-suite earn its 20+min on a slow fleet? Options to A/B
later: scope the test-suite smaller (a few golden-value tests not exhaustive coverage), OR make it a knob, OR
fix the empty-test failure first. Do NOT cut blindly — tests CAN catch bugs (but UNIQ4 had a budget bug AND
tests failed, so they did not help there). Pairs with the integrate-verify tail cost.

### [run-status 3rd facet + skeleton-first downside] finalize-spin false-kills a slow entry whose files WORK
UNIQ7 entry-point: attempt_history [judge_killed looping, judge_killed broken_code, judge_failed looping] -> 3
attempts exhausted -> FAILED -> blocked integrate-verify -> run-status FAILED on an app that RUNS PERFECTLY.
ROOT: the finalize-spin gate (judge.rs:325) fires Looping when an owned file is written but UNTOUCHED >=420s
while the worker still runs ("stuck re-verifying"). SKELETON-FIRST INTERACTION (a downside of my own change):
skeleton-first writes the file EARLY (the skeleton), so the 420s stale-timer starts early; while the slow 27B
THINKS about filling, the file is untouched -> finalize-spin fires though the worker is mid-fill, not stuck.
A non-skeleton-first worker writes LATE (one big write) so the timer is low at finish. So skeleton-first INVERTS
the finalize-spin assumption -> false-kills on complex entries (the very place skeleton-first was meant to help).
FIX OPTIONS (subtle, do with FOCUS): (a) raise finalize-spin threshold for entry/skeleton-first tasks; (b) when
a task EXHAUSTS retries but its owned files EXIST + PARSE (no compile_errors), SALVAGE it -> mark DONE not FAILED
(the files are usable; risk: parsing-but-stub files); (c) finalize-spin should reset on ANY tool activity not
just file edits (a worker running checks IS making progress). RECONSIDER skeleton-first default-on pending a
CLEAN complex A/B — it may be net-negative on complex entries via this interaction. JUDGE BY RUNNING caught it.

### [PARTIAL FIX] finalize-spin re-dispatch loop — re-dispatched worker verifies not rewrites (0fd08ef34)
The dominant cost of the finalize-spin false-kill (UNIQ7 entry-point killed 3x though files work) was the
RE-DISPATCH rewriting the working file from scratch -> slow -> re-killed -> exhausted. Fixed (prompt): the
existing_block now tells a re-dispatched worker the file ALREADY EXISTS -> run it, report DONE if it satisfies
the spec, do NOT rewrite. Low-risk (worker still verifies, no false-DONE). STILL OPEN: the finalize-spin
THRESHOLD (420s too aggressive for slow skeleton-first entries) + the SALVAGE (exhausted+parses -> done, risky).
VERIFY on UNIQ9+ (a complex entry should re-dispatch faster + succeed more often).

### [re-plan-after-ASK waste — 4th confirmation + assessment] UNIQ9 also ~30min planning
UNIQ4/5/7/9 all pay ~20-30min planning when they ASK. ASSESSMENT (read the plan loop + parallel_plan): the
re-plan (loop continue after ASK) re-calls parallel_plan = skeleton re-draft (~5min) + DETAILING (~15min, the
dominant cost — re-writes every subtask spec). So "re-detail-only" saves little (detailing IS the cost + must
re-run for the Q&A). The REAL lever: SKIP the re-plan entirely on the ASK retry — the Q&A is ALREADY injected
into worker prompts via research_findings, so workers get it regardless; the first plan's STRUCTURE is usually
fine (the ASK is about SEMANTICS not structure). Saves the WHOLE re-plan (~20min). RISK: a structural Q&A
would be missed (rare). This is a planning-flow behavior change -> needs a FOCUSED cycle + a guard (e.g. skip
re-plan only when the plan structure is unchanged / the Q&A is semantic) + test. NOT a rushed change. Confidence
MED (the quality impact of pre-Q&A specs is uncertain — worth an A/B: re-plan-on vs skip, same ASK spec).

### [UNIQ9 execute — over_read kills the ENTRY on attempt 1; skeleton-first not forcing an early write]
UNIQ9 dispatch trace: cli-app / tests-advanced / tests-core each dispatched x2 (re-dispatched ONCE);
0 finalize-spin kills, 0 kill loops (vs UNIQ7 entry killed 3x). The re-dispatch cause is over_reading
(3 over_reading verdicts, on exactly those 3 tasks) — killed on attempt 1 (>=16 tool_calls, no owned
write yet), recovered on attempt 2 (commands.py 6920b + __main__.py 3270b written). NET: the entry
recovered in ONE re-dispatch = BETTER than UNIQ7, but attempt 1 was wasted.
ROOT-CAUSE HYPOTHESIS (verify on completion via the session trace): the ENTRY legitimately reads
db.py/models.py/utils.py to wire imports, batching its reads BEFORE the first write -> trips the
over_read gate. skeleton_first is SUPPOSED to make the entry write a stub FIRST (which would give it an
early owned-write and exempt it), but cli-app still read-first -> the entry worker likely IGNORED the
skeleton-first instruction. FIX DIRECTION (atomic-writes theme, NOT relaxing the gate): make
skeleton_first ACTUALLY force the entry to write its stub before reading siblings (stronger prompt /
verify it emits an early write). CONFIDENCE MED — confirm from the attempt-1 session trace that the
entry read-batched before writing; if so this is a concrete skeleton_first-effectiveness bug.

### [VERIFIED from session traces — over_read gate is CORRECT; hypothesis OVERTURNED] UNIQ9
Read the killed attempts' actual tool calls (sessions.db) BEFORE touching judge.rs (memory read-logs-first).
Result CORRECTS the prior "over_read gate false-kills legitimate dep-reading" hypothesis:
- tests-advanced ATT0 (killed, 5 calls): cat on DIRECTORIES (errors), repeated find, repeated cat = GENUINE
  flailing/exploration, NOT clean distinct-dep reads. The over_read gate caught a REAL spin. Gate is RIGHT.
- tests-advanced ATT1 (killed, 12 calls): flailed 5 calls, THEN wrote its test file (call 6), ran pytest,
  then spent calls 7-12 reading/re-running to debug a FAILING test WITHOUT editing the file for >420s ->
  finalize-spin killed (backlog B, not over_read).
- cli-app ATT1 (RECOVERED, session 32): wrote commands.py + __main__.py IMMEDIATELY (calls 1-2), then ran +
  edited. CLEAN, no flailing. The verify-not-rewrite re-dispatch fix WORKED.
CONCLUSION: DO NOT relax the over_read gate (would let real flailing run). The real lever is the WEAK
tests-writer FLAILING on exploration (cat dirs, find) instead of reading the known dep files directly + then
finalize-spinning while debugging a failing test. FIX DIRECTIONS (verified-grounded):
  (1) anti-flail worker prompt: "You have the exact file manifest + dep APIs injected. Do NOT run find or cat
      directories; cat the SPECIFIC dep files you need (they error on a dir), then WRITE." CHEAP, low-risk,
      MED confidence (weak models may ignore, but can only help).
  (2) finalize-spin-while-debugging (backlog B): a worker that wrote its file then debugs a failing test for
      >420s without an edit is killed as looping — sometimes it is genuinely stuck (weak model), sometimes
      mid-debug. Threshold/salvage question stands.
WIN for the discipline: reading the trace flipped the conclusion (would have wrongly disarmed a correct gate).

### [KEY — finalize-spin false-kill of the ENTRY cascades to block integrate-verify -> run-status REGRESSION] UNIQ9
run_finished: done=[core-models-utils, db-layer], FAILED=[cli-app, integrate-verify, tests-advanced, tests-core].
BUT the golden PROVES the app works end-to-end + smoke reported entry_ok:True. So the run FALSELY reports FAILED.
CAUSE CHAIN (verified from report.tasks[].attempt_history):
  - cli-app: status=failed, 3 attempts, last_outcome=judge_failed. att2 was applying the SpecDrift --date fix,
    wrote a WORKING __main__.py (golden confirms), then spun >420s -> finalize-spin (Looping) -> exhausted ->
    judge_failed -> marked FAILED. The produced file WORKS but the TASK is failed.
  - integrate-verify: status=failed, attempts=0 (NEVER RAN). It depends on cli-app (legit entry dep, NOT a test
    dep so the test-dep-strip does not help here). cli-app FAILED -> integrate-verify BLOCKED -> never dispatched
    -> run reports FAILED.
NET: a finalize-spin FALSE-kill of the entry (which produced a working file) cascades to block integrate-verify
and fails the whole run. REGRESSION vs UNIQ8 (honest DONE). This is a run-status honesty bug, SAME CLASS as the
UNIQ8 fixes (judge-kill exemption, test-dep-strip) but a NEW facet: the entry itself finalize-spin-failing.
FIX = finalize-spin SALVAGE (scheduler.rs terminal fail path): when a NON-TEST task would be marked FAILED via
judge_failed/looping AND its owned files EXIST + PARSE (py_syntax_error clean), mark it DONE (salvaged) instead,
so a dependent (integrate-verify) can run + the run reports honestly. Scope to NON-TEST tasks (a parsing-but-
failing test is not done; and tests do not block integrate-verify anyway). Confidence MED-HIGH: the case is clear
+ the parse gate is safe (here the salvaged file actually WORKS per golden). Keep the finalize-spin test green.

### [UNIQ10 — cli-entry spec_drift FAILED (REAL, verified) + SALVAGE correctly did NOT fire + entry structural-drift is 2x now]
cli-entry-point terminal-failed via spec_drift (over_read -> spec_drift -> spec_drift FAILED, 3 attempts). VERIFIED
by smoke it is a REAL drift, not a false-negative:
  - `python -m splitwise --help` works (rc0) BUT commands are FLAT (group-add/member-add/expense-add) — spec
    required NESTED (group add / member add). And `--db` is PER-COMMAND not GLOBAL: `--db smoke.db init` -> rc2
    "No such option --db". Spec required a global --db BEFORE the subcommand.
  - Judge hint was exactly right: "spec requires a GLOBAL --db before subcommands ... code uses per-command
    positional db-path; commands should be nested groups like `group add`, not flat `group-add`".
PAYOFF: SpecDrift PAYS OFF (caught a real structural CLI-contract violation). Run-status HONEST (cli-entry failed
for a legit reason; integrate-verify will cascade-block, which is CORRECT here — the entry really is non-compliant).
SALVAGE correctly did NOT fire (salvaged_spin=0): the terminal verdict was spec_drift, not Looping — my salvage is
scoped to Looping only, and salvaging a genuinely-drifted entry would be WRONG. So SALVAGE validation on UNIQ10 is
INCONCLUSIVE (no finalize-spin occurred) — still shipped+unit-tested, awaits a real finalize-spin run.
KEY PATTERN: the ENTRY is repeatedly the hard part, drifting on STRUCTURE/INTERFACE — UNIQ9 (--date positional vs
--date flag) + UNIQ10 (flat commands + per-command db vs nested + global db). The weak model builds a working-ish
CLI but with the WRONG interface shape. FIX DIRECTION (high-value, 2x evidenced): a CLI-CONTRACT freeze analogous
to the DB-schema-freeze that PAID OFF — inject the EXACT required command tree (nested subcommands) + global option
signature into the entry worker prompt (and the entry skeleton pre-declares that argparse/click structure so the
worker only fills handlers, cannot drift the shape). Confidence MED-HIGH (mirrors the schema-freeze that worked).

### [UNIQ10 tail — AST review PAYS OFF (caught unwired commands.py) + entry REIMPLEMENTED inline, a 2nd entry problem]
UNIQ10 tail: test-suite completed (1535s=25min, slow), smoke gate PASS (entry runs), then AST review found 2:
(1) __main__.py `cli` function is a STUB/unimplemented; (2) `splitwise.commands` imported by NO non-test module =
built-but-UNWIRED. The entry (cli-entry-point, __main__.py 9182b) DUPLICATED the command logic INLINE instead of
importing+dispatching to commands.py (6433b) -> commands.py is DEAD. My golden worked because the logic is inline
in __main__.py. AST REVIEW PAYS OFF: caught it + dispatched ONE corrective wire-fix (gabee). So the entry has TWO
drift modes: (a) interface SHAPE (flat/global-db/cents) — the CLI-contract I just shipped targets this; (b)
REIMPLEMENT-INLINE instead of WIRE the sibling handlers — the CLI-contract does NOT target this yet. CANDIDATE
CLI-contract ENHANCEMENT (after validating the shape-contract on UNIQ11): add a rule — the entry only PARSES args
and DISPATCHES to the imported sibling handlers (import from e.g. splitwise.commands and CALL them); do NOT
reimplement their logic inline (that orphans the module + duplicates code). The entry-wiring instruction exists but
was not strong enough here. Note: the wire-fix may change UNIQ10 final state -> re-verify golden after run_finished
if __main__.py changed. Confidence MED (a prompt rule; weak model may still inline).

### [UNIQ10 tail — review wire-fix WORKED but the wire-fix worker SPINS, uncovered by SALVAGE (scheduler-only)]
The AST-review wire-fix rewrote __main__.py (9182b inline-duplicated -> 4182b): it now IMPORTS commands.py (no
longer orphaned) + keeps a working click group (--help rc0). So the review wire-fix FIXED the wiring (commands.py
wired) though it kept the FLAT interface shape (review targeted the unwired-module/cli-stub, not the shape drift).
BUT the wire-fix worker then SPUN: file written 5min ago, worker still GENERATING 9+min after dispatch, no
completion, no run_finished. This is a finalize-spin — but salvaged_spin=0 because the REVIEW wire-fix dispatches
OUTSIDE the scheduler, so the scheduler's finalize-spin gate + my SALVAGE do NOT cover it. FINDING (candidate fix):
the review corrective-fix dispatch lacks the finalize-spin/idle protection scheduler tasks have -> it can spin to
worker_max_turns, delaying/hanging run_finished. Extend a spin/idle-timeout (and possibly SALVAGE) to the review
dispatch path. Confidence MED (need to locate the review-dispatch code; separate from the scheduler terminal path).
Also reinforces the wire-not-inline CLI-contract enhancement: had the entry wired commands.py the first time, the
review wire-fix (and its spin) would not have been needed. CUT UNIQ10 here (stall 3+, app stable, verdict recorded)
to free the fleet for UNIQ11 (validate the CLI-contract).

### [NUANCE — re-plan-after-ASK is NOT pure waste: it produced a BETTER plan] UNIQ12
Earlier I framed the ASK re-plan as ~15min WASTE. UNIQ12 refines that: after the answers, the re-plan re-drafted
to confidence 88/100 (UP from the pre-answer 69) AND restructured 7 subtasks -> 5 (cleaner). So the re-plan
incorporates the answers into a measurably better plan (higher cross-draft agreement + tighter decomposition),
not just a delay. IMPLICATION for the ASK_REPLAN A/B (=1 re-plan vs =0 skip-reuse): the metric must be APP QUALITY
(build+run+correct), not only wall-clock. Skip (=0) reuses the LOW-confidence 69 plan + answers-via-research_findings;
re-plan (=1) builds the 88-confidence plan. If the better plan yields a better app often enough, the ~15min pays
off; if apps are equal, skip wins on time. Do the A/B on a spec that reliably ASKS (like this helpdesk/library
class, conf < 80). Confidence MED — genuinely uncertain which wins; that is WHY it needs the A/B, not a guess.

### [UNIQ12 — AST review FALSE-POSITIVE: `from PKG import MODULE` not detected as wiring -> spurious unwired finding + wasteful wire-fix]
UNIQ12 (a CLEAN WIN otherwise): smoke PASS (entry_ok), integrate-verify DONE, all 3 test tasks DONE (tests
PASSED — better than UNIQ9/10), CLI-contract WIN. BUT the AST review flagged "module helpdesk.cli is imported by
no non-test module — built-but-unwired". FALSE POSITIVE: __main__.py does `from helpdesk import cli` (verified) and
the golden RUNS (init/agent add/ticket open all rc0 via __main__ -> cli). So cli IS wired. The AST reviewer likely
detects `import pkg.mod` / `from pkg.mod import X` but NOT `from pkg import mod` (importing a submodule as a package
attribute). This spurious finding then triggers an UNNECESSARY review wire-fix (observed gabee processing) — wasted
work on a non-problem (and connects to the UNIQ10 review-wire-fix-spin waste).
CANDIDATE FIX (HIGH value, clean, fixes the CAUSE not the symptom): improve the AST review import-detection to also
count `from PKG import MOD` (and `from . import MOD`) as importing PKG.MOD, so a module wired that way is NOT flagged
unwired. MUST READ the AST-reviewer code first (swarm.rs review phase / the model-free AST reviewer) to confirm the
exact detection gap before building. Confidence MED-HIGH (clear false-positive + a well-scoped detection fix; risk =
missing another import form). This is likely the NEXT fix to build (beats the ASK_REPLAN A/B on cleanliness).

### [UNIQ13 — hardest module FLAILS on over_read, never writes (VERIFIED, 2nd instance) -> UNIQ13 FAIL] 
plan-shopping-module (owns plan.py + shopping.py; needs recipe/ingredient/pantry/db to aggregate) FAILED via
over_reading x3 -> cascade-blocked cli-entry-point + tests + integrate-verify -> UNIQ13 FAIL. VERIFIED from the
session traces (271 att0, 275 att1): att0 = ls x3, find, cat x3 (7 calls, NO write); att1 = tree, find x2, cat x4
(7 calls, NO write). It EXPLORES the layout (ls/tree/find) + reads deps (cat) but NEVER commits a write to its
owned files -> over_read gate kills it (correct). The worker prompt ALREADY forbids ls/find/tree + says WRITE
FIRST, but the weak model IGNORES it on this hard 2-file/4-dep task. This is the 2nd VERIFIED instance of the
hardest-module-flails pattern (UNIQ9 tests-writer was the 1st: cat dirs, find).
FIX CANDIDATES (over_read gate is CORRECT — do NOT relax it): the ONLY mechanically-reliable lever is to FORCE an
early owned-file write, because any_owned_written=true then EXEMPTS the over_read gate (not a prompt plea the model
ignores). LEADING: extend SKELETON-FIRST beyond the entry to MULTI-FILE / complex modules (owned_files.len() > 1,
like plan-shopping) — first action writes a COMPILING stub of each owned file (imports + signatures + pass bodies),
then read deps + fill. Currently skeleton-first is is_entry_file-only. Confidence MED (mechanical early-write helps,
but the weak model might write a broken stub; and skeleton-first was a WASH on SIMPLE apps so scope it to multi-file
/hard modules only, not blanket). SECONDARY: a stronger over_read RE-DISPATCH hint (you failed twice by exploring;
your VERY FIRST action MUST be write to <path>, no other command) — but prompt-pleas are what the model already
ignores. ASSESS + build the skeleton-first-for-multi-file after UNIQ13 finishes (or is cut). N=2 evidence.

### [UNIQ14 — multi-file stub-first fix NOT tested on the hard case (plan variance)]
UNIQ14 (same recipe spec as UNIQ13) plan = shared-types(__init__+models), database-layer(db.py), cli-entry(cli.py+
__main__.py), tests, integrate-verify. UNLIKE UNIQ13, the architect MERGED all commands + the shopping aggregation
into cli-entry (an ENTRY task) rather than splitting a separate non-entry plan-shopping module. So multifile_stub_note
(which is empty for entry tasks — skeleton_note covers them) fired only for shared-types (2 files, but __init__ is
trivial = EASY), which passed 0 over_read. The HARD case (a NON-entry complex multi-file module like UNIQ13
plan-shopping) did NOT recur -> the fix is UNTESTED on the case it targets. HONEST: shipped + unit-tested + weak-
positive on easy multi-file, but no in-the-wild proof on a hard non-entry multi-file module yet. cli-entry (the
complex entry with the aggregation) is handled by skeleton_note (entry skeleton-first) — watch it survive.
TO TEST PROPERLY: UNIQ15 needs a spec the architect will SPLIT into non-entry multi-file modules — i.e. 2+ distinct
complex algorithmic domains (not a thin CLI over one engine). Note: whether a module is entry-vs-non-entry-multi-file
is PLAN-dependent + not fully controllable; the fix is best-effort + the hard mechanism (raise the no-write elapsed
gate for owned_files.len()>1 in judge.rs) remains the fallback if a hard multi-file module flails again.

### [UNIQ14 — CLI-contract note REDUCED but did NOT eliminate entry drift; SpecDrift is the backstop (re-dispatch loop)]
UNIQ14 cli-entry drifted DESPITE the CLI-contract note: 2 spec_drift verdicts — (1) --db not added as a GLOBAL
option before parse, (2) positional servings/qty/unit instead of --flags. SpecDrift caught BOTH and re-dispatched
(4 dispatches). Current --help IS now compliant (recipes [-h] [--db DB] {init,recipe,ingredient,pantry,plan,
shopping}) so the loop is converging the interface, but a mid-fix golden showed recipe add not persisting (broken
transient). HONEST: the CLI-contract note is NOT a complete fix — it worked cleanly on UNIQ12 but the UNIQ14 entry
still drifted; SpecDrift + re-dispatch is the backstop that recovers it (at the cost of a 4-dispatch loop). So the
CLI-contract PAYS OFF partially (fewer/faster drifts) but SpecDrift remains essential. Not a new fix needed — just
honest scope: CLI-contract reduces entry drift, SpecDrift catches the residual. Watch UNIQ14 cli-entry converge or
exhaust; golden the FINAL state only.

### [UNIQ15 — multi-file STUB-FIRST fix VALIDATED on the HARD case (trace-confirmed WIN)]
UNIQ15 (ledger, 2 complex domains) forced a dep-heavy non-entry multi-file module: balance-reports owns balance.py +
reports.py (the reporting engine: reads types+db+transactions to compute running balances, trial-balance, income
statement) = the EXACT UNIQ13 plan-shopping pattern that over_read-flailed (ls/tree/find/cat, never wrote, killed x3).
RESULT: balance-reports dispatched ONCE, 0 over_read, 5 ok verdicts, both files on disk. TRACE-CONFIRMED (session
20260701_349): its FIRST TWO actions are `write` balance.py + `write` reports.py, THEN shell/checks — STUB-FIRST
exactly as multifile_stub_note instructs. NO exploration first. Also shared-types-db (3-file) clean, transaction-logic
(posting/balancing engine) DONE no broken_code. So the multi-file STUB-FIRST fix (1d6ac3d1a) WORKS on the hard case:
a dep-heavy 2-file non-entry module now writes stubs first (any_owned_written true -> exempt over-read gate) instead
of flailing. CONTRAST proven: UNIQ13 plan-shopping att0 = ls x3/find/cat x3/no-write/killed vs UNIQ15 balance-reports
= write/write/checks/clean. VERDICT: multi-file stub-first VALIDATED (N=1 hard-case trace-confirmed + shared-types-db
+ UNIQ14 easy-case). The HARD gate fix (raise no-write elapsed cap for multi-file) is NOT needed — the note sufficed.
Golden pending on run completion (does the reporting math compute right).

### [UNIQ15 — TEST-writers over_read despite multifile_stub_note (N=2); gate+re-dispatch RECOVERS (anti-flail-tests)]
Both test tasks in UNIQ15 are MULTI-FILE (test-transactions-decimal owns test_transactions.py + test_decimal.py;
test-balance-reports owns test_balance.py + test_reports.py) so multifile_stub_note applied — yet BOTH got over_reading
on attempt 0. test-transactions-decimal recovered on att1 (done, 388s); test-balance-reports re-dispatched (att1
running). So the stub-first note WORKS for a logic module (balance-reports wrote stubs first, clean) but does NOT
prevent TEST-writer flail: a test writer must read the module-under-test to write meaningful assertions (read-then-
write is its nature), and a test stub (def test_x(): pass) is not a natural early-write the way a module stub is.
IMPORTANT: the over_read gate + re-dispatch RECOVERED both (safety net PAYS OFF) — this is waste (an extra dispatch
+ ~388s), NOT a correctness failure. So a fix here is an OPTIMIZATION (save the flail dispatch), not a repair.
FIX CANDIDATE (MED-LOW confidence — prompt plea a weak model may ignore): a TEST-writer-specific note — the module-
under-test API is ALREADY injected in the dep_block, so tell the test worker to write tests FROM the injected API and
NOT open the implementation files (behavior via public API, not internals) -> less reading -> less over_read. MUST
READ the att0 test-writer trace FIRST (ts 08:36:30, map to session) to confirm it flails on reading-the-impl vs
exploring, before building. Do NOT relax the over_read gate (it correctly recovered). N=2 evidence.

### [UNIQ15 — ENTRY left ALL handlers as NotImplementedError stubs, marked done (skeleton-first hazard) — DOES THE REVIEW CATCH IT?]
UNIQ15 golden (app files complete): --help works (nested + global --db correct via cli.py build_parser), BUT every
command fails — __main__.py has ALL 8 handlers as `raise NotImplementedError("X not implemented yet")` (init_db,
add_account, list_accounts, add_transaction, list_transactions, get_balance, trial_balance_report,
income_statement_report). The entry (cli-entry-point, owns cli.py + __main__.py) wrote the compiling skeleton (parser
real, handlers registered, --help passes) but NEVER FILLED the handler bodies to call the modules, and was marked
DONE (994s — likely wrote skeleton, --help passed, ran out of turns before filling). Modules (accounts/transactions/
balance/reports .py) have real logic (balance-reports validated write-first). So the app is 100% non-functional
DESPITE clean modules — the skeleton-first hazard (entry accepted as done with placeholder bodies) FIRED on a complex
8-handler entry. NOT caused by multifile_stub_note (empty for entry). KEY: the AST_REVIEW_SCRIPT ALREADY detects
`raise NotImplementedError` as STUB/UNIMPLEMENTED (swarm.rs:4772/4802) and fires an ast_fix — BUT review runs at the
END (after integrate-verify, still running when goldened). SO: this is the PAYOFF TEST — does the review phase catch
the 8 stub handlers + re-dispatch a fix that fills them? IF YES -> phases pay off (skeleton hazard is backstopped as
the code comment claims), UNIQ15 recovers. IF NO (review misses __main__.py, or the fix re-dispatch fails/stubs
again) -> that is the real gap to fix (e.g. detect NotImplementedError in the completion guard so the entry is NOT
marked done with stubs, not just at end-review). WATCHING run completion. Do NOT build blindly — the detector exists.

### [UNIQ17 — CLI-contract POSITIONAL fix VALIDATED; but cross-module SCHEMA DRIFT bug (watching review)]
CLI-contract positional-vs-flag strengthening (a9466d898) VALIDATED: UNIQ17 cli.py kept EVERY spec positional —
member add NAME -> add_argument('name'); book add ISBN -> add_argument('isbn'); loan out ISBN MEMBER -> TWO
positionals add_argument('isbn')+add_argument('member'); report member MEMBER -> positional; --from/--to KEPT (not
renamed --source/--dest); --db global. Confirmed by RUNNING: member add Alice -> rc0 (positional accepted, NOT rc2
argparse). Direct contrast to UNIQ16 which drifted ALL positionals to flags. The fix WORKED (N=1 clean). Note:
cli-entry got a spec_drift verdict (caught + corrected -> final contract compliant) so note+SpecDrift worked together.
BUT the APP has a CROSS-MODULE SCHEMA DRIFT bug: report overdue crashes sqlite3.OperationalError: no such column:
isbn — db.py loans table schema is MISSING the isbn column that commands.py queries (SELECT isbn ... FROM loans).
book add + loan out + report crash on it; member add works. This is the classic contract-drift failure class (db
schema vs command queries) that DB-schema-CONTRACTS + CONTRACTS are meant to prevent — the schema still drifted.
WATCHING: run still finalizing (integrate-verify/review) — does the review/smoke catch the OperationalError + fix the
schema (like UNIQ15 stub entry)? If YES = review pays off again. If run finishes broken = the schema-contract needs
strengthening (inject the EXACT db table+column DDL into the command modules, or a smoke query check). Also tests-core
over_reading = N=3 for anti-flail-tests (UNIQ9, UNIQ15, UNIQ17) — recovers via re-dispatch though.

### [UNIQ17 — review/fix loop FIXED the schema but INTRODUCED an argparse regression (review-fix not verified)]
Follow-up to the schema-drift payoff test: the review/fix loop DID address the schema (db.py loans table now has
`isbn TEXT NOT NULL REFERENCES books(isbn)` — the missing column was added). SO review-catches-schema-drift = YES
(it ran, found the runtime SQL error class, re-dispatched a schema fix). BUT the SAME finalization introduced a NEW
bug: every command now crashes `TypeError: dest supplied twice for positional argument, did you mean metavar?` — a
fix re-dispatch added dest= to a POSITIONAL add_argument (illegal). Confirmed a REGRESSION: my first golden had
member add Alice -> rc0 (clean), now it crashes -> the fix broke cli.py. The fix's own verification did NOT catch
that EVERY command crashes on import/parse (an argparse construction error fails at first invocation — a trivial
--help would have caught it). EVIDENCE for backlog (2b) review-wire-fix-spin-protection / fix-not-verified: a
review/hardening re-dispatch that edits the entry MUST be gated on a post-fix smoke (python -m pkg --help exit 0)
before accepting — else it can ship a WORSE app than it started with. HONEST net: UNIQ17 positional fix VALIDATED
(headline, from the ORIGINAL clean cli.py) but the app was left BROKEN by the fix-loop regression (argparse dest
error). Fix candidate: after any review/smoke corrective re-dispatch that touches the entry, re-run the --help/
collect-only smoke and REJECT the fix (keep prior version or re-dispatch) if it now fails. Confidence MED. READ the
fix re-dispatch trace first to confirm which task added dest=.

### [UNIQ21 — raised-difficulty FAIL = weak-model coding limit, NOT a swarm-mechanism gap (trace-confirmed, no fix built)]
UNIQ21 (contacts, 10-cmd + multi-format-everywhere + JSON round-trip) FAILED honestly (run_finished marks cli-entrypoint
+ integrate-verify + test-formats-roundtrip + test-search-errors FAILED — run-status HONEST, no false success = GOOD).
Bugs: (1) format_members expects list-of-DICTS (m['contact_name']) but cli.py passes STRINGS -> member list crashes
TypeError (cross-module DATA-SHAPE mismatch; CONTRACTS injects signatures not data shapes); (2) export --file ->
unrecognized-argument (arg drift on export); (3) contact multi-format json/csv WORKS but member multi-format broken
(2nd formatter written inconsistently); (4) CLI structure flattened (nested->flat on 10-cmd surface).
TRACE (session 20260701_636): cli-entrypoint worker WROTE cli.py as its FIRST action (write-first, 7 calls) — so the
skeleton_note WORKED and the entry did NOT over_read from reading deps. Therefore the earlier "large-command-split /
big-entry-over_read" hypothesis is NOT confirmed — I did NOT build it (would have been the wrong fix). The failure is
GENUINE weak-model coding errors on a harder app (data-shape disagreement between 2 modules, arg drift), which the
swarm CORRECTLY caught + reported. NO clean MED+ confidence mechanically-sound swarm fix -> model-capability ceiling.
DECISION: no fix built (confidence rule — do not overbuild on 1 partial off an unconfirmed cause). The reusable
learnings: swarm HONESTLY fails at raised difficulty (does not lie); the ceiling is cross-module data-shape agreement
+ consistency across sibling modules (contract-drift class, the original v8 motivation) — a HARD problem for a weak
model. A future CONTRACTS enhancement conveying argument DATA SHAPES (not just signatures) could help but is MED-LOW
confidence + a big surface -> parked, not built. Calibrate: UNIQ22 isolates JSON round-trip ALONE (smaller surface,
1 entity, no multi-format) to test whether round-trip itself is the ceiling or it was UNIQ21's COMBINATION.

### [smoke+tests miss runtime validation/correctness bugs — N=2 finding + HONEST fix-confidence assessment]
FINDING (N=2): SMOKE = collect-only (imports) + --help (exit 0), swarm.rs:3746 run_smoke_gate — it NEVER runs commands
nor executes the tests. So apps ship with real runtime holes that "pass smoke": UNIQ21 (member list crash + export
--file broken) and UNIQ24 (order add skips unknown-customer + neg-amount guards, both rc0). Thin/over-read tests
compound it (UNIQ24 tests over_read -> none ran). This is the real recurring swarm-quality gap now that format/round-
trip/single-dim are all solved.
FIX CANDIDATES — assessed with HONEST confidence (user rule: flag LOWER confidence + why, never rank by effort):
  A) VALIDATION-SMOKE (extract spec 'reject with nonzero' clauses, RUN each, assert nonzero) — would catch the UNIQ24
     class (rc0-should-be-nonzero). CONFIDENCE: LOWER / the weakest of the three. WHY: it needs the weak local model to
     generate commands that are valid in EVERY field except the one validation under test, then disambiguate rc2
     (argparse/contract) vs rc1 (real validation) vs rc0 (bug). That is EXACTLY the measurement I (Opus) keep slipping
     on by hand (3 golden re-runs this campaign). A weak model doing it will false-fail (rc2 read as pass, or a
     mis-arg'd command read as a bug). I do NOT have confidence I can make this reliably non-false-failing -> do NOT
     rush it.
  B) TRACEBACK-SMOKE (run a happy-path command sequence, FAIL on a Python traceback in stderr) — catches the UNIQ21
     crash class only, NOT UNIQ24 (rc0, no crash). CONFIDENCE: MED. Assertion is deterministic (traceback=fail); the
     fragility is model-deriving a valid happy-path sequence.
  C) SMOKE RUNS THE TESTS (pytest -q, not just --collect-only) — CONFIDENCE: HIGH/deterministic + cheap + low blast
     radius. LIMITATION: payoff is bounded by test quality — catches nothing when tests over_read (UNIQ24) or are thin
     (UNIQ21). Strictly-positive but does not fix the two observed cases.
DECISION: NO fix built yet. The highest-PAYOFF fix (A, catches UNIQ24 class) is the LOWEST-CONFIDENCE (fragile model-
driven command-gen + rc-disambiguation); building it now risks a false-failing gate that HURTS good runs. Per the
confidence rule I flag that honestly and gather more evidence first (UNIQ25 = validation-stress app -> does the gap
recur at N=3?). If N=3 confirms, the least-fragile increment is likely C (run the tests) PLUS strengthening test
quality, not A. Root cause is partly MODEL capacity (weak model writes incomplete validations + thin tests) — no cheap
knob fully fixes that; be honest about the ceiling rather than ship a fragile gate.

### [CORRECTION — the "smoke+tests miss runtime validation bugs" finding COLLAPSES]
Re-checked UNIQ24 AFTER run_finished: the 2 order-add validation bugs (unknown-customer, neg-amount) are BOTH FIXED
(rc1) in the final app — the run's INTEGRATE-VERIFY phase fixed them; I had goldened BEFORE it ran. So UNIQ24 does NOT
evidence the "smoke misses validation bugs" finding — the swarm CAUGHT them. Combined with UNIQ25 (11/11 validations
implemented directly), the validation-completeness concern is CLOSED: (1) the weak model implements enumerated
validations well; (2) when it misses one, integrate-verify/review catches it (UNIQ24); (3) only a total run FAILURE
(UNIQ21, honestly marked failed) ships holes. NET: no validation fix warranted, AND the earlier N=2 was a MEASUREMENT
error (goldened mid-run). The real reusable lesson is the DISCIPLINE one: golden after run_finished, not mid-run.
smoke-runs-tests remains a minor nice-to-have (HIGH conf, cheap) but is NOT solving a demonstrated shipped-bug problem.
