# Local Edition backlog

## DONE — Model weights in the swarm (2026-07-11)
Per-node task-share weights so a slower machine does less. Both parts shipped:
1. **Per-node weight editor** in Swarm settings (fleet card) — a −/n/+ stepper per live node, writing
   `speed_weights` (a device-id→weight map the scheduler already reads via `speed_weight_for()` /
   substring match). Higher = a bigger share of tasks; turn a slower machine down so it does less.
   No Rust change needed — the weighted scheduler + config field already existed; only the UI was missing.
2. **Recipe chat model picker** — the model chip in "Build a recipe with the fleet" is now a dropdown of
   live fleet models (was hardcoded to the first coder model). Falls back to auto-pick if the choice unloads.

## NEXT (from the rigorous overnight assessment — 2026-07-11)
The fleet builds working ENGINES but drifts on the exact SPEC CONTRACT (invented CLI names, missing
commands, inverted `=` convention, wrong error codes), and its self-tests exercise INTERNAL functions
rather than the documented CLI — so a green test suite MASKS the drift (tracker/sheet/ledger all PARTIAL;
only vcs matched its spec). Candidate improvement: have the swarm derive CLI/contract tests from the spec's
literal commands and verify against them, not just internal unit tests. (Larger swarm-quality change —
raise with the user before starting.)
