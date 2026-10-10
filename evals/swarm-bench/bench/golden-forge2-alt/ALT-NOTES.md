# golden-forge2-alt — an independent Scope Ledger 2 built from the public material only

This is the fairness proof for forge-2.0: a second v2, written without the reference golden, so a high score here
shows the public text is enough to get there.

## What was read, and what was not

Read: the starter (`forge2/starter`, the v1 workspace) and the five public files as they stand on `harden/contract`
(FORGE2-CONTRACT.md, spec-build-forge2.md, STARTER.md, RATE-MODEL.json, BROWSER-TESTING.md). The kit's typings and
sources under `node_modules/` and `$FORGE_KIT` (manifest schema, OpenAPI) were used as API reference, as STARTER says.

To run the entrant's dev tools outside the harness I also read plumbing that sets up a workspace: `forge2/kit/README.md`
(how the kit is materialised, the control token goes in `FORGE_SITE_URL`'s password), the `FORGE_KIT`/`FORGE_SITE_URL`
lines of `run_build.py`, the browser variable names in `bench_isolation.py`, and the CLI line of `forge2_site.py`.
None of it describes grading.

Not read: `bench/golden-forge2`, the scorer, the checks, the oracle, the probe, the site code, `forge2/SPEC.md`,
`NOW.md`, `DESIGN-DRAFT.md`, `research/` (the integration plan included: it is not entrant material), the mutants and
the release manifest. One leak I could not avoid: the session opened with the git log, whose subject lines include
"S4 scorer TODO (non-UI-Kit admin page must be charged)". It changed nothing (§2 already requires UI Kit).

For the eventually consistent queries that `harden/contract` adds, I staged `harden/kit`'s dev-tool code (`bin/`,
`lib/`) next to the materialised kit and ran it, without reading it. The site on this branch has no concurrent
delivery plan, so concurrency (S1) was designed for, not exercised.

## Where the public text was unclear or silent, and what I decided

### Upgrade and migration (§9)

- **v1's working state.** v1 keeps its config under the KV key `config` and per-issue state in `sprint-issue`;
  STARTER says not to count on finding them. v2 keeps its own config under `config-v2` and its own state in
  `scope-member`. `sprint-issue` stays declared (removing an entity on upgrade felt riskier than leaving it) and is
  unused.
- **`<total>`.** v2 never writes `scope-change`, so counting it at any time gives what v1 left at the upgrade. The
  partitions counted are every sprint the scrum boards list (any state) plus v1's `config` sprints when that key is
  still there. If the admin page is opened before the first hourly run, it discovers the boards itself to count.
- **`<n>`.** Counted live from `scope-ledger` rows flagged `migrated` (a row whose change v1 recorded), so it is exact
  whichever path copied the row. The text reads `Migrated n of total v1 rows`, plus ` — complete` when n = total.
- **Who copies a v1 row.** Every writer (migration, event path, reconciliation) looks the change up in `scope-change`
  by key first and, when v1 has it, writes v1's change time, kind, issue, author and source. So "a migrated row keeps
  v1's" holds even when an event or the reconciliation reaches the change before the migration batch does.
- **A migrated row's `estimate`.** The issue's value, at copy time, of the field the sprint's board uses at the copy:
  there is no record of a board's field before the upgrade.
- **Surfaces during the migration** read the union of `scope-ledger` and the not-yet-copied `scope-change` rows until
  the plan is marked complete. In practice the first hourly run finishes the copy before any update.

### Numbers, fields and a changing world (§1, §12, §15)

- **A board's estimation field over time.** Jira keeps no history and a switch sends no event. The app records each
  board's field with the time it applies from. Every new row on the event path reads the board's configuration live
  (one GET per board per event), so a change after a switch gets the new field. A field first seen at an event takes
  effect from the top of the current hour (the last hourly look); first seen by the hourly run, from the previous one.
  So a change made in that window before the switch gets the new field if its row is only written after the switch
  is seen (a late or lost event); every change whose event arrives in order gets the field read at that moment.
