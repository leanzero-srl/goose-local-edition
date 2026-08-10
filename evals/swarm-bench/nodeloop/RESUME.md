# RESUME — stopped 2026-08-10 20:12 for a laptop power-down

The 5-minute loop is STOPPED. Working tree is clean; everything below is committed.

## State at shutdown

- **The sweep supervisor died at ~18:35 with no traceback** (`runs/nodeloop/loop.log` simply ends at
  the `NOW: baseline-n3-r2` line). Cause not established — suspect an OOM/jetsam kill or a torn-down
  parent. **This is unfinished business: nothing announced the death for ~75 minutes.**
- Its engine child (`goose swarm run`, pid 44400, entrant dir `swarm-3node-r2`) was **orphaned and
  still running** at 1h31m when the machine was stopped. Powering down ends it. Nothing is corrupted:
  the cell has no `nodeloop-result.json`, so the sweep treats it as not-done and re-runs it.
- `STOP` sentinel is ABSENT, so a restarted sweep begins work immediately.

## Restart, in order

    cd /Users/mihaiperdum/Projects/goose/evals/swarm-bench/nodeloop
    python3 boundary.py          # must say BOUNDARY-REACHED before anything else

1. **Fleet first.** `~/.lmstudio/bin/lms ps` must show **three DISTINCT identifiers**; the sweep
   measures nothing without them. Do not reconfigure the fleet unprompted.
2. **Optionally recover the dead unit's number** — only if `swarm-3node-r2/vendorsync/` survived the
   shutdown: `python3 orphanscore.py swarm-3node-r2`. INFORMATIONAL ONLY, never pooled, never quoted
   as pair r2. Do it BEFORE restarting the sweep; the entrant dir is reused and gets overwritten.
3. **Restart the sweep detached** and confirm it reparented:

        python3 -c "import subprocess,sys; subprocess.Popen([sys.executable,'-u','sweep.py'], \
          stdout=open('/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop/loop.log','a'), \
          stderr=subprocess.STDOUT, start_new_session=True)"
        ps -o ppid= -p <pid>     # must print 1

## The one decision waiting

**A rebuild is owed and has never been taken.** `swarm.rs` carries four committed changes that are
NOT in the running binary (built 08:44, `1786340680-235925264`):

- `probed_post` on the `spec_contract` event (`02701dc44`)
- `RepeatedPost::Vacuous` (`e958c9d2d`)
- 5xx on an advertised POST is now a finding (`78fb94047`)
- `GOOSE_SWARM_DOC_EXAMPLES` (`f0495f049`)

Sequence, and none of it may be skipped: `boundary.py` → BOUNDARY-REACHED, then `./greengate.sh`,
then build, then **`python3 probe.py --verify`** must show `probed_post` FLIP absent→present (its
pre-rebuild baseline is already recorded), then flip the `doc_examples` arm from reps 0 to 3.

⚠️ **A rebuild resets the whole corpus** — `is_done()` keys on `engine_build()`. The two matched
pairs below belong to the current binary and cannot be compared across it.

## What today established

**F754 — the node curve shows NO effect.** Two matched pairs on this binary, signs disagreeing:

    r0   3-node 0.7226 (75m)   1-node 0.9283 (73m)   delta -0.2057
    r1   3-node 0.7110 (119m)  1-node 0.4784 (126m)  delta +0.2326
    mean +0.0134 score, +1.7% speed

F750's reversed sign was refuted by its own falsifier, and F737's +0.2268 is a single-sign reading
from the same two-sided distribution. **No node-count effect is established on any binary.** The
design needs 5 matched pairs to reach p=0.031. Observation worth chasing (n=2, NOT a claim): the
variance sits on the 1-node side — spread 0.0116 vs 0.4499.

**F753 — the vendor integration fails in half the corpus (7 of 14), differently every time.** Wrong
JSON key 2 of 7, cursor→5xx 1, sqlite cross-thread 1; the sqlite-guard hypothesis was cross-tabbed
and REJECTED. No single defect explains the class, so the leverage is the engine noticing an empty
sync and repairing — which is what the Vacuous and 5xx changes do.

**F752 — my own scorer overstated the breadth.** Tier B is not 12 independent checks; one root defect
zeroes 7 of them. Fixed by attribution, not re-weighting: `SCORER_VERSION` stays `sb-3`, no
re-scoring, nothing published moved. The sb-4 plan is WITHDRAWN — it would have lifted a broken app
from 0.7226 to 0.8309.

**Three of my own claims died today** (F751, the sb-4 plan, the key-explains-the-class emphasis) plus
the sqlite hypothesis. Each is recorded as refuted in `DEFECT-BOARD.md` so none is rediscovered.

## Do not resurrect

`sb-4` scorer change · `doc_fetch` (reps 0, F736) · `doc_examples` arm (reps 0 **until**
`probe.py --verify` confirms the rebuild — running it on the current binary sets an env var nothing
reads and scores a `doc_fetch` replicate under a new name).

## Instruments

`boundary.py` (rebuild gate — now detects ORPHANED engines) · `probe.py` (binary carries the edit) ·
`greengate.sh` (fmt + clippy + suite) · `landcheck.py` (fields actually emit) · `repaircensus.py` ·
`rootcheck.py` · `keycensus.py` · `orphanscore.py`. Every one refuses in both directions.
