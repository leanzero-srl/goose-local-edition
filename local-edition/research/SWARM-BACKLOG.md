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
