# forge-1.0 — Scope Ledger (Atlassian Forge)

Build the Scope Ledger Forge app in this workspace: a working Jira app, not a tutorial sample.
`FORGE-CONTRACT.md` is the contract. `STARTER.md` describes the workspace, the installed
packages, the reference material and the offline dev tools (Forge runtime, dev Jira site,
linter). There is no internet and nothing to install. No reference application is provided.

You have a budget of 150 model calls. The harness stops the session at the budget and scores
whatever exists; plan to implement, test, and finish well inside it — polishing beyond the
scored behaviours earns nothing.

## Definition of done

Done means each of these works when the harness runs the app on its own seeded site; nothing
else is scored:

1. `npm run lint` reports no errors and no warnings; every manifest function bundles and loads.
2. The current modules of §2, with scopes your calls need and nothing more.
3. The first scheduled run backfills every change of every active sprint, through paginated
   search and rate limits (§1, §3).
4. Issue updates flow trigger → queue → consumer into the KVS entity: exactly one row per change
   under duplicates, reordering and loss; estimate changes move the numbers (§3).
5. Later scheduled runs heal what the event stream missed and change nothing otherwise (§3).
6. The widget: board choice through the dashboards edit API, correct numbers, chart (§4).
7. The sprint action: ledger table, sorting, router links, hidden count, one ADF comment per
   click as the viewer, flags, close (§5).
8. Nobody sees changes to issues they cannot browse (§1, §5, §6).
9. The Rovo action answers exactly; the skill and agent are wired and valid (§6).
10. Every surface works in light and dark, inside the Custom UI security policy, with a clean
    console (§7).

## Score bands

Tests earn the score. Conditions also cap it:

- Lint errors, or manifest functions that do not bundle and load: maximum 0.499.
- No working ledger (widget shows no data, backfill or event rows wrong, KVS entity unused):
  maximum 0.699.
- Missing current-platform surfaces (dashboards widget and its edit API, Rovo skill, sprint
  action table, Rovo action) or a surface broken in light or dark: maximum 0.799.
- Any duplicate, ordering, rate-limit, pagination, permission, policy or console defect:
  maximum 0.899.

Lint warnings cost points but never cap. A small excellence share rewards reaching Jira in few
calls and rendering each surface in few round trips. The harness keeps screenshots of every surface.

## Completion handoff

Work only with this workspace, its packages, the reference material and the dev tools. When the
definition of done holds, stop your temporary processes and return a final summary of files,
checks and remaining limitations; that response hands the app to the external scorer, which
starts after your session ends. Do not wait for or poll grader results.
