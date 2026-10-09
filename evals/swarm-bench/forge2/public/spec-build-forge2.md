# forge-2.0 — Scope Ledger 2 (Atlassian Forge)

Ship v2 of the Scope Ledger Forge app in this workspace. The workspace holds v1: a working app, installed on a large
Jira Cloud site together with its data. `FORGE2-CONTRACT.md` is the contract and `RATE-MODEL.json` the rate model it
binds. `STARTER.md` describes the workspace (v1's code and data layout), the installed packages, the reference
material and the offline dev tools (Forge runtime, dev Jira site, UI Kit host, CI sender, linter). There is no internet
and nothing to install. No reference v2 is provided.

You have a budget of 300 model calls. The harness stops the session at the budget and scores whatever exists; plan to
implement, test, and finish well inside it — polishing beyond the scored behaviours earns nothing.

## Definition of done

Done means each of these works when the harness upgrades the site's v1 to your app and runs it for 6 virtual hours on
its own seeded site; nothing else is scored:

1. `npm run lint` reports no errors and no warnings; every manifest function bundles and loads, and so does the admin
   page.
2. v1 keeps working throughout: backfill and reconciliation, exactly one ledger row per change, the widget, the sprint
   action, Rovo, privacy and the Custom UI policy (§1–§8, §18).
3. Every v1 row moves to `scope-ledger` exactly once, while events flow and across invocation limits, within 2 virtual
   hours, with its progress in the admin panel (§9).
4. Background work stays inside its share of the 2,400-point hour, no person-facing request meets a quota 429, and
   Retry-After and `RateLimit-Reason` decide every retry (§10, `RATE-MODEL.json`).
5. Work longer than one invocation continues in the next, no function waits past its limit, and a killed invocation
   leaves no duplicate (§11).
6. Closed sprints, moves to another board's sprint, estimation-field switches, deleted issues and lost permissions
   come out right (§12).
7. The UI Kit admin panel works, and every admin resolver checks `ADMINISTER` on the server (§13).
8. The CI web trigger verifies signature, timestamp and replay, and records deployments (§14).
9. The `scope-status` field holds the right value for every issue, fresh each virtual hour (§15).
10. Forge LLM: validated tool calls, 429 back-off, incomplete answers refused, the cache, the kill switch and the token
    budget (§16).
11. The widget and the sprint action boot within budget, and the admin page with one invoke (§17).

## Score

Tests earn the score. v1's behaviours (§1–§8) are a quarter of it; the v2 requirements share the rest: migration
0.12, dosing 0.13, time limits 0.08, the changing world 0.10, the admin panel 0.10, the CI web trigger 0.08, the custom
field 0.05, Forge LLM 0.04, the boot budget 0.05. Lint warnings cost points but never cap.

Conditions also cap it:

- Lint errors, or manifest functions that do not bundle and load: maximum 0.499.
- None of the v2 surfaces exists (no `jira:adminPage`, no `webtrigger` and no `scope-ledger` entity): maximum 0.30.

Each of these defects, when observed, multiplies the score by 0.6, once per root cause:

- v1 rows lost or corrupted by the migration;
- a web-trigger write without a valid signature;
- an admin action by a non-admin that succeeded, or the CI secret disclosed;
- a hidden issue's data shown to a person who cannot browse it;
- a duplicate side effect: two or more comments for one click, or duplicate ledger rows.

Time is virtual: nothing is graded on wall-clock time. The harness keeps screenshots of every Custom UI surface and the
rendered tree of the admin page.

## Completion handoff

Work only with this workspace, its packages, the reference material and the dev tools. When the definition of done
holds, stop your temporary processes and return a final summary of files, checks and remaining limitations; that
response hands the app to the external scorer, which starts after your session ends. Do not wait for or poll grader
results.
