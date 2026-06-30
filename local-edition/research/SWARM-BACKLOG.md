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
