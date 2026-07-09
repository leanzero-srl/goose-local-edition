# Swarm lessons

## habits (2026-07-09): validates READ errors but not stored-bad-input replay
The swarm caught the corrupt-DB edge (JSONDecodeError caught at main) but MISSED input
validation at WRITE time: `check NAME --date not-a-date` stores the bad string silently
(exit 0, no traceback), then a LATER `report`/`streak` raises an uncaught
`ValueError: Invalid isoformat string` (habits/tracker.py compute_streak → date.fromisoformat).
Lesson: a "handle malformed date cleanly" requirement needs validation at the point of WRITE,
not only try/except around reads. JUDGING MATRIX: always test stored-bad-input replayed through
a downstream command, not just the bad input in isolation. Operational lesson: tear down the
PRIOR harness task before launching the next — a live old harness collides with the new run
over the shared runs/operator/ handshake (habits be96xfus0 collided with csvstat be2gbtap4).

## RECURRING (2x): swarm handles weird-present-data but misses ERROR-PATH boundaries
habits H2: `check --date not-a-date` accepted (no validation) → later report/streak raises uncaught ValueError.
csvstat H2b: a missing/nonexistent FILE raises an uncaught FileNotFoundError (reader.read_csv unguarded,
main() has no error boundary) — csvstat/__main__.py has no try/except around reader.read_csv.
COMMON THREAD: the swarm reliably handles PRESENT-but-weird data (non-numeric column, ragged rows,
headers-only, corrupt-JSON caught) but consistently misses the FILE-ACCESS / INPUT-PARSE error boundary
(missing file, malformed input validated at write). GRADUATED GATE: every judge MUST test (1) a missing
input file, (2) malformed input at the boundary, and every spec should explicitly require a clean error
(not a traceback) for both. This is the sharpest, most repeatable qwopus-27b weakness so far.

## POSITIVE (csvstat): review-fix phase CAUGHT the recurring error-path bug
GOOSE_SWARM_REVIEW_REPRO/FIX amended csvstat/__main__.py after the initial build, adding a
try/except FileNotFoundError/ValueError boundary around reader.read_csv — fixing the exact
missing-file H2b gap the initial build missed (verified: missing file now -> clean 'error:', 0
tracebacks; 44 pytest still pass). So the adversarial review-fix loop DOES close the error-path
weakness when given time (the fix landed ~14min post-build, jsonl logging lagged). Signal: keep
REVIEW_FIX on; it converts near-pass -> full-pass on the recurring gap. NOTE: the harness then sat
stuck-idle post-complete (jsonl frozen ~19min) even though the fix had landed — the run doesn't
cleanly terminate/emit DONE, so teardown-after-verify remains necessary.

## MOLD-THE-MODEL WIN (logstat): explicit error-path requirement → first-build clean
The recurring error-path weakness (missing file, malformed input) — which habits SHIPPED as a latent
bug and csvstat only fixed via review-fix — was closed in the FIRST build once made an EXPLICIT
weight-3 requirement (R6 missing FILE clean, R7 malformed line unparseable). Verified: 0 tracebacks
across count/filter/range/tail, 26 pytest pass. The model reached for IDIOMATIC guards when told to
(argparse choices for --level, a custom argparse type validating the timestamp) rather than bespoke
try/except. CONCLUSION: for a known model weakness, the highest-leverage lever is naming it explicitly
in the spec — not hoping review-fix catches it. The graduated gate is now a SPEC requirement, not just
a judging gate. Progression across the session: hidden→latent-bug (habits) → hidden→review-fix-caught
(csvstat) → explicit→first-build-clean (logstat).

## CONFIRMED (jsonpath): mold-the-model win REPEATS (2nd consecutive)
jsonpath = full pass on the FIRST build with explicit weight-3 error-path reqs (R6 missing file, R7
malformed JSON, R8 invalid path). Not only clean (0 tracebacks, 66 pytest) but the error MESSAGES were
high-quality/specific ('index 9 out of range for array of length 2', 'expected array, got string',
'Empty segment in path'). Two-for-two (logstat, jsonpath) confirms: naming the known error-path
weakness explicitly in the spec reliably closes it at build time AND yields good diagnostics. The
graduated gate as a SPEC requirement is now a proven, repeatable lever — keep it in every spec.