- **Totals use the field the board uses now**, read live by the widget and the sprint action; every member row keeps
  the values of every estimation field any board uses or used, so a switch needs no re-read.
- **A change's points** are the current value of the row's `estimateField` (§1). The custom field's `added +<points>`
  uses the same rule, so after a switch an issue added before it shows the old field's value.
- **Closed sprints.** A close sends no event. The sprint is frozen (totals and per-change points from storage) as soon
  as anything sees it closed: the hourly run, an issue whose Sprint field reports it closed, an issue that left it with
  no changelog entry (the event path asks Jira before touching such a sprint), or the sprint action / Rovo opening
  it. Verified on the dev site by stepping the stream 10 updates at a time: the frozen numbers equal the last numbers
  before the close.
- **Deleted issues.** A tombstone first, then (after the query staleness window) every row of the issue gets
  `deleted: true` and its member rows count nothing. A lost delete event is caught by the hourly run: issues storage
  knows in an active sprint that the search no longer returns are confirmed missing with a bulkfetch as the app.
- **Rovo for a future sprint** is not specified: v1 answered with zeros and v2 keeps that.
- **Sprint action sorting.** Only `at` is specified; v1's points sort was dropped.

### Points and dosing (§10, RATE-MODEL.json)

- **Own count.** One row per virtual clock hour (`rate-budget`), updated by conditional transactions so concurrent
  invocations never lose a reservation. A search is reserved at its page size and the unreturned part handed back.
  429'd requests stay counted.
- **Retry-After holds are shared.** A burst or per-issue 429 records a hold in the hour row; every request, background
  or person-facing, waits it out ("whichever invocation sends it"). A per-issue 429 on the bulk field-value request
  holds that endpoint and resends the request.
- **"In a busy hour the others can bring the wall forward."** Read as: the wall can arrive before the app's own count
  says so. Background stops for the hour on a quota 429, and also when a response says fewer points remain than the
  share background leaves to people (X-RateLimit-Remaining under quota − background limit).
- **Person-facing requests** sit out a short Retry-After (5 s for surfaces, 30 s for Rovo); longer ones go back to the
  page, which waits and calls again, as v1 did.
- **5xx answers are not retried**: the contract says Retry-After and RateLimit-Reason decide every retry.

### Time limits (§11)

- A consumer meeting a Retry-After longer than 10 s returns an `InvocationError` with `retryAfter` (clamped to 900 s).
- The hourly run watches its time left and, below 60 s, continues in a queue invocation (`{kind: "reconcile"}`) that
  redoes the run idempotently. Tested by forcing the margin: the continuation finished the rest with no duplicate.
- A quota or share stop ends the hourly run; the next one resumes.

### Exactly once under concurrency and stale queries (§3)

- Rows are keyed by changelog id + sprint and created with `FAIL_IF_EXISTS`; the first writer keeps `source`.
- Member rows keep the fresher Jira read (a conditional write on the read time). The field value is written only by
  the freshest read of the issue, checked by key just before writing, on the event path and in the hourly bulk write.
- The event path takes the issue's changes from its own changelog (`expand=changelog` costs the same 1 point) and
  looks each row up by key, so no decision there depends on an index query that may not show the last 5 seconds.
- CI and deletions record their fact first, wait out the staleness window, then patch every row the index shows; each
  row writer re-reads both facts by key after writing.
- Surfaces read the ledger through its index, as §2 says, so a read within ~5 virtual seconds of a write can lag.

### Admin panel (§13)

- **`Migration`** is a read-only `Textfield` bound by `Label labelFor`; **`Recent admin changes`** a `DynamicTable`
  with that `caption` and column keys/headers `when`, `who`, `what`.
- **`who`** is the account id from the resolver context. **`what`** lists each changed setting as
  `<label>: old → new`; a rotation reads `Rotate CI secret`.
