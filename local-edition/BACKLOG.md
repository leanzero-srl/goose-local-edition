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

## Builds should not default to $HOME as the working dir (2026-07-12)
A UI-dispatched swarm BUILD uses the app's working dir, which defaults to $HOME. That (a) dumps generated
app dirs (~/inv, ~/csvql, …) into the user's home, and (b) makes the ENTIRE home tree "inside the working
directory", so workers (esp. integrate-verify) wander into unrelated sibling projects in home and verify the
wrong thing (observed: integrate-verify ran `cd ~/wc2 && pytest` instead of the app it was building). The
worker prompt already forbids cd/siblings, but home-as-workdir defeats it (siblings are children of the
workdir). FIX: when the swarm provider dispatches a build and the working dir is the home directory, build in
a dedicated project subdir (e.g. ~/goose-builds/<name> or a chosen/created project dir) instead of $HOME.
The app already supports `--dir <path>`; the gap is the DEFAULT for builds.

## Merge upstream (parent repo) changes — carefully, favoring OUR work (2026-07-15, requested by Mihai)
Bring the parent repo's changes into `main` and into our `local-edition` branch. HARD CONSTRAINT: merge
carefully with the interest of KEEPING WHAT WE'VE DONE over what they've done — our fork carries the whole
swarm feature set (crates/goose-cli/src/commands/swarm.rs, ui/desktop swarm panel, providers/swarm.rs, the
LeanZero branding, config tunables). Do NOT let an upstream change clobber our swarm engine/UI. Approach:
fetch upstream, review the diff for conflicts against our touched files FIRST, take upstream only where it
doesn't regress our work (deps bumps, unrelated crates, bug fixes we lack), and resolve every conflict in
OUR favor on the swarm/desktop surfaces. Gate hard after (cargo fmt/build/clippy -D warnings + cargo test
-p goose-cli; pnpm typecheck + eslint) and smoke-test a real swarm build before pushing. Never staged files
we must not touch (openai.rs, schedule.rs, pnpm-lock, openapi.json) unless upstream genuinely changed them.
