# Public requirement feedback for a second candidate

The original independently authored candidate stays frozen. A revised candidate must be labelled as authored after public requirement feedback. This brief contains only the public contract and the candidate's own source; no private seeds, fixture facts, expected outputs, scorer internals or qualification numbers are included.

In `src/jira.js`, count a returned single Jira object as one domain record. The `actualCost` reducer currently turns every absent collection into zero, so its single-object fallback cannot be reached. Maintain the installation's clock-hour background budget using the disclosed read/write costs. Public anchors: `FORGE2-CONTRACT.md` §10, lines 225–236 and 249–254; `RATE-MODEL.json`, `costs[].singleObject`.

In `src/views.js`, displayed change `points` must be the issue's current value of the estimation field recorded for that change. `personView` currently uses the immutable row `estimate` directly. Keep that stored estimate unchanged, derive current display points separately, and retain the state at close for closed sprints. Apply the same rule to the table, Rovo result and scope-status value. Public anchors: §1, lines 28–30; §5, lines 118–122; §6, lines 154–156; §9, lines 200–205; §12, lines 295–300; §15, lines 374–375.

After a board changes its estimation field, refresh totals for every issue contributing to committed, added or removed points, including issues that already left all active sprints. A search limited to current sprint membership or recent issue updates does not cover every contributor when the field switch itself sends no event. Keep earlier ledger estimates unchanged. Public anchors: §1, lines 23–30; §3, lines 64–69; §12, lines 299–306.

For a newly recorded change after a field switch, use the new field and preserve the field/board identity on the row. In `src/config.js` and `src/sync.js`, another issue change's timestamp is not evidence of when the board's estimation field changed. Check delayed and out-of-order processing around a field switch without rewriting existing rows. Public anchors: §9, lines 200–205; §12, lines 299–306.

Only Sprint or estimation-field updates warrant Jira or queue work. `discoverConfig` currently puts every numeric custom field into `estimateFields`, and the trigger treats all of them as estimation changes. Unrelated numeric fields must remain irrelevant. Public anchor: §3, lines 68–69.

When a sprint closes, clear scope-status for every previously tracked issue that no longer qualifies through another active sprint, including issues already removed before the close. The close sends no issue event, so reconciliation must cover this retired cohort. Public anchors: §12, lines 295–306; §15, lines 373–381.

Bind `Migration` to its read-only text and `Recent admin changes` to the table using the permitted label association. The adjacent headings in `src/frontend/admin.jsx` do not provide that binding. Public anchor: §13, lines 320–324.

No retry repair is requested from the observed retry implementation. Keep the awaited delays in `src/explain.js`: §16, lines 389–390 requires positive backoff, at most three attempts per virtual minute and an error flag after failure, with no Retry-After supplied by Forge LLM.
