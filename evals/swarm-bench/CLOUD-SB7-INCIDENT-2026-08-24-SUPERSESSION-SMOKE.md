# Cloud SB7 supersession smoke baseline incident

Date: 2026-08-24

## Failure

The generation-1 successor at
`/Users/mihaiperdum/goose-builds/cloud-sb7-20260823-live-r3` stopped before its
first smoke provider admission. The smoke coordinator incorrectly required all
full-build states to be pristine generation-0 states. A valid supersession
instead starts affected and unstarted entrants at their sealed predecessor
episode counts and preserves successful carried entrants in their terminal
state. The old predicate therefore rejected all five entrants before launching
any smoke call.

## Correction

Smoke readiness is now lineage-aware. Generation 0 requires the original zero
baseline, generation 1 requires only affected and unstarted entrants to remain
at the predecessor attempt baseline, and generation 2 retains its recovery
baseline. Carried generation-1 outcomes remain protected by the existing
lineage seal and by the smoke raw-tree before/after comparison.

A reusable pre-smoke instrument-repair transition handles this exact class of
failure without erasing the successor or rerunning a carried outcome. It is
available only for an `INITIALIZED` generation-1 successor in `ATTENTION`, with
zero smoke launches, zero smoke admissions, no successor full-build activity,
no live manager/monitor process or process group, free vendor ports, and exactly
one changed instrument: the coordinator. The transition preserves source
campaign, lineage, coordinator, build-state, smoke-state, and smoke-manager
hashes in an immutable bundle; updates the frozen coordinator and smoke
contract atomically; and validates both lineage and frozen-instrument identity
before returning. Repeating the command is idempotent.

The complete 211-test cloud harness suite and the six terminal-recovery tests
pass with regressions covering the supersession baseline and preservation of a
carried result across the repair.