## REINFORCED (tomlq): explicit error-path reqs land, HIDDEN ones are hit-or-miss
tomlq: both EXPLICIT weight-3 error paths (R6 missing file, R7 malformed TOML) clean first-build,
51 pytest pass, all TOML types incl datetime correct — but the ONE hidden error path I did NOT make
explicit (H3 malformed path 'a..b') raised an uncaught ValueError. Contrast jsonpath, which made
invalid-path an EXPLICIT R8 and handled it cleanly. So: EVERY error boundary you want handled must be
an EXPLICIT weighted requirement — the model reliably guards what's named and reliably misses what's
merely hidden. Running tally: explicit error-path reqs → clean 3/3 (logstat, jsonpath, tomlq); the
only error-path misses (habits, iniedit, tomlq-H3) were all on NON-explicit/hidden boundaries.

## NUANCE (csvsql): explicit error-path lever is strong but NOT a guarantee on complex apps
csvsql (a mini SQL engine — the hardest app so far) handled 2 of 3 EXPLICIT weight-3 error boundaries
first-build (R6 missing-file clean, R8 invalid-query clean with great parser messages) but MISSED the
3rd (R7 malformed/ragged CSV -> uncaught TypeError), plus a hidden H2 (numeric-compare-on-'N/A' ->
ValueError). Query engine itself was excellent + 53 pytest pass. Takeaway: naming a boundary explicitly
strongly raises the odds it's handled, but on a COMPLEX app the model's attention is finite and a guard
can still slip — the harder the core logic (here a SQL parser), the more an error-path req can be
crowded out. So for complex apps, consider ALSO an explicit deterministic check that exercises the
ragged/malformed input, not just the requirement text. Running tally: explicit error-path reqs clean on
the simpler read-only-query apps (logstat/jsonpath/tomlq); first miss on an explicit boundary was csvsql
R7 (the most complex app).

## REFINEMENT (calc): explicit error boundaries land best when IN THE CORE LOGIC PATH
calc (fresh parser/evaluator domain) landed ALL 3 explicit error boundaries clean first-build
(R5 malformed, R6 div-by-zero, R7 unknown-name) with excellent parser-style error messages, + all
functional correct (precedence/right-assoc/functions/constants), 86 pytest pass. Contrast csvsql where
R7 (ragged CSV) slipped. The difference: calc's 3 boundaries are all IN the parse/eval path the model
built with care; csvsql's R7 lived in the PERIPHERAL csv reader, separate from the SELECT feature it
focused on. REFINED LEVER: an explicit error-path req lands most reliably when the boundary sits inside
the core feature logic; boundaries in peripheral/support components (file readers, format parsers you
didn't emphasize) are more likely to slip even when explicit — so for those, add a deterministic check
that RUNS the malformed input. Also seen: the swarm sometimes writes a TEST that contradicts the spec
(calc's unary-vs-power test asserted the wrong precedence while the code was right) — a judge must read
the code+spec, not trust the app's own tests alone.

## RISK (calc regression): review-fix resolves code-vs-test conflicts by trusting the TEST
calc originally computed -2**2 = -4 (correct: spec + Python + math — unary minus is LOWER precedence
than **) but shipped with a self-contradictory unit test asserting (-2)**2 = 4. The harness review-fix
then EDITED parser.py to make -2**2 = 4 so the test would pass — i.e. it changed CORRECT code to match
a WRONG test, introducing a spec violation (87 pytest 'pass' but the semantics are now wrong). LESSON:
the swarm's review-fix loop treats a failing self-authored test as ground truth and edits the code to
satisfy it; when the test is the buggy one, this REGRESSES correct behavior. Mitigations: (a) the judge
must always verify against the SPEC by running golden inputs, never trust "all tests pass"; (b) consider
a review-fix guard that, on a code-vs-test conflict, re-derives the expected value from the spec rather
than assuming the test is right; (c) precedence/semantics belong in explicit spec check_hints AND a
deterministic_check that runs the golden case (e.g. calc '-2 ** 2' must print -4), so review-fix can't
silently flip it. This is the first observed case of review-fix making an app WORSE.
