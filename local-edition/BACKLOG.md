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

## Mihai asks 2026-07-18 (evening) — NEXT-DMG UI/UX + loop evolution
1. **AI-named sessions** (next DMG). Each chat/session should be named by AI from the prompt / what it builds, not the raw truncated first prompt ("Build X — a"). MACHINERY EXISTS: crates/goose/src/session/session_naming.rs (generate_description -> model call -> extract_short_title). It is NOT wired into the swarm-build session path. Wire it so a swarm build names its session (e.g. "logfold — Go log-template miner"). Confidence: high (mostly wiring an existing fn).
2. **Chat-list cap** (next DMG). NOT a deletion bug — sessions are all safe in ~/.local/share/goose/sessions.db (366MB). The sidebar hard-caps the VISIBLE list at MAX_RECENT_SESSIONS=25 (ui/desktop/src/hooks/useNavigationSessions.ts:13, .slice(0,25)). Raise the cap and/or add scroll + a "see all sessions" view so nothing drops off visibly. SEPARATE concern: the 366MB DB is bloating — investigate session-data growth. Confidence: high.
3. **Expandable/clickable activity log** (next DMG). The swarm run activity feed (SwarmRunPanel activity list) reads as an undifferentiated stream. Make each line CLICKABLE -> expand to full detail (the full brief, the full research finding, the full judge verdict, the exact files, the tool-call/turn detail). Mihai: "paramount that I can expand and see what was done ... right now it just feels like an iteration of stuff." Extends #101 (layering) + #102 (liveness). Confidence: med (real UX design work).

## STANDING loop-evolution asks (Mihai 2026-07-18) — make PART OF THE LOOP
4. **Inspiration workflows in the loop.** Constantly (every N ticks) run READ-ONLY dynamic workflows that scan online for how OTHER coding agents / agent frameworks work — read their GitHubs, papers, blogs — and mine concrete, adoptable ideas for goose's TOOL USAGE, EFFICIENCY, and overall QUALITY. Output ranked ideas + sources into SOLUTIONS.md/an INSPIRATION.md. Never open a browser, never leave a server.
5. **Meta-evolution ("knob-turning skill").** The loop itself must constantly evolve + iterate on what it needs to do, and improve the SKILL of improving goose (the formula for turning goose's knobs well). Periodically step back, review what's working in the campaign, and refine the loop's own strategy — not just execute a fixed checklist.
