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
