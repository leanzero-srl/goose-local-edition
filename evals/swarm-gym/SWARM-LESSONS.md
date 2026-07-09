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