- **Before any rotation** the panel shows `CI secret: none yet (rotate to create one)`. After a rotation the full secret
  is in that answer only; the next interaction, render or load shows `CI secret: ••••<last4>`.
- A save given `"true"`/`"false"` for the toggle (a direct resolver call) is accepted like a boolean.

### CI web trigger (§14)

- Header values are arrays; the first value is used. A body that fails to parse, or names another environment, after a
  valid signature and timestamp, is answered by a static `400` output (not specified); an unexpected failure by `500`.
- The eventId is claimed with `FAIL_IF_EXISTS` before the deployment is applied. Environments are kept in arrival order.

### Forge LLM (§5, §16)

- **One request per click.** forge-dev's `llm` log counts `list()` as a call, so the hourly run asks `list()` and
  remembers an active model; a click sends only the chat (it lists itself only when no recent answer is stored).
- **429.** At most 3 attempts per click, 3 s then 6 s apart, all inside the resolver's 25 s; then the error flag. I did
  not count attempts across clicks: "leaves the modal working" suggests the next click must still be able to succeed.
- **Token budget** counts `usage.total_tokens` of every answer that reports it (failed ones too), per UTC day of the
  virtual clock, and is checked before anything else, cache hits included.
- **Cache** key: the sprint plus the exact content sent (what this viewer may see), so viewers who see the same changes
  share an answer and nobody gets one built from changes they cannot see.

### Custom UI (§7, §17)

- React and the bridge together exceed 150 KB, so the widget and the sprint action are plain DOM code (bridge ≈ 95 KB,
  totals ≈ 102 KB and ≈ 107 KB of JS and CSS). The widget's edit surface is not budgeted and keeps v1's React.

### Other small calls

- The comment group check uses `GET /rest/api/3/user/groups` as the app (needs `read:jira-user`) and compares group
  names case-insensitively. The field-value API's scope is the granular `write:app-data:jira` (its classic list is
  empty).
- A double click is stopped twice: the page ignores a click while a post is in flight, and the server holds an
  in-flight lock per viewer, sprint and change (a lock older than 30 s counts as abandoned).
- An empty field value is written as `null`.

## How it was checked (dev site, forge-dev, the bundled browser)

- `npm run lint`: no issues. Every function loads; `forge-dev uikit scope-admin` renders the panel as the admin and
  refuses everyone else, with direct resolver calls as a non-admin changing nothing.
- v1 differential: v1 and this v2 run side by side over the hourly run and 60 updates: every widget and every sprint
  ledger equal.
- An independent check (a throwaway function, not shipped) that replays sprint membership from the changelog: after
  the whole dev stream (a sprint close, a field switch, a deletion, lost events) all five active sprints' numbers equal
  it, and every issue's scope-status equals it except the board that switched fields, where the check used the new
  field for changes made before the switch and the app (correctly, per §1) the old one.
- Under harden/kit's stale queries: diffing two hourly runs found a real bug (11 field values left empty), fixed; the
  next hourly runs corrected only what their own reasons explain (a lost event, a closed sprint).
- CI: valid 202, replay 200, bad signature / unsigned / tampered / stale 401, header case accepted. Forge LLM: clean,
  digits, refusal, malformed, error, unfinished, injected and rate-limited scripts; cache; kill switch; token budget.
- Browser at 380 px: no horizontal scroll, no clipped number, bars proportional, light and dark backgrounds painted,
  one invoke before the first data paint, no other origin, no console error; live updates through Realtime.
- Share at 10 %: the hour's own count stopped at 232 of 240 points.

## Known limits

- Concurrent delivery (S1) was reasoned about, not run: no dev tool on this branch delivers two related events at once.
- A change whose event was lost between a board's field switch and the app's next look at that board can get the
  old or the new field (see above).
- The hourly run reads every candidate issue (one search, one changelog bulkfetch) rather than only what changed:
  simpler, about 30 points an hour on the dev site.
